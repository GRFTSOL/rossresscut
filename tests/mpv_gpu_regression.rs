//! GPU regression tests for the mpv-engine export path.
//!
//! These cover the two properties the UI depends on and that manual spike runs
//! established:
//!
//! 1. Exported frames are `Bgra8Unorm` and survive the import with the channel
//!    order and colour values intact.
//! 2. An imported texture stays valid after the preview (and its render thread)
//!    is dropped, so the UI can keep displaying the last frame.
//!
//! Ignored by default because they need libmpv, a Vulkan device with DMA-BUF
//! import support and FFmpeg. Run with:
//!
//! ```text
//! cargo test --features mpv-preview --test mpv_gpu_regression -- --ignored
//! ```

#![cfg(all(feature = "mpv-preview", target_os = "linux"))]

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::process::Command;
use std::sync::Mutex;
use std::sync::mpsc;
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::{Duration, Instant};

use rossresscut::application::{Application, OpenState};
use rossresscut::domain::time::Timestamp;
use rossresscut::media::preview::{MpvPreview, MpvPreviewConfig};

const SIZE: u32 = 64;
const CHANNELS: u32 = 4;

/// libmpv's GL export backend does not tolerate several engines starting at
/// the same time, so the tests run one after another even under the default
/// parallel test harness.
static ENGINE_LOCK: Mutex<()> = Mutex::new(());

fn lock_engine() -> std::sync::MutexGuard<'static, ()> {
    ENGINE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Minimal executor: wgpu's adapter and device requests resolve without an
/// external runtime, so polling with a no-op waker is enough.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::yield_now(),
        }
    }
}

fn create_device() -> (wgpu::Device, wgpu::Queue) {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = mpv_engine::WGPU_BACKEND;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        ..Default::default()
    }))
    .unwrap_or_else(|error| panic!("no compatible GPU adapter: {error}"));
    block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("rossresscut regression device"),
        required_features: mpv_engine::REQUIRED_WGPU_FEATURES,
        required_limits: wgpu::Limits {
            max_texture_dimension_2d: 8192,
            ..Default::default()
        },
        ..Default::default()
    }))
    .unwrap_or_else(|error| panic!("failed to create a GPU device: {error}"))
}

fn generate_color_video(color: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("rossresscut-{color}-{}.mp4", std::process::id()));
    let source = format!("color=c={color}:s={SIZE}x{SIZE}:r=10:d=1");
    let status = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(["-f", "lavfi", "-i", &source])
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&path)
        .status()
        .unwrap_or_else(|error| panic!("failed to run ffmpeg: {error}"));
    assert!(status.success(), "ffmpeg failed for colour {color}");
    path
}

/// Generates one second each of red, green and blue, with a keyframe at every
/// second boundary.
fn generate_color_strip_video() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rossresscut-color-strip-{}.mp4",
        std::process::id()
    ));
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=64x64:r=10:d=1",
            "-f",
            "lavfi",
            "-i",
            "color=c=green:s=64x64:r=10:d=1",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=64x64:r=10:d=1",
            "-filter_complex",
            "[0:v][1:v][2:v]concat=n=3:v=1:a=0[v]",
            "-map",
            "[v]",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-g",
            "10",
            "-keyint_min",
            "10",
            "-sc_threshold",
            "0",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&path)
        .status()
        .unwrap_or_else(|error| panic!("failed to run ffmpeg: {error}"));
    assert!(status.success(), "ffmpeg failed for the colour strip");
    path
}

fn start_preview(path: &Path) -> MpvPreview {
    MpvPreview::start(
        MpvPreviewConfig {
            path: path.to_path_buf(),
            width: SIZE,
            height: SIZE,
            start_paused: true,
        },
        || {},
        || {},
    )
    .unwrap_or_else(|error| panic!("failed to start the mpv preview: {error}"))
}

fn wait_for_texture(mpv: &MpvPreview, device: &wgpu::Device) -> wgpu::Texture {
    wait_for_texture_matching(mpv, device, None)
}

/// Waits for an exported frame with exactly `width` x `height`, discarding
/// frames that were rendered before the resize took effect.
fn wait_for_texture_size(
    mpv: &MpvPreview,
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> wgpu::Texture {
    wait_for_texture_matching(mpv, device, Some((width, height)))
}

fn wait_for_texture_matching(
    mpv: &MpvPreview,
    device: &wgpu::Device,
    expected: Option<(u32, u32)>,
) -> wgpu::Texture {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match mpv.acquire_texture(device) {
            Ok(Some(texture))
                if expected.is_none_or(|(width, height)| {
                    (texture.width(), texture.height()) == (width, height)
                }) =>
            {
                return texture;
            }
            Ok(Some(_)) | Ok(None) => {}
            Err(error) => panic!("failed to acquire an mpv frame: {error}"),
        }
        thread::sleep(Duration::from_millis(10));
    }
    let expected = expected.map_or_else(
        || "a frame".to_owned(),
        |(width, height)| format!("a {width}x{height} frame"),
    );
    panic!("mpv did not export {expected} before the deadline");
}

