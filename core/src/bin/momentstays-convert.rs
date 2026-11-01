//! `momentstays-convert`: cross-platform convert CLI (ROADMAP item 11).
//!
//! Unlike `momentstays convert`, this binary never calls the macOS Swift
//! helper: everything runs on the Rust core, on any OS. Commands:
//!
//! * `vendor <input> <output> --to <vendor>` — Motion Photo vendor
//!   conversion (Google/Samsung/Huawei/OPPO/vivo JPEG; HEIC inputs route
//!   to the Google/Samsung HEIC assembly path). The cover is taken
//!   from the input unless `--cover-time` / `--cover-frame` overrides it;
//!   Time<->Frame conversion via the video sample tables happens
//!   automatically when the target vendor needs the other form.
//! * `motion-to-live <motion.jpg|heic> -o <dir>` — Motion Photo to a complete
//!   Live Photo pair (`<stem>.jpg|.heic` + `<stem>.mov`, shared
//!   ContentIdentifier, cover marker on the cover frame); see `live.rs`.
//! * `to-live <video.mp4> --uuid <uuid> -o <out.mov>` — Motion video to a
//!   QuickTime MOV with the content identifier (sample-copy remux).
//!   With `--cover-time-us <us>` the native-form still-image-time track
//!   is also written (ROADMAP item 9b2); without it, the output is a
//!   valid, identified MOV but not yet a complete Live Photo, and the
//!   command prints that warning.
//! * `to-motion <still.jpg> <movie.mov> --to <vendor> -o <out.jpg>` —
//!   Live Photo pair to a vendor Motion Photo JPEG. Cover from
//!   `--cover-time` / `--cover-frame`, or `--cover-from-mett` (the mett
//!   track's marker timing; the marker value itself is not checked).
//! * `scan <文件或目录> [--jobs N]` — Swift-aligned scan rows (ROADMAP item 4):
//!   one row per JPEG/HEIC file with motion flag, HDR class, cover
//!   text, detail text, vendor label and confirmed-static flag, in the
//!   `NativeDriver` scan shape. Files are scanned in parallel by
//!   default (`--jobs 1` restores the serial loop); output order
//!   always follows the sorted file order.
//!
//! Vendor aliases: `xiaomi` and `honor` use the Google-compatible layout
//! (like Swift `AndroidExport`: Honor's gallery plays the Google path).
//! HEIC inputs are supported for Google/Samsung targets only, like Swift
//! `AndroidExport.convert`.

use momentstays_core::cover;
use momentstays_core::files::{self, Mapped};
use momentstays_core::format;
use momentstays_core::live;
use momentstays_core::live2motion;
use momentstays_core::model::{self, Cover, Vendor};
use momentstays_core::mp4;
use momentstays_core::output;
use momentstays_core::remux;
use momentstays_core::{Error, Result};
use serde_json::{json, Value};
use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const VERSION: &str = "0.1.0";

fn err(msg: impl Into<String>) -> Error {
    Error::new(msg.into())
}

fn usage() -> String {
    [
        "usage:",
        "  momentstays-convert vendor <input> <output> --to <vendor> [--cover-time <us> | --cover-frame <n>] [--time-domain auto|media|presentation]",
        "  momentstays-convert motion-to-live <motion.jpg|heic> -o <目录> [--uuid <uuid>] [--time-domain auto|media|presentation] [--video-index <n>]",
        "  momentstays-convert to-live <video.mp4> --uuid <uuid> -o <out.mov> [--video-index <n>] [--cover-time-us <us>]",
        "  momentstays-convert to-motion <still.jpg> <movie.mov> --to <vendor> -o <out.jpg> [--cover-time <us> | --cover-frame <n> | --cover-from-mett] [--time-domain auto|media|presentation]",
        "  momentstays-convert scan <文件或目录> [--jobs N] — Swift 对齐的扫描行（NativeDriver scan 行键：relative/motion/hdr/cover/manufacturer/manufacturer_detail，另含 detail/confirmed_static/source；默认并行，--jobs 1 串行）",
        "vendors: google, xiaomi, honor (= google layout), samsung, huawei, oppo, vivo",
    ]
    .join("\n")
}

fn parse_vendor(s: &str) -> Result<Vendor> {
    match s {
        "google" | "xiaomi" | "honor" => Ok(Vendor::Google),
        "samsung" => Ok(Vendor::Samsung),
        "huawei" => Ok(Vendor::Huawei),
        "oppo" => Ok(Vendor::Oppo),
        "vivo" => Ok(Vendor::Vivo),
        _ => Err(err(format!("未知厂商: {s}"))),
    }
}

