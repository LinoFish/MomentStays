//! GStreamer: exact video-frame decode and audio decodability.
//!
//! Video frames bypass the demuxer: the Rust MP4 reader picks the sample
//! AVFoundation would show (edit list, presentation order, see
//! `portable::frame_plan`), and only that GOP is pushed through
//! `appsrc ! decodebin`, so frame timing never depends on a demuxer's edit
//! list handling. Audio goes through `qtdemux ! decodebin` per track, like
//! the Apple backend's one `AVAssetReader` per track.
use super::super::portable::{self, FramePlan, VideoCodec};
use crate::{Error, ErrorKind, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use std::path::Path;
use std::time::{Duration, Instant};

const TIME_LIMIT: Duration = Duration::from_secs(30);

fn init() -> Result<()> {
    static INIT: std::sync::OnceLock<std::result::Result<(), String>> = std::sync::OnceLock::new();
    INIT.get_or_init(|| gst::init().map_err(|e| e.to_string()))
        .clone()
        .map_err(|e| Error::with_kind(ErrorKind::Unsupported, format!("GStreamer 不可用：{e}")))
}

fn map(path: &Path) -> Result<memmap2::Mmap> {
    let file =
        std::fs::File::open(path).map_err(|e| Error::with_kind(ErrorKind::Io, e.to_string()))?;
    // Read-only private mapping of a file the engine owns for this call.
    unsafe { memmap2::Mmap::map(&file) }.map_err(|e| Error::with_kind(ErrorKind::Io, e.to_string()))
}

/// Ends the pipeline (state NULL) on every exit path.
struct Running(gst::Pipeline);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.set_state(gst::State::Null);
    }
}

/// What stopped a pipeline early.
enum Stop {
    /// A decoder/demuxer for this stream is not installed (e.g. HEVC
    /// without org.freedesktop.Platform.codecs-extra).
    Missing(String),
    /// The stream itself cannot be decoded.
    Failed(String),
}

/// Drain bus messages; Some(stop) once the pipeline failed.
fn bus_stop(bus: &gst::Bus, missing: &mut Option<String>) -> Option<Stop> {
    while let Some(msg) = bus.pop() {
        match msg.view() {
            gst::MessageView::Element(e) => {
                if let Some(s) = e.structure() {
                    if s.name() == "missing-plugin" {
                        missing.get_or_insert_with(|| s.to_string());
                    }
                }
            }
            gst::MessageView::Error(err) => {
                let e = err.error();
                let text = format!("{e}");
                let plugin = e.matches(gst::CoreError::MissingPlugin)
                    || e.matches(gst::StreamError::CodecNotFound);
                return Some(match missing.take() {
                    // Unknown sample descriptions surface as raw fourcc caps:
                    // undecodable on any system, like AVFoundation's refusal.
                    Some(detail) if detail.contains("x-gst-fourcc") => Stop::Failed(detail),
                    Some(detail) => Stop::Missing(detail),
                    None if plugin => Stop::Missing(text),
                    None => Stop::Failed(text),
                });
            }
            _ => {}
        }
    }
    None
}

fn caps(plan: &FramePlan) -> gst::Caps {
    let name = match plan.codec {
        VideoCodec::H264 => "video/x-h264",
        VideoCodec::H265 => "video/x-h265",
    };
    gst::Caps::builder(name)
        .field("stream-format", plan.stream_format)
        .field("alignment", "au")
        .field("codec_data", gst::Buffer::from_slice(plan.config.clone()))
        .build()
}

