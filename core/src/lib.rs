//! momentstays-core: a cross-platform, byte-level port of the MomentStays
//! Motion Photo container parser (native/PhotoFormat.swift, PhotoHEIF.swift).
//!
//! Scope: parse and validate Android Motion Photos (Google/Ultra HDR JPEG,
//! Huawei/Honor LIVE_, Samsung SEF JPEG/HEIC, OPPO, vivo), split the still and
//! the embedded MP4, and compute the same coding hashes the app verifies with.
//! No pixel decode, no AVFoundation: those stay in the Swift app.
//! Error messages intentionally match the Swift implementation verbatim.

pub mod binary;
pub mod format;
pub mod hdr;
pub mod heif;
pub mod jpeg;
pub mod mp4;
pub mod pyjpeg;
pub mod pymotion;
pub mod pyxml;
pub mod tiff;
pub mod vendor;
pub mod xmp;

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);
impl Error { pub fn new(s: impl Into<String>) -> Self { Error(s.into()) } }
impl fmt::Display for Error { fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str(&self.0) } }
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
pub(crate) fn fail<T>(msg: impl Into<String>) -> Result<T> { Err(Error::new(msg)) }

pub fn hex(b: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for x in b { s.push(D[(x >> 4) as usize] as char); s.push(D[(x & 15) as usize] as char); }
    s
}
pub fn sha256_hex(b: &[u8]) -> String { use sha2::Digest; hex(&sha2::Sha256::digest(b)) }

pub mod bench;
pub use format::{parse, MotionPhoto};

/// JSON summary used by the CLI and the parity checks. Keys shared with
/// tests/NativeDriver.swift `format` (timestamp_us, ultra, coding_hash) keep
/// their names so outputs can be diffed directly.
pub fn inspect(data: &[u8]) -> serde_json::Value {
    use serde_json::json;
    match parse(data) {
        Ok(p) => {
            let video = p.video(data);
            let times = mp4::image_times(video).ok().flatten();
            let (make, model) = if heif::matches(&p.still) {
                heif::exif(&p.still).ok().and_then(|e| jpeg::app(225, &e).ok())
                    .map(|a| { let mut j = vec![255, 216]; j.extend(a); j.extend([255, 217]); vendor::make_model(&j) })
                    .unwrap_or_default()
            } else { vendor::make_model(&p.still) };
            json!({
                "ok": true,
                "format": p.format,
                "timestamp_us": p.timestamp,
                "timestamp_tag": p.timestamp_tag,
                "cover_frame": p.cover_frame,
                "ultra": p.ultra,
                "coding_hash": format::coding_hash(&p.still).ok(),
                "auxiliary_sha256": format::auxiliary_hashes(&p.still).ok(),
                "still_sha256": sha256_hex(&p.still),
                "still_bytes": p.still.len(),
                "video_offset": p.video.start,
                "video_bytes": video.len(),
                "video_sha256": sha256_hex(video),
                "high_resolution_us": times.map(|t| t.high_resolution),
                "exif_make": make,
                "exif_model": model,
            })
        }
        Err(e) => json!({ "ok": false, "error": e.0 }),
    }
}
