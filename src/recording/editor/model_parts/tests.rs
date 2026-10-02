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
        frame_rate: 30.0,
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
fn inferred_pointer_enables_auto_zoom_without_forcing_cursor_composite() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 400.0, 300.0);
    state.sidecar.as_mut().unwrap().mark_inferred_from_video();

    assert!(state.supports_auto_zoom());
    assert!(!state.needs_composite());
    assert!(state.add_zoom_at_playhead().is_some());
    assert_eq!(state.selected_zoom_clip().unwrap().mode, ZoomMode::Auto);
    assert!(state.needs_composite());
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
fn export_quality_tiers_match_recording_crf() {
    assert_eq!(ExportQuality::Balanced.crf(), 23);
    assert_eq!(ExportQuality::High.crf(), 20);
    assert_eq!(ExportQuality::Ultra.crf(), 16);
    assert_eq!(ExportQuality::default(), ExportQuality::High);
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
fn a_lone_clip_cannot_be_slid_past_zero() {
    let mut state = VideoEditState::new(metadata());
    // A solo clip is the whole composition. Floating it would leave dead time
    // at the head that playback just jumps over.
    state.set_timeline_offset(5.0);
    assert_eq!(state.timeline_offset_seconds, 0.0);
    assert_eq!(state.segment_start(0), 0.0);
    state.set_segment_start(0, 3.0);
    assert_eq!(state.segment_start(0), 0.0);
    assert_eq!(state.composition_duration(), 10.0);

    // Once a cut makes an arrangement, moving a clip means something again.
    state.add_cut(4.0);
    state.set_segment_start(1, 6.0);
    assert!((state.segment_start(1) - 6.0).abs() < 1e-9);
    assert!(state.composition_duration() > state.source_duration());
}

#[test]
fn timeline_offset_shifts_clip_and_extends_composition() {
    let mut state = VideoEditState::new(metadata());
    assert_eq!(state.composition_duration(), 10.0);
    state.add_cut(4.0);
    state.playhead_seconds = 2.0;
    state.set_timeline_offset(5.0);
    assert!((state.composition_duration() - 15.0).abs() < 1e-9);
    // Moving a clip pans it on a fixed ruler. Zoom and playhead stay put.
    assert!((state.visible_span_seconds() - 10.0).abs() < 1e-9);
    assert!((state.playhead_seconds - 2.0).abs() < 1e-9);
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
    state.add_cut(4.0);
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
    state.add_cut(4.0);
    state.set_timeline_offset(240.0);
    state.follow_clip_on_timeline();
    assert!((state.timeline_offset_seconds - 240.0).abs() < 1e-9);
    assert!(state.timeline_canvas_seconds() > state.composition_duration());
    assert!(state.timeline_scroll_seconds > 0.0);
}

#[test]
fn format_timecode_pads_hours_minutes_and_millis() {
    assert_eq!(format_timecode(0.0), "00:00:00.000");
    assert_eq!(format_timecode(84.56), "00:01:24.560");
    assert_eq!(format_timecode(3661.005), "01:01:01.005");
}

#[test]
fn closest_aspect_ratio_picks_nearest() {
    assert_eq!(closest_aspect_ratio(1920, 1080), "16:9");
    assert_eq!(closest_aspect_ratio(1080, 1080), "1:1");
    assert_eq!(closest_aspect_ratio(1080, 1920), "9:16");
    assert_eq!(closest_aspect_ratio(1080, 1440), "3:4");
    assert_eq!(closest_aspect_ratio(1920, 822), "21:9");
}

#[test]
fn aspect_presets_fill_a_1080p_envelope() {
    for &(label, width, height) in &FRAME_ASPECT_RATIOS {
        assert_eq!((width % 2, height % 2), (0, 0), "{label} needs even dims");
        assert!(
            width.min(height) == 1080 || width.max(height) == 1920,
            "{label} should hold a 1080 short edge or cap its long edge at 1920"
        );
        assert_eq!(
            closest_aspect_ratio(width, height),
            label,
            "{label} should stay its own nearest preset"
        );
    }
}

#[test]
fn every_aspect_preset_keeps_the_source_inside_its_frame() {
    let mut state = VideoEditState::new(metadata());
    state.background = VideoBackground::Plain { r: 22, g: 22, b: 22 };
    let src_aspect = state.metadata.width as f64 / state.metadata.height as f64;
    for &(label, width, height) in &FRAME_ASPECT_RATIOS {
        state.apply_aspect_ratio(width, height);
        assert_eq!(
            state.output_dimensions(),
            (width, height),
            "{label} exports its frame size"
        );
        let (vw, vh) = state.video_rect_dimensions();
        assert_eq!((vw % 2, vh % 2), (0, 0), "{label} needs even video dims");
        assert!(
            vw <= width && vh <= height,
            "{label} keeps the video inside the frame"
        );
        assert!(width > vw || height > vh, "{label} leaves room for the fill");
        let fit_aspect = vw as f64 / vh as f64;
        assert!(
            (fit_aspect - src_aspect).abs() < 0.01,
            "{label} preserves the source aspect"
        );
    }
}

#[test]
fn apply_aspect_ratio_sets_custom_box() {
    let mut state = VideoEditState::new(metadata());
    state.apply_aspect_ratio(1080, 1080);
    assert_eq!(state.dimension_preset, DimensionPreset::Custom);
    assert_eq!((state.custom_width, state.custom_height), (1080, 1080));
    assert_eq!(state.canvas_dimensions(), (1080, 1080));
    // The picked Frame is the export size; the 16:9 source fits inside it.
    assert_eq!(state.video_rect_dimensions(), (1080, 608));
    assert_eq!(state.output_dimensions(), (1080, 1080));
    assert!(state.has_fixed_frame());
    assert!(state.needs_reencode());
    assert_eq!(state.canvas_label(), "1:1");
    state.reset_aspect_ratio();
    assert_eq!(state.dimension_preset, DimensionPreset::Original);
    assert_eq!(state.canvas_dimensions(), (1920, 1080));
    assert!(!state.has_fixed_frame());
    assert_eq!(state.canvas_label(), "Original");
    assert!(!state.needs_reencode());
}

#[test]
fn dimension_preset_original_uses_source_dimensions() {
    let state = VideoEditState::new(metadata());
    assert_eq!(state.video_rect_dimensions(), (1920, 1080));
    assert_eq!(state.output_dimensions(), (1920, 1080));
}

#[test]
fn dimension_preset_fits_inside_box_preserving_aspect() {
    let mut state = VideoEditState::new(metadata());
    state.dimension_preset = DimensionPreset::Custom;
    // Box is clamped to at least MIN_DIMENSION (64) on each side, then
    // the source is fitted inside without stretching.
    state.custom_width = 1919;
    state.custom_height = 57;

    let (w, h) = state.video_rect_dimensions();
    assert_eq!((w, h), (114, 64));
    // Aspect roughly matches 16:9 source.
    let aspect = w as f64 / h as f64;
    let src_aspect = 1920.0 / 1080.0;
    assert!((aspect - src_aspect).abs() < 0.05);
}

#[test]
fn dimension_preset_scales_source_up_into_the_frame() {
    // An explicit Frame is a chosen output size, so a smaller recording is
    // scaled up to fill the canvas instead of sitting small in the middle.
    let mut state = VideoEditState::new(VideoMetadata {
        path: PathBuf::from("/tmp/input.mp4"),
        duration_seconds: 5.0,
        width: 600,
        height: 744,
        file_size_bytes: 1024,
        has_audio: false,
        frame_rate: 30.0,
    });
    state.dimension_preset = DimensionPreset::P1080;
    let (w, h) = state.video_rect_dimensions();
    assert_eq!((w, h), (870, 1080));
    assert_eq!(state.output_dimensions(), (1920, 1080));
    // Aspect preserved (portrait source stays portrait inside the landscape frame).
    assert!(w < h);
    let aspect = w as f64 / h as f64;
    let src_aspect = 600.0 / 744.0;
    assert!((aspect - src_aspect).abs() < 0.01);
}

#[test]
fn frame_pick_holds_output_size_and_insets_the_video() {
    // 4:3 recording in a 16:9 frame with a fill: the frame stays the export
    // size and the fill covers the letterbox + padding around the video,
    // instead of the canvas growing past the picked ratio.
    let mut state = VideoEditState::new(VideoMetadata {
        path: PathBuf::from("/tmp/input.mp4"),
        duration_seconds: 5.0,
        width: 1280,
        height: 960,
        file_size_bytes: 1024,
        has_audio: false,
        frame_rate: 30.0,
    });
    state.apply_aspect_ratio(1920, 1080);
    state.background = VideoBackground::Plain { r: 0, g: 0, b: 0 };
    state.background_padding = 40.0;

    assert_eq!(state.output_dimensions(), (1920, 1080));
    // 40 slider units against a 1920px reference edge.
    assert!((state.background_padding_px() - 192.0).abs() < 1e-9);
    assert_eq!(state.video_rect_dimensions(), (928, 696));
    let (video_w, video_h) = state.video_rect_dimensions();
    assert!(1920 - video_w >= 384 && 1080 - video_h >= 384);
}

#[test]
fn frame_pick_without_fill_keeps_black_letterbox_size() {
    let mut state = VideoEditState::new(metadata());
    state.apply_aspect_ratio(854, 480);
    assert_eq!(state.background_padding_px(), 0.0);
    assert_eq!(state.output_dimensions(), (854, 480));
    // Same aspect as the source, so the video fills the frame exactly (the
    // fit snaps to the box instead of leaving a hairline letterbox).
    assert_eq!(state.video_rect_dimensions(), (854, 480));
}

#[test]
fn original_with_fill_grows_output_around_source() {
    // `Original` has no picked ratio to hold, so the fill grows the canvas
    // around the source (Auto sizing), keeping the old padding behaviour.
    let mut state = VideoEditState::new(metadata());
    state.background = VideoBackground::Plain { r: 0, g: 0, b: 0 };
    state.background_padding = 40.0;

    assert_eq!(state.output_dimensions(), (2304, 1464));
    assert_eq!(state.video_rect_dimensions(), (1920, 1080));
}

#[test]
fn fill_floors_padding_so_the_video_never_glues_to_the_edge() {
    let mut state = VideoEditState::new(metadata());
    state.apply_aspect_ratio(1080, 1080);
    state.background = VideoBackground::Plain { r: 0, g: 0, b: 0 };
    state.background_padding = 0.0;

    // BACKGROUND_MIN_GAP (8) against the 1080px reference edge.
    assert!((state.background_padding_px() - 8.0 * 2.7).abs() < 1e-9);
    let (w, _) = state.video_rect_dimensions();
    assert!(w < 1080);
}

#[test]
fn needs_reencode_when_dimensions_or_quality_change() {
    let mut state = VideoEditState::new(metadata());
    assert!(!state.needs_reencode());

    state.quality = ExportQuality::Balanced;
    assert!(state.needs_reencode());
    state.quality = ExportQuality::Ultra;
    assert!(state.needs_reencode());

    state.quality = ExportQuality::High;
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
        easing: ZoomEasing::Glide,
        mode: ZoomMode::Auto,
        ..Default::default()
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
        easing: ZoomEasing::Glide,
        mode: ZoomMode::Manual,
        ..Default::default()
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
fn adjacent_auto_zooms_morph_instead_of_pulsing() {
    let clips = [
        ZoomClip {
            start: 0.0,
            end: 2.0,
            scale: 1.5,
            center: (400.0, 300.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            ..Default::default()
        },
        ZoomClip {
            start: 2.0,
            end: 4.0,
            scale: 1.8,
            center: (1_500.0, 800.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            ..Default::default()
        },
    ];

    // The first zoom holds its framing instead of easing back out.
    let (held_scale, held_center) = eval_zoom(&clips, 1.9, 1920.0, 1080.0);
    assert!((held_scale - 1.5).abs() < 1e-9);
    assert!((held_center.0 - 400.0).abs() < 1e-9);
    assert!((held_center.1 - 300.0).abs() < 1e-9);

    // The second zoom morphs scale and framing from the first.
    let (morph_scale, morph_center) = eval_zoom(&clips, 2.3, 1920.0, 1080.0);
    assert!(morph_scale > 1.5 && morph_scale < 1.8);
    assert!(morph_center.0 > 400.0 && morph_center.0 < 1_500.0);
    assert!(morph_center.1 > 300.0 && morph_center.1 < 800.0);

    // With no neighbour after it, the last zoom settles back to full frame.
    let (end_scale, _) = eval_zoom(&clips, 4.0, 1920.0, 1080.0);
    assert!((end_scale - 1.0).abs() < 1e-9);
}

#[test]
fn zoom_gaps_hold_the_framing_for_the_next_auto_zoom() {
    let clips = [
        ZoomClip {
            start: 0.0,
            end: 2.0,
            scale: 1.5,
            center: (400.0, 300.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            ..Default::default()
        },
        ZoomClip {
            start: 2.3,
            end: 4.3,
            scale: 1.5,
            center: (1_500.0, 800.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            ..Default::default()
        },
    ];

    let (gap_scale, gap_center) = eval_zoom(&clips, 2.15, 1920.0, 1080.0);
    assert!((gap_scale - 1.5).abs() < 1e-9);
    assert!((gap_center.0 - 400.0).abs() < 1e-9);

    let (_, morph_center) = eval_zoom(&clips, 2.6, 1920.0, 1080.0);
    assert!(morph_center.0 > 400.0 && morph_center.0 < 1_500.0);
}

#[test]
fn a_morph_gap_keeps_the_evaluated_camera_endpoint() {
    use crate::recording::editor::sidecar::{CaptureRegion, CursorKind, PointerSample, PointerSidecar};

    let mut state = VideoEditState::new(metadata());
    // A pointer parked at the right edge pulls the first Auto zoom's camera
    // away from its stored center, so its evaluated endpoint is off-center.
    let mut sidecar = PointerSidecar::new(
        0,
        CaptureRegion {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        },
    );
    for t in [0.0, 0.5, 1.0, 1.5, 2.0, 2.15, 2.3, 2.6, 3.0] {
        sidecar.pointer.push(PointerSample {
            t,
            x: 1900.0,
            y: 300.0,
            kind: CursorKind::Default,
        });
    }
    state.sidecar = Some(sidecar);
    state.cursor.smooth = 0.0;
    for (start, end, center) in [(0.0, 2.0, (400.0, 300.0)), (2.3, 4.3, (1_500.0, 800.0))] {
        state.zoom_clips.push(ZoomClip {
            start,
            end,
            scale: 1.8,
            center,
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            ..Default::default()
        });
    }

    let (_, at_end) = state.eval_zoom(2.0);
    let (_, in_gap) = state.eval_zoom(2.15);
    assert!(
        (at_end.0 - in_gap.0).abs() < 20.0,
        "the morph gap must hold the followed endpoint, not the stored center: \
         end={at_end:?} gap={in_gap:?}"
    );
    // Sanity: the pointer did pull the camera right of the stored center.
    assert!(at_end.0 > 500.0, "expected following at the clip edge, got {at_end:?}");
}

#[test]
fn hidden_zooms_do_not_drive_the_next_transition() {
    let clips = [
        ZoomClip {
            start: 0.0,
            end: 2.0,
            scale: 2.5,
            center: (200.0, 200.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            hidden: true,
            ..Default::default()
        },
        ZoomClip {
            start: 2.3,
            end: 4.3,
            scale: 1.5,
            center: (1_500.0, 800.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            ..Default::default()
        },
    ];

    // The gap holds the full frame: the hidden clip contributes nothing.
    let (gap_scale, gap_center) = eval_zoom(&clips, 2.15, 1920.0, 1080.0);
    assert!((gap_scale - 1.0).abs() < 1e-9);
    assert!((gap_center.0 - 960.0).abs() < 1e-9);

    // The visible clip opens around its own focus, not the hidden framing.
    let (in_scale, in_center) = eval_zoom(&clips, 2.6, 1920.0, 1080.0);
    assert!(
        in_scale > 1.0 && in_scale < 1.5,
        "should ease in from the full frame, got {in_scale}"
    );
    assert!(
        (in_center.0 - 1_500.0).abs() < 1e-9,
        "should open on its own focus, got {in_center:?}"
    );
}

#[test]
fn manual_zooms_keep_independent_transitions() {
    let clips = [
        ZoomClip {
            start: 0.0,
            end: 2.0,
            scale: 2.0,
            center: (400.0, 300.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Manual,
            ..Default::default()
        },
        ZoomClip {
            start: 2.3,
            end: 4.3,
            scale: 2.0,
            center: (1_500.0, 800.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Manual,
            ..Default::default()
        },
    ];

    // Manual zooms still ease back out to the full frame...
    let (out_scale, _) = eval_zoom(&clips, 1.7, 1920.0, 1080.0);
    assert!(out_scale > 1.0 && out_scale < 2.0);
    // ...show the full frame between them...
    let (gap_scale, _) = eval_zoom(&clips, 2.15, 1920.0, 1080.0);
    assert!((gap_scale - 1.0).abs() < 1e-9);
    // ...and open around their own focus without morphing.
    let (in_scale, in_center) = eval_zoom(&clips, 2.6, 1920.0, 1080.0);
    assert!(in_scale > 1.0 && in_scale < 2.0);
    assert!((in_center.0 - 1_500.0).abs() < 1e-9);
}

#[test]
fn follow_camera_starts_at_the_clip_and_chases_the_group() {
    // A static pointer at the group centre leaves the camera where it
    // started; a distant group pulls it away, but the spring lags behind the
    // target instead of jumping to it in one step.
    let points = vec![(0.0, 1900.0, 540.0), (0.5, 1900.0, 540.0)];
    let budget = (480.0, 270.0);
    let start = (960.0, 540.0);
    let spring = CAMERA_FOLLOW_SPRING;
    let early = evaluate_spring_camera(start, &points, 0.0, 0.05, budget, spring);
    assert!(
        early.0 > start.0,
        "the camera should move toward the group, got {early:?}"
    );
    assert!(
        early.0 < 1900.0,
        "the spring should lag the target instead of snapping, got {early:?}"
    );
    let later = evaluate_spring_camera(start, &points, 0.0, 0.5, budget, spring);
    assert!(
        later.0 > early.0,
        "more time should mean more travel, got {early:?} then {later:?}"
    );
}

#[test]
fn follow_camera_keeps_its_viewport_inside_the_crop() {
    // A 2x viewport is 400x300 inside this 800x600 crop. Chasing a group in
    // the corner must not push the viewport past the crop edge, or the
    // export would show pixels the crop dropped.
    let crop = (100.0, 50.0, 800.0, 600.0);
    let points = vec![(0.0, 900.0, 650.0), (1.0, 900.0, 650.0)];
    let budget = movement_group_budget(crop, 2.0);
    let followed = evaluate_spring_camera((500.0, 350.0), &points, 0.0, 1.0, budget, CAMERA_FOLLOW_SPRING);
    let clamped = clamp_zoom_center(crop, 2.0, followed);
    let half_w = crop.2 / 2.0 / 2.0;
    let half_h = crop.3 / 2.0 / 2.0;
    assert!(clamped.0 <= crop.0 + crop.2 - half_w + 1e-9);
    assert!(clamped.0 >= crop.0 + half_w - 1e-9);
    assert!(clamped.1 <= crop.1 + crop.3 - half_h + 1e-9);
    assert!(clamped.1 >= crop.1 + half_h - 1e-9);
}

#[test]
fn movement_group_center_is_dwell_weighted() {
    // Two samples at the control, one quick pass through the corner: the
    // dwell-weighted centre stays near the control instead of the midpoint.
    let points = vec![
        (0.0, 100.0, 100.0),
        (1.0, 100.0, 100.0),
        (1.1, 900.0, 700.0),
        (1.2, 100.0, 100.0),
    ];
    let center = time_weighted_center(&points, Some(2.0)).unwrap();
    assert!(
        center.0 < 300.0 && center.1 < 300.0,
        "dwell should outweigh the fly-by, got {center:?}"
    );
}

#[test]
fn movement_groups_split_when_the_pointer_roams_far() {
    let points = vec![
        (0.0, 100.0, 100.0),
        (0.2, 120.0, 110.0),
        (0.4, 1500.0, 800.0),
        (0.6, 1520.0, 810.0),
    ];
    let groups = movement_groups(&points, (480.0, 270.0));
    assert_eq!(groups.len(), 2, "far travel should open a new group: {groups:?}");
    let first = movement_group_center_at(&points, 0.2, (480.0, 270.0)).unwrap();
    let second = movement_group_center_at(&points, 0.5, (480.0, 270.0)).unwrap();
    assert!(first.0 < 500.0, "early time follows the first group, got {first:?}");
    assert!(second.0 > 1000.0, "later time follows the second group, got {second:?}");
}

#[test]
fn spring_step_moves_toward_the_target() {
    let spring = CAMERA_FOLLOW_SPRING;
    let (value, velocity) = spring_step(0.0, 0.0, 100.0, spring, 1.0 / 120.0);
    assert!(value > 0.0 && value < 100.0, "a step should advance, got {value}");
    assert!(velocity > 0.0, "velocity should build toward the target");
    // The clamp keeps a fast approach from swinging far past the target.
    let (settled, _) = spring_step(99.0, 200.0, 100.0, spring, 1.0 / 120.0);
    assert!(
        settled <= 105.0,
        "clamped damping should not fling far past the target, got {settled}"
    );
}

#[test]
fn default_zoom_center_maps_area_pointer_into_video_pixels() {
    let mut state = VideoEditState::new(metadata());
    // Half-scale capture region: capture-local (480, 270) is video (960, 540).
    let mut sidecar = crate::recording::editor::sidecar::PointerSidecar::new(
        0,
        crate::recording::editor::sidecar::CaptureRegion {
            x: 0,
            y: 0,
            w: 960,
            h: 540,
        },
    );
    sidecar
        .pointer
        .push(crate::recording::editor::sidecar::PointerSample {
            t: 0.0,
            x: 480.0,
            y: 270.0,
            kind: crate::recording::editor::sidecar::CursorKind::Default,
        });
    state.sidecar = Some(sidecar);

    assert_eq!(state.default_zoom_center(0.0), (960.0, 540.0));
}

#[test]
fn default_zoom_center_converts_composition_time_to_source_time() {
    let mut state = VideoEditState::new(metadata());
    let mut sidecar = crate::recording::editor::sidecar::PointerSidecar::new(
        0,
        crate::recording::editor::sidecar::CaptureRegion::from_capture(None, None, None, None),
    );
    for (t, x, y) in [(0.0, 100.0, 100.0), (3.0, 400.0, 300.0)] {
        sidecar
            .pointer
            .push(crate::recording::editor::sidecar::PointerSample {
                t,
                x,
                y,
                kind: crate::recording::editor::sidecar::CursorKind::Default,
            });
    }
    state.sidecar = Some(sidecar);
    state.set_trim_start(2.0);

    // Composition 1.0 is source 3.0 once the head is trimmed, where the
    // pointer sits at (400, 300). Querying source time directly would read the
    // interpolated (200, 200) at source 1.0.
    assert_eq!(state.default_zoom_center(1.0), (400.0, 300.0));
}

#[test]
fn default_zoom_center_accounts_for_clip_speed() {
    let mut state = VideoEditState::new(metadata());
    let mut sidecar = crate::recording::editor::sidecar::PointerSidecar::new(
        0,
        crate::recording::editor::sidecar::CaptureRegion::from_capture(None, None, None, None),
    );
    for (t, x, y) in [(0.0, 100.0, 100.0), (2.0, 800.0, 600.0)] {
        sidecar
            .pointer
            .push(crate::recording::editor::sidecar::PointerSample {
                t,
                x,
                y,
                kind: crate::recording::editor::sidecar::CursorKind::Default,
            });
    }
    state.sidecar = Some(sidecar);
    state.selected_segment = Some(0);
    state.set_selected_clip_speed(2.0);

    // At 2x, composition 1.0 is source 2.0, where the pointer is (800, 600).
    assert_eq!(state.default_zoom_center(1.0), (800.0, 600.0));
}

#[test]
fn auto_zoom_follows_the_cursor_in_video_space() {
    let mut state = VideoEditState::new(metadata());
    // Half-scale capture region again: the raw sample sits at (700, 350), but
    // in the encoded video the cursor is at the zoom's (1400, 700) center.
    let mut sidecar = crate::recording::editor::sidecar::PointerSidecar::new(
        0,
        crate::recording::editor::sidecar::CaptureRegion {
            x: 0,
            y: 0,
            w: 960,
            h: 540,
        },
    );
    for (t, x, y) in [(0.0, 700.0, 350.0), (1.0, 700.0, 350.0)] {
        sidecar
            .pointer
            .push(crate::recording::editor::sidecar::PointerSample {
                t,
                x,
                y,
                kind: crate::recording::editor::sidecar::CursorKind::Default,
            });
    }
    state.sidecar = Some(sidecar);
    state.zoom_clips.push(ZoomClip {
        start: 0.0,
        end: 2.0,
        scale: 2.0,
        center: (1400.0, 700.0),
        ease_ms: 0,
        easing: ZoomEasing::Glide,
        mode: ZoomMode::Auto,
        ..Default::default()
    });

    let (_, camera_center) = state.eval_zoom(0.5);
    // The cursor maps exactly onto the view center, so the camera stays put.
    // Reading the raw capture-local point would drag it toward the top-left.
    assert!(
        (camera_center.0 - 1400.0).abs() < 1e-9 && (camera_center.1 - 700.0).abs() < 1e-9,
        "camera should sit on the video-space cursor, got {camera_center:?}"
    );
}

#[test]
fn auto_zoom_camera_lags_a_pointer_jump() {
    // The follow camera chases the movement-group centre on a spring, not
    // the raw cursor: a sudden jump splits the groups, and the spring needs
    // time to travel, so the framing lags behind the pointer.
    let mut state = VideoEditState::new(metadata());
    let mut sidecar = crate::recording::editor::sidecar::PointerSidecar::new(
        0,
        crate::recording::editor::sidecar::CaptureRegion {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        },
    );
    for (t, x) in [(0.0, 960.0), (0.4, 960.0), (0.5, 1900.0)] {
        sidecar
            .pointer
            .push(crate::recording::editor::sidecar::PointerSample {
                t,
                x,
                y: 540.0,
                kind: crate::recording::editor::sidecar::CursorKind::Default,
            });
    }
    state.sidecar = Some(sidecar);
    state.zoom_clips.push(ZoomClip {
        start: 0.0,
        end: 2.0,
        scale: 2.0,
        center: (960.0, 540.0),
        ease_ms: 0,
        easing: ZoomEasing::Glide,
        mode: ZoomMode::Auto,
        ..Default::default()
    });

    let (scale, camera_center) = state.eval_zoom(0.5);
    assert!((scale - 2.0).abs() < 1e-9);
    assert!(
        camera_center.0 < 1000.0,
        "camera must not race ahead on the jumped pointer path: {camera_center:?}"
    );
}

#[test]
fn instant_zoom_snaps_to_the_group_centre() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 1500.0, 800.0);
    state.zoom_clips.push(ZoomClip {
        start: 0.0,
        end: 2.0,
        scale: 2.0,
        center: (400.0, 300.0),
        ease_ms: 600,
        easing: ZoomEasing::Smooth,
        mode: ZoomMode::Auto,
        instant: true,
        ..Default::default()
    });

    // Scale snaps (no ease ramp) and the camera jumps to the group centre
    // instead of chasing it.
    let (scale, center) = state.eval_zoom_at(0.1, 0.1);
    assert!((scale - 2.0).abs() < 1e-9);
    let crop = state.crop_or_full();
    let expected = clamp_zoom_center(crop, 2.0, (1500.0, 800.0));
    assert!(
        (center.0 - expected.0).abs() < 1e-9 && (center.1 - expected.1).abs() < 1e-9,
        "instant should snap to the group centre, got {center:?}"
    );
}

#[test]
fn instant_zooms_do_not_morph_across_a_gap() {
    let clips = [
        ZoomClip {
            start: 0.0,
            end: 2.0,
            scale: 2.0,
            center: (400.0, 300.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            instant: true,
            ..Default::default()
        },
        ZoomClip {
            start: 2.3,
            end: 4.3,
            scale: 2.0,
            center: (1_500.0, 800.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            ..Default::default()
        },
    ];
    // An instant predecessor holds no framing across the gap: the gap shows
    // the full frame instead of morphing.
    let (gap_scale, gap_center) = eval_zoom(&clips, 2.15, 1920.0, 1080.0);
    assert!((gap_scale - 1.0).abs() < 1e-9);
    assert!((gap_center.0 - 960.0).abs() < 1e-9);
}

#[test]
fn follow_camera_matches_across_random_and_sequential_seeks() {
    // The evaluator carries no state: evaluating in playback order and in a
    // shuffled order must agree exactly.
    let mut state = VideoEditState::new(metadata());
    let mut sidecar = crate::recording::editor::sidecar::PointerSidecar::new(
        0,
        crate::recording::editor::sidecar::CaptureRegion {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        },
    );
    for (t, x) in [(0.0, 400.0), (0.5, 800.0), (1.0, 1200.0), (1.5, 1600.0)] {
        sidecar
            .pointer
            .push(crate::recording::editor::sidecar::PointerSample {
                t,
                x,
                y: 540.0,
                kind: crate::recording::editor::sidecar::CursorKind::Default,
            });
    }
    state.sidecar = Some(sidecar);
    state.zoom_clips.push(ZoomClip {
        start: 0.0,
        end: 2.0,
        scale: 2.0,
        center: (400.0, 300.0),
        ease_ms: 0,
        easing: ZoomEasing::Glide,
        mode: ZoomMode::Auto,
        ..Default::default()
    });

    let times = [0.0, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0];
    let sequential: Vec<(f64, (f64, f64))> =
        times.iter().map(|t| state.eval_zoom_at(*t, *t)).collect();
    let mut shuffled = times;
    shuffled.reverse();
    for t in shuffled {
        let evaluated = state.eval_zoom_at(t, t);
        let expected = sequential[times.iter().position(|x| *x == t).unwrap()];
        assert!(
            (evaluated.0 - expected.0).abs() < 1e-12
                && (evaluated.1.0 - expected.1.0).abs() < 1e-12
                && (evaluated.1.1 - expected.1.1).abs() < 1e-12,
            "seek to {t} should match sequential evaluation"
        );
    }
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
fn default_cursor_tool_uses_classic_theme() {
    let mut state = VideoEditState::new(metadata());
    assert_eq!(state.selected_tool, EditorTool::Cursor);
    assert_eq!(state.cursor.theme, CursorTheme::Adwaita);
    assert!((state.cursor.size - 1.0).abs() < 1e-9);
    state.cursor.theme = CursorTheme::Black;
    state.cursor.size = 1.6;
    state.cursor.shadow = 0.8;
    state.selected_tool = EditorTool::Timeline;
    assert_eq!(state.cursor.theme.label(), "Black");
    assert_eq!(state.cursor.theme.as_str(), "black");
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
fn clip_speed_controls_timeline_mapping_duration_and_reencode() {
    let mut state = VideoEditState::new(metadata());
    state.selected_segment = Some(0);
    state.set_selected_clip_speed(2.0);

    assert!((state.source_to_timeline(8.0) - 4.0).abs() < 1e-9);
    assert!((state.timeline_to_source(4.0) - 8.0).abs() < 1e-9);
    assert!((state.composition_duration() - 5.0).abs() < 1e-9);
    assert!(state.needs_reencode());
}

#[test]
fn clip_settings_at_a_cut_belong_to_the_clip_starting_there() {
    let mut state = VideoEditState::new(metadata());
    state.add_cut(4.0);
    state.selected_segment = Some(0);
    state.set_selected_clip_speed(0.5);
    state.selected_segment = Some(1);
    state.set_selected_clip_speed(2.0);
    state.set_selected_clip_muted(true);

    assert!((state.speed_for_source(4.0) - 2.0).abs() < 1e-9);
    assert!(state.muted_for_source(4.0));
    assert!((state.source_to_timeline(4.0) - state.segment_start(1)).abs() < 1e-9);
    assert!(state.video_has_edits());
}

#[test]
fn clip_mute_requires_reencode_and_deleted_segments_do_not_map() {
    let mut state = VideoEditState::new(metadata());
    state.set_selected_clip_muted(true);
    assert!(state.needs_reencode());

    state.add_cut(4.0);
    state.set_segment_start(1, 8.0);
    state.selected_segment = Some(1);
    state.remove_selected_clip();
    assert!((state.timeline_to_source(9.0) - 9.0).abs() < 1e-9);
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
fn switching_auto_zoom_to_manual_preserves_the_visible_center() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 1800.0, 540.0);
    state.playhead_seconds = 0.5;
    let index = state.add_zoom_at_playhead().unwrap();
    state.zoom_clips[index].start = 0.0;
    state.zoom_clips[index].end = 2.0;
    state.zoom_clips[index].center = (960.0, 540.0);
    state.zoom_clips[index].scale = 2.0;
    state.zoom_clips[index].ease_ms = 0;
    let (_, visible_center) = state.eval_zoom(state.source_playhead());

    state.set_selected_zoom_mode(ZoomMode::Manual);

    assert_eq!(state.zoom_clips[index].mode, ZoomMode::Manual);
    assert!((state.zoom_clips[index].center.0 - visible_center.0).abs() < 1e-6);
    assert!((state.zoom_clips[index].center.1 - visible_center.1).abs() < 1e-6);
}

fn attach_sidecar_with_clicks(state: &mut VideoEditState, clicks: &[(f64, f64, f64)]) {
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
            x: 960.0,
            y: 540.0,
            kind: crate::recording::editor::sidecar::CursorKind::Default,
        });
    for &(t, x, y) in clicks {
        sidecar
            .clicks
            .push(crate::recording::editor::sidecar::ClickSample { t, x, y, button: 1 });
    }
    state.sidecar = Some(sidecar);
}



#[test]
fn suggest_zoom_clips_merges_nearby_clicks_into_one_shot() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 400.0, 300.0), (4.3, 1500.0, 700.0)]);

    // The two clicks are close enough that one shot holds both.
    assert_eq!(state.suggest_zoom_clips(), 1);
    assert!((state.zoom_clips[0].start - 2.7).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 6.8).abs() < 1e-9);
}

#[test]
fn suggest_zoom_clips_clips_a_window_at_the_trim_boundary() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(5.9, 800.0, 500.0)]);
    state.set_trim_end(6.0);

    assert_eq!(state.suggest_zoom_clips(), 1);
    assert!((state.zoom_clips[0].start - 5.6).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 6.0).abs() < 1e-9);
}






