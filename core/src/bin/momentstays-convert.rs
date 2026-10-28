//! `momentstays-convert`: cross-platform convert CLI (ROADMAP item 11).
//!
//! Unlike `momentstays convert`, this binary never calls the macOS Swift
//! helper: everything runs on the Rust core, on any OS. Commands:
//!
//! * `vendor <input> <output> --to <vendor>` — Motion Photo vendor
//!   conversion (Google/Samsung/Huawei/OPPO/vivo JPEG). The cover is taken
//!   from the input unless `--cover-time` / `--cover-frame` overrides it;
//!   Time<->Frame conversion via the video sample tables happens
//!   automatically when the target vendor needs the other form.
//! * `to-live <video.mp4> --uuid <uuid> -o <out.mov>` — Motion video to a
//!   QuickTime MOV with the content identifier (sample-copy remux).
//!   WARNING: the still-image-time mett marker is NOT written yet
//!   (ROADMAP item 9b2), so the output is a valid, identified MOV but not
//!   yet a complete Live Photo. The command prints this warning every run.
//! * `to-motion <still.jpg> <movie.mov> --to <vendor> -o <out.jpg>` —
//!   Live Photo pair to a vendor Motion Photo JPEG. Cover from
//!   `--cover-time` / `--cover-frame`, or `--cover-from-mett` (the mett
//!   track's marker timing; the marker value itself is not checked).
//!
//! Vendor aliases: `xiaomi` and `honor` use the Google-compatible layout
//! (like Swift `AndroidExport`: Honor's gallery plays the Google path).
//! HEIC inputs are refused: HEIC writers are a stretch item.

use momentstays_core::cover;
use momentstays_core::format;
use momentstays_core::live2motion::{self, LivePhotoInput};
use momentstays_core::model::{Cover, MotionModel, Vendor};
use momentstays_core::mp4;
use momentstays_core::remux;
use momentstays_core::{Error, Result};
use serde_json::json;
use std::env;
use std::fs;
use std::process::ExitCode;

const VERSION: &str = "0.1.0";

fn err(msg: impl Into<String>) -> Error {
    Error::new(msg.into())
}

