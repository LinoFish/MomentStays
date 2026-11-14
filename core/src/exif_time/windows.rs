//! CRT adapters for the two POSIX time entry points absent from Windows libc.
//! The parent module uses the platform libc directly on non-Windows targets.

pub(super) use ::libc::{time_t, tm};

extern "C" {
    fn _mktime32(value: *mut tm) -> i32;
    fn _mktime64(value: *mut tm) -> i64;
}

/// The caller provides a valid, initialized CRT `tm` value.
pub(super) unsafe fn mktime(value: *mut tm) -> time_t {
    // libc uses 32-bit time_t on some 32-bit GNU targets, and 64-bit elsewhere.
    if std::mem::size_of::<time_t>() == 4 {
        unsafe { _mktime32(value) as time_t }
    } else {
        unsafe { _mktime64(value) as time_t }
    }
}

#[cfg(test)]
pub(super) unsafe fn localtime_r(value: *const time_t, output: *mut tm) -> *mut tm {
    // Microsoft's localtime_s takes the output pointer first and returns errno.
    if unsafe { ::libc::localtime_s(output, value) } == 0 {
        output
    } else {
        std::ptr::null_mut()
    }
}