#[test]
fn a_generated_zoom_follows_its_footage_through_a_head_trim() {
    // The pass runs, then the head is trimmed. The click's footage moves
    // earlier, so a clip that only remembered its composition time would be
    // left framing whatever now sits at that spot.
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(5.9, 800.0, 500.0)]);
    state.set_trim_end(6.0);
    assert_eq!(state.suggest_zoom_clips(), 1);
    assert!((state.zoom_clips[0].start - 5.6).abs() < 1e-9);

    state.set_trim_start(2.0);

    assert!((state.zoom_clips[0].start - 3.6).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 4.0).abs() < 1e-9);
}

#[test]
fn a_generated_zoom_keeps_its_footage_when_the_clip_is_retimed() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(5.9, 800.0, 500.0)]);
    state.set_trim_end(6.0);
    state.suggest_zoom_clips();

    state.set_selected_clip_speed(2.0);
    assert!((state.zoom_clips[0].start - 2.8).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 3.0).abs() < 1e-9);

    // And stretched to a quarter speed.
    state.set_selected_clip_speed(0.25);
    assert!((state.zoom_clips[0].start - 22.4).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 24.0).abs() < 1e-9);
}

#[test]
fn a_cut_inside_a_generated_zoom_leaves_it_with_the_piece_that_owns_its_start() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(5.9, 800.0, 500.0)]);
    state.set_trim_end(6.0);
    state.suggest_zoom_clips();
    assert!((state.zoom_clips[0].start - 5.6).abs() < 1e-9);

    state.add_cut(5.0);

    // The click sits after the cut, so the zoom stays with the tail.
    assert_eq!(state.zoom_clips.len(), 1);
    assert!((state.zoom_clips[0].start - 5.6).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 6.0).abs() < 1e-9);
}