/// Decode `window` and return the target frame as tight RGB.
fn decode_window(
    data: &[u8],
    plan: &FramePlan,
    window: std::ops::Range<usize>,
) -> std::result::Result<Option<(Vec<u8>, usize, usize)>, Stop> {
    let failed = |e: &dyn std::fmt::Display| Stop::Failed(e.to_string());
    let pipeline = gst::parse::launch(
        "appsrc name=src format=time ! decodebin ! videoconvert ! \
         video/x-raw,format=RGB ! appsink name=sink sync=false max-buffers=1",
    )
    .map_err(|e| Stop::Missing(e.to_string()))?
    .downcast::<gst::Pipeline>()
    .map_err(|_| Stop::Failed("pipeline".into()))?;
    let running = Running(pipeline);
    let src = running
        .0
        .by_name("src")
        .and_then(|e| e.downcast::<gst_app::AppSrc>().ok())
        .ok_or_else(|| Stop::Failed("appsrc".into()))?;
    let sink = running
        .0
        .by_name("sink")
        .and_then(|e| e.downcast::<gst_app::AppSink>().ok())
        .ok_or_else(|| Stop::Failed("appsink".into()))?;
    src.set_caps(Some(&caps(plan)));
    let bus = running.0.bus().ok_or_else(|| Stop::Failed("bus".into()))?;
    running
        .0
        .set_state(gst::State::Playing)
        .map_err(|e| failed(&e))?;
    // GStreamer clock times are unsigned: shift so the window starts >= 0.
    let samples = &plan.samples[window];
    let shift = samples
        .iter()
        .map(|s| s.pts_ns.min(s.dts_ns))
        .min()
        .unwrap_or(0)
        .min(0)
        .unsigned_abs();
    let at = |t: i64| gst::ClockTime::from_nseconds(t as u64 + shift);
    for s in samples {
        let mut buffer = gst::Buffer::from_slice(data[s.range.clone()].to_vec());
        {
            let b = buffer.get_mut().unwrap();
            b.set_pts(at(s.pts_ns));
            b.set_dts(at(s.dts_ns));
            b.set_duration(gst::ClockTime::from_nseconds(s.duration_ns.max(0) as u64));
            if !s.sync {
                b.set_flags(gst::BufferFlags::DELTA_UNIT);
            }
        }
        src.push_buffer(buffer).map_err(|e| failed(&e))?;
    }
    src.end_of_stream().map_err(|e| failed(&e))?;
    let target = at(plan.target_pts_ns());
    let started = Instant::now();
    let mut missing = None;
    loop {
        if let Some(stop) = bus_stop(&bus, &mut missing) {
            return Err(stop);
        }
        if started.elapsed() > TIME_LIMIT {
            return Err(Stop::Failed("视频帧解码超时".into()));
        }
        let Some(sample) = sink.try_pull_sample(gst::ClockTime::from_mseconds(100)) else {
            if sink.is_eos() {
                return match bus_stop(&bus, &mut missing) {
                    Some(stop) => Err(stop),
                    None => Ok(None),
                };
            }
            continue;
        };
        let Some(buffer) = sample.buffer() else {
            continue;
        };
        if buffer.pts() != Some(target) {
            continue;
        }
        let caps = sample.caps().ok_or_else(|| Stop::Failed("caps".into()))?;
        let info = gstreamer_video::VideoInfo::from_caps(caps).map_err(|e| failed(&e))?;
        let (w, h, stride) = (
            info.width() as usize,
            info.height() as usize,
            info.stride()[0].max(0) as usize,
        );
        let map = buffer.map_readable().map_err(|e| failed(&e))?;
        let offset = info.offset()[0];
        let pixels = map
            .as_slice()
            .get(offset..)
            .ok_or_else(|| Stop::Failed("帧缓冲区被截断".into()))?;
        let (rgb, w, h) = plan
            .orientation
            .apply(pixels, w, h, stride, 3)
            .map_err(|e| failed(&e))?;
        return Ok(Some((rgb, w, h)));
    }
}

/// `Native::frame_jpeg`: the frame at media time `seconds` of video track
/// `track`, track transform applied, JPEG at quality 1.0.
pub fn frame(path: &Path, track: usize, seconds: f64) -> Result<(Vec<u8>, u32, u32)> {
    let data = map(path)?;
    let plan = portable::frame_plan(&data, track, seconds)?;
    init()?;
    // An open-GOP target (HEVC RASL/leading pictures) may need the previous
    // GOP too; three tries cover any real phone recording.
    for extra in 0..3 {
        match decode_window(&data, &plan, plan.window(extra)) {
            Ok(Some((rgb, w, h))) => {
                if w > super::tj::MAX_SIDE || h > super::tj::MAX_SIDE {
                    return Err(Error::new("图像尺寸或行跨度无效"));
                }
                let jpeg = super::tj::encode(
                    &rgb,
                    w,
                    w * 3,
                    h,
                    super::tj::PF_RGB,
                    super::tj::SAMP_444,
                    1.0,
                )?;
                return Ok((jpeg, w as u32, h as u32));
            }
            Ok(None) if plan.window(extra).start == 0 => break,
            Ok(None) => continue,
            Err(Stop::Missing(detail)) => {
                return Err(Error::with_kind(
                    ErrorKind::Unsupported,
                    format!("系统缺少视频解码器，无法解码辅助轨高清画面（{detail}）"),
                ))
            }
            Err(Stop::Failed(detail)) => {
                return Err(Error::new(format!("视频帧解码失败：{detail}")))
            }
        }
    }
    Err(Error::new("视频帧解码失败"))
}

