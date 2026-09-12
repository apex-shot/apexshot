#[cfg(test)]
mod tests {
    use super::{
        draw_motion_backdrop, draw_motion_foreground, draw_motion_frame, motion_pose_differs,
        motion_text_contains_view_point, paint_card_shadow, paint_image_background,
        view_point_to_motion_text_position, CardLayout, MotionStage,
    };
    use crate::recording::editor::model::{
        project_card_corners, MotionBackgroundFillType, MotionState, MotionTransform,
        DEFAULT_MOTION_ZOOM,
    };
    use gtk4::cairo::{Context, Format, ImageSurface};

    /// Frame presets re-fit the scene into the largest centered rectangle of
    /// the target aspect; Standard (None) keeps the bounds untouched.
    #[test]
    fn frame_preset_refits_the_stage_aspect() {
        let (square_w, square_h) = super::fit_stage_aspect(100.0, 50.0, Some(1.0));
        assert_eq!((square_w, square_h), (50.0, 50.0));
        let (wide_w, wide_h) = super::fit_stage_aspect(50.0, 100.0, Some(16.0 / 9.0));
        assert!((wide_w / wide_h - 16.0 / 9.0).abs() < 1e-9);
        assert!(wide_w <= 50.0 && wide_h <= 100.0);
        assert_eq!(super::fit_stage_aspect(100.0, 50.0, None), (100.0, 50.0));
    }

    /// Shotbase starts Motion with an empty track; tests add their own clip.
    fn motion_with_first_clip() -> MotionState {
        let mut motion = MotionState::default();
        motion
            .add_segment_at(0.0)
            .expect("a fresh track accepts a first move");
        motion
    }

    /// Export-style layout: the full frame with Shotbase's default padding.
    fn frame_layout(
        surface: &ImageSurface,
        transform: MotionTransform,
        zoom_anchor: (f64, f64),
    ) -> CardLayout {
        CardLayout::with_padding(
            surface,
            MotionStage::frame(1440.0, 900.0),
            transform,
            zoom_anchor,
            96.0,
        )
    }

    fn render_appearance_frame(
        card: &ImageSurface,
        motion: &MotionState,
        preview: bool,
    ) -> ImageSurface {
        let frame = ImageSurface::create(Format::ARgb32, 128, 96).unwrap();
        {
            let context = Context::new(&frame).unwrap();
            draw_motion_frame(
                &context, 128, 96, card, motion, None, None, 0.0, preview, true, false, 1.0,
            );
        }
        frame.flush();
        frame
    }

    #[test]
    fn scene_fill_is_bounded_in_preview_and_full_frame_on_export() {
        let card = ImageSurface::create(Format::ARgb32, 8, 8).unwrap();
        let mut motion = MotionState::default();
        let (scene_x, scene_y, scene_w, scene_h) = super::motion_scene_bounds(128.0, 96.0);
        let center_x = (scene_x + scene_w * 0.5).floor() as usize;
        let center_y = (scene_y + scene_h * 0.5).floor() as usize;
        let center = center_y * 128 * 4 + center_x * 4;
        let corner_x = scene_x.ceil() as usize + 1;
        let corner_y = scene_y.ceil() as usize + 1;
        let scene_corner = corner_y * 128 * 4 + corner_x * 4;

        // The preview keeps the checkerboard canvas until a fill is chosen.
        let mut frame = render_appearance_frame(&card, &motion, true);
        let data = frame.data().unwrap();
        assert_ne!(&data[..4], &[0, 0, 0, 255]);

        // A chosen fill paints a square bounded scene panel: its center and
        // corners take the color while the outer canvas stays checkerboard.
        motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
        motion.appearance.background_color = [0.2, 0.4, 0.6, 1.0];
        let mut frame = render_appearance_frame(&card, &motion, true);
        let data = frame.data().unwrap();
        // Cairo ARgb32 is BGRA on the Linux targets we support.
        assert_eq!(&data[center..center + 4], &[153, 102, 51, 255]);
        assert_eq!(&data[scene_corner..scene_corner + 4], &[153, 102, 51, 255]);
        assert_ne!(&data[..4], &[153, 102, 51, 255]);

        // Exports have no editor canvas: the fill covers the whole frame and
        // an unset fill is Shotbase's black scene. The radius belongs to the
        // card, so the background corners stay filled regardless of it.
        motion.appearance.background_fill_type = MotionBackgroundFillType::None;
        motion.appearance.border_radius = 40.0;
        let mut frame = render_appearance_frame(&card, &motion, false);
        let data = frame.data().unwrap();
        assert_eq!(&data[..4], &[0, 0, 0, 255]);
        assert_eq!(&data[center..center + 4], &[0, 0, 0, 255]);
    }