/// Copies the texture back to the CPU and returns tightly packed BGRA rows.
fn read_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> (u32, u32, Vec<u8>) {
    let width = texture.width();
    let height = texture.height();
    let unpadded = width * CHANNELS;
    let padded =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rossresscut readback"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("rossresscut readback"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let slice = buffer.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap_or_else(|error| panic!("GPU poll failed: {error}"));
    receiver
        .recv()
        .unwrap_or_else(|error| panic!("buffer mapping callback was dropped: {error}"))
        .unwrap_or_else(|error| panic!("buffer mapping failed: {error}"));

    let data = slice
        .get_mapped_range()
        .unwrap_or_else(|error| panic!("failed to map the readback buffer: {error}"));
    let mut pixels = Vec::with_capacity((unpadded * height) as usize);
    for row in 0..height {
        let start = (row * padded) as usize;
        pixels.extend_from_slice(&data[start..start + unpadded as usize]);
    }
    drop(data);
    buffer.unmap();
    (width, height, pixels)
}

fn center_pixel(width: u32, height: u32, pixels: &[u8]) -> [u8; 4] {
    let index = ((height / 2) * width + width / 2) as usize * CHANNELS as usize;
    pixels[index..index + 4]
        .try_into()
        .unwrap_or_else(|_| panic!("readback buffer is too short"))
}

fn color_name([blue, green, red, _]: [u8; 4]) -> &'static str {
    if red > 180 && green < 80 && blue < 80 {
        "red"
    } else if green > 180 && red < 80 && blue < 80 {
        "green"
    } else if blue > 180 && red < 80 && green < 80 {
        "blue"
    } else {
        "other"
    }
}

fn wait_for_application_ready(application: &mut Application) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        application.update();
        if matches!(application.state(), OpenState::Ready) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("media did not become ready: {:?}", application.state());
}

