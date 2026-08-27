use super::*;
use std::fs;

fn metadata() -> VideoMetadata {
    VideoMetadata {
        path: PathBuf::from("/tmp/input.mp4"),
        duration_seconds: 10.0,
        width: 1920,
        height: 1080,
        file_size_bytes: 100 * 1024 * 1024,
        has_audio: true,
    }
}

fn attach_pointer(state: &mut VideoEditState, x: f64, y: f64) {
    let mut sidecar = crate::recording::editor::sidecar::PointerSidecar::new(
        0,
        crate::recording::editor::sidecar::CaptureRegion {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        },
    );
    sidecar
        .pointer
        .push(crate::recording::editor::sidecar::PointerSample {
            t: 0.0,
            x,
            y,
            kind: crate::recording::editor::sidecar::CursorKind::Default,
        });
    state.sidecar = Some(sidecar);
}

#[test]
fn has_source_video_requires_duration() {
    let mut state = VideoEditState::new(metadata());
    assert!(state.has_source_video());
    state.metadata.duration_seconds = 0.0;
    assert!(!state.has_source_video());
}

#[test]
fn default_session_is_not_dirty_and_trim_is() {
    let state = VideoEditState::new(metadata());
    assert!(state.session_is_default());
    assert!(!state.session_is_dirty(None));
    let mut trimmed = state.clone();
    trimmed.trim_start_seconds = 1.0;
    assert!(trimmed.session_is_dirty(None));
    let last = trimmed.to_project();
    assert!(!trimmed.session_is_dirty(Some(&last)));
}

#[test]
fn output_path_adds_edited_suffix() {
    let path = PathBuf::from("/tmp/ApexShot Recording.mp4");
    assert_eq!(
        edited_output_path(&path),
        PathBuf::from("/tmp/ApexShot Recording-edited.mp4")
    );
}

#[test]
fn sanitize_title_strips_illegal_chars_and_empty_becomes_untitled() {
    assert_eq!(sanitize_title("  My / Clip?  "), "My Clip");
    assert_eq!(sanitize_title(":::"), "Untitled");
    assert_eq!(
        title_from_path(Path::new("/tmp/ApexShot Recording.mp4")),
        "ApexShot Recording"
    );
}

#[test]
fn set_title_updates_state_and_export_path() {
    let mut state = VideoEditState::new(metadata());
    assert_eq!(state.title, "input");
    state.set_title("  Demo / Take 1  ");
    assert_eq!(state.title, "Demo Take 1");
    assert_eq!(state.project_media[0].display_name, "Demo Take 1");
    assert_eq!(state.project_media[1].display_name, "Demo Take 1 audio");
    assert_eq!(
        state.export_path(),
        PathBuf::from("/tmp/Demo Take 1-edited.mp4")
    );
}

#[test]
fn rail_lock_blocks_video_edits_and_zoom() {
    let mut state = VideoEditState::new(metadata());
    state.video_locked = true;
    state.set_trim_start(1.0);
    state.add_cut(3.0);
    assert_eq!(state.trim_start_seconds, 0.0);
    assert!(state.cuts.is_empty());

    state.video_locked = false;
    state.add_cut(3.0);
    assert_eq!(state.cuts, vec![3.0]);
    state.video_locked = true;
    state.reset_video_edits();
    assert_eq!(state.cuts, vec![3.0]);

    state.zoom_locked = true;
    assert!(state.add_zoom_at_playhead().is_none());
    assert!(state.zoom_clips.is_empty());
}

#[test]
fn toggle_mute_and_remove_audio_track() {
    let mut state = VideoEditState::new(metadata());
    assert!(state.has_audio_track());
    assert!(!state.is_muted());
    state.toggle_mute();
    assert!(state.is_muted());
    assert_eq!(state.audio_mode, AudioMode::Muted);

    state.audio_locked = true;
    state.toggle_mute();
    assert!(state.is_muted());
    state.remove_audio_track();
    assert!(state.has_audio_track());

    state.audio_locked = false;
    state.remove_audio_track();
    assert!(!state.has_audio_track());
    assert!(state.is_muted());
}

