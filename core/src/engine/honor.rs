//! Compatibility path for the dedicated HDR implementation.
//! New code should use `engine::hdr::honor`.
pub use super::hdr::honor::{convert, map_luma, OFFSET, PROFILE, STOPS};
