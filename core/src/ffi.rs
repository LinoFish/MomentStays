//! Owned C ABI responses. Input is borrowed for the call; output lives until free.
use serde_json::json;
use std::panic::{catch_unwind, AssertUnwindSafe};

pub struct Response {
    json: Vec<u8>,
    still: Vec<u8>,
}

fn error(message: &str) -> Response {
    Response {
        json: json!({"error": message}).to_string().into_bytes(),
        still: vec![],
    }
}

fn respond(operation: u32, data: &[u8]) -> Response {
    let result = (|| -> crate::Result<Response> {
        let mut still = vec![];
        let value = match operation {
            0 => {
                let p = crate::format::parse(data)?;
                let value = json!({"video_start":p.video.start,"video_end":p.video.end,
                    "timestamp":p.timestamp,"timestamp_tag":p.timestamp_tag,"ultra":p.ultra,
                    "cover_frame":p.cover_frame,"format":p.format});
                // The response owns its still: it must outlive the borrowed input.
                still = p.still.into_owned();
                value
            }
            1 => json!({"hash":crate::format::coding_hash(data)?}),
            2 => json!({"hashes":crate::format::auxiliary_hashes(data)?}),
            3 => {
                // Apple HDR facts of a JPEG or HEIC still. Slot 1 carries the
                // legacy gain-map JPEG when there is one.
                let r = crate::apple_hdr::read(data)?;
                still = r.gain.clone().unwrap_or_default();
                r.to_json()
            }
            _ => return crate::fail("Rust bridge: unknown operation"),
        };
        Ok(Response {
            json: value.to_string().into_bytes(),
            still,
        })
    })();
    result.unwrap_or_else(|e| error(&e.0))
}

/// Run `operation` (0 parse, 1 coding hash, 2 auxiliary hashes, 3 Apple HDR
/// read) on `input`.
///
/// # Safety
/// `input` points to `len` readable bytes for the duration of the call
/// (it may be null only when `len` is 0). The returned response is owned by
/// the caller and must be released with [`ms_core_free`].
#[no_mangle]
pub unsafe extern "C" fn ms_core_call(
    operation: u32,
    input: *const u8,
    len: usize,
) -> *mut Response {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if len > isize::MAX as usize || (input.is_null() && len != 0) {
            return error("Rust bridge: invalid input buffer");
        }
        let data = if len == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(input, len)
        };
        respond(operation, data)
    }));
    // Allocation failure aborts normally; parser panics never unwind across C.
    Box::into_raw(Box::new(
        result.unwrap_or_else(|_| error("Rust core panic; input rejected")),
    ))
}

/// Bytes of one response slot: 0 is UTF-8 JSON, 1 is the still image bytes.
///
/// # Safety
/// `response` is a live pointer from [`ms_core_call`] and `len` is writable.
/// The returned pointer is valid until the response is freed.
#[no_mangle]
pub unsafe extern "C" fn ms_core_bytes(
    response: *const Response,
    slot: u32,
    len: *mut usize,
) -> *const u8 {
    if response.is_null() || len.is_null() {
        return std::ptr::null();
    }
    let r = &*response;
    let bytes = match slot {
        0 => &r.json,
        1 => &r.still,
        _ => {
            *len = 0;
            return std::ptr::null();
        }
    };
    *len = bytes.len();
    bytes.as_ptr()
}

/// Release a response.
///
/// # Safety
/// Call exactly once per response returned by [`ms_core_call`], after all
/// reads of its slots; null is a no-op.
#[no_mangle]
pub unsafe extern "C" fn ms_core_free(response: *mut Response) {
    if !response.is_null() {
        drop(Box::from_raw(response));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ownership_and_rejection() {
        unsafe {
            for (operation, input, len) in [
                (0, std::ptr::null(), 0),
                (0, std::ptr::null(), 1),
                (99, std::ptr::null(), 0),
            ] {
                let r = ms_core_call(operation, input, len);
                let mut size = 0;
                let bytes = ms_core_bytes(r, 0, &mut size);
                let v: serde_json::Value =
                    serde_json::from_slice(std::slice::from_raw_parts(bytes, size)).unwrap();
                assert!(v["error"].is_string());
                ms_core_free(r);
            }
        }
    }
}

#[cfg(test)]
mod lifetime_tests {
    use super::*;

    #[test]
    fn response_owns_still_after_input_is_released() {
        unsafe {
            let input = crate::bench::google_motion_photo(0, 0);
            let expected = crate::parse(&input).unwrap().still.to_vec();
            let response = ms_core_call(0, input.as_ptr(), input.len());
            drop(input);
            let mut len = 0;
            let pointer = ms_core_bytes(response, 1, &mut len);
            assert_eq!(std::slice::from_raw_parts(pointer, len), expected);
            let pointer = ms_core_bytes(response, 0, &mut len);
            let json: serde_json::Value =
                serde_json::from_slice(std::slice::from_raw_parts(pointer, len)).unwrap();
            assert!(json.get("error").is_none());
            ms_core_free(response);
        }
    }
}