#[test]
fn hidden_zoom_skips_eval_and_clear_zoom_clips() {
    let mut state = VideoEditState::new(metadata());
    assert!(state.add_zoom_at_playhead().is_some());
    assert!(state.has_zoom_track());
    state.zoom_hidden = true;
    let (scale, center) = state.eval_zoom(0.5);
    assert_eq!(scale, 1.0);
    assert_eq!(center, (960.0, 540.0));
    assert!(!state.needs_composite());

    state.clear_zoom_clips();
    assert!(!state.has_zoom_track());
    assert!(!state.zoom_hidden);
}

#[test]
fn output_path_increments_when_existing_file_present() {
    let dir =
        std::env::temp_dir().join(format!("apexshot-video-editor-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join("recording.mp4");
    fs::write(dir.join("recording-edited.mp4"), b"existing").unwrap();
    fs::write(dir.join("recording-edited-2.mp4"), b"existing").unwrap();

    assert_eq!(
        edited_output_path(&input),
        dir.join("recording-edited-3.mp4")
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn trim_range_clamps_to_duration() {
    let mut state = VideoEditState::new(metadata());
    state.set_trim_start(-10.0);
    state.set_trim_end(50.0);

    assert_eq!(state.trim_start_seconds, 0.0);
    assert_eq!(state.trim_end_seconds, 10.0);
}

#[test]
fn trim_range_enforces_min_duration() {
    let mut state = VideoEditState::new(metadata());
    state.set_trim_start(9.95);

    assert_eq!(state.trim_start_seconds, 9.75);
    state.set_trim_end(9.8);
    assert_eq!(state.trim_end_seconds, 10.0);
}

#[test]
fn move_cut_keeps_cut_between_neighbors() {
    let mut state = VideoEditState::new(metadata());
    state.add_cut(3.0);
    state.add_cut(7.0);

    state.move_cut(0, 6.0);
    assert_eq!(state.cuts, vec![6.0, 7.0]);

    state.move_cut(0, 8.0);
    assert!((state.cuts[0] - 6.9).abs() < f64::EPSILON);
    assert_eq!(state.cuts[1], 7.0);

    state.move_cut(1, 0.0);
    assert!((state.cuts[0] - 6.9).abs() < f64::EPSILON);
    assert_eq!(state.cuts[1], 7.0);
}

#[test]
fn quality_maps_to_expected_crf_values() {
    assert_eq!(quality_to_crf(100), 18);
    assert_eq!(quality_to_crf(70), 22);
    assert_eq!(quality_to_crf(0), 32);
}

#[test]
fn extra_videos_get_their_own_tracks() {
    let mut state = VideoEditState::new(metadata());
    assert_eq!(state.video_tracks().len(), 1);
    assert!(state.extra_video_tracks().is_empty());

    state.add_project_media(ProjectMedia {
        path: PathBuf::from("/tmp/second.mp4"),
        display_name: "second".into(),
        kind: ProjectMediaKind::Video,
        duration_seconds: Some(4.0),
    });
    assert_eq!(state.video_tracks().len(), 2);
    assert_eq!(state.extra_video_tracks().len(), 1);
    assert_eq!(
        state.extra_video_tracks()[0].path,
        PathBuf::from("/tmp/second.mp4")
    );

    state.remove_project_media(Path::new("/tmp/input.mp4"), ProjectMediaKind::Video);
    assert_eq!(state.video_tracks().len(), 2);
    state.remove_project_media(Path::new("/tmp/second.mp4"), ProjectMediaKind::Video);
    assert_eq!(state.video_tracks().len(), 1);
    assert!(state.extra_video_tracks().is_empty());
}

#[test]
fn split_selects_left_segment_and_reorder_keeps_it() {
    let mut state = VideoEditState::new(metadata());
    state.add_cut(4.0);
    assert_eq!(state.cuts, vec![4.0]);
    assert_eq!(state.selected_segment, Some(0));
    assert_eq!(state.segment_order, vec![0, 1]);
    state.move_segment(1, 0);
    assert_eq!(state.segment_order, vec![1, 0]);
    assert_eq!(state.selected_segment, Some(0));
    state.clear_cuts();
    assert!(state.selected_segment.is_none());
}

#[test]
fn dragging_cut_segment_opens_a_gap() {
    let mut state = VideoEditState::new(metadata());
    state.add_cut(4.0);
    assert!((state.segment_start(0) - 0.0).abs() < 1e-9);
    assert!((state.segment_start(1) - 4.0).abs() < 1e-9);
    assert!(!state.has_segment_gaps());
    state.set_segment_start(1, 8.0);
    assert!((state.segment_start(0) - 0.0).abs() < 1e-9);
    assert!((state.segment_start(1) - 8.0).abs() < 1e-9);
    assert!(state.has_segment_gaps());
    assert!((state.composition_duration() - 14.0).abs() < 1e-9);
    assert!((state.source_to_timeline(5.0) - 9.0).abs() < 1e-9);
    state.set_segment_start(1, 3.0);
    assert!((state.segment_start(1) - 3.0).abs() < 1e-9);
    state.settle_segment_start(1);
    assert!((state.segment_start(1) - 4.0).abs() < 1e-9);
    state.set_segment_start(0, 4.5);
    state.settle_segment_start(0);
    assert!((state.segment_start(0) - 0.0).abs() < 1e-9);
    state.set_segment_start(0, 6.0);
    state.settle_segment_start(0);
    assert!((state.segment_start(0) - 10.0).abs() < 1e-9);
    assert!((state.segment_start(1) - 4.0).abs() < 1e-9);
}

#[test]
fn timeline_scale_zero_is_identity_mapping() {
    let state = VideoEditState::new(metadata());
    assert_eq!(state.time_to_x(0.0, 1000.0), 0.0);
    assert!((state.time_to_x(5.0, 1000.0) - 500.0).abs() < 0.01);
    assert!((state.x_to_time(250.0, 1000.0) - 2.5).abs() < 0.01);
}

#[test]
fn timeline_offset_shifts_clip_and_extends_composition() {
    let mut state = VideoEditState::new(metadata());
    assert_eq!(state.composition_duration(), 10.0);
    state.playhead_seconds = 2.0;
    state.set_timeline_offset(5.0);
    assert!((state.composition_duration() - 15.0).abs() < 1e-9);
    // Moving a clip pans it on a fixed ruler. Zoom and playhead stay put.
    assert!((state.visible_span_seconds() - 10.0).abs() < 1e-9);
    assert!((state.playhead_seconds - 2.0).abs() < 1e-9);
    assert!((state.source_to_x(0.0, 1000.0) - 500.0).abs() < 0.01);
    assert!((state.source_to_x(10.0, 1000.0) - 1500.0).abs() < 0.01);
    assert!((state.timeline_to_source(7.0) - 2.0).abs() < 1e-9);
    assert_eq!(state.timeline_scroll_seconds, 0.0);
    state.set_timeline_offset(-3.0);
    assert_eq!(state.timeline_offset_seconds, 0.0);
    state.set_timeline_offset(999.0);
    assert!((state.timeline_offset_seconds - 999.0).abs() < 1e-9);
    assert!(state.composition_duration() > state.source_duration());
    state.video_locked = true;
    state.set_timeline_offset(1.0);
    assert!((state.timeline_offset_seconds - 999.0).abs() < 1e-9);
    state.video_locked = false;
    state.reset_video_edits();
    assert_eq!(state.timeline_offset_seconds, 0.0);
    assert!(!state.video_has_edits());
    state.set_timeline_offset(2.0);
    assert!(state.video_has_edits());
    assert!(state.needs_reencode());
}

#[test]
fn timeline_stays_open_past_the_clip() {
    let mut state = VideoEditState::new(metadata());
    assert!((state.visible_span_seconds() - 10.0).abs() < 1e-9);
    state.timeline_scale = 100.0 / 7.0;
    state.set_timeline_scroll(10.0);
    assert!((state.x_to_time(0.0, 1000.0) - 10.0).abs() < 0.01);
    state.set_timeline_offset(240.0);
    state.follow_clip_on_timeline();
    assert!((state.timeline_offset_seconds - 240.0).abs() < 1e-9);
    assert!(state.timeline_canvas_seconds() > state.composition_duration());
    assert!(state.timeline_scroll_seconds > 0.0);
}

#[test]
fn format_webcut_time_matches_reference() {
    assert_eq!(format_webcut_time(0.0), "00:00:00.000");
    assert_eq!(format_webcut_time(84.56), "00:01:24.560");
    assert_eq!(format_webcut_time(3661.005), "01:01:01.005");
}

#[test]
fn closest_aspect_ratio_picks_nearest() {
    assert_eq!(closest_aspect_ratio(1920, 1080), "16:9");
    assert_eq!(closest_aspect_ratio(1080, 1080), "1:1");
    assert_eq!(closest_aspect_ratio(608, 1080), "9:16");
}

#[test]
fn apply_aspect_ratio_sets_custom_box() {
    let mut state = VideoEditState::new(metadata());
    state.apply_aspect_ratio(1080, 1080);
    assert_eq!(state.dimension_preset, DimensionPreset::Custom);
    assert_eq!((state.custom_width, state.custom_height), (1080, 1080));
    assert_eq!(state.canvas_dimensions(), (1080, 1080));
    assert_eq!(state.target_dimensions(), (1080, 608));
    assert_eq!(state.padded_output_dimensions(), (1080, 1080));
    assert!(state.needs_reencode());
    assert_eq!(state.canvas_label(), "1:1");
    state.reset_aspect_ratio();
    assert_eq!(state.dimension_preset, DimensionPreset::Original);
    assert_eq!(state.canvas_dimensions(), (1920, 1080));
    assert_eq!(state.canvas_label(), "Original");
    assert!(!state.needs_reencode());
}

#[test]
fn dimension_preset_original_uses_source_dimensions() {
    let state = VideoEditState::new(metadata());
    assert_eq!(state.target_dimensions(), (1920, 1080));
}

#[test]
fn dimension_preset_fits_inside_box_preserving_aspect() {
    let mut state = VideoEditState::new(metadata());
    state.dimension_preset = DimensionPreset::Custom;
    // Box is clamped to at least MIN_DIMENSION (64) on each side, then
    // the source is fitted inside without stretching.
    state.custom_width = 1919;
    state.custom_height = 57;

    let (w, h) = state.target_dimensions();
    assert_eq!((w, h), (114, 64));
    // Aspect roughly matches 16:9 source.
    let aspect = w as f64 / h as f64;
    let src_aspect = 1920.0 / 1080.0;
    assert!((aspect - src_aspect).abs() < 0.05);
}

#[test]
fn dimension_preset_does_not_upscale_or_stretch() {
    let mut state = VideoEditState::new(VideoMetadata {
        path: PathBuf::from("/tmp/input.mp4"),
        duration_seconds: 5.0,
        width: 600,
        height: 744,
        file_size_bytes: 1024,
        has_audio: false,
    });
    state.dimension_preset = DimensionPreset::P1080;
    let (w, h) = state.target_dimensions();
    assert!(w <= 600 && h <= 744);
    assert_eq!((w, h), (600, 744));
}

#[test]
fn needs_reencode_when_dimensions_or_quality_change() {
    let mut state = VideoEditState::new(metadata());
    assert!(!state.needs_reencode());

    state.quality = 40;
    assert!(state.needs_reencode());

    state.quality = 70;
    state.dimension_preset = DimensionPreset::P720;
    assert!(state.needs_reencode());
}

#[test]
fn needs_reencode_when_zoom_or_background_present() {
    let mut state = VideoEditState::new(metadata());
    assert!(!state.needs_reencode());
    state.zoom_clips.push(ZoomClip {
        start: 1.0,
        end: 2.8,
        scale: 1.8,
        center: (960.0, 540.0),
        ease_ms: 200,
        mode: ZoomMode::Auto,
    });
    assert!(state.needs_reencode());

    let mut padded = VideoEditState::new(metadata());
    padded.background = VideoBackground::Plain {
        r: 20,
        g: 20,
        b: 24,
    };
    assert!(padded.needs_reencode());
}

#[test]
fn eval_zoom_eases_in_and_out() {
    let clips = [ZoomClip {
        start: 1.0,
        end: 2.8,
        scale: 2.0,
        center: (200.0, 100.0),
        ease_ms: 200,
        mode: ZoomMode::Manual,
    }];
    let (outside, _) = eval_zoom(&clips, 0.5, 1920.0, 1080.0);
    assert!((outside - 1.0).abs() < 1e-9);

    let (hold, center) = eval_zoom(&clips, 1.9, 1920.0, 1080.0);
    assert!((hold - 2.0).abs() < 1e-9);
    assert!((center.0 - 200.0).abs() < 1e-9);
    assert!((center.1 - 100.0).abs() < 1e-9);

    let (ease_in, ease_center) = eval_zoom(&clips, 1.1, 1920.0, 1080.0);
    assert!(ease_in > 1.0 && ease_in < 2.0);
    assert!((ease_center.0 - 200.0).abs() < 1e-9);
    assert!((ease_center.1 - 100.0).abs() < 1e-9);

    let (ease_out, _) = eval_zoom(&clips, 2.7, 1920.0, 1080.0);
    assert!(ease_out > 1.0 && ease_out < 2.0);
}

#[test]
fn selected_zoom_mode_and_scale_update_clip() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 960.0, 540.0);
    assert!(state.add_zoom_at_playhead().is_some());
    assert_eq!(state.selected_zoom_clip().unwrap().mode, ZoomMode::Auto);
    assert!((state.selected_zoom_clip().unwrap().scale - DEFAULT_ZOOM_SCALE).abs() < 1e-9);

    state.set_selected_zoom_mode(ZoomMode::Manual);
    state.set_selected_zoom_scale(1.5);
    let clip = state.selected_zoom_clip().unwrap();
    assert_eq!(clip.mode, ZoomMode::Manual);
    assert!((clip.scale - 1.5).abs() < 1e-9);
    assert_eq!(format_zoom_scale(clip.scale), "1.5×");
}

#[test]
fn add_zoom_uses_manual_when_auto_zoom_is_unavailable() {
    let mut state = VideoEditState::new(metadata());
    assert!(!state.supports_auto_zoom());
    assert!(state.add_zoom_at_playhead().is_some());
    assert_eq!(state.selected_zoom_clip().unwrap().mode, ZoomMode::Manual);

    state.set_selected_zoom_mode(ZoomMode::Auto);
    assert_eq!(state.selected_zoom_clip().unwrap().mode, ZoomMode::Manual);
}

#[test]
fn add_zoom_uses_auto_when_pointer_samples_exist() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 400.0, 300.0);
    assert!(state.supports_auto_zoom());
    assert!(state.add_zoom_at_playhead().is_some());
    assert_eq!(state.selected_zoom_clip().unwrap().mode, ZoomMode::Auto);
}

