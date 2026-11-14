//! Android Motion Photo -> Apple Live Photo, cover policy and pairing assembly.
//! HDR is delegated to engine::hdr; output integrity to engine::output.
use crate::engine::candidates::{self, Session};
use crate::engine::movie::{self, MovieRequest};
use crate::engine::options::ConversionOptions;
use crate::engine::output::{
    candidate_video_role, expected_names, names_json, publish, resume, Stage, WorkDir, MANIFEST,
    REVISION,
};
use crate::engine::paths::{self, file_name, json as s, stem};
use crate::engine::still;
use crate::engine::{
    failure, hash_file, read, uuid, write, write_json, Native, StopToken, Timings,
};
use crate::{format, heif, jpeg, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

fn movie_request<'a>(
    still: &'a [u8],
    identifier: &'a str,
    options: &'a ConversionOptions,
) -> MovieRequest<'a> {
    MovieRequest {
        still: Some(still),
        identifier,
        video_track: &options.video_track,
        missing_time: &options.missing_time,
        time_domain: &options.time_domain,
        ..Default::default()
    }
}

/// Swift `NativeBatch.convert`: one Motion Photo to a `.livephoto` folder.
pub fn convert(
    native: &dyn Native,
    source: &Path,
    target: &Path,
    options: &ConversionOptions,
    token: &StopToken,
    overwrite: bool,
    timings: &Timings,
) -> Result<Value> {
    let mut options = options.clone();
    if let Some(i) = options
        .cover_selections
        .get(&*paths::normalized(source).to_string_lossy())
    {
        options.cover_index = Some(*i);
    }
    if let Some(c) = options
        .manual_covers
        .get(&*paths::normalized(source).to_string_lossy())
    {
        options.manual_cover = Some(c.clone());
    }
    let options = &options;
    let modes = [
        "livephoto",
        "livephoto+auxiliary",
        "auxiliary-only",
        "livephoto+candidates",
    ];
    if !["primary", "secondary"].contains(&options.video_track.as_str())
        || !modes.contains(&options.output_mode.as_str())
    {
        return failure("轨道或导出模式无效");
    }
    if options.cover_index.is_some() && options.output_mode == "auxiliary-only" {
        return failure("仅导出辅助轨时不能更换照片封面");
    }
    if !((options.cover_index.is_none() && options.output_mode != "livephoto+candidates")
        || options.video_track == "primary")
    {
        return failure("高清候选封面需搭配主运动轨");
    }
    if !["static", "livephoto"].contains(&options.candidate_delivery.as_str()) {
        return failure("候选照片的交付格式无效");
    }
    let source_hash = timings.measure("source_hash", || hash_file(source))?;
    let data = timings.measure("source_read", || read(source))?;
    let photo = timings.measure("source_parse", || crate::engine::manual::parse(&data))?;
    let legacy = photo.format == crate::engine::manual::FORMAT;
    if legacy && native.image_decodes(&photo.still) == Some(false) {
        return failure("旧华为静态照片损坏，人工选择不能修复输入");
    }
    let manual = match &options.manual_cover {
        Some(selection) => Some(crate::engine::manual::resolve(&data, source, selection)?),
        None if legacy => {
            return failure("旧式华为封面未确定；请人工选择主视频帧，不使用原双数字或中点猜测")
        }
        None => None,
    };
    if legacy
        && (options.video_track != "primary"
            || options.output_mode != "livephoto"
            || options.cover_index.is_some())
    {
        return failure("旧式华为人工选帧仅支持主视频与原静态照片 Live Photo；不使用辅助轨候选");
    }
    if !overwrite && target.exists() {
        return resume(source, target, options, token, &source_hash);
    }
    if options.output_mode != "auxiliary-only"
        && options.cover_index.is_none()
        && manual.is_none()
        && photo.timestamp.is_none()
        && photo.cover_frame.is_none()
        && options.missing_time == "error"
    {
        return failure("原片没有有效封面时间戳或封面帧号，未使用推测位置");
    }
    if token.is_cancelled() {
        return failure("已停止");
    }
    let stage = Stage::new(target)?;
    let work = WorkDir::new()?;
    let video = photo.video(&data);
    let mut names = expected_names(source, options);
    let wants_candidates =
        options.cover_index.is_some() || options.output_mode == "livephoto+candidates";
    let session: Option<Session> = if wants_candidates {
        candidates::prepare(video, work.path())?
    } else {
        None
    };
    if let Some(index) = options.cover_index {
        match &session {
            Some(s) => {
                s.candidate(index)?;
            }
            None => return failure("原片没有辅助高清候选，请使用原始封面"),
        }
    }

    // Auxiliary track export.
    let mut auxiliary: Option<Value> = None;
    if let Some(aux_name) = names.get("auxiliary").cloned() {
        let aux_path = stage.path().join(&aux_name);
        let mut repair: Option<Value> = None;
        let mut input: Vec<u8> = video.to_vec();
        if options.repair_auxiliary_duration {
            if let Some((fixed, info)) = movie::repair_auxiliary_duration(video)? {
                input = fixed;
                repair = Some(info);
            }
        }
        let (mov, mut info) = movie::export_auxiliary(&input)?;
        if let Some(r) = repair {
            if info["compressed_sha256"] != r["original_sample_sha256"] {
                return failure("修复后的辅助轨压缩数据与原片不一致");
            }
            info["duration_repair"] = r;
        }
        write(&aux_path, &mov)?;
        let _ = crate::engine::stamp_output_time(source, &aux_path, options.exif_file_times);
        auxiliary = Some(info);
    }
    if options.output_mode == "auxiliary-only" {
        let aux = auxiliary.unwrap_or(Value::Null);
        let name = names["auxiliary"].clone();
        let manifest = json!({
            "version": "1.1.4", "implementation": REVISION, "source": s(source),
            "source_sha256": source_hash, "original_filename": file_name(source),
            "options": options.json(), "output_files": names_json(&names),
            "output_sha256": {name.clone(): hash_file(&stage.path().join(&name))?},
            "timing": aux, "auxiliary_video": aux, "hdr": {"status": "not-applicable"}, "warnings": [],
        });
        write_json(&manifest, &stage.path().join(MANIFEST))?;
        timings.measure("validation_and_publish", || {
            publish(
                native,
                &stage,
                target,
                source,
                &source_hash,
                token,
                overwrite,
                options.export_data_files,
            )
        })?;
        let repaired = if aux.get("duration_repair").is_some() {
            "；已按设置修复零时长，画面数据未改变"
        } else {
            ""
        };
        let num = |k: &str| aux[k].to_string();
        return Ok(json!({
            "status": "converted", "source": s(source), "output": s(target),
            "detail": format!("辅助轨已单独导出：{} × {}，{} 帧，{} 秒；无音频，未重新编码{repaired}", num("width"), num("height"), num("frames"), num("duration_seconds")),
            "auxiliary_video": aux, "verification": manifest,
        }));
    }

    // Cover selection belongs to live-photo assembly. HDR processing accepts
    // only still bytes and independent policy, and never touches the movie.
    let mut image: Vec<u8> = photo.still.to_vec();
    let mut selected_candidate: Option<Value> = None;
    if let (Some(index), Some(sess)) = (options.cover_index, &session) {
        let selected = work.path().join("selected-cover.jpg");
        selected_candidate = Some(sess.write_jpeg(native, index, &selected)?);
        image = read(&selected)?;
    }
    let is_heic = heif::matches(&image);
    let processed = crate::engine::hdr::process(
        native,
        image,
        crate::engine::hdr::Options {
            try_private_hdr: options.legacy_hdr && options.cover_index.is_none(),
            already_ultra_hdr: photo.ultra && options.cover_index.is_none(),
            allow_legacy_conversion: !legacy && options.cover_index.is_none(),
        },
    )?;
    let image = processed.image;
    let mut hdr = processed.report;
    let mut warnings = processed.warnings;
    let honor = processed.honor_extension;
    if auxiliary
        .as_ref()
        .is_some_and(|a| a.get("duration_repair").is_some())
    {
        warnings.push("已按高级设置修复独立辅助视频的零时长，压缩画面与主封面时间未改变".into());
    }
    if options.cover_index.is_some() {
        hdr = json!({"input": "auxiliary-video", "status": "decoded-sdr", "reason": "辅助高清帧已解码为 JPEG 封面；主运动轨保持原编码，未复制原封面的 HDR 增益图"})
            .as_object().cloned().unwrap_or_default();
    }

    let mut hdr_value = Value::Object(hdr);
    crate::engine::hdr::annotate(&mut hdr_value);
    hdr = hdr_value.as_object().cloned().unwrap_or_default();
    let identifier = uuid();
    let live = still::live_still(&image, &identifier, true)?;
    if crate::engine::hdr::honor_private(&live)? != honor {
        return failure("荣耀私有 HDR 静态扩展字节校验失败");
    }
    let static_ok = if is_heic {
        heif::layout(&live)?
            .boxes
            .iter()
            .all(|b| !b.is(b"mpvd") && !b.is(b"moov"))
    } else {
        jpeg::static_only(&live)? == live
    };
    if !static_ok {
        return failure("导出静态图片仍包含图片范围之外的数据");
    }
    if format::coding_hash(&live)? != format::coding_hash(&image)?
        || format::auxiliary_hashes(&live)? != format::auxiliary_hashes(&image)?
    {
        return failure("静态主图压缩数据发生变化");
    }
    let still_path = stage.path().join(&names["still"]);
    let mov_path = stage.path().join(&names["video"]);
    write(&still_path, &live)?;

    let mut request = movie_request(&live, &identifier, options);
    let candidate_time;
    if let Some(cover) = &manual {
        request.timestamp_us = Some(cover.marker_us);
        request.time_domain = "presentation";
        request.timestamp_source = Some("user-frame");
    } else if let (Some(index), Some(sess)) = (options.cover_index, &session) {
        candidate_time = sess.candidate(index)?.timestamp_us;
        request.timestamp_us = Some(candidate_time);
        request.time_domain = "presentation";
        request.timestamp_source = Some("high-res-candidate");
    } else {
        request.timestamp_us = photo.timestamp;
        request.cover_frame_index = photo.cover_frame;
    }
    if options.cover_index.is_none()
        && photo.timestamp_tag.as_deref() == Some("MP4:motionphoto-image-meta")
    {
        request.timestamp_source = Some("video-metadata");
        warnings.push("主 XMP 缺失；已从内嵌视频元数据恢复原始封面时间".into());
    }
    let (mov, mut timing) = timings.measure("media_remux_and_validation", || {
        movie::movie(video, &request)
    })?;
    write(&mov_path, &mov)?;
    let expected_audio = timing["tracks"]
        .as_array()
        .map_or(0, |v| v.iter().filter(|t| t["type"] == "soun").count());
    let audio_checked = native.audio_decodes(&mov_path, expected_audio)?;
    if audio_checked == Some(false) {
        return failure("Apple 原生音频解码校验失败；未发布可能无声的实况结果");
    }
    timing["audio_native_decode"] = json!(if expected_audio == 0 {
        "not-applicable"
    } else if audio_checked == Some(true) {
        "completed"
    } else {
        "unavailable"
    });

    // Extra candidate stills (and optional Live pairs for each).
    let mut candidate_photos: Vec<Value> = Vec::new();
    if options.output_mode == "livephoto+candidates" {
        let source_stem = stem(source);
        let pair_candidate = |names: &mut BTreeMap<String, String>,
                              role: &str,
                              name: &str,
                              timestamp: Option<i64>,
                              frame: Option<i64>,
                              domain: &str|
         -> Result<Option<Value>> {
            if options.candidate_delivery != "livephoto" {
                return Ok(None);
            }
            if token.is_cancelled() {
                return failure("已停止");
            }
            let path = stage.path().join(name);
            let id = uuid();
            let bytes = still::live_still(&read(&path)?, &id, true)?;
            write(&path, &bytes)?;
            let movie_name = format!("{}.mov", stem(Path::new(name)));
            let mut req = movie_request(&bytes, &id, options);
            req.time_domain = domain;
            req.video_track = "primary";
            req.timestamp_us = timestamp;
            req.cover_frame_index = frame;
            if role != "original" {
                req.timestamp_source = Some("high-res-candidate");
            }
            let (m, t) =
                timings.measure("media_remux_and_validation", || movie::movie(video, &req))?;
            write(&stage.path().join(&movie_name), &m)?;
            let expected_audio = t["tracks"]
                .as_array()
                .map_or(0, |v| v.iter().filter(|t| t["type"] == "soun").count());
            if native.audio_decodes(&stage.path().join(&movie_name), expected_audio)? == Some(false)
            {
                return failure("Apple 原生音频解码校验失败；未发布候选实况结果");
            }
            names.insert(candidate_video_role(role), movie_name);
            Ok(Some(t))
        };
        if options.cover_index.is_some() {
            let name = format!(
                "{source_stem}.original.{}",
                source.extension().and_then(|e| e.to_str()).unwrap_or("")
            );
            let original = still::live_still(&photo.still, &identifier, false)?;
            write(&stage.path().join(&name), &original)?;
            names.insert("original".into(), name.clone());
            let mut info =
                json!({"role": "original", "file": name, "encoding": "original-photo-preserved"});
            if let Some(t) = pair_candidate(
                &mut names,
                "original",
                &name,
                photo.timestamp,
                photo.cover_frame,
                &options.time_domain,
            )? {
                info["timing"] = t;
            }
            candidate_photos.push(info);
        }
        if let Some(sess) = &session {
            for c in &sess.candidates {
                if Some(c.index as i64) == options.cover_index {
                    continue;
                }
                if token.is_cancelled() {
                    return failure("已停止");
                }
                let role = format!("candidate_{}", c.index);
                let name = format!("{source_stem}.candidate-{:03}.jpg", c.index + 1);
                let mut info =
                    sess.write_jpeg(native, c.index as i64, &stage.path().join(&name))?;
                info["role"] = json!(role);
                info["file"] = json!(name);
                names.insert(role.clone(), name.clone());
                if let Some(t) = pair_candidate(
                    &mut names,
                    &role,
                    &name,
                    Some(c.timestamp_us),
                    None,
                    "presentation",
                )? {
                    info["timing"] = t;
                }
                candidate_photos.push(info);
            }
        }
    }
    if legacy {
        names.extend(crate::engine::manual::archive(&data, &photo, stage.path())?);
        warnings.push("人工定位动态封面，原相机静态照片保留；原EXIF、私有块和尾标在输出目录附属数据中，相册仅接收JPEG/MOV".into());
    }
    let _ = crate::engine::stamp_output_time(source, &still_path, options.exif_file_times);
    let _ = crate::engine::stamp_output_time(source, &mov_path, options.exif_file_times);
    let mut hashes = BTreeMap::new();
    for name in names.values() {
        hashes.insert(name.clone(), hash_file(&stage.path().join(name))?);
    }
    let null_or = |v: Option<Value>| v.unwrap_or(Value::Null);
    let mut manifest = json!({
        "version": "1.1.4", "implementation": REVISION, "source": s(source),
        "source_sha256": source_hash, "identifier": identifier, "options": options.json(),
        "output_files": names_json(&names), "original_filename": file_name(source),
        "source_timestamp_us": null_or(photo.timestamp.map(Value::from)),
        "source_timestamp_tag": null_or(photo.timestamp_tag.clone().map(Value::from)),
        "source_cover_frame_index": null_or(photo.cover_frame.map(Value::from)),
        "source_format": photo.format, "still_contains_embedded_media": false,
        "timing": timing, "hdr": Value::Object(hdr.clone()), "warnings": warnings,
        "primary_coding_sha256": format::coding_hash(&live)?,
        "auxiliary_sha256": format::auxiliary_hashes(&live)?,
        "output_sha256": names_json(&hashes),
    });
    if let Some(selection) = &options.manual_cover {
        manifest["manual_cover"] = selection.json();
        manifest["original_cover_status"] = json!("unknown");
        manifest["preservation"] =
            json!({"archive":"source-original.bin","source_sha256":source_hash,"reversible":true});
    }
    if let Some(a) = &auxiliary {
        manifest["auxiliary_video"] = a.clone();
    }
    if let Some(c) = &selected_candidate {
        manifest["selected_cover"] = c.clone();
    }
    if options.output_mode == "livephoto+candidates" {
        manifest["candidate_photos"] = Value::from(candidate_photos.clone());
    }
    write_json(&manifest, &stage.path().join(MANIFEST))?;
    timings.measure("validation_and_publish", || {
        publish(
            native,
            &stage,
            target,
            source,
            &source_hash,
            token,
            overwrite,
            options.export_data_files,
        )
    })?;
    let mut detail = if warnings.is_empty() {
        hdr.get("reason")
            .and_then(Value::as_str)
            .unwrap_or("已转换并校验")
            .to_string()
    } else {
        warnings.join("；")
    };
    detail = format!(
        "实况已转换；{}；{}",
        crate::engine::hdr::summary(&Value::Object(hdr.clone())),
        detail
    );
    if options.output_mode == "livephoto+candidates" {
        detail += &format!(
            "；另存 {} 张{}",
            candidate_photos.len(),
            if options.candidate_delivery == "livephoto" {
                "独立实况照片"
            } else {
                "独立高清静态照片"
            }
        );
    }
    Ok(json!({
        "status": "converted", "source": s(source), "output": s(target), "timing": timing,
        "hdr": Value::Object(hdr), "warnings": warnings, "output_files": names_json(&names),
        "detail": detail, "verification": manifest,
    }))
}