/// Convert a cover to the form the target vendor needs, using the video's
/// sample tables. Refuses (never guesses) when the conversion is out of
/// tolerance or ambiguous.
///
/// Time -> frame (Huawei target) matches the time on the media and/or
/// presentation (edit-list) timeline per `domain`, like `motion-to-live`;
/// `Auto` refuses when they name different frames. Frame -> time writes the
/// frame's raw presentation time (media timeline), as before.
fn convert_cover(
    video: &[u8],
    cover: Cover,
    target: Vendor,
    domain: live::TimeDomain,
) -> Result<Cover> {
    let need_frame = target == Vendor::Huawei;
    if matches!(
        (cover, need_frame),
        (Cover::Frame(_), true) | (Cover::Time(_), false)
    ) {
        return Ok(cover);
    }
    convert_cover_in(video, &mp4::Movie::parse(video)?, cover, target, domain)
}

/// [`convert_cover`] with the video already parsed.
fn convert_cover_in(
    video: &[u8],
    movie: &mp4::Movie,
    cover: Cover,
    target: Vendor,
    domain: live::TimeDomain,
) -> Result<Cover> {
    let need_frame = target == Vendor::Huawei;
    let single_video = || -> Result<()> {
        match movie
            .tracks
            .iter()
            .filter(|t| t.samples.handler == "vide")
            .count()
        {
            1 => Ok(()),
            0 => Err(err("视频没有视频轨，无法换算封面")),
            _ => Err(err("视频含多个视频轨，无法确定封面所在轨")),
        }
    };
    match (cover, need_frame) {
        (Cover::Frame(_), true) | (Cover::Time(_), false) => Ok(cover),
        (Cover::Time(_), true) => {
            single_video()?;
            let m = live::resolve_cover_in(video, movie, cover, domain, 0)?;
            Ok(Cover::Frame(m.frame as i64))
        }
        (Cover::Frame(f), false) => {
            single_video()?;
            let track = &movie.video(0)?.samples;
            Ok(Cover::Time(cover::frame_to_time_us(track, f as usize)?))
        }
    }
}

fn parse_domain(v: Option<String>) -> Result<live::TimeDomain> {
    match v.as_deref() {
        None | Some("auto") => Ok(live::TimeDomain::Auto),
        Some("media") => Ok(live::TimeDomain::Media),
        Some("presentation") => Ok(live::TimeDomain::Presentation),
        Some(other) => Err(err(format!(
            "未知时间线: {other}（auto、media 或 presentation）"
        ))),
    }
}

struct Opts {
    rest: Vec<String>,
}

impl Opts {
    fn take(&mut self, flag: &str) -> Option<String> {
        let i = self.rest.iter().position(|a| a == flag)?;
        self.rest.remove(i);
        if i < self.rest.len() {
            Some(self.rest.remove(i))
        } else {
            None
        }
    }
    fn has(&mut self, flag: &str) -> bool {
        if let Some(i) = self.rest.iter().position(|a| a == flag) {
            self.rest.remove(i);
            true
        } else {
            false
        }
    }
}

fn cmd_vendor(args: &[String]) -> Result<()> {
    let mut o = Opts {
        rest: args.to_vec(),
    };
    let to = o
        .take("--to")
        .ok_or_else(|| err("vendor 需要 --to <厂商>"))?;
    let target = parse_vendor(&to)?;
    let domain = parse_domain(o.take("--time-domain"))?;
    let cover = match (o.take("--cover-time"), o.take("--cover-frame")) {
        (Some(t), None) => Some(Cover::Time(
            t.parse::<i64>().map_err(|_| err("封面时间不是整数"))?,
        )),
        (None, Some(f)) => Some(Cover::Frame(
            f.parse::<i64>().map_err(|_| err("封面帧号不是整数"))?,
        )),
        (Some(_), Some(_)) => return Err(err("--cover-time 与 --cover-frame 只能二选一")),
        (None, None) => None,
    };
    if o.rest.len() != 2 {
        return Err(err("vendor 需要 <input> <output>"));
    }
    let (input, output) = (o.rest[0].clone(), o.rest[1].clone());
    output::ensure_distinct(&[Path::new(&input)], Path::new(&output))?;
    let mapped = Mapped::open(Path::new(&input)).map_err(|e| err(format!("读取输入失败: {e}")))?;
    let data = mapped.bytes();
    let parsed = format::parse(data)?;
    let video = parsed.video(data);
    let cover = match cover {
        Some(c) => c,
        None => model::cover_of(&parsed)?,
    };
    let cover = convert_cover(video, cover, target, domain)?;
    // HEIC inputs take the HEIC assembly path (Google/Samsung only);
    // JPEG inputs take the vendor writer. Either way the output is
    // streamed as head + video (borrowed from the input map) + tail.
    let plan = if momentstays_core::heif::matches(&parsed.still) {
        if target != Vendor::Google && target != Vendor::Samsung {
            return Err(err("HEIC 动态照片仅支持 Google 与三星目标"));
        }
        let time = match cover {
            Cover::Time(t) => t,
            Cover::Frame(_) => return Err(err("HEIC 目标需要封面时间")),
        };
        momentstays_core::heifwrite::plan_heic(&parsed.still, video, target, time)?
    } else {
        model::plan(&parsed.still, video, cover, target)?
    };
    let bytes = output::write_atomic(
        Path::new(&output),
        &[&plan.head, video, &plan.tail],
        |written| plan.verify(written, video),
    )?;
    println!(
        "{}",
        json!({"status": "ok", "output": output, "vendor": target.label(), "bytes": bytes})
    );
    Ok(())
}

