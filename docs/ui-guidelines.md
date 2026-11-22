# UI Rules

This file is the source of constraints for the UI and desktop integration. Entry points
and hard constraints are in `AGENTS.md`; layering and data flow are in
`docs/architecture.md`.

## 1. The egui UI must not block

Running long tasks directly inside the per-frame egui update logic is forbidden.

This includes:

- FFmpeg
- FFprobe
- video decoding
- bulk file reads
- waveform generation
- scene detection
- large-scale data processing
- blocking process waits
- blocking channel receives

This work must be placed outside the UI frame.

Long tasks must be cancellable, and cancellation must actually terminate the external
process (for example, `kill` FFmpeg when the scene scan window is closed); discarding
the result channel alone is not enough. While a task is running, the UI must actively
request repaints, otherwise progress stalls at its first value.

---

## 2. The UI is not responsible for business logic

The UI is responsible for:

- displaying state
- gathering user input
- sending commands
- displaying progress
- displaying errors

The UI is not responsible for:

- building FFmpeg commands
- parsing FFprobe JSON
- computing media time
- deciding whether a segment is valid
- performing media export
- manipulating underlying files

---

## 3. UI state is separate from domain state

Distinguish:

### Domain state

For example:

- the current media
- the timeline
- segments
- stream selection
- export configuration

### Application state

For example:

- the current task
- export progress
- scene scan progress (0–99%, replaced by the Ready state when finished)
- error state
- cancellation state

### UI state

For example:

- the control currently being dragged
- the current hover position
- panel expansion state
- temporary input content

**UI state must not be the sole source of domain state.**

---

## 4. State changes must go through the application layer

The UI is not allowed to modify domain state directly.

- Every operation that changes domain state must be expressed as an application-layer
  command, with an `enabled` check and a failure reason.
- The UI only builds arguments, invokes commands, and displays results.
- Code paths outside commands must not mutate domain state.

---

## 5. Desktop integration

- On Linux, opening files must use the XDG desktop portal (`rfd`'s `xdg-portal`
  backend); do not build a custom file picker.
- On Windows, opening / saving files uses `rfd`'s Windows backend (Shell COM's
  `IFileOpenDialog` / `IFileSaveDialog`); likewise do not build a custom file picker.
- The top shows a tab with the current media title, showing only the file name stem
  without the extension; no editable path input box is provided.
- The theme follows the system by default, and the settings page allows manually
  selecting Dark / Light; the settings page also provides font selection and font
  scaling.
- When the system does not report a theme, automatic theming reads the GTK / Qt
  configuration (including nwg-look and qt6ct); if it still cannot be determined, the
  manual theme is used.
- Automatic theming is only detected at startup, when settings are opened, or when
  switching to automatic theme; there is no periodic polling.
- The Windows main window merges the title bar and the menu bar into a single row whose
  height matches the Windows Explorer title bar (32 px at 96 DPI):
  menu on the left, and only minimize / maximize / close on the right. The window
  button area has no border and no outer margin, so after maximizing the mouse can move
  straight to the top-right corner of the screen to close; the blank area can be
  dragged and double-clicked to maximize. Menu button hover backgrounds are 24 px high,
  leaving 4 px above and below inside the 32 px bar, so they do not touch the edges.
  Linux keeps compositor decorations and a separate menu bar.
- The current file name is shown on the left side of the bottom row, the timecode is
  centered on the window center as its axis, and the Export button is on the right.
- The View menu provides toggles for "Show batch queue" and "Show playback info"; the
  settings entry is in the Tools menu.
- Settings is a separate operating-system window (a second egui viewport); it is no
  longer embedded in the main window and no longer uses collapsible groups, but four
  tabs: "General / Preview / Export / Shortcuts"; each tab contains only one vertical
  scroll area. The window uses the same custom-drawn title bar style as the main window
  (centered title + close). Shortcuts are laid out in two paired columns; clicking a
  binding starts recording, and pressing Backspace during recording clears it.
- The "Preview" tab provides decoder backend selection: libmpv's `auto-safe` / `auto` /
  `d3d11va` / `nvdec` / `cuda` / `vaapi` / `no` (only items available on the current
  platform are listed) plus external FFmpeg; switching rebuilds the preview immediately.
- Primary dialog actions can be confirmed with Enter; when a text box has focus, Enter
  is still delivered to the text box, and only the topmost window responds.
- The "View" menu provides fullscreen and reset / increase / decrease font size; the
  settings entry is in the Tools menu.
- Default shortcuts are listed in `README.md`; menu commands without a default can also be bound
  here. Settings are persisted as TOML to `$XDG_CONFIG_HOME/rossresscut/config.toml` (falling
  back to `~/.config`).
- The sidebar playback info is hidden by default and toggled by "View > Show playback
  info"; the info shows only the decoder (the actual hwdec), frame, video, and audio.
  Below it are two tabs, Segments / Chapters, with Segments by default.
- The volume button is fixed at 32×24: the icon changes with the volume, but the button
  rectangle and the popup anchor must not move with the icon width; the volume popup is
  centered on the button's center as its axis.
- Stream info is collapsed by default; the collapsed state is recorded by egui memory.
- The timeline occupies its own row; playback controls sit below it and are horizontally
  centered, using symbol buttons with hover tooltips; previous second / next second are
  buttons, while single-frame stepping only has shortcuts.
- Segments on the timeline display their number in Timeline order; when the width is
  insufficient to hold the number, it is drawn to the left of the segment.
- Font selection is grouped by font family; families can be searched and the weight
  selected separately; font size and font family are in the same area.
- The right-hand segment list shares the color scheme with the timeline; Ctrl-click
  allows multi-selection.
- The Edit menu provides undo / redo / cut / copy / paste / select all; undo history
  only covers Timeline state.
- The Segments menu provides random segment, sort / shuffle, merge all overlapping
  segments / invert segments (swap the selected and unselected regions), clear all.
- The Tools menu provides "Executed FFmpeg commands": it lists all FFmpeg commands from
  the current session in newest-first order, annotated with their purpose (export /
  scene detection / waveform / subtitle / merge / probe, etc.); preview decoding is not
  counted. The content can be copied, not executed directly.
- The export button reads "Export…"; clicking it opens the export panel; the output
  directory can be the source file's directory or a specified directory.
- The export button is in the bottom-right corner of the main window and is not placed
  in the File menu.
- Cut point interaction: the first "Add cut point" sets the segment start; adding
  another after it immediately creates a Segment; "Remove cut point" clears the pending
  cut point, and when there is no pending cut point it deletes the selected Segment;
  "Split segment" splits at the playhead.
- When no media is open, clicking anywhere in the preview area opens the file picker;
  the hint text is a non-selectable label.
- The About window shows the version, the project license (GPL-3.0-or-later) and an
  expandable list of third-party components (licenses of FFmpeg, libmpv and the Rust
  dependencies); the list is kept consistent with `ATTRIBUTION.md`.
- Startup failures must be visible: there is no console on Windows, so `main` pops a
  native error dialog (`rfd`'s `MessageDialog`) including the error text and a
  `RUST_LOG=info` hint; other platforms still print to stderr.

---
