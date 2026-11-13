//! Shared image/video codec boundary; no batch, live-photo or HDR policy.
use crate::{Error, ErrorKind, Result};
use std::path::Path;

/// Error for a step that needs the host's image or video codecs.
pub fn needs_native<T>(what: &str) -> Result<T> {
    Err(Error::with_kind(
        ErrorKind::Unsupported,
        format!("需要原生处理：{what}"),
    ))
}

/// An 8-bit single-channel image (gain maps).
#[derive(Clone, Debug, PartialEq)]
pub struct Gray {
    pub width: usize,
    pub height: usize,
    /// Bytes per row (>= width).
    pub stride: usize,
    pub pixels: Vec<u8>,
}

/// Opaque 8-bit sRGB RGBA, in JPEG coded-pixel row order (no EXIF rotation).
#[derive(Clone, Debug, PartialEq)]
pub struct Rgba {
    pub width: usize,
    pub height: usize,
    /// Bytes per row (>= width * 4), RGBA channel order; JPEG alpha is 255.
    pub stride: usize,
    pub pixels: Vec<u8>,
}

/// The HDR gain map ImageIO exposes for a HEIC (`kCGImageAuxiliaryDataTypeHDRGainMap`).
#[derive(Clone, Debug)]
pub struct HeicGain {
    /// `HDRGainMapVersion` values in the auxiliary metadata (Apple namespace).
    pub versions: Vec<String>,
    /// `HDRGainMapHeadroom` values.
    pub headrooms: Vec<f64>,
    /// None when the pixel format is not 8-bit one-component.
    pub image: Option<Gray>,
}

/// Host codecs and services. Every method has a refusing default, so a
/// host implements only what it supports.
pub trait Native: Sync {
    /// Decode audio through the target platform; None on unavailable backends.
    /// Silence is valid: this checks decodability, not perceived loudness.
    fn audio_decodes(&self, _path: &Path, _expected_tracks: usize) -> Result<Option<bool>> {
        Ok(None)
    }
    /// ImageIO-style full decode check of a still (complete, positive size).
    /// None = not available; callers then rely on the byte checks alone.
    fn image_decodes(&self, _data: &[u8]) -> Option<bool> {
        None
    }
    /// The sRGB ICC profile tagged onto a legacy-HDR still without one.
    fn srgb_icc(&self) -> Result<Vec<u8>> {
        needs_native("sRGB ICC 色彩描述")
    }
    /// Decode a JPEG gain/luma map; Ok(None) when it is not 8-bit monochrome.
    fn decode_gray_jpeg(&self, _jpeg: &[u8]) -> Result<Option<Gray>> {
        needs_native("解码 HDR 灰度资源")
    }
    /// Decode the primary JPEG into opaque sRGB RGBA in coded orientation.
    /// Kept separate from gray-map decoding to preserve private map code values.
    fn decode_srgb_jpeg(&self, _jpeg: &[u8]) -> Result<Option<Rgba>> {
        needs_native("解码 sRGB 主图像素")
    }
    /// Encode an 8-bit gray image as JPEG at `quality` (0..=1).
    fn encode_gray_jpeg(&self, _image: &Gray, _quality: f64) -> Result<Vec<u8>> {
        needs_native("编码增益图 JPEG")
    }
    /// Decode the still at `path` and re-encode it as JPEG at `quality`,
    /// keeping its image properties (EXIF, orientation, colour).
    fn reencode_jpeg(&self, _path: &Path, _quality: f64) -> Result<Vec<u8>> {
        needs_native("重新编码静态图")
    }
    /// The HDR gain map of a HEIC still, if ImageIO exposes one.
    fn heic_gain_map(&self, _path: &Path) -> Result<Option<HeicGain>> {
        needs_native("读取 HEIC HDR 增益图")
    }
    /// Decode one frame of video track `track` (0-based among video tracks)
    /// of the movie at `video`, at media time `time_seconds`, with the track
    /// transform applied, and encode it as JPEG at quality 1.0.
    /// Returns (jpeg, width, height).
    fn frame_jpeg(
        &self,
        _video: &Path,
        _track: usize,
        _time_seconds: f64,
    ) -> Result<(Vec<u8>, u32, u32)> {
        needs_native("解码辅助轨高清画面")
    }
}

/// No host codecs: pixel steps are refused, byte steps still run.
pub struct NoNative;
impl Native for NoNative {}