enum Audio {
    Frames(u64),
    Undecodable,
    Missing,
}

fn decode_audio(path: &Path, index: usize) -> Result<Audio> {
    let pipeline = gst::parse::launch(&format!(
        "filesrc name=file ! qtdemux name=demux demux.audio_{index} ! queue ! decodebin ! \
         audioconvert ! audio/x-raw,format=S16LE,layout=interleaved ! \
         appsink name=sink sync=false max-buffers=8"
    ))
    .map_err(|e| Error::with_kind(ErrorKind::Unsupported, e.to_string()))?
    .downcast::<gst::Pipeline>()
    .map_err(|_| Error::new("pipeline"))?;
    let running = Running(pipeline);
    let file = running
        .0
        .by_name("file")
        .ok_or_else(|| Error::new("filesrc"))?;
    file.set_property("location", path.to_string_lossy().as_ref());
    let sink = running
        .0
        .by_name("sink")
        .and_then(|e| e.downcast::<gst_app::AppSink>().ok())
        .ok_or_else(|| Error::new("appsink"))?;
    let bus = running.0.bus().ok_or_else(|| Error::new("bus"))?;
    if running.0.set_state(gst::State::Playing).is_err() {
        return Ok(Audio::Undecodable);
    }
    let started = Instant::now();
    let mut frames = 0u64;
    let mut missing = None;
    loop {
        match bus_stop(&bus, &mut missing) {
            Some(Stop::Missing(_)) => return Ok(Audio::Missing),
            Some(Stop::Failed(_)) => return Ok(Audio::Undecodable),
            None => {}
        }
        if frames > 100_000_000 || started.elapsed() > TIME_LIMIT {
            return Err(Error::new("原生音频校验超过资源限制，未发布结果"));
        }
        let Some(sample) = sink.try_pull_sample(gst::ClockTime::from_mseconds(100)) else {
            if sink.is_eos() {
                return Ok(match bus_stop(&bus, &mut missing) {
                    Some(Stop::Missing(_)) => Audio::Missing,
                    Some(Stop::Failed(_)) => Audio::Undecodable,
                    None => Audio::Frames(frames),
                });
            }
            continue;
        };
        let channels = sample
            .caps()
            .and_then(|c| c.structure(0))
            .and_then(|s| s.get::<i32>("channels").ok())
            .unwrap_or(1)
            .max(1) as u64;
        if let Some(b) = sample.buffer() {
            frames = frames.saturating_add(b.size() as u64 / (2 * channels));
        }
    }
}

/// `Native::audio_decodes`. Some(false): a track count mismatch or a track
/// that does not decode to PCM (or decodes to zero frames). None: this
/// system lacks a decoder for a known codec, so decodability is unknown.
pub fn audio_decodes(path: &Path, expected: usize) -> Result<Option<bool>> {
    let data = map(path)?;
    let movie = crate::mp4::Movie::parse(&data).map_err(|_| Error::new("音频校验无法打开 MOV"))?;
    let count = movie
        .tracks
        .iter()
        .filter(|t| t.samples.handler == "soun")
        .count();
    drop(data);
    if count != expected {
        return Ok(Some(false));
    }
    if count > 32 {
        return Err(Error::new("音频校验轨道数量超过限制"));
    }
    if count == 0 {
        return Ok(Some(true));
    }
    init()?;
    for i in 0..count {
        match decode_audio(path, i)? {
            Audio::Frames(0) | Audio::Undecodable => return Ok(Some(false)),
            Audio::Frames(_) => {}
            Audio::Missing => return Ok(None),
        }
    }
    Ok(Some(true))
}
