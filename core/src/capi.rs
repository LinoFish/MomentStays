//! Minimal C ABI (ROADMAP item 10): `inspect` and `parse` over FFI.
//!
//! Safety contract, mirrored in `include/momentstays_core.h`:
//! - `momentstays_version` returns a static NUL-terminated string that
//!   must never be freed.
//! - `momentstays_inspect` returns 0 on success and stores a freshly
//!   allocated NUL-terminated JSON string in `*out_json`; the caller
//!   frees it with `momentstays_free_string`. Any other return value is
//!   a failure and `*out_json` is left untouched (NULL input pointers
//!   are a failure, not a crash).
//! - `momentstays_parse` is the old op-0 bridge semantics in the new
//!   shape: on a supported motion photo it returns 0 with the parse
//!   JSON (video_start/video_end/timestamp/timestamp_tag/ultra/
//!   cover_frame/format) plus the still bytes in a fresh buffer freed
//!   by `momentstays_free_bytes`. On an unsupported file it still
//!   returns 0, but the JSON carries an `error` key and the still
//!   buffer is NULL/0. NULL out-pointers, or a NULL input pointer
//!   with nonzero length, are a hard failure (nonzero, outputs
//!   untouched); a NULL input pointer with zero length is treated as
//!   empty input, which parses to an `error` JSON like the old bridge.
//! - `momentstays_coding_hash` / `momentstays_auxiliary_hashes` are
//!   the old op-1/op-2 bridge semantics, calling `format::coding_hash`
//!   / `format::auxiliary_hashes` directly on the input bytes. They
//!   must NOT be served by `inspect`: the app calls them on still-only
//!   bytes, where `inspect`'s parse fails and would report ok:false.
//!   Success JSON is {"hash": ...} / {"hashes": [...]}, failure JSON
//!   carries an `error` key (both rc 0); hard failures are nonzero.
//! - A Rust panic inside `inspect`/`parse` is caught at this boundary
//!   and reported as failure: a panic must never unwind across FFI,
//!   which would be undefined behavior.
//! - No Swift bridge lives here; the Swift side (native/RustCore.swift)
//!   calls these functions, and its compile check is a Needs Mac check.

use std::ffi::{c_char, CString};

static VERSION_C: &[u8] = concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes();

/// Library version string (Cargo package version). Static; do not free.
#[no_mangle]
pub extern "C" fn momentstays_version() -> *const c_char {
    VERSION_C.as_ptr() as *const c_char
}

/// Inspect a file's bytes and return the `inspect` JSON summary.
/// Returns 0 on success, nonzero on failure (NULL pointers, allocation
/// failure, or a caught panic). The JSON string is heap-allocated;
/// free it with `momentstays_free_string`.
#[no_mangle]
pub extern "C" fn momentstays_inspect(
    data: *const u8,
    len: usize,
    out_json: *mut *mut c_char,
) -> i32 {
    if data.is_null() || out_json.is_null() {
        return 1;
    }
    // SAFETY: `data`/`len` come from the caller and are trusted to
    // describe a readable buffer, like any C API. A zero length with a
    // non-NULL pointer is fine (`from_raw_parts` allows it).
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    let value = match std::panic::catch_unwind(|| crate::inspect(bytes)) {
        Ok(v) => v,
        Err(_) => return 2,
    };
    match CString::new(value.to_string()) {
        Ok(s) => {
            unsafe { *out_json = s.into_raw() };
            0
        }
        Err(_) => 3,
    }
}

/// Free a string returned by `momentstays_inspect` or
/// `momentstays_parse`. NULL is a no-op.
#[no_mangle]
pub extern "C" fn momentstays_free_string(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    // SAFETY: `s` must be a string previously returned by
    // `momentstays_inspect`/`momentstays_parse` and not freed before;
    // anything else is a caller bug, like `free` on a foreign pointer.
    unsafe { let _ = CString::from_raw(s); }
}

