//! Compiled D3D12 shaders kept on disk between launches.
//!
//! FXC is slow: the window's three-line present pass took 285 ms to compile
//! on the reference laptop (run 45148678530), and the scene renderer's
//! WGSL → HLSL → DXBC pipelines are far larger. The DXBC a given HLSL source,
//! entry point and target compile to never changes, so it is kept in a
//! content-addressed file — `d3d12-<hash>.dxbc` in the folder named by
//! [`set_shader_cache_dir`] — and only the first launch pays for FXC.
//!
//! The key hashes the HLSL text, the entry point, the target and the
//! compile flags (FNV-1a, 64-bit: stable across Rust versions, unlike
//! `DefaultHasher`). A file whose bytes do not start with the `DXBC` magic
//! is ignored and rewritten. Writes go through a temporary file and a
//! rename, so a crash mid-write never leaves a torn blob behind.
//! `VIEWW_SHADER_CACHE=off` disables the cache.

use std::path::PathBuf;
use std::sync::Mutex;

use windows::Win32::Graphics::Direct3D::Fxc::D3DCreateBlob;
use windows::Win32::Graphics::Direct3D::ID3DBlob;

static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Keep compiled shaders in `dir` (created on first write). `None` turns
/// the cache off. The platform layer points it beside the pipeline cache.
pub fn set_shader_cache_dir(dir: Option<PathBuf>) {
    if let Ok(mut slot) = DIR.lock() {
        *slot = dir;
    }
}

fn dir() -> Option<PathBuf> {
    if std::env::var("VIEWW_SHADER_CACHE").is_ok_and(|v| v == "off") {
        return None;
    }
    DIR.lock().ok().and_then(|slot| slot.clone())
}

fn key(hlsl: &str, entry: &str, target: &[u8], flags: u32) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        // A separator, so ("ab","c") and ("a","bc") differ.
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    };
    feed(hlsl.as_bytes());
    feed(entry.as_bytes());
    feed(target);
    feed(&flags.to_le_bytes());
    hash
}

fn path(hlsl: &str, entry: &str, target: &[u8], flags: u32) -> Option<PathBuf> {
    dir().map(|d| {
        d.join(format!(
            "d3d12-{:016x}.dxbc",
            key(hlsl, entry, target, flags)
        ))
    })
}

/// The cached DXBC for this compile, if there is a valid one.
pub(crate) fn load(hlsl: &str, entry: &str, target: &[u8], flags: u32) -> Option<ID3DBlob> {
    let bytes = std::fs::read(path(hlsl, entry, target, flags)?).ok()?;
    if bytes.len() < 4 || &bytes[..4] != b"DXBC" {
        return None;
    }
    // SAFETY: a fresh blob of exactly `bytes.len()` bytes, filled once.
    unsafe {
        let blob = D3DCreateBlob(bytes.len()).ok()?;
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            blob.GetBufferPointer().cast::<u8>(),
            bytes.len(),
        );
        Some(blob)
    }
}

/// Keep `blob` for the next launch. Failures are silent: a cache that
/// cannot be written only costs the next launch a compile.
pub(crate) fn store(hlsl: &str, entry: &str, target: &[u8], flags: u32, blob: &ID3DBlob) {
    let Some(file) = path(hlsl, entry, target, flags) else {
        return;
    };
    // SAFETY: a blob's buffer is `GetBufferSize` bytes for its lifetime.
    let bytes = unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize())
    };
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let temp = file.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&temp, bytes).is_ok() && std::fs::rename(&temp, &file).is_err() {
        let _ = std::fs::remove_file(&temp);
    }
}

#[cfg(test)]
mod tests {
    use super::key;

    #[test]
    fn the_key_separates_every_input() {
        let base = key("float4 x;", "main", b"ps_5_1\0", 1);
        assert_ne!(base, key("float4 y;", "main", b"ps_5_1\0", 1));
        assert_ne!(base, key("float4 x;", "other", b"ps_5_1\0", 1));
        assert_ne!(base, key("float4 x;", "main", b"vs_5_1\0", 1));
        assert_ne!(base, key("float4 x;", "main", b"ps_5_1\0", 2));
        assert_ne!(key("ab", "c", b"", 0), key("a", "bc", b"", 0));
        assert_eq!(base, key("float4 x;", "main", b"ps_5_1\0", 1));
    }
}
