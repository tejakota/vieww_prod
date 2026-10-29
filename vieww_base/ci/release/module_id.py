"""The id and load address a crash frame names a binary by (FW-12).

    python3 module_id.py BINARY

Prints one line of JSON: {"format", "id", "base"}. `id` is spelled as
`vieww_foundation::crash::raw_frames` prints it: the ELF build-id note in
hex, the Mach-O LC_UUID in hex, or the PE CodeView GUID (symbol-server byte
order) followed by the age in hex. `base` is the address the image's first
byte has in the file's own address space (0 for a PIE or shared object, the
__TEXT vmaddr for Mach-O, ImageBase for PE): a frame's `module+offset` is
looked up at base + offset. Standard library only.
"""

import json
import struct
import sys


def elf(b):
    if b[4] != 2:
        raise ValueError("32-bit ELF")
    phoff, = struct.unpack_from("<Q", b, 0x20)
    phentsize, phnum = struct.unpack_from("<HH", b, 0x36)
    shoff, = struct.unpack_from("<Q", b, 0x28)
    shentsize, shnum = struct.unpack_from("<HH", b, 0x3A)
    base = None
    notes = []
    for i in range(phnum):
        p = phoff + i * phentsize
        ptype, = struct.unpack_from("<I", b, p)
        off, vaddr = struct.unpack_from("<QQ", b, p + 8)
        filesz, = struct.unpack_from("<Q", b, p + 32)
        if ptype == 1 and base is None:
            base = vaddr - off
        if ptype == 4:
            notes.append((off, filesz))
    # A stripped binary keeps PT_NOTE; a .debug file may only have sections.
    for i in range(shnum):
        s = shoff + i * shentsize
        stype, = struct.unpack_from("<I", b, s + 4)
        off, size = struct.unpack_from("<QQ", b, s + 24)
        if stype == 7:
            notes.append((off, size))
    for off, size in notes:
        at = off
        while at + 12 <= off + size:
            namesz, descsz, ntype = struct.unpack_from("<III", b, at)
            name_at = at + 12
            desc_at = name_at + ((namesz + 3) & ~3)
            if ntype == 3 and b[name_at:name_at + namesz].rstrip(b"\0") == b"GNU":
                return "elf", b[desc_at:desc_at + descsz].hex(), base or 0
            at = desc_at + ((descsz + 3) & ~3)
    return "elf", None, base or 0


def macho(b):
    magic, = struct.unpack_from("<I", b, 0)
    if magic != 0xFEEDFACF:
        raise ValueError("not a little-endian 64-bit Mach-O (thin it with lipo first)")
    ncmds, = struct.unpack_from("<I", b, 16)
    at = 32
    uuid = None
    base = 0
    for _ in range(ncmds):
        cmd, size = struct.unpack_from("<II", b, at)
        if cmd == 0x1B:
            uuid = b[at + 8:at + 24].hex()
        if cmd == 0x19 and b[at + 8:at + 24].rstrip(b"\0") == b"__TEXT":
            base, = struct.unpack_from("<Q", b, at + 24)
        at += size
    return "macho", uuid, base


def pe(b):
    pe_at, = struct.unpack_from("<I", b, 0x3C)
    if b[pe_at:pe_at + 4] != b"PE\0\0":
        raise ValueError("not PE")
    coff = pe_at + 4
    nsect, = struct.unpack_from("<H", b, coff + 2)
    optsize, = struct.unpack_from("<H", b, coff + 16)
    opt = coff + 20
    magic, = struct.unpack_from("<H", b, opt)
    if magic == 0x20B:
        base, = struct.unpack_from("<Q", b, opt + 24)
        dirs = opt + 112
    else:
        base, = struct.unpack_from("<I", b, opt + 28)
        dirs = opt + 96
    debug_rva, debug_size = struct.unpack_from("<II", b, dirs + 6 * 8)
    sects = opt + optsize

    def file_off(rva):
        for i in range(nsect):
            s = sects + i * 40
            vsize, vaddr, rawsize, rawptr = struct.unpack_from("<IIII", b, s + 8)
            if vaddr <= rva < vaddr + max(vsize, rawsize):
                return rawptr + rva - vaddr
        return None

    d = file_off(debug_rva) if debug_rva else None
    for k in range(debug_size // 28 if d is not None else 0):
        e = d + k * 28
        dtype, = struct.unpack_from("<I", b, e + 12)
        raw, = struct.unpack_from("<I", b, e + 24)
        if dtype == 2 and b[raw:raw + 4] == b"RSDS":
            g = b[raw + 4:raw + 20]
            age, = struct.unpack_from("<I", b, raw + 20)
            guid = g[0:4][::-1] + g[4:6][::-1] + g[6:8][::-1] + g[8:16]
            return "pe", f"{guid.hex()}{age:x}", base
    return "pe", None, base


def identify(path):
    with open(path, "rb") as f:
        b = f.read()
    if b[:4] == b"\x7fELF":
        fmt, ident, base = elf(b)
    elif b[:4] in (b"\xcf\xfa\xed\xfe",):
        fmt, ident, base = macho(b)
    elif b[:2] == b"MZ":
        fmt, ident, base = pe(b)
    else:
        raise ValueError(f"{path}: not ELF, Mach-O or PE")
    return {"format": fmt, "id": ident, "base": base}


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(__doc__)
        sys.exit(2)
    print(json.dumps(identify(sys.argv[1])))