/// Parse a motion photo: JSON summary plus the still bytes.
/// Returns 0 with the parse JSON (video_start/video_end/timestamp/
/// timestamp_tag/ultra/cover_frame/format, same keys as the old op-0
/// bridge) and the still bytes in `*out_still`/`*out_still_len`, freed
/// by `momentstays_free_bytes`. An unsupported file still returns 0,
/// but the JSON carries an `error` key and the still is NULL/0.
/// NULL out-pointers, or a NULL input pointer with nonzero length,
/// return nonzero with outputs untouched; a NULL input pointer with
/// zero length is treated as empty input. A caught panic returns 2.
#[no_mangle]
pub extern "C" fn momentstays_parse(
    data: *const u8,
    len: usize,
    out_json: *mut *mut c_char,
    out_still: *mut *const u8,
    out_still_len: *mut usize,
) -> i32 {
    if out_json.is_null() || out_still.is_null() || out_still_len.is_null() {
        return 1;
    }
    if data.is_null() && len != 0 {
        return 1;
    }
    // SAFETY: `data`/`len` come from the caller and are trusted to
    // describe a readable buffer, like any C API. A zero length with
    // a NULL pointer is empty input (old bridge behavior).
    let bytes: &[u8] = if len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    let parsed = match std::panic::catch_unwind(|| crate::format::parse(bytes)) {
        Ok(v) => v,
        Err(_) => return 2,
    };
    let (json, still_vec) = match parsed {
        Ok(p) => (
            serde_json::json!({
                "video_start": p.video.start,
                "video_end": p.video.end,
                "timestamp": p.timestamp,
                "timestamp_tag": p.timestamp_tag,
                "ultra": p.ultra,
                "cover_frame": p.cover_frame,
                "format": p.format,
            }),
            Some(p.still.into_owned()),
        ),
        Err(e) => (serde_json::json!({ "error": e.0 }), None),
    };
    match CString::new(json.to_string()) {
        Ok(s) => {
            unsafe { *out_json = s.into_raw() };
            match still_vec {
                Some(v) if !v.is_empty() => {
                    let mut v = v;
                    let l = v.len();
                    let ptr = v.as_mut_ptr();
                    std::mem::forget(v);
                    unsafe {
                        *out_still = ptr;
                        *out_still_len = l;
                    }
                }
                _ => unsafe {
                    *out_still = std::ptr::null();
                    *out_still_len = 0;
                },
            }
            0
        }
        Err(_) => 3,
    }
}

/// Free still bytes returned by `momentstays_parse`. NULL is a no-op.
/// `len` must be the length reported alongside the pointer.
#[no_mangle]
pub extern "C" fn momentstays_free_bytes(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: `ptr`/`len` must be a buffer previously returned by
    // `momentstays_parse` and not freed before; anything else is a
    // caller bug, like `free` on a foreign pointer.
    unsafe {
        let _ = Vec::from_raw_parts(ptr, len, len);
    }
}

/// Shared helper for the JSON-string hash functions: run `f` on the
/// input bytes, render {"hash"|"hashes": ...} or {"error": ...} JSON.
/// NULL out-pointer, or NULL input with nonzero length, is a hard
/// failure (nonzero, output untouched); NULL input with zero length
/// is empty input. A caught panic returns 2.
fn hash_json<F>(data: *const u8, len: usize, out_json: *mut *mut c_char, f: F) -> i32
where
    F: FnOnce(&[u8]) -> crate::Result<serde_json::Value> + std::panic::UnwindSafe,
{
    if out_json.is_null() {
        return 1;
    }
    if data.is_null() && len != 0 {
        return 1;
    }
    // SAFETY: same buffer contract as `momentstays_parse`.
    let bytes: &[u8] = if len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(data, len) }
    };
    let value = match std::panic::catch_unwind(|| f(bytes)) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => serde_json::json!({ "error": e.0 }),
        Err(_) => return 2,
    };
    match CString::new(value.to_string()) {
        Ok(s) => {
            unsafe { *out_json = s.into_raw() };
            0
        }
        Err(_) => 3,
    }
}

