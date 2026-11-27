//! Platform-independent halves of a system codec backend: which video
//! sample a frame request means, which track transform applies, and which
//! JPEG metadata segments a re-encode carries over. No pixel codec here, so
//! every target compiles and tests this; the Linux backend
//! (`--features linux-native`) only adds the decoders.
use crate::mp4::{self, BoxRef, Movie};
use crate::{fail, fail_as, jpeg, ErrorKind, Result};

/// One of the eight axis-aligned image transforms (the dihedral group D4),
/// as the integer part of a QuickTime/ISO-BMFF matrix:
/// `x' = m00·x + m01·y`, `y' = m10·x + m11·y` (y grows downwards), then
/// translated so every output coordinate is non-negative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Orientation {
    pub m00: i8,
    pub m01: i8,
    pub m10: i8,
    pub m11: i8,
}

impl Orientation {
    pub const IDENTITY: Orientation = Orientation {
        m00: 1,
        m01: 0,
        m10: 0,
        m11: 1,
    };

    fn valid(self) -> bool {
        let row = |a: i8, b: i8| (a == 0) != (b == 0) && a.abs() <= 1 && b.abs() <= 1;
        row(self.m00, self.m01) && row(self.m10, self.m11) && (self.m00 == 0) == (self.m11 == 0)
    }

    /// From a `tkhd` matrix (`a b u c d v x y w`, 16.16 / 2.30 fixed point).
    /// Only exact multiples of 90° with optional mirroring are accepted:
    /// anything else (scaling, shear, arbitrary angles) is refused instead
    /// of approximated.
    pub fn from_matrix(m: &[i32; 9]) -> Result<Orientation> {
        let unit = |v: i32| match v {
            0 => Some(0i8),
            0x10000 => Some(1),
            -0x10000 => Some(-1),
            _ => None,
        };
        let o = match (unit(m[0]), unit(m[1]), unit(m[3]), unit(m[4])) {
            // QuickTime: x' = a·x + c·y, y' = b·x + d·y.
            (Some(a), Some(b), Some(c), Some(d)) => Orientation {
                m00: a,
                m01: c,
                m10: b,
                m11: d,
            },
            _ => {
                return fail_as(
                    ErrorKind::Unsupported,
                    "视频轨变换不是 90° 倍数旋转/镜像，未解码",
                )
            }
        };
        if !o.valid() || m[2] != 0 || m[5] != 0 || m[8] != 0x4000_0000 {
            return fail_as(
                ErrorKind::Unsupported,
                "视频轨变换不是 90° 倍数旋转/镜像，未解码",
            );
        }
        Ok(o)
    }

    /// EXIF/TIFF Orientation (1..=8): the transform that displays the
    /// stored pixels upright.
    pub fn from_exif(value: u16) -> Option<Orientation> {
        let (m00, m01, m10, m11) = match value {
            1 => (1, 0, 0, 1),
            2 => (-1, 0, 0, 1),
            3 => (-1, 0, 0, -1),
            4 => (1, 0, 0, -1),
            5 => (0, 1, 1, 0),
            6 => (0, -1, 1, 0),
            7 => (0, -1, -1, 0),
            8 => (0, 1, -1, 0),
            _ => return None,
        };
        Some(Orientation { m00, m01, m10, m11 })
    }

    pub fn exif(self) -> u16 {
        (1..=8)
            .find(|v| Orientation::from_exif(*v) == Some(self))
            .unwrap_or(1)
    }

    /// `self` followed by `next`.
    pub fn then(self, next: Orientation) -> Orientation {
        Orientation {
            m00: next.m00 * self.m00 + next.m01 * self.m10,
            m01: next.m00 * self.m01 + next.m01 * self.m11,
            m10: next.m10 * self.m00 + next.m11 * self.m10,
            m11: next.m10 * self.m01 + next.m11 * self.m11,
        }
    }

    /// HEIF `irot`: anticlockwise rotation by `quarter_turns` × 90°.
    pub fn heif_rotation(quarter_turns: u8) -> Orientation {
        // One anticlockwise quarter turn is EXIF 8.
        let ccw = Orientation::from_exif(8).unwrap();
        let mut o = Orientation::IDENTITY;
        for _ in 0..quarter_turns % 4 {
            o = o.then(ccw);
        }
        o
    }

    /// HEIF `imir`: axis 0 mirrors about the vertical axis (left↔right),
    /// axis 1 about the horizontal axis (top↔bottom).
    pub fn heif_mirror(axis: u8) -> Orientation {
        Orientation::from_exif(if axis & 1 == 0 { 2 } else { 4 }).unwrap()
    }

