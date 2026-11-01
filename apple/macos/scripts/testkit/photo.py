"""Google XMP parsing, image extraction and lossless Live Photo still metadata."""
import hashlib
import struct
import xml.etree.ElementTree as ET
from dataclasses import dataclass

from .jpeg import FormatError, TIFF, app, edit_header, exif_identifier, image_hash, mpf_images, segments

XMP = b"http://ns.adobe.com/xap/1.0/\0"
EXTENDED = b"http://ns.adobe.com/xmp/extension/\0"
NOTE = "http://ns.adobe.com/xmp/note/"
CAMERA = "http://ns.google.com/photos/1.0/camera/"
CONTAINER = "http://ns.google.com/photos/1.0/container/"
ITEM = "http://ns.google.com/photos/1.0/container/item/"
HDR = "http://ns.adobe.com/hdr-gain-map/1.0/"
RDF = "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
for prefix, uri in (("x", "adobe:ns:meta/"), ("rdf", RDF), ("GCamera", CAMERA),
                    ("Container", CONTAINER), ("Item", ITEM), ("hdrgm", HDR)):
    ET.register_namespace(prefix, uri)


def q(ns, name):
    return "{" + ns + "}" + name


def values(root, ns, name):
    key = q(ns, name)
    return [v for e in root.iter() for k, v in e.attrib.items() if k == key] + [e.text.strip() for e in root.iter(key) if e.text]


class XMPPacket(tuple):
    def __new__(cls, segment, root, guid=None, fragments=()):
        packet = super().__new__(cls, (segment, root))
        packet.guid, packet.fragments = guid, fragments
        return packet


def xmp_packets(data):
    result = []
    groups = {}
    def document(xml):
        try:
            return ET.fromstring(xml.rstrip(b"\0"))
        except ET.ParseError as e:
            raise FormatError("Malformed XMP: " + str(e)) from e
    for s in segments(data):
        if s.marker == 0xE1 and s.payload.startswith(EXTENDED):
            p = len(EXTENDED)
            if len(s.payload) < p + 40:
                raise FormatError("Truncated Extended XMP header")
            try:
                guid = s.payload[p:p + 32].decode("ascii").upper()
            except UnicodeDecodeError as e:
                raise FormatError("Invalid Extended XMP GUID") from e
            if len(guid) != 32 or any(c not in "0123456789ABCDEF" for c in guid):
                raise FormatError("Invalid Extended XMP GUID")
            size, offset = struct.unpack_from(">II", s.payload, p + 32)
            chunk = s.payload[p + 40:]
            if not 0 < size <= len(data) or offset > size or len(chunk) > size - offset:
                raise FormatError("Invalid Extended XMP chunk bounds")
            groups.setdefault(guid, []).append((s, size, offset, chunk))
        elif s.marker == 0xE1 and s.payload.startswith(XMP):
            result.append(XMPPacket(s, document(s.payload[len(XMP):])))
    for guid, chunks in sorted(groups.items()):
        chunks.sort(key=lambda c: c[2])
        size, xml = chunks[0][1], bytearray()
        for _, total, offset, chunk in chunks:
            if total != size or offset != len(xml):
                raise FormatError("Missing, overlapping or inconsistent Extended XMP chunks")
            xml.extend(chunk)
        if len(xml) != size or hashlib.md5(xml).hexdigest().upper() != guid:
            raise FormatError("Extended XMP length/checksum mismatch")
        fragments = sorted((c[0] for c in chunks), key=lambda s: s.start)
        result.append(XMPPacket(fragments[0], document(bytes(xml)), guid, fragments))
    for packet in result:
        if packet.guid is None:
            for guid in values(packet[1], NOTE, "HasExtendedXMP"):
                if guid.upper() not in groups:
                    raise FormatError("Missing referenced Extended XMP")
    return result


