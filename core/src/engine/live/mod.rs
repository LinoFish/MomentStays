//! Live/Motion Photo workflows. Depends on HDR and output domains, never the reverse.
//! Pairing metadata and video primitives remain in engine::still/movie and the
//! existing container libraries; no duplicate codec or writer implementation.
pub mod motion;
pub mod pairing;
pub mod to_android;
pub mod to_apple;
pub mod transfer;

pub mod honor_to_google;
