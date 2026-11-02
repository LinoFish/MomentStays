//! Motion Photo -> Apple Live Photo pair: a still (JPEG or HEIC) and a MOV
//! that share one ContentIdentifier, with the still-image-time marker on the
//! cover frame. The reverse of [`crate::live2motion`].
//!
//! Mirrors the former Swift engine (`PhotoFormat.liveStill` +
//! `MotionLive.movie`), without re-encoding anything:
//! - still: static-only, motion XMP removed, ContentIdentifier written to the
//!   Apple MakerNote (tag 17); coding data must hash the same as the source;
//! - movie: sample-copy remux of the primary video track plus audio, the
//!   content identifier in `moov/udta`, and a still-image-time track;
//! - cover: the source cover time is matched to a real video frame on the
//!   media timeline and on the presentation (edit-list) timeline; when the
//!   two disagree the conversion is refused instead of guessing, and the
//!   caller can pick one with [`TimeDomain`]. A Huawei frame number is used
//!   as is. There is no midpoint fallback for files without a cover.
//!
//! Not covered (needs pixel work the core does not do): converting a legacy
//! Apple HDR gain map. Ultra HDR / gain-map bytes are kept unchanged.

use crate::cover::sorted_presentation;
use crate::format::{self, MotionPhoto};
use crate::model::Cover;
use crate::mp4::{self, Movie};
use crate::{
    fail, fail_as, heif, heifwrite, jpeg, live2motion, pymotion, remux, tiff, ErrorKind, Result,
};

/// Which timeline the source cover time is measured on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeDomain {
    /// Try both and require them to agree on one frame (Swift default).
    Auto,
    /// Raw sample presentation times (edit list ignored).
    Media,
    /// Movie timeline after the video track's edit list.
    Presentation,
}

/// How the cover was placed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverMatch {
    /// What the source file recorded.
    pub source: Cover,
    /// "media", "presentation-edit-list", "presentation-identity" or "frame".
    pub domain: &'static str,
    /// Presentation-sorted frame index of the cover in the video track.
    pub frame: usize,
    /// That frame's raw presentation time, microseconds.
    pub frame_time_us: i64,
    /// Where the still-image-time marker sits in the output movie, microseconds.
    pub marker_us: i64,
    /// Distance between the recorded time and the frame, microseconds
    /// (None for a frame-number cover).
    pub error_us: Option<i64>,
    /// The recorded time mapped onto the track's media timeline,
    /// microseconds (through the edit list for "presentation-edit-list";
    /// None for a frame-number cover).
    pub mapped_us: Option<i64>,
}

/// A verified Live Photo pair.
#[derive(Clone, Debug)]
pub struct LivePhoto {
    pub still: Vec<u8>,
    /// True when `still` is HEIC (write it as `.heic`), else JPEG.
    pub heic: bool,
    pub movie: Vec<u8>,
    pub identifier: String,
    pub cover: CoverMatch,
}

/// A random UUID v4 in Apple's uppercase form, for a new ContentIdentifier.
///
/// The identifier only has to be unique, not secret, so std's OS-seeded
/// SipHash keys (`RandomState`) mixed with time, pid and a process counter
/// are enough; this avoids an extra RNG dependency in the app builds.
pub fn new_content_identifier() -> Result<String> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut b = [0u8; 16];
    for i in 0..2 {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(now);
        h.write_u32(std::process::id());
        h.write_u64(n);
        h.write_usize(i);
        b[i * 8..i * 8 + 8].copy_from_slice(&h.finish().to_be_bytes());
    }
    b[6] = (b[6] & 0x0F) | 0x40;
    b[8] = (b[8] & 0x3F) | 0x80;
    let h = crate::hex(&b).to_uppercase();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    ))
}