#[test]
fn a_generated_zoom_goes_when_its_footage_leaves_the_composition() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(2.0, 800.0, 500.0), (8.0, 800.0, 500.0)]);
    assert_eq!(state.suggest_zoom_clips(), 2);
    state.add_cut(5.0);

    state.selected_segment = Some(0);
    state.remove_selected_clip();

    // The clip over the dropped segment has nothing left to frame; the one
    // over the kept tail stays on its click.
    assert_eq!(state.zoom_clips.len(), 1);
    assert!((state.zoom_clips[0].start - 7.7).abs() < 1e-9);
}

#[test]
fn a_zoom_the_user_moves_keeps_the_time_they_gave_it() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(5.9, 800.0, 500.0)]);
    state.set_trim_end(6.0);
    state.suggest_zoom_clips();

    state.set_zoom_range(0, 3.0, 4.0);
    state.set_trim_start(2.0);

    assert!(state.zoom_clips[0].anchor.is_none());
    assert!((state.zoom_clips[0].start - 3.0).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 4.0).abs() < 1e-9);
}

#[test]
fn a_duplicated_generated_zoom_is_not_anchored_to_the_original() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(5.9, 800.0, 500.0)]);
    state.set_trim_end(6.0);
    state.suggest_zoom_clips();

    assert_eq!(state.duplicate_zoom_clip(0, 0.0), Some(0));

    state.set_trim_start(2.0);

    // The copy sits where it was dropped; only the clip the generator placed
    // follows the footage.
    assert!((state.zoom_clips[0].start - 0.0).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 0.4).abs() < 1e-9);
    assert!((state.zoom_clips[1].start - 3.6).abs() < 1e-9);
}

#[test]
fn undoing_a_zoom_edit_keeps_the_clip_on_its_footage() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(5.9, 800.0, 500.0)]);
    state.set_trim_end(6.0);
    state.suggest_zoom_clips();
    state.move_zoom_clip(0, 1.0);
    // A composition edit after the step was taken moves the anchored clip.
    state.set_trim_start(2.0);

    assert!(state.undo_zoom_edit());

    // Undo restores the clip the drag took over, and it lands on the footage
    // its anchor names rather than the composition slot it was captured at.
    assert!((state.zoom_clips[0].start - 3.6).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 4.0).abs() < 1e-9);
}

#[test]
fn suggest_zoom_clips_assigns_exact_cut_click_to_following_segment() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(4.0, 800.0, 500.0)]);
    state.add_cut(4.0);

    assert_eq!(state.suggest_zoom_clips(), 1);
    assert!((state.zoom_clips[0].start - 4.0).abs() < 1e-9);
}

#[test]
fn suggest_zoom_clips_uses_cut_boundary_tolerance() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(4.0 - 5e-10, 800.0, 500.0)]);
    state.add_cut(4.0);

    assert_eq!(state.suggest_zoom_clips(), 1);
    assert!((state.zoom_clips[0].start - 4.0).abs() < 1e-9);
}



#[test]
fn suggest_zoom_clips_skips_clicks_outside_kept_segments() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 960.0, 540.0), (8.5, 960.0, 540.0)]);
    state.set_trim_start(4.0);
    // The head-trimmed click is gone; the one in the kept tail is reframed.
    assert_eq!(state.suggest_zoom_clips(), 1);
    assert!((state.zoom_clips[0].start - 4.2).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 5.2).abs() < 1e-9);
}





#[test]
fn suggest_zoom_clips_rejects_targets_outside_the_editor_crop() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 1600.0, 700.0)]);
    state.set_crop(0.0, 0.0, 900.0, 700.0);
    assert_eq!(state.suggest_zoom_clips(), 0);
    assert!(state.zoom_clips.is_empty());
}

#[test]
fn suggest_zoom_clips_ignores_stop_bar_clicks() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(9.4, 941.0, -215.0)]);
    assert_eq!(state.suggest_zoom_clips(), 0);
}

#[test]
fn click_only_sidecar_supports_auto_zoom_suggestions() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 800.0, 500.0)]);
    state.sidecar.as_mut().unwrap().pointer.clear();

    assert!(state.supports_auto_zoom());
    assert_eq!(state.suggest_zoom_clips(), 1);
    assert_eq!(state.zoom_clips[0].mode, ZoomMode::Auto);
    assert!((state.zoom_clips[0].center.0 - 800.0).abs() < 1e-9);
    assert!((state.zoom_clips[0].center.1 - 500.0).abs() < 1e-9);
}

#[test]
fn click_redetection_preserves_manual_zoom_clips() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 960.0, 540.0);
    let manual = state.add_zoom_at(0.25).unwrap();
    state.set_selected_zoom_mode(ZoomMode::Manual);
    let manual_clip = state.zoom_clips[manual].clone();
    attach_sidecar_with_clicks(&mut state, &[(5.0, 1200.0, 600.0)]);

    assert!(state.redetect_zoom_clips());
    assert_eq!(state.zoom_clips.len(), 2);
    assert!(state.zoom_clips.contains(&manual_clip));
    assert!(state
        .zoom_clips
        .iter()
        .any(|clip| clip.mode == ZoomMode::Auto));
}


#[test]
fn redetect_zoom_clips_replaces_generated_auto_zooms_for_this_video() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(8.0, 800.0, 500.0)]);
    assert_eq!(state.suggest_zoom_clips(), 1);
    assert_eq!(state.zoom_clips[0].mode, ZoomMode::Auto);
    assert_eq!(state.zoom_clips[0].origin, ZoomOrigin::Generated);
    attach_sidecar_with_clicks(&mut state, &[(4.0, 500.0, 400.0)]);
    assert!(state.redetect_zoom_clips());
    assert_eq!(state.zoom_clips.len(), 1);
    assert_eq!(state.zoom_clips[0].origin, ZoomOrigin::Generated);
    assert!((state.zoom_clips[0].start - 3.7).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 6.5).abs() < 1e-9);
}

#[test]
fn redetect_keeps_a_user_added_auto_zoom() {
    // add_zoom_at creates an Auto clip, but the user placed it, so a re-run
    // must not treat it as disposable.
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 960.0, 540.0);
    let index = state.add_zoom_at(0.0).unwrap();
    assert_eq!(state.zoom_clips[index].mode, ZoomMode::Auto);
    assert_eq!(state.zoom_clips[index].origin, ZoomOrigin::User);
    let user_clip = state.zoom_clips[index].clone();

    attach_sidecar_with_clicks(&mut state, &[(4.0, 500.0, 400.0)]);
    assert!(state.redetect_zoom_clips());
    assert!(state.zoom_clips.contains(&user_clip));
    assert!(state
        .zoom_clips
        .iter()
        .any(|clip| clip.origin == ZoomOrigin::Generated));
}

#[test]
fn editing_a_generated_zoom_protects_it_from_regeneration() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(4.0, 500.0, 400.0)]);
    assert_eq!(state.suggest_zoom_clips(), 1);
    assert_eq!(state.zoom_clips[0].origin, ZoomOrigin::Generated);
    state.selected_zoom = Some(0);
    state.set_selected_zoom_scale(2.0);
    assert_eq!(state.zoom_clips[0].origin, ZoomOrigin::User);
    let edited = state.zoom_clips[0].clone();

    attach_sidecar_with_clicks(&mut state, &[(8.0, 1200.0, 600.0)]);
    assert!(state.redetect_zoom_clips());
    assert!(state.zoom_clips.contains(&edited));
}

