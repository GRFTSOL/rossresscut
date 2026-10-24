//! Media layer: FFmpeg and FFprobe process integration.
//!
//! This layer owns external process execution and raw output parsing. It must
//! not depend on `eframe` or `egui`.

pub mod ffprobe;
pub mod preview;