def xmp_rewrites(packets, changed):
    changed = set(changed)
    replacements, ids = {}, {}
    for i in sorted(changed):
        packet = packets[i]
        if packet.guid is None:
            continue
        xml = ET.tostring(packet[1], encoding="utf-8")
        new_guid = hashlib.md5(xml).hexdigest().upper()
        ids[packet.guid] = new_guid
        maximum = 65533 - len(EXTENDED) - 40
        encoded = b"".join(app(0xE1, EXTENDED + new_guid.encode("ascii") + struct.pack(">II", len(xml), offset)
                             + xml[offset:offset + maximum]) for offset in range(0, len(xml), maximum))
        for segment in packet.fragments:
            replacements[segment.start] = b""
        replacements[packet[0].start] = encoded
    for old, new in ids.items():
        linked = False
        for i, packet in enumerate(packets):
            if packet.guid is not None:
                continue
            for element in packet[1].iter():
                key = q(NOTE, "HasExtendedXMP")
                if element.get(key, "").upper() == old:
                    element.set(key, new); changed.add(i); linked = True
                if element.tag == key and (element.text or "").upper() == old:
                    element.text = new; changed.add(i); linked = True
        if not linked:
            raise FormatError("Modified Extended XMP has no main-packet reference")
    for i in changed:
        if packets[i].guid is None:
            s, root = packets[i]
            replacements[s.start] = app(0xE1, XMP + ET.tostring(root, encoding="utf-8"))
    return replacements


def integer_unique(packets, name):
    found = [v for _, root in packets for v in values(root, CAMERA, name)]
    try:
        parsed = {int(v) for v in found}
    except ValueError as e:
        raise FormatError("Invalid " + name) from e
    if len(parsed) > 1:
        raise FormatError("Conflicting " + name)
    return next(iter(parsed)) if parsed else None


@dataclass
class MotionPhoto:
    still: bytes
    video: bytes
    timestamp_us: object
    timestamp_tag: object
    hdr: str
    warnings: list


def parse(data):
    packets = xmp_packets(data)
    motion = integer_unique(packets, "MotionPhoto")
    micro = integer_unique(packets, "MicroVideo")
    if motion == 0 or (motion != 1 and micro != 1):
        raise FormatError("Not a marked Google Motion Photo")
    lengths = []
    gainmap = False
    for _, root in packets:
        for element in root.iter():
            # Support both attribute and element XMP forms of Container:Item.
            semantic = element.get(q(ITEM, "Semantic"))
            length = element.get(q(ITEM, "Length"))
            if element.tag == q(CONTAINER, "Item") and semantic is None:
                semantic = element.findtext(q(ITEM, "Semantic"))
                length = element.findtext(q(ITEM, "Length"))
            if semantic == "MotionPhoto":
                try:
                    lengths.append(int(length))
                except (ValueError, TypeError) as e:
                    raise FormatError("Missing/invalid MotionPhoto item length") from e
            if semantic == "GainMap":
                gainmap = True
    if len(set(lengths)) > 1:
        raise FormatError("Conflicting MotionPhoto lengths")
    length = lengths[0] if lengths else integer_unique(packets, "MicroVideoOffset")
    if not length or length < 16 or length >= len(data):
        raise FormatError("Cannot locate embedded video from XMP")
    start = len(data) - length
    still, video = data[:start], data[start:]
    if start < segments(data)[-1].end:
        raise FormatError("Video overlaps JPEG")
    if video[4:8] != b"ftyp":
        raise FormatError("XMP offset does not point to an MP4 ftyp box")
    # Strict box walk; never search video bytes for JPEG EOI or ftyp signatures.
    pos, boxes = 0, []
    while pos < len(video):
        if pos + 8 > len(video):
            raise FormatError("Truncated MP4 header")
        size = struct.unpack_from(">I", video, pos)[0]
        header = 8
        if size == 1:
            if pos + 16 > len(video):
                raise FormatError("Truncated MP4 large size")
            size, header = struct.unpack_from(">Q", video, pos + 8)[0], 16
        elif size == 0:
            size = len(video) - pos
        if size < header or pos + size > len(video):
            raise FormatError("Invalid MP4 box size")
        boxes.append(video[pos + 4:pos + 8])
        pos += size
    if b"moov" not in boxes or b"mdat" not in boxes:
        raise FormatError("MP4 missing moov/mdat")
    timestamp = integer_unique(packets, "MotionPhotoPresentationTimestampUs")
    tag = "MotionPhotoPresentationTimestampUs"
    if timestamp is None or timestamp < 0:
        legacy = integer_unique(packets, "MicroVideoPresentationTimestampUs")
        if legacy is not None and legacy >= 0:
            timestamp, tag = legacy, "MicroVideoPresentationTimestampUs"
        else:
            timestamp, tag = None, None
    # Version is an unambiguous Ultra HDR signal; auxiliary payload is verified.
    ultra = any(values(root, HDR, "Version") for _, root in packets)
    aux = mpf_images(still)
    warnings = []
    if ultra:
        if len(aux) < 2:
            raise FormatError("Ultra HDR declaration without an MPF auxiliary image")
        for image in aux[1:]:
            segments(image)
        hdr = "ultra-hdr"
    else:
        hdr = "legacy-or-sdr"
        if gainmap:
            warnings.append("GainMap container item without Ultra HDR Version; inspect legacy HDR")
    return MotionPhoto(still, video, timestamp, tag, hdr, warnings)