#[test]
fn hiding_a_generated_zoom_protects_it_from_regeneration() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(4.0, 500.0, 400.0)]);
    assert_eq!(state.suggest_zoom_clips(), 1);
    state.set_zoom_hidden(0, true);
    assert_eq!(state.zoom_clips[0].origin, ZoomOrigin::User);

    attach_sidecar_with_clicks(&mut state, &[(8.0, 1200.0, 600.0)]);
    assert!(state.redetect_zoom_clips());
    assert!(state.zoom_clips.iter().any(|clip| clip.hidden));
}

#[test]
fn legacy_clips_are_never_replaced_by_regeneration() {
    // A clip loaded from an older project has no origin; it defaults to the
    // protected variant so an unknown clip is not silently thrown away.
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(ZoomClip {
        start: 0.0,
        end: 2.0,
        scale: 2.0,
        center: (960.0, 540.0),
        mode: ZoomMode::Auto,
        ..Default::default()
    });
    assert_eq!(state.zoom_clips[0].origin, ZoomOrigin::Legacy);

    attach_sidecar_with_clicks(&mut state, &[(4.0, 500.0, 400.0)]);
    assert!(state.redetect_zoom_clips());
    assert!(state
        .zoom_clips
        .iter()
        .any(|clip| clip.origin == ZoomOrigin::Legacy));
}

#[test]
fn undo_takes_a_generation_pass_back_in_one_step() {
    // One Detect can add several clips; undoing has to put the track back the
    // way a single click found it, not clip by clip.
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 800.0, 500.0), (8.8, 1200.0, 600.0)]);

    assert!(state.redetect_zoom_clips());
    let added = state.zoom_clips.clone();
    assert_eq!(added.len(), 2);

    assert!(state.undo_zoom_edit());
    assert!(state.zoom_clips.is_empty());

    assert!(state.redo_zoom_edit());
    assert_eq!(state.zoom_clips, added);
}

#[test]
fn a_slider_sweep_is_one_undo_step() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 960.0, 540.0);
    let index = state.add_zoom_at(1.0).unwrap();
    state.selected_zoom = Some(index);
    let original = state.zoom_clips[index].scale;

    // A drag arrives as a stream of values; they must not each become a step.
    for step in 1..=5 {
        state.set_selected_zoom_scale(original + step as f64 * 0.1);
    }
    assert!((state.zoom_clips[index].scale - (original + 0.5)).abs() < 1e-9);

    assert!(state.undo_zoom_edit());
    assert!((state.zoom_clips[index].scale - original).abs() < 1e-9);

    // The add itself is the step below the sweep, and nothing is left after it.
    assert!(state.undo_zoom_edit());
    assert!(state.zoom_clips.is_empty());
    assert!(!state.undo_zoom_edit());
}

#[test]
fn a_timeline_drag_is_one_undo_step() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 960.0, 540.0);
    let index = state.add_zoom_at(1.0).unwrap();
    let placed = state.zoom_clips[index].clone();

    // A drag arrives as a stream of positions; one drag is one step, not one
    // per pointer event.
    for step in 1..=5 {
        state.move_zoom_clip(index, 1.0 + step as f64 * 0.2);
    }
    assert!((state.zoom_clips[index].start - 2.0).abs() < 1e-9);

    assert!(state.undo_zoom_edit());
    assert_eq!(state.zoom_clips[index], placed);
}

#[test]
fn a_command_after_a_sweep_keeps_its_own_step() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(4.0, 500.0, 400.0)]);
    assert_eq!(state.suggest_zoom_clips(), 1);
    state.selected_zoom = Some(0);
    let suggested = state.zoom_clips[0].scale;
    state.set_selected_zoom_scale(suggested + 0.5);
    state.set_zoom_hidden(0, true);

    assert!(state.undo_zoom_edit());
    assert!(!state.zoom_clips[0].hidden);
    assert!((state.zoom_clips[0].scale - (suggested + 0.5)).abs() < 1e-9);

    // Undoing the sweep also puts the clip back in the generator's hands: the
    // edit it made had promoted it to the user's work.
    assert!(state.undo_zoom_edit());
    assert!((state.zoom_clips[0].scale - suggested).abs() < 1e-9);
    assert_eq!(state.zoom_clips[0].origin, ZoomOrigin::Generated);
}

#[test]
fn undoing_a_cut_drops_the_pending_paste() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 960.0, 540.0);
    state.add_zoom_at(1.0).unwrap();

    assert!(state.cut_zoom_clip(0));
    assert!(state.zoom_clips.is_empty());
    assert!(state.is_pasting_clip());

    assert!(state.undo_zoom_edit());
    assert_eq!(state.zoom_clips.len(), 1);
    // A clip back on the track with a paste still pending would leave the
    // editor dimmed for an action the user just took back.
    assert!(!state.is_pasting_clip());
}

#[test]
fn undo_skips_a_command_that_changed_nothing() {
    let mut state = VideoEditState::new(metadata());
    // No recorded pointer data, so the pass can place nothing.
    assert!(!state.redetect_zoom_clips());
    assert!(!state.undo_zoom_edit());
}

#[test]
fn zoom_history_keeps_a_bounded_number_of_steps() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(4.0, 500.0, 400.0)]);
    assert_eq!(state.suggest_zoom_clips(), 1);
    for step in 0..ZOOM_HISTORY_LIMIT + 8 {
        state.set_zoom_hidden(0, step % 2 == 0);
    }

    let mut undone = 0;
    while state.undo_zoom_edit() {
        undone += 1;
    }
    assert_eq!(undone, ZOOM_HISTORY_LIMIT);
}

#[test]
fn undo_takes_back_the_pass_that_ran_on_open() {
    // Taking the automatic pass back is a rejection that has to stick: the
    // reviewed flag is not part of the step, so a reopened project asks again
    // — and the pass must not return.
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(4.0, 500.0, 400.0)]);
    assert!(state.suggest_zooms_on_open());
    assert_eq!(state.zoom_clips.len(), 1);

    assert!(state.undo_zoom_edit());
    assert!(state.zoom_clips.is_empty());
    let saved = state.to_project();

    let mut reloaded = VideoEditState::new(metadata());
    reloaded.apply_project(saved);
    assert!(reloaded.zoom_clips.is_empty());
    assert!(!reloaded.suggest_zooms_on_open());
}

#[test]
fn a_reloaded_project_has_nothing_to_undo() {    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(4.0, 500.0, 400.0)]);
    assert_eq!(state.suggest_zoom_clips(), 1);
    let saved = state.to_project();

    let mut reloaded = VideoEditState::new(metadata());
    reloaded.apply_project(saved);
    assert_eq!(reloaded.zoom_clips.len(), 1);
    // The steps are runtime-only: a reopened recording starts clean.
    assert!(!reloaded.undo_zoom_edit());
}

#[test]
fn suggest_zoom_clips_respects_zoom_lock() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 960.0, 540.0)]);
    state.zoom_locked = true;
    assert_eq!(state.suggest_zoom_clips(), 0);
    assert!(state.zoom_clips.is_empty());
}

#[test]
fn opening_a_fresh_recording_suggests_once() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 960.0, 540.0)]);
    assert!(!state.zoom_suggestions_reviewed());
    assert!(state.suggest_zooms_on_open());
    assert_eq!(state.zoom_clips.len(), 1);
    assert!(state.zoom_suggestions_reviewed());
    // A second open must not stack another suggestion on top.
    state.zoom_clips.clear();
    assert!(!state.suggest_zooms_on_open());
    assert!(state.zoom_clips.is_empty());
}

#[test]
fn the_review_pass_runs_quietly_when_it_finds_nothing() {
    // No sidecar means no suggestions, but the pass still counts as reviewed:
    // the recording has been looked at.
    let mut state = VideoEditState::new(metadata());
    assert!(!state.suggest_zooms_on_open());
    assert!(state.zoom_suggestions_reviewed());
    assert!(state.zoom_clips.is_empty());
}

#[test]
fn rejecting_every_suggestion_survives_reopening() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 960.0, 540.0)]);
    assert!(state.suggest_zooms_on_open());
    assert_eq!(state.zoom_clips.len(), 1);

    // The user deletes the suggestion to reject it. The zoom list is now
    // empty, but the review state is separate, so opening again stays quiet.
    state.zoom_clips.clear();
    assert!(!state.suggest_zooms_on_open());
    assert!(state.zoom_clips.is_empty());
}

#[test]
fn explicit_redetect_marks_the_recording_reviewed() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 960.0, 540.0)]);
    assert!(state.redetect_zoom_clips());
    assert!(state.zoom_suggestions_reviewed());
}

#[test]
fn resetting_the_review_state_allows_the_pass_again() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 960.0, 540.0)]);
    assert!(state.suggest_zooms_on_open());
    state.zoom_clips.clear();
    state.reset_zoom_suggestions_reviewed();
    assert!(!state.zoom_suggestions_reviewed());
    assert!(state.suggest_zooms_on_open());
    assert_eq!(state.zoom_clips.len(), 1);
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

    // A lone clip cannot slide, so the snap is exercised on a cut arrangement.
    state.add_cut(4.0);
    let offset = snap_range_to_target(2.94, state.trim_duration(), state.playhead_seconds, 0.12);
    state.set_timeline_offset(offset);
    assert!((state.timeline_offset_seconds - 3.0).abs() < 1e-9);
}

#[test]
fn cursor_hide_clips_reject_overlap_and_zero_alpha_inside() {
    let mut state = VideoEditState::new(metadata());
    let index = state.add_cursor_hide_at_playhead().unwrap();
    assert_eq!(index, 0);
    assert_eq!(state.cursor_hide_clips.len(), 1);
    assert!(
        (state.cursor_hide_clips[0].duration() - DEFAULT_CURSOR_HIDE_DURATION_SECONDS).abs() < 1e-9
    );
    assert!(state.add_cursor_hide_at(0.2).is_none());
    assert_eq!(state.cursor_hide_clips.len(), 1);
    assert!((state.cursor_hide_alpha(0.5) - 0.0).abs() < 1e-12);
    assert!((state.cursor_hide_alpha(4.0) - 1.0).abs() < 1e-12);
    assert!((state.cursor_hide_alpha_for_source(0.5) - 0.0).abs() < 1e-12);

    state.playhead_seconds = 5.0;
    let later = state.add_cursor_hide_at_playhead().unwrap();
    assert_eq!(later, 1);
    let duration = state.cursor_hide_clips[later].duration();
    let start = snap_range_to_target(2.94, duration, 3.0, 0.12);
    state.move_cursor_hide_clip(later, start);
    assert!((state.cursor_hide_clips[later].start - 3.0).abs() < 1e-9);

    state.selected_zoom = Some(0);
    state.selected_cursor_hide = Some(0);
    assert!(state.selected_cursor_hide_clip().is_some());
}

#[test]
fn effect_clip_spans_are_clamped_to_the_video() {
    // The program is only as long as its media, so a zoom or hide block may
    // never run into the empty canvas past the last frame.
    let state = VideoEditState::new(metadata());
    assert!((super::effect_clip_limit(&state) - 10.0).abs() < 1e-9);
    assert_eq!(super::fit_effect_span(&state, 9.5, 12.0), Some((9.5, 10.0)));
    assert_eq!(super::fit_effect_span(&state, -3.0, 1.0), Some((0.0, 1.0)));
    assert_eq!(
        super::fit_effect_span(&state, 9.95, 12.0),
        None,
        "less than the 0.2s minimum cannot be placed"
    );
    assert_eq!(super::fit_effect_move(&state, 9.9, 1.0), (9.0, 10.0));
    assert_eq!(super::fit_effect_move(&state, -5.0, 1.0), (0.0, 1.0));
}

#[test]
fn a_zoom_cannot_be_placed_past_the_video_end() {
    let mut state = VideoEditState::new(metadata());
    // The default span is trimmed to the boundary when it would overshoot...
    let index = state.add_zoom_at(9.5).expect("a partial zoom still fits");
    assert!((state.zoom_clips[index].end - 10.0).abs() < 1e-9);
    assert!((state.zoom_clips[index].duration() - 0.5).abs() < 1e-9);

    // ...and placement is refused when too little of it would fit.
    assert!(state.add_zoom_at(9.95).is_none(), "0.05s is below the minimum");
    assert!(state.add_zoom_at(10.5).is_none(), "nothing fits past the end");
    assert!(state.add_zoom_at(30.0).is_none(), "nothing fits past the end");
}

#[test]
fn a_cursor_hide_clip_cannot_be_placed_past_the_video_end() {
    let mut state = VideoEditState::new(metadata());
    let index = state.add_cursor_hide_at(9.5).expect("a partial hide fits");
    assert!((state.cursor_hide_clips[index].end - 10.0).abs() < 1e-9);
    assert!(state.add_cursor_hide_at(9.95).is_none());
    assert!(state.add_cursor_hide_at(12.0).is_none());
}

#[test]
fn moving_an_effect_clip_keeps_it_inside_the_video() {
    let mut state = VideoEditState::new(metadata());
    let zoom = state.add_zoom_at(0.0).unwrap();
    let duration = state.zoom_clips[zoom].duration();
    state.move_zoom_clip(zoom, 9.0);
    assert!(
        (state.zoom_clips[zoom].end - 10.0).abs() < 1e-9,
        "the end stops at the last frame"
    );
    assert!(
        (state.zoom_clips[zoom].duration() - duration).abs() < 1e-9,
        "moving must not resize the clip"
    );

    let hide = state.add_cursor_hide_at(0.5).unwrap();
    let hide_duration = state.cursor_hide_clips[hide].duration();
    state.move_cursor_hide_clip(hide, 20.0);
    assert!((state.cursor_hide_clips[hide].end - 10.0).abs() < 1e-9);
    assert!((state.cursor_hide_clips[hide].duration() - hide_duration).abs() < 1e-9);
}

#[test]
fn resizing_an_effect_clip_stops_at_the_video_end() {
    let mut state = VideoEditState::new(metadata());
    let zoom = state.add_zoom_at(0.0).unwrap();
    state.set_zoom_range(zoom, 0.0, 25.0);
    assert!((state.zoom_clips[zoom].start - 0.0).abs() < 1e-9);
    assert!((state.zoom_clips[zoom].end - 10.0).abs() < 1e-9);

    // Dragging the whole span past the end leaves it where it was rather than
    // writing a degenerate zero-length clip.
    state.set_zoom_range(zoom, 25.0, 30.0);
    assert!((state.zoom_clips[zoom].start - 0.0).abs() < 1e-9);
    assert!((state.zoom_clips[zoom].end - 10.0).abs() < 1e-9);
}

