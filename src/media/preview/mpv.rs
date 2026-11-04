//! Safe wrapper around `mpv-engine`'s exported-frame backend.
//!
//! The engine owns libmpv, its hidden GL context and the render thread. This
//! module only exposes project-owned types: a configuration, commands,
//! playback events and imported `wgpu::Texture`s. It must not depend on
//! `egui` or `eframe`.
//!
//! The exported texture is GPU-resident; no CPU pixel copy happens here.

use std::fmt;
use std::path::PathBuf;

use mpv_engine::{Engine, ExportOptions, PlaybackEvent};

use crate::domain::time::Timestamp;

/// Configuration for starting the mpv preview backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MpvPreviewConfig {
    /// Media file to preview.
    pub path: PathBuf,
    /// Export width in physical pixels.
    pub width: u32,
    /// Export height in physical pixels.
    pub height: u32,
    /// Start paused after loading; used when opening media before the first play.
    pub start_paused: bool,
}

/// Project-owned playback events drained from mpv.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MpvPreviewEvents {
    /// A file finished loading.
    pub loaded: bool,
    /// Playback restarted after a seek completed.
    pub restarted: bool,
    /// Playback reached the end.
    pub ended: bool,
    /// Playback failed; the message is the mpv error description.
    pub failed: Option<String>,
}

/// Errors produced by the mpv preview backend.
#[derive(Debug)]
pub enum MpvPreviewError {
    /// The requested configuration cannot be used.
    InvalidConfig {
        /// What is invalid.
        detail: String,
    },
    /// mpv requires a UTF-8 path.
    NonUtf8Path {
        /// Path that cannot be passed to mpv.
        path: PathBuf,
    },
    /// The libmpv engine could not be created.
    Engine {
        /// Media file being previewed.
        path: PathBuf,
        /// Underlying mpv error.
        detail: String,
    },
    /// The exported-frame render backend could not be attached.
    Attach {
        /// Media file being previewed.
        path: PathBuf,
        /// Underlying mpv error.
        detail: String,
    },
    /// The media file could not be loaded.
    Load {
        /// Media file being previewed.
        path: PathBuf,
        /// Underlying mpv error.
        detail: String,
    },
    /// A playback command failed.
    Command {
        /// Operation that failed.
        operation: &'static str,
        /// Underlying mpv error.
        detail: String,
    },
    /// An exported frame could not be imported into wgpu.
    Frame {
        /// Media file being previewed.
        path: PathBuf,
        /// Underlying import error.
        detail: String,
    },
}

