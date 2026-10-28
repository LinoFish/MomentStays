//! Owned C ABI responses. Input is borrowed for the call; output lives until free.
use std::panic::{catch_unwind, AssertUnwindSafe};
use serde_json::json;

pub struct Response { json: Vec<u8>, still: Vec<u8> }

fn error(message: &str) -> Response {
    Response { json: json!({"error": message}).to_string().into_bytes(), still: vec![] }
}

fn respond(operation: u32, data: &[u8]) -> Response {
    let result = (|| -> crate::Result<Response> {
        let mut still = vec![];
        let value = match operation {
            0 => {
                let p = crate::parse(data)?;
                let value = json!({"video_start":p.video.start,"video_end":p.video.end,
                    "timestamp":p.timestamp,"timestamp_tag":p.timestamp_tag,"ultra":p.ultra,
                    "cover_frame":p.cover_frame,"format":p.format});
                still = p.still;
                value
            }
            1 => json!({"hash":crate::format::coding_hash(data)?}),
            2 => json!({"hashes":crate::format::auxiliary_hashes(data)?}),
            _ => return crate::fail("Rust bridge: unknown operation"),
        };
        Ok(Response { json: value.to_string().into_bytes(), still })
    })();
    result.unwrap_or_else(|e| error(&e.0))
}

/// Safety: input points to len readable bytes for this call (null only if len=0).
#[no_mangle]
pub unsafe extern "C" fn ms_core_call(operation: u32, input: *const u8, len: usize) -> *mut Response {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if len > isize::MAX as usize || (input.is_null() && len != 0) {
            return error("Rust bridge: invalid input buffer");
        }
        let data = if len == 0 { &[] } else { std::slice::from_raw_parts(input, len) };
        respond(operation, data)
    }));
    // Allocation failure aborts normally; parser panics never unwind across C.
    Box::into_raw(Box::new(result.unwrap_or_else(|_| error("Rust core panic; input rejected"))))
}

/// Safety: response must be live. slot 0 is UTF-8 JSON, slot 1 is still bytes.
#[no_mangle]
pub unsafe extern "C" fn ms_core_bytes(response: *const Response, slot: u32, len: *mut usize) -> *const u8 {
    if response.is_null() || len.is_null() { return std::ptr::null(); }
    let r = &*response;
    let bytes = match slot { 0 => &r.json, 1 => &r.still, _ => { *len = 0; return std::ptr::null(); } };
    *len = bytes.len();
    bytes.as_ptr()
}

/// Safety: free each returned response exactly once, after all output reads.
#[no_mangle]
pub unsafe extern "C" fn ms_core_free(response: *mut Response) {
    if !response.is_null() { drop(Box::from_raw(response)); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ownership_and_rejection() { unsafe {
        for (operation, input, len) in [(0, std::ptr::null(), 0), (0, std::ptr::null(), 1), (99, std::ptr::null(), 0)] {
            let r = ms_core_call(operation, input, len);
            let mut size = 0;
            let bytes = ms_core_bytes(r, 0, &mut size);
            let v: serde_json::Value = serde_json::from_slice(std::slice::from_raw_parts(bytes, size)).unwrap();
            assert!(v["error"].is_string());
            ms_core_free(r);
        }
    }}
}