/// Coding hash of the input bytes (old op-1 semantics). Works on
/// still-only bytes as well as full motion files; the app verifies
/// stills with it, so it must not require a successful parse.
#[no_mangle]
pub extern "C" fn momentstays_coding_hash(
    data: *const u8,
    len: usize,
    out_json: *mut *mut c_char,
) -> i32 {
    hash_json(data, len, out_json, |b| {
        Ok(serde_json::json!({ "hash": crate::format::coding_hash(b)? }))
    })
}

/// Auxiliary (gain-map) hashes of the input bytes (old op-2 semantics).
/// HEIC input yields an empty list, like the old bridge.
#[no_mangle]
pub extern "C" fn momentstays_auxiliary_hashes(
    data: *const u8,
    len: usize,
    out_json: *mut *mut c_char,
) -> i32 {
    hash_json(data, len, out_json, |b| {
        Ok(serde_json::json!({ "hashes": crate::format::auxiliary_hashes(b)? }))
    })
}

// ---------------------------------------------------------------------------
// Conversion entries (wiring): the app's conversion paths over FFI.
// Contract additions, same shape as `momentstays_parse`:
// - Every entry returns 0 with a JSON document whose `status` is "ok",
//   "needs_pixel_work" (only the conversion entries that can ask the app
//   to do a pixel step and retry), or "error" (message in `error`).
//   Byte outputs are NULL/0 unless status is "ok".
// - NULL out-pointers, or a NULL input pointer with nonzero length, are a
//   hard failure (nonzero, outputs untouched); a NULL input pointer with
//   zero length is empty input, which fails softly like any bad input.
// - A Rust panic inside an entry is caught and reported as 2.
// - Options arrive as a JSON C string (NULL = defaults where the entry
//   allows it, else a soft error); malformed JSON is a soft error.
// ---------------------------------------------------------------------------

fn input_bytes<'a>(data: *const u8, len: usize) -> Option<&'a [u8]> {
    if data.is_null() && len != 0 {
        return None;
    }
    Some(if len == 0 {
        &[]
    } else {
        // SAFETY: same buffer contract as `momentstays_parse`.
        unsafe { std::slice::from_raw_parts(data, len) }
    })
}

fn write_json(out_json: *mut *mut c_char, value: serde_json::Value) -> i32 {
    match CString::new(value.to_string()) {
        Ok(s) => {
            unsafe { *out_json = s.into_raw() };
            0
        }
        Err(_) => 3,
    }
}

fn write_bytes(out_ptr: *mut *const u8, out_len: *mut usize, v: Vec<u8>) {
    if v.is_empty() {
        unsafe {
            *out_ptr = std::ptr::null();
            *out_len = 0;
        }
        return;
    }
    let mut v = v;
    let l = v.len();
    let ptr = v.as_mut_ptr();
    std::mem::forget(v);
    unsafe {
        *out_ptr = ptr;
        *out_len = l;
    }
}

fn opt_string(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(|s| s.to_string())
}

fn opt_i64(v: &serde_json::Value, key: &str) -> Option<i64> {
    v.get(key)?.as_i64()
}

fn opt_usize(v: &serde_json::Value, key: &str) -> Option<usize> {
    v.get(key)?.as_u64().map(|n| n as usize)
}

fn opt_str_or<'a>(v: &'a serde_json::Value, key: &str, default: &'a str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or(default)
}

fn parse_opts(ptr: *const c_char) -> Result<serde_json::Value, String> {
    if ptr.is_null() {
        return Ok(serde_json::json!({}));
    }
    // SAFETY: `ptr` is trusted to be a NUL-terminated C string.
    let s = unsafe { std::ffi::CStr::from_ptr(ptr) };
    match s.to_str() {
        Ok(t) => serde_json::from_str(t).map_err(|e| e.to_string()),
        Err(_) => Err("选项不是有效 UTF-8".to_string()),
    }
}

