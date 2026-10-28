"""Recognize existing Apple HDR data without relying on Monterey's HDR renderer."""
import math
import struct

from .jpeg import FormatError, TIFF, mpf_images, segments
from .photo import values, xmp_packets

APPLE = "http://ns.apple.com/HDRGainMap/1.0/"
PIXEL = "http://ns.apple.com/pixeldatainfo/1.0/"


def legacy_apple(still):
    images = mpf_images(still)
    candidates = []
    for image in images[1:]:
        for _, root in xmp_packets(image):
            if "urn:com:apple:photo:2020:aux:hdrgainmap" in values(root, PIXEL, "AuxiliaryImageType"):
                if values(root, APPLE, "HDRGainMapVersion") != ["65536"]:
                    return None, "Unsupported legacy Apple gain-map version; original HDR retained"
                candidates.append(image)
    if not candidates:
        return None, "No recognized legacy Apple gain map; SDR is not expanded"
    if len(candidates) != 1:
        return None, "Multiple legacy HDR maps; original retained"
    for segment in segments(still):
        if segment.marker != 0xE1 or not segment.payload.startswith(b"Exif\0\0"):
            continue
        t = TIFF(segment.payload[6:])
        root, _ = t.directory(t.u32(4))
        ptr = t.tag(root, 0x8769)
        if ptr is None:
            continue
        exif, _ = t.directory(struct.unpack(t.order + "I", ptr[8:])[0])
        entry = t.tag(exif, 0x927C)
        if entry is None:
            continue
        _, typ, count, offset = struct.unpack(t.order + "HHII", entry)
        if typ != 7 or offset + count > len(t.data):
            raise FormatError("Invalid legacy MakerNote")
        note = t.data[offset:offset + count]
        if not note.startswith(b"Apple iOS\0\0\x01"):
            continue
        order = ">" if note[12:14] == b"MM" else "<" if note[12:14] == b"II" else None
        if order is None or len(note) < 16:
            raise FormatError("Invalid Apple MakerNote byte order")
        n = struct.unpack_from(order + "H", note, 14)[0]
        if 16 + n * 12 + 4 > len(note):
            raise FormatError("Truncated Apple MakerNote")
        scalars = {}
        for i in range(n):
            at = 16 + i * 12
            tag, field_type, length = struct.unpack_from(order + "HHI", note, at)
            if tag not in (33, 48) or length != 1:
                continue
            if field_type == 11:
                value = struct.unpack_from(order + "f", note, at + 8)[0]
            elif field_type in (5, 10, 12):
                position = struct.unpack_from(order + "I", note, at + 8)[0]
                if position < 16 + n * 12 + 4 or position + 8 > len(note):
                    raise FormatError("Invalid Apple headroom scalar offset")
                if field_type == 12:
                    value = struct.unpack_from(order + "d", note, position)[0]
                else:
                    num, den = struct.unpack_from(order + ("ii" if field_type == 10 else "II"), note, position)
                    if den == 0:
                        raise FormatError("Zero Apple headroom denominator")
                    value = num / den
            else:
                continue
            scalars[tag] = value
        if 33 in scalars and 48 in scalars:
            a, b = scalars[33], scalars[48]
            if not math.isfinite(a) or not math.isfinite(b):
                raise FormatError("Non-finite legacy HDR headroom")
            stops = max((-20 * b + 1.8 if b <= 0.01 else -0.101 * b + 1.601) if a < 1
                        else (-70 * b + 3.0 if b <= 0.01 else -0.303 * b + 2.303), 0)
            if 0 < stops <= 16:
                return {"gain": candidates[0], "stops": stops}, "Existing legacy Apple HDR"
    return None, "Legacy HDR map present, but valid Apple 33/48 headroom unavailable; original retained"