#[test]
fn the_paste_ghost_and_paste_agree_past_the_video_end() {
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(0.0, 1.0));
    state.copy_zoom_clip(0);

    // A one-second clip at 9.9 leaves less than the minimum before the end.
    state.playhead_seconds = 9.9;
    assert!(
        !state.paste_spot_is_free(9.9, 1.0, true),
        "the ghost must read as unplaceable"
    );
    assert!(!state.can_paste_clipboard_at_playhead());
    assert!(state.paste_clipboard_at(9.9).is_none());

    // Further back it lands, trimmed to the boundary.
    state.playhead_seconds = 9.5;
    assert!(state.can_paste_clipboard_at_playhead());
    let placed = state.paste_clipboard_at(9.5).expect("the paste fits");
    assert!((state.zoom_clips[placed].start - 9.5).abs() < 1e-9);
    assert!((state.zoom_clips[placed].end - 10.0).abs() < 1e-9);
}

#[test]
fn duplicating_a_clip_at_the_end_has_nowhere_to_go() {
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(9.0, 10.0));
    state.selected_zoom = Some(0);
    assert!(
        state.duplicate_selected_clip().is_none(),
        "the copy would have to overlap the original to stay inside the video"
    );
    assert_eq!(state.zoom_clips.len(), 1);
}

#[test]
fn adding_cursor_hide_clears_zoom_selection() {
    let mut state = VideoEditState::new(metadata());
    assert!(state.add_zoom_at_playhead().is_some());
    assert!(state.selected_zoom.is_some());
    state.playhead_seconds = 4.0;
    assert!(state.add_cursor_hide_at_playhead().is_some());
    assert!(state.selected_zoom.is_none());
    assert!(state.selected_cursor_hide.is_some());
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
fn estimate_size_follows_quality_tier() {
    let estimate = |tier| {
        let mut state = VideoEditState::new(metadata());
        state.quality = tier;
        state.estimated_size_bytes(false)
    };

    assert!(estimate(ExportQuality::Balanced) < estimate(ExportQuality::High));
    assert!(estimate(ExportQuality::High) < estimate(ExportQuality::Ultra));

    // The untouched-export estimate ignores quality: a stream copy is the
    // source bytes whatever tier is picked.
    let trim_estimate = |tier| {
        let mut state = VideoEditState::new(metadata());
        state.quality = tier;
        state.estimated_size_bytes(true)
    };
    assert_eq!(trim_estimate(ExportQuality::Balanced), trim_estimate(ExportQuality::High));
    assert_eq!(trim_estimate(ExportQuality::High), trim_estimate(ExportQuality::Ultra));
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
fn zoom_camera_keeps_right_focus_at_stage_center() {
    let (tx, ty, sx, sy) =
        zoom_camera_transform((0.0, 0.0, 1920.0, 1080.0), 1920.0, 1080.0, 1920.0, 1080.0);
    assert!((tx).abs() < 1e-9 && ty.abs() < 1e-9);
    assert!((sx - 1.0).abs() < 1e-9 && (sy - 1.0).abs() < 1e-9);

    let (tx, ty, sx, sy) =
        zoom_camera_transform((960.0, 270.0, 960.0, 540.0), 1920.0, 1080.0, 1920.0, 1080.0);
    assert!((sx - 2.0).abs() < 1e-9 && (sy - 2.0).abs() < 1e-9);
    let x = (1440.0 / 1920.0 * 1920.0) * sx + tx;
    let y = (540.0 / 1080.0 * 1080.0) * sy + ty;
    assert!((x - 960.0).abs() < 1e-6);
    assert!((y - 540.0).abs() < 1e-6);
}

#[test]
fn overlay_point_tracks_zoom_without_scaling_sprite() {
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(ZoomClip {
        start: 0.0,
        end: 4.0,
        scale: 2.0,
        center: (960.0, 540.0),
        ease_ms: 0,
        easing: ZoomEasing::Glide,
        mode: ZoomMode::Auto,
        ..Default::default()
    });
    let (zoom, center) = state.eval_zoom(1.0);
    assert!((zoom - 2.0).abs() < 1e-9);

    let crop = state.crop_or_full();
    let (zx, zy, zw, zh) = even_crop_rect(
        zoom,
        (center.0 - crop.0, center.1 - crop.1),
        crop.2.max(2.0) as u32,
        crop.3.max(2.0) as u32,
    );
    let view_1x = crop;
    let view_2x = (crop.0 + zx as f64, crop.1 + zy as f64, zw as f64, zh as f64);
    let widget_w = 960.0;
    let widget_h = 540.0;
    let src = (200.0, 180.0);
    let p1 = source_to_zoomed_point(src.0, src.1, view_1x, widget_w, widget_h);
    let p2 = source_to_zoomed_point(src.0, src.1, view_2x, widget_w, widget_h);
    assert!(
        (p1.0 - p2.0).abs() > 10.0 || (p1.1 - p2.1).abs() > 10.0,
        "hotspot must move with 2× zoom, got {p1:?} vs {p2:?}"
    );

    let mid_1x = source_to_zoomed_point(960.0, 540.0, view_1x, widget_w, widget_h);
    let mid_2x = source_to_zoomed_point(960.0, 540.0, view_2x, widget_w, widget_h);
    assert!((mid_1x.0 - mid_2x.0).abs() < 1e-6);
    assert!((mid_1x.1 - mid_2x.1).abs() < 1e-6);

    let size = 1.4;
    let scale_1x = crate::recording::editor::cursor_sprite::overlay_scale(size, 1.0);
    let scale_2x = crate::recording::editor::cursor_sprite::overlay_scale(size, zoom);
    assert!((scale_1x - scale_2x).abs() < 1e-12);
    assert!((scale_2x - size).abs() < 1e-12);
    assert!((scale_2x - size * zoom).abs() > 0.5);
}

#[test]
fn settings_clamp_click_style_and_duration() {
    let settings = CursorSettings {
        size: 9.0,
        click_scale: 8.0,
        click_opacity: 4.0,
        click_duration_ms: 5000,
        click_intensity: 3.0,
        ..CursorSettings::default()
    }
    .clamped();
    assert!((settings.size - MAX_CURSOR_SIZE).abs() < 1e-12);
    assert!((settings.click_scale - MAX_CLICK_SCALE).abs() < 1e-12);
    assert!((settings.click_opacity - 1.0).abs() < 1e-12);
    assert_eq!(settings.click_duration_ms, MAX_CLICK_DURATION_MS);
    assert!((settings.click_intensity - 1.0).abs() < 1e-12);

    let low = CursorSettings {
        click_scale: 0.01,
        click_opacity: -1.0,
        click_duration_ms: 1,
        ..CursorSettings::default()
    }
    .clamped();
    assert!((low.click_scale - MIN_CLICK_SCALE).abs() < 1e-12);
    assert!((low.click_opacity - 0.0).abs() < 1e-12);
    assert_eq!(low.click_duration_ms, MIN_CLICK_DURATION_MS);
}

#[test]
fn motion_blur_exposure_uses_shutter_angle_and_cap() {
    let settings = MotionBlurSettings {
        enabled: true,
        zoom_strength: 1.0,
        shutter_angle: 360.0,
        transform_temporal_exposure_cap: 0.02,
        ..MotionBlurSettings::default()
    };
    assert!((settings.exposure_seconds(30.0) - 0.02).abs() < 1e-12);

    // A 180° shutter at 30 fps exposes for half the frame interval.
    let half = MotionBlurSettings {
        shutter_angle: 180.0,
        transform_temporal_exposure_cap: 1.0,
        ..settings
    };
    assert!((half.exposure_seconds(30.0) - 1.0 / 60.0).abs() < 1e-12);
}

#[test]
fn motion_blur_samples_span_the_exposure_and_grow_with_travel() {
    let settings = MotionBlurSettings {
        enabled: true,
        zoom_strength: 1.0,
        transform_temporal_exposure_cap: 1.0,
        ..MotionBlurSettings::default()
    };
    // Below one pixel of travel the pose holds: nothing to blur.
    assert!(settings
        .temporal_offsets(30.0, 0.0, MotionBlurBudgetMode::FullQuality)
        .is_empty());

    let exposure = settings.exposure_seconds(30.0);
    let slow = settings.temporal_offsets(30.0, 3.0, MotionBlurBudgetMode::FullQuality);
    let fast = settings.temporal_offsets(30.0, 200.0, MotionBlurBudgetMode::FullQuality);
    let preview =
        settings.temporal_offsets(30.0, 200.0, MotionBlurBudgetMode::LivePreviewPlayback);
    assert!(slow.len() >= 2);
    assert!(fast.len() > slow.len());
    assert!(preview.len() < fast.len());
    for offsets in [&slow, &fast, &preview] {
        assert!(
            offsets.windows(2).all(|pair| pair[0] > pair[1]),
            "subframes must be ordered newest to oldest"
        );
        assert!(offsets
            .iter()
            .all(|offset| *offset <= 0.0 && *offset >= -exposure - f64::EPSILON));
        // The newest subframe is the current frame itself.
        assert!(offsets[0].abs() < f64::EPSILON);
    }
}

#[test]
fn motion_blur_amount_honors_enablement_multiplier_and_cap() {
    let disabled = MotionBlurSettings {
        enabled: false,
        zoom_strength: 1.0,
        ..MotionBlurSettings::default()
    };
    assert_eq!(disabled.effective_zoom_amount(), 0.0);
    assert_eq!(disabled.exposure_seconds(30.0), 0.0);
    assert!(disabled
        .temporal_offsets(30.0, 100.0, MotionBlurBudgetMode::FullQuality)
        .is_empty());

    let enabled = MotionBlurSettings {
        enabled: true,
        zoom_strength: 0.8,
        zoom_blur_amount_multiplier: 2.0,
        zoom_blur_max_amount: 0.65,
        shutter_angle: 360.0,
        transform_temporal_exposure_cap: 1.0,
        ..MotionBlurSettings::default()
    };
    assert!((enabled.effective_zoom_amount() - 0.65).abs() < 1e-12);
    // Strength scales the physical exposure window, not a ghost opacity.
    assert!((enabled.exposure_seconds(30.0) - (1.0 / 30.0) * 0.65).abs() < 1e-12);
}

#[test]
fn ease_timing_keeps_the_recovered_bezier_and_does_not_overshoot() {
    let timing = MotionEffectTransformTiming::default();
    assert_eq!(timing.kind, MotionTimingKind::Ease);
    assert!((timing.apply(0.35) - cubic_bezier_ease(timing, 0.35)).abs() < 1e-12);
    let peak = (0..=100)
        .map(|index| timing.apply(index as f64 / 100.0))
        .fold(f64::MIN, f64::max);
    assert!(peak <= 1.0 + f64::EPSILON, "ease must not overshoot: {peak}");
}

#[test]
fn spring_timing_settles_by_the_end_of_its_duration() {
    for bounce in [0.0, DEFAULT_MOTION_SPRING_BOUNCE, MAX_MOTION_SPRING_BOUNCE] {
        let timing = MotionEffectTransformTiming {
            kind: MotionTimingKind::Spring,
            spring_bounce: bounce,
            ..MotionEffectTransformTiming::default()
        };
        assert!(timing.apply(0.0).abs() < 1e-12, "bounce {bounce}");
        // The held target takes over at the window's end, so the spring must
        // already have settled there.
        assert!(
            (timing.apply(0.999) - 1.0).abs() < 0.02,
            "bounce {bounce} has not settled: {}",
            timing.apply(0.999)
        );
        assert!((timing.apply(1.0) - 1.0).abs() < f64::EPSILON);
    }
}

#[test]
fn spring_bounce_reads_as_the_first_overshoot() {
    let response_peak = |bounce: f64| {
        let timing = MotionEffectTransformTiming {
            kind: MotionTimingKind::Spring,
            spring_bounce: bounce,
            ..MotionEffectTransformTiming::default()
        };
        (0..=1000)
            .map(|index| timing.apply(index as f64 / 1000.0))
            .fold(f64::MIN, f64::max)
    };
    assert!(response_peak(0.0) <= 1.0 + 1e-9, "critically damped overshot");
    assert!(
        (response_peak(0.2) - 1.2).abs() < 0.02,
        "a 20% bounce should peak near 1.2, got {}",
        response_peak(0.2)
    );
}

#[test]
fn default_spring_preset_is_a_soft_settle_not_a_bounce() {
    let timing = MotionEffectTransformTiming {
        kind: MotionTimingKind::Spring,
        ..MotionEffectTransformTiming::default()
    };
    let peak = (0..=1000)
        .map(|index| timing.apply(index as f64 / 1000.0))
        .fold(f64::MIN, f64::max);
    assert!(
        (peak - 1.05).abs() < 0.02,
        "the default spring should round off about 5% past the target, got {peak}"
    );
}

#[test]
fn timing_is_per_clip_and_new_clips_inherit_the_selected_curve() {
    let mut motion = MotionState::default();
    motion.add_segment_at(0.0).expect("first clip");
    let mut first = motion.selected_transform_timing();
    first.transition_duration = 0.9;
    first.kind = MotionTimingKind::Spring;
    first.spring_bounce = 0.4;
    motion.set_transform_timing(first);

    motion.add_segment_at(1.0).expect("second clip");
    let inherited = motion.selected_transform_timing();
    assert_eq!(inherited.transition_duration, 0.9);
    assert_eq!(inherited.kind, MotionTimingKind::Spring);
    assert_eq!(inherited.spring_bounce, 0.4);

    // Editing the second clip must not touch the first clip's curve.
    let mut second = inherited;
    second.transition_duration = 0.2;
    second.kind = MotionTimingKind::Ease;
    motion.set_transform_timing(second);
    assert_eq!(motion.segments[0].timing.transition_duration, 0.9);
    assert_eq!(motion.segments[0].timing.kind, MotionTimingKind::Spring);
    assert_eq!(motion.segments[0].timing.spring_bounce, 0.4);
    assert_eq!(motion.segments[1].timing.transition_duration, 0.2);
    assert_eq!(motion.segments[1].timing.kind, MotionTimingKind::Ease);
}

#[test]
fn motion_blur_uses_recovered_setting_bounds() {
    let settings = MotionBlurSettings {
        cursor_strength: 50.0,
        zoom_strength: 50.0,
        capture_movement_strength: 50.0,
        shutter_angle: 500.0,
        zoom_blur_amount_multiplier: 50.0,
        zoom_blur_max_amount: 500.0,
        transform_temporal_exposure_cap: 50.0,
        transform_trail_opacity: 1.0,
        ..MotionBlurSettings::default()
    }
    .clamped();

    assert_eq!(settings.cursor_strength, 5.0);
    assert_eq!(settings.zoom_strength, 5.0);
    assert_eq!(settings.capture_movement_strength, 5.0);
    assert_eq!(settings.shutter_angle, 360.0);
    assert_eq!(settings.zoom_blur_amount_multiplier, 3.0);
    assert_eq!(settings.zoom_blur_max_amount, 120.0);
    assert_eq!(settings.transform_temporal_exposure_cap, 8.0);
    assert_eq!(settings.transform_trail_opacity, 0.4);
}

#[test]
fn motion_projection_uses_aspect_invariant_depth_ratio() {
    let perspective = 0.18;
    let wide = card_depth(800.0, 450.0, perspective) / 800.0_f64.hypot(450.0);
    let square = card_depth(600.0, 600.0, perspective) / 600.0_f64.hypot(600.0);
    let tall = card_depth(360.0, 640.0, perspective) / 360.0_f64.hypot(640.0);
    assert!((wide - square).abs() < 1e-12);
    assert!((square - tall).abs() < 1e-12);
}

#[test]
fn motion_preset_matching_is_derived_from_knobs() {
    let mut settings = CursorSettings::default();
    assert!(settings.matching_motion_preset().is_none());
    settings.apply_motion_preset(CursorMotionStyle::Focused);
    assert_eq!(
        settings.matching_motion_preset(),
        Some(CursorMotionStyle::Focused)
    );
    assert!((settings.size - CURSOR_MOTION_FOCUSED.size).abs() < 1e-12);
    assert!((settings.smooth - CURSOR_MOTION_FOCUSED.smooth).abs() < 1e-12);
    assert!((settings.speed - CURSOR_MOTION_FOCUSED.speed).abs() < 1e-12);
    settings.apply_motion_preset(CursorMotionStyle::Smooth);
    assert_eq!(
        settings.matching_motion_preset(),
        Some(CursorMotionStyle::Smooth)
    );
    settings.smooth = 0.5;
    assert!(settings.matching_motion_preset().is_none());
}

#[test]
fn zoom_easing_curves_match_at_boundaries() {
    for easing in ZoomEasing::ALL {
        assert!((easing.apply(0.0) - 0.0).abs() < 1e-12);
        assert!((easing.apply(1.0) - 1.0).abs() < 1e-12);
    }
    assert!((ZoomEasing::Linear.apply(0.5) - 0.5).abs() < 1e-12);
    assert!((ZoomEasing::Smooth.apply(0.5) - 0.5).abs() < 1e-12);
    assert!((ZoomEasing::Glide.apply(0.5) - 0.875).abs() < 1e-12);
    assert!((ZoomEasing::Snappy.apply(0.5) - 0.96875).abs() < 1e-12);
    assert!(ZoomEasing::Snappy.apply(0.5) > ZoomEasing::Glide.apply(0.5));
    assert!(ZoomEasing::Glide.apply(0.5) > ZoomEasing::Linear.apply(0.5));
}

#[test]
fn eval_zoom_uses_easing_curve_during_ease_in() {
    let clip = |easing| ZoomClip {
        start: 1.0,
        end: 3.0,
        scale: 2.0,
        center: (200.0, 100.0),
        ease_ms: 1000,
        easing,
        mode: ZoomMode::Manual,
        ..Default::default()
    };
    let linear = eval_zoom(&[clip(ZoomEasing::Linear)], 1.5, 1920.0, 1080.0).0;
    let glide = eval_zoom(&[clip(ZoomEasing::Glide)], 1.5, 1920.0, 1080.0).0;
    let snappy = eval_zoom(&[clip(ZoomEasing::Snappy)], 1.5, 1920.0, 1080.0).0;
    let at_start = eval_zoom(&[clip(ZoomEasing::Snappy)], 1.0, 1920.0, 1080.0).0;
    let at_hold = eval_zoom(&[clip(ZoomEasing::Linear)], 2.0, 1920.0, 1080.0).0;
    assert!((linear - 1.5).abs() < 1e-9);
    assert!((glide - 1.875).abs() < 1e-9);
    assert!(snappy > glide);
    assert!((at_start - 1.0).abs() < 1e-9);
    assert!((at_hold - 2.0).abs() < 1e-9);
}

fn zoom_scale_steps(clips: &[ZoomClip], start: f64, end: f64, samples: usize) -> Vec<f64> {
    let mut scales = Vec::with_capacity(samples + 1);
    for index in 0..=samples {
        let t = start + (end - start) * index as f64 / samples as f64;
        scales.push(eval_zoom(clips, t, 1920.0, 1080.0).0);
    }
    scales
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .collect()
}

#[test]
fn suggest_zoom_clips_ease_in_and_out_at_rest() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 960.0, 540.0)]);
    assert_eq!(state.suggest_zoom_clips(), 1);
    let (start, end) = (state.zoom_clips[0].start, state.zoom_clips[0].end);
    assert_eq!(state.zoom_clips[0].easing, ZoomEasing::Smooth);
    assert_eq!(state.zoom_clips[0].ease_ms, DEFAULT_ZOOM_EASE_MS);

    let steps = zoom_scale_steps(&state.zoom_clips, start, end, 90);
    let max_step = steps.iter().copied().fold(0.0_f64, f64::max);
    let (first, last) = (steps[0], steps[steps.len() - 1]);
    assert!(
        first < max_step * 0.05,
        "auto zoom must launch from rest: first frame step {first} vs peak {max_step}"
    );
    assert!(
        last < max_step * 0.05,
        "auto zoom must settle to rest: last frame step {last} vs peak {max_step}"
    );

    // The measurement must be able to see a launch at speed: an unshaped ramp
    // steps evenly from its very first frame.
    state.zoom_clips[0].easing = ZoomEasing::Linear;
    let linear = zoom_scale_steps(&state.zoom_clips, start, end, 90);
    assert!(
        linear[0] > first * 10.0,
        "boundary-step measurement must detect fast launches: linear {} vs smooth {}",
        linear[0],
        first
    );
}