#[test]
fn zoom_fill_maps_box_to_the_full_frame() {
    let target = 2.0;
    let ox = 0.5;
    let oy = 0.2;
    let (tx, ty, scale) = zoom_fill_transform(target, target, ox, oy);
    let left = tx + scale * (ox - 0.5 / target);
    let right = tx + scale * (ox + 0.5 / target);
    let top = ty + scale * (oy - 0.5 / target);
    let bottom = ty + scale * (oy + 0.5 / target);
    assert!((left - 0.0).abs() < 1e-9);
    assert!((right - 1.0).abs() < 1e-9);
    assert!((top - 0.0).abs() < 1e-9);
    assert!((bottom - 1.0).abs() < 1e-9);
    let (idle_x, idle_y, idle_s) = zoom_fill_transform(1.0, target, ox, oy);
    assert!((idle_x).abs() < 1e-9 && idle_y.abs() < 1e-9 && (idle_s - 1.0).abs() < 1e-9);
}

#[test]
fn view_to_source_and_click_sets_manual_center() {
    let view = (480.0, 270.0, 960.0, 540.0);
    let (x, y) = view_to_source(view, 480.0, 270.0, 960.0, 540.0);
    assert!((x - 960.0).abs() < 1e-9);
    assert!((y - 540.0).abs() < 1e-9);

    let mut state = VideoEditState::new(metadata());
    assert!(state.add_zoom_at_playhead().is_some());
    state.set_selected_zoom_mode(ZoomMode::Manual);
    state.set_selected_zoom_center((800.0, 400.0));
    let center = state.selected_zoom_clip().unwrap().center;
    assert!((center.0 - 800.0).abs() < 2.0);
    assert!((center.1 - 400.0).abs() < 2.0);

    state.set_selected_zoom_center((-50.0, 10_000.0));
    let clamped = state.selected_zoom_clip().unwrap().center;
    assert!(clamped.0 > 0.0);
    assert!(clamped.1 < 1080.0);
}

