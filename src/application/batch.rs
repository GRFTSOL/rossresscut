//! Batch export: apply the current segment list to a queue of files.
//!
//! Every file is probed and exported on a worker thread, so the UI stays
//! responsive and the currently opened media is never replaced.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration as StdDuration;

use crate::domain::time::{Duration, Timestamp};
use crate::media::export::{
    ExportEvent, ExportProcess, ExportRequest, ExportSegment, MultiExport, MultiExportRequest,
};
use crate::media::ffprobe;

use super::Application;

/// One file in the batch queue.
#[derive(Clone, Debug)]
pub struct BatchItem {
    path: PathBuf,
    status: BatchItemStatus,
}

impl BatchItem {
    /// Returns the queued file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the current status.
    pub fn status(&self) -> &BatchItemStatus {
        &self.status
    }
}

/// Progress of one queued file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchItemStatus {
    /// Waiting for its turn.
    Pending,
    /// Being probed or exported.
    Running,
    /// Export finished.
    Completed {
        /// Where the result was written.
        output: PathBuf,
    },
    /// Export failed.
    Failed {
        /// User-facing failure description.
        message: String,
    },
    /// The file was skipped, for example because no segment fits.
    Skipped {
        /// Why the file was skipped.
        reason: String,
    },
}

/// What every queued file should be exported with.
#[derive(Clone, Debug)]
pub struct BatchRequest {
    /// Files to process, in queue order.
    pub files: Vec<PathBuf>,
    /// Enabled segments of the current timeline.
    pub segments: Vec<(Timestamp, Timestamp)>,
    /// Fixed output directory; `None` writes next to each source file.
    pub output_directory: Option<PathBuf>,
    /// Whether to generate chapters.
    pub chapters: bool,
    /// Output title metadata.
    pub title: Option<String>,
    /// When set, the finished outputs are concatenated into this file.
    pub merge_output: Option<PathBuf>,
}

/// Result of merging the finished batch outputs.
#[derive(Clone, Debug)]
pub struct BatchMerge {
    /// Where the merged file was written.
    pub output: PathBuf,
    /// Failure description, or `None` on success.
    pub error: Option<String>,
}

pub(super) struct BatchJob {
    receiver: Receiver<BatchEvent>,
    cancel: Arc<AtomicBool>,
}

enum BatchEvent {
    Started(usize),
    Finished {
        index: usize,
        status: BatchItemStatus,
    },
    Merged {
        output: PathBuf,
        error: Option<String>,
    },
}

impl Application {
    /// Returns the queued files.
    pub fn batch_items(&self) -> &[BatchItem] {
        &self.batch_items
    }

    /// Returns whether a batch is currently running.
    pub fn is_batching(&self) -> bool {
        self.batch_job.is_some()
    }

    /// Returns the merge result of the last batch, when it asked for one.
    pub fn batch_merge(&self) -> Option<&BatchMerge> {
        self.batch_merge.as_ref()
    }

    /// Replaces the queue with `files`, all waiting to start.
    pub fn queue_batch(&mut self, files: Vec<PathBuf>) {
        if self.batch_job.is_some() {
            return;
        }
        self.batch_items = files
            .into_iter()
            .map(|path| BatchItem {
                path,
                status: BatchItemStatus::Pending,
            })
            .collect();
    }

    /// Replaces the queue and starts processing it.
    ///
    /// Returns `false` when another batch is still running or the request is
    /// empty.
    pub fn start_batch(&mut self, request: BatchRequest) -> bool {
        if self.batch_job.is_some() || request.files.is_empty() {
            return false;
        }
        self.batch_merge = None;

        self.batch_items = request
            .files
            .iter()
            .map(|path| BatchItem {
                path: path.clone(),
                status: BatchItemStatus::Pending,
            })
            .collect();

        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let worker_request = request;

        let spawn_result = thread::Builder::new()
            .name("batch-export".to_owned())
            .spawn(move || run_batch(worker_request, sender, worker_cancel));

        match spawn_result {
            Ok(_handle) => {
                self.batch_job = Some(BatchJob { receiver, cancel });
                log::info!("started a batch of {} files", self.batch_items.len());
                true
            }
            Err(error) => {
                log::warn!("failed to start the batch worker: {error}");
                self.batch_items.clear();
                false
            }
        }
    }

