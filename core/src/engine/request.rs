//! JSON request dispatch shared by `momentstays-driver` and the C ABI
//! (`ms_core_engine`). Modes and keys follow
//! `apple/shared/tests/NativeDriver.swift`, plus `movie` and
//! `export-auxiliary` (Swift `MotionLive` helpers).

use crate::engine::batch::{self, ExistingOutput, Progress, RunRequest};
use crate::engine::movie::{self, MovieRequest};
use crate::engine::options::{AndroidOptions, ConversionOptions};
use crate::engine::scanrow::{ScanRow, SourceVersion};
use crate::engine::{android, candidates, needs_native, still, Native, StopToken};
use crate::{format, sha256_hex, Error, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Callbacks from a running request to its host.
pub trait Events: Sync {
    /// Batch progress (Swift `BatchProgress`).
    fn progress(&self, _p: &Progress) {}
    /// A decision for an output that already exists; None = use the
    /// request's `existing_output` policy (or keep the old result).
    fn existing_output(&self, _source: &Path, _target: &Path) -> Option<ExistingOutput> {
        None
    }
    /// Scan rows as they complete, in batches.
    fn rows_ready(&self, _rows: &[ScanRow]) {}
}

/// No host callbacks.
pub struct NoEvents;
impl Events for NoEvents {}

fn path(req: &Value, key: &str) -> Result<PathBuf> {
    req[key]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(format!("Missing {key}")))
}

// Bounded byte input for headless callers; all writes use the same Rust engine.
fn request_bytes(req: &Value, key: &str) -> Result<Vec<u8>> {
    let text = req[key]
        .as_str()
        .ok_or_else(|| Error::new(format!("Missing {key}")))?;
    if text.len() > 256 * 1024 * 1024 {
        return Err(Error::new("Byte request exceeds resource limit"));
    }
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|_| Error::new(format!("Invalid {key}")))
}

fn read(p: &Path) -> Result<Vec<u8>> {
    crate::engine::read(p)
}

fn write(p: &Path, d: &[u8]) -> Result<()> {
    crate::engine::write(p, d)
}

fn inputs(req: &Value) -> Vec<PathBuf> {
    if let Some(a) = req["inputs"].as_array() {
        return a
            .iter()
            .filter_map(Value::as_str)
            .map(PathBuf::from)
            .collect();
    }
    req["input"]
        .as_str()
        .map(PathBuf::from)
        .into_iter()
        .collect()
}

/// A scan row with what `run` needs to reuse it (source and version).
pub fn row_json(row: &ScanRow) -> Value {
    let mut v = row.json();
    v["source"] = json!(row.source.to_string_lossy());
    v["needs_inspection"] = json!(row.needs_inspection());
    if let Some(ver) = row.version {
        v["version"] = json!({
            "device": ver.device,
            "inode": ver.inode,
            "size": ver.size,
            "modified_ns": ver.modified_ns.to_string(),
            "changed_ns": ver.changed_ns.to_string(),
        });
    }
    v
}

pub(crate) fn row_from_json(v: &Value) -> Option<ScanRow> {
    let version = v.get("version").and_then(|ver| {
        Some(SourceVersion {
            device: ver["device"].as_u64()?,
            inode: ver["inode"].as_u64()?,
            size: ver["size"].as_u64()?,
            modified_ns: ver["modified_ns"].as_str()?.parse().ok()?,
            changed_ns: ver["changed_ns"].as_str()?.parse().ok()?,
        })
    });
    Some(ScanRow {
        source: PathBuf::from(v["source"].as_str()?),
        relative: v["relative"].as_str().unwrap_or("").into(),
        is_motion: v["motion"].as_bool().unwrap_or(false),
        hdr: v["hdr"].as_str().unwrap_or("").into(),
        cover: v["cover"].as_str().unwrap_or("").into(),
        detail: v["detail"].as_str().unwrap_or("").into(),
        manufacturer: v["manufacturer"].as_str().unwrap_or("未知").into(),
        manufacturer_detail: v["manufacturer_detail"].as_str().unwrap_or("").into(),
        confirmed_static: v["static"].as_bool().unwrap_or(false),
        version,
        burst: v.get("burst").filter(|v| v.is_object()).cloned(),
    })
}

