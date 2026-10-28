"""Read, edit and write Android binary XML (a compiled AndroidManifest.xml).

    doc = axml.parse(data)
    doc.add_permission("android.permission.FOREGROUND_SERVICE")
    data = doc.serialize()

Why this exists: cargo-apk writes the manifest from Cargo.toml metadata and
has no way to declare a <service>. Background audio (AU-04) needs one, a
foreground service of type mediaPlayback, so apk-add-code.sh adds it to the
compiled manifest after the build.

The model keeps strings as Python strings rather than pool indices, and
rebuilds the string pool on write. That is what makes insertion safe:
attribute names that carry an android resource id must sit at the front of
the pool, aligned with the resource map, and adding one shifts every other
index. Rebuilding avoids renumbering in place.

Chunk layouts follow frameworks/base/libs/androidfw/include/androidfw/
ResourceTypes.h (ResStringPool_header, ResXMLTree_node, ResXMLTree_attrExt,
ResXMLTree_attribute, Res_value).
"""

import struct

RES_STRING_POOL = 0x0001
RES_XML = 0x0003
RES_XML_START_NAMESPACE = 0x0100
RES_XML_END_NAMESPACE = 0x0101
RES_XML_START_ELEMENT = 0x0102
RES_XML_END_ELEMENT = 0x0103
RES_XML_CDATA = 0x0104
RES_XML_RESOURCE_MAP = 0x0180
UTF8_FLAG = 0x100
NONE = 0xFFFFFFFF

TYPE_STRING = 0x03
TYPE_INT_HEX = 0x11
TYPE_INT_BOOLEAN = 0x12

ANDROID_NS = "http://schemas.android.com/apk/res/android"
ATTR = {
    "name": 0x01010003,
    "exported": 0x01010010,
    "foregroundServiceType": 0x01010599,
}
#: android:foregroundServiceType="mediaPlayback"
FGS_MEDIA_PLAYBACK = 0x2


def _read_strings(buf, at):
    count, styles, flags, start, _ = struct.unpack_from("<5I", buf, at + 8)
    if styles:
        raise ValueError("styled strings in a manifest pool are not supported")
    offsets = struct.unpack_from("<%dI" % count, buf, at + 28)
    base = at + start
    utf8 = bool(flags & UTF8_FLAG)
    out = []
    for off in offsets:
        p = base + off
        if utf8:
            for _ in range(2):
                n = buf[p]
                p += 1
                if n & 0x80:
                    n = ((n & 0x7F) << 8) | buf[p]
                    p += 1
            out.append(bytes(buf[p:p + n]).decode("utf-8"))
        else:
            n = struct.unpack_from("<H", buf, p)[0]
            p += 2
            if n & 0x8000:
                n = ((n & 0x7FFF) << 16) | struct.unpack_from("<H", buf, p)[0]
                p += 2
            out.append(bytes(buf[p:p + 2 * n]).decode("utf-16-le"))
    return out, utf8


