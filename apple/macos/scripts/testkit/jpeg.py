"""JPEG/EXIF/MPF edits. No image decoder or encoder is used here."""
import hashlib
import struct
from dataclasses import dataclass


class FormatError(ValueError):
    pass


@dataclass
class Segment:
    marker: int
    start: int
    end: int
    payload: bytes


def segments(data):
    """Parse one JPEG, including progressive scans and stuffed entropy bytes."""
    if data[:2] != b"\xff\xd8":
        raise FormatError("Not a JPEG")
    result, pos = [], 2
    while pos < len(data):
        start = pos
        if data[pos] != 255:
            raise FormatError("Invalid JPEG marker at %d" % pos)
        while pos < len(data) and data[pos] == 255:
            pos += 1
        if pos >= len(data):
            break
        marker = data[pos]
        pos += 1
        if marker == 0xD9:
            result.append(Segment(marker, start, pos, b""))
            return result
        if marker in (0xD8, 0x01) or 0xD0 <= marker <= 0xD7:
            raise FormatError("Unexpected standalone JPEG marker")
        if pos + 2 > len(data):
            break
        length = struct.unpack_from(">H", data, pos)[0]
        end = pos + length
        if length < 2 or end > len(data):
            raise FormatError("Truncated JPEG segment")
        payload = data[pos + 2:end]
        pos = end
        if marker == 0xDA:
            # Include the entropy stream with SOS; restart markers are preserved.
            while pos < len(data):
                at = data.find(b"\xff", pos)
                if at < 0:
                    raise FormatError("Unterminated JPEG entropy stream")
                p = at + 1
                while p < len(data) and data[p] == 255:
                    p += 1
                if p == len(data):
                    raise FormatError("Truncated JPEG entropy marker")
                if data[p] == 0 or 0xD0 <= data[p] <= 0xD7:
                    pos = p + 1
                else:
                    pos = at
                    break
            end = pos
        result.append(Segment(marker, start, end, payload))
    raise FormatError("Missing JPEG EOI")


def image_hash(data):
    """Hash coding tables, frame/scan headers and entropy, excluding metadata."""
    h = hashlib.sha256()
    for s in segments(data):
        if not 0xE0 <= s.marker <= 0xEF and s.marker != 0xFE:
            h.update(data[s.start:s.end])
    return h.hexdigest()


def app(marker, payload):
    if len(payload) + 2 > 65535:
        raise FormatError("JPEG metadata exceeds 64 KiB segment limit")
    return bytes((255, marker)) + struct.pack(">H", len(payload) + 2) + payload


class TIFF:
    def __init__(self, data):
        self.data = data
        if data[:2] not in (b"II", b"MM"):
            raise FormatError("Invalid TIFF byte order")
        self.order = "<" if data[:2] == b"II" else ">"
        if self.u16(2) != 42:
            raise FormatError("Invalid TIFF version")

    def u16(self, p):
        return self.unpack("H", p)

    def u32(self, p):
        return self.unpack("I", p)

    def unpack(self, fmt, p):
        try:
            return struct.unpack_from(self.order + fmt, self.data, p)[0]
        except struct.error as e:
            raise FormatError("Truncated TIFF") from e

    def directory(self, offset):
        n = self.u16(offset)
        if offset < 8 or offset + 2 + n * 12 + 4 > len(self.data):
            raise FormatError("Invalid TIFF directory")
        entries = [self.data[offset + 2 + i * 12:offset + 14 + i * 12] for i in range(n)]
        return entries, self.u32(offset + 2 + n * 12)

    def tag(self, entries, tag):
        return next((e for e in entries if struct.unpack(self.order + "H", e[:2])[0] == tag), None)

    def entry(self, tag, typ, count, value):
        return struct.pack(self.order + "HHII", tag, typ, count, value)

    def serialize(self, entries, next_ifd=0):
        entries = sorted(entries, key=lambda e: struct.unpack(self.order + "H", e[:2])[0])
        return struct.pack(self.order + "H", len(entries)) + b"".join(entries) + struct.pack(self.order + "I", next_ifd)


def maker_note(identifier, old=b""):
    """Retain existing Apple gain-map tags and their original relative offsets."""
    value = identifier.encode("ascii") + b"\0"
    if old.startswith(b"Apple iOS\0\x00\x01"):
        order = ">" if old[12:14] == b"MM" else "<" if old[12:14] == b"II" else None
        if order is None or len(old) < 16:
            raise FormatError("Malformed Apple MakerNote")
        n = struct.unpack_from(order + "H", old, 14)[0]
        end = 16 + n * 12 + 4
        if end > len(old):
            raise FormatError("Truncated Apple MakerNote")
        entries = [old[16 + i * 12:28 + i * 12] for i in range(n)]
        kept = [e for e in entries if struct.unpack(order + "H", e[:2])[0] != 0x11]
        # Growing the front table moves old data: adjust external value offsets.
        delta = (len(kept) + 1 - n) * 12
        sizes = {1: 1, 2: 1, 3: 2, 4: 4, 5: 8, 6: 1, 7: 1, 8: 2, 9: 4, 10: 8, 11: 4, 12: 8}
        patched = []
        for e in kept:
            _, typ, count, offset = struct.unpack(order + "HHII", e)
            if typ not in sizes:
                raise FormatError("Unknown Apple MakerNote field type")
            if sizes[typ] * count > 4:
                if offset < end or offset + sizes[typ] * count > len(old):
                    raise FormatError("Unsupported Apple MakerNote offset")
                e = e[:8] + struct.pack(order + "I", offset + delta)
            patched.append(e)
        note_end = len(old) + delta
        patched.append(struct.pack(order + "HHII", 0x11, 2, len(value), note_end))
        patched.sort(key=lambda e: struct.unpack(order + "H", e[:2])[0])
        return old[:14] + struct.pack(order + "H", len(patched)) + b"".join(patched) + b"\0" * 4 + old[end:] + value
    return (b"Apple iOS\0\x00\x01MM" + struct.pack(">HHHII", 1, 0x11, 2, len(value), 32)
            + b"\0" * 4 + value)


