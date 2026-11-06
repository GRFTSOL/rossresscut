//! Project persistence and container chapters.
//!
//! Uses the synthetic fixtures so the real FFprobe path is exercised.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rossresscut::application::{Application, OpenState};
use rossresscut::domain::time::Timestamp;

fn temporary_directory() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "rossresscut-project-{}-{stamp}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).expect("create temp directory");
    directory
}

fn wait_for_ready(application: &mut Application) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        application.update();
        if matches!(application.state(), OpenState::Ready) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "media did not become ready before the deadline"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn creates_segments_from_container_chapters() {
    let mut application = Application::new();
    application.open_media(fixture("sample.mkv"));
    wait_for_ready(&mut application);

    let segments: Vec<_> = application.timeline().segments().to_vec();
    assert_eq!(segments.len(), 2, "one segment per container chapter");

    // FFprobe versions disagree on Matroska's negative container start time
    // (0 or about -23ms), so the normalized first chapter start may be a few
    // milliseconds past zero. The chapter boundary and the duration clamp are
    // the stable parts.
    assert!(
        segments[0].start().micros() <= 100_000,
        "the first chapter should start at (or within a fraction of) zero, got {:?}",
        segments[0].start()
    );
    assert_eq!(segments[0].end(), segments[1].start());
    // The second chapter ends at 2s but the container is 1.904s long.
    assert_eq!(segments[1].end(), Timestamp::from_micros(1_904_000));

    assert_eq!(application.chapter_title(segments[0].id()), Some("Opening"));
    assert_eq!(application.chapter_title(segments[1].id()), Some("Closing"));
}

#[test]
fn media_without_chapters_starts_empty() {
    let mut application = Application::new();
    application.open_media(fixture("sample.mp4"));
    wait_for_ready(&mut application);

    assert!(application.timeline().segments().is_empty());
}

#[test]
fn saves_and_restores_chapter_titles() {
    let directory = temporary_directory();
    let project = directory.join("clip.rrc");

    let mut application = Application::new();
    application.open_media(fixture("sample.mkv"));
    wait_for_ready(&mut application);
    let first = application.timeline().segments()[0].id();
    application.set_chapter_title(first, "Custom opening".to_owned());
    application.save_project(&project).expect("save project");

    let text = fs::read_to_string(&project).expect("read project");
    assert!(text.contains("chapter_title = \"Custom opening\""));

    let mut restored = Application::new();
    restored.load_project(&project).expect("load project");
    wait_for_ready(&mut restored);
    assert_eq!(
        restored.chapter_title(first),
        Some("Custom opening"),
        "chapter title should survive a project round trip"
    );
    assert_eq!(restored.timeline().segments().len(), 2);

    if let Err(error) = fs::remove_dir_all(&directory) {
        eprintln!("could not remove {}: {error}", directory.display());
    }
}