/// Convert a parsed Motion Photo (`data` is the whole file) into a Live Photo
/// pair. `video_index` picks the video track among video tracks (0 = the
/// primary motion track; Google files may carry a secondary one).
pub fn from_motion(
    photo: &MotionPhoto,
    data: &[u8],
    identifier: &str,
    domain: TimeDomain,
    video_index: usize,
) -> Result<LivePhoto> {
    if identifier.is_empty() {
        return fail_as(ErrorKind::InvalidArgument, "内容标识不能为空");
    }
    let source_cover = crate::model::cover_of(photo)?;
    let video = photo.video(data);
    // Each MP4 is parsed once: the source here, the output inside `remux_mov`.
    let source = Movie::parse(video)?;
    let cover = resolve_cover_in(video, &source, source_cover, domain, video_index)?;

    // Movie: remux (verified sample by sample inside), identifier, marker.
    let (movie, parsed) = remux::remux_mov(
        video,
        &source,
        video_index,
        Some(identifier),
        Some(cover.marker_us),
    )?;
    if remux::read_content_identifier(&movie)?.as_deref() != Some(identifier) {
        return fail_as(
            ErrorKind::VerifyFailed,
            "实况视频读回校验失败：内容标识不同",
        );
    }
    if live2motion::mett_cover_time_in(&movie, &parsed)? != Some(cover.marker_us) {
        return fail_as(
            ErrorKind::VerifyFailed,
            "实况视频读回校验失败：封面标记时间不同",
        );
    }
    // The marker must land on the same frame in the output movie timeline.
    let back = resolve_cover_in(
        &movie,
        &parsed,
        Cover::Time(cover.marker_us),
        TimeDomain::Presentation,
        0,
    )?;
    if back.frame != cover.frame {
        return fail_as(
            ErrorKind::VerifyFailed,
            "实况视频读回校验失败：封面标记不在原封面帧上",
        );
    }

    // Still: same coding data, identifier in the MakerNote, no motion left.
    let heic = heif::matches(&photo.still);
    let still = if heic {
        heifwrite::live_still_heic(&photo.still, identifier)?
    } else {
        pymotion::live_still_with_identifier(&photo.still, identifier)?
    };
    if format::coding_hash(&still)? != format::coding_hash(&photo.still)? {
        return fail_as(
            ErrorKind::VerifyFailed,
            "实况静态图读回校验失败：图像编码数据不同",
        );
    }
    if still_content_identifier(&still)?.as_deref() != Some(identifier) {
        return fail_as(
            ErrorKind::VerifyFailed,
            "实况静态图读回校验失败：内容标识不同",
        );
    }
    if format::parse(&still).is_ok() {
        return fail_as(
            ErrorKind::VerifyFailed,
            "实况静态图读回校验失败：仍含动态照片数据",
        );
    }
    Ok(LivePhoto {
        still,
        heic,
        movie,
        identifier: identifier.to_string(),
        cover,
    })
}

fn round_div(n: i128, d: i128) -> i128 {
    if n >= 0 {
        (n + d / 2) / d
    } else {
        -((-n + d / 2) / d)
    }
}

/// Match the source cover to a frame of video track `video_index` and place
/// the marker in movie time. All arithmetic is exact (i128 rationals).
pub fn resolve_cover(
    video: &[u8],
    cover: Cover,
    domain: TimeDomain,
    video_index: usize,
) -> Result<CoverMatch> {
    resolve_cover_in(video, &Movie::parse(video)?, cover, domain, video_index)
}

