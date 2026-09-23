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
fn auto_zoom_camera_feathers_edge_following() {
    let center = (960.0, 540.0);
    let inner_right = center.0 + 1920.0 / 2.0 / 2.0 - 1920.0 / 2.0 * 0.22;
    let barely_outside =
        recenter_if_near_edge(center, (inner_right + 1.0, center.1), 2.0, 1920.0, 1080.0);
    assert!(barely_outside.0 > center.0);
    assert!(
        barely_outside.0 - center.0 < 0.01,
        "camera should ease into following instead of matching cursor velocity immediately"
    );

    let farther_outside =
        recenter_if_near_edge(center, (inner_right + 57.6, center.1), 2.0, 1920.0, 1080.0);
    assert!(farther_outside.0 > barely_outside.0);
    assert!(farther_outside.0 - center.0 < 57.6);
}

#[test]
fn auto_zoom_camera_tracks_the_smoothed_cursor_path() {
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
    state.cursor.smooth = 1.0;
    state.cursor.speed = MIN_CURSOR_SPEED;
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
        "camera must not race ahead on the unsmoothed pointer path: {camera_center:?}"
    );
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

fn attach_sidecar_with_landings(state: &mut VideoEditState, landings: &[(f64, f64, f64)]) {
    let mut sidecar = crate::recording::editor::sidecar::PointerSidecar::new(
        0,
        crate::recording::editor::sidecar::CaptureRegion {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        },
    );
    for &(t, x, y) in landings {
        sidecar.pointer.extend([
            crate::recording::editor::sidecar::PointerSample {
                t: t - 0.6,
                x: x - 300.0,
                y: y - 150.0,
                kind: crate::recording::editor::sidecar::CursorKind::Default,
            },
            crate::recording::editor::sidecar::PointerSample {
                t: t - 0.25,
                x: x - 150.0,
                y: y - 75.0,
                kind: crate::recording::editor::sidecar::CursorKind::Default,
            },
            crate::recording::editor::sidecar::PointerSample {
                t,
                x,
                y,
                kind: crate::recording::editor::sidecar::CursorKind::Default,
            },
            crate::recording::editor::sidecar::PointerSample {
                t: t + 0.15,
                x: x + 1.0,
                y,
                kind: crate::recording::editor::sidecar::CursorKind::Default,
            },
            crate::recording::editor::sidecar::PointerSample {
                t: t + 0.5,
                x,
                y: y + 1.0,
                kind: crate::recording::editor::sidecar::CursorKind::Default,
            },
        ]);
    }
    state.sidecar = Some(sidecar);
}

#[test]
fn suggest_zoom_clips_places_regions_at_pointer_landings() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_landings(&mut state, &[(3.0, 960.0, 540.0), (8.0, 960.0, 540.0)]);
    assert_eq!(state.suggest_zoom_clips(), 2);
    assert_eq!(state.zoom_clips.len(), 2);
    let first = &state.zoom_clips[0];
    assert!((first.start - 2.65).abs() < 1e-9);
    assert!((first.end - 4.6).abs() < 1e-9);
    assert!((first.scale - 1.5).abs() < 1e-9);
    assert_eq!(first.mode, ZoomMode::Auto);
    assert!((first.center.0 - 960.0).abs() < 1e-9);
    assert!((first.center.1 - 540.0).abs() < 1e-9);
    assert!((state.zoom_clips[1].start - 7.65).abs() < 1e-9);
    assert!((state.zoom_clips[1].end - 9.6).abs() < 1e-9);
}

#[test]
fn suggest_zoom_clips_skips_regions_overlapping_existing_zooms() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_landings(&mut state, &[(3.0, 960.0, 540.0), (8.0, 960.0, 540.0)]);
    state.add_zoom_at(2.6);
    assert_eq!(state.suggest_zoom_clips(), 1);
    assert_eq!(state.zoom_clips.len(), 2);
    assert!((state.zoom_clips[1].start - 7.65).abs() < 1e-9);
}

#[test]
fn suggest_zoom_clips_keeps_adjacent_click_sessions() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(3.0, 400.0, 300.0), (4.3, 1500.0, 700.0)]);

    assert_eq!(state.suggest_zoom_clips(), 2);
    assert_eq!(state.zoom_clips.len(), 2);
    assert!(state.zoom_clips[0].end <= state.zoom_clips[1].start);
}

