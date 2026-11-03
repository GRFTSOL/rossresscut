//! Project files: persisting and restoring a timeline.
//!
//! A project stores the source media path, the ordered segments and the ID
//! counter. It deliberately does not store the media metadata, keyframes or
//! waveform: those are re-probed when the project is opened.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::segment::{Segment, SegmentId, Timeline};
use crate::domain::time::Timestamp;

use super::{Application, OpenState, PendingProject};

/// File extension used for project files.
pub const PROJECT_EXTENSION: &str = "rrc";

/// Current project file version.
const PROJECT_VERSION: u32 = 1;

/// Errors produced while reading or writing a project.
#[derive(Debug)]
pub enum ProjectError {
    /// The project file could not be read.
    Read {
        /// Project file path.
        path: PathBuf,
        /// Underlying I/O failure.
        source: std::io::Error,
    },
    /// The project file could not be written.
    Write {
        /// Project file path.
        path: PathBuf,
        /// Underlying I/O failure.
        source: std::io::Error,
    },
    /// The project file was not valid TOML or did not match the schema.
    Parse {
        /// Project file path.
        path: PathBuf,
        /// TOML parser failure.
        source: toml::de::Error,
    },
    /// The serialized project could not be produced.
    Serialize {
        /// Project file path.
        path: PathBuf,
        /// TOML serializer failure.
        source: toml::ser::Error,
    },
    /// The file parsed but its contents are inconsistent.
    Invalid {
        /// Project file path.
        path: PathBuf,
        /// Which invariant failed.
        detail: String,
    },
    /// The file was written by a newer version of the application.
    UnsupportedVersion {
        /// Project file path.
        path: PathBuf,
        /// Version found in the file.
        found: u32,
    },
    /// No media is open, so there is nothing to save.
    NoTimeline,
}

impl fmt::Display for ProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(formatter, "reading project {}: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(formatter, "writing project {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(formatter, "parsing project {}: {source}", path.display())
            }
            Self::Serialize { path, source } => {
                write!(
                    formatter,
                    "serializing project {}: {source}",
                    path.display()
                )
            }
            Self::Invalid { path, detail } => {
                write!(formatter, "invalid project {}: {detail}", path.display())
            }
            Self::UnsupportedVersion { path, found } => write!(
                formatter,
                "project {} uses version {found}, this build supports {PROJECT_VERSION}",
                path.display()
            ),
            Self::NoTimeline => formatter.write_str("no media is open; nothing to save"),
        }
    }
}

impl std::error::Error for ProjectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Serialize { source, .. } => Some(source),
            Self::Invalid { .. } | Self::UnsupportedVersion { .. } | Self::NoTimeline => None,
        }
    }
}