def _encode_strings(strings, utf8):
    data = bytearray()
    offsets = []
    for s in strings:
        offsets.append(len(data))
        if utf8:
            raw = s.encode("utf-8")
            for n in (len(s.encode("utf-16-le")) // 2, len(raw)):
                if n > 0x7F:
                    data += bytes([0x80 | (n >> 8), n & 0xFF])
                else:
                    data.append(n)
            data += raw + b"\0"
        else:
            raw = s.encode("utf-16-le")
            n = len(raw) // 2
            if n > 0x7FFF:
                data += struct.pack("<HH", 0x8000 | (n >> 16), n & 0xFFFF)
            else:
                data += struct.pack("<H", n)
            data += raw + b"\0\0"
    while len(data) % 4:
        data.append(0)
    header = 28
    start = header + 4 * len(strings)
    size = start + len(data)
    out = struct.pack("<HHI5I", RES_STRING_POOL, header, size, len(strings), 0,
                      UTF8_FLAG if utf8 else 0, start, 0)
    out += struct.pack("<%dI" % len(offsets), *offsets)
    return out + bytes(data)


class Attr:
    """One attribute. `rid` is its android resource id (None for a plain
    name); `raw` the raw string value or None; `dtype`/`data` the typed
    value, with `data` a str when `dtype` is TYPE_STRING."""

    def __init__(self, ns, name, rid, raw, dtype, data):
        self.ns, self.name, self.rid = ns, name, rid
        self.raw, self.dtype, self.data = raw, dtype, data

    def __repr__(self):
        return "Attr(%s=%r)" % (self.name, self.data)


class Node:
    """A start/end element, namespace or CDATA node."""

    def __init__(self, kind, line=1, comment=None, **f):
        self.kind, self.line, self.comment = kind, line, comment
        self.ns = f.get("ns")
        self.name = f.get("name")
        self.attrs = f.get("attrs", [])
        self.indices = f.get("indices", (0, 0, 0))  # id, class, style
        self.prefix = f.get("prefix")
        self.uri = f.get("uri")
        self.text = f.get("text")
        self.typed = f.get("typed", (0, 0))

    def attr(self, name):
        for a in self.attrs:
            if a.name == name:
                return a
        return None


class Document:
    def __init__(self, nodes, utf8):
        self.nodes = nodes
        self.utf8 = utf8

    # --- queries -------------------------------------------------------

    def elements(self, name):
        return [n for n in self.nodes if n.kind == RES_XML_START_ELEMENT and n.name == name]

    def _index(self, name, end=False):
        kind = RES_XML_END_ELEMENT if end else RES_XML_START_ELEMENT
        for i, n in enumerate(self.nodes):
            if n.kind == kind and n.name == name:
                return i
        raise ValueError("no <%s> in the manifest" % name)

    def has(self, element, android_name):
        for e in self.elements(element):
            a = e.attr("name")
            if a is not None and a.data == android_name:
                return e
        return None

    # --- edits ---------------------------------------------------------

    def _android_attr(self, name, dtype, value):
        raw = value if dtype == TYPE_STRING else None
        return Attr(ANDROID_NS, name, ATTR[name], raw, dtype, value)

    def _insert(self, at, name, attrs):
        attrs = sorted(attrs, key=lambda a: a.rid if a.rid is not None else 1 << 32)
        start = Node(RES_XML_START_ELEMENT, ns=None, name=name, attrs=attrs)
        end = Node(RES_XML_END_ELEMENT, ns=None, name=name)
        self.nodes[at:at] = [start, end]

    def add_permission(self, permission):
        """<uses-permission android:name=…/> before <application>."""
        if self.has("uses-permission", permission):
            return False
        self._insert(self._index("application"), "uses-permission",
                     [self._android_attr("name", TYPE_STRING, permission)])
        return True

    def add_service(self, cls, foreground_type=None, exported=False):
        """<service android:name=… android:exported=… [fgsType]/> as the
        last child of <application>."""
        if self.has("service", cls):
            return False
        attrs = [
            self._android_attr("name", TYPE_STRING, cls),
            self._android_attr("exported", TYPE_INT_BOOLEAN, NONE if exported else 0),
        ]
        if foreground_type is not None:
            attrs.append(self._android_attr("foregroundServiceType", TYPE_INT_HEX, foreground_type))
        self._insert(self._index("application", end=True), "service", attrs)
        return True

    # --- write ---------------------------------------------------------

    def serialize(self):
        # The pool: resource-id attribute names first (aligned with the
        # resource map), then every other string in first-use order.
        rid_names, rids = [], []
        for n in self.nodes:
            for a in n.attrs:
                if a.rid is not None and a.name not in rid_names:
                    rid_names.append(a.name)
                    rids.append(a.rid)
        pool = list(rid_names)
        index = {s: i for i, s in enumerate(pool)}

        def ref(s):
            if s is None:
                return NONE
            if s not in index:
                index[s] = len(pool)
                pool.append(s)
            return index[s]

        body = bytearray()
        for n in self.nodes:
            comment = ref(n.comment)
            if n.kind in (RES_XML_START_NAMESPACE, RES_XML_END_NAMESPACE):
                body += struct.pack("<HHIIIII", n.kind, 16, 24, n.line, comment,
                                    ref(n.prefix), ref(n.uri))
            elif n.kind == RES_XML_END_ELEMENT:
                body += struct.pack("<HHIIIII", n.kind, 16, 24, n.line, comment,
                                    ref(n.ns), ref(n.name))
            elif n.kind == RES_XML_CDATA:
                dtype, data = n.typed
                if dtype == TYPE_STRING:
                    data = ref(data)
                body += struct.pack("<HHIIIIHBBI", n.kind, 16, 28, n.line, comment,
                                    ref(n.text), 8, 0, dtype, data)
            elif n.kind == RES_XML_START_ELEMENT:
                attrs = bytearray()
                for a in n.attrs:
                    name = ref(a.name)
                    if a.rid is not None and name >= len(rids):
                        raise AssertionError("resource attribute outside the map")
                    data = ref(a.data) if a.dtype == TYPE_STRING else a.data
                    attrs += struct.pack("<IIIHBBI", ref(a.ns), name, ref(a.raw), 8, 0,
                                         a.dtype, data)
                size = 16 + 20 + len(attrs)
                idx, cls, sty = n.indices
                body += struct.pack("<HHIIIIIHHHHHH", n.kind, 16, size, n.line, comment,
                                    ref(n.ns), ref(n.name), 20, 20, len(n.attrs),
                                    idx, cls, sty)
                body += attrs
            else:
                raise ValueError("unknown node kind %#x" % n.kind)

        strings = _encode_strings(pool, self.utf8)
        resmap = struct.pack("<HHI", RES_XML_RESOURCE_MAP, 8, 8 + 4 * len(rids))
        resmap += struct.pack("<%dI" % len(rids), *rids)
        total = 8 + len(strings) + len(resmap) + len(body)
        return struct.pack("<HHI", RES_XML, 8, total) + strings + resmap + bytes(body)


def parse(data):
    buf = bytes(data)
    kind, header, _size = struct.unpack_from("<HHI", buf, 0)
    if kind != RES_XML:
        raise ValueError("not binary XML (type %#x)" % kind)
    strings, resmap, utf8, nodes = [], [], True, []

    def s(i):
        return None if i == NONE else strings[i]

    at = header
    while at < len(buf):
        kind, hsize, size = struct.unpack_from("<HHI", buf, at)
        if size < 8 or at + size > len(buf):
            raise ValueError("corrupt chunk at %d" % at)
        if kind == RES_STRING_POOL:
            strings, utf8 = _read_strings(buf, at)
        elif kind == RES_XML_RESOURCE_MAP:
            resmap = list(struct.unpack_from("<%dI" % ((size - hsize) // 4), buf, at + hsize))
        elif kind in (RES_XML_START_NAMESPACE, RES_XML_END_NAMESPACE, RES_XML_START_ELEMENT,
                      RES_XML_END_ELEMENT, RES_XML_CDATA):
            line, comment = struct.unpack_from("<II", buf, at + 8)
            body = at + hsize
            if kind in (RES_XML_START_NAMESPACE, RES_XML_END_NAMESPACE):
                prefix, uri = struct.unpack_from("<II", buf, body)
                nodes.append(Node(kind, line, s(comment), prefix=s(prefix), uri=s(uri)))
            elif kind == RES_XML_END_ELEMENT:
                ns, name = struct.unpack_from("<II", buf, body)
                nodes.append(Node(kind, line, s(comment), ns=s(ns), name=s(name)))
            elif kind == RES_XML_CDATA:
                text, _vs, _r, dtype, data = struct.unpack_from("<IHBBI", buf, body)
                if dtype == TYPE_STRING:
                    data = s(data)
                nodes.append(Node(kind, line, s(comment), text=s(text), typed=(dtype, data)))
            else:
                ns, name, astart, asize, count, idx, cls, sty = struct.unpack_from(
                    "<IIHHHHHH", buf, body)
                attrs = []
                for i in range(count):
                    a = body + astart + i * asize
                    ans, aname, raw, _vs, _r, dtype, data = struct.unpack_from("<IIIHBBI", buf, a)
                    if dtype == TYPE_STRING:
                        data = s(data)
                    rid = resmap[aname] if aname < len(resmap) else None
                    attrs.append(Attr(s(ans), s(aname), rid, s(raw), dtype, data))
                nodes.append(Node(kind, line, s(comment), ns=s(ns), name=s(name),
                                  attrs=attrs, indices=(idx, cls, sty)))
        at += size
    return Document(nodes, utf8)


def add_background_audio(data, service="dev.vieww.audio.PlaybackService"):
    """The manifest entries AU-04 needs. Returns (bytes, list of changes)."""
    doc = parse(data)
    changes = []
    for p in ("android.permission.FOREGROUND_SERVICE",
              "android.permission.FOREGROUND_SERVICE_MEDIA_PLAYBACK",
              "android.permission.WAKE_LOCK"):
        if doc.add_permission(p):
            changes.append("uses-permission " + p)
    if doc.add_service(service, foreground_type=FGS_MEDIA_PLAYBACK):
        changes.append("service " + service + " (mediaPlayback)")
    return doc.serialize(), changes


def check_background_audio(data, service="dev.vieww.audio.PlaybackService"):
    """What is missing for background audio, as sentences (empty = fine)."""
    doc = parse(data)
    missing = []
    for p in ("android.permission.FOREGROUND_SERVICE",
              "android.permission.FOREGROUND_SERVICE_MEDIA_PLAYBACK"):
        if not doc.has("uses-permission", p):
            missing.append("no <uses-permission %s>" % p)
    s = doc.has("service", service)
    if s is None:
        missing.append("no <service %s>" % service)
    else:
        t = s.attr("foregroundServiceType")
        if t is None or not (t.data & FGS_MEDIA_PLAYBACK):
            missing.append("<service %s> is not foregroundServiceType=mediaPlayback" % service)
    return missing
