//! Smart-cut planning for multi-segment exports.
//!
//! Planning probes where a stream-copy seek lands, so a copy piece starts at
//! its planned keyframe even when the container exposes sparse cue points. It
//! runs on a worker thread because it starts FFmpeg probes.

use std::path::Path;

use crate::domain::time::{Duration, Timestamp};
use crate::media::ffprobe;

use super::concat::{ExportSegment, MultiExportError, SmartCutContext};
use super::smart_cut::{CopyBoundary, SmartCutPlan, SmartCutUnavailable};

/// Probes where a stream-copy seek lands, caching repeated targets.
struct SeekProbe<'a> {
    source: &'a Path,
    stream_index: usize,
    start_time: i64,
    cache: Vec<(Timestamp, Option<Timestamp>)>,
}

impl SeekProbe<'_> {
    fn landing(&mut self, target: Timestamp) -> Result<Option<Timestamp>, MultiExportError> {
        if let Some((_, landing)) = self.cache.iter().find(|(key, _)| *key == target) {
            return Ok(*landing);
        }
        let landing = ffprobe::probe_seek_start(self.source, self.stream_index, target)
            .map_err(MultiExportError::Probe)?
            .map(|landing| {
                Timestamp::from_micros(landing.micros().saturating_sub(self.start_time))
            });
        self.cache.push((target, landing));
        Ok(landing)
    }
}

/// Splits every segment according to the smart-cut settings.
pub(super) fn plan_segments(
    source: &Path,
    segments: &[ExportSegment],
    context: Option<&SmartCutContext>,
) -> Result<Vec<SmartCutPlan>, MultiExportError> {
    let Some(context) = context else {
        return Ok(segments
            .iter()
            .map(|segment| SmartCutPlan::copy_only(segment.start(), segment.end()))
            .collect());
    };
    let mut probe = SeekProbe {
        source,
        stream_index: context.stream_index,
        start_time: context.start_time.map_or(0, Timestamp::micros),
        cache: Vec::new(),
    };
    segments
        .iter()
        .enumerate()
        .map(|(index, segment)| plan_segment(&mut probe, *segment, context, index))
        .collect()
}

fn plan_segment(
    probe: &mut SeekProbe<'_>,
    segment: ExportSegment,
    context: &SmartCutContext,
    index: usize,
) -> Result<SmartCutPlan, MultiExportError> {
    let start = segment.start();
    let end = segment.end();
    let next = context
        .keyframes
        .at_or_after(start)
        .ok_or(MultiExportError::SmartCut {
            index,
            reason: SmartCutUnavailable::NoKeyframeAfterStart,
        })?;

    let Some(copy) = find_copy_start(probe, start, next, end)? else {
        return Ok(SmartCutPlan::encoded(start, end));
    };
    SmartCutPlan::plan_with_seek(
        start,
        end,
        copy,
        &context.keyframes,
        Some(context.frame_duration),
    )
    .map_err(|reason| MultiExportError::SmartCut { index, reason })
}

/// Finds a copy start at or after `start` that a stream-copy seek reaches.
///
/// `-ss <keyframe>` can land on an earlier Matroska cue point. A later target
/// makes the demuxer land on the planned keyframe instead, so the head can be
/// encoded up to that point and the copy takes nothing from before it.
fn find_copy_start(
    probe: &mut SeekProbe<'_>,
    start: Timestamp,
    next: Timestamp,
    end: Timestamp,
) -> Result<Option<CopyBoundary>, MultiExportError> {
    find_copy_start_with(start, next, end, |target| probe.landing(target))
}

fn find_copy_start_with(
    start: Timestamp,
    next: Timestamp,
    end: Timestamp,
    mut landing: impl FnMut(Timestamp) -> Result<Option<Timestamp>, MultiExportError>,
) -> Result<Option<CopyBoundary>, MultiExportError> {
    let step = Duration::from_micros(500_000);
    let mut target = next;
    for _ in 0..40 {
        if target >= end {
            return Ok(None);
        }
        match landing(target)? {
            Some(found) if found >= start && found < end => {
                return Ok(Some(CopyBoundary {
                    start: found,
                    seek: target,
                }));
            }
            // No packet means the probe could not read this source; encoding
            // the whole range is safer than trying every later target.
            None => return Ok(None),
            Some(_) => {}
        }
        let Some(advanced) = target.checked_add(step) else {
            return Ok(None);
        };
        target = advanced;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_a_seek_target_that_reaches_the_planned_keyframe() {
        let start = Timestamp::from_micros(60_000_000);
        let next = Timestamp::from_micros(60_560_000);
        let end = Timestamp::from_micros(66_000_000);
        let boundary = Timestamp::from_micros(61_060_000);

        let found = find_copy_start_with(start, next, end, |target| {
            Ok(Some(if target < boundary {
                Timestamp::from_micros(53_220_000)
            } else {
                Timestamp::from_micros(60_560_000)
            }))
        })
        .expect("searched");

        assert_eq!(
            found,
            Some(CopyBoundary {
                start: Timestamp::from_micros(60_560_000),
                seek: boundary,
            })
        );
    }

    #[test]
    fn gives_up_when_no_seek_reaches_the_segment() {
        let start = Timestamp::from_micros(60_000_000);
        let next = Timestamp::from_micros(60_560_000);
        let end = Timestamp::from_micros(66_000_000);

        let found = find_copy_start_with(start, next, end, |_| {
            Ok(Some(Timestamp::from_micros(53_220_000)))
        })
        .expect("searched");

        assert_eq!(found, None);
    }
}
