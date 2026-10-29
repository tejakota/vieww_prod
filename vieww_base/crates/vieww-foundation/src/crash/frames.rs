//! Raw stack frames a stripped binary can still be symbolised from (FW-12).
//!
//! `std::backtrace` resolves names in-process. A shipping build
//! (`--profile dist`) is stripped, so on a phone that resolution yields a
//! column of `<unknown>` with no addresses, and nothing can be recovered
//! afterwards. This records, for every frame, what an offline symboliser
//! needs:
//!
//! ```text
//! #3 0x00007f3a1c2b4f10 libmain.so+0x2b4f10 build-id=5f1c…
//! ```
//!
//! * the instruction address;
//! * the module it falls in, and the offset into it: the address minus the
//!   module's load bias, which is the address the debug file knows;
//! * the module's **build id** (the ELF `NT_GNU_BUILD_ID` note, the Mach-O
//!   `LC_UUID`, the PE CodeView GUID and age), read from the image already
//!   in memory, so the report names the exact build whose debug file
//!   decodes it.
//!
//! `ci/release/symbols.sh` keeps those debug files per build id, and
//! `ci/release/symbolize.py` turns a report's frames back into functions
//! and lines.
//!
//! Walking frames: `_Unwind_Backtrace` (Linux, Android, Apple) or
//! `RtlCaptureStackBackTrace` (Windows). Modules: `dladdr` or
//! `GetModuleHandleExW`. None of it allocates beyond the returned strings,
//! and none of it can panic on a malformed image: every read is bounded by
//! the header it came from. Elsewhere (wasm, 32-bit Arm, where
//! `_Unwind_GetIP` is a macro) the list is empty.

/// The calling thread's frames, outermost last, each as one line.
#[must_use]
pub fn raw_frames() -> Vec<String> {
    imp::frames()
        .into_iter()
        .enumerate()
        .map(|(i, f)| f.line(i))
        .collect()
}

/// One frame, resolved to its module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) ip: usize,
    pub(crate) module: Option<Module>,
}

/// The module a frame falls in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Module {
    pub(crate) name: String,
    pub(crate) offset: usize,
    pub(crate) build_id: Option<String>,
}

impl Frame {
    fn line(&self, i: usize) -> String {
        match &self.module {
            Some(m) => {
                let file = m.name.rsplit(['/', '\\']).next().unwrap_or(&m.name);
                let id = m.build_id.as_deref().unwrap_or("unknown");
                format!(
                    "#{i} {:#018x} {file}+{:#x} build-id={id}",
                    self.ip, m.offset
                )
            }
            None => format!("#{i} {:#018x} ?", self.ip),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Bounded little-endian reads from a byte image.
fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// An ELF64 image's load bias and build id, given the bytes from its base:
/// the program headers and, at their virtual addresses, the notes.
/// `read(vaddr, len)` returns the image's bytes at a virtual address.
#[allow(
    dead_code,
    reason = "each platform reads one image format; all three are tested everywhere"
)]
pub(crate) fn elf_info(
    header: &[u8],
    read: &dyn Fn(u64, usize) -> Option<Vec<u8>>,
) -> Option<(u64, Option<String>)> {
    if header.get(..4)? != b"\x7fELF" || *header.get(4)? != 2 {
        return None; // not ELF64
    }
    let phoff = usize::try_from(u64_at(header, 32)?).ok()?;
    let phentsize = usize::from(u16_at(header, 54)?);
    let phnum = usize::from(u16_at(header, 56)?);
    if phentsize < 56 || phnum > 256 {
        return None;
    }
    let phdrs = read(phoff as u64, phentsize * phnum)?;
    let mut min_load = u64::MAX;
    let mut notes = Vec::new();
    for i in 0..phnum {
        let p = &phdrs.get(i * phentsize..(i + 1) * phentsize)?;
        let kind = u32_at(p, 0)?;
        let vaddr = u64_at(p, 16)?;
        let filesz = u64_at(p, 32)?;
        match kind {
            1 => min_load = min_load.min(vaddr), // PT_LOAD
            4 => notes.push((vaddr, filesz)),    // PT_NOTE
            _ => {}
        }
    }
    let mut id = None;
    for (vaddr, size) in notes {
        let size = usize::try_from(size.min(4096)).ok()?;
        let Some(n) = read(vaddr, size) else { continue };
        let mut at = 0;
        while at + 12 <= n.len() {
            let namesz = u32_at(&n, at)? as usize;
            let descsz = u32_at(&n, at + 4)? as usize;
            let kind = u32_at(&n, at + 8)?;
            let name_at = at + 12;
            let desc_at = name_at + namesz.div_ceil(4) * 4;
            let end = desc_at + descsz.div_ceil(4) * 4;
            if end > n.len() {
                break;
            }
            if kind == 3 && n.get(name_at..name_at + namesz) == Some(b"GNU\0") {
                id = Some(hex(&n[desc_at..desc_at + descsz]));
                break;
            }
            at = end;
        }
        if id.is_some() {
            break;
        }
    }
    Some((if min_load == u64::MAX { 0 } else { min_load }, id))
}

/// A Mach-O 64 image's UUID (`LC_UUID`), from the bytes at its base.
#[allow(
    dead_code,
    reason = "each platform reads one image format; all three are tested everywhere"
)]
pub(crate) fn macho_uuid(header: &[u8]) -> Option<String> {
    if u32_at(header, 0)? != 0xfeed_facf {
        return None;
    }
    let ncmds = u32_at(header, 16)? as usize;
    let mut at = 32;
    for _ in 0..ncmds.min(512) {
        let cmd = u32_at(header, at)?;
        let size = u32_at(header, at + 4)? as usize;
        if cmd == 0x1b {
            return Some(hex(header.get(at + 8..at + 24)?));
        }
        if size < 8 {
            return None;
        }
        at += size;
    }
    None
}

