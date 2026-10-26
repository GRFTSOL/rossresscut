//! Media layer: FFmpeg/FFprobe process integration and preview backends.
//!
//! This layer owns external process execution, raw output parsing, and the
//! `mpv-engine` preview wrapper. It must not depend on `eframe` or `egui`.

pub mod ffprobe;
pub mod preview;