/// Motion Photo -> complete Live Photo pair: `<dir>/<stem>.jpg|.heic` and
/// `<dir>/<stem>.mov`, same ContentIdentifier, still-image-time marker on
/// the cover frame. Both files are verified before either is written.
fn cmd_motion_to_live(args: &[String]) -> Result<()> {
    let mut o = Opts {
        rest: args.to_vec(),
    };
    let dir = o
        .take("-o")
        .ok_or_else(|| err("motion-to-live 需要 -o <输出目录>"))?;
    let uuid = match o.take("--uuid") {
        Some(u) => u,
        None => live::new_content_identifier()?,
    };
    let domain = parse_domain(o.take("--time-domain"))?;
    let video_index = o
        .take("--video-index")
        .map(|v| v.parse::<usize>().map_err(|_| err("视频索引不是整数")))
        .transpose()?
        .unwrap_or(0);
    if o.rest.len() != 1 {
        return Err(err("motion-to-live 需要 <动态照片>"));
    }
    let input = PathBuf::from(&o.rest[0]);
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| err("输入文件名无效"))?;
    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        return Err(err(format!("输出目录不存在: {}", dir.display())));
    }
    let mapped = Mapped::open(&input).map_err(|e| err(format!("读取输入失败: {e}")))?;
    let data = mapped.bytes();
    let photo = format::parse(data)?;
    let pair = live::from_motion(&photo, data, &uuid, domain, video_index)?;
    let still_path = dir.join(format!("{stem}.{}", if pair.heic { "heic" } else { "jpg" }));
    let movie_path = dir.join(format!("{stem}.mov"));
    for out in [&still_path, &movie_path] {
        output::ensure_distinct(&[&input], out)?;
    }
    let still_parts: [&[u8]; 1] = [&pair.still];
    let movie_parts: [&[u8]; 1] = [&pair.movie];
    output::write_atomic_many(
        &[(&still_path, &still_parts), (&movie_path, &movie_parts)],
        |i, written| output::same_bytes(if i == 0 { &still_parts } else { &movie_parts })(written),
    )?;
    let c = &pair.cover;
    let source = match c.source {
        Cover::Time(t) => json!({"time_us": t, "tag": photo.timestamp_tag}),
        Cover::Frame(f) => json!({"frame": f}),
    };
    println!(
        "{}",
        json!({
            "status": "ok",
            "still": still_path.to_string_lossy(),
            "movie": movie_path.to_string_lossy(),
            "content_identifier": pair.identifier,
            "source_format": photo.format,
            "ultra_hdr": photo.ultra,
            "cover": {
                "source": source,
                "domain": c.domain,
                "frame": c.frame,
                "frame_time_us": c.frame_time_us,
                "marker_us": c.marker_us,
                "error_us": c.error_us,
            },
        })
    );
    Ok(())
}