/// A PE image's CodeView id (`RSDS` GUID and age, as symbol servers name a
/// PDB), from the image mapped at its base.
#[allow(
    dead_code,
    reason = "each platform reads one image format; all three are tested everywhere"
)]
pub(crate) fn pe_codeview(image: &[u8]) -> Option<String> {
    let pe = u32_at(image, 0x3c)? as usize;
    if image.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let opt = pe + 24;
    let dirs = match u16_at(image, opt)? {
        0x20b => opt + 112, // PE32+
        0x10b => opt + 96,  // PE32
        _ => return None,
    };
    let debug_rva = u32_at(image, dirs + 6 * 8)? as usize;
    let debug_size = u32_at(image, dirs + 6 * 8 + 4)? as usize;
    for i in 0..(debug_size / 28).min(16) {
        let d = debug_rva + i * 28;
        if u32_at(image, d + 12)? != 2 {
            continue; // not IMAGE_DEBUG_TYPE_CODEVIEW
        }
        let raw = u32_at(image, d + 20)? as usize;
        if image.get(raw..raw + 4)? != b"RSDS" {
            continue;
        }
        let g = image.get(raw + 4..raw + 20)?;
        let age = u32_at(image, raw + 20)?;
        // The symbol-server spelling: Data1-3 big-endian, then the rest.
        let mut guid = Vec::with_capacity(16);
        guid.extend(g[0..4].iter().rev());
        guid.extend(g[4..6].iter().rev());
        guid.extend(g[6..8].iter().rev());
        guid.extend(&g[8..16]);
        return Some(format!("{}{age:x}", hex(&guid)));
    }
    None
}

#[cfg(all(
    any(target_os = "linux", target_os = "android", target_vendor = "apple"),
    not(target_arch = "arm"),
    target_pointer_width = "64"
))]
mod imp {
    use super::{Frame, Module};
    use std::ffi::{c_char, c_void, CStr};

    #[repr(C)]
    struct DlInfo {
        fname: *const c_char,
        fbase: *mut c_void,
        sname: *const c_char,
        saddr: *mut c_void,
    }

    type Trace = extern "C" fn(*mut c_void, *mut c_void) -> i32;

    extern "C" {
        fn _Unwind_Backtrace(trace: Trace, arg: *mut c_void) -> i32;
        fn _Unwind_GetIP(ctx: *mut c_void) -> usize;
    }

    #[cfg_attr(any(target_os = "linux", target_os = "android"), link(name = "dl"))]
    extern "C" {
        fn dladdr(addr: *const c_void, info: *mut DlInfo) -> i32;
    }

