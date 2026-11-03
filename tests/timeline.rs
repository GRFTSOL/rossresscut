//! Integration tests for segment editing commands.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use rossresscut::application::{Application, OpenState, SegmentCommandError};
use rossresscut::domain::segment::{SegmentId, Timeline};
use rossresscut::domain::time::Timestamp;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn wait_until_ready(application: &mut Application) {
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

fn micros(value: i64) -> Timestamp {
    Timestamp::from_micros(value)
}

fn output_order(application: &Application) -> Vec<SegmentId> {
    application
        .timeline()
        .segments()
        .iter()
        .map(|segment| segment.id())
        .collect()
}

fn timeline_order(timeline: &Timeline) -> Vec<SegmentId> {
    timeline
        .segments()
        .iter()
        .map(|segment| segment.id())
        .collect()
}

/// Guardrail for export: the EDL order is independent of source time, and no
/// operation may silently re-sort it by `start`.
#[test]
fn edl_order_is_independent_of_source_time() {
    let mut timeline = Timeline::new(PathBuf::from("source.mp4"));
    let a = timeline
        .push(micros(0), micros(10_000_000))
        .expect("A is valid");
    let b = timeline
        .push(micros(20_000_000), micros(30_000_000))
        .expect("B is valid");
    let c = timeline
        .push(micros(5_000_000), micros(15_000_000))
        .expect("C is valid");

    let selected = c;

    // Build B -> C -> A.
    assert!(timeline.move_to(b, 0));
    assert!(timeline.move_to(c, 1));
    assert_eq!(timeline_order(&timeline), vec![b, c, a]);

    // Moving a segment must not trigger a source-time sort.
    assert!(timeline.move_up(c));
    assert_eq!(timeline_order(&timeline), vec![c, b, a]);
    assert_eq!(
        timeline.get(selected).map(|segment| segment.id()),
        Some(selected)
    );

    // Only an explicit sort may restore source-time order.
    timeline.sort_by_start();
    assert_eq!(timeline_order(&timeline), vec![a, c, b]);
    assert_eq!(
        timeline.get(selected).map(|segment| segment.id()),
        Some(selected)
    );
}

#[test]
fn selected_id_survives_reorder_and_sort() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    // Same source-time relationship as the Domain test, scaled to the fixture:
    // A = [0, 1s), B = [2s, 3s), C = [0.5s, 1.5s).
    application.set_in_point(micros(0));
    application.set_out_point(micros(1_000_000));
    let a = application.add_segment().expect("A");

    application.set_in_point(micros(2_000_000));
    application.set_out_point(micros(3_000_000));
    let b = application.add_segment().expect("B");

    application.set_in_point(micros(500_000));
    application.set_out_point(micros(1_500_000));
    let c = application.add_segment().expect("C");

    assert_eq!(output_order(&application), vec![a, b, c]);

    // Build B -> C -> A.
    application.select_segment(Some(b));
    assert!(application.move_selected_segment_up());
    application.select_segment(Some(c));
    assert!(application.move_selected_segment_up());
    assert_eq!(output_order(&application), vec![b, c, a]);

    // Selection is tracked by SegmentId, not by position.
    application.select_segment(Some(c));
    assert_eq!(application.selected_segment(), Some(c));
    assert!(application.move_selected_segment_up());
    assert_eq!(output_order(&application), vec![c, b, a]);
    assert_eq!(application.selected_segment(), Some(c));

    // Explicit sort changes output order but not identity or selection.
    application.sort_segments();
    assert_eq!(output_order(&application), vec![a, c, b]);
    assert_eq!(application.selected_segment(), Some(c));
    assert!(application.timeline().get(c).is_some());
}

#[test]
fn output_order_is_creation_order_until_explicitly_sorted() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.set_in_point(micros(200_000));
    application.set_out_point(micros(800_000));
    let first = application.add_segment().expect("first segment");

    application.set_in_point(micros(1_200_000));
    application.set_out_point(micros(2_000_000));
    let second = application.add_segment().expect("second segment");

    application.set_in_point(micros(0));
    application.set_out_point(micros(100_000));
    let third = application.add_segment().expect("third segment");

    assert_ne!(first, second);
    assert_ne!(second, third);
    assert_eq!(application.selected_segment(), Some(third));
    assert_eq!(output_order(&application), vec![first, second, third]);

    application.sort_segments();

    assert_eq!(output_order(&application), vec![third, first, second]);
    assert_eq!(application.selected_segment(), Some(third));
}

#[test]
fn reorders_segments_and_keeps_selection_by_id() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.set_in_point(micros(0));
    application.set_out_point(micros(500_000));
    let first = application.add_segment().expect("first segment");

    application.set_in_point(micros(1_000_000));
    application.set_out_point(micros(1_500_000));
    let second = application.add_segment().expect("second segment");

    application.set_in_point(micros(2_000_000));
    application.set_out_point(micros(2_500_000));
    let third = application.add_segment().expect("third segment");

    application.select_segment(Some(second));
    assert!(application.move_selected_segment_up());
    assert_eq!(output_order(&application), vec![second, first, third]);
    assert_eq!(application.selected_segment(), Some(second));

    assert!(application.move_selected_segment_down());
    assert_eq!(output_order(&application), vec![first, second, third]);
    assert_eq!(application.selected_segment(), Some(second));
}