fn cmd_to_live(args: &[String]) -> Result<()> {
    let mut o = Opts {
        rest: args.to_vec(),
    };
    let uuid = o
        .take("--uuid")
        .ok_or_else(|| err("to-live 需要 --uuid <uuid>"))?;
    let output = o
        .take("-o")
        .ok_or_else(|| err("to-live 需要 -o <out.mov>"))?;
    let video_index = o
        .take("--video-index")
        .map(|v| v.parse::<usize>().map_err(|_| err("视频索引不是整数")))
        .transpose()?
        .unwrap_or(0);
    let cover_time_us = o
        .take("--cover-time-us")
        .map(|v| {
            v.parse::<i64>()
                .map_err(|_| err("--cover-time-us 不是整数"))
        })
        .transpose()?;
    if o.rest.len() != 1 {
        return Err(err("to-live 需要 <video.mp4>"));
    }
    output::ensure_distinct(&[Path::new(&o.rest[0])], Path::new(&output))?;
    let mapped =
        Mapped::open(Path::new(&o.rest[0])).map_err(|e| err(format!("读取输入失败: {e}")))?;
    let data = mapped.bytes();
    // The remux functions verify sample by sample before returning.
    let out = match cover_time_us {
        Some(t) => remux::remux_to_mov_with_identifier_and_mett(data, video_index, &uuid, t)?,
        None => remux::remux_to_mov_with_identifier(data, video_index, &uuid)?,
    };
    let parts: [&[u8]; 1] = [&out];
    output::write_atomic(Path::new(&output), &parts, output::same_bytes(&parts))?;
    println!(
        "{}",
        json!({"status": "ok", "output": output, "bytes": out.len(), "content_identifier": uuid})
    );
    if cover_time_us.is_none() {
        eprintln!("警告：still-image-time 标记尚未写入（未传 --cover-time-us），输出是有效且带内容标识的 MOV，尚不是完整的 Live Photo。");
    }
    Ok(())
}

fn cmd_to_motion(args: &[String]) -> Result<()> {
    let mut o = Opts {
        rest: args.to_vec(),
    };
    let to = o
        .take("--to")
        .ok_or_else(|| err("to-motion 需要 --to <厂商>"))?;
    let target = parse_vendor(&to)?;
    let output = o
        .take("-o")
        .ok_or_else(|| err("to-motion 需要 -o <out.jpg>"))?;
    let from_mett = o.has("--cover-from-mett");
    let domain = parse_domain(o.take("--time-domain"))?;
    let cover = match (o.take("--cover-time"), o.take("--cover-frame"), from_mett) {
        (Some(t), None, false) => Some(Cover::Time(
            t.parse::<i64>().map_err(|_| err("封面时间不是整数"))?,
        )),
        (None, Some(f), false) => Some(Cover::Frame(
            f.parse::<i64>().map_err(|_| err("封面帧号不是整数"))?,
        )),
        (None, None, true) => None,
        _ => {
            return Err(err(
                "--cover-time、--cover-frame、--cover-from-mett 只能三选一",
            ))
        }
    };
    if o.rest.len() != 2 {
        return Err(err("to-motion 需要 <still.jpg> <movie.mov>"));
    }
    let (still_path, movie_path) = (o.rest[0].clone(), o.rest[1].clone());
    output::ensure_distinct(
        &[Path::new(&still_path), Path::new(&movie_path)],
        Path::new(&output),
    )?;
    let still_map =
        Mapped::open(Path::new(&still_path)).map_err(|e| err(format!("读取静态图失败: {e}")))?;
    let movie_map =
        Mapped::open(Path::new(&movie_path)).map_err(|e| err(format!("读取视频失败: {e}")))?;
    let movie_mov = movie_map.bytes();
    // One parse of the MOV for the marker, the cover conversion and the remux.
    let parsed = mp4::Movie::parse(movie_mov)?;
    let cover = match cover {
        Some(c) => convert_cover_in(movie_mov, &parsed, c, target, domain)?,
        None => {
            // The marker time is movie time (edit lists applied) by
            // definition, so it is matched on that timeline only: the cover
            // is the frame shown at the marker. Huawei gets its index, other
            // vendors that frame's movie time.
            let t = match live2motion::mett_cover_time_in(movie_mov, &parsed)? {
                Some(t) => t,
                None => return Err(err(
                    "MOV 没有 still-image-time 标记，请用 --cover-time 或 --cover-frame 指定封面",
                )),
            };
            let m = live::resolve_cover_in(
                movie_mov,
                &parsed,
                Cover::Time(t),
                live::TimeDomain::Presentation,
                0,
            )?;
            if target == Vendor::Huawei {
                Cover::Frame(m.frame as i64)
            } else {
                Cover::Time(m.marker_us)
            }
        }
    };
    let (still, video) = live2motion::prepare_parsed(still_map.bytes(), movie_mov, &parsed)?;
    let plan = model::plan(&still, &video, cover, target)?;
    let out_len = output::write_atomic(
        Path::new(&output),
        &[&plan.head, &video, &plan.tail],
        |written| plan.verify(written, &video),
    )?;
    println!(
        "{}",
        json!({"status": "ok", "output": output, "vendor": target.label(), "bytes": out_len})
    );
    Ok(())
}

