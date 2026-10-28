//! Minimal C ABI (ROADMAP item 10): `inspect` over FFI, nothing more.
//!
//! Safety contract, mirrored in `include/momentstays_core.h`:
//! - `momentstays_version` returns a static NUL-terminated string that
//!   must never be freed.
//! - `momentstays_inspect` returns 0 on success and stores a freshly
//!   allocated NUL-terminated JSON string in `*out_json`; the caller
//!   frees it with `momentstays_free_string`. Any other return value is
//!   a failure and `*out_json` is left untouched (NULL input pointers
//!   are a failure, not a crash).
//! - A Rust panic inside `inspect` (hostile input can still reach
//!   slicing code the fuzzer item has not hardened yet) is caught at
//!   this boundary and reported as failure: a panic must never unwind
//!   across FFI, which would be undefined behavior.
//! - No Swift bridge lives here; that part is a Needs Mac check.

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

/// Free a string returned by `momentstays_inspect`. NULL is a no-op.
#[no_mangle]
pub extern "C" fn momentstays_free_string(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    // SAFETY: `s` must be a string previously returned by
    // `momentstays_inspect` and not freed before; anything else is a
    // caller bug, like `free` on a foreign pointer.
    unsafe { let _ = CString::from_raw(s); }
}