fn wait_for_application_texture(
    application: &mut Application,
    device: &wgpu::Device,
) -> wgpu::Texture {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        application.update();
        match application.acquire_mpv_texture(device) {
            Ok(Some(texture)) => return texture,
            Ok(None) => {}
            Err(error) => panic!("failed to acquire an mpv frame: {error}"),
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("application did not publish an mpv frame before the deadline");
}

#[test]
#[ignore = "requires libmpv, a Vulkan device with DMA-BUF import and FFmpeg"]
fn precise_seek_hides_the_previous_texture_until_the_target_frame() {
    let _guard = lock_engine();
    let path = generate_color_strip_video();
    let (device, queue) = create_device();
    let mut application = Application::new();
    application.enable_mpv_preview();
    application.open_media(path.clone());
    wait_for_application_ready(&mut application);
    assert!(application.uses_mpv_preview());
    application.pause();
    // Let mpv finish loading and publish its first frame before seeking, as the
    // UI does once the preview is visible.
    let initial = wait_for_application_texture(&mut application, &device);
    drop(initial);

    application.seek(Timestamp::from_micros(2_500_000));
    assert!(
        application.is_preview_stale(),
        "the seek gate was not armed"
    );
    let blue = wait_for_application_texture(&mut application, &device);
    let (width, height, pixels) = read_pixels(&device, &queue, &blue);
    assert_eq!(color_name(center_pixel(width, height, &pixels)), "blue");
    assert!(!application.is_preview_stale());
    drop(blue);

    application.seek(Timestamp::from_micros(500_000));
    assert!(
        application.is_preview_stale(),
        "the seek gate was not armed for the second seek"
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut first = None;
    while Instant::now() < deadline {
        application.update();
        match application.acquire_mpv_texture(&device) {
            Ok(Some(texture)) => {
                let (width, height, pixels) = read_pixels(&device, &queue, &texture);
                first = Some(color_name(center_pixel(width, height, &pixels)));
                assert!(!application.is_preview_stale());
                break;
            }
            Ok(None) => {}
            Err(error) => panic!("failed to acquire an mpv frame: {error}"),
        }
        thread::sleep(Duration::from_millis(5));
    }

    assert_eq!(
        first,
        Some("red"),
        "the first frame after seeking to red was not red"
    );
    drop(application);
    let _ = std::fs::remove_file(path);
}

#[test]
#[ignore = "requires libmpv, a Vulkan device with DMA-BUF import and FFmpeg"]
fn resizing_the_export_updates_the_texture() {
    let _guard = lock_engine();
    let path = generate_color_video("red");
    let (device, queue) = create_device();
    let mpv = start_preview(&path);
    let initial = wait_for_texture(&mpv, &device);
    assert_eq!((initial.width(), initial.height()), (SIZE, SIZE));
    drop(initial);

    mpv.set_export_size(96, 54)
        .unwrap_or_else(|error| panic!("failed to resize the export: {error}"));
    let resized = wait_for_texture_size(&mpv, &device, 96, 54);
    assert_eq!((resized.width(), resized.height()), (96, 54));

    let (width, height, pixels) = read_pixels(&device, &queue, &resized);
    let [blue, green, red, _alpha] = center_pixel(width, height, &pixels);
    assert!(
        red > 240 && green < 16 && blue < 16,
        "resizing changed the frame content: B={blue} G={green} R={red}"
    );
    drop(resized);

    mpv.set_export_size(32, 32)
        .unwrap_or_else(|error| panic!("failed to resize the export again: {error}"));
    let small = wait_for_texture_size(&mpv, &device, 32, 32);
    assert_eq!((small.width(), small.height()), (32, 32));

    drop(small);
    drop(mpv);
    let _ = std::fs::remove_file(path);
}

#[test]
#[ignore = "requires libmpv, a Vulkan device with DMA-BUF import and FFmpeg"]
fn exported_frames_keep_bgra_channel_order() {
    let _guard = lock_engine();
    let path = generate_color_video("red");
    let (device, queue) = create_device();
    let mpv = start_preview(&path);
    let texture = wait_for_texture(&mpv, &device);

    assert_eq!(
        texture.format(),
        wgpu::TextureFormat::Bgra8Unorm,
        "the exported texture format changed"
    );
    assert_eq!((texture.width(), texture.height()), (SIZE, SIZE));

    let (width, height, pixels) = read_pixels(&device, &queue, &texture);
    let [blue, green, red, _alpha] = center_pixel(width, height, &pixels);
    assert!(
        red > 240 && green < 16 && blue < 16,
        "expected a red frame in BGRA order, got B={blue} G={green} R={red}"
    );

    drop(mpv);
    let _ = std::fs::remove_file(path);
}

#[test]
#[ignore = "requires libmpv, a Vulkan device with DMA-BUF import and FFmpeg"]
fn gray_frames_are_not_gamma_shifted() {
    let _guard = lock_engine();
    let path = generate_color_video("gray");
    let (device, queue) = create_device();
    let mpv = start_preview(&path);
    let texture = wait_for_texture(&mpv, &device);

    let (width, height, pixels) = read_pixels(&device, &queue, &texture);
    let [blue, green, red, _alpha] = center_pixel(width, height, &pixels);
    for (name, value) in [("B", blue), ("G", green), ("R", red)] {
        assert!(
            (120..=136).contains(&value),
            "expected 50% gray, got {name}={value} (B={blue} G={green} R={red})"
        );
    }

    drop(mpv);
    let _ = std::fs::remove_file(path);
}

#[test]
#[ignore = "requires libmpv, a Vulkan device with DMA-BUF import and FFmpeg"]
fn texture_survives_dropping_the_preview() {
    let _guard = lock_engine();
    let path = generate_color_video("red");
    let (device, queue) = create_device();
    let mpv = start_preview(&path);
    let texture = wait_for_texture(&mpv, &device);
    let before = read_pixels(&device, &queue, &texture);

    drop(mpv);

    let after = read_pixels(&device, &queue, &texture);
    assert_eq!(
        before, after,
        "the imported texture changed after the preview was dropped"
    );

    let (width, height, pixels) = after;
    let [blue, green, red, _alpha] = center_pixel(width, height, &pixels);
    assert!(
        red > 240 && green < 16 && blue < 16,
        "the texture content was lost after drop: B={blue} G={green} R={red}"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
#[ignore = "requires libmpv, a Vulkan device with DMA-BUF import and FFmpeg"]
fn consecutive_frames_do_not_mix_content() {
    let _guard = lock_engine();
    let red_path = generate_color_video("red");
    let gray_path = generate_color_video("gray");
    let (device, queue) = create_device();

    let red = start_preview(&red_path);
    let red_texture = wait_for_texture(&red, &device);
    let (width, height, red_pixels) = read_pixels(&device, &queue, &red_texture);
    let [red_blue, red_green, red_red, _] = center_pixel(width, height, &red_pixels);
    assert!(red_red > 240 && red_green < 16 && red_blue < 16);

    let gray = start_preview(&gray_path);
    let gray_texture = wait_for_texture(&gray, &device);
    let (width, height, gray_pixels) = read_pixels(&device, &queue, &gray_texture);
    let [gray_blue, gray_green, gray_red, _] = center_pixel(width, height, &gray_pixels);
    assert!(
        (120..=136).contains(&gray_red)
            && (120..=136).contains(&gray_green)
            && (120..=136).contains(&gray_blue),
        "the second preview reused stale frame content: B={gray_blue} G={gray_green} R={gray_red}"
    );

    drop(red);
    drop(gray);
    let _ = std::fs::remove_file(red_path);
    let _ = std::fs::remove_file(gray_path);
}