impl fmt::Display for MpvPreviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig { detail } => {
                write!(formatter, "invalid mpv preview configuration: {detail}")
            }
            Self::NonUtf8Path { path } => write!(
                formatter,
                "mpv preview requires a UTF-8 path, got {}",
                path.display()
            ),
            Self::Engine { path, detail } => write!(
                formatter,
                "failed to create the mpv engine for {}: {detail}",
                path.display()
            ),
            Self::Attach { path, detail } => write!(
                formatter,
                "failed to attach the mpv exported render backend for {}: {detail}",
                path.display()
            ),
            Self::Load { path, detail } => {
                write!(
                    formatter,
                    "failed to load {} in mpv: {detail}",
                    path.display()
                )
            }
            Self::Command { operation, detail } => {
                write!(formatter, "mpv {operation} failed: {detail}")
            }
            Self::Frame { path, detail } => write!(
                formatter,
                "failed to import an mpv frame from {} into wgpu: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for MpvPreviewError {}

/// Owns the libmpv engine used by the primary preview path.
///
/// Dropping this value detaches the exported render backend and joins its
/// render thread. Frames may still be outstanding; mpv-engine keeps their
/// imported memory alive until wgpu has finished with it.
pub struct MpvPreview {
    engine: Engine,
    path: PathBuf,
}

impl MpvPreview {
    /// Starts libmpv, attaches the exported-frame backend and loads `config.path`.
    ///
    /// mpv keeps its default `video-sync=audio`: frame selection and A/V sync
    /// stay inside libmpv, and this wrapper only reads `time-pos`.
    ///
    /// `on_frame_update` is called when a new frame is available.
    /// `on_event` is called when mpv queues an event. Both may run on mpv
    /// threads and must be cheap and thread-safe.
    pub fn start(
        config: MpvPreviewConfig,
        on_frame_update: impl Fn() + Send + Sync + 'static,
        on_event: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, MpvPreviewError> {
        if config.width == 0 || config.height == 0 {
            return Err(MpvPreviewError::InvalidConfig {
                detail: "export width and height must be non-zero".to_owned(),
            });
        }

        let path = config.path;
        let source = path
            .to_str()
            .ok_or_else(|| MpvPreviewError::NonUtf8Path { path: path.clone() })?;

        let engine = Engine::video()
            // Keeping the file loaded at EOF lets the user drag back from the
            // end without rebuilding the mpv backend. The preview would
            // otherwise fall back to the slow external FFmpeg path.
            .property("keep-open", "yes")
            .property("hr-seek", "yes")
            // `auto-safe` picks the platform media engine (VAAPI on Linux,
            // D3D11VA on Windows, VideoToolbox on macOS) and falls back to
            // software when no device is available.
            .property("hwdec", "auto-safe")
            .property("pause", if config.start_paused { "yes" } else { "no" })
            .build()
            .map_err(|error| MpvPreviewError::Engine {
                path: path.clone(),
                detail: error.to_string(),
            })?;

        engine
            .attach_exported_render(
                ExportOptions::new(config.width, config.height),
                on_frame_update,
            )
            .map_err(|error| MpvPreviewError::Attach {
                path: path.clone(),
                detail: error.to_string(),
            })?;

        engine.set_wakeup_callback(on_event);
        engine
            .load_when_ready(source)
            .map_err(|error| MpvPreviewError::Load {
                path: path.clone(),
                detail: error.to_string(),
            })?;

        Ok(Self { engine, path })
    }

    /// Takes the newest exported frame, if one is waiting, and imports it
    /// into `device` without a CPU copy.
    pub fn acquire_texture(
        &self,
        device: &wgpu::Device,
    ) -> Result<Option<wgpu::Texture>, MpvPreviewError> {
        match self.engine.acquire_frame() {
            Ok(Some(frame)) => {
                frame
                    .into_wgpu_texture(device)
                    .map(Some)
                    .map_err(|error| MpvPreviewError::Frame {
                        path: self.path.clone(),
                        detail: error.to_string(),
                    })
            }
            Ok(None) => Ok(None),
            Err(error) => Err(MpvPreviewError::Frame {
                path: self.path.clone(),
                detail: error.to_string(),
            }),
        }
    }

    /// Current playback position, if mpv reports one.
    pub fn position(&self) -> Option<Timestamp> {
        seconds_to_micros(self.engine.position()?).map(Timestamp::from_micros)
    }

    /// Pauses or resumes playback.
    pub fn set_paused(&self, paused: bool) -> Result<(), MpvPreviewError> {
        self.engine
            .set_paused(paused)
            .map_err(|error| command_error("pause", error))
    }

    /// Seeks to an absolute position.
    pub fn seek(&self, position: Timestamp) -> Result<(), MpvPreviewError> {
        let seconds = timestamp_seconds(position);
        self.engine
            .seek_absolute(seconds)
            .map_err(|error| command_error("seek", error))
    }

    /// Chooses precise (`true`) or keyframe (`false`) seeking for the next seek.
    ///
    /// Use keyframe seeking while scrubbing and precise seeking when the drag
    /// ends.
    pub fn set_precise_seek(&self, precise: bool) -> Result<(), MpvPreviewError> {
        let value = if precise { "yes" } else { "no" };
        self.engine
            .set_property("hr-seek", value)
            .map_err(|error| command_error("set hr-seek", error))
    }

    /// Changes the exported frame size in physical pixels.
    pub fn set_export_size(&self, width: u32, height: u32) -> Result<(), MpvPreviewError> {
        self.engine
            .set_export_size(width, height)
            .map_err(|error| command_error("set export size", error))
    }

    /// Hardware decoder mpv is actually using, for example `vaapi`,
    /// `vaapi-copy`, `d3d11va` or `no` after falling back to software.
    pub fn hardware_decoder(&self) -> Option<String> {
        self.engine.get_property::<String>("hwdec-current").ok()
    }

    /// Discards the currently published frame without importing it.
    ///
    /// Used before a precise seek so a frame rendered at the old position
    /// cannot be mistaken for the seek target.
    pub fn discard_published_frame(&self) -> Result<(), MpvPreviewError> {
        self.engine
            .acquire_frame()
            .map(|_| ())
            .map_err(|error| command_error("discard frame", error))
    }

    /// Steps one decoded frame forward or backward, then pauses.
    ///
    /// mpv uses the source frame boundaries, so this stays exact for VFR and
    /// non-integer frame rates. `frame-back-step` is unsupported by some
    /// decoders; callers should fall back to a time-based step on error.
    pub fn frame_step(&self, forward: bool) -> Result<(), MpvPreviewError> {
        let command = if forward {
            "frame-step"
        } else {
            "frame-back-step"
        };
        self.engine
            .command(command, &[])
            .map_err(|error| command_error("frame step", error))
    }

    /// Drains pending mpv events into project-owned flags.
    pub fn poll_events(&self) -> MpvPreviewEvents {
        let mut events = MpvPreviewEvents::default();
        for event in self.engine.pump_events() {
            match event {
                PlaybackEvent::Loaded => events.loaded = true,
                PlaybackEvent::PlaybackRestart => events.restarted = true,
                PlaybackEvent::Ended { .. } => events.ended = true,
                PlaybackEvent::Failed { message, .. } => events.failed = Some(message),
                _ => {}
            }
        }
        events
    }
}

impl Drop for MpvPreview {
    fn drop(&mut self) {
        self.engine.detach_render();
    }
}

fn command_error(operation: &'static str, error: mpv_engine::Error) -> MpvPreviewError {
    MpvPreviewError::Command {
        operation,
        detail: error.to_string(),
    }
}

fn seconds_to_micros(seconds: f64) -> Option<i64> {
    if !seconds.is_finite() {
        return None;
    }
    let micros = (seconds * 1_000_000.0).round();
    if micros < i64::MIN as f64 || micros > i64::MAX as f64 {
        return None;
    }
    Some(micros as i64)
}

fn timestamp_seconds(timestamp: Timestamp) -> f64 {
    timestamp.micros() as f64 / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_seconds_to_timestamps() {
        assert_eq!(
            seconds_to_micros(1.5).map(Timestamp::from_micros),
            Some(Timestamp::from_micros(1_500_000))
        );
        assert_eq!(
            seconds_to_micros(-0.25).map(Timestamp::from_micros),
            Some(Timestamp::from_micros(-250_000))
        );
    }

    #[test]
    fn rejects_non_finite_seconds() {
        assert_eq!(seconds_to_micros(f64::NAN), None);
        assert_eq!(seconds_to_micros(f64::INFINITY), None);
        assert_eq!(seconds_to_micros(f64::NEG_INFINITY), None);
    }

    #[test]
    fn converts_timestamps_to_seconds() {
        assert_eq!(timestamp_seconds(Timestamp::from_micros(1_500_000)), 1.5);
        assert_eq!(timestamp_seconds(Timestamp::from_micros(-250_000)), -0.25);
    }

    #[test]
    fn rejects_zero_export_dimensions_before_starting_mpv() {
        let result = MpvPreview::start(
            MpvPreviewConfig {
                path: PathBuf::from("unused.mp4"),
                width: 0,
                height: 720,
                start_paused: true,
            },
            || {},
            || {},
        );
        assert!(matches!(result, Err(MpvPreviewError::InvalidConfig { .. })));
    }
}