    /// Requests cancellation of the running batch.
    pub fn cancel_batch(&mut self) {
        if let Some(job) = self.batch_job.as_ref() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Removes finished items from the queue.
    pub fn clear_batch(&mut self) {
        if self.batch_job.is_none() {
            self.batch_items.clear();
            self.batch_merge = None;
        }
    }

    /// Polls the batch worker without blocking.
    pub(super) fn poll_batch(&mut self) {
        let mut disconnected = false;
        if let Some(job) = self.batch_job.as_ref() {
            loop {
                match job.receiver.try_recv() {
                    Ok(BatchEvent::Started(index)) => {
                        if let Some(item) = self.batch_items.get_mut(index) {
                            item.status = BatchItemStatus::Running;
                        }
                    }
                    Ok(BatchEvent::Finished { index, status }) => {
                        if let Some(item) = self.batch_items.get_mut(index) {
                            item.status = status;
                        }
                    }
                    Ok(BatchEvent::Merged { output, error }) => {
                        self.batch_merge = Some(BatchMerge { output, error });
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        if disconnected {
            self.batch_job = None;
        }
    }
}

fn run_batch(request: BatchRequest, sender: Sender<BatchEvent>, cancel: Arc<AtomicBool>) {
    let mut outputs = Vec::new();

    for (index, path) in request.files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if sender.send(BatchEvent::Started(index)).is_err() {
            return;
        }
        let status = process_file(path, &request, &cancel);
        if let BatchItemStatus::Completed { output } = &status {
            outputs.push(output.clone());
        }
        if sender.send(BatchEvent::Finished { index, status }).is_err() {
            return;
        }
    }

    if let Some(target) = &request.merge_output {
        let result = if outputs.is_empty() {
            Err("no file was exported to merge".to_owned())
        } else {
            crate::media::export::merge_files(&outputs, target).map_err(|error| error.to_string())
        };
        let _ = sender.send(BatchEvent::Merged {
            output: target.clone(),
            error: result.err(),
        });
    }
}

fn process_file(path: &Path, request: &BatchRequest, cancel: &AtomicBool) -> BatchItemStatus {
    if cancel.load(Ordering::Relaxed) {
        return BatchItemStatus::Skipped {
            reason: "cancelled".to_owned(),
        };
    }

    let media = match ffprobe::probe(path) {
        Ok(media) => media,
        Err(error) => {
            return BatchItemStatus::Failed {
                message: error.to_string(),
            };
        }
    };
    let Some(duration) = media.duration else {
        return BatchItemStatus::Skipped {
            reason: "the duration is unknown".to_owned(),
        };
    };

    let ranges = clamp_segments(&request.segments, duration);
    if ranges.is_empty() {
        return BatchItemStatus::Skipped {
            reason: "no segment fits this file".to_owned(),
        };
    }

    let output = output_path(path, request.output_directory.as_deref());
    let result = if ranges.len() == 1 {
        export_single(path, ranges[0], &output, request, cancel)
    } else {
        export_multi(path, &ranges, &output, request, cancel)
    };

    match result {
        Ok(()) => BatchItemStatus::Completed { output },
        Err(message) => BatchItemStatus::Failed { message },
    }
}

/// Keeps the parts of `segments` that fall inside `duration`.
fn clamp_segments(
    segments: &[(Timestamp, Timestamp)],
    duration: Duration,
) -> Vec<(Timestamp, Timestamp)> {
    let end = Timestamp::from_micros(i64::try_from(duration.micros()).unwrap_or(i64::MAX));
    segments
        .iter()
        .filter_map(|(start, finish)| {
            let start = *start.min(&end);
            let finish = *finish.min(&end);
            (start < finish).then_some((start, finish))
        })
        .collect()
}

fn output_path(source: &Path, directory: Option<&Path>) -> PathBuf {
    let stem = source.file_stem().map_or_else(
        || "export".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("mkv");
    let name = format!("{stem}-export.{extension}");
    match directory {
        Some(directory) => directory.join(name),
        None => source.with_file_name(name),
    }
}

fn export_single(
    source: &Path,
    range: (Timestamp, Timestamp),
    output: &Path,
    request: &BatchRequest,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut export = ExportRequest::new(source, range.0, range.1, output)
        .ok_or_else(|| format!("could not build an export request for {}", source.display()))?;
    if request.chapters {
        let duration = export.duration();
        export = export.with_chapters(vec![crate::media::export::ChapterSpec {
            start: Duration::ZERO,
            end: duration,
            title: "Segment 1".to_owned(),
        }]);
    }
    export = export.with_title(request.title.clone());

    let mut process = ExportProcess::start(&export).map_err(|error| error.to_string())?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            process.cancel();
            let _ = process.wait();
            return Err("cancelled".to_owned());
        }
        match process.poll() {
            Some(ExportEvent::Progress { .. }) => {}
            Some(ExportEvent::Finished { stderr, ended }) => {
                let exit = process.wait();
                return match exit {
                    Ok(status) if ended && status.success() => Ok(()),
                    Ok(status) => {
                        let details = stderr.trim();
                        Err(if details.is_empty() {
                            format!("ffmpeg exited with {status}")
                        } else {
                            format!("ffmpeg exited with {status}: {details}")
                        })
                    }
                    Err(error) => Err(format!("could not wait for ffmpeg: {error}")),
                };
            }
            None => thread::sleep(StdDuration::from_millis(20)),
        }
    }
}

fn export_multi(
    source: &Path,
    ranges: &[(Timestamp, Timestamp)],
    output: &Path,
    request: &BatchRequest,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let segments: Vec<ExportSegment> = ranges
        .iter()
        .filter_map(|(start, finish)| ExportSegment::new(*start, *finish))
        .collect();
    let multi = MultiExportRequest::new(source, segments, output)
        .ok_or_else(|| format!("could not build an export request for {}", source.display()))?
        .with_chapters(request.chapters)
        .with_title(request.title.clone());

    let mut process = MultiExport::start(multi).map_err(|error| error.to_string())?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            process.cancel();
            return Err("cancelled".to_owned());
        }
        match process.poll() {
            Some(crate::media::export::MultiExportEvent::Progress { .. }) => {}
            Some(crate::media::export::MultiExportEvent::Finished { error }) => {
                return error.map_or(Ok(()), Err);
            }
            None => thread::sleep(StdDuration::from_millis(20)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_segments_to_the_file_duration() {
        let duration = Duration::from_micros(5_000_000);
        let segments = [
            (Timestamp::ZERO, Timestamp::from_micros(2_000_000)),
            (
                Timestamp::from_micros(4_000_000),
                Timestamp::from_micros(9_000_000),
            ),
            (
                Timestamp::from_micros(6_000_000),
                Timestamp::from_micros(7_000_000),
            ),
        ];

        let ranges = clamp_segments(&segments, duration);

        assert_eq!(ranges.len(), 2);
        assert_eq!(
            ranges[1],
            (
                Timestamp::from_micros(4_000_000),
                Timestamp::from_micros(5_000_000)
            )
        );
    }

    #[test]
    fn output_name_keeps_the_extension_and_avoids_the_source() {
        let source = Path::new("/media/clip.mp4");

        assert_eq!(
            output_path(source, None),
            Path::new("/media/clip-export.mp4")
        );
        assert_eq!(
            output_path(source, Some(Path::new("/tmp/out"))),
            Path::new("/tmp/out/clip-export.mp4")
        );
    }
}