    #[test]
    fn transformed_motion_composition_stays_inside_the_background() {
        let blank_card = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        let white_card = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&white_card).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().ok();
        }
        white_card.flush();

        let mut motion = motion_with_first_clip();
        motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
        motion.appearance.background_color = [0.2, 0.4, 0.6, 1.0];
        motion.appearance.background_padding = 0.0;
        motion.set_selected_transition_ms(0);
        motion.set_selected_end_scale(4.0);

        let mut baseline = render_appearance_frame(&blank_card, &motion, true);
        let baseline_data = baseline.data().unwrap();
        let stage = MotionStage::preview(128.0, 96.0, None);
        let scene_center_x = stage.center_x.floor() as usize;
        let scene_center_y = stage.center_y.floor() as usize;
        let outside_scene = scene_center_y * 128 * 4 + 10 * 4;
        let expected_canvas_pixel = baseline_data[outside_scene..outside_scene + 4].to_vec();
        drop(baseline_data);

        let mut frame = render_appearance_frame(&white_card, &motion, true);
        let data = frame.data().unwrap();
        let scene_center = scene_center_y * 128 * 4 + scene_center_x * 4;
        assert_eq!(
            &data[outside_scene..outside_scene + 4],
            expected_canvas_pixel.as_slice()
        );
        assert_eq!(&data[scene_center..scene_center + 4], &[255, 255, 255, 255]);
    }

    #[test]
    fn border_radius_rounds_the_captured_card_not_the_background() {
        let card = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().ok();
        }
        card.flush();
        let mut motion = MotionState::default();
        motion.appearance.background_padding = 0.0;
        // Black export scene behind an opaque white card: the card fills the
        // middle of the frame, so its corner pixels are directly observable.
        motion.appearance.background_fill_type = MotionBackgroundFillType::None;
        motion.appearance.background_color = [0.0, 0.0, 0.0, 1.0];
        let card_center = 48 * 128 * 4 + 64 * 4;
        let card_corner = 17 * 128 * 4 + 33 * 4;

        // Square card: the image reaches into its own corners.
        motion.appearance.border_radius = 0.0;
        let mut frame = render_appearance_frame(&card, &motion, false);
        let data = frame.data().unwrap();
        assert_eq!(&data[card_corner..card_corner + 4], &[255, 255, 255, 255]);

        // A radius of half the card side rounds the corners away entirely:
        // the image corner is cut and the background shows through, while the
        // card center stays image.
        motion.appearance.border_radius = 32.0;
        let mut frame = render_appearance_frame(&card, &motion, false);
        let data = frame.data().unwrap();
        assert_eq!(&data[card_corner..card_corner + 4], &[0, 0, 0, 255]);
        assert_eq!(&data[card_center..card_center + 4], &[255, 255, 255, 255]);
    }

    #[test]
    fn card_shadow_has_a_smooth_falloff_and_stays_inside_the_scene() {
        let mut frame = ImageSurface::create(Format::ARgb32, 160, 120).unwrap();
        let stage = MotionStage {
            bounds_w: 120.0,
            bounds_h: 80.0,
            center_x: 80.0,
            center_y: 60.0,
        };
        let corners = [(50.0, 40.0), (110.0, 40.0), (110.0, 80.0), (50.0, 80.0)];
        let mut appearance = MotionState::default().appearance;
        appearance.shadow_opacity = 1.0;
        appearance.shadow_blur = 20.0;
        appearance.shadow_position = (0.0, 0.0);
        {
            let context = Context::new(&frame).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
            paint_card_shadow(&context, stage, corners, &appearance, 0.0);
        }
        frame.flush();
        let data = frame.data().unwrap();
        let channel = |x: usize, y: usize| data[(y * 160 + x) * 4];

        assert_eq!(channel(10, 60), 255, "shadow escaped the scene bounds");
        assert!(channel(80, 60) < channel(46, 60));
        assert!(channel(46, 60) < channel(30, 60));

        let mut falloff = (25..50).map(|x| channel(x, 60)).collect::<Vec<_>>();
        falloff.sort_unstable();
        falloff.dedup();
        assert!(falloff.len() > 12, "shadow edge is visibly stepped");
    }

    #[test]
    fn zero_blur_keeps_a_hard_offset_shadow() {
        let mut frame = ImageSurface::create(Format::ARgb32, 100, 80).unwrap();
        let stage = MotionStage::frame(100.0, 80.0);
        let corners = [(30.0, 20.0), (70.0, 20.0), (70.0, 60.0), (30.0, 60.0)];
        let mut appearance = MotionState::default().appearance;
        appearance.shadow_opacity = 1.0;
        appearance.shadow_blur = 0.0;
        appearance.shadow_position = (8.0, 8.0);
        {
            let context = Context::new(&frame).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
            paint_card_shadow(&context, stage, corners, &appearance, 0.0);
        }
        frame.flush();
        let data = frame.data().unwrap();
        let channel = |x: usize, y: usize| data[(y * 100 + x) * 4];
        assert_eq!(channel(75, 40), 0);
        assert_eq!(channel(25, 40), 255);
    }

    #[test]
    fn background_blur_softens_image_edges() {
        let source = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&source).unwrap();
            context.set_source_rgb(0.0, 0.0, 0.0);
            context.rectangle(0.0, 0.0, 32.0, 64.0);
            context.fill().unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.rectangle(32.0, 0.0, 32.0, 64.0);
            context.fill().unwrap();
        }
        source.flush();

        let mut frame = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&frame).unwrap();
            paint_image_background(&context, &source, 0.0, 0.0, 64.0, 64.0, 1.0);
        }
        frame.flush();
        let data = frame.data().unwrap();
        let channel = |x: usize| data[(32 * 64 + x) * 4];
        assert!(channel(28) > 0);
        assert!(channel(28) < channel(36));
        assert!(channel(36) < 255);
    }

    #[test]
    fn scene_shadow_placement_splits_above_and_below_the_card() {
        use crate::recording::editor::model::{MotionSceneShadowPreset, MotionSceneShadowPlacement};

        let card = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
        }
        card.flush();

        let render = |motion: &MotionState| {
            let frame = ImageSurface::create(Format::ARgb32, 128, 96).unwrap();
            {
                let context = Context::new(&frame).unwrap();
                draw_motion_frame(
                    &context, 128, 96, &card, motion, None, None, 0.0, false, true, false, 1.0,
                );
            }
            frame.flush();
            frame
        };

        // A white scene behind a white card: only the shading can darken
        // pixels. Padding 0 centers the 64px card in the 128x96 stage.
        let mut motion = MotionState::default();
        motion.appearance.background_padding = 0.0;
        motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
        motion.appearance.background_color = [1.0, 1.0, 1.0, 1.0];
        motion.appearance.shadow_opacity = 0.0;
        motion.scene_shadow.preset = MotionSceneShadowPreset::Side;
        motion.scene_shadow.opacity = 1.0;

        // Underlay shades the background next to the card but never the
        // card itself.
        motion.scene_shadow.placement = MotionSceneShadowPlacement::Underlay;
        let mut frame = render(&motion);
        let data = frame.data().unwrap();
        let scene_left = (48 * 128 + 8) * 4;
        let card_center = (48 * 128 + 64) * 4;
        assert!(data[scene_left] < 255, "underlay must shade the scene");
        assert_eq!(&data[card_center..card_center + 4], &[255, 255, 255, 255]);

        // Overlay shades both the scene and the card, and the card center
        // now sits under the shadow.
        motion.scene_shadow.placement = MotionSceneShadowPlacement::Overlay;
        let mut frame = render(&motion);
        let data = frame.data().unwrap();
        assert!(data[card_center] < 255, "overlay must shade the card");
    }

    #[test]
    fn watermark_is_a_card_space_layer_in_the_shared_compositor() {
        let card = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        let mark = ImageSurface::create(Format::ARgb32, 8, 8).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
        }
        {
            let context = Context::new(&mark).unwrap();
            context.set_source_rgb(1.0, 0.0, 0.0);
            context.paint().unwrap();
        }
        card.flush();
        mark.flush();

        let mut motion = MotionState::default();
        motion.appearance.background_padding = 0.0;
        motion.watermark.image_file_name = Some("mark.png".into());
        motion.watermark.size = 0.25;
        motion.watermark.inset = 0.0;
        motion.watermark.position = (0.5, 0.5);
        let mut frame = ImageSurface::create(Format::ARgb32, 128, 96).unwrap();
        {
            let context = Context::new(&frame).unwrap();
            draw_motion_frame(
                &context,
                128,
                96,
                &card,
                &motion,
                None,
                Some(&mark),
                0.0,
                false,
                true,
                false,
                1.0,
            );
        }
        frame.flush();
        let data = frame.data().unwrap();
        // Cairo ARgb32 is BGRA on the Linux targets we support. The card
        // center would be white without the selected watermark.
        assert_eq!(
            &data[(48 * 128 + 64) * 4..(48 * 128 + 64) * 4 + 4],
            &[0, 0, 255, 255]
        );
    }

    #[test]
    fn new_segment_starts_identity_and_holds_shotbase_default_zoom() {
        let motion = motion_with_first_clip();
        let start = motion.sample(0.0);
        let end = motion.sample(motion.duration);
        assert!((start.scale - 1.0).abs() < 1e-6);
        assert!(start.rotation_y.abs() < 1e-6);
        assert!((end.scale - DEFAULT_MOTION_ZOOM).abs() < 1e-6);
        assert!(end.rotation_y.abs() < 1e-6);
        assert_eq!(motion.segments.len(), 1);
        assert_eq!(motion.selected, Some(0));
    }

    #[test]
    fn held_pose_does_not_spend_a_motion_blur_sample() {
        let pose = MotionTransform::default();
        assert!(!motion_pose_differs(pose, pose, (0.5, 0.5), (0.5, 0.5)));
        assert!(motion_pose_differs(
            MotionTransform {
                scale: 1.001,
                ..pose
            },
            pose,
            (0.5, 0.5),
            (0.5, 0.5),
        ));
        assert!(motion_pose_differs(pose, pose, (0.51, 0.5), (0.5, 0.5)));
    }

    #[test]
    fn transition_playback_uses_the_same_card_mesh_as_the_still_preview() {
        let card = ImageSurface::create(Format::ARgb32, 96, 64).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
            context.set_source_rgb(0.0, 0.0, 0.0);
            for y in 0..8 {
                for x in 0..12 {
                    if (x + y) % 2 == 0 {
                        context.rectangle((x * 8) as f64, (y * 8) as f64, 8.0, 8.0);
                    }
                }
            }
            context.fill().unwrap();
        }
        card.flush();

        let motion = motion_with_first_clip();
        let time = 0.3;
        let render = |live_preview| {
            let frame = ImageSurface::create(Format::ARgb32, 192, 128).unwrap();
            {
                let context = Context::new(&frame).unwrap();
                draw_motion_frame(
                    &context,
                    192,
                    128,
                    &card,
                    &motion,
                    None,
                    None,
                    time,
                    true,
                    true,
                    live_preview,
                    1.0,
                );
            }
            frame.flush();
            frame
        };

        let mut still = render(false);
        let mut playing = render(true);
        let still_pixels = still.data().unwrap().to_vec();
        let playing_pixels = playing.data().unwrap().to_vec();
        assert_eq!(
            still_pixels,
            playing_pixels,
            "playing an Ease preview must not change the perspective mesh"
        );
    }

    #[test]
    fn cached_backdrop_composition_matches_a_direct_preview_frame() {
        let card = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(0.15, 0.45, 0.85);
            context.paint().unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.rectangle(16.0, 16.0, 32.0, 32.0);
            context.fill().unwrap();
        }
        card.flush();

        let mut motion = motion_with_first_clip();
        motion.appearance.background_fill_type = MotionBackgroundFillType::Gradient;
        motion.appearance.gradient_color_1 = [0.08, 0.12, 0.22, 1.0];
        motion.appearance.gradient_color_2 = [0.42, 0.18, 0.54, 1.0];
        motion.appearance.background_noise = 0.4;
        let time = 0.35;

        let direct = ImageSurface::create(Format::ARgb32, 160, 120).unwrap();
        {
            let context = Context::new(&direct).unwrap();
            draw_motion_frame(
                &context, 160, 120, &card, &motion, None, None, time, true, true, true, 1.0,
            );
        }
        direct.flush();

        let backdrop = ImageSurface::create(Format::ARgb32, 160, 120).unwrap();
        {
            let context = Context::new(&backdrop).unwrap();
            draw_motion_backdrop(&context, 160, 120, &motion, None, true, true);
        }
        backdrop.flush();

        let cached = ImageSurface::create(Format::ARgb32, 160, 120).unwrap();
        {
            let context = Context::new(&cached).unwrap();
            context.set_source_surface(&backdrop, 0.0, 0.0).unwrap();
            context.paint().unwrap();
            draw_motion_foreground(
                &context, 160, 120, &card, &motion, None, time, true, true, 1.0,
            );
        }
        cached.flush();

        let mut direct = direct;
        let mut cached = cached;
        assert_eq!(
            direct.data().unwrap().to_vec(),
            cached.data().unwrap().to_vec(),
            "the cached backdrop path must preserve the direct preview pixels"
        );
    }

    #[test]
    fn stretching_duration_holds_end_pose_after_the_move() {
        let mut motion = motion_with_first_clip();
        let move_end = motion.segments[0].end;
        motion.set_duration(6.0);
        assert!((motion.segments[0].end - move_end).abs() < 1e-6);
        let end = motion.sample(6.0);
        assert!((end.scale - DEFAULT_MOTION_ZOOM).abs() < 1e-6);
    }

    #[test]
    fn effect_segments_inherit_the_camera_pose_before_them() {
        let mut motion = motion_with_first_clip();
        motion.set_duration(6.0);
        motion
            .add_segment_at(2.0)
            .expect("a second non-overlapping move");

        let first_end = motion.segments[0].to;
        assert_eq!(motion.segments[1].from, first_end);
        assert_eq!(
            motion.sample(2.0),
            MotionTransform {
                perspective: motion.perspective_intensity,
                ..first_end
            }
        );

        motion.selected = Some(0);
        motion.set_selected_end_scale(1.5);
        assert!((motion.segments[1].from.scale - 1.5).abs() < 1e-6);
    }

    #[test]
    fn timeline_snap_targets_include_the_playhead_and_track_boundaries() {
        let mut motion = motion_with_first_clip();
        motion.set_duration(6.0);
        motion.playhead = 2.5;
        motion
            .add_text_at(3.0)
            .expect("a text segment after the seed move");

        assert!((motion.snap_effect_time(2.47, 0.05, None) - 2.5).abs() < 1e-6);
        assert!((motion.snap_text_time(2.96, 0.05, None) - 3.0).abs() < 1e-6);
        assert!((motion.snap_effect_time(5.98, 0.05, None) - 6.0).abs() < 1e-6);
    }

    #[test]
    fn card_placement_round_trips_through_a_tilted_projection() {
        let surface = ImageSurface::create(Format::ARgb32, 1200, 675).expect("surface");
        let transform = MotionTransform {
            scale: 1.18,
            rotation_x: -9.0,
            rotation_y: 14.0,
            rotation_z: 3.0,
            perspective: 0.24,
            pos_x: 0.18,
            pos_y: -0.12,
        };
        let zoom_anchor = (0.22, 0.78);
        let layout = frame_layout(&surface, transform, zoom_anchor);
        let expected = (0.27, 0.71);
        let point = layout.project(expected.0 * layout.img_w, expected.1 * layout.img_h);
        let actual = view_point_to_motion_text_position(
            &surface,
            MotionStage::frame(1440.0, 900.0),
            96.0,
            transform,
            zoom_anchor,
            point.0,
            point.1,
        );
        assert!((actual.0 - expected.0).abs() < 0.003, "x: {actual:?}");
        assert!((actual.1 - expected.1).abs() < 0.003, "y: {actual:?}");
    }

    #[test]
    fn title_hit_test_rejects_empty_preview_space() {
        let surface = ImageSurface::create(Format::ARgb32, 1200, 675).expect("surface");
        let mut motion = MotionState::default();
        let index = motion.add_text_at(0.0).expect("title clip");
        let segment = &motion.text_segments[index];
        let transform = MotionTransform {
            scale: 1.12,
            rotation_x: -7.0,
            rotation_y: 10.0,
            perspective: 0.18,
            ..MotionTransform::default()
        };
        let zoom_anchor = (0.32, 0.68);
        let layout = frame_layout(&surface, transform, zoom_anchor);
        let title_center =
            layout.project(segment.pos_x * layout.img_w, segment.pos_y * layout.img_h);

        assert!(motion_text_contains_view_point(
            &surface,
            MotionStage::frame(1440.0, 900.0),
            96.0,
            transform,
            zoom_anchor,
            segment,
            0.5,
            title_center.0,
            title_center.1,
        ));
        assert!(!motion_text_contains_view_point(
            &surface,
            MotionStage::frame(1440.0, 900.0),
            96.0,
            transform,
            zoom_anchor,
            segment,
            0.5,
            4.0,
            4.0,
        ));
    }

    #[test]
    fn shotbase_transform_timing_defaults_drive_glide_motion() {
        let mut motion = motion_with_first_clip();
        // Keep the clip longer than the 1.2s transition so the timing curve
        // is not clamped by the default one-second clip.
        motion.set_segment_range(0, 0.0, 2.0);
        let timing = motion.transform_timing;
        assert!((timing.transition_duration - 1.2).abs() < f64::EPSILON);
        assert!((timing.easing_x1 - 0.25).abs() < f64::EPSILON);
        assert!((timing.easing_y1 - 1.0).abs() < f64::EPSILON);
        assert!((timing.easing_x2 - 0.50).abs() < f64::EPSILON);
        assert!((timing.easing_y2 - 1.0).abs() < f64::EPSILON);

        let default_progress = motion.sample(0.6).scale;
        motion.set_transform_timing(
            crate::recording::editor::model::MotionEffectTransformTiming {
                transition_duration: 1.2,
                easing_x1: 0.0,
                easing_y1: 0.0,
                easing_x2: 1.0,
                easing_y2: 1.0,
            },
        );
        let linear_progress = motion.sample(0.6).scale;
        assert!(
            default_progress > linear_progress + 0.01,
            "Shotbase's recovered Bézier should advance ahead of linear at mid-transition"
        );
        // linear progress at t=0.6 of a 1.2s transition is 0.5: scale 1 + (2-1)*0.5
        assert!((linear_progress - 1.5).abs() < 0.002);
    }

    #[test]
    fn transition_ms_slider_drives_the_global_timing() {
        let mut motion = motion_with_first_clip();
        motion.set_selected_transition_ms(300);
        assert!((motion.transform_timing.transition_duration - 0.3).abs() < 1e-9);
        // Inside the shortened window the move has already finished.
        let held = motion.sample(0.35);
        assert!((held.scale - DEFAULT_MOTION_ZOOM).abs() < 1e-6);
        // A zero-duration transition jumps straight to the target pose.
        motion.set_selected_transition_ms(0);
        assert!((motion.sample(0.0).scale - DEFAULT_MOTION_ZOOM).abs() < 1e-6);
    }

    #[test]
    fn blur_and_position_setters_stick() {
        let mut motion = motion_with_first_clip();
        motion.set_motion_blur(0.4);
        motion.set_selected_end_pos_x(0.5);
        motion.set_selected_end_pos_y(-0.25);
        assert!((motion.motion_blur - 0.4).abs() < 1e-6);
        assert!(motion.motion_blur_settings.enabled);
        assert!((motion.effective_motion_blur() - 0.4).abs() < 1e-6);
        let end = motion.sample(motion.duration);
        assert!((end.pos_x - 0.5).abs() < 1e-6);
        assert!((end.pos_y + 0.25).abs() < 1e-6);
        let start = motion.sample(0.0);
        assert!(start.pos_x.abs() < 1e-6);
        assert!(start.pos_y.abs() < 1e-6);
    }

    #[test]
    fn recovered_disabled_effect_and_text_fields_are_no_ops() {
        let mut motion = motion_with_first_clip();
        motion.set_selected_perspective(0.0);
        motion.set_selected_disabled(true);
        assert_eq!(motion.sample(0.4), MotionTransform::default());
        assert_eq!(motion.sample(motion.duration), MotionTransform::default());

        let text = motion.add_text_at(0.05).expect("text clip");
        motion.set_selected_text_scope(crate::recording::editor::model::MotionTextScope::Word);
        motion.set_selected_text_typewriter_time(0.25);
        motion.set_selected_text_disabled(true);
        let segment = &motion.text_segments[text];
        assert_eq!(
            segment.scope,
            crate::recording::editor::model::MotionTextScope::Word
        );
        assert!((segment.typewriter_time - 0.25).abs() < f64::EPSILON);
        assert!(segment.sample(0.1).is_none());
    }

    #[test]
    fn recovered_perspective_intensity_is_global_across_effect_segments() {
        let mut motion = motion_with_first_clip();
        motion.set_selected_perspective(0.42);
        motion.add_segment_at(3.0).expect("second effect clip");

        assert_eq!(motion.segments.len(), 2);
        assert!((motion.sample(0.25).perspective - 0.42).abs() < f64::EPSILON);
        assert!((motion.sample(3.25).perspective - 0.42).abs() < f64::EPSILON);
        assert!((motion.sample(motion.duration).perspective - 0.42).abs() < f64::EPSILON);
    }

    #[test]
    fn recovered_segment_intensity_blends_the_camera_target() {
        let mut motion = motion_with_first_clip();
        motion.set_selected_perspective(0.0);
        motion.set_selected_intensity(0.0);

        assert_eq!(motion.sample(motion.duration), MotionTransform::default());

        motion.set_selected_intensity(0.5);
        let half = motion.sample(motion.duration);
        assert!((half.scale - 1.5).abs() < 1e-6);
        assert!(half.rotation_y.abs() < 1e-6);
    }

    #[test]
    fn recovered_zoom_anchor_holds_its_source_point_during_scale() {
        let surface = ImageSurface::create(Format::ARgb32, 1200, 675).expect("surface");
        let zoom_anchor = (0.2, 0.78);
        let transform = MotionTransform {
            scale: 1.32,
            rotation_x: -7.0,
            rotation_y: 12.0,
            rotation_z: 2.0,
            perspective: 0.24,
            pos_x: 0.1,
            pos_y: -0.08,
        };
        let mut unzoomed = transform;
        unzoomed.scale = 1.0;
        let before = frame_layout(&surface, unzoomed, (0.5, 0.5));
        let after = frame_layout(&surface, transform, zoom_anchor);
        let source_x = zoom_anchor.0 * before.img_w;
        let source_y = zoom_anchor.1 * before.img_h;
        let expected = before.project(source_x, source_y);
        let actual = after.project(source_x, source_y);
        assert!(
            (expected.0 - actual.0).abs() < 1e-6,
            "x: {expected:?} {actual:?}"
        );
        assert!(
            (expected.1 - actual.1).abs() < 1e-6,
            "y: {expected:?} {actual:?}"
        );
    }

    #[test]
    fn position_extremes_map_the_card_center_to_the_stage_edges() {
        let surface = ImageSurface::create(Format::ARgb32, 1600, 900).expect("surface");
        let layout = frame_layout(
            &surface,
            MotionTransform {
                scale: 2.0,
                pos_x: 1.0,
                pos_y: 1.0,
                ..MotionTransform::default()
            },
            (0.5, 0.5),
        );

        assert!((layout.cx - 1440.0).abs() < 1e-6);
        assert!((layout.cy - 900.0).abs() < 1e-6);
    }

    #[test]
    fn text_effect_presets_and_typewriter_scope_are_applied() {
        let mut motion = motion_with_first_clip();
        let first = motion.add_text_at(0.05).expect("text clip");
        assert_eq!(motion.text_segments.len(), 1);
        assert_eq!(motion.selected_text, Some(first));
        assert!(motion.selected.is_none());
        assert!(motion.add_text_at(0.1).is_none());
        let none = motion.text_segments[0].sample(0.12).expect("none sample");
        motion.set_selected_text_animation(
            crate::recording::editor::model::MotionTextAnimation::SlideBottom,
        );
        let slide = motion.text_segments[0].sample(0.12).expect("slide sample");
        assert!(slide.offset_y > none.offset_y + 4.0);
        assert!(slide.alpha > 0.0 && slide.alpha < 1.0);
        motion.set_selected_text_animation(
            crate::recording::editor::model::MotionTextAnimation::Typewriter,
        );
        let typewriter = motion.text_segments[0]
            .sample(0.12)
            .expect("typewriter sample");
        assert!(typewriter.reveal > 0.0 && typewriter.reveal < 1.0);
        assert!((motion.text_segments[0].pos_x - 0.5).abs() < 1e-6);
        motion.set_selected_text_pos(0.2, 0.3);
        motion.set_selected_text_size(1.6);
        assert!((motion.text_segments[0].pos_x - 0.2).abs() < 1e-6);
        assert!((motion.text_segments[0].pos_y - 0.3).abs() < 1e-6);
        assert!((motion.text_segments[0].size - 1.6).abs() < 1e-6);
        assert!(motion.remove_selected());
        assert!(motion.text_segments.is_empty());
        assert!(motion.has_segments());
    }

    #[test]
    fn add_segment_fills_a_gap_and_rejects_overlap() {
        let mut motion = MotionState::default();
        let first = motion.add_segment_at(0.0).expect("first clip");
        assert!(
            (motion.segments[first].end - motion.segments[first].start - 1.0).abs() < f64::EPSILON,
            "new clips are one second long"
        );
        assert!(motion.add_segment_at(2.0).is_some(), "second clip in the gap");
        assert_eq!(motion.segments.len(), 2);

        // The one-second gap takes a full clip even when the pointer is late
        // in the gap: it is fitted flush against the following clip.
        let fitted = motion.add_segment_at(1.6).expect("the gap fits a clip");
        assert!((motion.segments[fitted].start - 1.0).abs() < 1e-9);
        assert!((motion.segments[fitted].end - 2.0).abs() < 1e-9);

        // Clicking inside an existing clip never adds.
        assert!(motion.add_segment_at(0.2).is_none());
        motion.selected = Some(fitted);
        assert!(motion.remove_selected());
        assert_eq!(motion.segments.len(), 2);
    }

    #[test]
    fn add_segment_rejects_gaps_shorter_than_a_clip() {
        let mut motion = MotionState::default();
        motion.add_segment_at(0.0).expect("first clip");
        motion.add_segment_at(1.5).expect("clip leaving a 0.5s gap");
        assert!(motion.add_segment_at(1.1).is_none());
    }

    #[test]
    fn identity_projection_keeps_the_card_axis_aligned() {
        let transform = MotionTransform::default();
        let corners = project_card_corners(400.0, 200.0, 1.0, transform, 500.0, 300.0);
        let width = corners[1].0 - corners[0].0;
        let height = corners[3].1 - corners[0].1;
        assert!((width - 400.0).abs() < 1.0);
        assert!((height - 200.0).abs() < 1.0);
        assert!((corners[0].1 - corners[1].1).abs() < 0.5);
        assert!((corners[0].0 - corners[3].0).abs() < 0.5);
    }

    #[test]
    fn yaw_projection_stays_a_single_quad_not_a_fan() {
        let transform = MotionTransform {
            scale: 1.12,
            rotation_y: 8.0,
            perspective: 0.18,
            ..MotionTransform::default()
        };
        let corners = project_card_corners(800.0, 450.0, 1.0, transform, 960.0, 540.0);
        for (x, y) in corners {
            assert!(x.is_finite() && y.is_finite(), "corner exploded to {x},{y}");
            assert!(x > 200.0 && x < 1720.0, "corner x {x} left the viewport");
            assert!(y > 100.0 && y < 980.0, "corner y {y} left the viewport");
        }
        let top_w = corners[1].0 - corners[0].0;
        let bot_w = corners[2].0 - corners[3].0;
        assert!(top_w > 200.0 && bot_w > 200.0);
        assert!(
            (top_w - bot_w).abs() < top_w * 0.35,
            "quad crossed or shredded: top {top_w} bot {bot_w}"
        );
    }

    #[test]
    fn pitch_quad_is_not_a_parallelogram() {
        let transform = MotionTransform {
            rotation_x: 16.0,
            perspective: 0.22,
            ..MotionTransform::default()
        };
        let corners = project_card_corners(800.0, 450.0, 1.0, transform, 960.0, 540.0);
        for (x, y) in corners {
            assert!(x.is_finite() && y.is_finite());
        }
        let top_w = (corners[1].0 - corners[0].0).abs();
        let bot_w = (corners[2].0 - corners[3].0).abs();
        assert!(
            (top_w - bot_w).abs() > 8.0,
            "pitch should change top vs bottom width, got {top_w} vs {bot_w}"
        );
        let parallelogram = (
            corners[0].0 + (corners[1].0 - corners[0].0) + (corners[3].0 - corners[0].0),
            corners[0].1 + (corners[1].1 - corners[0].1) + (corners[3].1 - corners[0].1),
        );
        let gap = ((parallelogram.0 - corners[2].0).powi(2)
            + (parallelogram.1 - corners[2].1).powi(2))
        .sqrt();
        assert!(
            gap > 8.0,
            "a 3-point affine would miss the fourth pitch corner by only {gap}"
        );
    }

    #[test]
    fn downscaled_preview_card_matches_the_full_resolution_composition() {
        let card = ImageSurface::create(Format::ARgb32, 2560, 1440).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(0.15, 0.45, 0.85);
            context.paint().unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.rectangle(320.0, 180.0, 1920.0, 1080.0);
            context.fill().unwrap();
        }
        card.flush();

        let mut motion = motion_with_first_clip();
        motion.appearance.border_radius = 32.0;
        motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
        motion.appearance.background_color = [0.05, 0.05, 0.05, 1.0];
        let _ = motion.add_text_at(0.0);

        let render = |surface: &ImageSurface, card_scale: f64| {
            let mut frame = ImageSurface::create(Format::ARgb32, 1280, 800).unwrap();
            {
                let context = Context::new(&frame).unwrap();
                draw_motion_frame(
                    &context,
                    1280,
                    800,
                    surface,
                    &motion,
                    None,
                    None,
                    0.35,
                    true,
                    true,
                    false,
                    card_scale,
                );
            }
            frame.flush();
            let pixels = frame.data().unwrap().to_vec();
            pixels
        };

        let full = render(&card, 1.0);
        let (preview, scale) = super::scaled_card_preview(&card).expect("preview texture");
        assert!(scale < 1.0);
        let scaled = render(&preview, scale);

        let mean = full
            .iter()
            .zip(scaled.iter())
            .map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64)
            .sum::<u64>() as f64
            / full.len() as f64;
        assert!(
            mean < 4.0,
            "downscaled preview drifted from the full-resolution composition by {mean} mean channel levels"
        );
    }

    #[test]
    fn affine_from_three_points_maps_source_to_dest() {
        let matrix = super::affine_from_three_points(
            [(0.0, 0.0), (10.0, 0.0), (0.0, 5.0)],
            [(100.0, 200.0), (120.0, 204.0), (98.0, 210.0)],
        )
        .expect("triangle");
        let map = |x: f64, y: f64| {
            (
                matrix.xx() * x + matrix.xy() * y + matrix.x0(),
                matrix.yx() * x + matrix.yy() * y + matrix.y0(),
            )
        };
        let p0 = map(0.0, 0.0);
        let p1 = map(10.0, 0.0);
        let p2 = map(0.0, 5.0);
        assert!((p0.0 - 100.0).abs() < 1e-6 && (p0.1 - 200.0).abs() < 1e-6);
        assert!((p1.0 - 120.0).abs() < 1e-6 && (p1.1 - 204.0).abs() < 1e-6);
        assert!((p2.0 - 98.0).abs() < 1e-6 && (p2.1 - 210.0).abs() < 1e-6);
    }
}