fn cover_json(m: &crate::cover::CoverMapping) -> serde_json::Value {
    serde_json::json!({
        "frame": m.frame,
        "frame_time_us": m.frame_time_us,
        "frame_duration_us": m.frame_duration_us,
        "mapped_us": m.mapped_us,
        "domain": m.domain,
        "policy": m.policy,
        "error_us": m.error_us,
    })
}

/// Android Motion Photo -> Apple Live Photo pair.
/// `opts_json`: uuid (required), timestamp_us?, cover_frame_index?,
/// timestamp_source?, time_domain? (default "auto"), video_track?
/// (default "primary"), missing_time? (default "error"),
/// still_override_kind? ("preconverted-ultra" | "candidate").
/// `still_override` (NULL = none) replaces the motion file's still: a
/// decoded candidate JPEG or a pixel-preconverted ultra still.
#[no_mangle]
pub extern "C" fn momentstays_to_live(
    motion: *const u8,
    motion_len: usize,
    opts_json: *const c_char,
    still_override: *const u8,
    still_override_len: usize,
    has_override: i32,
    out_json: *mut *mut c_char,
    out_still: *mut *const u8,
    out_still_len: *mut usize,
    out_mov: *mut *const u8,
    out_mov_len: *mut usize,
) -> i32 {
    if out_json.is_null()
        || out_still.is_null()
        || out_still_len.is_null()
        || out_mov.is_null()
        || out_mov_len.is_null()
    {
        return 1;
    }
    let motion_bytes = match input_bytes(motion, motion_len) {
        Some(b) => b,
        None => return 1,
    };
    let override_bytes = if has_override != 0 {
        match input_bytes(still_override, still_override_len) {
            Some(b) => Some(b),
            None => return 1,
        }
    } else {
        None
    };
    let opts = match parse_opts(opts_json) {
        Ok(o) => o,
        Err(e) => return write_json(out_json, serde_json::json!({"status": "error", "error": e})),
    };
    let to_live_opts = crate::convert::ToLiveOpts {
        uuid: match opt_string(&opts, "uuid") {
            Some(u) => u,
            None => {
                return write_json(
                    out_json,
                    serde_json::json!({"status": "error", "error": "缺少内容标识 uuid"}),
                )
            }
        },
        timestamp_us: opt_i64(&opts, "timestamp_us"),
        cover_frame_index: opt_usize(&opts, "cover_frame_index"),
        timestamp_source: opt_string(&opts, "timestamp_source"),
        time_domain: opt_str_or(&opts, "time_domain", "auto").to_string(),
        video_track: opt_str_or(&opts, "video_track", "primary").to_string(),
        missing_time: opt_str_or(&opts, "missing_time", "error").to_string(),
        still_override_kind: opt_string(&opts, "still_override_kind"),
    };
    let result = match std::panic::catch_unwind(|| {
        crate::convert::to_live(motion_bytes, &to_live_opts, override_bytes)
    }) {
        Ok(v) => v,
        Err(_) => return 2,
    };
    match result {
        Ok(crate::convert::ConvertOutcome::Done(o)) => {
            let json = serde_json::json!({
                "status": "ok",
                "cover": cover_json(&o.cover),
                "hdr": {
                    "input": o.hdr_input,
                    "status": o.hdr_status,
                    "reason": o.hdr_reason,
                    "output": o.hdr_output,
                },
                "video_tracks": o.video_tracks,
            });
            let rc = write_json(out_json, json);
            if rc == 0 {
                write_bytes(out_still, out_still_len, o.still);
                write_bytes(out_mov, out_mov_len, o.mov);
            }
            rc
        }
        Ok(crate::convert::ConvertOutcome::NeedsPixelWork(w)) => write_json(
            out_json,
            serde_json::json!({
                "status": "needs_pixel_work",
                "reason": w.reason,
                "stops": w.stops,
                "detail": w.detail,
            }),
        ),
        Err(e) => write_json(
            out_json,
            serde_json::json!({"status": "error", "error": e.0}),
        ),
    }
}