/// [`resolve_cover`] on an already parsed movie.
pub fn resolve_cover_in(
    video: &[u8],
    movie: &Movie,
    cover: Cover,
    domain: TimeDomain,
    video_index: usize,
) -> Result<CoverMatch> {
    let t = movie.video(video_index)?;
    let (track, trak, movie_ts) = (&t.samples, &t.trak, movie.timescale);
    let ts = track.timescale as i128;
    if ts <= 0 {
        return fail("视频时间尺度无效");
    }
    let mts = movie_ts as i128;
    let sorted = sorted_presentation(track);
    if sorted.is_empty() {
        return fail_as(ErrorKind::Cover, "视频没有样本，无法确定封面");
    }
    if sorted.iter().any(|(t, _)| *t < 0) {
        return fail_as(ErrorKind::Unsupported, "视频含负时间样本，暂不无损转换");
    }
    let edits = mp4::edit_list(video, trak)?;
    // Duration of sorted frame i in track ticks (gap to the next frame, or
    // the last stts delta), like `cover.rs` and Swift `durationAt`.
    let duration = |i: usize| -> Option<i128> {
        let d = if i + 1 < sorted.len() {
            (sorted[i + 1].0 - sorted[i].0) as i128
        } else {
            track.samples.last()?.duration as i128
        };
        (d > 0).then_some(d)
    };

    let (frame, label, error_us, mapped_us) = match cover {
        Cover::Frame(f) => {
            if f < 0 || f as usize >= sorted.len() {
                return fail_as(ErrorKind::Cover, "封面帧号超出视频样本范围");
            }
            (f as usize, "frame", None, None)
        }
        Cover::Time(us) => {
            if us < 0 {
                return fail_as(ErrorKind::Cover, "封面时间为负，无法换算");
            }
            // ~31 years; keeps us * timescale * movie timescale (each < 2^32)
            // far inside i128.
            if us > 1_000_000_000_000_000 {
                return fail_as(ErrorKind::Cover, "封面时间超出范围");
            }
            let us = us as i128;
            // Candidates in track ticks scaled by S = 1e6 * movie_ts, so the
            // edit-list mapping stays an integer.
            let s = 1_000_000 * mts;
            let mut candidates: Vec<(i128, &'static str)> = Vec::new();
            if domain != TimeDomain::Presentation {
                candidates.push((us * ts * mts, "media"));
            }
            if domain != TimeDomain::Media {
                match &edits {
                    None => candidates.push((us * ts * mts, "presentation-identity")),
                    Some(entries) => {
                        let target = us * mts; // movie ticks * 1e6
                        let mut cursor: i128 = 0; // movie ticks
                        for (seg_dur, media_start) in entries {
                            let (seg_dur, media_start) = (*seg_dur as i128, *media_start as i128);
                            if media_start >= 0
                                && cursor * 1_000_000 <= target
                                && target < (cursor + seg_dur) * 1_000_000
                            {
                                let c = media_start * s + (target - cursor * 1_000_000) * ts;
                                candidates.push((c, "presentation-edit-list"));
                            }
                            cursor += seg_dur;
                            if cursor > i64::MAX as i128 {
                                return fail("视频编辑列表时长超出范围");
                            }
                            if cursor > i64::MAX as i128 {
                                return fail("视频编辑列表时长超出范围");
                            }
                        }
                    }
                }
            }
            let mut matches: Vec<(usize, &'static str, i128, i128)> = Vec::new();
            for (c, label) in candidates {
                let mut best: Option<(i128, usize)> = None;
                let mut tie = false;
                for (i, (pts, _)) in sorted.iter().enumerate() {
                    let diff = (*pts as i128 * s - c).abs();
                    match best {
                        Some((b, _)) if diff > b => {}
                        Some((b, _)) if diff == b => tie = true,
                        _ => {
                            best = Some((diff, i));
                            tie = false;
                        }
                    }
                }
                let Some((diff, i)) = best else { continue };
                let Some(dur) = duration(i) else { continue };
                // Within half a frame plus 2 us, like Swift and `cover.rs`.
                if tie || 2 * diff > dur * s + 4 * ts * mts {
                    continue;
                }
                if !matches.iter().any(|m| m.0 == i) {
                    matches.push((i, label, diff, c));
                }
            }
            match matches.len() {
                1 => {
                    let (i, label, diff, c) = matches[0];
                    // c is track ticks * 1e6 * movie_ts: microseconds = c / (ts * mts).
                    (
                        i,
                        label,
                        Some(round_div(diff, ts * mts) as i64),
                        Some(round_div(c, ts * mts) as i64),
                    )
                }
                0 => return fail_as(ErrorKind::Cover, "封面时间不在视频媒体时间线或编辑列表范围内；不自动改用其他帧"),
                _ => {
                    return fail_as(ErrorKind::Cover, "封面时间在媒体时间线与显示时间线上对应不同帧，存在歧义；请用 --time-domain media 或 presentation 指定",
                    )
                }
            }
        }
    };

    // Marker: the frame's presentation in movie time, through the edit list.
    let pts = sorted[frame].0 as i128;
    let marker_us = match &edits {
        None => round_div(pts * 1_000_000, ts),
        Some(entries) => {
            let mut cursor: i128 = 0;
            let mut found = None;
            for (seg_dur, media_start) in entries {
                let (seg_dur, media_start) = (*seg_dur as i128, *media_start as i128);
                if media_start >= 0
                    && media_start <= pts
                    && (pts - media_start) * mts < seg_dur * ts
                {
                    let n = cursor * 1_000_000 * ts + (pts - media_start) * 1_000_000 * mts;
                    found = Some(round_div(n, mts * ts));
                    break;
                }
                cursor += seg_dur;
                if cursor > i64::MAX as i128 {
                    return fail("视频编辑列表时长超出范围");
                }
            }
            match found {
                Some(m) => m,
                None => {
                    return fail_as(
                        ErrorKind::Cover,
                        "封面帧不在视频编辑列表内，实况照片中不可见",
                    )
                }
            }
        }
    };
    let marker_us = i64::try_from(marker_us)
        .map_err(|_| crate::Error::with_kind(ErrorKind::Cover, "封面时间超出范围"))?;
    Ok(CoverMatch {
        source: cover,
        domain: label,
        frame,
        frame_time_us: round_div(pts * 1_000_000, ts) as i64,
        marker_us,
        error_us,
        mapped_us,
    })
}

/// The Apple ContentIdentifier (MakerNote tag 17) of a JPEG or HEIC still,
/// if it has one.
pub fn still_content_identifier(still: &[u8]) -> Result<Option<String>> {
    // Both paths end in "Exif\0\0" + TIFF.
    let exif = if heif::matches(still) {
        match heif::exif(still) {
            Ok(e) => e,
            Err(_) => return Ok(None),
        }
    } else {
        let segs = jpeg::segments(still)?;
        match segs
            .iter()
            .find(|s| s.marker == 0xE1 && s.payload(still).starts_with(jpeg::EXIF))
        {
            Some(s) => s.payload(still).to_vec(),
            None => return Ok(None),
        }
    };
    let t = tiff::Tiff::new(&exif[jpeg::EXIF.len()..])?;
    let (root, _) = t.root()?;
    let Some(ptr) = tiff::find(&root, 0x8769) else {
        return Ok(None);
    };
    let (fields, _) = t.directory(ptr.offset(t.little)?)?;
    let Some(note) = tiff::find(&fields, 0x927C) else {
        return Ok(None);
    };
    let at = note.offset(t.little)?;
    let Some(note) = t.data.get(at..at.saturating_add(note.count)) else {
        return fail("MakerNote 越界");
    };
    if !note.starts_with(b"Apple iOS\0\x00\x01") || note.len() < 16 {
        return Ok(None);
    }
    let little = match &note[12..14] {
        b"II" => true,
        b"MM" => false,
        _ => return fail("Apple MakerNote 字节序无效"),
    };
    let n = tiff::u16_at(note, 14, little) as usize;
    for i in 0..n {
        let p = 16 + i * 12;
        let Some(e) = note.get(p..p + 12) else {
            return fail("Apple MakerNote 被截断");
        };
        if tiff::u16_at(e, 0, little) != 17 {
            continue;
        }
        let count = tiff::u32_at(e, 4, little) as usize;
        let raw = if count <= 4 {
            &e[8..8 + count]
        } else {
            let off = tiff::u32_at(e, 8, little) as usize;
            match note.get(off..off.saturating_add(count)) {
                Some(r) => r,
                None => return fail("Apple MakerNote 内容标识越界"),
            }
        };
        let text = String::from_utf8_lossy(raw);
        return Ok(Some(text.trim_end_matches('\0').to_string()));
    }
    Ok(None)
}
