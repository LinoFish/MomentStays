//! Google Motion Photo stack ported from the retired Python `motionlive` package.
//!
//! These modules back the `momentstays` CLI and the Live Photo / Motion Photo writers
//! (`live2motion`, `heifwrite`, `model`, `scan`). Byte output and error text stay
//! identical to the Python implementation; `tests/pycli.rs` pins them to frozen
//! expected outputs, so Python is not needed to run the checks.
//! The Swift-parity parser is the separate `format` module.

pub mod jpeg;
pub mod motion;
pub mod xml;