/// Apple Live Photo -> Android Motion Photo.
/// `opts_json`: vendor (required: google/xiaomi/samsung/huawei/oppo/vivo/
/// honor), container? (default "jpeg"), cover_time_us?, cover_frame?,
/// preserve_jpeg? (default true).
#[no_mangle]
pub extern "C" fn momentstays_to_motion(
    still: *const u8,
    still_len: usize,
    movie: *const u8,
    movie_len: usize,
    opts_json: *const c_char,
    out_json: *mut *mut c_char,
    out_bytes: *mut *const u8,
    out_bytes_len: *mut usize,
) -> i32 {
    if out_json.is_null() || out_bytes.is_null() || out_bytes_len.is_null() {
        return 1;
    }
    let still_bytes = match input_bytes(still, still_len) {
        Some(b) => b,
        None => return 1,
    };
    let movie_bytes = match input_bytes(movie, movie_len) {
        Some(b) => b,
        None => return 1,
    };
    let opts = match parse_opts(opts_json) {
        Ok(o) => o,
        Err(e) => return write_json(out_json, serde_json::json!({"status": "error", "error": e})),
    };
    let to_motion_opts = crate::convert::ToMotionOpts {
        vendor: match opt_string(&opts, "vendor") {
            Some(v) => v,
            None => {
                return write_json(
                    out_json,
                    serde_json::json!({"status": "error", "error": "缺少目标厂商 vendor"}),
                )
            }
        },
        container: opt_str_or(&opts, "container", "jpeg").to_string(),
        cover_time_us: opt_i64(&opts, "cover_time_us"),
        cover_frame: opt_i64(&opts, "cover_frame"),
        preserve_jpeg: opts
            .get("preserve_jpeg")
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
    };
    let result = match std::panic::catch_unwind(|| {
        crate::convert::to_motion(still_bytes, movie_bytes, &to_motion_opts)
    }) {
        Ok(v) => v,
        Err(_) => return 2,
    };
    match result {
        Ok(crate::convert::ConvertOutcome::Done(o)) => {
            let json = serde_json::json!({
                "status": "ok",
                "hdr": {
                    "input": o.hdr_input,
                    "status": o.hdr_status,
                    "reason": o.hdr_reason,
                    "output": o.hdr_output,
                },
                "cover_time_us": o.cover_time_us,
                "cover_frame": o.cover_frame,
            });
            let rc = write_json(out_json, json);
            if rc == 0 {
                write_bytes(out_bytes, out_bytes_len, o.bytes);
            }
            rc
        }
        Ok(crate::convert::ConvertOutcome::NeedsPixelWork(w)) => write_json(
            out_json,
            serde_json::json!({
                "status": "needs_pixel_work",
                "reason": w.reason,
                "stops": w.stops,
                "detail": w.detail,
            }),
        ),
        Err(e) => write_json(
            out_json,
            serde_json::json!({"status": "error", "error": e.0}),
        ),
    }
}

/// Attach a pixel-converted gain JPEG to a base still (Swift's
/// `colorProfile` + `ultra` steps, byte-level). `srgb_icc` (NULL = none)
/// is inserted first when the base lacks an ICC profile and declares
/// EXIF sRGB.
#[no_mangle]
pub extern "C" fn momentstays_attach_gain(
    base: *const u8,
    base_len: usize,
    gain: *const u8,
    gain_len: usize,
    stops: f64,
    srgb_icc: *const u8,
    srgb_icc_len: usize,
    has_icc: i32,
    out_json: *mut *mut c_char,
    out_bytes: *mut *const u8,
    out_bytes_len: *mut usize,
) -> i32 {
    if out_json.is_null() || out_bytes.is_null() || out_bytes_len.is_null() {
        return 1;
    }
    let base_bytes = match input_bytes(base, base_len) {
        Some(b) => b,
        None => return 1,
    };
    let gain_bytes = match input_bytes(gain, gain_len) {
        Some(b) => b,
        None => return 1,
    };
    let icc = if has_icc != 0 {
        match input_bytes(srgb_icc, srgb_icc_len) {
            Some(b) => Some(b),
            None => return 1,
        }
    } else {
        None
    };
    let result = match std::panic::catch_unwind(|| {
        crate::convert::attach_gain_for_cabi(base_bytes, gain_bytes, stops, icc)
    }) {
        Ok(v) => v,
        Err(_) => return 2,
    };
    match result {
        Ok(bytes) => {
            let rc = write_json(out_json, serde_json::json!({"status": "ok"}));
            if rc == 0 {
                write_bytes(out_bytes, out_bytes_len, bytes);
            }
            rc
        }
        Err(e) => write_json(out_json, serde_json::json!({"status": "error", "error": e.0})),
    }
}