def live_still(still, identifier):
    replacements, insert, has_exif = {}, b"", False
    for s in segments(still):
        if s.marker == 0xE1 and s.payload.startswith(b"Exif\0\0"):
            if has_exif:
                raise FormatError("Multiple EXIF segments")
            replacements[s.start] = app(0xE1, exif_identifier(s.payload, identifier))
            has_exif = True
    if not has_exif:
        insert = app(0xE1, exif_identifier(None, identifier))
    motion_keys = {q(CAMERA, name) for name in (
        "MotionPhoto", "MotionPhotoVersion", "MotionPhotoPresentationTimestampUs",
        "MicroVideo", "MicroVideoVersion", "MicroVideoOffset", "MicroVideoPresentationTimestampUs")}
    packets, changed = xmp_packets(still), set()
    for i, (s, root) in enumerate(packets):
        for parent in list(root.iter()):
            for key in list(parent.attrib):
                if key in motion_keys:
                    del parent.attrib[key]
                    changed.add(i)
            for child in list(parent):
                if child.tag in motion_keys:
                    parent.remove(child)
                    changed.add(i)
        # Remove only MotionPhoto items; keep GainMap and its HDR metadata.
        for parent in list(root.iter()):
            for child in list(parent):
                candidates = list(child.iter())
                if any(e.get(q(ITEM, "Semantic")) == "MotionPhoto" or
                       (e.tag == q(ITEM, "Semantic") and e.text == "MotionPhoto") for e in candidates):
                    # Only remove RDF list items, not entire Description/RDF trees.
                    if child.tag == q(RDF, "li"):
                        parent.remove(child)
                        changed.add(i)
    replacements.update(xmp_rewrites(packets, changed))
    output = edit_header(still, replacements, insert)
    before, after = mpf_images(still), mpf_images(output)
    if [hashlib.sha256(x).hexdigest() for x in before[1:]] != [hashlib.sha256(x).hexdigest() for x in after[1:]]:
        raise FormatError("Auxiliary image bytes changed")
    return output


