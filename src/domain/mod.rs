//! Domain layer: media, timeline, and segment models.
//!
//! The models described in `docs/media-model.md` live here. This layer must not
//! depend on `eframe`, `egui`, or FFprobe's JSON representation.

pub mod media;
pub mod segment;
pub mod silence;
pub mod time;
pub mod waveform;