/// Run one request. `token` cancels batch and scan work.
pub fn handle(
    native: &dyn Native,
    events: &dyn Events,
    token: &StopToken,
    req: &Value,
) -> Result<Value> {
    let mode = req["mode"]
        .as_str()
        .ok_or_else(|| Error::new("Missing request"))?;
    let inputs = inputs(req);
    let first = || {
        inputs
            .first()
            .cloned()
            .ok_or_else(|| Error::new("Missing input"))
    };
    match mode {
        "export-plan" => crate::engine::export::plan(native, token, req),
        "export" => crate::engine::export::run(events, token, req),
        "format" => {
            let data = read(&first()?)?;
            let photo = format::parse(&data)?;
            let mut result = json!({
                "timestamp_us": photo.timestamp,
                "format": photo.format,
                "cover_frame": photo.cover_frame,
                "timestamp_tag": photo.timestamp_tag,
                "video_sha256": sha256_hex(photo.video(&data)),
                "ultra": photo.ultra,
                "coding_hash": format::coding_hash(&photo.still)?,
            });
            if let Ok(output) = path(req, "output") {
                let live =
                    still::live_still(&photo.still, "CCA01975-5655-4A4C-88C6-D620975EF319", true)?;
                write(&output, &live)?;
                result["output_coding_hash"] = json!(format::coding_hash(&live)?);
            }
            Ok(result)
        }
        "audio-check" => {
            let file = first()?;
            let movie = crate::mp4::Movie::parse(&read(&file)?)?;
            let count = movie
                .tracks
                .iter()
                .filter(|t| t.samples.handler == "soun")
                .count();
            match native.audio_decodes(&file, count)? {
                Some(true) => Ok(json!({"audio_tracks":count,"native_decode":"completed"})),
                Some(false) => Err(Error::new(
                    "Apple 原生音频解码失败，请重新转换后交付；未导入可能无声的旧结果",
                )),
                None => Err(Error::new("当前平台缺少原生音频校验能力")),
            }
        }
        "burst-filename" => Ok(
            json!({"candidate":crate::engine::burst::filename_candidate(req["filename"].as_str().unwrap_or(""))}),
        ),
        "honor-hdr-preflight" => {
            let path = Path::new(
                req["source"]
                    .as_str()
                    .ok_or_else(|| Error::new("缺少来源"))?,
            );
            let bytes = crate::engine::read(path)?;
            Ok(match crate::engine::hdr::honor::preflight(&bytes) {
                Ok(()) => json!({"supported":true}),
                Err(e) => json!({"supported":false,"reason":e.0}),
            })
        }
        "burst-plan" => {
            let rows: Vec<_> = req["scan_rows"]
                .as_array()
                .map(|a| a.iter().filter_map(row_from_json).collect())
                .unwrap_or_default();
            Ok(json!({"groups":crate::engine::burst::plans(&rows)}))
        }
        "scan" => {
            let direction = req["direction"].as_str().unwrap_or("to-apple");
            let rows = batch::scan(
                native,
                &inputs,
                direction,
                token,
                req["concurrency"].as_u64().map(|n| n as usize),
                |rows| events.rows_ready(rows),
                |index, total| {
                    events.progress(&Progress {
                        index,
                        total,
                        relative: String::new(),
                        status: "scanned".into(),
                        detail: String::new(),
                    })
                },
            )?;
            let full = req["full_rows"].as_bool().unwrap_or(false);
            Ok(
                json!({"rows": rows.iter().map(|r| if full { row_json(r) } else { r.json() }).collect::<Vec<_>>()}),
            )
        }
        "scan-file" => {
            // One row for a file the host already read (Swift `scanFile`).
            let file = first()?;
            let relative = req["relative"].as_str().unwrap_or("");
            let row = match req["direction"].as_str().unwrap_or("to-apple") {
                "to-android" => {
                    crate::engine::live::pairing::scan_file_with_native(native, &file, relative)
                }
                "honor-to-google" => {
                    crate::engine::live::honor_to_google::scan_file(native, &file, relative)
                }
                "android-to-android" => {
                    crate::engine::transfer::scan_file(native, &file, relative, None)
                }
                _ => crate::engine::scanrow::scan_file(native, &file, relative, None),
            };
            Ok(row_json(&row))
        }
        "batch" => {
            let options = ConversionOptions::from_request(req);
            let policy = req["existing_output"].as_str().map(|p| match p {
                "overwrite" => ExistingOutput::Overwrite,
                "skip" => ExistingOutput::Skip,
                "resume" => ExistingOutput::Resume,
                _ => ExistingOutput::Cancel,
            });
            let decide = |s: &Path, t: &Path| {
                events
                    .existing_output(s, t)
                    .or(policy)
                    .unwrap_or(ExistingOutput::Cancel)
            };
            let has_decider =
                policy.is_some() || req["ask_existing_output"].as_bool().unwrap_or(false);
            let output = path(req, "output")?;
            let cancel_after = req["cancel_after"].as_u64().map(|n| n as usize);
            let scan_rows: Option<Vec<ScanRow>> = req["scan_rows"]
                .as_array()
                .map(|a| a.iter().filter_map(row_from_json).collect());
            let outcome = batch::run(
                native,
                &RunRequest {
                    sources: &inputs,
                    output: &output,
                    options: &options,
                    scan_rows: scan_rows.as_deref(),
                    concurrency: req["concurrency"].as_u64().map(|n| n as usize),
                },
                token,
                has_decider.then_some(&decide as &(dyn Fn(&Path, &Path) -> ExistingOutput + Sync)),
                |p| {
                    events.progress(p);
                    if cancel_after.is_some_and(|after| p.index >= after) {
                        token.cancel();
                    }
                },
            )?;
            let mut result = json!({
                "counts": outcome.counts,
                "report": outcome.report.as_ref().map(|r| r.to_string_lossy().into_owned()),
                "cancelled": outcome.cancelled,
            });
            if outcome.report.is_none() || req["include_report_data"].as_bool().unwrap_or(false) {
                result["report_data"] = outcome.report_data;
            }
            Ok(result)
        }
        "convert" => {
            let options = ConversionOptions::from_request(req);
            let timings = crate::engine::Timings::default();
            let source = first()?;
            let target = path(req, "output")?;
            let overwrite = req["overwrite"].as_bool().unwrap_or(false);
            let row = if options.direction == "honor-to-google" {
                crate::engine::live::honor_to_google::scan_file(native, &source, "")
            } else if options.direction == "to-android" {
                crate::engine::live::pairing::scan_file_with_native(native, &source, "")
            } else {
                crate::engine::scanrow::scan_file(native, &source, "", None)
            };
            if options.direction == "honor-to-google" && !row.is_motion && !row.is_static_hdr() {
                return Err(Error::new(row.detail));
            }
            if row.burst.is_some() {
                return Err(Error::new(
                    "连拍必须通过批次组计划执行，以检查完整成员和组事务",
                ));
            }
            if row.is_static_hdr() {
                return crate::engine::static_hdr::convert(
                    native, &source, &target, &options, token, overwrite, &timings,
                );
            }
            match options.direction.as_str() {
                "honor-to-google" => crate::engine::live::honor_to_google::convert(
                    native, &source, &target, &options, token, overwrite, &timings,
                ),
                "to-android" => android::convert(
                    native, &source, &target, &options, token, overwrite, &timings,
                ),
                "android-to-android" => crate::engine::transfer::convert(
                    native, &source, &target, &options, token, overwrite, &timings,
                ),
                _ => batch::convert(
                    native, &source, &target, &options, token, overwrite, &timings,
                ),
            }
        }
        "verify" => {
            let folder = first()?;
            if req["verification"].is_object() {
                crate::engine::output::verify_manifest(&folder, &req["verification"])?;
                Ok(req["verification"].clone())
            } else {
                batch::verify(&folder)
            }
        }
        "candidates-video" => {
            let folder = path(req, "output")?;
            std::fs::create_dir_all(&folder).map_err(|e| crate::engine::io_error(&folder, e))?;
            let session = candidates::prepare(&read(&first()?)?, &folder)?;
            let list: Vec<Value> = session
                .as_ref()
                .map(|s| s.candidates.iter().map(|c| c.json()).collect())
                .unwrap_or_default();
            let mut out = json!({"candidates":list,"present":session.is_some(),"duration_repair":session.as_ref().and_then(|s| s.duration_repair.clone())});
            if let (Some(i), Some(session)) = (req["index"].as_i64(), session.as_ref()) {
                out["image"] = session.write_jpeg(native, i, &folder.join("candidate.jpg"))?;
            }
            Ok(out)
        }
        "android-assemble" => {
            let (still_bytes, video_bytes) = if req["still_base64"].is_string() {
                (
                    request_bytes(req, "still_base64")?,
                    request_bytes(req, "video_base64")?,
                )
            } else {
                let data = read(&first()?)?;
                let photo = format::parse(&data)?;
                (photo.still.to_vec(), photo.video(&data).to_vec())
            };
            let options = AndroidOptions::vendor(req["vendor"].as_str().unwrap_or("honor"));
            let out = android::assemble(
                &still_bytes,
                &video_bytes,
                &options,
                req["time_us"].as_i64().unwrap_or(400_000),
                req["frame"].as_i64().unwrap_or(12),
            )?;
            if req["output"].is_string() {
                write(&path(req, "output")?, &out)?;
            }
            let back = format::parse(&out)?;
            Ok(json!({
                "format": back.format,
                "timestamp_us": back.timestamp,
                "cover_frame": back.cover_frame,
                "video_sha256": sha256_hex(back.video(&out)),
                "coding_hash": format::coding_hash(&back.still)?,
                "data_base64": if req["output"].is_string() { Value::Null } else { json!(base64::engine::general_purpose::STANDARD.encode(&out)) },
            }))
        }
        "android-still" => {
            let options = AndroidOptions {
                vendor: "google".into(),
                container: req["container"].as_str().unwrap_or("jpeg").into(),
                hdr: "ultra".into(),
                image_quality: req["image_quality"].as_f64().unwrap_or(0.95),
                gain_quality: req["gain_quality"].as_f64().unwrap_or(0.95),
                preserve_jpeg: req["preserve_jpeg"].as_bool().unwrap_or(true),
            };
            let (bytes, hdr) = android::prepare_still(
                native,
                &first()?,
                &options,
                req["legacy_hdr"].as_str() != Some("preserve"),
            )?;
            write(&path(req, "output")?, &bytes)?;
            Ok(json!({"hdr": hdr, "bytes": bytes.len()}))
        }
        "android-pair" => {
            // Swift `AndroidExport.pair` for the preview and Photos import.
            let p = android::pair(&first()?, req["time_domain"].as_str().unwrap_or("auto"))?;
            Ok(json!({
                "still": p.still.to_string_lossy(),
                "movie": p.movie.to_string_lossy(),
                "identifier": p.identifier,
                "cover_us": p.cover_us,
                "cover_seconds": p.cover_seconds,
                "frame": p.frame,
            }))
        }
        "cover-candidates" => {
            let data = read(&first()?)?;
            let photo = format::parse(&data)?;
            let folder = path(req, "output")?;
            std::fs::create_dir_all(&folder).map_err(|e| crate::engine::io_error(&folder, e))?;
            let session = candidates::prepare(photo.video(&data), &folder)?;
            let list: Vec<Value> = session
                .as_ref()
                .map(|s| s.candidates.iter().map(|c| c.json()).collect())
                .unwrap_or_default();
            let mut result = json!({"candidates": list});
            if let (Some(index), Some(s)) = (req["index"].as_i64(), &session) {
                result["image"] = s.write_jpeg(native, index, &folder.join("candidate.jpg"))?;
            }
            Ok(result)
        }
        "movie" => {
            let source = read(&first()?)?;
            let still_bytes = match req["still"].as_str() {
                Some(p) => Some(read(Path::new(p))?),
                None => None,
            };
            let request = MovieRequest {
                still: still_bytes.as_deref(),
                identifier: req["identifier"].as_str().unwrap_or(""),
                video_track: req["video_track"].as_str().unwrap_or("primary"),
                timestamp_us: req["timestamp_us"].as_i64(),
                cover_frame_index: req["cover_frame_index"].as_i64(),
                timestamp_source: req["timestamp_source"].as_str(),
                missing_time: req["missing_time"].as_str().unwrap_or("error"),
                time_domain: req["time_domain"].as_str().unwrap_or("auto"),
            };
            let (mov, timing) = movie::movie(&source, &request)?;
            write(&path(req, "output")?, &mov)?;
            Ok(timing)
        }
        "export-auxiliary" => {
            let (mov, info) = movie::export_auxiliary(&read(&first()?)?)?;
            write(&path(req, "output")?, &mov)?;
            Ok(info)
        }
        "live-still" => {
            let data = request_bytes(req, "data_base64")?;
            let out = still::live_still(
                &data,
                req["identifier"].as_str().unwrap_or(""),
                req["update_identifier"].as_bool().unwrap_or(true),
            )?;
            Ok(json!({"data_base64": base64::engine::general_purpose::STANDARD.encode(out)}))
        }
        "still-check" => {
            // Photos import guard: the still holds only image data.
            let data = read(&first()?)?;
            let ok = if crate::heif::matches(&data) {
                crate::heif::layout(&data).is_ok()
            } else {
                crate::jpeg::static_only(&data).is_ok_and(|s| s == data)
            };
            if !ok {
                return Err(Error::new("静态图片仍含嵌入视频或额外载荷"));
            }
            Ok(json!({"ok": true}))
        }
        "manual-cover-frames" => crate::engine::manual::frames(&first()?, &path(req, "output")?),
        "extract-to-apple" => {
            let data = read(&first()?)?;
            let photo = crate::engine::manual::parse(&data)?;
            write(&path(req, "output")?, photo.video(&data))?;
            Ok(json!({"format":photo.format}))
        }
        "preview-cover" => preview_cover(req, &first()?),
        "fixture" => needs_native("生成测试素材（H.264 编码）"),
        _ => Err(Error::new("Unknown mode")),
    }
}