#[test]
fn suggest_zoom_clips_refits_click_near_trim_boundary() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(5.9, 800.0, 500.0)]);
    state.set_trim_end(6.0);

    assert_eq!(state.suggest_zoom_clips(), 1);
    assert!((state.zoom_clips[0].start - 4.6).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 6.0).abs() < 1e-9);
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
fn suggest_zoom_clips_skips_landings_outside_kept_segments() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_landings(&mut state, &[(3.0, 960.0, 540.0), (8.0, 960.0, 540.0)]);
    state.set_trim_start(4.0);
    assert_eq!(state.suggest_zoom_clips(), 1);
    assert!((state.zoom_clips[0].start - 3.65).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 5.6).abs() < 1e-9);
}

#[test]
fn suggest_zoom_clips_requires_a_purposeful_landing() {
    let mut state = VideoEditState::new(metadata());
    attach_pointer(&mut state, 960.0, 540.0);
    assert_eq!(state.suggest_zoom_clips(), 0);
    assert!(state.zoom_clips.is_empty());
}

#[test]
fn suggest_zoom_clips_places_regions_on_pointer_landings() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_landings(&mut state, &[(2.0, 960.0, 540.0), (7.0, 1200.0, 600.0)]);
    assert_eq!(state.suggest_zoom_clips(), 2);
    assert!((state.zoom_clips[0].start - 1.65).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 3.6).abs() < 1e-9);
    assert!((state.zoom_clips[0].center.0 - 960.0).abs() < 1.0);
    assert!((state.zoom_clips[1].start - 6.65).abs() < 1e-9);
    assert!((state.zoom_clips[1].scale - 1.5).abs() < 1e-9);
}

#[test]
fn suggest_zoom_clips_rejects_targets_outside_the_editor_crop() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_landings(&mut state, &[(3.0, 1600.0, 700.0)]);
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
fn click_zoom_wins_when_its_region_overlaps_a_landing() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_landings(&mut state, &[(3.0, 400.0, 300.0)]);
    state
        .sidecar
        .as_mut()
        .unwrap()
        .clicks
        .push(crate::recording::editor::sidecar::ClickSample {
            t: 4.0,
            x: 1100.0,
            y: 600.0,
            button: 1,
        });

    assert_eq!(state.suggest_zoom_clips(), 1);
    assert!((state.zoom_clips[0].center.0 - 1100.0).abs() < 1e-9);
    assert!((state.zoom_clips[0].center.1 - 600.0).abs() < 1e-9);
}

#[test]
fn redetect_zoom_clips_replaces_auto_zooms_for_this_video() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_clicks(&mut state, &[(9.4, 941.0, -215.0)]);
    state.add_zoom_at(0.5);
    assert_eq!(state.zoom_clips.len(), 1);
    assert_eq!(state.zoom_clips[0].mode, ZoomMode::Auto);
    attach_sidecar_with_landings(&mut state, &[(4.0, 500.0, 400.0)]);
    assert!(state.redetect_zoom_clips());
    assert_eq!(state.zoom_clips.len(), 1);
    assert!((state.zoom_clips[0].start - 3.65).abs() < 1e-9);
    assert!((state.zoom_clips[0].end - 5.6).abs() < 1e-9);
}

#[test]
fn suggest_zoom_clips_respects_zoom_lock() {
    let mut state = VideoEditState::new(metadata());
    attach_sidecar_with_landings(&mut state, &[(3.0, 960.0, 540.0)]);
    state.zoom_locked = true;
    assert_eq!(state.suggest_zoom_clips(), 0);
    assert!(state.zoom_clips.is_empty());
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
    settings.trail = 0.12;
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
    attach_sidecar_with_landings(&mut state, &[(3.0, 960.0, 540.0)]);
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
    attach_sidecar_with_landings(&mut state, &[(3.0, 960.0, 540.0)]);
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
fn freeze_playhead_holds_the_last_source_frame() {
    let mut state = VideoEditState::new(metadata());
    state.extend_last_segment(1.0);
    state.playhead_seconds = 10.5;
    // Past the source end the player seeks the last real frame and stops
    // there, so the preview shows the held frame instead of running out.
    assert!((state.source_playhead() - 10.0).abs() < 1e-6);
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
