//! Phase 4C step 1 smoke tests for the optional mpv-engine preview dependency.

#![cfg(feature = "mpv-preview")]

#[test]
fn native_wgpu_backend_is_selected() {
    #[cfg(target_os = "linux")]
    assert!(
        mpv_engine::WGPU_BACKEND.contains(eframe::wgpu::Backends::VULKAN),
        "Linux preview import must use the Vulkan backend"
    );
    #[cfg(target_os = "macos")]
    assert!(
        mpv_engine::WGPU_BACKEND.contains(eframe::wgpu::Backends::METAL),
        "macOS preview import must use the Metal backend"
    );
    #[cfg(target_os = "windows")]
    assert!(
        mpv_engine::WGPU_BACKEND.contains(eframe::wgpu::Backends::DX12),
        "Windows preview import must use the DX12 backend"
    );
}

#[test]
fn headless_engine_starts() {
    let engine = mpv_engine::Engine::headless()
        .build()
        .expect("libmpv must be available when mpv-preview is enabled");
    drop(engine);
}