#[test]
fn clip_speed_and_mute_work_without_selection() {
    let mut state = VideoEditState::new(metadata());
    assert_eq!(state.selected_segment, None);
    assert_eq!(state.selected_clip_speed(), Some(1.0));
    state.set_selected_clip_speed(2.0);
    state.set_selected_clip_muted(true);
    assert_eq!(state.selected_segment, Some(0));
    assert!((state.speed_for_source(1.0) - 2.0).abs() < 1e-9);
    assert!(state.muted_for_source(1.0));
}

#[test]
fn selected_clip_speed_mute_and_delete() {
    let mut state = VideoEditState::new(metadata());
    state.selected_segment = Some(0);
    assert_eq!(state.selected_clip_speed(), Some(1.0));
    assert_eq!(state.selected_clip_muted(), Some(false));

    state.set_selected_clip_speed(2.0);
    state.set_selected_clip_muted(true);
    assert!((state.selected_clip_speed().unwrap() - 2.0).abs() < 1e-9);
    assert_eq!(state.selected_clip_muted(), Some(true));
    assert!(state.needs_reencode());
    assert!((state.speed_for_source(1.0) - 2.0).abs() < 1e-9);
    assert!(state.muted_for_source(1.0));

    state.add_cut(5.0);
    assert_eq!(state.segment_speeds, vec![2.0, 2.0]);
    assert_eq!(state.segment_muted, vec![true, true]);
    state.selected_segment = Some(1);
    state.set_selected_clip_speed(0.5);
    state.set_selected_clip_muted(false);
    assert!((state.segment_speed(0) - 2.0).abs() < 1e-9);
    assert!((state.segment_speed(1) - 0.5).abs() < 1e-9);
    assert!(state.segment_is_muted(0));
    assert!(!state.segment_is_muted(1));

    state.remove_selected_clip();
    assert_eq!(state.selected_segment, None);
    assert_eq!(state.segments_kept, vec![true, false]);
}