#[test]
fn suggested_zoom_easing_can_be_overridden() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 960.0, 540.0)]);
    assert_eq!(state.suggest_zoom_clips(), 1);
    state.selected_zoom = Some(0);
    state.set_selected_zoom_easing(ZoomEasing::Glide);
    assert_eq!(state.zoom_clips[0].easing, ZoomEasing::Glide);
    let steps = zoom_scale_steps(&state.zoom_clips, state.zoom_clips[0].start, state.zoom_clips[0].end, 90);
    let max_step = steps.iter().copied().fold(0.0_f64, f64::max);
    assert!(
        steps[0] > max_step * 0.5,
        "the chosen preset's shape must survive: first frame step {} vs peak {}",
        steps[0],
        max_step
    );
}

#[test]
fn eval_zoom_pose_eases_yaw_like_still_motion() {
    let mut state = VideoEditState::new(metadata());
    assert!(state.add_zoom_at_playhead().is_some());
    state.zoom_clips[0].start = 0.0;
    state.zoom_clips[0].end = 2.0;
    state.zoom_clips[0].ease_ms = 600;
    state.zoom_clips[0].easing = ZoomEasing::Linear;
    state.set_selected_zoom_yaw(8.0);
    let start = state.eval_zoom_pose(0.0);
    let mid = state.eval_zoom_pose(0.3);
    let hold = state.eval_zoom_pose(1.2);
    assert!(start.rotation_y.abs() < 1e-6);
    assert!((mid.rotation_y - 4.0).abs() < 0.2);
    assert!((hold.rotation_y - 8.0).abs() < 1e-6);
    assert!(!state.zoom_clips[0].has_card_motion() || state.zoom_clips[0].rotation_y > 0.0);
}

#[test]
fn zoom_pose_follows_the_shared_easing_presets() {
    for easing in ZoomEasing::ALL {
        let mut state = VideoEditState::new(metadata());
        assert!(state.add_zoom_at_playhead().is_some());
        state.zoom_clips[0].start = 0.0;
        state.zoom_clips[0].end = 2.0;
        state.zoom_clips[0].ease_ms = 600;
        state.zoom_clips[0].easing = easing;
        state.set_selected_zoom_yaw(8.0);
        let expected = 8.0 * easing.apply(0.5);
        let mid = state.eval_zoom_pose(0.3);
        assert!(
            (mid.rotation_y - expected).abs() < 1e-9,
            "{easing:?} pose must follow its own preset curve: got {} want {expected}",
            mid.rotation_y
        );
    }
}

#[test]
fn selected_zoom_easing_and_ease_ms_update_clip() {
    let mut state = VideoEditState::new(metadata());
    assert!(state.add_zoom_at_playhead().is_some());
    assert_eq!(
        state.selected_zoom_clip().unwrap().easing,
        ZoomEasing::Glide
    );
    assert_eq!(
        state.selected_zoom_clip().unwrap().ease_ms,
        DEFAULT_ZOOM_EASE_MS
    );
    state.set_selected_zoom_easing(ZoomEasing::Snappy);
    state.set_selected_zoom_ease_ms(240);
    let clip = state.selected_zoom_clip().unwrap();
    assert_eq!(clip.easing, ZoomEasing::Snappy);
    assert_eq!(clip.ease_ms, 240);
    state.reset_zoom_animation();
    let clip = state.selected_zoom_clip().unwrap();
    assert_eq!(clip.easing, ZoomEasing::Glide);
    assert_eq!(clip.ease_ms, DEFAULT_ZOOM_EASE_MS);
}