    const MAX: usize = 128;

    extern "C" fn trace(ctx: *mut c_void, arg: *mut c_void) -> i32 {
        // SAFETY: `arg` is the Vec below; `ctx` is the unwinder's.
        let ips = unsafe { &mut *arg.cast::<Vec<usize>>() };
        let ip = unsafe { _Unwind_GetIP(ctx) };
        if ip != 0 {
            ips.push(ip);
        }
        if ips.len() >= MAX {
            5 // _URC_END_OF_STACK
        } else {
            0 // _URC_NO_REASON
        }
    }

    pub(super) fn frames() -> Vec<Frame> {
        let mut ips: Vec<usize> = Vec::with_capacity(MAX);
        // SAFETY: a valid callback and argument for the walk's duration.
        unsafe { _Unwind_Backtrace(trace, (&raw mut ips).cast()) };
        ips.into_iter()
            .map(|ip| Frame {
                ip,
                module: module(ip),
            })
            .collect()
    }

    /// Bytes of the mapped image at `base + at`. The image is mapped (the
    /// frame's code is in it), and every caller bounds `len` by headers
    /// read from that same image.
    fn bytes(base: usize, at: u64, len: usize) -> Option<Vec<u8>> {
        let start = base.checked_add(usize::try_from(at).ok()?)?;
        // SAFETY: inside the mapped image, per the headers.
        Some(unsafe { std::slice::from_raw_parts(start as *const u8, len) }.to_vec())
    }

    fn module(ip: usize) -> Option<Module> {
        let mut info = DlInfo {
            fname: std::ptr::null(),
            fbase: std::ptr::null_mut(),
            sname: std::ptr::null(),
            saddr: std::ptr::null_mut(),
        };
        // SAFETY: a valid out-pointer; `ip - 1` points into the call.
        if unsafe { dladdr((ip - 1) as *const c_void, &raw mut info) } == 0 || info.fbase.is_null()
        {
            return None;
        }
        let base = info.fbase as usize;
        let name = if info.fname.is_null() {
            String::from("?")
        } else {
            // SAFETY: dladdr's path is NUL-terminated.
            unsafe { CStr::from_ptr(info.fname) }
                .to_string_lossy()
                .into_owned()
        };
        #[cfg(target_vendor = "apple")]
        {
            let header = bytes(base, 0, 32 + 4096)?;
            Some(Module {
                name,
                offset: ip - base,
                build_id: super::macho_uuid(&header),
            })
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let header = bytes(base, 0, 64)?;
            let read = |vaddr: u64, len: usize| bytes(base, vaddr, len);
            // The ELF header is at the image's base, so `read` takes file
            // offsets for the program headers; the first PT_LOAD of a
            // shared object or PIE has offset 0 and vaddr 0, so for those
            // the two agree.
            let (min_load, build_id) = super::elf_info(&header, &read).unwrap_or((0, None));
            let bias = base.wrapping_sub(usize::try_from(min_load).unwrap_or(0));
            Some(Module {
                name,
                offset: ip.wrapping_sub(bias),
                build_id,
            })
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::{Frame, Module};
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn RtlCaptureStackBackTrace(
            skip: u32,
            count: u32,
            frames: *mut *mut c_void,
            hash: *mut u32,
        ) -> u16;
        fn GetModuleHandleExW(flags: u32, addr: *const c_void, module: *mut *mut c_void) -> i32;
        fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
    }

    const FROM_ADDRESS: u32 = 0x4;
    const UNCHANGED_REFCOUNT: u32 = 0x2;
    const MAX: usize = 62; // RtlCaptureStackBackTrace's documented limit

    pub(super) fn frames() -> Vec<Frame> {
        let mut raw = [std::ptr::null_mut::<c_void>(); MAX];
        // SAFETY: a buffer of MAX pointers.
        let n = unsafe {
            RtlCaptureStackBackTrace(0, MAX as u32, raw.as_mut_ptr(), std::ptr::null_mut())
        };
        raw[..usize::from(n)]
            .iter()
            .map(|&p| Frame {
                ip: p as usize,
                module: module(p as usize),
            })
            .collect()
    }

    fn module(ip: usize) -> Option<Module> {
        let mut h = std::ptr::null_mut();
        // SAFETY: valid out-pointer; the address is code in a loaded module.
        if unsafe {
            GetModuleHandleExW(
                FROM_ADDRESS | UNCHANGED_REFCOUNT,
                ip as *const c_void,
                &raw mut h,
            )
        } == 0
        {
            return None;
        }
        let mut buf = [0u16; 520];
        // SAFETY: a buffer of its stated size.
        let len = unsafe { GetModuleFileNameW(h, buf.as_mut_ptr(), buf.len() as u32) } as usize;
        let name = String::from_utf16_lossy(&buf[..len.min(buf.len())]);
        let base = h as usize;
        // SAFETY: the module's headers are mapped; the read is bounded by
        // `SizeOfImage` from those headers.
        let image = unsafe {
            let header = std::slice::from_raw_parts(base as *const u8, 0x400);
            let pe = super::u32_at(header, 0x3c).unwrap_or(0) as usize;
            let size = super::u32_at(header, pe + 24 + 56).unwrap_or(0x400) as usize;
            std::slice::from_raw_parts(base as *const u8, size.max(0x400))
        };
        Some(Module {
            name,
            offset: ip - base,
            build_id: super::pe_codeview(image),
        })
    }
}

#[cfg(not(any(
    windows,
    all(
        any(target_os = "linux", target_os = "android", target_vendor = "apple"),
        not(target_arch = "arm"),
        target_pointer_width = "64"
    )
)))]
mod imp {
    pub(super) fn frames() -> Vec<super::Frame> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_captured_with_their_module() {
        let frames = raw_frames();
        if cfg!(any(
            windows,
            all(
                any(
                    target_os = "linux",
                    target_os = "android",
                    target_vendor = "apple"
                ),
                target_pointer_width = "64"
            )
        )) {
            assert!(frames.len() > 2, "{frames:?}");
            // The test binary's own frames name it, with an offset.
            assert!(
                frames
                    .iter()
                    .any(|f| f.contains('+') && f.contains("build-id=")),
                "{frames:?}"
            );
        }
    }

