//! Replaceable system codec boundary. Container decisions stay in `engine`.
//!
//! * Apple: ImageIO / CoreGraphics / AVFoundation (`apple.rs`).
//! * Linux with `--features linux-native`: libjpeg-turbo, lcms2, libheif and
//!   GStreamer (`linux/`), the libraries of the GNOME 50 Flatpak runtime.
//! * Anything else: codec steps are refused; byte-level work still runs.
#[cfg(target_vendor = "apple")]
mod apple;
#[cfg(target_vendor = "apple")]
pub(crate) mod apple_fs;
#[cfg(target_vendor = "apple")]
pub use apple::SystemNative;

#[cfg(all(feature = "linux-native", target_os = "linux"))]
pub(crate) mod linux;
#[cfg(all(feature = "linux-native", target_os = "linux"))]
pub use linux::SystemNative;

pub mod portable;

#[cfg(not(any(
    target_vendor = "apple",
    all(feature = "linux-native", target_os = "linux")
)))]
pub struct SystemNative;
#[cfg(not(any(
    target_vendor = "apple",
    all(feature = "linux-native", target_os = "linux")
)))]
impl crate::engine::Native for SystemNative {
    fn image_decodes(&self, _data: &[u8]) -> Option<bool> {
        Some(false)
    }
}

/// Weighted-work admission budget: physical memory / 8, at least 256 MiB
/// (the reference engine's policy). Apple reads `hw.memsize`, Linux
/// `/proc/meminfo` `MemTotal`; other hosts, or a failed read, use 1 GiB.
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
    #[cfg(target_os = "linux")]
    if let Some(budget) = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| portable::budget_from_meminfo(&text))
    {
        return budget;
    }
    1 << 30
}