#[test]
fn reset_zoom_animation_restores_the_modes_default() {
    let mut auto = VideoEditState::new(metadata());
    attach_pointer(&mut auto, 960.0, 540.0);
    assert!(auto.add_zoom_at_playhead().is_some());
    assert_eq!(auto.selected_zoom_clip().unwrap().mode, ZoomMode::Auto);
    auto.set_selected_zoom_easing(ZoomEasing::Snappy);
    auto.set_selected_zoom_yaw(8.0);
    auto.reset_zoom_animation();
    let clip = auto.selected_zoom_clip().unwrap();
    // Reset must not bring the edge snap back to an auto-placed zoom.
    assert_eq!(clip.easing, ZoomEasing::Smooth);
    assert_eq!(clip.ease_ms, DEFAULT_ZOOM_EASE_MS);
    assert_eq!(clip.rotation_y, 0.0);

    let mut manual = VideoEditState::new(metadata());
    assert!(manual.add_zoom_at_playhead().is_some());
    assert_eq!(manual.selected_zoom_clip().unwrap().mode, ZoomMode::Manual);
    manual.set_selected_zoom_easing(ZoomEasing::Snappy);
    manual.reset_zoom_animation();
    assert_eq!(manual.selected_zoom_clip().unwrap().easing, ZoomEasing::Glide);
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

#[test]
fn replay_rewinds_from_the_end_and_keeps_mid_playhead() {
    assert_eq!(playhead_for_replay(7.0, 7.0), 0.0);
    assert_eq!(playhead_for_replay(6.96, 7.0), 0.0);
    assert!((playhead_for_replay(3.2, 7.0) - 3.2).abs() < 1e-12);
}

#[test]
fn media_timestamp_zero_is_valid_once_seek_completes() {
    assert_eq!(usable_media_timestamp_seconds(0, false), Some(0.0));
    assert_eq!(usable_media_timestamp_seconds(1_500_000, false), Some(1.5));
    assert_eq!(usable_media_timestamp_seconds(0, true), None);
    assert_eq!(usable_media_timestamp_seconds(-1, false), None);
}

#[test]
fn freeze_extends_the_composition_past_the_source_end() {
    let mut state = VideoEditState::new(metadata());
    assert_eq!(state.freeze_tail_seconds(), 0.0);
    assert!(state.extend_last_segment(1.0));
    assert!((state.freeze_tail_seconds() - 1.0).abs() < 1e-9);
    assert!((state.trim_end_seconds - 10.0).abs() < 1e-9, "source end is untouched");
    assert!(
        (state.composition_duration() - 11.0).abs() < 1e-9,
        "the held frame lengthens the composition"
    );
    assert!(
        (state.content_end_seconds() - 11.0).abs() < 1e-9,
        "playback runs through the hold"
    );
}

#[test]
fn a_freeze_hold_starts_when_the_media_ends_short_of_the_container() {
    // The last decodable frame's timestamp sits one frame before the container
    // duration, so the playhead never reaches `footage_end` on its own before
    // the source runs out. The finished media must be enough to enter the hold,
    // or the tail is skipped and playback stops at the source end.
    assert!(
        freeze_hold_active(1.0, true, 9.9667, 10.0, 0.0),
        "media end must open the hold even when the playhead is short"
    );
    // The one-frame lead starts it without waiting for the media to report
    // end, so the playhead does not sit on the last frame at the handoff.
    assert!(freeze_hold_active(
        1.0,
        false,
        9.9667,
        10.0,
        freeze_hold_lead(30.0)
    ));
    // Crossing footage_end opens it too, with or without a lead.
    assert!(freeze_hold_active(1.0, false, 10.0, 10.0, 0.0));
}

#[test]
fn no_hold_without_a_tail_or_before_the_source_is_exhausted() {
    // No tail: nothing to hold, so the media end still stops playback.
    assert!(!freeze_hold_active(0.0, true, 10.0, 10.0, 0.0));
    // Mid-clip playback must not leap into a hold, even with the lead.
    assert!(!freeze_hold_active(
        1.0,
        false,
        4.0,
        10.0,
        freeze_hold_lead(30.0)
    ));
}

#[test]
fn zoom_clips_keep_advancing_through_the_freeze_hold() {
    let mut state = VideoEditState::new(metadata());
    state.extend_last_segment(2.0);
    assert!((state.composition_duration() - 12.0).abs() < 1e-9);
    // The hold is part of the composition, so a zoom fits inside it.
    let index = state
        .add_zoom_at(10.5)
        .expect("a zoom placed over the hold must fit");
    state.zoom_clips[index].mode = ZoomMode::Manual;
    state.playhead_seconds = 11.0;
    let source_t = state.source_playhead();
    assert!(
        (source_t - 10.0).abs() < 1e-6,
        "the source frame is pinned to the last real frame"
    );
    // Matching on the pinned source time would miss the clip entirely.
    assert!(state.eval_zoom(source_t).0 <= 1.01);
    // The composition time is what the hold advances through, so it finds it.
    let (scale, _) = state.eval_zoom_at(state.playhead_seconds, source_t);
    assert!(
        scale > 1.01,
        "the zoom must keep applying inside the hold, got {scale}"
    );
}

#[test]
fn cursor_hide_clips_keep_working_through_the_freeze_hold() {
    let mut state = VideoEditState::new(metadata());
    state.extend_last_segment(2.0);
    let index = state
        .add_cursor_hide_at(10.5)
        .expect("a hide placed over the hold must fit");
    assert!(state.cursor_hide_clips[index].end <= 12.0 + 1e-9);
    state.playhead_seconds = 11.0;
    let source_t = state.source_playhead();
    // The pinned source time sits before the hide...
    assert!((state.cursor_hide_alpha_for_source(source_t) - 1.0).abs() < 1e-12);
    // ...but the composition time lands inside it.
    assert!((state.cursor_hide_alpha(state.playhead_seconds) - 0.0).abs() < 1e-12);
}

#[test]
fn freeze_playhead_holds_the_last_source_frame() {
    let mut state = VideoEditState::new(metadata());
    state.extend_last_segment(1.0);
    state.playhead_seconds = 10.5;
    // Past the source end the player seeks the last real frame and stops
    // there, so the preview shows the held frame instead of running out.
    assert!((state.source_playhead() - 10.0).abs() < 1e-6);
}

#[test]
fn a_freeze_after_a_trim_holds_the_trimmed_last_frame() {
    let mut state = VideoEditState::new(metadata());
    state.set_trim_end(8.0);
    assert!(state.extend_last_segment(2.0));
    // The composition runs 8..10s holding the frame at 8 — not the frames the
    // trim cut away (9, 10).
    state.playhead_seconds = 9.0;
    assert!(
        (state.source_playhead() - 8.0).abs() < 1e-6,
        "the hold must freeze the trimmed end, got {}",
        state.source_playhead()
    );

    // Drop the hold and the same playhead is back over the trimmed band, which
    // still previews the frame a reveal would bring in.
    assert!(state.clear_freeze_tail());
    state.playhead_seconds = 9.0;
    assert!(
        (state.source_playhead() - 9.0).abs() < 1e-6,
        "the trimmed band must preview the footage it would reveal, got {}",
        state.source_playhead()
    );
}

#[test]
fn follow_playhead_pans_the_freeze_tail_into_view() {
    let mut state = VideoEditState::new(metadata());
    state.extend_last_segment(6.0);
    state.playhead_seconds = 15.0;
    state.follow_playhead_on_timeline();
    let visible = state.visible_span_seconds();
    assert!(
        state.timeline_scroll_seconds > 0.0,
        "the tail past the source must be panned into view"
    );
    assert!(
        state.playhead_seconds <= state.timeline_scroll_seconds + visible + 1e-9,
        "the playhead must end up inside the window"
    );
    assert!(state.timeline_scroll_seconds <= state.max_timeline_scroll() + 1e-9);
}

#[test]
fn dragging_the_right_edge_past_the_end_opens_a_hold_that_applies() {
    let mut state = VideoEditState::new(metadata());
    // The tail has to name its segment, or nothing applies it: the composition
    // would not lengthen and export would not pad.
    state.set_trim_end(11.0);
    assert!((state.freeze_tail_seconds() - 1.0).abs() < 1e-9);
    assert!(
        state.freeze_applies_to_segment(0),
        "the hold must belong to the final segment"
    );
    assert!(
        (state.composition_duration() - 11.0).abs() < 1e-9,
        "the clip actually gets longer"
    );
    assert!(
        (state.content_end_seconds() - 11.0).abs() < 1e-9,
        "playback runs through the hold"
    );
    assert!(state.needs_reencode());
}

#[test]
fn trimming_right_consumes_the_freeze_tail_first() {
    let mut state = VideoEditState::new(metadata());
    state.extend_last_segment(2.0);
    state.set_trim_end(10.5);
    assert!(
        (state.freeze_tail_seconds() - 0.5).abs() < 1e-9,
        "the hold gives way before real frames"
    );
    state.set_trim_end(9.0);
    assert_eq!(state.freeze_tail_seconds(), 0.0, "the hold is spent first");
    assert!((state.trim_end_seconds - 9.0).abs() < 1e-9);
}

#[test]
fn clearing_the_freeze_tail_restores_the_original_end() {
    let mut state = VideoEditState::new(metadata());
    state.extend_last_segment(3.0);
    assert!(state.clear_freeze_tail());
    assert_eq!(state.freeze_tail_seconds(), 0.0);
    assert!((state.trim_end_seconds - 10.0).abs() < 1e-9);
    assert!(!state.clear_freeze_tail(), "clearing twice is a no-op");
}

#[test]
fn freeze_needs_a_kept_final_segment() {
    let mut state = VideoEditState::new(metadata());
    state.add_cut(5.0);
    // Removing the last segment leaves nothing on screen to hold.
    state.toggle_segment(1);
    assert!(!state.extend_last_segment(1.0));
    assert_eq!(state.freeze_tail_seconds(), 0.0);
}

#[test]
fn freeze_survives_a_project_roundtrip() {
    let mut state = VideoEditState::new(metadata());
    state.extend_last_segment(1.5);
    let file = state.to_project();
    let mut restored = VideoEditState::new(metadata());
    restored.apply_project(file);
    assert_eq!(restored.frozen_segment, state.frozen_segment);
    assert!(
        (restored.freeze_tail_seconds() - 1.5).abs() < 1e-9,
        "the hold is restored on reload"
    );
}

#[test]
fn reset_video_edits_clears_the_freeze_tail() {
    let mut state = VideoEditState::new(metadata());
    state.extend_last_segment(2.0);
    state.reset_video_edits();
    assert_eq!(state.freeze_tail_seconds(), 0.0);
    assert!((state.trim_end_seconds - 10.0).abs() < 1e-9);
}

#[test]
fn a_freeze_forces_reencode_because_stream_copy_cannot_hold_a_frame() {
    let mut state = VideoEditState::new(metadata());
    assert!(!state.needs_reencode());
    state.extend_last_segment(1.0);
    assert!(state.needs_reencode());
}

#[test]
fn gradient_normalizes_stops_into_a_usable_shape() {
    // Unsorted, out-of-range, and over the stop ceiling all at once.
    let gradient = VideoGradient {
        kind: GradientKind::Linear,
        stops: (0..12)
            .rev()
            .map(|i| GradientStop::new(i as f64 / 4.0, i as u8, 0, 255 - i as u8))
            .collect(),
        angle_degrees: 45.0,
        reversed: false,
    }
    .normalized();

    assert_eq!(gradient.stops.len(), MAX_GRADIENT_STOPS);
    // Sorted ascending, every position inside 0..=1.
    for pair in gradient.stops.windows(2) {
        assert!(pair[0].position <= pair[1].position);
    }
    assert!(gradient.stops.iter().all(|s| (0.0..=1.0).contains(&s.position)));
}

#[test]
fn gradient_pads_a_single_stop_up_to_the_minimum() {
    let gradient = VideoGradient {
        stops: vec![GradientStop::new(0.5, 10, 20, 30)],
        ..VideoGradient::default()
    }
    .normalized();
    assert_eq!(gradient.stops.len(), MIN_GRADIENT_STOPS);
}

#[test]
fn reversing_a_gradient_flips_draw_order_only() {
    let gradient = VideoGradient {
        kind: GradientKind::Linear,
        stops: vec![
            GradientStop::new(0.0, 255, 0, 0),
            GradientStop::new(1.0, 0, 0, 255),
        ],
        angle_degrees: 30.0,
        reversed: true,
    };
    let drawn = gradient.draw_stops();
    assert_eq!(drawn[0].r, 0);
    assert_eq!(drawn[0].b, 255);
    assert_eq!(drawn[1].r, 255);
    // Reversal is a view concern — the stored spec is untouched.
    assert_eq!(gradient.stops[0].r, 255);
    assert!(gradient.reversed);
}

#[test]
fn gradient_endpoints_span_the_box_and_flip_with_the_angle() {
    let gradient = VideoGradient {
        angle_degrees: 0.0,
        ..VideoGradient::default()
    };
    let ((x0, y0), (x1, y1)) = gradient.endpoints(400.0, 200.0);
    // 0 degrees is left-to-right and centred vertically.
    assert!(x0 < x1);
    assert!((y0 - 100.0).abs() < 1e-6);
    assert!((y1 - 100.0).abs() < 1e-6);

    let vertical = VideoGradient {
        angle_degrees: 90.0,
        ..VideoGradient::default()
    };
    let ((vx0, vy0), (vx1, vy1)) = vertical.endpoints(400.0, 200.0);
    assert!(vy0 < vy1);
    assert!((vx0 - 200.0).abs() < 1e-6);
    assert!((vx1 - 200.0).abs() < 1e-6);
}

#[test]
fn corner_radius_scales_like_padding_and_starts_square() {
    let mut state = VideoEditState::new(metadata());
    state.apply_aspect_ratio(1920, 1080);

    // Fresh projects must not inherit the old inert 18.0 default.
    assert_eq!(state.background_corner_radius, 0.0);
    assert_eq!(state.background_corner_radius_px(), 0.0);
    assert!(!state.has_corner_radius());
    // No fill, no radius: nothing forces the composite graph.
    assert!(!state.needs_composite());

    state.background_corner_radius = 20.0;
    // 20 slider units against a 1920px reference edge, same as padding.
    assert!((state.background_corner_radius_px() - 96.0).abs() < 1e-9);
    assert!(state.has_corner_radius());
    // A radius with no background still needs the composite graph.
    assert!(state.needs_composite());
}

#[test]
fn a_negligible_corner_radius_does_not_force_a_composite() {
    let mut state = VideoEditState::new(metadata());
    state.apply_aspect_ratio(1920, 1080);
    // Sub-pixel once scaled — not worth a mask.
    state.background_corner_radius = 0.1;
    assert!(!state.has_corner_radius());
    assert!(!state.needs_composite());
}

fn zoom_clip_at(start: f64, end: f64) -> ZoomClip {
    ZoomClip {
        start,
        end,
        scale: 1.8,
        center: (960.0, 540.0),
        mode: ZoomMode::Manual,
        ..Default::default()
    }
}

#[test]
fn a_hidden_zoom_stops_framing_the_preview_but_keeps_its_span() {
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.8));
    let zoomed = state.eval_zoom(1.9);
    assert!(zoomed.0 > 1.01, "the clip frames the preview while it is on");

    state.set_zoom_hidden(0, true);
    let flat = state.eval_zoom(1.9);
    assert!(
        (flat.0 - 1.0).abs() < 1e-9,
        "a hidden clip must not scale the frame",
    );
    // The span survives, which is the whole point of hiding rather than
    // deleting: the user can turn it back on.
    assert_eq!(state.zoom_clips.len(), 1);
    assert!((state.zoom_clips[0].start - 1.0).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 2.8).abs() < 1e-9);

    state.set_zoom_hidden(0, false);
    assert!(
        state.eval_zoom(1.9).0 > 1.01,
        "showing a clip again must restore its framing",
    );
}

#[test]
fn a_hidden_clip_does_not_force_a_composite_on_its_own() {
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.8));
    assert!(state.needs_composite());

    state.set_zoom_hidden(0, true);
    assert!(
        !state.needs_composite(),
        "a clip that changes nothing in the output must not drag in the \
         composite graph",
    );
}

#[test]
fn a_hidden_hide_stops_hiding_the_cursor() {
    let mut state = VideoEditState::new(metadata());
    state.cursor_hide_clips.push(CursorHideClip {
        start: 1.0,
        end: 2.8,
        hidden: false,
    });
    assert_eq!(state.cursor_hide_alpha(1.9), 0.0);

    state.set_cursor_hide_hidden(0, true);
    assert_eq!(
        state.cursor_hide_alpha(1.9),
        1.0,
        "a disabled hide must leave the cursor visible",
    );
}

#[test]
fn duplicating_a_clip_lands_it_right_after_its_source() {
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));
    state.selected_zoom = Some(0);

    let new_index = state.duplicate_selected_clip().expect("duplicate lands");
    assert_eq!(new_index, 1);
    assert_eq!(state.zoom_clips.len(), 2);
    assert!((state.zoom_clips[1].start - 2.0).abs() < 1e-9);
    assert!((state.zoom_clips[1].end - 3.0).abs() < 1e-9);
    // The copy is the selection, so the panel edits what was just made.
    assert_eq!(state.selected_zoom, Some(1));
}

#[test]
fn duplicating_refuses_to_overlap_an_existing_clip() {
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));
    // The neighbour sits exactly where the copy would land.
    state.zoom_clips.push(zoom_clip_at(2.0, 3.0));

    assert!(
        state.duplicate_zoom_clip(0, 2.0).is_none(),
        "a duplicate that would overlap must be refused",
    );
    assert_eq!(state.zoom_clips.len(), 2);
}

#[test]
fn cut_then_paste_moves_a_clip_to_the_playhead() {
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));

    assert!(state.cut_zoom_clip(0));
    assert!(
        state.zoom_clips.is_empty(),
        "cut must take the clip off the timeline",
    );
    // The span is on the clipboard, so the paste has something to restore.
    let span = match state.clipboard_clip().expect("cut fills the clipboard") {
        ClipClipboard::Zoom(clip) => clip.duration(),
        ClipClipboard::Hide(_) => panic!("a zoom was cut, so the clipboard holds a zoom"),
    };
    assert!((span - 1.0).abs() < 1e-9);

    state.playhead_seconds = 5.0;
    state.timeline_offset_seconds = 0.0;
    let pasted = state.paste_clipboard_at_playhead().expect("paste lands");
    assert_eq!(pasted, 0);
    let clip = &state.zoom_clips[0];
    assert!((clip.start - 5.0).abs() < 1e-9, "paste lands at the playhead");
    assert!((clip.end - 6.0).abs() < 1e-9);
}

#[test]
fn copy_leaves_the_source_clip_in_place() {
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));

    assert!(state.copy_zoom_clip(0));
    assert_eq!(state.zoom_clips.len(), 1, "copy must not remove the source");
    assert!(state.clipboard_clip().is_some());

    state.clear_clipboard();
    assert!(state.clipboard_clip().is_none());
    assert!(state.paste_clipboard_at_playhead().is_none());
}

#[test]
fn a_paste_that_would_overlap_is_refused_and_changes_nothing() {
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));
    state.copy_zoom_clip(0);

    // The playhead sits on the clip that was just copied.
    state.playhead_seconds = 1.5;
    state.timeline_offset_seconds = 0.0;
    assert!(state.paste_clipboard_at_playhead().is_none());
    assert_eq!(state.zoom_clips.len(), 1, "a refused paste must be a no-op");
}

