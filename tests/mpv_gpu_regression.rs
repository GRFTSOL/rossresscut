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
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match mpv.acquire_texture(device) {
            Ok(Some(texture)) => return texture,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => panic!("failed to acquire an mpv frame: {error}"),
        }
    }
    panic!("mpv did not export a frame before the deadline");
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