/// Live Photo still cleaning / identifier injection, mirroring Swift
/// `PhotoFormat.liveStill`. JPEG: update_identifier != 0 sets the EXIF
/// MakerNote identifier, else clean only. HEIC always sets the identifier
/// (Swift's HEIC path ignores the flag).
#[no_mangle]
pub extern "C" fn momentstays_live_still(
    still: *const u8,
    still_len: usize,
    uuid: *const c_char,
    update_identifier: i32,
    out_json: *mut *mut c_char,
    out_bytes: *mut *const u8,
    out_bytes_len: *mut usize,
) -> i32 {
    if uuid.is_null() || out_json.is_null() || out_bytes.is_null() || out_bytes_len.is_null()
    {
        return 1;
    }
    let still_bytes = match input_bytes(still, still_len) {
        Some(b) => b,
        None => return 1,
    };
    // SAFETY: `uuid` is trusted to be a NUL-terminated C string.
    let uuid_str = unsafe { std::ffi::CStr::from_ptr(uuid) };
    let uuid_str = match uuid_str.to_str() {
        Ok(s) => s,
        Err(_) => {
            return write_json(
                out_json,
                serde_json::json!({"status": "error", "error": "uuid 不是有效 UTF-8"}),
            )
        }
    };
    let result = match std::panic::catch_unwind(|| {
        if crate::heif::matches(still_bytes) {
            crate::heifwrite::live_still_heic(still_bytes, uuid_str)
        } else if update_identifier != 0 {
            crate::pymotion::live_still_with_identifier(still_bytes, uuid_str)
        } else {
            crate::pymotion::live_still_clean(still_bytes)
        }
    }) {
        Ok(v) => v,
        Err(_) => return 2,
    };
    match result {
        Ok(bytes) => {
            let rc = write_json(out_json, serde_json::json!({"status": "ok"}));
            if rc == 0 {
                write_bytes(out_bytes, out_bytes_len, bytes);
            }
            rc
        }
        Err(e) => write_json(out_json, serde_json::json!({"status": "error", "error": e.0})),
    }
}

/// One scan row for a file's bytes, mirroring Swift `BatchEngine.scanFile`
/// (to-apple direction). Always status "ok": unparseable files become
/// error rows inside the JSON, like the Swift scanner.
#[no_mangle]
pub extern "C" fn momentstays_scan_row(
    data: *const u8,
    len: usize,
    out_json: *mut *mut c_char,
) -> i32 {
    if out_json.is_null() {
        return 1;
    }
    let bytes = match input_bytes(data, len) {
        Some(b) => b,
        None => return 1,
    };
    let row = match std::panic::catch_unwind(|| crate::scan::scan_row(bytes)) {
        Ok(r) => r,
        Err(_) => return 2,
    };
    write_json(
        out_json,
        serde_json::json!({
            "status": "ok",
            "is_motion": row.is_motion,
            "hdr": row.hdr,
            "cover": row.cover,
            "detail": row.detail,
            "manufacturer": row.manufacturer,
            "manufacturer_detail": row.manufacturer_detail,
            "confirmed_static": row.confirmed_static,
        }),
    )
}