#[test]
fn auto_zoom_recenters_when_cursor_nears_edge() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 1800.0, 540.0);
    state.playhead_seconds = 0.5;
    let index = state.add_zoom_at_playhead().unwrap();
    state.zoom_clips[index].start = 0.0;
    state.zoom_clips[index].end = 2.0;
    state.zoom_clips[index].center = (960.0, 540.0);
    state.zoom_clips[index].scale = 2.0;
    state.zoom_clips[index].mode = ZoomMode::Auto;
    state.zoom_clips[index].ease_ms = 0;

    let (_, auto_center) = state.eval_zoom(0.5);
    assert!(
        auto_center.0 > 960.0,
        "auto zoom should follow a cursor near the right edge, got {}",
        auto_center.0
    );

    state.zoom_clips[index].mode = ZoomMode::Manual;
    let (_, manual_center) = state.eval_zoom(0.5);
    assert!((manual_center.0 - 960.0).abs() < 1e-6);

    state.zoom_clips[index].mode = ZoomMode::Auto;
    state.zoom_classic = true;
    let (_, classic_center) = state.eval_zoom(0.5);
    assert!((classic_center.0 - 960.0).abs() < 1e-6);
}

#[test]
fn snap_to_target_uses_threshold() {
    assert!((snap_to_target(2.95, 3.0, 0.1) - 3.0).abs() < 1e-9);
    assert!((snap_to_target(2.8, 3.0, 0.1) - 2.8).abs() < 1e-9);
    assert!((snap_to_target(3.08, 3.0, 0.1) - 3.0).abs() < 1e-9);
}

