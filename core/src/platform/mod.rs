//! Replaceable system codec boundary. Container decisions stay in `engine`.
#[cfg(target_vendor = "apple")]
mod apple;
#[cfg(target_vendor = "apple")]
pub use apple::SystemNative;
#[cfg(not(target_vendor = "apple"))]
pub struct SystemNative;
#[cfg(not(target_vendor = "apple"))]
impl crate::engine::Native for SystemNative {
    fn image_decodes(&self, _data: &[u8]) -> Option<bool> {
        Some(false)
    }
}

/// Match the reference engine's memory admission on Apple. Other hosts can
/// supply their own backend; use a conservative fixed budget until then.
pub(crate) fn memory_budget() -> u64 {
    #[cfg(target_vendor = "apple")]
    unsafe {
        let mut bytes = 0u64;
        let mut size = std::mem::size_of::<u64>();
        if libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut bytes as *mut u64).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        ) == 0
            && bytes > 0
        {
            return (bytes / 8).max(256 << 20);
        }
    }
    1 << 30
}