def exif_identifier(payload, identifier):
    raw = payload[6:] if payload else b"MM\0*\0\0\0\x08\0\0\0\0\0\0"
    t = TIFF(raw)
    root, root_next = t.directory(t.u32(4))
    pointer = t.tag(root, 0x8769)
    exif, exif_next = t.directory(struct.unpack(t.order + "I", pointer[8:])[0]) if pointer else ([], 0)
    old_entry = t.tag(exif, 0x927C)
    old = b""
    if old_entry:
        _, typ, count, offset = struct.unpack(t.order + "HHII", old_entry)
        if typ != 7 or count <= 4 or offset + count > len(raw):
            raise FormatError("Unsupported MakerNote layout")
        old = raw[offset:offset + count]
    note = maker_note(identifier, old)
    # Append directories, retaining all original offsets, GPS, thumbnail and data.
    out = bytearray(raw)
    if len(out) % 2:
        out += b"\0"
    note_at = len(out)
    out += note
    if len(out) % 2:
        out += b"\0"
    exif_at = len(out)
    kept = [e for e in exif if e[:2] != struct.pack(t.order + "H", 0x927C)]
    out += t.serialize(kept + [t.entry(0x927C, 7, len(note), note_at)], exif_next)
    root_at = len(out)
    kept = [e for e in root if e[:2] != struct.pack(t.order + "H", 0x8769)]
    out += t.serialize(kept + [t.entry(0x8769, 4, 1, exif_at)], root_next)
    struct.pack_into(t.order + "I", out, 4, root_at)
    return b"Exif\0\0" + bytes(out)


def edit_header(data, replacements, insert=b""):
    """Apply segment edits and fix MPF addresses after all header size changes."""
    segs = segments(data)
    edits = [(s.start, s.end, replacements[s.start]) for s in segs if s.start in replacements]
    if insert:
        edits.append((2, 2, insert))
    edits.sort(key=lambda e: (e[0], e[1]))

    def mapped(p):
        delta = 0
        for a, b, replacement in edits:
            if b <= p:
                delta += len(replacement) - (b - a)
            elif a < p < b:
                raise FormatError("MPF pointer targets replaced metadata")
        return p + delta

    out, cursor = bytearray(), 0
    for a, b, replacement in edits:
        if a < cursor:
            raise FormatError("Overlapping JPEG edits")
        out += data[cursor:a] + replacement
        cursor = b
    out += data[cursor:]
    delta = len(out) - len(data)
    for s in segs:
        if s.marker != 0xE2 or not s.payload.startswith(b"MPF\0") or s.start in replacements:
            continue
        old_base = s.start + 8
        new_base = mapped(s.start) + 8
        t = TIFF(s.payload[4:])
        entries, _ = t.directory(t.u32(4))
        entry = t.tag(entries, 0xB002)
        if not entry:
            raise FormatError("MPF has no MPEntry index")
        count, offset = struct.unpack(t.order + "II", entry[4:])
        if count % 16 or offset + count > len(t.data):
            raise FormatError("Invalid MPEntry array")
        for index in range(count // 16):
            at = offset + index * 16
            size, relative = struct.unpack_from(t.order + "II", t.data, at + 4)
            if index == 0:
                if relative != 0:
                    raise FormatError("Invalid primary MPEntry offset")
                struct.pack_into(t.order + "I", out, new_base + at + 4, size + delta)
            elif relative:
                target = old_base + relative
                if target + size > len(data) or data[target:target + 2] != b"\xff\xd8":
                    raise FormatError("MPF auxiliary image lies outside still data")
                struct.pack_into(t.order + "I", out, new_base + at + 8, mapped(target) - new_base)
    if image_hash(data) != image_hash(out):
        raise FormatError("JPEG coding data changed")
    return bytes(out)


def mpf_images(data):
    for s in segments(data):
        if s.marker == 0xE2 and s.payload.startswith(b"MPF\0"):
            t = TIFF(s.payload[4:])
            entries, _ = t.directory(t.u32(4))
            e = t.tag(entries, 0xB002)
            if e is None:
                raise FormatError("MPF index missing")
            count, offset = struct.unpack(t.order + "II", e[4:])
            if count % 16 or offset + count > len(t.data):
                raise FormatError("Invalid MPF array")
            result = []
            for i in range(count // 16):
                size, relative = struct.unpack_from(t.order + "II", t.data, offset + i * 16 + 4)
                start = 0 if i == 0 else s.start + 8 + relative
                if start + size > len(data):
                    raise FormatError("Truncated MPF image")
                result.append(data[start:start + size])
            return result
    return []
