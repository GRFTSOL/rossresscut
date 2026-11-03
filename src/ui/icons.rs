//! Nerd Font glyphs used by icon buttons.
//!
//! The bundled `JetBrainsMono Nerd Font Propo` is registered as a fallback in
//! `fonts.rs`. Propo glyphs have a fixed two-cell advance, so the icons keep
//! the same size inside their buttons.

/// Jump to the start of the timeline.
pub(super) const GO_TO_START: &str = "\u{f100}";
/// Select the previous segment.
pub(super) const PREVIOUS_SEGMENT: &str = "\u{f062}";
/// Jump to the selected segment start.
pub(super) const SEGMENT_START: &str = "\u{f104}";
/// Set the selected segment start from the playhead (start flag).
pub(super) const SET_START: &str = "\u{f024}";
/// Jump to the previous keyframe.
pub(super) const PREVIOUS_KEYFRAME: &str = "\u{f049}";
/// Step one frame back.
pub(super) const PREVIOUS_FRAME: &str = "\u{f048}";
/// Start playback.
pub(super) const PLAY: &str = "\u{f04b}";
/// Pause playback.
pub(super) const PAUSE: &str = "\u{f04c}";
/// Step one frame forward.
pub(super) const NEXT_FRAME: &str = "\u{f051}";
/// Jump to the next keyframe.
pub(super) const NEXT_KEYFRAME: &str = "\u{f050}";
/// Set the selected segment end from the playhead (finish flag).
pub(super) const SET_END: &str = "\u{f11e}";
/// Jump to the selected segment end.
pub(super) const SEGMENT_END: &str = "\u{f105}";
/// Select the next segment.
pub(super) const NEXT_SEGMENT: &str = "\u{f063}";
/// Jump to the end of the timeline.
pub(super) const GO_TO_END: &str = "\u{f101}";
/// Open the settings window.
pub(super) const SETTINGS: &str = "\u{f013}";
/// Toggle the batch panel.
pub(super) const BATCH: &str = "\u{f0ae}";
/// Add a cut point.
pub(super) const ADD_CUT: &str = "\u{f067}";
/// Remove the pending cut point.
pub(super) const REMOVE_CUT: &str = "\u{f00d}";
/// Delete the selected segment.
pub(super) const DELETE_SEGMENT: &str = "\u{f068}";
/// Move the selected segment up in the EDL.
pub(super) const MOVE_UP: &str = "\u{f106}";
/// Move the selected segment down in the EDL.
pub(super) const MOVE_DOWN: &str = "\u{f107}";
/// Split the selected segment at the playhead.
pub(super) const SPLIT: &str = "\u{f0c4}";