/// Swift-aligned scan rows (ROADMAP item 4): one row per file, in the
/// `NativeDriver` scan shape plus `detail`, `confirmed_static` and
/// `source`. Unreadable files become error rows, like Swift's per-file
/// catch; only directory-walk failures abort the command.
fn cmd_scan(args: &[String]) -> Result<()> {
    let mut jobs: Option<usize> = None;
    let mut positional: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--jobs" {
            let v = args
                .get(i + 1)
                .ok_or_else(|| err("--jobs 需要一个正整数"))?;
            let n: usize = v.parse().map_err(|_| err("--jobs 需要一个正整数"))?;
            if n == 0 {
                return Err(err("--jobs 必须 >= 1"));
            }
            jobs = Some(n);
            i += 2;
        } else if let Some(v) = args[i].strip_prefix("--jobs=") {
            let n: usize = v.parse().map_err(|_| err("--jobs 需要一个正整数"))?;
            if n == 0 {
                return Err(err("--jobs 必须 >= 1"));
            }
            jobs = Some(n);
            i += 1;
        } else {
            positional.push(&args[i]);
            i += 1;
        }
    }
    if positional.len() != 1 {
        return Err(err("scan 需要 <文件或目录> [--jobs N]"));
    }
    let input = Path::new(positional[0]);
    let (found, root) = if input.is_file() {
        if !files::is_media(input) {
            return Err(err("scan 输入支持 .jpg/.jpeg/.heic/.heif"));
        }
        (vec![files::Found::File(input.to_path_buf())], None)
    } else if input.is_dir() {
        (files::walk(input), Some(input.to_path_buf()))
    } else {
        return Err(err(format!("输入不存在: {}", input.display())));
    };
    let jobs = jobs.unwrap_or_else(files::default_jobs);
    // Parallel scan (perf item 7): results stay in file order.
    let rows: Vec<Value> =
        files::parallel_map(found.len(), jobs, |i| scan_row_for(&found[i], &root));
    println!(
        "{}",
        serde_json::to_string_pretty(&Value::Array(rows)).map_err(|e| err(e.to_string()))?
    );
    Ok(())
}

/// One scan row for a file, in the `NativeDriver` scan shape plus
/// `detail`, `confirmed_static` and `source`. Unreadable files become
/// error rows, like Swift's per-file catch.
fn scan_row_for(found: &files::Found, root: &Option<PathBuf>) -> Value {
    let file = found.path();
    let relative = match root {
        Some(r) => file
            .strip_prefix(r)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| file.to_string_lossy().into_owned()),
        None => file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.to_string_lossy().into_owned()),
    };
    let opened = match found {
        files::Found::File(p) => Mapped::open(p).map_err(|e| e.to_string()),
        files::Found::Unreadable(_, e) => Err(e.clone()),
    };
    let row = match opened {
        Ok(mapped) => momentstays_core::scan::scan_row(mapped.bytes()),
        Err(e) => {
            let maker = momentstays_core::scan::identify(&[], "");
            momentstays_core::scan::ScanRow {
                is_motion: false,
                hdr: "—".to_string(),
                cover: "—".to_string(),
                detail: format!("读取失败：{e}"),
                manufacturer: maker.name,
                manufacturer_detail: maker.detail,
                confirmed_static: false,
            }
        }
    };
    json!({
        "source": file.to_string_lossy(),
        "relative": relative,
        "motion": row.is_motion,
        "hdr": row.hdr,
        "cover": row.cover,
        "detail": row.detail,
        "manufacturer": row.manufacturer,
        "manufacturer_detail": row.manufacturer_detail,
        "confirmed_static": row.confirmed_static,
    })
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("{}", usage());
        return Err(err("缺少命令"));
    }
    match args[0].as_str() {
        "--version" => {
            println!("momentstays-convert {VERSION}");
            Ok(())
        }
        "vendor" => cmd_vendor(&args[1..]),
        "motion-to-live" => cmd_motion_to_live(&args[1..]),
        "to-live" => cmd_to_live(&args[1..]),
        "to-motion" => cmd_to_motion(&args[1..]),
        "scan" => cmd_scan(&args[1..]),
        other => Err(err(format!("未知命令: {other}"))),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let msg = e.to_string();
            if msg != "缺少命令" {
                eprintln!("error: {msg}");
            }
            if msg == "缺少命令"
                || msg.starts_with("未知命令")
                || msg.starts_with("未知厂商")
                || msg.contains("需要 ")
                || msg.contains("只能")
            {
                ExitCode::from(2)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}
