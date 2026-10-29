"""module_id.py reads ids as the crash reporter spells them (FW-12).

    python3 -I ci/release/test_module_id.py
"""

import os
import subprocess
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import module_id  # noqa: E402

DATA = os.path.join(HERE, "..", "..", "crates", "vieww-foundation", "src", "crash", "testdata")


class ModuleId(unittest.TestCase):
    def test_pe_codeview_guid_and_image_base(self):
        r = module_id.identify(os.path.join(DATA, "pe-codeview.bin"))
        self.assertEqual(r, {"format": "pe", "id": "33221100554477668899aabbccddeeff2a", "base": 0x140000000})

    def test_macho_uuid_and_text_vmaddr(self):
        r = module_id.identify(os.path.join(DATA, "macho-uuid.bin"))
        self.assertEqual(r, {"format": "macho", "id": "000102030405060708090a0b0c0d0e0f", "base": 0x100000000})

    def test_elf_build_id_of_this_python(self):
        if not sys.executable or not open(sys.executable, "rb").read(4) == b"\x7fELF":
            self.skipTest("not an ELF host")
        r = module_id.identify(sys.executable)
        self.assertEqual(r["format"], "elf")
        try:
            out = subprocess.run(["readelf", "-n", sys.executable], capture_output=True, text=True).stdout
        except FileNotFoundError:
            self.skipTest("no readelf")
        want = next((l.split()[-1] for l in out.splitlines() if "Build ID" in l), None)
        self.assertEqual(r["id"], want)

    def test_garbage_is_refused(self):
        path = os.path.join(DATA, "..", "frames.rs")
        with self.assertRaises(ValueError):
            module_id.identify(path)


if __name__ == "__main__":
    unittest.main()