#[test]
fn edits_enabled_state_and_bounds_by_id() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.set_in_point(micros(200_000));
    application.set_out_point(micros(800_000));
    let id = application.add_segment().expect("segment");

    assert!(application.set_segment_enabled(id, false));
    assert!(!application.timeline().segments()[0].enabled());

    assert_eq!(
        application.set_selected_segment_start(micros(50_000)),
        Ok(())
    );
    assert_eq!(
        application.set_selected_segment_end(micros(900_000)),
        Ok(())
    );

    let segment = application
        .timeline()
        .segments()
        .iter()
        .find(|segment| segment.id() == id)
        .copied()
        .expect("segment by id");
    assert_eq!(segment.start(), micros(50_000));
    assert_eq!(segment.end(), micros(900_000));
    assert!(!segment.enabled());

    assert_eq!(
        application.set_selected_segment_start(micros(1_000_000)),
        Err(SegmentCommandError::EmptyRange)
    );
    assert_eq!(
        application.set_selected_segment_end(micros(0)),
        Err(SegmentCommandError::EmptyRange)
    );
}

#[test]
fn removes_selected_segment_and_selects_a_neighbour() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.set_in_point(micros(0));
    application.set_out_point(micros(500_000));
    let first = application.add_segment().expect("first segment");

    application.set_in_point(micros(1_000_000));
    application.set_out_point(micros(1_500_000));
    let second = application.add_segment().expect("second segment");

    application.select_segment(Some(first));
    assert_eq!(
        application
            .remove_selected_segment()
            .map(|segment| segment.id()),
        Some(first)
    );

    assert_eq!(application.timeline().len(), 1);
    assert_eq!(application.selected_segment(), Some(second));
}

#[test]
fn rejects_incomplete_or_reversed_markers() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    assert_eq!(
        application.add_segment(),
        Err(SegmentCommandError::MissingInPoint)
    );

    application.set_in_point(micros(500_000));
    assert_eq!(
        application.add_segment(),
        Err(SegmentCommandError::MissingOutPoint)
    );

    application.set_out_point(micros(500_000));
    assert_eq!(
        application.add_segment(),
        Err(SegmentCommandError::EmptyRange)
    );
}

#[test]
fn closing_media_clears_the_timeline() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.set_in_point(micros(0));
    application.set_out_point(micros(500_000));
    assert!(application.add_segment().is_ok());

    application.close_media();

    assert!(application.timeline().is_empty());
    assert_eq!(application.timeline().source(), None);
    assert_eq!(application.selected_segment(), None);
}

#[test]
fn undo_redo_copy_paste_and_clear_keep_identity() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.set_in_point(micros(0));
    application.set_out_point(micros(1_000_000));
    let first = application.add_segment().expect("first segment");

    assert!(application.can_undo());
    assert!(application.undo());
    assert!(application.timeline().is_empty());
    assert!(application.can_redo());
    assert!(application.redo());
    assert_eq!(output_order(&application), vec![first]);

    application.select_segment(Some(first));
    assert_eq!(application.copy_selected_segments(), 1);
    assert_eq!(application.paste_segments(), 1);
    assert_eq!(application.timeline().len(), 2);
    assert_ne!(output_order(&application)[1], first);

    application.select_segment(Some(first));
    assert_eq!(application.cut_selected_segments(), 1);
    assert_eq!(application.timeline().len(), 1);

    application.select_all_segments();
    assert_eq!(application.selected_ids().len(), 1);

    application.clear_segments();
    assert!(application.timeline().is_empty());
    assert!(application.undo());
    assert_eq!(application.timeline().len(), 1);
}

#[test]
fn random_shuffle_and_merge_stay_valid() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    let created = application.add_random_segments(3).expect("random segments");
    assert_eq!(created, 3);
    assert_eq!(application.timeline().len(), 3);
    let mut ranges: Vec<_> = application.timeline().segments().to_vec();
    ranges.sort_by_key(|segment| segment.start());
    for pair in ranges.windows(2) {
        assert!(
            pair[0].end() <= pair[1].start(),
            "random segments must not overlap"
        );
    }

    application.clear_segments();
    application.set_in_point(micros(0));
    application.set_out_point(micros(2_000_000));
    let wide = application.add_segment().expect("wide segment");
    application.set_in_point(micros(1_000_000));
    application.set_out_point(micros(3_000_000));
    application.add_segment().expect("overlapping segment");

    application.select_segment(Some(wide));
    application.shuffle_segments();
    application.sort_segments();
    let merged = application.merge_overlapping_segments();
    assert!(merged >= 1, "overlapping segments should merge");
    assert!(application.timeline().get(wide).is_some());
}

