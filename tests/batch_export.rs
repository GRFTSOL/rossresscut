//! Integration test for batch export.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use rossresscut::application::{Application, BatchItemStatus, BatchRequest};
use rossresscut::domain::time::Timestamp;

#[test]
fn exports_a_queued_file_with_the_current_segments() {
    let source = PathBuf::from("tests/fixtures/sample.mp4");
    let directory = std::env::temp_dir().join(format!("rossresscut-batch-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("output directory");

    let mut application = Application::new();
    let started = application.start_batch(BatchRequest {
        files: vec![source.clone()],
        segments: vec![(
            Timestamp::from_micros(200_000),
            Timestamp::from_micros(800_000),
        )],
        output_directory: Some(directory.clone()),
        chapters: false,
        title: None,
        merge_output: None,
    });
    assert!(started, "batch should start");

    let deadline = Instant::now() + StdDuration::from_secs(60);
    while application.is_batching() && Instant::now() < deadline {
        application.update();
        thread::sleep(StdDuration::from_millis(50));
    }

    let output = directory.join("sample-export.mp4");
    let item = application.batch_items().first().expect("one item");
    let status = item.status().clone();
    let exists = output.exists();
    fs::remove_dir_all(&directory).ok();

    assert!(!application.is_batching(), "batch should finish");
    assert!(
        matches!(status, BatchItemStatus::Completed { .. }),
        "unexpected status: {status:?}"
    );
    assert!(exists, "expected {} to exist", output.display());
}

#[test]
fn skips_files_without_a_matching_segment() {
    let source = PathBuf::from("tests/fixtures/sample.mp4");
    let directory =
        std::env::temp_dir().join(format!("rossresscut-batch-skip-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("output directory");

    let mut application = Application::new();
    application.start_batch(BatchRequest {
        files: vec![source],
        segments: vec![(
            Timestamp::from_micros(5_000_000),
            Timestamp::from_micros(6_000_000),
        )],
        output_directory: Some(directory.clone()),
        chapters: false,
        title: None,
        merge_output: None,
    });

    let deadline = Instant::now() + StdDuration::from_secs(60);
    while application.is_batching() && Instant::now() < deadline {
        application.update();
        thread::sleep(StdDuration::from_millis(20));
    }

    let status = application
        .batch_items()
        .first()
        .expect("one item")
        .status()
        .clone();
    fs::remove_dir_all(&directory).ok();

    assert!(
        matches!(status, BatchItemStatus::Skipped { .. }),
        "unexpected status: {status:?}"
    );
    assert!(!Path::new(&directory).exists());
}

#[test]
fn merges_the_batch_outputs_into_one_file() {
    let root = std::env::temp_dir().join(format!("rossresscut-merge-{}", std::process::id()));
    let outputs = root.join("out");
    fs::create_dir_all(&outputs).expect("workspace");

    let source = PathBuf::from("tests/fixtures/sample.mp4");
    let first = root.join("a.mp4");
    let second = root.join("b.mp4");
    fs::copy(&source, &first).expect("copy a");
    fs::copy(&source, &second).expect("copy b");

    let merged = root.join("merged.mp4");
    let mut application = Application::new();
    let started = application.start_batch(BatchRequest {
        files: vec![first, second],
        segments: vec![(Timestamp::from_micros(0), Timestamp::from_micros(400_000))],
        output_directory: Some(outputs.clone()),
        chapters: false,
        title: None,
        merge_output: Some(merged.clone()),
    });
    assert!(started, "batch should start");

    let deadline = Instant::now() + StdDuration::from_secs(90);
    while application.is_batching() && Instant::now() < deadline {
        application.update();
        thread::sleep(StdDuration::from_millis(50));
    }

    let merge = application.batch_merge().cloned();
    let exists = merged.exists();
    let size = fs::metadata(&merged).map(|meta| meta.len()).unwrap_or(0);
    fs::remove_dir_all(&root).ok();

    let merge = merge.expect("merge result");
    assert_eq!(merge.error, None, "merge failed: {:?}", merge.error);
    assert!(exists, "expected {}", merged.display());
    assert!(size > 0, "merged file is empty");
}