    #[test]
    fn a_build_id_note_is_found_and_malformed_images_are_refused() {
        // An ELF64 header with one PT_NOTE at vaddr 0x100 holding a GNU
        // build-id note.
        let mut img = vec![0u8; 0x200];
        img[..4].copy_from_slice(b"\x7fELF");
        img[4] = 2;
        img[32..40].copy_from_slice(&64u64.to_le_bytes()); // phoff
        img[54..56].copy_from_slice(&56u16.to_le_bytes()); // phentsize
        img[56..58].copy_from_slice(&1u16.to_le_bytes()); // phnum
        img[64..68].copy_from_slice(&4u32.to_le_bytes()); // PT_NOTE
        img[64 + 16..64 + 24].copy_from_slice(&0x100u64.to_le_bytes());
        img[64 + 32..64 + 40].copy_from_slice(&24u64.to_le_bytes());
        let note = [4u32, 8, 3];
        for (i, v) in note.iter().enumerate() {
            img[0x100 + i * 4..0x104 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        img[0x10c..0x110].copy_from_slice(b"GNU\0");
        img[0x110..0x118].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef, 1, 2, 3, 4]);
        let read =
            |at: u64, len: usize| img.get(at as usize..at as usize + len).map(<[u8]>::to_vec);
        let (_, id) = elf_info(&img[..64], &read).unwrap();
        assert_eq!(id.as_deref(), Some("deadbeef01020304"));
        // Truncated, hostile counts, wrong class: refused, no panic.
        assert!(elf_info(b"\x7fELF", &read).is_none());
        let mut bad = img.clone();
        bad[56..58].copy_from_slice(&u16::MAX.to_le_bytes());
        let read_bad =
            |at: u64, len: usize| bad.get(at as usize..at as usize + len).map(<[u8]>::to_vec);
        assert!(elf_info(&bad[..64], &read_bad).is_none());
        assert!(macho_uuid(b"nope").is_none());
        assert!(pe_codeview(&[0u8; 64]).is_none());
    }
}