#[test]
fn the_clipboard_remembers_which_track_the_clip_came_from() {
    let mut state = VideoEditState::new(metadata());
    state.cursor_hide_clips.push(CursorHideClip {
        start: 1.0,
        end: 2.0,
        hidden: false,
    });

    assert!(state.copy_cursor_hide_clip(0));
    state.playhead_seconds = 4.0;
    state.paste_clipboard_at_playhead().expect("hide paste lands");

    assert!(
        state.zoom_clips.is_empty(),
        "a copied hide must not paste as a zoom",
    );
    assert_eq!(state.cursor_hide_clips.len(), 2);
    assert!((state.cursor_hide_clips[1].start - 4.0).abs() < 1e-9);
}

#[test]
fn the_context_menu_delete_drops_the_clip_it_was_opened_on() {
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));
    state.zoom_clips.push(zoom_clip_at(3.0, 4.0));
    // The selection is on the *other* clip, which is the case the old
    // select-then-delete flow got wrong.
    state.selected_zoom = Some(1);

    state.remove_zoom_clip(0);

    assert_eq!(state.zoom_clips.len(), 1);
    assert!(
        (state.zoom_clips[0].start - 3.0).abs() < 1e-9,
        "the clip the menu was opened on must be the one that goes",
    );
    assert_eq!(
        state.selected_zoom,
        Some(0),
        "the selection follows the shift instead of pointing past the end",
    );
}

#[test]
fn hiding_a_zoom_leaves_its_neighbour_morphing_on_its_own() {
    // Two auto zooms close enough to morph. Disabling the first must not let
    // the second keep framing the gap as if the first were still there.
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(ZoomClip {
        start: 1.0,
        end: 2.0,
        scale: 2.0,
        mode: ZoomMode::Auto,
        ..Default::default()
    });
    state.zoom_clips.push(ZoomClip {
        start: 2.1,
        end: 3.1,
        scale: 2.0,
        mode: ZoomMode::Auto,
        ..Default::default()
    });

    state.set_zoom_hidden(1, true);
    let (scale_in_gap, _) = state.eval_zoom(2.05);
    assert!(
        (scale_in_gap - 1.0).abs() < 1e-9,
        "with the second clip off there is no neighbour to morph into, so the \
         gap must show the full frame",
    );
}

#[test]
fn paste_places_the_clip_at_the_playhead_on_the_timeline_clock() {
    // Clip spans are composition times, the same space `add_zoom_at` writes.
    // A paste placed from the *source* clock would land somewhere else as soon
    // as the composition is trimmed, so this pins the two together: a trim
    // must not shift where a paste lands.
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));
    state.copy_zoom_clip(0);

    state.trim_start_seconds = 3.0;
    state.playhead_seconds = 4.0;
    state.paste_clipboard_at_playhead().expect("paste lands");

    let pasted = state.zoom_clips.last().expect("a clip was pasted");
    assert!(
        (pasted.start - 4.0).abs() < 1e-9,
        "the paste must land on the playhead's timeline position, got {}",
        pasted.start,
    );
}

#[test]
fn paste_is_offered_only_when_it_would_land() {
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));
    // Nothing copied yet, so there is nothing to paste.
    assert!(!state.can_paste_clipboard_at_playhead());

    state.copy_zoom_clip(0);
    // The playhead sits on the clip that was copied, so a paste would collide.
    state.playhead_seconds = 1.5;
    assert!(
        !state.can_paste_clipboard_at_playhead(),
        "a paste that would overlap must not be offered",
    );

    state.playhead_seconds = 5.0;
    assert!(
        state.can_paste_clipboard_at_playhead(),
        "a clear spot under the playhead must offer a paste",
    );
    // Offering it and doing it must agree.
    assert!(state.paste_clipboard_at_playhead().is_some());
}

#[test]
fn cut_always_leaves_something_to_paste_back() {
    // Cut is destructive on the timeline, so the clipboard is the only way to
    // undo it. This pins that a cut always fills the slot a paste reads.
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));
    state.cut_zoom_clip(0);

    assert!(
        state.clipboard_clip().is_some(),
        "a cut that left the clipboard empty would strand the clip",
    );
    state.playhead_seconds = 6.0;
    assert!(state.can_paste_clipboard_at_playhead());
    assert!(state.paste_clipboard_at_playhead().is_some());
    assert_eq!(state.zoom_clips.len(), 1, "the cut clip came back");
}

#[test]
fn the_ghost_only_shows_on_the_track_the_clip_came_from() {
    // A zoom copied off the Zoom track must not offer a ghost on the Hide
    // lane, or a click there would place a zoom into a hide.
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));
    state.copy_zoom_clip(0);

    assert_eq!(state.clipboard_duration_for(true), Some(1.0));
    assert_eq!(
        state.clipboard_duration_for(false),
        None,
        "a copied zoom must not ghost on the Hide track",
    );

    state.cursor_hide_clips.push(CursorHideClip {
        start: 5.0,
        end: 6.5,
        hidden: false,
    });
    state.copy_cursor_hide_clip(0);
    assert_eq!(state.clipboard_duration_for(false), Some(1.5));
    assert_eq!(state.clipboard_duration_for(true), None);
}

#[test]
fn click_to_place_pastes_at_the_pointer_and_leaves_the_playhead_alone() {
    // The ghost previews a paste at the pointer, so the click has to land
    // exactly there — and it must not drag the playhead along, or the first
    // place would move the second one's destination.
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));
    state.copy_zoom_clip(0);
    state.playhead_seconds = 8.0;

    let placed = state
        .paste_clipboard_at(4.0)
        .expect("a free spot takes the paste");
    let clip = &state.zoom_clips[placed];
    assert!((clip.start - 4.0).abs() < 1e-9, "the paste lands on the pointer");
    assert!((clip.end - 5.0).abs() < 1e-9);
    assert!(
        (state.playhead_seconds - 8.0).abs() < 1e-9,
        "click-to-place must not move the playhead",
    );
}

#[test]
fn the_ghost_reports_an_occupied_spot_before_the_click() {
    // The painter asks this to decide filled versus hollow, so it has to agree
    // with what the paste would actually do.
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));
    state.copy_zoom_clip(0);

    assert!(
        !state.paste_spot_is_free(1.5, 1.0, true),
        "a spot under an existing clip is not free",
    );
    assert!(state.paste_spot_is_free(4.0, 1.0, true));
    // Offered and refused must match the paste itself.
    assert!(state.paste_clipboard_at(1.5).is_none());
    assert!(state.paste_clipboard_at(4.0).is_some());
}

#[test]
fn placing_a_clip_ends_the_copy_state() {
    // The editor dims while a clip is held. If a paste left the clipboard
    // filled, the window would stay dimmed and the ghost would stay on the
    // lane, so the next click would place another copy instead of going back
    // to normal editing.
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));
    state.copy_zoom_clip(0);
    assert!(state.is_pasting_clip(), "a copy holds the clip");
    assert_eq!(state.clipboard_duration_for(true), Some(1.0));

    state.paste_clipboard_at(4.0).expect("the paste lands");

    assert!(
        !state.is_pasting_clip(),
        "a successful paste must finish the copy",
    );
    assert_eq!(
        state.clipboard_duration_for(true),
        None,
        "the ghost must go away once the clip is placed",
    );
    // Nothing left to paste, so the menu stops offering it.
    assert!(!state.can_paste_clipboard_at_playhead());
}

#[test]
fn a_refused_paste_keeps_the_clip_for_another_try() {
    // A click on an occupied spot has to leave the copy alone: the user is
    // still placing it, and dropping the clipboard there would lose it.
    let mut state = VideoEditState::new(metadata());
    state.zoom_clips.push(zoom_clip_at(1.0, 2.0));
    state.copy_zoom_clip(0);

    assert!(state.paste_clipboard_at(1.5).is_none(), "occupied");
    assert!(
        state.is_pasting_clip(),
        "a refused paste must keep the clip on the clipboard",
    );

    // And it still places once a free spot is clicked.
    assert!(state.paste_clipboard_at(5.0).is_some());
    assert!(!state.is_pasting_clip());
}

#[test]
fn a_new_zoom_opens_with_the_last_edited_style() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 960.0, 540.0);
    let first = state.add_zoom_at(0.5).expect("first zoom fits");
    state.selected_zoom = Some(first);
    state.set_selected_zoom_scale(2.4);
    state.set_selected_zoom_easing(ZoomEasing::Linear);

    let second = state.add_zoom_at(5.0).expect("second zoom fits");
    let clip = &state.zoom_clips[second];
    assert!((clip.scale - 2.4).abs() < 1e-9);
    assert_eq!(clip.easing, ZoomEasing::Linear);
}

#[test]
fn a_new_zoom_keeps_the_factory_style_until_a_zoom_is_edited() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 960.0, 540.0);
    let index = state.add_zoom_at(0.5).expect("zoom fits");
    let clip = &state.zoom_clips[index];
    assert!((clip.scale - DEFAULT_ZOOM_SCALE).abs() < 1e-9);
    assert_eq!(clip.easing, ZoomEasing::Glide);
    assert!(!clip.instant);
}

#[test]
fn an_instant_zoom_snaps_without_an_eased_ramp() {
    let clips = [ZoomClip {
        start: 1.0,
        end: 2.8,
        scale: 2.0,
        center: (200.0, 100.0),
        ease_ms: 600,
        easing: ZoomEasing::Glide,
        mode: ZoomMode::Auto,
        instant: true,
        ..Default::default()
    }];
    // Inside the ease window an animated zoom would still be ramping in;
    // the instant zoom is already at full framing.
    let (scale, center) = eval_zoom(&clips, 1.1, 1920.0, 1080.0);
    assert!((scale - 2.0).abs() < 1e-9);
    assert!((center.0 - 200.0).abs() < 1e-9);
    assert!((center.1 - 100.0).abs() < 1e-9);
    let (out_scale, out_center) = eval_zoom(&clips, 2.7, 1920.0, 1080.0);
    assert!((out_scale - 2.0).abs() < 1e-9);
    assert!((out_center.0 - 200.0).abs() < 1e-9);
}

#[test]
fn an_instant_zoom_never_morphs_from_its_neighbour() {
    let clips = [
        ZoomClip {
            start: 0.0,
            end: 2.0,
            scale: 1.5,
            center: (400.0, 300.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            ..Default::default()
        },
        ZoomClip {
            start: 2.0,
            end: 4.0,
            scale: 1.8,
            center: (1_500.0, 800.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            instant: true,
            ..Default::default()
        },
    ];
    // The second zoom snaps to its own framing instead of blending from
    // the first.
    let (scale, center) = eval_zoom(&clips, 2.3, 1920.0, 1080.0);
    assert!((scale - 1.8).abs() < 1e-9);
    assert!((center.0 - 1_500.0).abs() < 1e-9);
    assert!((center.1 - 800.0).abs() < 1e-9);
    // And the first zoom eases back out on its own: nothing morphs from an
    // instant neighbour, so its framing is not held for one.
    let (held_scale, _) = eval_zoom(&clips, 1.9, 1920.0, 1080.0);
    assert!(held_scale < 1.5);
}

#[test]
fn a_gap_after_an_instant_zoom_returns_to_full_frame() {
    let clips = [
        ZoomClip {
            start: 0.0,
            end: 2.0,
            scale: 1.5,
            center: (400.0, 300.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            instant: true,
            ..Default::default()
        },
        ZoomClip {
            start: 2.3,
            end: 4.3,
            scale: 1.5,
            center: (1_500.0, 800.0),
            ease_ms: 600,
            easing: ZoomEasing::Smooth,
            mode: ZoomMode::Auto,
            ..Default::default()
        },
    ];
    let (gap_scale, _) = eval_zoom(&clips, 2.15, 1920.0, 1080.0);
    assert!((gap_scale - 1.0).abs() < 1e-9);
}

#[test]
fn set_selected_zoom_instant_flips_the_flag_and_protects_the_clip() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 960.0, 540.0);
    let index = state.add_zoom_at(0.5).expect("zoom fits");
    state.selected_zoom = Some(index);
    assert!(!state.selected_zoom_clip().unwrap().instant);
    state.set_selected_zoom_instant(true);
    let clip = state.selected_zoom_clip().unwrap();
    assert!(clip.instant);
    assert_eq!(clip.origin, ZoomOrigin::User);
    state.set_selected_zoom_instant(false);
    assert!(!state.selected_zoom_clip().unwrap().instant);
}

#[test]
fn a_new_zoom_inherits_the_last_edited_instant_choice() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 960.0, 540.0);
    let first = state.add_zoom_at(0.5).expect("first zoom fits");
    state.selected_zoom = Some(first);
    state.set_selected_zoom_instant(true);
    let second = state.add_zoom_at(5.0).expect("second zoom fits");
    assert!(state.zoom_clips[second].instant);
}

#[test]
fn generated_zooms_open_animated_despite_an_instant_style() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(1.0, 400.0, 300.0)]);
    let first = state.add_zoom_at(5.0).expect("zoom fits");
    state.selected_zoom = Some(first);
    state.set_selected_zoom_instant(true);
    assert_eq!(state.suggest_zoom_clips(), 1);
    let generated = state
        .zoom_clips
        .iter()
        .find(|clip| clip.origin == ZoomOrigin::Generated)
        .expect("a generated zoom lands");
    assert!(!generated.instant);
}

#[test]
fn reset_zoom_animation_clears_the_instant_snap() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 960.0, 540.0);
    let index = state.add_zoom_at(0.5).expect("zoom fits");
    state.selected_zoom = Some(index);
    state.set_selected_zoom_instant(true);
    state.reset_zoom_animation();
    assert!(!state.selected_zoom_clip().unwrap().instant);
}

#[test]
fn eval_zoom_at_snaps_an_instant_zoom_to_the_movement_group_centre() {
    let mut state = VideoEditState::new(metadata());
    // A pointer parked at the right edge is the whole movement group, so an
    // instant zoom snaps to it (clamped into the crop) while the animated
    // spring is still travelling from the stored centre early in the clip.
    attach_pointer(&mut state, 1900.0, 540.0);
    let index = state.add_zoom_at(0.5).expect("zoom fits");
    state.selected_zoom = Some(index);
    state.zoom_clips[index].center = (960.0, 540.0);
    state.zoom_clips[index].scale = 2.0;
    let timeline_t = state.zoom_clips[index].start + 0.05;
    let source_t = state.timeline_to_source(timeline_t);
    let (_, animated) = state.eval_zoom_at(timeline_t, source_t);
    assert!(
        animated.0 > 960.0 && animated.0 < 1200.0,
        "the animated spring should still be travelling, got {animated:?}"
    );
    state.set_selected_zoom_instant(true);
    let (scale, snapped) = state.eval_zoom_at(timeline_t, source_t);
    assert!((scale - 2.0).abs() < 1e-9);
    assert!((snapped.0 - 1440.0).abs() < 1e-6);
    assert!((snapped.1 - 540.0).abs() < 1e-6);
}