#[test]
fn snap_range_prefers_start_then_end() {
    assert!((snap_range_to_target(2.95, 2.0, 3.0, 0.12) - 3.0).abs() < 1e-9);
    assert!((snap_range_to_target(1.05, 2.0, 3.0, 0.12) - 1.0).abs() < 1e-9);
    assert!((snap_range_to_target(2.95, 0.1, 3.0, 0.12) - 3.0).abs() < 1e-9);
    assert!((snap_range_to_target(0.0, 2.0, 5.0, 0.12) - 0.0).abs() < 1e-9);
}

#[test]
fn zoom_and_clip_moves_snap_start_to_playhead() {
    let mut state = VideoEditState::new(metadata());
    state.playhead_seconds = 3.0;
    let index = state.add_zoom_at(0.0).unwrap();
    let duration = state.zoom_clips[index].duration();
    let start = snap_range_to_target(2.94, duration, state.playhead_seconds, 0.12);
    state.move_zoom_clip(index, start);
    assert!((state.zoom_clips[index].start - 3.0).abs() < 1e-9);
    assert!((state.zoom_clips[index].duration() - duration).abs() < 1e-9);

    let offset = snap_range_to_target(2.94, state.trim_duration(), state.playhead_seconds, 0.12);
    state.set_timeline_offset(offset);
    assert!((state.timeline_offset_seconds - 3.0).abs() < 1e-9);
}