/// `project.rrc` layout.
#[derive(Debug, Serialize, Deserialize)]
struct ProjectFile {
    version: u32,
    source: ProjectSource,
    timeline: ProjectTimeline,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProjectSource {
    path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProjectTimeline {
    next_id: u64,
    #[serde(default)]
    segments: Vec<ProjectSegment>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProjectSegment {
    id: u64,
    /// Start on the source timeline, in microseconds.
    start: i64,
    /// End on the source timeline, in microseconds.
    end: i64,
    #[serde(default = "default_enabled")]
    enabled: bool,
    /// Export chapter title, when one was set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chapter_title: Option<String>,
}

fn default_enabled() -> bool {
    true
}

/// A parsed project, ready to be applied once the media is probed.
#[derive(Debug)]
struct ProjectData {
    source: PathBuf,
    segments: Vec<Segment>,
    next_id: u64,
    chapter_titles: BTreeMap<SegmentId, String>,
}

impl Application {
    /// Writes the current timeline to `path`.
    pub fn save_project(&self, path: &Path) -> Result<(), ProjectError> {
        let Some(source) = self.timeline.source() else {
            return Err(ProjectError::NoTimeline);
        };

        let segments = self
            .timeline
            .segments()
            .iter()
            .map(|segment| ProjectSegment {
                id: segment.id().get(),
                start: segment.start().micros(),
                end: segment.end().micros(),
                enabled: segment.enabled(),
                chapter_title: self.chapter_titles.get(&segment.id()).cloned(),
            })
            .collect();

        let file = ProjectFile {
            version: PROJECT_VERSION,
            source: ProjectSource {
                path: source.to_path_buf(),
            },
            timeline: ProjectTimeline {
                next_id: self.timeline.next_id(),
                segments,
            },
        };

        let text = toml::to_string_pretty(&file).map_err(|source| ProjectError::Serialize {
            path: path.to_path_buf(),
            source,
        })?;
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|source| ProjectError::Write {
                path: path.to_path_buf(),
                source,
            })?;
        }
        fs::write(path, text).map_err(|source| ProjectError::Write {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Loads `path` and starts probing its source media.
    ///
    /// When the source file is missing the segments are still loaded so the
    /// project is readable; the open state reports the missing file and the
    /// caller can offer [`Application::rebind_source`].
    pub fn load_project(&mut self, path: &Path) -> Result<(), ProjectError> {
        let project = read_project(path)?;
        let ProjectData {
            source,
            segments,
            next_id,
            chapter_titles,
        } = project;
        let timeline =
            Timeline::restore(source.clone(), segments.clone(), next_id).ok_or_else(|| {
                ProjectError::Invalid {
                    path: path.to_path_buf(),
                    detail: "the segments or the ID counter are inconsistent".to_owned(),
                }
            })?;

        self.reset_editing();
        self.timeline = timeline;
        self.chapter_titles = chapter_titles.clone();
        self.timeline_revision += 1;

        if source.is_file() {
            self.pending_project = Some(PendingProject {
                segments,
                next_id,
                chapter_titles,
            });
            self.start_probe(source);
            return Ok(());
        }

        self.pending_project = None;
        self.media = None;
        self.state = OpenState::Failed {
            path: source,
            message: "the project source file is missing; open it to rebind".to_owned(),
        };
        Ok(())
    }

    /// Re-probes `path` while keeping the current segments.
    ///
    /// Used when a project source moved and the user picks a replacement.
    pub fn rebind_source(&mut self, path: PathBuf) {
        let segments = self.timeline.segments().to_vec();
        let next_id = self.timeline.next_id();
        let chapter_titles = self.chapter_titles.clone();
        self.pending_project = Some(PendingProject {
            segments,
            next_id,
            chapter_titles,
        });
        self.timeline.rebind(path.clone());
        self.start_probe(path);
    }

    /// Returns a counter that changes whenever the timeline content changes.
    ///
    /// The UI uses it to decide when an autosave is due.
    pub fn timeline_revision(&self) -> u64 {
        self.timeline_revision
    }

    /// Returns the project source path when a loaded project's media is gone.
    ///
    /// The timeline is still populated, so the UI can show the missing source
    /// instead of an empty editor.
    pub fn missing_project_source(&self) -> Option<&Path> {
        match &self.state {
            OpenState::Failed { path, .. }
                if self.media.is_none() && self.timeline.source().is_some() =>
            {
                Some(path.as_path())
            }
            _ => None,
        }
    }
}

fn read_project(path: &Path) -> Result<ProjectData, ProjectError> {
    let text = fs::read_to_string(path).map_err(|source| ProjectError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let file: ProjectFile = toml::from_str(&text).map_err(|source| ProjectError::Parse {
        path: path.to_path_buf(),
        source,
    })?;

    if file.version != PROJECT_VERSION {
        return Err(ProjectError::UnsupportedVersion {
            path: path.to_path_buf(),
            found: file.version,
        });
    }
    if file.source.path.as_os_str().is_empty() {
        return Err(ProjectError::Invalid {
            path: path.to_path_buf(),
            detail: "the source path is empty".to_owned(),
        });
    }

    let mut segments = Vec::with_capacity(file.timeline.segments.len());
    let mut chapter_titles = BTreeMap::new();
    for saved in file.timeline.segments {
        let id = SegmentId::new(saved.id);
        let Some(segment) = Segment::new(
            id,
            Timestamp::from_micros(saved.start),
            Timestamp::from_micros(saved.end),
        ) else {
            return Err(ProjectError::Invalid {
                path: path.to_path_buf(),
                detail: format!("segment {id} has an empty or reversed range"),
            });
        };
        if let Some(title) = saved.chapter_title {
            let title = title.trim();
            if !title.is_empty() {
                chapter_titles.insert(id, title.to_owned());
            }
        }
        segments.push(segment.with_enabled(saved.enabled));
    }

    Ok(ProjectData {
        source: file.source.path,
        segments,
        next_id: file.timeline.next_id,
        chapter_titles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::time::Timestamp;

    fn temporary_project(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "rossresscut-project-{name}-{}.rrc",
            std::process::id()
        ));
        path
    }

    fn application_with_segments() -> Application {
        let mut application = Application::new();
        application.timeline = Timeline::new(PathBuf::from("/media/clip.mp4"));
        application
            .timeline
            .push(
                Timestamp::from_micros(1_000_000),
                Timestamp::from_micros(2_000_000),
            )
            .expect("segment");
        let second = application
            .timeline
            .push(
                Timestamp::from_micros(5_000_000),
                Timestamp::from_micros(6_000_000),
            )
            .expect("segment");
        application.timeline.set_enabled(second, false);
        application.timeline.move_up(second);
        application
    }

    #[test]
    fn saves_and_restores_segments_with_identity() {
        let path = temporary_project("roundtrip");
        let application = application_with_segments();
        application.save_project(&path).expect("save");

        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("version = 1"));
        assert!(text.contains("[[timeline.segments]]"));

        let data = read_project(&path).expect("parse");
        fs::remove_file(&path).ok();

        assert_eq!(data.source, PathBuf::from("/media/clip.mp4"));
        assert_eq!(data.next_id, 3);
        assert_eq!(data.segments.len(), 2);
        assert_eq!(data.segments[0].id().get(), 2);
        assert!(!data.segments[0].enabled());
        assert_eq!(data.segments[1].id().get(), 1);
    }

    #[test]
    fn saves_and_restores_chapter_titles() {
        let path = temporary_project("chapters");
        let mut application = application_with_segments();
        let first = application.timeline.segments()[1].id();
        application.set_chapter_title(first, "Intro".to_owned());
        application.save_project(&path).expect("save");

        let data = read_project(&path).expect("parse");
        fs::remove_file(&path).ok();

        assert_eq!(
            data.chapter_titles.get(&first).map(String::as_str),
            Some("Intro")
        );
    }

    #[test]
    fn rejects_reversed_ranges_on_load() {
        let path = temporary_project("reversed");
        fs::write(
            &path,
            "version = 1\n[source]\npath = \"/media/clip.mp4\"\n[timeline]\nnext_id = 2\n\
             [[timeline.segments]]\nid = 1\nstart = 5000\nend = 1000\n",
        )
        .expect("write");

        let error = read_project(&path).expect_err("invalid range");
        fs::remove_file(&path).ok();

        assert!(error.to_string().contains("empty or reversed"));
    }

    #[test]
    fn rejects_an_unknown_version() {
        let path = temporary_project("version");
        fs::write(
            &path,
            "version = 99\n[source]\npath = \"/media/clip.mp4\"\n[timeline]\nnext_id = 1\n",
        )
        .expect("write");

        let error = read_project(&path).expect_err("unsupported version");
        fs::remove_file(&path).ok();

        assert!(error.to_string().contains("supports 1"));
    }

    #[test]
    fn rejects_id_at_or_above_the_counter() {
        let path = temporary_project("counter");
        fs::write(
            &path,
            "version = 1\n[source]\npath = \"/media/clip.mp4\"\n[timeline]\nnext_id = 1\n\
             [[timeline.segments]]\nid = 1\nstart = 0\nend = 1000\n",
        )
        .expect("write");

        let mut application = Application::new();
        let error = application
            .load_project(&path)
            .expect_err("counter too small");
        fs::remove_file(&path).ok();

        assert!(error.to_string().contains("inconsistent"));
    }

    #[test]
    fn saving_without_media_is_rejected() {
        let path = temporary_project("empty");
        let application = Application::new();

        let error = application.save_project(&path).expect_err("no timeline");

        assert!(error.to_string().contains("nothing to save"));
    }
}
