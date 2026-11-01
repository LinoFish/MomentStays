//! C ABI for the engine: `ms_core_engine(native, request, token)`.
//!
//! The request is the same JSON as `momentstays-driver` (see
//! [`crate::engine::request`]); the reply is a [`crate::ffi`] response
//! (slot 0 = JSON). Host codecs and events come in through [`MsNative`], a
//! table of optional C callbacks. Variable-length results are written into
//! Rust-owned [`MsBuffer`]s with [`ms_buffer_append`].
//!
//! Threads: batch and scan work runs on several Rust worker threads, so
//! codec callbacks may be called concurrently and must be thread-safe.
//! `progress` and `rows_ready` are serialized. `existing_output` is called
//! by one worker at a time and may block (it waits for the user).

use crate::engine::batch::{ExistingOutput, Progress};
use crate::engine::request::{self, row_json, Events};
use crate::engine::scanrow::ScanRow;
use crate::engine::{needs_native, Gray, HeicGain, Native, StopToken};
use crate::{Error, Result};
use serde_json::{json, Value};
use std::ffi::{c_char, c_void, CString};
use std::path::Path;

/// A growable byte buffer owned by Rust, filled by host callbacks.
#[derive(Default)]
pub struct MsBuffer(Vec<u8>);

/// Append `len` bytes to `buffer`.
///
/// # Safety
/// `buffer` is a buffer passed to a callback during that callback, and
/// `data` points to `len` readable bytes (null only when `len` is 0).
#[no_mangle]
pub unsafe extern "C" fn ms_buffer_append(buffer: *mut MsBuffer, data: *const u8, len: usize) {
    if buffer.is_null() || (data.is_null() && len != 0) {
        return;
    }
    if len > 0 {
        (*buffer)
            .0
            .extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

/// A cancellation token for a running request.
#[no_mangle]
pub extern "C" fn ms_stop_new() -> *mut StopToken {
    Box::into_raw(Box::new(StopToken::new()))
}

/// Request cancellation; the running request stops at its next check.
///
/// # Safety
/// `token` is a live pointer from [`ms_stop_new`].
#[no_mangle]
pub unsafe extern "C" fn ms_stop_cancel(token: *const StopToken) {
    if let Some(t) = token.as_ref() {
        t.cancel();
    }
}

/// Release a token, after the request using it returned.
///
/// # Safety
/// Call once per token from [`ms_stop_new`]; null is a no-op.
#[no_mangle]
pub unsafe extern "C" fn ms_stop_free(token: *mut StopToken) {
    if !token.is_null() {
        drop(Box::from_raw(token));
    }
}

/// Callback status codes.
pub const MS_OK: i32 = 0;
/// The result is absent (Option::None), e.g. "not an 8-bit gray JPEG".
pub const MS_NONE: i32 = 1;
/// Failure; the error buffer holds a UTF-8 message.
pub const MS_ERROR: i32 = -1;

/// Host callbacks. Any entry may be null: the step is then refused with
/// "需要原生处理：…" (codecs) or skipped (events). Paths are NUL-terminated
/// UTF-8. Gray images are 8-bit, `width`×`height`, rows `stride` bytes apart.
#[repr(C)]
pub struct MsNative {
    pub context: *mut c_void,
    /// 1 = decodes completely with a positive size, 0 = does not, -1 = unknown.
    pub image_decodes: Option<unsafe extern "C" fn(*mut c_void, *const u8, usize) -> i32>,
    pub srgb_icc: Option<unsafe extern "C" fn(*mut c_void, *mut MsBuffer, *mut MsBuffer) -> i32>,
    /// (ctx, jpeg, len, pixels_out, dims_out[3] = width,height,stride, err) -> MS_OK / MS_NONE / MS_ERROR
    pub decode_gray_jpeg: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const u8,
            usize,
            *mut MsBuffer,
            *mut i64,
            *mut MsBuffer,
        ) -> i32,
    >,
    /// (ctx, pixels, len, width, height, stride, quality, jpeg_out, err)
    pub encode_gray_jpeg: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const u8,
            usize,
            i64,
            i64,
            i64,
            f64,
            *mut MsBuffer,
            *mut MsBuffer,
        ) -> i32,
    >,
    /// (ctx, path, quality, jpeg_out, err)
    pub reencode_jpeg: Option<
        unsafe extern "C" fn(*mut c_void, *const c_char, f64, *mut MsBuffer, *mut MsBuffer) -> i32,
    >,
    /// (ctx, path, info_json_out, pixels_out, err) -> MS_OK / MS_NONE (no gain map) / MS_ERROR.
    /// info: {"versions":[..],"headrooms":[..],"width":w,"height":h,"stride":s}
    /// (width/height/stride absent when the pixels are not 8-bit one-component).
    pub heic_gain_map: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            *mut MsBuffer,
            *mut MsBuffer,
            *mut MsBuffer,
        ) -> i32,
    >,
    /// (ctx, video_path, track, media_time_seconds, jpeg_out, dims_out[2], err)
    pub frame_jpeg: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            i64,
            f64,
            *mut MsBuffer,
            *mut i64,
            *mut MsBuffer,
        ) -> i32,
    >,
    /// (ctx, progress_json, len): {"index","total","relative","status","detail"}
    pub progress: Option<unsafe extern "C" fn(*mut c_void, *const u8, usize)>,
    /// (ctx, source, target) -> 0 skip, 1 overwrite, 2 cancel, -1 use the request policy
    pub existing_output:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> i32>,
    /// (ctx, rows_json, len): a JSON array of scan rows
    pub rows_ready: Option<unsafe extern "C" fn(*mut c_void, *const u8, usize)>,
}