/// Edit-list-aware cover mapping, mirroring Swift `sourceCoverTiming`.
/// `opts_json`: cover_frame_index?, timestamp_us?, timestamp_source?,
/// time_domain? (default "auto"), video_track? (default "primary"),
/// missing_time? (default "error").
#[no_mangle]
pub extern "C" fn momentstays_cover_map(
    video: *const u8,
    video_len: usize,
    opts_json: *const c_char,
    out_json: *mut *mut c_char,
) -> i32 {
    if out_json.is_null() {
        return 1;
    }
    let video_bytes = match input_bytes(video, video_len) {
        Some(b) => b,
        None => return 1,
    };
    let opts = match parse_opts(opts_json) {
        Ok(o) => o,
        Err(e) => return write_json(out_json, serde_json::json!({"status": "error", "error": e})),
    };
    let req = crate::cover::CoverRequest {
        cover_frame_index: opt_usize(&opts, "cover_frame_index"),
        timestamp_us: opt_i64(&opts, "timestamp_us"),
        timestamp_source: opt_string(&opts, "timestamp_source"),
        time_domain: opt_str_or(&opts, "time_domain", "auto").to_string(),
        video_track: opt_str_or(&opts, "video_track", "primary").to_string(),
        missing_time: opt_str_or(&opts, "missing_time", "error").to_string(),
    };
    let result = match std::panic::catch_unwind(|| crate::cover::map_cover(video_bytes, &req)) {
        Ok(v) => v,
        Err(_) => return 2,
    };
    match result {
        Ok(m) => write_json(
            out_json,
            serde_json::json!({"status": "ok", "cover": cover_json(&m)}),
        ),
        Err(e) => write_json(out_json, serde_json::json!({"status": "error", "error": e.0})),
    }
}

/// Auxiliary video track -> silent MOV, mirroring Swift
/// `PhotoFormat.exportAuxiliary` (zero-duration repair included).
#[no_mangle]
pub extern "C" fn momentstays_export_auxiliary(
    motion: *const u8,
    motion_len: usize,
    out_json: *mut *mut c_char,
    out_mov: *mut *const u8,
    out_mov_len: *mut usize,
) -> i32 {
    if out_json.is_null() || out_mov.is_null() || out_mov_len.is_null() {
        return 1;
    }
    let motion_bytes = match input_bytes(motion, motion_len) {
        Some(b) => b,
        None => return 1,
    };
    let result = match std::panic::catch_unwind(|| crate::convert::export_auxiliary(motion_bytes))
    {
        Ok(v) => v,
        Err(_) => return 2,
    };
    match result {
        Ok(o) => {
            let json = serde_json::json!({
                "status": "ok",
                "repaired": o.repaired,
                "repair_method": o.repair_method,
                "repaired_duration_seconds": o.repaired_duration_seconds,
                "original_sample_sha256": o.original_sample_sha256,
            });
            let rc = write_json(out_json, json);
            if rc == 0 {
                write_bytes(out_mov, out_mov_len, o.mov);
            }
            rc
        }
        Err(e) => write_json(out_json, serde_json::json!({"status": "error", "error": e.0})),
    }
}

/// Google -> Honor vendor conversion, mirroring Swift
/// `AndroidTransfer.convert` (strict acceptance + assembly + parse-back
/// validation). The CGImageSource decode check and the XMP metadata-tree
/// post-check stay in the app: the first needs system decode, the second
/// is verification, not conversion.
#[no_mangle]
pub extern "C" fn momentstays_vendor_convert(
    motion: *const u8,
    motion_len: usize,
    target: *const c_char,
    out_json: *mut *mut c_char,
    out_bytes: *mut *const u8,
    out_bytes_len: *mut usize,
) -> i32 {
    if target.is_null() || out_json.is_null() || out_bytes.is_null() || out_bytes_len.is_null()
    {
        return 1;
    }
    let motion_bytes = match input_bytes(motion, motion_len) {
        Some(b) => b,
        None => return 1,
    };
    // SAFETY: `target` is trusted to be a NUL-terminated C string.
    let target_str = unsafe { std::ffi::CStr::from_ptr(target) };
    let target_str = match target_str.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => {
            return write_json(
                out_json,
                serde_json::json!({"status": "error", "error": "目标厂商不是有效 UTF-8"}),
            )
        }
    };
    let result = match std::panic::catch_unwind(|| {
        crate::convert::vendor_convert(motion_bytes, &target_str)
    }) {
        Ok(v) => v,
        Err(_) => return 2,
    };
    match result {
        Ok(o) => {
            let json = serde_json::json!({
                "status": "ok",
                "cover_time_us": o.cover_time_us,
            });
            let rc = write_json(out_json, json);
            if rc == 0 {
                write_bytes(out_bytes, out_bytes_len, o.bytes);
            }
            rc
        }
        Err(e) => write_json(out_json, serde_json::json!({"status": "error", "error": e.0})),
    }
}

