//! momentstays-core: a cross-platform, byte-level port of the MomentStays
//! Motion Photo / Live Photo engine (originally ported from the Swift engine).
//!
//! Scope: parse and validate Android Motion Photos (Google/Ultra HDR JPEG,
//! Huawei/Honor LIVE_, Samsung SEF JPEG/HEIC, OPPO, vivo), split the still and
//! the embedded MP4, and compute the same coding hashes the app verifies with.
//! `model` adds the reverse direction: a neutral model built from any parsed
//! vendor file, written back out for Google / Samsung / Huawei / OPPO / vivo
//! (JPEG path; video bytes are copied, never re-encoded).
//! No pixel decode or encode. Error texts are stable (kept from the Swift engine).

// Style lints that would force churn in byte-layout code without improving it.
#![allow(
    clippy::type_complexity,
    clippy::single_range_in_vec_init,
    clippy::doc_lazy_continuation
)]

pub mod apple_hdr;
pub mod binary;
pub mod capi;
pub mod cover;
pub mod engine;
pub mod ffi;
pub mod files;
pub mod format;
pub mod hdr;
pub mod heif;
pub mod heifwrite;
pub mod jpeg;
pub mod live;
pub mod live2motion;
pub mod model;
pub mod mp4;
pub mod output;
pub mod pyjpeg;
pub mod pymotion;
pub mod pyxml;
pub mod recovery;
pub mod remux;
pub mod scan;
pub mod tiff;
pub mod vendor;
pub mod xmp;

use std::fmt;

/// What went wrong, for callers that branch on it (host apps, the C ABI,
/// localization). The message text stays the human-readable detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The file is not a Motion Photo this crate recognizes.
    NotMotionPhoto,
    /// The data is damaged or inconsistent (truncated, out of range, ...).
    Malformed,
    /// Valid data, but a layout, version, size or target this crate does
    /// not handle (yet), refused rather than guessed.
    Unsupported,
    /// The cover cannot be determined or placed without guessing
    /// (missing, ambiguous, outside the video).
    Cover,
    /// An output failed its read-back check; nothing was published.
    VerifyFailed,
    /// File system error.
    Io,
    /// A caller-supplied argument is invalid.
    InvalidArgument,
}

impl ErrorKind {
    /// Stable snake_case code, used in JSON output (`error_kind`).
    pub fn code(self) -> &'static str {
        match self {
            ErrorKind::NotMotionPhoto => "not_motion_photo",
            ErrorKind::Malformed => "malformed",
            ErrorKind::Unsupported => "unsupported",
            ErrorKind::Cover => "cover",
            ErrorKind::VerifyFailed => "verify_failed",
            ErrorKind::Io => "io",
            ErrorKind::InvalidArgument => "invalid_argument",
        }
    }
}

/// Error message (`.0`, also `Display`) plus its [`ErrorKind`].
#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String, ErrorKind);
impl Error {
    /// A [`ErrorKind::Malformed`] error.
    pub fn new(s: impl Into<String>) -> Self {
        Error(s.into(), ErrorKind::Malformed)
    }
    pub fn with_kind(kind: ErrorKind, s: impl Into<String>) -> Self {
        Error(s.into(), kind)
    }
    pub fn kind(&self) -> ErrorKind {
        self.1
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
/// A [`ErrorKind::Malformed`] failure.
pub(crate) fn fail<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::new(msg))
}
pub(crate) fn fail_as<T>(kind: ErrorKind, msg: impl Into<String>) -> Result<T> {
    Err(Error::with_kind(kind, msg))
}

pub fn hex(b: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push(D[(x >> 4) as usize] as char);
        s.push(D[(x & 15) as usize] as char);
    }
    s
}
pub fn sha256_hex(b: &[u8]) -> String {
    use sha2::Digest;
    hex(&sha2::Sha256::digest(b))
}

pub mod bench;
pub use format::{parse, MotionPhoto};

/// JSON summary used by the CLI and the parity checks. Keys shared with
/// tests/NativeDriver.swift `format` (timestamp_us, ultra, coding_hash) keep
/// their names so outputs can be diffed directly.
pub fn inspect(data: &[u8]) -> serde_json::Value {
    inspect_inner(data, true)
}

/// Metadata-only inspect without the video SHA-256 (perf item 1):
/// identical to `inspect` except `video_sha256` is null, so large
/// videos skip the full hash pass. `inspect` itself is unchanged;
/// callers that need the video hash keep calling it.
pub fn inspect_light(data: &[u8]) -> serde_json::Value {
    inspect_inner(data, false)
}

fn inspect_inner(data: &[u8], with_video_hash: bool) -> serde_json::Value {
    use serde_json::json;
    match parse(data) {
        Ok(p) => {
            let video = p.video(data);
            let times = mp4::image_times(video).ok().flatten();
            // One shared JPEG segment walk for the still metadata
            // (perf item 3); HEIC stills keep the original paths.
            let still_segs = if heif::matches(&p.still) {
                None
            } else {
                jpeg::segments(&p.still).ok()
            };
            let (make, model) = if heif::matches(&p.still) {
                heif::exif(&p.still)
                    .ok()
                    .and_then(|e| jpeg::app(225, &e).ok())
                    .map(|a| {
                        let mut j = vec![255, 216];
                        j.extend(a);
                        j.extend([255, 217]);
                        vendor::make_model(&j)
                    })
                    .unwrap_or_default()
            } else {
                match &still_segs {
                    Some(s) => vendor::make_model_from_segments(&p.still, s),
                    None => vendor::make_model(&p.still),
                }
            };
            // Single-pass dual still hash (perf item 4): one read feeds
            // both the coding hash and the full SHA-256.
            let (coding_hash, still_sha256) = match &still_segs {
                Some(s) => {
                    let (c, f) = format::coding_and_full_hash_from_segments(&p.still, s);
                    (Some(c), f)
                }
                None => (format::coding_hash(&p.still).ok(), sha256_hex(&p.still)),
            };
            let video_sha256 = if with_video_hash {
                serde_json::Value::String(sha256_hex(video))
            } else {
                serde_json::Value::Null
            };
            json!({
                "ok": true,
                "format": p.format,
                "timestamp_us": p.timestamp,
                "timestamp_tag": p.timestamp_tag,
                "cover_frame": p.cover_frame,
                "ultra": p.ultra,
                "coding_hash": coding_hash,
                "auxiliary_sha256": match &still_segs { Some(s) => format::auxiliary_hashes_from_segments(&p.still, s).ok(), None => format::auxiliary_hashes(&p.still).ok() },
                "still_sha256": still_sha256,
                "still_bytes": p.still.len(),
                "video_offset": p.video.start,
                "video_bytes": video.len(),
                "video_sha256": video_sha256,
                "high_resolution_us": times.map(|t| t.high_resolution),
                "exif_make": make,
                "exif_model": model,
            })
        }
        Err(e) => json!({ "ok": false, "error": e.0, "error_kind": e.kind().code() }),
    }
}