    /// The transform that undoes `self` (D4 matrices are orthogonal).
    pub fn inverse(self) -> Orientation {
        Orientation {
            m00: self.m00,
            m01: self.m10,
            m10: self.m01,
            m11: self.m11,
        }
    }

    pub fn swaps_axes(self) -> bool {
        self.m00 == 0
    }

    /// Output size for a `width`×`height` input.
    pub fn size(self, width: usize, height: usize) -> (usize, usize) {
        if self.swaps_axes() {
            (height, width)
        } else {
            (width, height)
        }
    }

    /// Transform packed pixels (`channels` bytes each, rows `stride` apart)
    /// into a tightly packed buffer of the output size.
    pub fn apply(
        self,
        pixels: &[u8],
        width: usize,
        height: usize,
        stride: usize,
        channels: usize,
    ) -> Result<(Vec<u8>, usize, usize)> {
        let row = width
            .checked_mul(channels)
            .filter(|&r| r <= stride)
            .ok_or_else(|| crate::Error::new("像素行跨度无效"))?;
        let need = stride
            .checked_mul(height.saturating_sub(1))
            .and_then(|n| n.checked_add(row))
            .filter(|&n| n <= pixels.len());
        if width == 0 || height == 0 || need.is_none() {
            return fail("像素缓冲区被截断");
        }
        let (ow, oh) = self.size(width, height);
        if self == Orientation::IDENTITY {
            let mut out = Vec::with_capacity(row * height);
            for y in 0..height {
                out.extend_from_slice(&pixels[y * stride..y * stride + row]);
            }
            return Ok((out, ow, oh));
        }
        let mut out = vec![0u8; ow * oh * channels];
        let shift = |a: i8, b: i8, extent: usize| -> i64 {
            if a + b < 0 {
                extent as i64 - 1
            } else {
                0
            }
        };
        let (sx, sy) = (shift(self.m00, self.m01, ow), shift(self.m10, self.m11, oh));
        for y in 0..height {
            for x in 0..width {
                let tx = self.m00 as i64 * x as i64 + self.m01 as i64 * y as i64 + sx;
                let ty = self.m10 as i64 * x as i64 + self.m11 as i64 * y as i64 + sy;
                let from = y * stride + x * channels;
                let to = (ty as usize * ow + tx as usize) * channels;
                out[to..to + channels].copy_from_slice(&pixels[from..from + channels]);
            }
        }
        Ok((out, ow, oh))
    }
}

/// Video codec of a frame plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    H265,
}

/// One compressed sample to feed a decoder, with nanosecond timestamps.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanSample {
    pub range: std::ops::Range<usize>,
    pub pts_ns: i64,
    pub dts_ns: i64,
    pub duration_ns: i64,
    pub sync: bool,
    /// Contains an IDR picture (H.264 type 5, HEVC types 19/20): decoding
    /// can start here without any earlier reference, unlike a CRA or a
    /// recovery-point sync sample.
    pub idr: bool,
}

/// Everything a decoder needs to produce the frame AVFoundation's
/// `AVAssetImageGenerator` returns for (`track`, media time) with zero
/// tolerance: the sample shown at that time, the codec configuration,
/// samples in decode order and the track transform.
#[derive(Clone, Debug)]
pub struct FramePlan {
    pub codec: VideoCodec,
    /// GStreamer `stream-format` (`avc`, `avc3`, `hvc1`, `hev1`).
    pub stream_format: &'static str,
    /// `avcC` / `hvcC` payload.
    pub config: Vec<u8>,
    /// Sample-entry size (before the transform).
    pub width: u32,
    pub height: u32,
    /// All samples of the track, decode order.
    pub samples: Vec<PlanSample>,
    /// Index into `samples` of the frame to return.
    pub target: usize,
    pub orientation: Orientation,
}

impl FramePlan {
    /// Decode window ending at the target, starting at a random-access
    /// point: the IDR (else `stss` sync sample) at or before it, `extra`
    /// further ones back on retries; sample 0 when there is none.
    pub fn window(&self, extra: usize) -> std::ops::Range<usize> {
        let before = &self.samples[..=self.target];
        let points: Vec<usize> = if before.iter().any(|s| s.idr) {
            (0..before.len()).filter(|&i| before[i].idr).collect()
        } else {
            (0..before.len()).filter(|&i| before[i].sync).collect()
        };
        let start = points.len().checked_sub(extra + 1).map_or(0, |i| points[i]);
        start..self.target + 1
    }
    pub fn target_pts_ns(&self) -> i64 {
        self.samples[self.target].pts_ns
    }
}

