"""Tests for axml.py: python3 -I ci/mobile/test_axml.py"""

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import axml  # noqa: E402

FIXTURE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "testdata",
                       "AndroidManifest.androguard-TestActivity.bin")


def shape(doc):
    """Everything a parser sees, as plain data."""
    out = []
    for n in doc.nodes:
        attrs = [(a.ns, a.name, a.rid, a.raw, a.dtype, a.data) for a in n.attrs]
        out.append((n.kind, n.ns, n.name, n.prefix, n.uri, n.text, attrs))
    return out


class Axml(unittest.TestCase):
    def setUp(self):
        with open(FIXTURE, "rb") as f:
            self.data = f.read()

    def test_round_trip_is_exact_in_both_pool_encodings(self):
        doc = axml.parse(self.data)
        for utf8 in (False, True):
            doc.utf8 = utf8
            again = axml.parse(doc.serialize())
            self.assertEqual(shape(again), shape(axml.parse(self.data)))
            self.assertEqual(again.utf8, utf8)

    def test_background_audio_entries_are_added_once(self):
        self.assertEqual(len(axml.check_background_audio(self.data)), 3)
        out, changes = axml.add_background_audio(self.data)
        self.assertEqual(len(changes), 4)
        self.assertEqual(axml.check_background_audio(out), [])
        out2, changes2 = axml.add_background_audio(out)
        self.assertEqual(changes2, [])
        self.assertEqual(shape(axml.parse(out2)), shape(axml.parse(out)))

    def test_the_service_sits_inside_application_and_permissions_before_it(self):
        out, _ = axml.add_background_audio(self.data)
        doc = axml.parse(out)
        kinds = [(n.kind, n.name) for n in doc.nodes]
        app_start = kinds.index((axml.RES_XML_START_ELEMENT, "application"))
        app_end = kinds.index((axml.RES_XML_END_ELEMENT, "application"))
        service = kinds.index((axml.RES_XML_START_ELEMENT, "service"))
        self.assertTrue(app_start < service < app_end)
        for i, k in enumerate(kinds):
            if k == (axml.RES_XML_START_ELEMENT, "uses-permission"):
                self.assertLess(i, app_start)
        s = doc.has("service", "dev.vieww.audio.PlaybackService")
        self.assertEqual([a.name for a in s.attrs], ["name", "exported", "foregroundServiceType"])
        self.assertEqual(s.attr("exported").data, 0)
        self.assertEqual(s.attr("foregroundServiceType").data, axml.FGS_MEDIA_PLAYBACK)

    def test_resource_attribute_names_lead_the_pool(self):
        out, _ = axml.add_background_audio(self.data)
        doc = axml.parse(out)
        # Every resource-id attribute resolved back to its id: the map and
        # the pool front stayed aligned after insertion.
        for n in doc.nodes:
            for a in n.attrs:
                if a.ns == axml.ANDROID_NS and a.name in axml.ATTR:
                    self.assertEqual(a.rid, axml.ATTR[a.name])


if __name__ == "__main__":
    unittest.main()