/// Cover timing for the read-only motion preview (Swift `MotionPreview`,
/// which used `sourceCoverTiming`): the cover frame on the primary video
/// track and, for Motion Photos with a secondary track, each
/// high-resolution candidate. Times are microseconds on the movie
/// (presentation) timeline; a cover that cannot be placed gives `issue`.
fn preview_cover(req: &Value, source: &Path) -> Result<Value> {
    use crate::live::{resolve_cover_in, TimeDomain};
    use crate::model::Cover;
    use crate::mp4::Movie;
    let domain = match req["time_domain"].as_str().unwrap_or("auto") {
        "media" => TimeDomain::Media,
        "presentation" => TimeDomain::Presentation,
        _ => TimeDomain::Auto,
    };
    let direction = req["direction"].as_str().unwrap_or("to-apple");
    let (video, cover, movie_path): (Vec<u8>, Option<Cover>, Option<PathBuf>) =
        if direction == "to-android" {
            let p = android::pair(source, req["time_domain"].as_str().unwrap_or("auto"))?;
            (
                read(&p.movie)?,
                Some(Cover::Frame(p.frame as i64)),
                Some(p.movie),
            )
        } else {
            let data = read(source)?;
            let photo = if direction == "to-apple" {
                crate::engine::manual::parse(&data)?
            } else {
                format::parse(&data)?
            };
            let cover = match (photo.cover_frame, photo.timestamp) {
                (Some(f), _) => Some(Cover::Frame(f)),
                (None, Some(t)) => Some(Cover::Time(t)),
                _ => None,
            };
            (photo.video(&data).to_vec(), cover, None)
        };
    let movie = Movie::parse(&video)?;
    let track = movie
        .video(0)
        .map_err(|_| Error::new("没有可预览的视频轨"))?;
    let frames = crate::cover::sorted_presentation(&track.samples).len();
    if frames == 0 {
        return Err(Error::new("视频没有可预览的画面"));
    }
    let place = |c: Cover, d: TimeDomain| -> Value {
        match resolve_cover_in(&video, &movie, c, d, 0) {
            Ok(m) => {
                json!({"frame": m.frame, "presented_us": m.marker_us, "media_us": m.frame_time_us})
            }
            Err(e) => json!({"issue": crate::engine::movie::cover_error(e, false).0}),
        }
    };
    let cover_json = match cover {
        Some(c) => place(c, domain),
        None => {
            json!({"issue": "No valid source cover timestamp; use --missing-time midpoint only if intended"})
        }
    };
    let mut result = json!({"frames": frames, "cover": cover_json});
    if let Some(m) = movie_path {
        result["movie"] = json!(m.to_string_lossy());
    }
    if direction != "to-android"
        && movie
            .tracks
            .iter()
            .filter(|t| t.samples.handler == "vide")
            .count()
            > 1
    {
        let work = crate::engine::batch::WorkDir::new()?;
        match candidates::prepare(&video, work.path()) {
            Ok(Some(s)) => {
                let list: Vec<Value> = s
                    .candidates
                    .iter()
                    .map(|c| {
                        let mut v = place(Cover::Time(c.timestamp_us), TimeDomain::Presentation);
                        v["index"] = json!(c.index);
                        v["timestamp_us"] = json!(c.timestamp_us);
                        v
                    })
                    .collect();
                result["candidates"] = Value::from(list);
            }
            Ok(None) => {}
            Err(e) => result["candidate_issue"] = json!(e.0),
        }
    }
    Ok(result)
}