struct Host(*const MsNative);
// SAFETY: the host promises (see module docs) that the table and its
// context stay valid for the call and that callbacks are thread-safe.
unsafe impl Send for Host {}
unsafe impl Sync for Host {}

fn cstring(p: &Path) -> Result<CString> {
    CString::new(p.to_string_lossy().into_owned()).map_err(|_| Error::new("路径包含 NUL 字符"))
}

fn status(code: i32, err: MsBuffer, what: &str) -> Result<bool> {
    match code {
        MS_OK => Ok(true),
        MS_NONE => Ok(false),
        _ => {
            let text = String::from_utf8_lossy(&err.0).into_owned();
            Err(Error::new(if text.is_empty() {
                format!("{what}失败")
            } else {
                text
            }))
        }
    }
}

impl Host {
    fn table(&self) -> &MsNative {
        // SAFETY: non-null, valid for the call (checked in ms_core_engine).
        unsafe { &*self.0 }
    }
}

impl Native for Host {
    fn image_decodes(&self, data: &[u8]) -> Option<bool> {
        let t = self.table();
        let f = t.image_decodes?;
        // SAFETY: data is valid for the call.
        match unsafe { f(t.context, data.as_ptr(), data.len()) } {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        }
    }
    fn srgb_icc(&self) -> Result<Vec<u8>> {
        let t = self.table();
        let Some(f) = t.srgb_icc else {
            return needs_native("sRGB ICC 色彩描述");
        };
        let (mut out, mut err) = (MsBuffer::default(), MsBuffer::default());
        let code = unsafe { f(t.context, &mut out, &mut err) };
        if !status(code, err, "读取 sRGB ICC")? {
            return needs_native("sRGB ICC 色彩描述");
        }
        Ok(out.0)
    }
    fn decode_gray_jpeg(&self, jpeg: &[u8]) -> Result<Option<Gray>> {
        let t = self.table();
        let Some(f) = t.decode_gray_jpeg else {
            return needs_native("解码旧 Apple 增益图");
        };
        let (mut out, mut err) = (MsBuffer::default(), MsBuffer::default());
        let mut dims = [0i64; 3];
        let code = unsafe {
            f(
                t.context,
                jpeg.as_ptr(),
                jpeg.len(),
                &mut out,
                dims.as_mut_ptr(),
                &mut err,
            )
        };
        if !status(code, err, "解码增益图")? {
            return Ok(None);
        }
        gray(out.0, dims[0], dims[1], dims[2]).map(Some)
    }
    fn encode_gray_jpeg(&self, image: &Gray, quality: f64) -> Result<Vec<u8>> {
        let t = self.table();
        let Some(f) = t.encode_gray_jpeg else {
            return needs_native("编码增益图 JPEG");
        };
        let (mut out, mut err) = (MsBuffer::default(), MsBuffer::default());
        let code = unsafe {
            f(
                t.context,
                image.pixels.as_ptr(),
                image.pixels.len(),
                image.width as i64,
                image.height as i64,
                image.stride as i64,
                quality,
                &mut out,
                &mut err,
            )
        };
        status(code, err, "编码增益图")?;
        Ok(out.0)
    }
    fn reencode_jpeg(&self, path: &Path, quality: f64) -> Result<Vec<u8>> {
        let t = self.table();
        let Some(f) = t.reencode_jpeg else {
            return needs_native("重新编码静态图");
        };
        let p = cstring(path)?;
        let (mut out, mut err) = (MsBuffer::default(), MsBuffer::default());
        let code = unsafe { f(t.context, p.as_ptr(), quality, &mut out, &mut err) };
        status(code, err, "重新编码静态图")?;
        Ok(out.0)
    }
    fn heic_gain_map(&self, path: &Path) -> Result<Option<HeicGain>> {
        let t = self.table();
        let Some(f) = t.heic_gain_map else {
            return needs_native("读取 HEIC HDR 增益图");
        };
        let p = cstring(path)?;
        let (mut info, mut pixels, mut err) = (
            MsBuffer::default(),
            MsBuffer::default(),
            MsBuffer::default(),
        );
        let code = unsafe { f(t.context, p.as_ptr(), &mut info, &mut pixels, &mut err) };
        if !status(code, err, "读取 HEIC 增益图")? {
            return Ok(None);
        }
        let v: Value =
            serde_json::from_slice(&info.0).map_err(|_| Error::new("HEIC 增益图信息无效"))?;
        let versions = v["versions"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let headrooms = v["headrooms"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        let image = match (
            v["width"].as_i64(),
            v["height"].as_i64(),
            v["stride"].as_i64(),
        ) {
            (Some(w), Some(h), Some(s)) => Some(gray(pixels.0, w, h, s)?),
            _ => None,
        };
        Ok(Some(HeicGain {
            versions,
            headrooms,
            image,
        }))
    }
    fn frame_jpeg(
        &self,
        video: &Path,
        track: usize,
        time_seconds: f64,
    ) -> Result<(Vec<u8>, u32, u32)> {
        let t = self.table();
        let Some(f) = t.frame_jpeg else {
            return needs_native("解码辅助轨高清画面");
        };
        let p = cstring(video)?;
        let (mut out, mut err) = (MsBuffer::default(), MsBuffer::default());
        let mut dims = [0i64; 2];
        let code = unsafe {
            f(
                t.context,
                p.as_ptr(),
                track as i64,
                time_seconds,
                &mut out,
                dims.as_mut_ptr(),
                &mut err,
            )
        };
        status(code, err, "解码高清画面")?;
        let (Ok(w), Ok(h)) = (u32::try_from(dims[0]), u32::try_from(dims[1])) else {
            return Err(Error::new("高清画面尺寸无效"));
        };
        Ok((out.0, w, h))
    }
}

fn gray(pixels: Vec<u8>, w: i64, h: i64, s: i64) -> Result<Gray> {
    let (Ok(width), Ok(height), Ok(stride)) =
        (usize::try_from(w), usize::try_from(h), usize::try_from(s))
    else {
        return Err(Error::new("增益图尺寸无效"));
    };
    if stride < width || pixels.len() < stride.saturating_mul(height) {
        return Err(Error::new("增益图数据被截断"));
    }
    Ok(Gray {
        width,
        height,
        stride,
        pixels,
    })
}

impl Events for Host {
    fn progress(&self, p: &Progress) {
        let t = self.table();
        if let Some(f) = t.progress {
            let text = json!({"index": p.index, "total": p.total, "relative": p.relative, "status": p.status, "detail": p.detail}).to_string();
            unsafe { f(t.context, text.as_ptr(), text.len()) };
        }
    }
    fn existing_output(&self, source: &Path, target: &Path) -> Option<ExistingOutput> {
        let t = self.table();
        let f = t.existing_output?;
        let (s, d) = (cstring(source).ok()?, cstring(target).ok()?);
        match unsafe { f(t.context, s.as_ptr(), d.as_ptr()) } {
            0 => Some(ExistingOutput::Skip),
            1 => Some(ExistingOutput::Overwrite),
            2 => Some(ExistingOutput::Cancel),
            _ => None,
        }
    }
    fn rows_ready(&self, rows: &[ScanRow]) {
        let t = self.table();
        if let Some(f) = t.rows_ready {
            let text = Value::from(rows.iter().map(row_json).collect::<Vec<_>>()).to_string();
            unsafe { f(t.context, text.as_ptr(), text.len()) };
        }
    }
}

/// Run one engine request (JSON, see `engine::request`).
///
/// # Safety
/// `native` is null or points to a table valid (with its context) until
/// this call returns, whose callbacks follow the contracts above.
/// `request` points to `len` readable bytes. `token` is null or a live
/// token from [`ms_stop_new`]. The response is owned by the caller and
/// released with [`crate::ffi::ms_core_free`].
#[no_mangle]
pub unsafe extern "C" fn ms_core_engine(
    native: *const MsNative,
    request: *const u8,
    len: usize,
    token: *const StopToken,
) -> *mut crate::ffi::Response {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Value> {
        if len > isize::MAX as usize || (request.is_null() && len != 0) {
            return Err(Error::new("Rust bridge: invalid input buffer"));
        }
        let bytes = if len == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(request, len)
        };
        let req: Value =
            serde_json::from_slice(bytes).map_err(|_| Error::new("Missing request"))?;
        let own = StopToken::new();
        let token = token.as_ref().unwrap_or(&own);
        if native.is_null() {
            return request::handle(&crate::engine::NoNative, &request::NoEvents, token, &req);
        }
        let host = Host(native);
        request::handle(&host, &host, token, &req)
    }));
    let value = match result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => json!({"error": e.0, "error_kind": e.kind().code()}),
        Err(_) => json!({"error": "Rust core panic; request rejected"}),
    };
    Box::into_raw(Box::new(crate::ffi::Response::json(value)))
}
