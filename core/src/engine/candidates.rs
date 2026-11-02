//! High-resolution cover candidates (Swift `CoverCandidateSession`).
//!
//! The candidate list (secondary-track frames, Google high-resolution
//! times, edit-list mapping) is computed here from the container. Only
//! decoding a frame to JPEG goes to the host ([`Native::frame_jpeg`]).

use crate::engine::{failure, movie, write, Native};
use crate::mp4::{self, Movie};
use crate::pyjpeg;
use crate::xmp::{HEADER as XMP_HEADER, RDF};
use crate::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub index: usize,
    pub timestamp_us: i64,
    pub time_source: &'static str,
    /// The frame's media time on the secondary track, seconds.
    pub media_seconds: f64,
}

impl Candidate {
    pub fn json(&self) -> Value {
        json!({"index": self.index, "timestamp_us": self.timestamp_us, "time_source": self.time_source})
    }
}

pub struct Session {
    pub candidates: Vec<Candidate>,
    pub duration_repair: Option<Value>,
    /// The decode copy handed to the host (repaired when needed).
    pub video: PathBuf,
}

/// Map a media time (track ticks) to the movie timeline through the edit
/// list (Swift `presented`). Returns microseconds.
fn presented_us(
    media: i64,
    track_ts: i64,
    movie_ts: i64,
    edits: &Option<Vec<(i64, i64)>>,
) -> Result<i64> {
    let (track_ts, movie_ts) = (track_ts.max(1) as i128, movie_ts.max(1) as i128);
    let Some(entries) = edits else {
        return Ok((media as i128 * 1_000_000 / track_ts) as i64);
    };
    let mut cursor: i128 = 0;
    let mut hits = Vec::new();
    for (dur, start) in entries {
        let (dur, start) = (*dur as i128, *start as i128);
        if start >= 0 && dur > 0 {
            let span = dur * track_ts / movie_ts;
            let m = media as i128;
            if start <= m && m < start + span.max(1) {
                hits.push(cursor * 1_000_000 / movie_ts + (m - start) * 1_000_000 / track_ts);
            }
        }
        cursor += dur;
    }
    if hits.len() != 1 {
        return failure("辅助轨编辑时间线无法唯一定位候选画面");
    }
    Ok(hits[0] as i64)
}

/// Swift `CoverCandidateSession.prepare`: None when the video has no
/// secondary track.
pub fn prepare(video: &[u8], folder: &Path) -> Result<Option<Session>> {
    let repaired = movie::repair_auxiliary_duration(video)?;
    let decode: &[u8] = repaired.as_ref().map_or(video, |r| &r.0);
    let path = folder.join("candidate-source.mp4");
    write(&path, decode)?;
    let parsed = Movie::parse(decode)?;
    let videos: Vec<&mp4::Track> = parsed
        .tracks
        .iter()
        .filter(|t| t.samples.handler == "vide")
        .collect();
    if videos.len() > 2 {
        return failure("高清候选需要唯一的辅助视频轨");
    }
    if videos.len() != 2 {
        return Ok(None);
    }
    let track = videos[1];
    let mut points: Vec<i64> = track
        .samples
        .samples
        .iter()
        .map(|s| s.presentation_time)
        .collect();
    points.sort_unstable();
    if points.is_empty() || points.windows(2).any(|w| w[0] >= w[1]) {
        return failure("辅助轨没有可唯一定位的高清候选帧");
    }
    let recorded = mp4::image_times(video)?
        .map(|t| t.high_resolution)
        .unwrap_or_default();
    if !recorded.is_empty() && recorded.len() != points.len() {
        return failure("高清候选时间描述符与辅助轨帧数不一致");
    }
    let edits = mp4::edit_list(decode, &track.trak)?;
    let ts = track.samples.timescale as i64;
    let mut candidates = Vec::new();
    for (index, point) in points.iter().enumerate() {
        let presented = presented_us(*point, ts, parsed.timescale as i64, &edits)?;
        if presented < 0 {
            return failure("辅助轨时间超出支持范围");
        }
        let declared = recorded.get(index).copied().flatten();
        let us = declared.unwrap_or(presented);
        if us < 0 {
            return failure("高清候选时间无效");
        }
        candidates.push(Candidate {
            index,
            timestamp_us: us,
            time_source: if declared.is_none() {
                "auxiliary-track-presentation"
            } else {
                "google-high-res-metadata"
            },
            media_seconds: *point as f64 / ts.max(1) as f64,
        });
    }
    Ok(Some(Session {
        candidates,
        duration_repair: repaired.map(|r| r.1),
        video: path,
    }))
}

impl Session {
    pub fn candidate(&self, index: i64) -> Result<&Candidate> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.candidates.get(i))
            .map_or_else(|| failure("所选高清候选不存在；请重新选择封面"), Ok)
    }

    /// Swift `writeJPEG`: decoded frame, tagged with the candidate time.
    pub fn write_jpeg(&self, native: &dyn Native, index: i64, to: &Path) -> Result<Value> {
        let choice = self.candidate(index)?.clone();
        let (encoded, width, height) = native.frame_jpeg(&self.video, 1, choice.media_seconds)?;
        if encoded.len() < 2 || encoded[..2] != [0xFF, 0xD8] {
            return failure("候选照片编码失败");
        }
        let xml = format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"{RDF}\"><rdf:Description xmlns:MomentStays=\"urn:momentstays:cover:1.0\" MomentStays:CandidatePresentationTimestampUs=\"{}\" MomentStays:CandidateIndex=\"{}\"/></rdf:RDF></x:xmpmeta>",
            choice.timestamp_us, choice.index
        );
        let mut payload = XMP_HEADER.to_vec();
        payload.extend(xml.as_bytes());
        let mut tagged = encoded[..2].to_vec();
        tagged.extend(pyjpeg::app(0xE1, &payload)?);
        tagged.extend(&encoded[2..]);
        write(to, &tagged)?;
        let mut result = choice.json();
        result["width"] = json!(width);
        result["height"] = json!(height);
        result["encoding"] = json!("decoded-video-to-jpeg");
        if let Some(r) = &self.duration_repair {
            result["decode_duration_repair"] = r.clone();
        }
        Ok(result)
    }
}