fn usage() -> String {
    [
        "usage:",
        "  momentstays-convert vendor <input> <output> --to <vendor> [--cover-time <us> | --cover-frame <n>]",
        "  momentstays-convert to-live <video.mp4> --uuid <uuid> -o <out.mov> [--video-index <n>]",
        "  momentstays-convert to-motion <still.jpg> <movie.mov> --to <vendor> -o <out.jpg> [--cover-time <us> | --cover-frame <n> | --cover-from-mett]",
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

fn vendor_label(v: Vendor) -> &'static str {
    match v {
        Vendor::Google => "google",
        Vendor::Samsung => "samsung",
        Vendor::Huawei => "huawei",
        Vendor::Oppo => "oppo",
        Vendor::Vivo => "vivo",
    }
}

/// Convert a cover to the form the target vendor needs, using the video's
/// sample tables. Refuses (never guesses) when the conversion is out of
/// tolerance or ambiguous, like `cover.rs`.
fn convert_cover(video: &[u8], cover: Cover, target: Vendor) -> Result<Cover> {
    let need_frame = target == Vendor::Huawei;
    match (cover, need_frame) {
        (Cover::Frame(_), true) | (Cover::Time(_), false) => Ok(cover),
        (Cover::Time(t), true) => {
            let tables = mp4::sample_tables(video)?;
            let track = cover::video_track(&tables)?;
            Ok(Cover::Frame(cover::time_us_to_frame(track, t)? as i64))
        }
        (Cover::Frame(f), false) => {
            let tables = mp4::sample_tables(video)?;
            let track = cover::video_track(&tables)?;
            Ok(Cover::Time(cover::frame_to_time_us(track, f as usize)?))
        }
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
    let mut o = Opts { rest: args.to_vec() };
    let to = o.take("--to").ok_or_else(|| err("vendor 需要 --to <厂商>"))?;
    let target = parse_vendor(&to)?;
    let cover = match (o.take("--cover-time"), o.take("--cover-frame")) {
        (Some(t), None) => Some(Cover::Time(t.parse::<i64>().map_err(|_| err("封面时间不是整数"))?)),
        (None, Some(f)) => Some(Cover::Frame(f.parse::<i64>().map_err(|_| err("封面帧号不是整数"))?)),
        (Some(_), Some(_)) => return Err(err("--cover-time 与 --cover-frame 只能二选一")),
        (None, None) => None,
    };
    if o.rest.len() != 2 {
        return Err(err("vendor 需要 <input> <output>"));
    }
    let (input, output) = (o.rest[0].clone(), o.rest[1].clone());
    let data = fs::read(&input).map_err(|e| err(format!("读取输入失败: {e}")))?;
    let parsed = format::parse(&data)?;
    if parsed.format.contains("heic") {
        return Err(err("HEIC 动态照片暂不支持厂商互转，需 HEIC 写入支持"));
    }
    let mut model = MotionModel::from_parsed(&parsed, &data)?;
    if let Some(c) = cover {
        model.cover = c;
    }
    model.cover = convert_cover(&model.video, model.cover, target)?;
    let out = momentstays_core::model::write(&model, target)?;
    fs::write(&output, &out).map_err(|e| err(format!("写出失败: {e}")))?;
    println!("{}", json!({"status": "ok", "output": output, "vendor": vendor_label(target), "bytes": out.len()}));
    Ok(())
}

fn cmd_to_live(args: &[String]) -> Result<()> {
    let mut o = Opts { rest: args.to_vec() };
    let uuid = o.take("--uuid").ok_or_else(|| err("to-live 需要 --uuid <uuid>"))?;
    let output = o.take("-o").ok_or_else(|| err("to-live 需要 -o <out.mov>"))?;
    let video_index = o
        .take("--video-index")
        .map(|v| v.parse::<usize>().map_err(|_| err("视频索引不是整数")))
        .transpose()?
        .unwrap_or(0);
    if o.rest.len() != 1 {
        return Err(err("to-live 需要 <video.mp4>"));
    }
    let data = fs::read(&o.rest[0]).map_err(|e| err(format!("读取输入失败: {e}")))?;
    let out = remux::remux_to_mov_with_identifier(&data, video_index, &uuid)?;
    fs::write(&output, &out).map_err(|e| err(format!("写出失败: {e}")))?;
    println!("{}", json!({"status": "ok", "output": output, "bytes": out.len(), "content_identifier": uuid}));
    eprintln!("警告：still-image-time 标记尚未写入（ROADMAP item 9b2），输出是有效且带内容标识的 MOV，尚不是完整的 Live Photo。");
    Ok(())
}

fn cmd_to_motion(args: &[String]) -> Result<()> {
    let mut o = Opts { rest: args.to_vec() };
    let to = o.take("--to").ok_or_else(|| err("to-motion 需要 --to <厂商>"))?;
    let target = parse_vendor(&to)?;
    let output = o.take("-o").ok_or_else(|| err("to-motion 需要 -o <out.jpg>"))?;
    let from_mett = o.has("--cover-from-mett");
    let cover = match (o.take("--cover-time"), o.take("--cover-frame"), from_mett) {
        (Some(t), None, false) => Some(Cover::Time(t.parse::<i64>().map_err(|_| err("封面时间不是整数"))?)),
        (None, Some(f), false) => Some(Cover::Frame(f.parse::<i64>().map_err(|_| err("封面帧号不是整数"))?)),
        (None, None, true) => None,
        _ => return Err(err("--cover-time、--cover-frame、--cover-from-mett 只能三选一")),
    };
    if o.rest.len() != 2 {
        return Err(err("to-motion 需要 <still.jpg> <movie.mov>"));
    }
    let (still_path, movie_path) = (o.rest[0].clone(), o.rest[1].clone());
    let still_jpeg = fs::read(&still_path).map_err(|e| err(format!("读取静态图失败: {e}")))?;
    let movie_mov = fs::read(&movie_path).map_err(|e| err(format!("读取视频失败: {e}")))?;
    let cover = match cover {
        Some(c) => c,
        None => match live2motion::mett_cover_time(&movie_mov)? {
            Some(t) => Cover::Time(t),
            None => return Err(err("MOV 没有 still-image-time 标记，请用 --cover-time 或 --cover-frame 指定封面")),
        },
    };
    let cover = convert_cover(&movie_mov, cover, target)?;
    let input = LivePhotoInput { still_jpeg, movie_mov };
    let out = live2motion::assemble(&input, target, cover)?;
    fs::write(&output, &out).map_err(|e| err(format!("写出失败: {e}")))?;
    println!("{}", json!({"status": "ok", "output": output, "vendor": vendor_label(target), "bytes": out.len()}));
    Ok(())
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
        "to-live" => cmd_to_live(&args[1..]),
        "to-motion" => cmd_to_motion(&args[1..]),
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