#[test]
fn even_crop_stays_inside_frame() {
    let (x, y, w, h) = even_crop_rect(1.8, (10.0, 10.0), 1920, 1080);
    assert!(w.is_multiple_of(2) && h.is_multiple_of(2));
    assert!(x + w <= 1920);
    assert!(y + h <= 1080);
    assert!(w < 1920 && h < 1080);
}

#[test]
fn estimate_size_scales_with_trim_duration() {
    let full = VideoEditState::new(metadata());
    let mut half = full.clone();
    half.set_trim_end(5.0);

    assert!(half.estimated_size_bytes(true) < full.estimated_size_bytes(true));
    assert_eq!(
        half.estimated_size_bytes(true),
        full.metadata.file_size_bytes / 2
    );
}

#[test]
fn estimate_size_scales_with_dimensions() {
    let original = VideoEditState::new(metadata());
    let mut smaller = original.clone();
    smaller.dimension_preset = DimensionPreset::P720;

    assert!(smaller.estimated_size_bytes(false) < original.estimated_size_bytes(false));
}

#[test]
fn full_frame_crop_selection_reverts_to_original() {
    let mut state = VideoEditState::new(metadata());
    state.set_crop(10.0, 10.0, 1000.0, 500.0);
    assert!(state.crop.is_some());
    assert_eq!(state.canvas_dimensions(), (1000, 500));
    assert!(state.needs_reencode());

    // Dragging the border back over the whole frame reverts to original.
    state.set_crop(0.0, 0.0, 1920.0, 1080.0);
    assert!(state.crop.is_none());
    assert_eq!(state.canvas_dimensions(), (1920, 1080));
    assert!(!state.needs_reencode());
}

#[test]
fn picture_layout_full_frame_fills_clip() {
    assert_eq!(
        picture_layout((0.0, 0.0, 1920.0, 1080.0), 1920.0, 1080.0, 1920.0, 1080.0),
        (1920, 1080, 0, 0)
    );
}

#[test]
fn picture_layout_crop_scales_and_offsets() {
    assert_eq!(
        picture_layout((0.0, 0.0, 960.0, 1080.0), 1920.0, 1080.0, 960.0, 1080.0),
        (1920, 1080, 0, 0)
    );
    assert_eq!(
        picture_layout((960.0, 0.0, 960.0, 1080.0), 1920.0, 1080.0, 960.0, 1080.0),
        (1920, 1080, -960, 0)
    );
}

#[test]
fn crop_selection_is_clamped_and_even() {
    let mut state = VideoEditState::new(metadata());
    state.set_crop(-50.0, -20.0, 99999.0, 333.0);

    let crop = state.crop.expect("crop should clamp, not drop");
    assert_eq!(crop.x, 0);
    assert_eq!(crop.y, 0);
    assert_eq!(crop.width, 1920);
    assert_eq!(crop.height, 332);
    assert_eq!(state.crop_or_full(), (0.0, 0.0, 1920.0, 332.0));
    assert_eq!(state.effective_source_dimensions(), (1920, 332));
}