fn ns(ticks: i128, timescale: u32) -> i64 {
    (ticks * 1_000_000_000 / timescale as i128) as i64
}

fn tkhd_matrix(d: &[u8], trak: &BoxRef) -> Result<[i32; 9]> {
    let tkhd = mp4::find_box(d, trak, b"tkhd")?;
    let p = tkhd.payload;
    if tkhd.end - p < 4 {
        return fail("tkhd 被截断");
    }
    // version 1 has 64-bit times and duration.
    let at =
        p + if d[p] == 1 {
            4 + 8 + 8 + 4 + 4 + 8
        } else {
            4 + 4 + 4 + 4 + 4 + 4
        } + 8
            + 8;
    if tkhd.end < at + 36 {
        return fail("tkhd 被截断");
    }
    let mut m = [0i32; 9];
    for (i, v) in m.iter_mut().enumerate() {
        *v = i32::from_be_bytes(d[at + 4 * i..at + 4 * i + 4].try_into().unwrap());
    }
    Ok(m)
}

fn sample_entry(d: &[u8], trak: &BoxRef) -> Result<(BoxRef, Vec<BoxRef>)> {
    let mut b = trak.clone();
    for t in [b"mdia", b"minf", b"stbl", b"stsd"] {
        b = mp4::find_box(d, &b, t)?;
    }
    if b.end - b.payload < 8 {
        return fail("stsd 被截断");
    }
    let entries = mp4::boxes(d, b.payload + 8, b.end)?;
    let Some(entry) = entries.into_iter().next() else {
        return fail("stsd 没有样本描述");
    };
    if entry.end - entry.payload < 78 {
        return fail("视频样本描述被截断");
    }
    // QuickTime sample entries may end with a 4-byte zero terminator
    // (iPhone `hvc1`), so the child walk tolerates a short tail.
    let children = mp4::boxes_with_mode(
        d,
        entry.payload + 78,
        entry.end,
        mp4::BoxParseMode::Tolerant,
    )?
    .boxes;
    Ok((entry, children))
}

