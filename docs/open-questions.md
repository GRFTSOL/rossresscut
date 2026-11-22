# Open questions

This file records problems that have been identified but are not being solved in the
current phase.

Rules:

- Each entry contains: the problem, current status, impact, planned handling time, and
  exit condition.
- Resolved problems are deleted from this file, or marked "resolved" in the heading
  with a note on which document holds the conclusion.
- Do not write technical facts here that can be determined immediately; investigate
  first, then decide whether to record it.
- OQ numbers stay stable and are never reused.

## Resolved numbers

OQ-1…OQ-26 have all been resolved or invalidated; the conclusions are spread across
ADR 0009 / 0010 / 0013 / 0014, `docs/investigations/` and
`docs/history/completed-phases.md`. For a per-item trace, see the git history.

OQ-27…OQ-29 (Windows build reproducibility, packaging automation, runtime failure
visibility) are resolved:

- OQ-27 → `scripts/mpv-import-lib.sh` (see `docs/development.md`)
- OQ-28 → `scripts/package-windows.sh` (see `RELEASING.md`)
- OQ-29 → `logging::install_panic_hook`, log location in `docs/development.md`

---

There are currently no open questions.
