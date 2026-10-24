//! Phase 0 integration checks: the library is usable without a desktop session.

use rossresscut::ui::RossresscutApp;

#[test]
fn desktop_app_is_constructible_without_a_window() {
    let app = RossresscutApp::default();

    fn assert_eframe_app<T: eframe::App>(_: &T) {}

    assert_eframe_app(&app);
}