/// Plan the frame `Native::frame_jpeg` returns: video track `track`
/// (0-based among video tracks), media time `seconds`. Mirrors the Apple
/// backend: the media time must fall in exactly one non-empty edit (when
/// the track has edits), and the frame is the sample displayed at it.
pub fn frame_plan(d: &[u8], track: usize, seconds: f64) -> Result<FramePlan> {
    if !seconds.is_finite() || seconds.abs() > i64::MAX as f64 / 1e9 {
        return fail("视频帧时间无效");
    }
    let movie = Movie::parse(d)?;
    let t = movie.video(track)?;
    let ts = t.samples.timescale;
    if ts == 0 || t.samples.samples.is_empty() {
        return fail("视频轨没有可解码的画面");
    }
    // Apple: CMTime(value: round(seconds·1e9), timescale: 1e9).
    let media_ns = (seconds * 1e9).round() as i128;
    let scaled = media_ns * ts as i128; // media time · ts · 1e9 / 1e9
    let giga = 1_000_000_000i128;
    let mts = movie.timescale as i128;
    let end = t
        .samples
        .samples
        .iter()
        .map(|s| s.presentation_time as i128 + s.duration as i128)
        .max()
        .unwrap_or(0);
    let hits = match mp4::edit_list(d, &t.trak)? {
        Some(edits) => edits
            .iter()
            .filter(|(_, media)| *media >= 0)
            .filter(|(dur, media)| {
                let from = *media as i128 * giga * mts;
                let to = (*media as i128 * mts + *dur as i128 * ts as i128) * giga;
                let at = scaled * mts;
                from <= at && at < to
            })
            .count(),
        None => usize::from(0 <= scaled && scaled < end * giga),
    };
    if hits != 1 {
        return fail("视频帧时间无法唯一定位");
    }
    // The frame on screen at that time: the latest presentation start <= t.
    let target = t
        .samples
        .samples
        .iter()
        .enumerate()
        .filter(|(_, s)| (s.presentation_time as i128) * giga <= scaled)
        .max_by_key(|(_, s)| s.presentation_time)
        .map(|(i, _)| i)
        .ok_or_else(|| crate::Error::new("视频帧时间早于首帧"))?;
    let (entry, children) = sample_entry(d, &t.trak)?;
    let (codec, stream_format, config_box) = match &entry.ty {
        b"avc1" => (VideoCodec::H264, "avc", b"avcC"),
        b"avc3" => (VideoCodec::H264, "avc3", b"avcC"),
        b"hvc1" => (VideoCodec::H265, "hvc1", b"hvcC"),
        b"hev1" => (VideoCodec::H265, "hev1", b"hvcC"),
        other => {
            return fail_as(
                ErrorKind::Unsupported,
                format!(
                    "视频编码 {} 不受支持，未解码",
                    String::from_utf8_lossy(other)
                ),
            )
        }
    };
    let Some(config) = children.iter().find(|b| &b.ty == config_box) else {
        return fail("视频样本描述缺少解码配置");
    };
    let config_bytes = &d[config.payload..config.end];
    let length_size = match codec {
        VideoCodec::H264 => config_bytes.get(4).map(|b| (b & 3) as usize + 1),
        VideoCodec::H265 => config_bytes.get(21).map(|b| (b & 3) as usize + 1),
    }
    .ok_or_else(|| crate::Error::new("视频解码配置被截断"))?;
    let width = u16::from_be_bytes([d[entry.payload + 24], d[entry.payload + 25]]) as u32;
    let height = u16::from_be_bytes([d[entry.payload + 26], d[entry.payload + 27]]) as u32;
    let samples = t
        .samples
        .samples
        .iter()
        .map(|s| {
            let range = s.offset..s.offset.saturating_add(s.size);
            if range.end > d.len() {
                return fail("视频采样超出文件范围");
            }
            Ok(PlanSample {
                range,
                pts_ns: ns(s.presentation_time as i128, ts),
                dts_ns: ns(s.decode_time as i128, ts),
                duration_ns: ns(s.duration as i128, ts),
                sync: s.sync,
                idr: has_idr(&d[s.offset..s.offset + s.size], length_size, codec),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(FramePlan {
        codec,
        stream_format,
        config: d[config.payload..config.end].to_vec(),
        width,
        height,
        samples,
        target,
        orientation: Orientation::from_matrix(&tkhd_matrix(d, &t.trak)?)?,
    })
}

/// Whether a length-prefixed access unit contains an IDR picture.
pub fn has_idr(sample: &[u8], length_size: usize, codec: VideoCodec) -> bool {
    let mut p = 0usize;
    while p + length_size < sample.len() {
        let n = sample[p..p + length_size]
            .iter()
            .fold(0usize, |a, b| (a << 8) | *b as usize);
        p += length_size;
        if n == 0 || n > sample.len() - p {
            return false;
        }
        let idr = match codec {
            VideoCodec::H264 => sample[p] & 0x1F == 5,
            VideoCodec::H265 => matches!((sample[p] >> 1) & 0x3F, 19 | 20),
        };
        if idr {
            return true;
        }
        p += n;
    }
    false
}

/// Video tracks' transform, by index among video tracks (for tests/UI).
pub fn video_orientation(d: &[u8], track: usize) -> Result<Orientation> {
    let movie = Movie::parse(d)?;
    Orientation::from_matrix(&tkhd_matrix(d, &movie.video(track)?.trak)?)
}

const ICC: &[u8] = b"ICC_PROFILE\0";
const IPTC: &[u8] = b"Photoshop 3.0\0";

fn header_segments(src: &[u8]) -> Result<Vec<jpeg::Segment>> {
    let segs = jpeg::segments(src)?;
    let sos = segs
        .iter()
        .position(|s| s.marker == 0xDA)
        .unwrap_or(segs.len());
    Ok(segs.into_iter().take(sos).collect())
}

/// The source segments an ImageIO re-encode with the source's properties
/// keeps: EXIF (APP1), ICC (APP2 chunks) and IPTC (APP13), in file order.
/// XMP, MPF and vendor segments are not carried, like ImageIO.
pub fn carried_segments(src: &[u8]) -> Result<Vec<Vec<u8>>> {
    Ok(header_segments(src)?
        .into_iter()
        .filter(|s| {
            let p = s.payload(src);
            (s.marker == 0xE1 && p.starts_with(jpeg::EXIF))
                || (s.marker == 0xE2 && p.starts_with(ICC))
                || (s.marker == 0xED && p.starts_with(IPTC))
        })
        .map(|s| src[s.start..s.end].to_vec())
        .collect())
}

/// The embedded ICC profile (APP2 `ICC_PROFILE` chunks, reassembled in
/// sequence order). Inconsistent chunk numbering is ignored, like most
/// decoders: the image is then treated as untagged.
pub fn jpeg_icc(src: &[u8]) -> Result<Option<Vec<u8>>> {
    let mut chunks: Vec<(u8, u8, &[u8])> = header_segments(src)?
        .iter()
        .map(|s| s.payload(src))
        .filter(|p| p.starts_with(ICC) && p.len() > ICC.len() + 2)
        .map(|p| (p[ICC.len()], p[ICC.len() + 1], &p[ICC.len() + 2..]))
        .collect();
    if chunks.is_empty() {
        return Ok(None);
    }
    chunks.sort_by_key(|c| c.0);
    let total = chunks[0].1 as usize;
    let consistent = chunks.len() == total
        && chunks
            .iter()
            .enumerate()
            .all(|(i, c)| c.0 as usize == i + 1 && c.1 as usize == total);
    if !consistent {
        return Ok(None);
    }
    Ok(Some(
        chunks.iter().flat_map(|c| c.2.iter().copied()).collect(),
    ))
}

/// APP2 segments carrying `icc`.
pub fn icc_segments(icc: &[u8]) -> Result<Vec<Vec<u8>>> {
    const MAX: usize = 65535 - 2 - 14;
    let parts: Vec<&[u8]> = icc.chunks(MAX).collect();
    if parts.is_empty() || parts.len() > 255 {
        return fail_as(ErrorKind::Unsupported, "ICC 色彩描述过大");
    }
    parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            let mut payload = ICC.to_vec();
            payload.extend([(i + 1) as u8, parts.len() as u8]);
            payload.extend_from_slice(part);
            jpeg::app(0xE2, &payload)
        })
        .collect()
}

/// Insert whole segments right after SOI and the encoder's JFIF APP0.
pub fn insert_segments(encoded: &[u8], segments: &[Vec<u8>]) -> Result<Vec<u8>> {
    if !encoded.starts_with(&[0xFF, 0xD8]) {
        return fail("JPEG 编码结果无效");
    }
    let mut at = 2;
    if encoded.len() > 6 && encoded[2..4] == [0xFF, 0xE0] {
        at = 4 + u16::from_be_bytes([encoded[4], encoded[5]]) as usize;
        if at > encoded.len() {
            return fail("JPEG 编码结果无效");
        }
    }
    let mut out = Vec::with_capacity(encoded.len() + segments.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(&encoded[..at]);
    for s in segments {
        out.extend_from_slice(s);
    }
    out.extend_from_slice(&encoded[at..]);
    Ok(out)
}

/// A HEIF `Exif` item (4-byte TIFF header offset, then data) as a JPEG
/// APP1 segment. None for malformed or oversized items.
pub fn heif_exif_segment(item: &[u8]) -> Option<Vec<u8>> {
    let skip = u32::from_be_bytes(item.get(..4)?.try_into().ok()?) as usize;
    let tiff = item.get(4usize.checked_add(skip)?..)?;
    if !(tiff.starts_with(b"II*\0") || tiff.starts_with(b"MM\0*")) {
        return None;
    }
    let mut payload = jpeg::EXIF.to_vec();
    payload.extend_from_slice(tiff);
    jpeg::app(0xE1, &payload).ok()
}

/// Rewrite IFD0 Orientation in an APP1 EXIF segment in place. Returns
/// false when the tag is absent or not a single SHORT.
pub fn set_exif_orientation(segment: &mut [u8], value: u16) -> bool {
    let tiff_at = 4 + jpeg::EXIF.len();
    if segment.len() < tiff_at + 8 {
        return false;
    }
    let tiff = &mut segment[tiff_at..];
    let little = tiff.starts_with(b"II");
    let u16_at = |t: &[u8], p: usize| -> Option<u16> {
        let b: [u8; 2] = t.get(p..p + 2)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |t: &[u8], p: usize| -> Option<u32> {
        let b: [u8; 4] = t.get(p..p + 4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let Some(ifd) = u32_at(tiff, 4) else {
        return false;
    };
    let ifd = ifd as usize;
    let Some(n) = u16_at(tiff, ifd) else {
        return false;
    };
    for i in 0..n as usize {
        let e = ifd + 2 + 12 * i;
        if u16_at(tiff, e) == Some(0x0112) {
            if u16_at(tiff, e + 2) != Some(3) || u32_at(tiff, e + 4) != Some(1) {
                return false;
            }
            let bytes = if little {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            };
            if e + 10 > tiff.len() {
                return false;
            }
            tiff[e + 8..e + 10].copy_from_slice(&bytes);
            return true;
        }
    }
    false
}

/// A minimal big-endian EXIF APP1 segment holding only IFD0 Orientation.
pub fn exif_orientation_segment(value: u16) -> Vec<u8> {
    let mut payload = jpeg::EXIF.to_vec();
    payload.extend(b"MM\0*\0\0\0\x08\0\x01");
    payload.extend(0x0112u16.to_be_bytes());
    payload.extend(3u16.to_be_bytes());
    payload.extend(1u32.to_be_bytes());
    payload.extend(value.to_be_bytes());
    payload.extend([0, 0, 0, 0, 0, 0]);
    jpeg::app(0xE1, &payload).expect("small segment")
}

/// Transform of the HEIF primary item from its `irot`/`imir` properties,
/// in `ipma` order. None when it has neither.
pub fn heif_primary_orientation(d: &[u8]) -> Result<Option<Orientation>> {
    let layout = crate::heif::layout_lenient(d)?;
    let Some(pitm) = crate::apple_hdr::child(&layout.children, b"pitm") else {
        return Ok(None);
    };
    if pitm.end - pitm.payload < 6 {
        return Ok(None);
    }
    let primary = if d[pitm.payload] == 0 {
        crate::binary::be(d, pitm.payload + 4, 2)?
    } else {
        crate::binary::be(d, pitm.payload + 4, 4)?
    };
    heif_item_orientation(d, primary)
}

/// Transform of HEIF item `item` from its `irot`/`imir` properties.
pub fn heif_item_orientation(d: &[u8], item: usize) -> Result<Option<Orientation>> {
    let layout = crate::heif::layout_lenient(d)?;
    let Some(iprp) = crate::apple_hdr::child(&layout.children, b"iprp") else {
        return Ok(None);
    };
    let parts = mp4::boxes(d, iprp.payload, iprp.end)?;
    let Some(ipco) = crate::apple_hdr::child(&parts, b"ipco") else {
        return Ok(None);
    };
    let props = mp4::boxes(d, ipco.payload, ipco.end)?;
    let assoc = crate::apple_hdr::associations(d, &parts)?;
    let mut found = None;
    for index in assoc.get(&item).into_iter().flatten() {
        let Some(p) = index.checked_sub(1).and_then(|i| props.get(i)) else {
            continue;
        };
        let step = match (&p.ty, d.get(p.payload)) {
            (b"irot", Some(v)) => Orientation::heif_rotation(v & 3),
            (b"imir", Some(v)) => Orientation::heif_mirror(v & 1),
            _ => continue,
        };
        found = Some(found.unwrap_or(Orientation::IDENTITY).then(step));
    }
    Ok(found)
}

/// Linux `MemTotal` policy: the Apple backend's physical memory / 8, at
/// least 256 MiB. `meminfo` is the text of `/proc/meminfo`.
pub fn budget_from_meminfo(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|l| l.starts_with("MemTotal:"))?;
    let mut parts = line["MemTotal:".len()..].split_whitespace();
    let value: u64 = parts.next()?.parse().ok()?;
    let bytes = match parts.next() {
        Some("kB") | Some("KB") | Some("kb") => value.checked_mul(1024)?,
        None => value,
        _ => return None,
    };
    (bytes > 0).then(|| (bytes / 8).max(256 << 20))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exif_orientations_round_trip_and_compose() {
        for v in 1..=8 {
            let o = Orientation::from_exif(v).unwrap();
            assert!(o.valid());
            assert_eq!(o.exif(), v);
        }
        let cw = Orientation::from_exif(6).unwrap();
        assert_eq!(cw.then(cw).exif(), 3);
        assert_eq!(cw.then(cw).then(cw).exif(), 8);
        assert_eq!(Orientation::heif_rotation(1).exif(), 8);
        assert_eq!(Orientation::heif_rotation(3).exif(), 6);
        assert_eq!(Orientation::heif_rotation(2).exif(), 3);
        assert_eq!(Orientation::heif_mirror(0).exif(), 2);
        assert_eq!(Orientation::heif_mirror(1).exif(), 4);
        // Mirror left-right after a clockwise turn is the transpose.
        assert_eq!(cw.then(Orientation::heif_mirror(0)).exif(), 5);
        assert_eq!(Orientation::heif_mirror(0).then(cw).exif(), 7);
        for v in 1..=8 {
            let o = Orientation::from_exif(v).unwrap();
            assert_eq!(o.then(o.inverse()), Orientation::IDENTITY);
        }
    }

    #[test]
    fn iphone_portrait_matrix_is_clockwise_quarter_turn() {
        // a b u / c d v / x y w of a portrait iPhone recording.
        let m = [0, 0x10000, 0, -0x10000, 0, 0, 1080 << 16, 0, 0x4000_0000];
        assert_eq!(Orientation::from_matrix(&m).unwrap().exif(), 6);
        let identity = [0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x4000_0000];
        assert_eq!(
            Orientation::from_matrix(&identity).unwrap(),
            Orientation::IDENTITY
        );
        let scaled = [0x20000, 0, 0, 0, 0x10000, 0, 0, 0, 0x4000_0000];
        assert_eq!(
            Orientation::from_matrix(&scaled).unwrap_err().kind(),
            ErrorKind::Unsupported
        );
    }

    #[test]
    fn apply_moves_corners() {
        // 3x2 image, one byte per pixel: 0 1 2 / 3 4 5.
        let px = [0u8, 1, 2, 3, 4, 5];
        let run = |v| {
            Orientation::from_exif(v)
                .unwrap()
                .apply(&px, 3, 2, 3, 1)
                .unwrap()
        };
        assert_eq!(run(1), (px.to_vec(), 3, 2));
        assert_eq!(run(2), (vec![2, 1, 0, 5, 4, 3], 3, 2));
        assert_eq!(run(3), (vec![5, 4, 3, 2, 1, 0], 3, 2));
        assert_eq!(run(4), (vec![3, 4, 5, 0, 1, 2], 3, 2));
        // Clockwise: the left column (0,3) becomes the top row, 3 first.
        assert_eq!(run(6), (vec![3, 0, 4, 1, 5, 2], 2, 3));
        assert_eq!(run(8), (vec![2, 5, 1, 4, 0, 3], 2, 3));
        assert_eq!(run(5), (vec![0, 3, 1, 4, 2, 5], 2, 3));
        assert_eq!(run(7), (vec![5, 2, 4, 1, 3, 0], 2, 3));
        // Row padding is dropped; truncated buffers are refused.
        let padded = [0u8, 1, 2, 9, 3, 4, 5, 9];
        assert_eq!(
            Orientation::IDENTITY.apply(&padded, 3, 2, 4, 1).unwrap().0,
            px.to_vec()
        );
        assert!(Orientation::IDENTITY.apply(&px, 3, 3, 3, 1).is_err());
        assert!(Orientation::IDENTITY.apply(&px, 4, 1, 3, 1).is_err());
    }

    fn jpeg_with(segments: &[Vec<u8>]) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        v.extend(jpeg::app(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0").unwrap());
        for s in segments {
            v.extend(s);
        }
        // SOS with an empty header body, then EOI.
        v.extend([0xFF, 0xDA, 0, 2, 0xFF, 0xD9]);
        v
    }

    fn exif_segment(little: bool, orientation: u16) -> Vec<u8> {
        let mut tiff = Vec::new();
        let (w16, w32): (fn(u16) -> [u8; 2], fn(u32) -> [u8; 4]) = if little {
            tiff.extend(b"II*\0");
            (u16::to_le_bytes, u32::to_le_bytes)
        } else {
            tiff.extend(b"MM\0*");
            (u16::to_be_bytes, u32::to_be_bytes)
        };
        tiff.extend(w32(8));
        tiff.extend(w16(1));
        tiff.extend(w16(0x0112));
        tiff.extend(w16(3));
        tiff.extend(w32(1));
        tiff.extend(w16(orientation));
        tiff.extend([0, 0]);
        tiff.extend(w32(0));
        let mut payload = jpeg::EXIF.to_vec();
        payload.extend(tiff);
        jpeg::app(0xE1, &payload).unwrap()
    }

    #[test]
    fn carried_segments_keep_exif_icc_iptc_only() {
        let exif = exif_segment(false, 6);
        let mut xmp = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        xmp.extend(b"<x/>");
        let xmp = jpeg::app(0xE1, &xmp).unwrap();
        let icc: Vec<u8> = (0..70000u32).map(|i| i as u8).collect();
        let icc_segs = icc_segments(&icc).unwrap();
        assert_eq!(icc_segs.len(), 2);
        let iptc = jpeg::app(0xED, b"Photoshop 3.0\0abc").unwrap();
        let vendor = jpeg::app(0xE9, b"vendor").unwrap();
        let mut all = vec![exif.clone(), xmp, vendor];
        all.extend(icc_segs.iter().rev().cloned());
        all.push(iptc.clone());
        let src = jpeg_with(&all);
        let kept = carried_segments(&src).unwrap();
        let mut expected = vec![exif.clone()];
        expected.extend(icc_segs.iter().rev().cloned());
        expected.push(iptc);
        assert_eq!(kept, expected);
        assert_eq!(jpeg_icc(&src).unwrap().unwrap(), icc);
        // A missing chunk makes the profile unusable, not corrupt.
        let partial = jpeg_with(&[icc_segs[0].clone()]);
        assert_eq!(jpeg_icc(&partial).unwrap(), None);
        // Inserted after JFIF, before the scan.
        let plain = jpeg_with(&[]);
        let out = insert_segments(&plain, std::slice::from_ref(&exif)).unwrap();
        assert_eq!(carried_segments(&out).unwrap(), vec![exif]);
        assert_eq!(out[2..4], [0xFF, 0xE0]);
    }

    #[test]
    fn exif_orientation_patch_both_byte_orders() {
        for little in [true, false] {
            let mut seg = exif_segment(little, 6);
            assert!(set_exif_orientation(&mut seg, 1));
            let src = jpeg_with(&[seg]);
            let segs = jpeg::segments(&src).unwrap();
            let app1 = segs.iter().find(|s| s.marker == 0xE1).unwrap();
            let tiff = crate::tiff::Tiff::new(&app1.payload(&src)[6..]).unwrap();
            let (fields, _) = tiff.root().unwrap();
            let o = crate::tiff::find(&fields, 0x0112).unwrap();
            assert_eq!(crate::tiff::u16_at(&o.value, 0, little), 1);
        }
        assert!(!set_exif_orientation(&mut [0u8; 8], 1));
        let mut made = exif_orientation_segment(8);
        assert!(set_exif_orientation(&mut made, 3));
        let tiff = crate::tiff::Tiff::new(&made[10..]).unwrap();
        let (fields, _) = tiff.root().unwrap();
        assert_eq!(
            crate::tiff::u16_at(&crate::tiff::find(&fields, 0x0112).unwrap().value, 0, false),
            3
        );
    }

    #[test]
    fn heif_exif_item_becomes_app1() {
        let mut item = 6u32.to_be_bytes().to_vec();
        item.extend(b"Exif\0\0");
        item.extend(b"MM\0*\0\0\0\x08\0\0\0\0\0\0");
        let seg = heif_exif_segment(&item).unwrap();
        assert_eq!(&seg[..2], &[0xFF, 0xE1]);
        assert_eq!(&seg[4..10], jpeg::EXIF);
        assert_eq!(&seg[10..14], b"MM\0*");
        assert!(heif_exif_segment(&[0, 0, 0, 99]).is_none());
        assert!(heif_exif_segment(b"\0\0\0\0junk").is_none());
    }

    #[test]
    fn idr_detection_and_decode_windows() {
        // 4-byte lengths: an SEI then an IDR slice (H.264), a CRA (HEVC).
        let h264 = [0, 0, 0, 2, 0x06, 0x05, 0, 0, 0, 2, 0x65, 0x88];
        assert!(has_idr(&h264, 4, VideoCodec::H264));
        assert!(!has_idr(&[0, 0, 0, 2, 0x41, 0x9a], 4, VideoCodec::H264));
        assert!(has_idr(&[0, 0, 0, 2, 20 << 1, 1], 4, VideoCodec::H265));
        assert!(!has_idr(&[0, 0, 0, 2, 21 << 1, 1], 4, VideoCodec::H265));
        assert!(!has_idr(&[0, 0, 0, 9, 0x65], 4, VideoCodec::H264));
        let sample = |sync, idr| PlanSample {
            range: 0..0,
            pts_ns: 0,
            dts_ns: 0,
            duration_ns: 0,
            sync,
            idr,
        };
        let mut plan = FramePlan {
            codec: VideoCodec::H265,
            stream_format: "hvc1",
            config: vec![],
            width: 1,
            height: 1,
            // IDR, CRA (sync), P, P, IDR, P
            samples: vec![
                sample(true, true),
                sample(true, false),
                sample(false, false),
                sample(false, false),
                sample(true, true),
                sample(false, false),
            ],
            target: 3,
            orientation: Orientation::IDENTITY,
        };
        // A CRA flagged in stss is skipped in favour of the IDR.
        assert_eq!(plan.window(0), 0..4);
        plan.target = 5;
        assert_eq!(plan.window(0), 4..6);
        assert_eq!(plan.window(1), 0..6);
        assert_eq!(plan.window(5), 0..6);
        // No IDR at all: stss sync samples.
        for s in &mut plan.samples {
            s.idr = false;
        }
        assert_eq!(plan.window(0), 4..6);
        assert_eq!(plan.window(1), 1..6);
    }

    #[test]
    fn meminfo_budget_matches_apple_policy() {
        let text = "MemTotal:       16303912 kB\nMemFree:  1 kB\n";
        assert_eq!(budget_from_meminfo(text), Some(16303912 * 1024 / 8));
        assert_eq!(budget_from_meminfo("MemTotal: 1024 kB\n"), Some(256 << 20));
        assert_eq!(budget_from_meminfo("MemFree: 1 kB\n"), None);
        assert_eq!(budget_from_meminfo("MemTotal: 0 kB\n"), None);
        assert_eq!(budget_from_meminfo("MemTotal: x kB\n"), None);
    }
}
