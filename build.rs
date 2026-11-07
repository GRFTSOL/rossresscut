//! Build script: prepare the application icons.
//!
//! The window icon is resized from `assets/img/icon.png` into raw RGBA at build
//! time, so the runtime does not need an image decoder. On Windows the
//! multi-size `assets/img/icon.ico` is embedded into the executable resource.

use std::path::PathBuf;

/// Runtime window icon size. Windows and most compositors scale from here.
const WINDOW_ICON_SIZE: u32 = 256;

fn main() {
    generate_window_icon();
    embed_windows_icon();
}

fn out_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"))
}

/// Resizes the source PNG into the raw RGBA blob included by the UI.
fn generate_window_icon() {
    const SOURCE: &str = "assets/img/icon.png";
    println!("cargo:rerun-if-changed={SOURCE}");

    let image = image::open(SOURCE)
        .unwrap_or_else(|error| panic!("could not read {SOURCE}: {error}"))
        .into_rgba8();
    let resized = image::imageops::resize(
        &image,
        WINDOW_ICON_SIZE,
        WINDOW_ICON_SIZE,
        image::imageops::FilterType::Lanczos3,
    );
    let path = out_dir().join("window-icon.rgba");
    std::fs::write(&path, resized.as_raw())
        .unwrap_or_else(|error| panic!("could not write {}: {error}", path.display()));
}

/// Embeds the multi-size icon and version strings into the Windows executable.
///
/// `winresource` is only available when building on Windows; other hosts skip
/// this step, which is fine because they do not produce PE resources.
#[cfg(windows)]
fn embed_windows_icon() {
    const ICON: &str = "assets/img/icon.ico";
    println!("cargo:rerun-if-changed={ICON}");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(ICON);
    resource.set("FileDescription", "Rossresscut");
    resource.set("ProductName", "Rossresscut");
    if let Err(error) = resource.compile() {
        panic!("could not embed the Windows icon: {error}");
    }
}

#[cfg(not(windows))]
fn embed_windows_icon() {}