/// Per-frame candidate timestamps, mirroring Swift
/// `CoverCandidateSession` (timing only; JPEG rendering stays in Swift).
#[no_mangle]
pub extern "C" fn momentstays_candidates(
    video: *const u8,
    video_len: usize,
    out_json: *mut *mut c_char,
) -> i32 {
    if out_json.is_null() {
        return 1;
    }
    let video_bytes = match input_bytes(video, video_len) {
        Some(b) => b,
        None => return 1,
    };
    let result = match std::panic::catch_unwind(|| crate::convert::candidate_times(video_bytes))
    {
        Ok(v) => v,
        Err(_) => return 2,
    };
    match result {
        Ok((candidates, repaired)) => {
            let list: Vec<serde_json::Value> = candidates
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "index": c.index,
                        "timestamp_us": c.timestamp_us,
                        "time_source": c.time_source,
                        "sample_time_us": c.sample_time_us,
                    })
                })
                .collect();
            write_json(
                out_json,
                serde_json::json!({
                    "status": "ok",
                    "repaired": repaired,
                    "candidates": list,
                }),
            )
        }
        Err(e) => write_json(out_json, serde_json::json!({"status": "error", "error": e.0})),
    }
}

/// Live Photo still+movie pairing, mirroring Swift `AndroidExport.pair()`
/// (content identifiers + edit-list-aware cover + MakerNote 33 presence).
/// `time_domain` (NULL = "auto") is the cover-time interpretation domain
/// ("auto" | "media" | "presentation"), matching the app's time-domain option.
#[no_mangle]
pub extern "C" fn momentstays_live_pair(
    still: *const u8,
    still_len: usize,
    movie: *const u8,
    movie_len: usize,
    time_domain: *const c_char,
    out_json: *mut *mut c_char,
) -> i32 {
    if out_json.is_null() {
        return 1;
    }
    let still_bytes = match input_bytes(still, still_len) {
        Some(b) => b,
        None => return 1,
    };
    let movie_bytes = match input_bytes(movie, movie_len) {
        Some(b) => b,
        None => return 1,
    };
    // NULL time_domain means "auto"; a non-UTF-8 value is a soft error.
    let domain: String = if time_domain.is_null() {
        "auto".to_string()
    } else {
        // SAFETY: `time_domain` is trusted to be a NUL-terminated C string.
        match unsafe { std::ffi::CStr::from_ptr(time_domain) }.to_str() {
            Ok(t) => t.to_string(),
            Err(_) => {
                return write_json(
                    out_json,
                    serde_json::json!({"status": "error", "error": "时间域不是有效 UTF-8"}),
                )
            }
        }
    };
    let result = match std::panic::catch_unwind(|| {
        crate::convert::live_pair(still_bytes, movie_bytes, &domain)
    }) {
        Ok(v) => v,
        Err(_) => return 2,
    };
    match result {
        Ok(p) => write_json(
            out_json,
            serde_json::json!({
                "status": "ok",
                "still_identifier": p.still_identifier,
                "movie_identifier": p.movie_identifier,
                "cover_time_us": p.cover_time_us,
                "cover_frame": p.cover_frame,
                "domain": p.domain,
                "policy": p.policy,
                "maker33": p.maker33,
            }),
        ),
        Err(e) => write_json(out_json, serde_json::json!({"status": "error", "error": e.0})),
    }
}

