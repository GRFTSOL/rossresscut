//! Core library for the rossresscut desktop media tool.
//!
//! The crate is split into the UI, application, domain, and media layers
//! described in `docs/architecture.md`. Only the UI layer may depend on
//! `eframe`/`egui`; every other layer must stay usable without a window so it
//! can be tested headlessly.

pub mod application;
pub mod domain;
pub mod error;
pub mod logging;
pub mod media;
pub mod ui;

/// Current development phase, shown in the About window.
pub const PHASE: &str = "Phase 5";
