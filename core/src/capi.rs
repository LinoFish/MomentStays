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