def ultra_from_apple(still, gain_jpeg, stops):
    """Repackage a converted legacy gain map; primary JPEG coding is unchanged."""
    gain_root = ET.Element(q("adobe:ns:meta/", "xmpmeta"))
    rdf = ET.SubElement(gain_root, q(RDF, "RDF"))
    ET.SubElement(rdf, q(RDF, "Description"), {
        q(HDR, "Version"): "1.0", q(HDR, "GainMapMin"): "0",
        q(HDR, "GainMapMax"): str(stops), q(HDR, "Gamma"): "1",
        q(HDR, "OffsetSDR"): "0", q(HDR, "OffsetHDR"): "0",
        q(HDR, "HDRCapacityMin"): "0", q(HDR, "HDRCapacityMax"): str(stops),
        q(HDR, "BaseRenditionIsHDR"): "False"})
    gain = gain_jpeg[:2] + app(0xE1, XMP + ET.tostring(gain_root)) + gain_jpeg[2:]
    primary_end = segments(still)[-1].end
    primary = still[:primary_end]
    replacements = {}
    for s in segments(primary):
        if s.marker == 0xE2 and s.payload.startswith(b"MPF\0"):
            replacements[s.start] = b""
    root = ET.Element(q("adobe:ns:meta/", "xmpmeta"))
    rdf = ET.SubElement(root, q(RDF, "RDF"))
    desc = ET.SubElement(rdf, q(RDF, "Description"), {q(HDR, "Version"): "1.0"})
    seq = ET.SubElement(ET.SubElement(desc, q(CONTAINER, "Directory")), q(RDF, "Seq"))
    for semantic, length in (("Primary", 0), ("GainMap", len(gain))):
        li = ET.SubElement(seq, q(RDF, "li"), {q(RDF, "parseType"): "Resource"})
        ET.SubElement(li, q(CONTAINER, "Item"), {q(ITEM, "Semantic"): semantic,
            q(ITEM, "Mime"): "image/jpeg", q(ITEM, "Length"): str(length)})
    # Merge with original XMP without duplicating Container:Directory.
    packets = xmp_packets(primary)
    for s, old_root in packets:
        for parent in list(old_root.iter()):
            for child in list(parent):
                if child.tag == q(CONTAINER, "Directory"):
                    parent.remove(child)
        old_rdf = old_root.find(q(RDF, "RDF"))
        if old_rdf is None:
            raise FormatError("XMP RDF missing")
        old_rdf.append(desc)
        replacements[s.start] = app(0xE1, XMP + ET.tostring(old_root))
        break
    addition = b"" if packets else app(0xE1, XMP + ET.tostring(root))
    # Existing MPF is deliberately replaced with a fresh two-image index.
    primary = edit_header(primary, replacements, addition)
    # MPF TIFF: version, image count, 32-byte MPEntry array.
    tiff = b"MM\0*\0\0\0\x08" + struct.pack(">H", 3)
    tiff += struct.pack(">HHI4s", 0xB000, 7, 4, b"0100")
    tiff += struct.pack(">HHII", 0xB001, 4, 1, 2)
    tiff += struct.pack(">HHII", 0xB002, 7, 32, 50) + b"\0" * 4
    mpf_len = 4 + 4 + len(tiff) + 32
    final_size = len(primary) + mpf_len
    # New MPF is immediately after SOI; TIFF origin is at absolute offset 10.
    tiff += struct.pack(">IIIHH", 0x20030000, final_size, 0, 0, 0)
    tiff += struct.pack(">IIIHH", 0, len(gain), final_size - 10, 0, 0)
    output = primary[:2] + app(0xE2, b"MPF\0" + tiff) + primary[2:] + gain
    if image_hash(still) != image_hash(output):
        raise FormatError("Primary JPEG changed during legacy HDR conversion")
    if mpf_images(output)[1] != gain:
        raise FormatError("Generated Ultra HDR MPF index is invalid")
    return output


def legacy_color_profile(still, srgb_icc):
    """Ultra HDR requires an ICC profile. Only infer sRGB from explicit EXIF sRGB."""
    segs = segments(still)
    if any(s.marker == 0xE2 and s.payload.startswith(b"ICC_PROFILE\0") for s in segs):
        return still
    for s in segs:
        if s.marker != 0xE1 or not s.payload.startswith(b"Exif\0\0"):
            continue
        t = TIFF(s.payload[6:])
        root, _ = t.directory(t.u32(4))
        ptr = t.tag(root, 0x8769)
        if ptr:
            exif, _ = t.directory(struct.unpack(t.order + "I", ptr[8:])[0])
            color = t.tag(exif, 0xA001)
            if (color and struct.unpack(t.order + "HHI", color[:8]) == (0xA001, 3, 1)
                    and struct.unpack(t.order + "H", color[8:10])[0] == 1):
                return edit_header(still, {}, app(0xE2, b"ICC_PROFILE\0\x01\x01" + srgb_icc))
    raise FormatError("Legacy HDR has no ICC profile or explicit EXIF sRGB; color space not guessed")
