//! Library surface for the milbus-analyzer CLI.
//!
//! Exposes the [`capture`] module so integration tests and the binary share one
//! implementation of JSON frame capture.

pub mod capture;

pub use capture::{Capture, CaptureError, WireCapture};
