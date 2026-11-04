//! Opt-in diagnostic drivers for UI behavior that normally needs pointer input.

use std::time::{Duration, Instant};

use eframe::egui;

use crate::domain::time::Timestamp;

use super::RossresscutApp;

impl RossresscutApp {
    /// Drives a fixed scrub sequence for `ROSS_RESCUT_DIAG_SCRUB`.
    ///
    /// This exercises the same `scrub_to` / `finish_scrub` path as a pointer
    /// drag while a real eframe window and audio output are running.
    pub(super) fn run_diagnostic_scrub(&mut self, context: &egui::Context) {
        const STEPS: u32 = 40;
        let Some(duration) = self.application.duration() else {
            return;
        };
        if self.diagnostic_scrub_start.is_none() {
            self.application.play();
            self.scrub_was_playing = self.application.is_playing();
            self.diagnostic_scrub_start = Some(Instant::now());
            log::info!(
                "diagnostic scrub: started (audio_clock={})",
                self.application.uses_audio_clock()
            );
        }
        let Some(start) = self.diagnostic_scrub_start else {
            return;
        };
        let duration_micros = duration.micros() as f64;

        if self.diagnostic_scrub_step < STEPS {
            let fraction = 0.1 + 0.8 * f64::from(self.diagnostic_scrub_step) / f64::from(STEPS);
            self.scrub_to(Timestamp::from_micros((duration_micros * fraction) as i64));
            self.diagnostic_scrub_step += 1;
            context.request_repaint();
        } else if self.diagnostic_scrub_step == STEPS {
            let target = Timestamp::from_micros((duration_micros * 0.9) as i64);
            self.finish_scrub(target);
            log::info!(
                "diagnostic scrub: {STEPS} steps in {:?}, target={target}, position={}",
                start.elapsed(),
                self.application.position()
            );
            self.diagnostic_scrub_step += 1;
            context.request_repaint_after(Duration::from_millis(500));
        } else if self.diagnostic_scrub_step == STEPS + 1
            && start.elapsed() >= Duration::from_millis(1500)
        {
            log::info!(
                "diagnostic scrub: position after resume={}",
                self.application.position()
            );
            self.diagnostic_scrub_step += 1;
        } else {
            context.request_repaint_after(Duration::from_millis(200));
        }
    }
}