#[test]
fn inverting_segments_creates_the_complement() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.set_in_point(micros(0));
    application.set_out_point(micros(1_000_000));
    application.add_segment().expect("first segment");

    let count = application.invert_segments();
    assert_eq!(count, 1);
    let segments = application.timeline().segments();
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].start(), micros(1_000_000));
    assert!(segments[0].end() > micros(1_000_000));
}

#[test]
fn cut_shortcut_edits_the_selected_segment_head_and_tail() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.set_in_point(micros(500_000));
    application.set_out_point(micros(1_500_000));
    let id = application.add_segment().expect("segment");
    application.select_segment(Some(id));

    // A playhead inside the selected segment moves its head.
    application
        .add_cut_point_or_set_start(micros(700_000))
        .expect("set start");
    let segment = *application.timeline().get(id).expect("segment");
    assert_eq!(segment.start(), micros(700_000));
    assert_eq!(segment.end(), micros(1_500_000));

    // The same contextual family sets the tail.
    application
        .set_end_or_complete_cut_point(micros(1_200_000))
        .expect("set end");
    let segment = *application.timeline().get(id).expect("segment");
    assert_eq!(segment.end(), micros(1_200_000));
}

#[test]
fn cut_shortcut_after_the_segment_end_starts_a_new_marker() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.set_in_point(micros(500_000));
    application.set_out_point(micros(1_500_000));
    let id = application.add_segment().expect("segment");
    application.select_segment(Some(id));

    // The playhead sits after the selected segment, so the shortcut starts a
    // new cut point instead of moving the existing head.
    application
        .add_cut_point_or_set_start(micros(2_500_000))
        .expect("new cut point");
    assert_eq!(application.in_point(), Some(micros(2_500_000)));
    let segment = *application.timeline().get(id).expect("segment");
    assert_eq!(segment.start(), micros(500_000));
}

#[test]
fn cut_shortcut_without_a_selection_completes_the_cut_point() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.select_segment(None);
    application
        .add_cut_point_or_set_start(micros(500_000))
        .expect("in point");
    assert_eq!(application.in_point(), Some(micros(500_000)));

    // The same family closes the pending point instead of deleting it.
    application
        .set_end_or_complete_cut_point(micros(800_000))
        .expect("out point");
    assert!(application.in_point().is_none());
    let segments = application.timeline().segments();
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].start(), micros(500_000));
    assert_eq!(segments[0].end(), micros(800_000));
}

#[test]
fn a_pending_cut_point_takes_precedence_over_the_selection() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.set_in_point(micros(0));
    application.set_out_point(micros(1_000_000));
    let existing = application.add_segment().expect("existing segment");

    application.select_segment(Some(existing));
    application
        .add_cut_point_or_set_start(micros(2_000_000))
        .expect("pending point");
    assert_eq!(application.in_point(), Some(micros(2_000_000)));

    // Alt+O finishes the pending point instead of moving the selected segment.
    application
        .set_end_or_complete_cut_point(micros(2_500_000))
        .expect("complete the pending point");

    let unchanged = *application.timeline().get(existing).expect("existing");
    assert_eq!(unchanged.end(), micros(1_000_000));
    assert_eq!(application.timeline().segments().len(), 2);
    let created = application.timeline().segments()[1];
    assert_eq!(created.start(), micros(2_000_000));
    assert_eq!(created.end(), micros(2_500_000));
}

#[test]
fn a_pending_cut_point_keeps_alt_i_in_the_cut_flow() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.set_in_point(micros(0));
    application.set_out_point(micros(1_000_000));
    let existing = application.add_segment().expect("existing segment");

    // Start a pending point without a selection, then select the old segment.
    application.select_segment(None);
    application
        .add_cut_point_or_set_start(micros(500_000))
        .expect("pending point");
    application.select_segment(Some(existing));

    // Alt+I continues the pending cut even though the playhead is inside the
    // selected segment; the old segment must not lose its head.
    application
        .add_cut_point_or_set_start(micros(800_000))
        .expect("complete the pending point");

    let unchanged = *application.timeline().get(existing).expect("existing");
    assert_eq!(unchanged.start(), micros(0));
    assert_eq!(application.timeline().segments().len(), 2);
}

#[test]
fn completing_a_cut_point_rejects_a_reversed_range() {
    let mut application = Application::new();
    application.open_media(fixture("sample-keyframes.mp4"));
    wait_until_ready(&mut application);

    application.select_segment(None);
    application
        .add_cut_point_or_set_start(micros(1_000_000))
        .expect("in point");

    let error = application
        .set_end_or_complete_cut_point(micros(500_000))
        .expect_err("end before start");
    assert_eq!(error, SegmentCommandError::EmptyRange);
    assert_eq!(application.in_point(), Some(micros(1_000_000)));
    assert!(application.timeline().segments().is_empty());
}
