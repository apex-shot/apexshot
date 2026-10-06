#[cfg(test)]
mod tests {
    use super::{
        draw_motion_backdrop, draw_motion_foreground, draw_motion_frame, draw_transformed_card,
        motion_text_contains_view_point, motion_text_revealed_bytes, paint_card_shadow,
        paint_image_background, view_point_to_motion_text_position, CardLayout, MotionStage,
    };
    use crate::capture::editor::composition::{
        motion_background_composition, BackgroundComposition, CompositionLayout,
    };
    use crate::capture::editor::types::{
        BackgroundAlignment, BackgroundStyle, CropAspectRatio, DrawColor, FrameStyle,
    };
    use crate::recording::editor::model::{
        project_card_corners, MotionAppearance, MotionBackgroundFillType,
        MotionEffectTransformTiming, MotionFrame, MotionState, MotionTransform,
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

    /// The appearance the tilted-frame probes lay out with. Corner radius is
    /// not part of the composition, so a probe and the rendered card share
    /// geometry at any radius.
    fn probe_appearance() -> MotionAppearance {
        MotionAppearance {
            background_padding: 0.0,
            frame_style: FrameStyle::Border,
            border_thickness: 6.0,
            ..MotionAppearance::default()
        }
    }

    /// Motion starts with an empty track; tests add their own clip.
    fn motion_with_first_clip() -> MotionState {
        let mut motion = MotionState::default();
        motion
            .add_segment_at(0.0)
            .expect("a fresh track accepts a first move");
        motion
    }

    fn assert_perspective_mesh_has_no_internal_seams(source_alpha: f64, opacity: f64) {
        let card = ImageSurface::create(Format::ARgb32, 512, 288).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgba(1.0, 1.0, 1.0, source_alpha);
            context.paint().unwrap();
        }
        let transform = MotionTransform {
            rotation_x: 8.0,
            rotation_y: 16.0,
            rotation_z: 3.5,
            perspective: 0.36,
            ..MotionTransform::default()
        };
        let appearance = MotionAppearance::default();
        let layout = card_layout(
            &card, 1.0, MotionStage::frame(640.0, 360.0), &appearance,
            None, transform, (0.5, 0.5),
        );
        let mut frame = ImageSurface::create(Format::ARgb32, 640, 360).unwrap();
        {
            let context = Context::new(&frame).unwrap();
            draw_transformed_card(&context, &card, layout, &appearance, opacity, 8, gtk4::cairo::Filter::Good);
        }
        frame.flush();
        let inner = [(8.0, 8.0), (504.0, 8.0), (504.0, 280.0), (8.0, 280.0)]
            .map(|(x, y)| layout.project(x, y));
        let expected = (source_alpha * opacity * 255.0).round() as u8;
        let stride = frame.stride() as usize;
        let data = frame.data().unwrap();
        let mut mismatches = 0;
        let mut interior = 0;
        let mut edge_pixels = 0;
        for y in 0..360usize {
            for x in 0..640usize {
                let alpha = data[y * stride + x * 4 + 3];
                if source_alpha == 1.0 && opacity == 1.0 && alpha > 0 && alpha < 255 {
                    edge_pixels += 1;
                }
                let point = (x as f64 + 0.5, y as f64 + 0.5);
                let inside = (0..4).all(|index| {
                    let (a, b) = (inner[index], inner[(index + 1) % 4]);
                    (b.0 - a.0) * (point.1 - a.1) - (b.1 - a.1) * (point.0 - a.0) >= 0.0
                });
                if inside {
                    interior += 1;
                    if alpha.abs_diff(expected) > 2 {
                        mismatches += 1;
                    }
                }
            }
        }
        assert!(interior > 10000);
        assert_eq!(mismatches, 0, "mesh seams affect {mismatches}/{interior} interior pixels");
        if source_alpha == 1.0 && opacity == 1.0 {
            assert!(edge_pixels > 0, "the exterior must remain antialiased");
        }
    }

    #[test]
    fn opaque_perspective_mesh_has_no_internal_seams() {
        assert_perspective_mesh_has_no_internal_seams(1.0, 1.0);
    }

    #[test]
    fn transparent_perspective_mesh_applies_opacity_once() {
        assert_perspective_mesh_has_no_internal_seams(0.5, 0.7);
    }

    #[test]
    fn motion_disabled_clip_releases_the_camera_like_an_empty_gap() {
        let mut motion = motion_with_first_clip();
        motion.set_selected_perspective(0.0);
        motion.set_selected_transition_ms(1000);
        motion.add_segment_at(1.0).unwrap();
        motion.set_selected_disabled(true);
        let mut gap = motion.clone();
        gap.selected = Some(1);
        gap.remove_selected();

        for time in [1.01, 1.25, 1.5, 1.99, 2.01] {
            assert_eq!(motion.sample(time), gap.sample(time), "time {time}");
        }
    }

    #[test]
    fn motion_enabled_clip_after_a_disabled_clip_starts_from_identity() {
        let mut motion = motion_with_first_clip();
        motion.set_selected_perspective(0.0);
        motion.add_segment_at(1.0).unwrap();
        motion.add_segment_at(2.0).unwrap();
        motion.selected = Some(1);
        motion.set_selected_disabled(true);

        assert_eq!(motion.segments[2].from, MotionTransform::default());
        assert_eq!(motion.sample(2.0), MotionTransform::default());
    }

    #[test]
    fn motion_adjacent_zoom_anchors_keep_the_rendered_card_continuous() {
        let card = ImageSurface::create(Format::ARgb32, 200, 100).unwrap();
        let mut motion = motion_with_first_clip();
        motion.set_selected_perspective(0.0);
        motion.set_selected_zoom_anchor(0.15, 0.25);
        motion.add_segment_at(1.0).unwrap();
        motion.set_selected_zoom_anchor(0.85, 0.75);
        let before = frame_layout(&card, motion.sample(1.0), motion.zoom_anchor_at(1.0));
        let after = frame_layout(
            &card,
            motion.sample(1.0 + 1e-7),
            motion.zoom_anchor_at(1.0 + 1e-7),
        );

        assert!((before.cx - after.cx).abs() < 0.01);
        assert!((before.cy - after.cy).abs() < 0.01);
        assert_eq!(motion.zoom_anchor_at(2.0), (0.85, 0.75));
    }

    #[test]
    fn motion_zero_intensity_preserves_the_previous_camera_anchor() {
        let mut motion = motion_with_first_clip();
        motion.set_selected_perspective(0.0);
        motion.set_selected_zoom_anchor(0.15, 0.25);
        motion.add_segment_at(1.0).unwrap();
        motion.set_selected_zoom_anchor(0.85, 0.75);
        motion.set_selected_intensity(0.0);

        for time in [1.0, 1.25, 1.75, 2.0] {
            assert_eq!(motion.zoom_anchor_at(time), (0.15, 0.25));
            assert_eq!(motion.sample(time), motion.sample(1.0));
        }
        motion.add_segment_at(2.0).unwrap();
        let anchor = motion.zoom_anchor_at(2.0 + 1e-9);
        assert!((anchor.0 - 0.15).abs() < 1e-7);
        assert!((anchor.1 - 0.25).abs() < 1e-7);
    }

    #[test]
    fn motion_spring_preserves_zoom_anchor_overshoot_in_the_rendered_pose() {
        use crate::recording::editor::model::MotionTimingKind;

        let card = ImageSurface::create(Format::ARgb32, 200, 100).unwrap();
        let mut motion = motion_with_first_clip();
        motion.set_selected_perspective(0.0);
        motion.set_selected_zoom_anchor(0.15, 0.25);
        motion.add_segment_at(1.0).unwrap();
        motion.set_selected_end_scale(2.0);
        motion.set_selected_zoom_anchor(0.85, 0.75);
        let timing = MotionEffectTransformTiming {
            kind: MotionTimingKind::Spring,
            spring_bounce: 0.35,
            transition_duration: 1.0,
            ..MotionEffectTransformTiming::default()
        };
        motion.set_transform_timing(timing);
        let damping = -0.35_f64.ln() / 0.35_f64.ln().hypot(std::f64::consts::PI);
        let frequency = -0.01_f64.ln() / damping;
        let peak_time = std::f64::consts::PI / (frequency * (1.0 - damping * damping).sqrt());
        let time = 1.0 + peak_time;
        let anchor = motion.zoom_anchor_at(time);
        let expected_anchor = (0.15 + 0.70 * 1.35, 0.25 + 0.50 * 1.35);
        assert!((anchor.0 - expected_anchor.0).abs() < 1e-9);
        assert!((anchor.1 - expected_anchor.1).abs() < 1e-9);
        let pose = motion.sample(time);
        let layout = frame_layout(&card, pose, anchor);
        let expected_x = layout.base_x
            - (pose.scale - 1.0) * (expected_anchor.0 * 2.0 - 1.0)
                * layout.img_w() * layout.source_fit() * 0.5;
        assert!((layout.cx - expected_x).abs() < 1e-9);
        assert_eq!(motion.zoom_anchor_at(2.0), (0.85, 0.75));
    }

    #[test]
    fn motion_mp4_frames_match_their_encoded_timestamps() {
        use crate::recording::editor::model::MotionFramePreset;
        use std::process::Command;

        if crate::recording::editor::ffmpeg::ensure_tools_available().is_err() {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "apexshot-motion-encode-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let snapshot = image::RgbaImage::from_pixel(64, 32, image::Rgba([255, 255, 255, 255]));
        let source = root.join("timestamp-check.png");
        let mut motion = motion_with_first_clip();
        motion.set_duration(1.0);
        motion.set_selected_perspective(0.0);
        motion.set_selected_end_pos_x(0.5);
        motion.set_selected_transition_ms(1000);
        motion.segments[0].timing.easing_x1 = 0.0;
        motion.segments[0].timing.easing_y1 = 0.0;
        motion.segments[0].timing.easing_x2 = 1.0;
        motion.segments[0].timing.easing_y2 = 1.0;
        motion.frame.preset = MotionFramePreset::Custom;
        motion.frame.custom_width = 320;
        motion.frame.custom_height = 180;
        let output = super::export_motion_mp4(&snapshot, &motion, true, &source).unwrap();
        let probe = Command::new("ffprobe")
            .args([
                "-v", "error", "-select_streams", "v:0", "-show_entries",
                "stream=width,height,nb_frames,r_frame_rate,duration", "-of", "json",
            ])
            .arg(&output)
            .output()
            .unwrap();
        let decoded = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&output)
            .args([
                "-vf", "select=eq(n\\,29)", "-frames:v", "1", "-f", "rawvideo",
                "-pix_fmt", "rgba", "pipe:1",
            ])
            .output()
            .unwrap();
        std::fs::remove_file(output).unwrap();
        std::fs::remove_dir_all(root).unwrap();

        assert!(probe.status.success());
        let metadata: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        let stream = &metadata["streams"][0];
        assert_eq!(stream["width"], 320);
        assert_eq!(stream["height"], 180);
        assert_eq!(stream["nb_frames"], "30");
        assert_eq!(stream["r_frame_rate"], "30/1");
        assert_eq!(stream["duration"], "1.000000");
        assert!(decoded.status.success());
        assert_eq!(decoded.stdout.len(), 320 * 180 * 4);
        let mut count = 0;
        let mut sum_x = 0.0;
        for (index, pixel) in decoded.stdout.chunks_exact(4).enumerate() {
            if pixel[0] > 220 && pixel[1] > 220 && pixel[2] > 220 {
                count += 1;
                sum_x += (index % 320) as f64 + 0.5;
            }
        }
        assert!(count > 0);
        let card = crate::capture::editor::render::rgba_image_to_surface(&snapshot).unwrap();
        let time = 29.0 / 30.0;
        let expected = card_layout(
            &card, 1.0, MotionStage::frame(320.0, 180.0), &motion.appearance,
            motion.frame.effective_aspect(), motion.sample(time), motion.zoom_anchor_at(time),
        );
        let actual_x = sum_x / f64::from(count);
        assert!(
            (actual_x - expected.cx).abs() < 0.75,
            "last encoded frame center {actual_x} must match its timestamp: {}",
            expected.cx
        );
    }

    /// Export-style layout: the full frame under the default appearance.
    fn frame_layout(
        surface: &ImageSurface,
        transform: MotionTransform,
        zoom_anchor: (f64, f64),
    ) -> CardLayout {
        let appearance = MotionAppearance::default();
        card_layout(
            surface,
            1.0,
            MotionStage::frame(1440.0, 900.0),
            &appearance,
            None,
            transform,
            zoom_anchor,
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

    /// Build a card layout the way production does: the shared composition for
    /// the source dimensions behind the texture, then the stage.
    fn card_layout(
        surface: &ImageSurface,
        card_scale: f64,
        stage: MotionStage,
        appearance: &MotionAppearance,
        canvas_aspect: Option<f64>,
        transform: MotionTransform,
        zoom_anchor: (f64, f64),
    ) -> CardLayout {
        let (source_w, source_h) = super::motion_source_size(surface, card_scale);
        CardLayout::new(
            motion_background_composition(source_w, source_h, appearance, canvas_aspect).compute(),
            stage,
            transform,
            zoom_anchor,
        )
    }

    /// The preview stage production builds for a still at this widget size.
    fn preview_stage(
        surface: &ImageSurface,
        width: f64,
        height: f64,
        appearance: &MotionAppearance,
        frame: &MotionFrame,
    ) -> MotionStage {
        let (source_w, source_h) = super::motion_source_size(surface, 1.0);
        let composition = super::motion_canvas(source_w, source_h, appearance, frame);
        MotionStage::preview(
            width,
            height,
            composition.canvas_width,
            composition.canvas_height,
        )
    }

    /// Build the appearance Motion renders a card with for one geometry case.
    fn geometry_appearance(
        padding: f64,
        style: FrameStyle,
        filled: bool,
        insert: f64,
        alignment: BackgroundAlignment,
    ) -> MotionAppearance {
        MotionAppearance {
            background_padding: padding,
            background_insert: insert,
            background_alignment: alignment,
            background_fill_type: if filled {
                MotionBackgroundFillType::Color
            } else {
                MotionBackgroundFillType::None
            },
            frame_style: style,
            border_thickness: 6.0,
            ..MotionAppearance::default()
        }
    }

    /// The static canvas's own composition, written the way the canvas
    /// renders it, so the comparison is against the real Static path and not
    /// against Motion's builder twice.
    fn static_composition(
        source_w: f64,
        source_h: f64,
        padding: f64,
        frame_style: FrameStyle,
        filled: bool,
        aspect: CropAspectRatio,
        insert: f64,
        alignment: BackgroundAlignment,
    ) -> CompositionLayout {
        let fill = if filled {
            BackgroundStyle::PlainColor(DrawColor::new(0.0, 0.0, 0.0, 1.0))
        } else {
            BackgroundStyle::None
        };
        BackgroundComposition::new(source_w, source_h)
            .with_style(fill)
            .with_padding(padding)
            .with_insert(insert)
            .with_alignment(alignment)
            .with_corner_radius(18.0)
            .with_aspect_ratio(aspect)
            .with_frame_style(frame_style)
            .with_frame_border_thickness(6.0)
            .compute()
    }

    fn normalized_rect(layout: &CompositionLayout) -> [f64; 4] {
        [
            layout.image_rect.x / layout.canvas_width,
            layout.image_rect.y / layout.canvas_height,
            layout.image_rect.width / layout.canvas_width,
            layout.image_rect.height / layout.canvas_height,
        ]
    }

    /// Switching Static to Motion with an empty effects track must preserve
    /// the card's normalized rectangle inside its background: same padding,
    /// legacy inset, frame ratio, frame style and alignment, at any viewport.
    /// The comparison uses the card corners the renderer actually projects
    /// against the scene rectangle it actually paints.
    #[test]
    fn static_and_motion_share_the_normalized_card_rectangle() {
        let styles = [
            FrameStyle::Default,
            FrameStyle::Stack,
            FrameStyle::Stack2,
            FrameStyle::Border,
            FrameStyle::InsetLight,
        ];
        let cases: [(f64, Option<f64>, CropAspectRatio); 3] = [
            (0.0, None, CropAspectRatio::Original),
            (40.0, Some(16.0 / 9.0), CropAspectRatio::SixteenNine),
            (96.0, Some(1.0), CropAspectRatio::Square),
        ];
        let viewports = [(1600.0, 1000.0), (320.0, 200.0)];
        for (source_w, source_h) in [(1920.0, 1080.0), (320.0, 180.0)] {
            let card = ImageSurface::create(Format::ARgb32, source_w as i32, source_h as i32)
                .expect("card surface");
            for (padding, canvas_aspect, crop_aspect) in cases {
                for style in styles {
                    for (insert, alignment) in [
                        (0.0, BackgroundAlignment::Center),
                        (60.0, BackgroundAlignment::BottomRight),
                    ] {
                        let appearance =
                            geometry_appearance(padding, style, true, insert, alignment);
                        let expected = normalized_rect(&static_composition(
                            source_w,
                            source_h,
                            padding,
                            style,
                            true,
                            crop_aspect,
                            insert,
                            alignment,
                        ));
                        let composition = motion_background_composition(
                            source_w,
                            source_h,
                            &appearance,
                            canvas_aspect,
                        )
                        .compute();
                        for (width, height) in viewports {
                            let scene = super::motion_preview_scene_rect(
                                width,
                                height,
                                composition.canvas_width,
                                composition.canvas_height,
                            );
                            let stage = MotionStage::preview(
                                width,
                                height,
                                composition.canvas_width,
                                composition.canvas_height,
                            );
                            let layout = card_layout(
                                &card,
                                1.0,
                                stage,
                                &appearance,
                                canvas_aspect,
                                MotionTransform::default(),
                                (0.5, 0.5),
                            );
                            let top_left = layout.project(0.0, 0.0);
                            let bottom_right = layout.project(layout.img_w(), layout.img_h());
                            let actual = [
                                (top_left.0 - scene.0) / scene.2,
                                (top_left.1 - scene.1) / scene.3,
                                (bottom_right.0 - top_left.0) / scene.2,
                                (bottom_right.1 - top_left.1) / scene.3,
                            ];
                            for (a, b) in expected.iter().zip(actual.iter()) {
                                assert!(
                                    (a - b).abs() < 1e-9,
                                    "{source_w}x{source_h} padding {padding} {style:?} \
                                     insert {insert} at {width}x{height}: {expected:?} vs {actual:?}"
                                );
                            }
                        }
                    }
                }
            }
            let small = geometry_appearance(
                0.0,
                FrameStyle::Default,
                true,
                0.0,
                BackgroundAlignment::Center,
            );
            let composition =
                motion_background_composition(source_w, source_h, &small, None).compute();
            if source_w < 1600.0 {
                let scene = super::motion_preview_scene_rect(
                    1600.0,
                    1000.0,
                    composition.canvas_width,
                    composition.canvas_height,
                );
                assert!((scene.2 - composition.canvas_width).abs() < 1e-9);
                assert!((scene.3 - composition.canvas_height).abs() < 1e-9);
            }
        }
    }

    /// Stack backings offset in composition canvas pixels, exactly like the
    /// canvas the composition itself reserves for them. Legacy inset > 0
    /// scales the card, so using the source fit here shrank every peek by the
    /// draw scale and the sheets drifted inside the space Static had
    /// reserved. Measured from the pixels the renderer paints.
    #[test]
    fn stack_backing_peek_tracks_the_composition_canvas() {
        let card = ImageSurface::create(Format::ARgb32, 200, 200).expect("card");
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
        }
        card.flush();

        let appearance = geometry_appearance(
            24.0,
            FrameStyle::Stack2,
            true,
            40.0,
            BackgroundAlignment::Center,
        );
        let source_w = 200.0;
        let source_h = 200.0;
        let composition =
            motion_background_composition(source_w, source_h, &appearance, None).compute();
        let stage = MotionStage::frame(400.0, 400.0);
        let layout = card_layout(
            &card,
            1.0,
            stage,
            &appearance,
            None,
            MotionTransform::default(),
            (0.5, 0.5),
        );
        assert!(
            (layout.canvas_fit - 1.0).abs() < 1e-9,
            "the stage must draw the composition at its native size"
        );
        assert!(
            (layout.source_fit() - 0.8).abs() < 1e-9,
            "the legacy inset must scale the source, so the two units differ"
        );

        let mut frame = ImageSurface::create(Format::ARgb32, 400, 400).unwrap();
        {
            let context = Context::new(&frame).unwrap();
            draw_transformed_card(
                &context,
                &card,
                layout,
                &appearance,
                1.0,
                8,
                gtk4::cairo::Filter::Good,
            );
        }
        frame.flush();

        let backing = FrameStyle::Stack2.spec().backing1.expect("Stack2 backing");
        let draw_w = composition.image_rect.width * layout.canvas_fit;
        let draw_h = composition.image_rect.height * layout.canvas_fit;
        let canvas_x = stage.center_x - composition.canvas_width * layout.canvas_fit / 2.0;
        let canvas_y = stage.center_y - composition.canvas_height * layout.canvas_fit / 2.0;
        let image_left = canvas_x + composition.image_rect.x * layout.canvas_fit;
        let image_top = canvas_y + composition.image_rect.y * layout.canvas_fit;
        let (sin, cos) = backing.rotation_deg.to_radians().sin_cos();
        let pivot = (
            image_left + draw_w + backing.offset_x * layout.canvas_fit,
            image_top + draw_h + backing.offset_y * layout.canvas_fit,
        );
        let (corner_x, corner_y) = (0.0, -draw_h);
        let top = (
            pivot.0 + corner_x * cos - corner_y * sin,
            pivot.1 + corner_x * sin + corner_y * cos,
        );

        let stride = frame.stride() as usize;
        let data = frame.data().unwrap();
        let alpha_at =
            |x: f64, y: f64| data[y.round() as usize * stride + x.round() as usize * 4 + 3];
        assert!(
            alpha_at(top.0 - 2.0, top.1 + 3.0) > 200,
            "the painted backing must reach the peek the composition reserved"
        );
        assert_eq!(
            alpha_at(top.0 - 2.0, top.1 - 4.0),
            0,
            "nothing may be painted above the reserved peek"
        );
    }

    /// The downscaled preview texture must describe the same card as the
    /// full-resolution export: fixed-pixel frame overhangs would otherwise
    /// shrink with the preview.
    #[test]
    fn downscaled_preview_keeps_the_source_card_geometry() {
        let card = ImageSurface::create(Format::ARgb32, 2560, 1440).unwrap();
        let (preview, scale) = super::scaled_card_preview(&card).expect("preview texture");
        let appearance = geometry_appearance(
            40.0,
            FrameStyle::Stack2,
            true,
            30.0,
            BackgroundAlignment::TopLeft,
        );
        let stage = MotionStage::frame(1280.0, 800.0);
        let full = card_layout(
            &card,
            1.0,
            stage,
            &appearance,
            Some(16.0 / 9.0),
            MotionTransform::default(),
            (0.5, 0.5),
        );
        let downscaled = card_layout(
            &preview,
            scale,
            stage,
            &appearance,
            Some(16.0 / 9.0),
            MotionTransform::default(),
            (0.5, 0.5),
        );
        assert!((full.img_w() - downscaled.img_w()).abs() < 1e-9);
        assert!((full.source_fit() - downscaled.source_fit()).abs() < 1e-9);
        for (a, b) in [
            (full.project(0.0, 0.0), downscaled.project(0.0, 0.0)),
            (
                full.project(full.img_w(), full.img_h()),
                downscaled.project(downscaled.img_w(), downscaled.img_h()),
            ),
        ] {
            assert!(
                (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6,
                "{a:?} vs {b:?}"
            );
        }
    }

    /// Inverse title placement and its hit region must use the same
    /// source-derived stage at any preview size.
    #[test]
    fn inverse_hit_test_tracks_the_preview_stage() {
        let surface = ImageSurface::create(Format::ARgb32, 1200, 675).expect("surface");
        let mut motion = MotionState::default();
        let index = motion.add_text_at(0.0).expect("title clip");
        motion.set_selected_text_attachment(
            crate::recording::editor::model::MotionTextCoordinateSpace::MotionCanvasLocal,
            motion.text_segments[index].pos_x,
            motion.text_segments[index].pos_y,
        );
        motion.set_selected_text_wrap_width(0.0);
        motion.set_selected_text_alignment(MotionTextAlignment::Center);
        let segment = &motion.text_segments[index];
        let appearance = geometry_appearance(
            40.0,
            FrameStyle::Border,
            true,
            0.0,
            BackgroundAlignment::Center,
        );
        for (width, height) in [(1440.0, 900.0), (360.0, 240.0)] {
            let stage = preview_stage(&surface, width, height, &appearance, &motion.frame);
            let layout = card_layout(
                &surface,
                1.0,
                stage,
                &appearance,
                motion.frame.effective_aspect(),
                MotionTransform::default(),
                (0.5, 0.5),
            );
            let center = layout.project(
                segment.pos_x * layout.img_w(),
                segment.pos_y * layout.img_h(),
            );
            assert!(
                motion_text_contains_view_point(layout, segment, 0.5, center.0, center.1),
                "the title hit region must track the {width}x{height} preview"
            );
            assert!(!motion_text_contains_view_point(
                layout, segment, 0.5, 2.0, 2.0
            ));
            let restored = view_point_to_motion_text_position(layout, center.0, center.1);
            assert!((restored.0 - segment.pos_x).abs() < 0.02, "{restored:?}");
        }
    }

    #[test]
    fn flat_card_pose_draws_exactly_its_rect() {
        let card = ImageSurface::create(Format::ARgb32, 100, 50).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(1.0, 0.0, 0.0);
            context.paint().unwrap();
        }
        card.flush();

        let motion = motion_with_first_clip();
        let mut frame = ImageSurface::create(Format::ARgb32, 400, 300).unwrap();
        {
            let context = Context::new(&frame).unwrap();
            // No rotation and no perspective take the single-rectangle path.
            let layout = card_layout(
                &card,
                1.0,
                MotionStage::frame(400.0, 300.0),
                &motion.appearance,
                None,
                MotionTransform::default(),
                (0.5, 0.5),
            );
            super::draw_transformed_card(
                &context,
                &card,
                layout,
                &motion.appearance,
                1.0,
                8,
                gtk4::cairo::Filter::Good,
            );
        }
        frame.flush();

        let stride = frame.stride() as usize;
        let data = frame.data().unwrap();
        let mut painted = 0usize;
        for y in 0..300usize {
            for x in 0..400usize {
                let offset = y * stride + x * 4;
                if data[offset + 2] > 200 && data[offset + 1] < 60 && data[offset] < 60 {
                    painted += 1;
                }
            }
        }
        assert!(
            (painted as i64 - 5000).abs() < 400,
            "the flat card must cover its 100x50 rect exactly, painted {painted} px"
        );
    }

    #[test]
    fn scene_fill_is_bounded_in_preview_and_full_frame_on_export() {
        let card = ImageSurface::create(Format::ARgb32, 8, 8).unwrap();
        let mut motion = MotionState::default();

        // The preview keeps the checkerboard canvas until a fill is chosen.
        let mut frame = render_appearance_frame(&card, &motion, true);
        let data = frame.data().unwrap();
        assert_ne!(&data[..4], &[0, 0, 0, 255]);

        motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
        motion.appearance.background_color = [0.2, 0.4, 0.6, 1.0];
        let (source_w, source_h) = super::motion_source_size(&card, 1.0);
        let composition =
            super::motion_canvas(source_w, source_h, &motion.appearance, &motion.frame);
        let (scene_x, scene_y, scene_w, scene_h) = super::motion_preview_scene_rect(
            128.0,
            96.0,
            composition.canvas_width,
            composition.canvas_height,
        );
        let center_x = (scene_x + scene_w * 0.5).floor() as usize;
        let center_y = (scene_y + scene_h * 0.5).floor() as usize;
        let center = center_y * 128 * 4 + center_x * 4;
        let corner_x = scene_x.ceil() as usize + 1;
        let corner_y = scene_y.ceil() as usize + 1;
        let scene_corner = corner_y * 128 * 4 + corner_x * 4;
        let mut frame = render_appearance_frame(&card, &motion, true);
        let data = frame.data().unwrap();
        // Cairo ARgb32 is BGRA on the Linux targets we support.
        assert_eq!(&data[center..center + 4], &[153, 102, 51, 255]);
        assert_eq!(&data[scene_corner..scene_corner + 4], &[153, 102, 51, 255]);
        assert_ne!(&data[..4], &[153, 102, 51, 255]);

        // Exports have no editor canvas: the fill covers the whole frame and
        // an unset fill is a black scene. The radius belongs to the
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
        let stage = preview_stage(&blank_card, 128.0, 96.0, &motion.appearance, &motion.frame);
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
        // card center stays image. Slider units are reference px against a
        // 400px long edge, so a 64px card needs 128 to reach a 32px stage radius.
        motion.appearance.border_radius = 128.0;
        let mut frame = render_appearance_frame(&card, &motion, false);
        let data = frame.data().unwrap();
        assert_eq!(&data[card_corner..card_corner + 4], &[0, 0, 0, 255]);
        assert_eq!(&data[card_center..card_center + 4], &[255, 255, 255, 255]);
    }

    /// Tilted frame outlines must keep the card's rounded corners. Regression:
    /// mid-clip poses traced the sharp projected quad, so the Liquid/border
    /// frame lost its radius while the clip played and snapped back after.
    #[test]
    fn tilted_frame_outline_keeps_the_cards_rounded_corners() {
        let transform = MotionTransform {
            rotation_y: 8.0,
            perspective: 0.18,
            ..MotionTransform::default()
        };
        let depth = crate::recording::editor::model::card_depth(100.0, 60.0, transform.perspective);
        let outline =
            super::projected_rounded_rect_points(100.0, 60.0, 20.0, transform, depth, 500.0, 300.0);
        assert_eq!(outline.len(), 44);
        assert!(
            outline.iter().all(|(x, y)| x.is_finite() && y.is_finite()),
            "tilted outline has non-finite points: {outline:?}"
        );
        // No outline point reaches the sharp quad corners: the radius cut
        // keeps every projected quad corner well clear of the path.
        let quad = project_card_corners(200.0, 120.0, 1.0, transform, 500.0, 300.0);
        for (qx, qy) in quad {
            let nearest = outline
                .iter()
                .map(|(x, y)| ((x - qx).powi(2) + (y - qy).powi(2)).sqrt())
                .fold(f64::INFINITY, f64::min);
            assert!(
                nearest > 2.0,
                "tilted outline should cut quad corner ({qx},{qy}), nearest point is {nearest}px away"
            );
        }
    }

    /// Tilted border follows the rounded card edge. Full-frame regression
    /// for the playback bug: with a radius set, the tilted border must cut
    /// the projected quad corner instead of stroking through it.
    #[test]
    fn tilted_border_stroke_cuts_the_quad_corner_when_radius_is_set() {
        let card = ImageSurface::create(Format::ARgb32, 100, 50).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(1.0, 0.0, 0.0);
            context.paint().unwrap();
        }
        card.flush();
        let transform = MotionTransform {
            rotation_y: 12.0,
            perspective: 0.25,
            ..MotionTransform::default()
        };
        let stage = MotionStage::frame(400.0, 300.0);
        let probe = card_layout(
            &card,
            1.0,
            stage,
            &probe_appearance(),
            None,
            transform,
            (0.5, 0.5),
        );
        let fit = probe.source_fit();
        let (cx, cy) = (probe.cx, probe.cy);
        // Quad order is TL, TR, BR, BL.
        let (qx, qy) = project_card_corners(100.0, 50.0, fit, transform, cx, cy)[1];
        let px = qx.round() as i32;
        let py = qy.round() as i32;
        assert!(
            px > 4 && px < 396 && py > 4 && py < 296,
            "tilted quad corner ({qx},{qy}) should land inside the frame"
        );
        let render = |radius: f64| {
            let mut motion = MotionState::default();
            motion.appearance.background_padding = 0.0;
            motion.appearance.background_fill_type = MotionBackgroundFillType::None;
            motion.appearance.frame_style = crate::capture::editor::types::FrameStyle::Border;
            motion.appearance.border_thickness = 6.0;
            motion.appearance.border_fill_color = [1.0, 1.0, 1.0, 1.0];
            motion.appearance.border_radius = radius;
            motion.appearance.shadow_opacity = 0.0;
            let mut frame = ImageSurface::create(Format::ARgb32, 400, 300).unwrap();
            {
                let context = Context::new(&frame).unwrap();
                let layout = card_layout(
                    &card,
                    1.0,
                    stage,
                    &motion.appearance,
                    None,
                    transform,
                    (0.5, 0.5),
                );
                super::draw_transformed_card(
                    &context,
                    &card,
                    layout,
                    &motion.appearance,
                    1.0,
                    8,
                    gtk4::cairo::Filter::Good,
                );
            }
            frame.flush();
            let stride = frame.stride() as usize;
            let data = frame.data().unwrap();
            let offset = (py as usize) * stride + (px as usize) * 4;
            [
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]
        };
        // Sharp frame strokes through the quad corner.
        let sharp = render(0.0);
        assert!(
            sharp[0] > 200 && sharp[1] > 200 && sharp[2] > 200,
            "radius 0 should stroke the tilted quad corner white, got {sharp:?}"
        );
        // Rounded frame cuts it: the quad corner shows the scene behind.
        let rounded = render(120.0);
        assert!(
            rounded[0] < 100 && rounded[1] < 100 && rounded[2] < 100,
            "radius should cut the tilted quad corner, got {rounded:?}"
        );
    }

    /// Zooming the camera must scale the frame's corner radius together with
    /// the image. Regression: the frame radius ignored `transform.scale`, so
    /// at scale > 1 the band's corner curve stayed tighter than the texture's
    /// rounded corner and a sliver of background showed between them.
    #[test]
    fn zoomed_frame_band_stays_glued_to_the_rounded_corner() {
        const BORDER_RADIUS: f64 = 60.0;
        const THICKNESS: f64 = 24.0;
        const SCALE: f64 = 3.0;
        let card = ImageSurface::create(Format::ARgb32, 100, 50).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
        }
        card.flush();
        // Match the compositor: round the texture in source pixels, then let
        // the mesh draw it at `fit * scale`.
        let source_radius = BORDER_RADIUS * 100.0 / 400.0;
        let rounded = super::rounded_motion_surface(&card, source_radius).expect("rounded card");

        let transform = MotionTransform {
            scale: SCALE,
            ..MotionTransform::default()
        };
        let stage = MotionStage::frame(400.0, 300.0);
        let probe = card_layout(
            &card,
            1.0,
            stage,
            &probe_appearance(),
            None,
            transform,
            (0.5, 0.5),
        );
        let fit = probe.source_fit();
        let (cx, cy) = (probe.cx, probe.cy);
        let hw = 100.0 * fit * SCALE / 2.0;
        let hh = 50.0 * fit * SCALE / 2.0;
        // On-screen corner radius of the texture: the source-pixel radius at
        // the same `fit * scale` the mesh draws with.
        let radius = source_radius * fit * SCALE;
        // Top-right corner arc centre of the rounded image.
        let arc_x = cx + hw - radius;
        let arc_y = cy - hh + radius;

        let mut motion = MotionState::default();
        motion.appearance.background_padding = 0.0;
        motion.appearance.background_fill_type = MotionBackgroundFillType::None;
        motion.appearance.frame_style = crate::capture::editor::types::FrameStyle::Border;
        motion.appearance.border_thickness = THICKNESS;
        motion.appearance.border_fill_color = [1.0, 1.0, 1.0, 1.0];
        motion.appearance.border_radius = BORDER_RADIUS;
        motion.appearance.shadow_opacity = 0.0;
        let mut frame = ImageSurface::create(Format::ARgb32, 400, 300).unwrap();
        {
            let context = Context::new(&frame).unwrap();
            let layout = card_layout(
                &rounded,
                1.0,
                stage,
                &motion.appearance,
                None,
                transform,
                (0.5, 0.5),
            );
            super::draw_transformed_card(
                &context,
                &rounded,
                layout,
                &motion.appearance,
                1.0,
                8,
                gtk4::cairo::Filter::Good,
            );
        }
        frame.flush();
        let stride = frame.stride() as usize;
        let data = frame.data().unwrap();
        let alpha_at = |x: f64, y: f64| -> u8 {
            data[y.round() as usize * stride + x.round() as usize * 4 + 3]
        };

        // Walk rays across the corner arc from inside the image outwards. The
        // image and its frame band must be contiguous, with no background run
        // between them — that sliver is the reported bug.
        for step in 0..=10 {
            let angle_deg = 20.0 + 5.0 * step as f64;
            let angle = angle_deg.to_radians();
            let (dx, dy) = (angle.cos(), -angle.sin());
            let mut samples = Vec::new();
            let mut d = radius * 0.6;
            while d <= radius * 1.9 {
                samples.push(alpha_at(arc_x + dx * d, arc_y + dy * d));
                d += 1.0;
            }
            // Everything past the outermost bright pixel is empty scene; only
            // the span up to and including the band matters here.
            let last_bright = samples
                .iter()
                .rposition(|alpha| *alpha > 128)
                .unwrap_or(samples.len().saturating_sub(1));
            let mut run = 0;
            let mut worst = 0;
            for alpha in &samples[..=last_bright] {
                if *alpha < 24 {
                    run += 1;
                    // A single antialiased dip at the seam is fine; a
                    // sustained transparent run is not.
                    if run >= 2 {
                        worst = worst.max(run);
                    }
                } else {
                    run = 0;
                }
            }
            assert!(
                worst == 0,
                "background sliver of {worst}px between image and frame band at {angle_deg:.0}°"
            );
        }
        // Sanity: the zoomed corner is still cut, so the band did not simply
        // swallow the quad corner.
        let corner_alpha = alpha_at(cx + hw, cy - hh);
        assert!(
            corner_alpha < 32,
            "zoomed quad corner should stay cut, got alpha {corner_alpha}"
        );
    }

    #[test]
    fn flat_projected_outline_matches_the_card_rect() {
        // Unrotated pose: the projected outline is the card rect itself, so
        // the mid-clip frame path agrees with the flat rounded-rect path.
        let transform = MotionTransform::default();
        let depth = crate::recording::editor::model::card_depth(100.0, 60.0, transform.perspective);
        let outline =
            super::projected_rounded_rect_points(100.0, 60.0, 20.0, transform, depth, 500.0, 300.0);
        assert_eq!(outline.len(), 44);
        // First point is the top edge where the TR corner leaves it.
        let (x, y) = outline[0];
        assert!(
            (x - 580.0).abs() < 1.0 && (y - 240.0).abs() < 1.0,
            "flat outline should start at the top edge (580,240), got ({x},{y})"
        );
        // Zero radius collapses to the sharp quad.
        let sharp =
            super::projected_rounded_rect_points(100.0, 60.0, 0.0, transform, depth, 500.0, 300.0);
        assert_eq!(sharp.len(), 4);
        let quad = project_card_corners(200.0, 120.0, 1.0, transform, 500.0, 300.0);
        for (x, y) in &sharp {
            let nearest = quad
                .iter()
                .map(|(qx, qy)| ((x - qx).powi(2) + (y - qy).powi(2)).sqrt())
                .fold(f64::INFINITY, f64::min);
            assert!(
                nearest < 1e-6,
                "zero-radius outline should match the quad, got ({x},{y})"
            );
        }
    }

    #[test]
    fn card_shadow_has_a_smooth_falloff_and_stays_inside_the_scene() {
        let mut frame = ImageSurface::create(Format::ARgb32, 160, 120).unwrap();
        let stage = MotionStage {
            ..MotionStage::frame(120.0, 80.0)
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
            paint_card_shadow(
                &context,
                stage,
                corners,
                0.0,
                appearance.shadow_opacity,
                appearance.shadow_blur,
                appearance.shadow_position,
            );
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
            paint_card_shadow(
                &context,
                stage,
                corners,
                0.0,
                appearance.shadow_opacity,
                appearance.shadow_blur,
                appearance.shadow_position,
            );
        }
        frame.flush();
        let data = frame.data().unwrap();
        let channel = |x: usize, y: usize| data[(y * 100 + x) * 4];
        assert_eq!(channel(75, 40), 0);
        assert_eq!(channel(25, 40), 255);
    }

    /// The shadow is the card's silhouette, so it must follow the card's own
    /// corner radius. With a hard (unblurred) shadow the extreme corner is the
    /// clearest tell: sharp leaves it filled, rounded cuts it away.
    #[test]
    fn shadow_corner_follows_the_card_border_radius() {
        let render = |radius: f64| -> Vec<u8> {
            let mut frame = ImageSurface::create(Format::ARgb32, 100, 80).unwrap();
            {
                let context = Context::new(&frame).unwrap();
                context.set_source_rgb(1.0, 1.0, 1.0);
                context.paint().unwrap();
                paint_card_shadow(
                    &context,
                    MotionStage::frame(100.0, 80.0),
                    [(30.0, 20.0), (70.0, 20.0), (70.0, 60.0), (30.0, 60.0)],
                    radius,
                    1.0,
                    0.0,
                    (0.0, 0.0),
                );
            }
            frame.flush();
            let pixels = frame.data().unwrap().to_vec();
            pixels
        };
        let channel = |pixels: &[u8], x: usize, y: usize| pixels[(y * 100 + x) * 4];

        let sharp = render(0.0);
        let rounded = render(12.0);
        // Card center stays a solid shadow in both.
        assert_eq!(channel(&sharp, 50, 40), 0);
        assert_eq!(channel(&rounded, 50, 40), 0);
        // The very corner is only filled when the silhouette is a sharp square.
        assert_eq!(
            channel(&sharp, 30, 20),
            0,
            "sharp shadow must fill the corner"
        );
        assert_eq!(
            channel(&rounded, 30, 20),
            255,
            "rounded shadow must cut the corner"
        );
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
    fn background_noise_grains_the_motion_backdrop_only() {
        let card = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(0.35, 0.35, 0.35);
            context.paint().unwrap();
        }
        card.flush();

        let render = |noise: f64| {
            let mut motion = MotionState::default();
            motion.appearance.background_padding = 0.0;
            motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
            motion.appearance.background_color = [0.08, 0.08, 0.08, 1.0];
            motion.appearance.background_noise = noise;
            motion.appearance.shadow_opacity = 0.0;
            motion.scene_shadow.opacity = 0.0;
            let frame = ImageSurface::create(Format::ARgb32, 128, 96).unwrap();
            {
                let context = Context::new(&frame).unwrap();
                draw_motion_frame(
                    &context, 128, 96, &card, &motion, None, None, 0.0, false, true, false, 1.0,
                );
            }
            frame.flush();
            frame
        };
        let sample = |surface: &mut ImageSurface, x: usize, y: usize| {
            surface.data().unwrap()[(y * 128 + x) * 4]
        };

        // Padding 0 centers the 64px card in the 128x96 frame, so the corners
        // are fill and the middle is card.
        let mut flat = render(0.0);
        let mut grainy = render(1.0);
        assert_eq!(
            sample(&mut flat, 4, 4),
            sample(&mut flat, 6, 4),
            "without noise the fill is flat"
        );
        // A single pixel can land on a faint speckle, so scan the fill band.
        let grained = (0..16).any(|x| sample(&mut grainy, x, 4) != sample(&mut flat, x, 4));
        assert!(grained, "the Motion backdrop fill must carry visible grain");
        assert_eq!(
            sample(&mut grainy, 64, 48),
            sample(&mut flat, 64, 48),
            "the card composites above the grain and stays clean"
        );
    }

    #[test]
    fn scene_shadow_placement_splits_above_and_below_the_card() {
        use crate::recording::editor::model::{
            MotionSceneShadowPlacement, MotionSceneShadowPreset,
        };

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
    fn new_segment_starts_identity_reaches_the_default_zoom_and_releases() {
        let motion = motion_with_first_clip();
        let start = motion.sample(0.0);
        let reach = motion.sample(motion.segments[0].end);
        let releasing = motion.sample(motion.segments[0].end + 0.6);
        let end = motion.sample(motion.duration);
        assert!((start.scale - 1.0).abs() < 1e-6);
        assert!(start.rotation_y.abs() < 1e-6);
        assert!((reach.scale - DEFAULT_MOTION_ZOOM).abs() < 1e-6);
        assert!(reach.rotation_y.abs() < 1e-6);
        assert!(
            releasing.scale > 1.0 && releasing.scale < DEFAULT_MOTION_ZOOM,
            "the tail eases back instead of freezing on the last pose"
        );
        assert!((end.scale - 1.0).abs() < 1e-6);
        assert_eq!(motion.segments.len(), 1);
        assert_eq!(motion.selected, Some(0));
    }

    /// A linear camera move whose exposure window covers real travel: the
    /// discriminating case for temporal accumulation versus ghost trails.
    fn fast_linear_motion() -> MotionState {
        let mut motion = motion_with_first_clip();
        motion.set_segment_range(0, 0.0, 1.0);
        motion.set_transform_timing(MotionEffectTransformTiming {
            transition_duration: 0.1,
            easing_x1: 0.0,
            easing_y1: 0.0,
            easing_x2: 1.0,
            easing_y2: 1.0,
            ..MotionEffectTransformTiming::default()
        });
        motion.set_selected_end_scale(4.0);
        motion.appearance.background_fill_type = MotionBackgroundFillType::None;
        motion.appearance.background_padding = 0.0;
        motion.set_motion_blur(1.0);
        motion.motion_blur_settings.shutter_angle = 360.0;
        motion
    }

    fn render_blur_frame(card: &ImageSurface, motion: &MotionState, time: f64) -> ImageSurface {
        let frame = ImageSurface::create(Format::ARgb32, 200, 150).unwrap();
        {
            let context = Context::new(&frame).unwrap();
            draw_motion_frame(
                &context, 200, 150, card, motion, None, None, time, false, true, false, 1.0,
            );
        }
        frame.flush();
        frame
    }

    /// True motion blur averages the exposure, so a fast move leaves a
    /// continuous gradient at the card's leading edge. The previous trail
    /// implementation stacked a fully opaque copy of the current pose on top,
    /// which is the "lagging" look users reported.
    #[test]
    fn motion_blur_smears_the_moving_card_instead_of_stacking_copies() {
        let card = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
        }
        card.flush();

        let motion = fast_linear_motion();
        let time = 0.05;
        let scale = motion.sample(time).scale;
        assert!(
            (scale - 2.5).abs() < 1e-6,
            "linear move at half time: {scale}"
        );

        let layout = card_layout(
            &card,
            1.0,
            MotionStage::frame(200.0, 150.0),
            &motion.appearance,
            motion.frame.effective_aspect(),
            motion.sample(time),
            motion.zoom_anchor_at(time),
        );
        let edge = layout.project(64.0, 32.0);
        let probe_x = (edge.0 - 2.0).round() as usize;
        let probe_y = edge.1.round() as usize;
        let value_at = |data: &[u8], stride: usize, x: usize, y: usize| data[y * stride + x * 4];

        let sharp = {
            let mut sharp = motion.clone();
            sharp.set_motion_blur(0.0);
            render_blur_frame(&card, &sharp, time)
        };
        let mut blurred = render_blur_frame(&card, &motion, time);
        let stride = blurred.stride() as usize;
        let blurred_data = blurred.data().unwrap().to_vec();
        let mut sharp = sharp;
        let sharp_data = sharp.data().unwrap().to_vec();

        assert_eq!(
            value_at(&sharp_data, stride, probe_x, probe_y),
            255,
            "without blur the current pose is opaque at its leading edge"
        );
        let lead = value_at(&blurred_data, stride, probe_x, probe_y);
        assert!(
            (1..200).contains(&lead),
            "the leading edge must be a partial exposure, got {lead}"
        );

        let row = edge.1.round() as usize;
        let mut levels = (32..probe_x)
            .map(|x| value_at(&blurred_data, stride, x, row))
            .filter(|value| (1..255).contains(value))
            .collect::<Vec<_>>();
        levels.dedup();
        assert!(
            levels.len() >= 8,
            "the smear must be continuous, saw {} levels",
            levels.len()
        );
    }

    /// A held pose has no travel during the exposure, so blur must leave the
    /// frame identical to the sharp render — no lingering trail.
    #[test]
    fn motion_blur_leaves_held_poses_sharp() {
        let card = ImageSurface::create(Format::ARgb32, 64, 64).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
        }
        card.flush();

        let mut motion = fast_linear_motion();
        motion.set_transform_timing(MotionEffectTransformTiming {
            transition_duration: 0.3,
            easing_x1: 0.0,
            easing_y1: 0.0,
            easing_x2: 1.0,
            easing_y2: 1.0,
            ..MotionEffectTransformTiming::default()
        });
        let sharp = {
            let mut sharp = motion.clone();
            sharp.set_motion_blur(0.0);
            sharp
        };
        for time in [0.0, 0.4] {
            let mut blurred = render_blur_frame(&card, &motion, time);
            let mut reference = render_blur_frame(&card, &sharp, time);
            assert_eq!(
                blurred.data().unwrap().to_vec(),
                reference.data().unwrap().to_vec(),
                "held pose at {time}s must not be blurred"
            );
        }
    }

    #[test]
    fn transition_playback_uses_the_same_card_mesh_as_the_still_preview() {
        // Solid card: filter-invariant, so this pins the perspective mesh
        // (geometry) rather than the resampling filter. The still preview
        // intentionally uses Good (sharp, matches Static) while playback uses
        // Bilinear (cheap); textured content may differ by a pixel, but the
        // mesh must not change when pressing play.
        let card = ImageSurface::create(Format::ARgb32, 96, 64).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(1.0, 1.0, 1.0);
            context.paint().unwrap();
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
        assert_eq!(
            (still.width(), still.height()),
            (playing.width(), playing.height())
        );
        // Center pixel pins the mesh (card position/geometry): it sits deep
        // inside the solid card in both renders. Edge fringes may differ by a
        // step because the still uses Good (sharp, matches Static) while
        // playback uses Bilinear (cheap) — that filter split is intentional.
        let center_pixel = |surface: &mut ImageSurface| {
            surface.flush();
            let stride = surface.stride() as usize;
            let offset = 64usize * stride + 96usize * 4;
            surface.data().unwrap()[offset..offset + 4].to_vec()
        };
        assert_eq!(
            center_pixel(&mut still),
            center_pixel(&mut playing),
            "playing an Ease preview must not move the card mesh"
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
        motion.appearance.gradient = crate::recording::editor::model::VideoGradient {
            stops: vec![
                crate::recording::editor::model::GradientStop::new(0.0, 20, 31, 56),
                crate::recording::editor::model::GradientStop::new(1.0, 107, 46, 138),
            ],
            ..crate::recording::editor::model::VideoGradient::default()
        };
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
            draw_motion_backdrop(
                &context,
                160,
                120,
                &motion,
                None,
                true,
                true,
                Some(super::motion_source_size(&card, 1.0)),
            );
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
    fn stretching_duration_keeps_the_move_and_releases_after_it() {
        let mut motion = motion_with_first_clip();
        let move_end = motion.segments[0].end;
        motion.set_duration(6.0);
        assert!((motion.segments[0].end - move_end).abs() < 1e-6);
        assert!((motion.sample(move_end).scale - DEFAULT_MOTION_ZOOM).abs() < 1e-6);
        // The camera returns to the initial framing by the track's end.
        assert!((motion.sample(6.0).scale - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_shorter_gap_releases_faster_at_the_same_elapsed_time() {
        let scale_after = |gap: f64| {
            let mut motion = motion_with_first_clip();
            motion.set_duration(6.0);
            motion
                .add_segment_at(1.0 + gap)
                .expect("a move after the gap");
            motion.sample(1.0 + 0.1).scale
        };
        assert!(
            scale_after(0.25) < scale_after(2.0),
            "a short gap pulls the camera home faster than a long one"
        );
    }

    #[test]
    fn flush_moves_chain_and_gapped_moves_restart_from_identity() {
        let mut flush = motion_with_first_clip();
        flush.set_duration(6.0);
        flush.add_segment_at(1.0).expect("a flush second move");
        let first_end = flush.segments[0].to;
        assert_eq!(flush.segments[1].from, first_end);
        flush.selected = Some(0);
        flush.set_selected_end_scale(1.5);
        assert!((flush.segments[1].from.scale - 1.5).abs() < 1e-6);

        let mut gapped = motion_with_first_clip();
        gapped.set_duration(6.0);
        gapped.add_segment_at(2.0).expect("a move after a gap");
        assert_eq!(
            gapped.segments[1].from,
            MotionTransform::default(),
            "a gap releases the camera to identity before the next move"
        );
        assert_eq!(
            gapped.sample(2.0),
            MotionTransform {
                perspective: gapped.perspective_intensity,
                ..MotionTransform::default()
            }
        );
        // The gap is consumed by the release: identity by the next move.
        assert!((gapped.sample(2.0 - 1e-6).scale - 1.0).abs() < 1e-3);
    }

    #[test]
    fn a_move_added_flush_while_zoomed_defaults_to_pulling_back_out() {
        let mut motion = motion_with_first_clip();
        motion.set_duration(6.0);
        motion.add_segment_at(1.0).expect("a flush second move");

        let second = &motion.segments[1];
        assert!((second.from.scale - DEFAULT_MOTION_ZOOM).abs() < 1e-6);
        assert!((second.to.scale - 1.0).abs() < 1e-6);
        let mid = motion.sample(second.start + second.duration() / 2.0);
        assert!(
            mid.scale > 1.0 && mid.scale < DEFAULT_MOTION_ZOOM,
            "the flush second move must animate instead of holding 2x → 2x"
        );
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
        let point = layout.project(expected.0 * layout.img_w(), expected.1 * layout.img_h());
        let actual = view_point_to_motion_text_position(layout, point.0, point.1);
        assert!((actual.0 - expected.0).abs() < 0.003, "x: {actual:?}");
        assert!((actual.1 - expected.1).abs() < 0.003, "y: {actual:?}");
    }

    #[test]
    fn title_hit_test_rejects_empty_preview_space() {
        let surface = ImageSurface::create(Format::ARgb32, 1200, 675).expect("surface");
        let mut motion = MotionState::default();
        let index = motion.add_text_at(0.0).expect("title clip");
        motion.set_selected_text_attachment(
            crate::recording::editor::model::MotionTextCoordinateSpace::MotionCanvasLocal,
            motion.text_segments[index].pos_x,
            motion.text_segments[index].pos_y,
        );
        motion.set_selected_text_wrap_width(0.0);
        motion.set_selected_text_alignment(MotionTextAlignment::Center);
        let segment = &motion.text_segments[index];
        let transform = MotionTransform {
            scale: 1.12,
            rotation_x: -7.0,
            rotation_y: 10.0,
            perspective: 0.18,
            ..MotionTransform::default()
        };
        let zoom_anchor = (0.32, 0.68);
        let stage = preview_stage(&surface, 1440.0, 900.0, &motion.appearance, &motion.frame);
        let layout = card_layout(
            &surface,
            1.0,
            stage,
            &motion.appearance,
            motion.frame.effective_aspect(),
            transform,
            zoom_anchor,
        );
        let title_center = layout.project(
            segment.pos_x * layout.img_w(),
            segment.pos_y * layout.img_h(),
        );

        assert!(motion_text_contains_view_point(
            layout,
            segment,
            0.5,
            title_center.0,
            title_center.1,
        ));
        assert!(!motion_text_contains_view_point(
            layout, segment, 0.5, 4.0, 4.0,
        ));
    }

    #[test]
    fn transform_timing_defaults_drive_glide_motion() {
        let mut motion = motion_with_first_clip();
        // Keep the clip longer than the 1.2s transition so the timing curve
        // is not clamped by the default one-second clip.
        motion.set_segment_range(0, 0.0, 2.0);
        let timing = motion.segments[0].timing;
        assert!((timing.transition_duration - 1.2).abs() < f64::EPSILON);
        assert!((timing.easing_x1 - 0.25).abs() < f64::EPSILON);
        assert!((timing.easing_y1 - 1.0).abs() < f64::EPSILON);
        assert!((timing.easing_x2 - 0.50).abs() < f64::EPSILON);
        assert!((timing.easing_y2 - 1.0).abs() < f64::EPSILON);

        let default_progress = motion.sample(0.6).scale;
        motion.set_transform_timing(MotionEffectTransformTiming {
            transition_duration: 1.2,
            easing_x1: 0.0,
            easing_y1: 0.0,
            easing_x2: 1.0,
            easing_y2: 1.0,
            ..MotionEffectTransformTiming::default()
        });
        let linear_progress = motion.sample(0.6).scale;
        assert!(
            default_progress > linear_progress + 0.01,
            "The default Bézier should advance ahead of linear at mid-transition"
        );
        // linear progress at t=0.6 of a 1.2s transition is 0.5: scale 1 + (2-1)*0.5
        assert!((linear_progress - 1.5).abs() < 0.002);
    }

    #[test]
    fn transition_ms_slider_drives_the_selected_clips_timing() {
        let mut motion = motion_with_first_clip();
        motion.set_selected_transition_ms(300);
        assert!((motion.segments[0].timing.transition_duration - 0.3).abs() < 1e-9);
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
        let end = motion.sample(motion.segments[0].end);
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

        assert_eq!(
            motion.sample(motion.segments[0].end),
            MotionTransform::default()
        );

        motion.set_selected_intensity(0.5);
        let half = motion.sample(motion.segments[0].end);
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
        let source_x = zoom_anchor.0 * before.img_w();
        let source_y = zoom_anchor.1 * before.img_h();
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
    fn position_extremes_map_the_camera_to_the_stage_edges() {
        // Camera framing: pad bottom-right shows bottom-right, so the card
        // itself shifts to the opposite corner.
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

        assert!((layout.cx - 0.0).abs() < 1e-6);
        assert!((layout.cy - 0.0).abs() < 1e-6);

        let layout = frame_layout(
            &surface,
            MotionTransform {
                scale: 2.0,
                pos_x: -1.0,
                pos_y: -1.0,
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
        assert!(
            motion.add_segment_at(2.0).is_some(),
            "second clip in the gap"
        );
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
                    &context, 1280, 800, surface, &motion, None, None, 0.35, true, true, false,
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

    use super::motion_text_anchor_view_point;
    use crate::recording::editor::model::{
        MotionTextAlignment, MotionTextAnimation, MotionTextCoordinateSpace, MotionTextScope,
        DEFAULT_MOTION_TEXT_TRANSITION_SECONDS,
    };

    const TITLE_BLUE: [f64; 4] = [0.1, 0.3, 0.9, 1.0];

    fn black_card(width: i32, height: i32) -> ImageSurface {
        let card = ImageSurface::create(Format::ARgb32, width, height).unwrap();
        {
            let context = Context::new(&card).unwrap();
            context.set_source_rgb(0.0, 0.0, 0.0);
            context.paint().unwrap();
        }
        card.flush();
        card
    }

    /// A flat blue composition carrying one black card and one white title:
    /// the only bright pixels in a render are the title's own ink.
    fn title_motion(
        attachment: MotionTextCoordinateSpace,
        pose: MotionTransform,
        pos: (f64, f64),
        configure: impl FnOnce(&mut MotionState),
    ) -> MotionState {
        let mut motion = MotionState::default();
        motion.appearance.background_fill_type = MotionBackgroundFillType::Color;
        motion.appearance.background_color = TITLE_BLUE;
        motion.appearance.background_padding = 0.0;
        motion.appearance.shadow_opacity = 0.0;
        motion.scene_shadow.opacity = 0.0;
        motion.add_segment_at(0.0).expect("move");
        motion.set_selected_transition_ms(0);
        motion.set_selected_end_scale(pose.scale.clamp(1.0, 4.0));
        motion.set_selected_end_pos_x(pose.pos_x);
        motion.set_selected_end_pos_y(pose.pos_y);
        motion.set_selected_end_yaw(pose.rotation_y);
        motion.add_text_at(0.0).expect("title clip");
        motion.set_selected_text_attachment(attachment, pos.0, pos.1);
        motion.set_selected_text_value("HEADLINE".into());
        motion.set_selected_text_wrap_width(0.25);
        motion.set_selected_text_shadow(false);
        configure(&mut motion);
        motion
    }

    fn render_composition(
        motion: &MotionState,
        width: i32,
        height: i32,
        time: f64,
    ) -> ImageSurface {
        let card = black_card(200, 150);
        let frame = ImageSurface::create(Format::ARgb32, width, height).unwrap();
        {
            let context = Context::new(&frame).unwrap();
            draw_motion_frame(
                &context, width, height, &card, motion, None, None, time, false, true, false, 1.0,
            );
        }
        frame.flush();
        frame
    }

    /// Bounding box of the brightest pixels: the white title ink, never the
    /// blue fill or the black card. Half-open on the far edge.
    fn bright_bbox(surface: &mut ImageSurface) -> Option<(usize, usize, usize, usize)> {
        surface.flush();
        let stride = surface.stride() as usize;
        let (width, height) = (surface.width() as usize, surface.height() as usize);
        let data = surface.data().unwrap();
        let (mut left, mut top, mut right, mut bottom) = (width, height, 0usize, 0usize);
        let mut found = false;
        for y in 0..height {
            for x in 0..width {
                let offset = y * stride + x * 4;
                if data[offset] > 170 && data[offset + 1] > 170 && data[offset + 2] > 170 {
                    found = true;
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x + 1);
                    bottom = bottom.max(y + 1);
                }
            }
        }
        found.then_some((left, top, right, bottom))
    }

    fn canvas_contains(
        card: &ImageSurface,
        stage: MotionStage,
        segment: &crate::recording::editor::model::MotionTextSegment,
        time: f64,
        point: (f64, f64),
    ) -> bool {
        motion_text_contains_view_point(
            card_layout(
                card,
                1.0,
                stage,
                &MotionAppearance {
                    background_padding: 0.0,
                    background_fill_type: MotionBackgroundFillType::Color,
                    ..MotionAppearance::default()
                },
                None,
                MotionTransform::default(),
                (0.5, 0.5),
            ),
            segment,
            time,
            point.0,
            point.1,
        )
    }

    /// Bounding boxes of the title's ink, one per visual line: contiguous
    /// bright rows, each with its own horizontal extent.
    fn ink_lines(surface: &mut ImageSurface) -> Vec<(usize, usize, usize, usize)> {
        surface.flush();
        let stride = surface.stride() as usize;
        let (width, height) = (surface.width() as usize, surface.height() as usize);
        let data = surface.data().unwrap();
        let mut lines: Vec<(usize, usize, usize, usize)> = Vec::new();
        let mut current: Option<(usize, usize, usize, usize)> = None;
        for y in 0..height {
            let mut row: Option<(usize, usize)> = None;
            for x in 0..width {
                let offset = y * stride + x * 4;
                if data[offset] > 170 && data[offset + 1] > 170 && data[offset + 2] > 170 {
                    row = Some((row.map_or(x, |(x0, _)| x0), x));
                }
            }
            match (current, row) {
                (None, Some((x0, x1))) => current = Some((x0, x1, y, y + 1)),
                (Some((x0, x1, y0, y1)), Some((rx0, rx1))) => {
                    current = Some((x0.min(rx0), x1.max(rx1), y0, y1.max(y + 1)))
                }
                (Some(line), None) => {
                    lines.push(line);
                    current = None;
                }
                (None, None) => {}
            }
        }
        lines.extend(current);
        lines
    }

    #[test]
    fn canvas_text_pad_keeps_the_whole_title_visible_at_every_edge() {
        for alignment in MotionTextAlignment::ALL {
            let mut motion = title_motion(
                MotionTextCoordinateSpace::Canvas,
                MotionTransform::default(),
                (0.5, 0.5),
                |motion| {
                    motion.set_selected_text_value("Corner\ntext".into());
                    motion.set_selected_text_alignment(alignment);
                    motion.set_selected_text_wrap_width(0.55);
                },
            );
            let mut centered = render_composition(&motion, 600, 400, 0.5);
            let (x0, y0, x1, y1) = bright_bbox(&mut centered).expect("centered title");
            let expected = ((x1 - x0) as i32, (y1 - y0) as i32);
            for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                motion.set_selected_text_pos(x, y);
                let mut frame = render_composition(&motion, 600, 400, 0.5);
                let (left, top, right, bottom) = bright_bbox(&mut frame).expect("edge title");
                assert!(
                    ((right - left) as i32 - expected.0).abs() <= 2
                        && ((bottom - top) as i32 - expected.1).abs() <= 2,
                    "{alignment:?} at ({x}, {y}) clipped the title: {left},{top}..{right},{bottom}; expected {expected:?}"
                );
                assert!(if x == 0.0 { left <= 3 } else { right >= 597 });
                assert!(if y == 0.0 { top <= 3 } else { bottom >= 397 });
            }
        }
    }

    #[test]
    fn image_text_pad_tracks_the_image_bounds_at_every_edge() {
        let pose = MotionTransform {
            pos_x: -0.5,
            ..MotionTransform::default()
        };
        for alignment in MotionTextAlignment::ALL {
            let mut motion = title_motion(
                MotionTextCoordinateSpace::MotionCanvasLocal,
                pose,
                (0.5, 0.5),
                |motion| {
                    motion.set_selected_text_value("Inside\nimage".into());
                    motion.set_selected_text_alignment(alignment);
                    motion.set_selected_text_wrap_width(0.55);
                },
            );
            let mut centered = render_composition(&motion, 600, 400, 0.5);
            let (x0, y0, x1, y1) = bright_bbox(&mut centered).expect("centered title");
            let expected = ((x1 - x0) as i32, (y1 - y0) as i32);
            for (x, y) in [(0.05, 0.05), (0.95, 0.05), (0.05, 0.95), (0.95, 0.95)] {
                motion.set_selected_text_pos(x, y);
                let mut frame = render_composition(&motion, 600, 400, 0.5);
                let (left, top, right, bottom) = bright_bbox(&mut frame).expect("image title");
                assert!(left >= 350 && right <= 550 && top >= 125 && bottom <= 275);
                assert!(
                    ((right - left) as i32 - expected.0).abs() <= 2
                        && ((bottom - top) as i32 - expected.1).abs() <= 2,
                    "{alignment:?} at ({x}, {y}) clipped image text: {left},{top}..{right},{bottom}"
                );
                assert!(if x == 0.05 { left <= 353 } else { right >= 547 });
                assert!(if y == 0.05 { top <= 128 } else { bottom >= 272 });
            }
        }
    }

    #[test]
    fn text_placement_and_drag_share_the_scaled_tilted_image_geometry() {
        let card = black_card(2560, 1440);
        let (preview, scale) = super::scaled_card_preview(&card).expect("scaled texture");
        let stage = MotionStage::rect_at(24.0, 24.0, 1100.0, 760.0);
        let transform = MotionTransform {
            scale: 1.1,
            rotation_x: -9.0,
            rotation_y: 14.0,
            rotation_z: 12.0,
            perspective: 0.24,
            pos_x: -0.18,
            pos_y: 0.12,
        };
        let zoom_anchor = (0.22, 0.78);
        let mut motion = title_motion(
            MotionTextCoordinateSpace::MotionCanvasLocal,
            transform,
            (0.5, 0.5),
            |motion| {
                motion.set_selected_text_value("IMAGE".into());
            },
        );
        let context = Context::new(&preview).unwrap();
        for position in [(0.05, 0.05), (0.5, 0.5), (0.95, 0.95)] {
            motion.set_selected_text_pos(position.0, position.1);
            let segment = &motion.text_segments[0];
            let placement = super::motion_text_placement(
                &context,
                card_layout(
                    &preview,
                    scale,
                    stage,
                    &MotionAppearance {
                        background_padding: 40.0,
                        background_fill_type: MotionBackgroundFillType::Color,
                        ..MotionAppearance::default()
                    },
                    None,
                    transform,
                    zoom_anchor,
                ),
                segment,
                0.5,
            )
            .expect("preview placement");
            let point = placement.anchor_view_point();
            let restored = placement
                .position_at(point.0, point.1)
                .expect("inverse placement");
            assert!(
                (restored.0 - position.0).abs() < 0.003,
                "{restored:?} vs {position:?}"
            );
            assert!(
                (restored.1 - position.1).abs() < 0.003,
                "{restored:?} vs {position:?}"
            );
            let original_context = Context::new(&card).unwrap();
            let original = super::motion_text_placement(
                &original_context,
                card_layout(
                    &card,
                    1.0,
                    stage,
                    &MotionAppearance {
                        background_padding: 40.0,
                        background_fill_type: MotionBackgroundFillType::Color,
                        ..MotionAppearance::default()
                    },
                    None,
                    transform,
                    zoom_anchor,
                ),
                segment,
                0.5,
            )
            .expect("original placement")
            .anchor_view_point();
            assert!((point.0 - original.0).abs() < 3.0);
            assert!((point.1 - original.1).abs() < 3.0);
        }
        motion.set_selected_text_pos(0.5, 0.5);
        let placement = super::motion_text_placement(
            &context,
            card_layout(
                &preview,
                scale,
                stage,
                &MotionAppearance {
                    background_padding: 40.0,
                    background_fill_type: MotionBackgroundFillType::Color,
                    ..MotionAppearance::default()
                },
                None,
                transform,
                zoom_anchor,
            ),
            &motion.text_segments[0],
            0.5,
        )
        .unwrap();
        let point = placement.anchor_view_point();
        let target = (point.0 + 50.0, point.1 - 20.0);
        let position = placement.position_at(target.0, target.1).unwrap();
        motion.set_selected_text_pos(position.0, position.1);
        let moved = super::motion_text_placement(
            &context,
            card_layout(
                &preview,
                scale,
                stage,
                &MotionAppearance {
                    background_padding: 40.0,
                    background_fill_type: MotionBackgroundFillType::Color,
                    ..MotionAppearance::default()
                },
                None,
                transform,
                zoom_anchor,
            ),
            &motion.text_segments[0],
            0.5,
        )
        .unwrap()
        .anchor_view_point();
        assert!((moved.0 - target.0).abs() < 0.01);
        assert!((moved.1 - target.1).abs() < 0.01);
    }

    #[test]
    fn changing_text_attachment_preserves_its_visible_center() {
        let card = black_card(200, 150);
        let context = Context::new(&card).unwrap();
        let stage = MotionStage::frame(600.0, 400.0);
        let mut motion = title_motion(
            MotionTextCoordinateSpace::Canvas,
            MotionTransform::default(),
            (0.6, 0.5),
            |motion| {
                motion.set_selected_text_value("Text".into());
            },
        );
        let initial = super::motion_text_placement(
            &context,
            card_layout(
                &card,
                1.0,
                stage,
                &MotionAppearance {
                    background_padding: 0.0,
                    background_fill_type: MotionBackgroundFillType::Color,
                    ..MotionAppearance::default()
                },
                None,
                MotionTransform::default(),
                (0.5, 0.5),
            ),
            &motion.text_segments[0],
            0.5,
        )
        .unwrap()
        .anchor_view_point();
        for attachment in [
            MotionTextCoordinateSpace::MotionCanvasLocal,
            MotionTextCoordinateSpace::Canvas,
        ] {
            let mut target = motion.text_segments[0].clone();
            target.annotation_coordinate_space = attachment;
            let placement = super::motion_text_placement(
                &context,
                card_layout(
                    &card,
                    1.0,
                    stage,
                    &MotionAppearance {
                        background_padding: 0.0,
                        background_fill_type: MotionBackgroundFillType::Color,
                        ..MotionAppearance::default()
                    },
                    None,
                    MotionTransform::default(),
                    (0.5, 0.5),
                ),
                &target,
                0.5,
            )
            .unwrap();
            let position = placement.position_at(initial.0, initial.1).unwrap();
            motion.set_selected_text_attachment(attachment, position.0, position.1);
            let point = super::motion_text_placement(
                &context,
                card_layout(
                    &card,
                    1.0,
                    stage,
                    &MotionAppearance {
                        background_padding: 0.0,
                        background_fill_type: MotionBackgroundFillType::Color,
                        ..MotionAppearance::default()
                    },
                    None,
                    MotionTransform::default(),
                    (0.5, 0.5),
                ),
                &motion.text_segments[0],
                0.5,
            )
            .unwrap()
            .anchor_view_point();
            assert!((point.0 - initial.0).abs() < 0.001);
            assert!((point.1 - initial.1).abs() < 0.001);
        }
    }

    #[test]
    fn canvas_title_holds_still_while_the_image_moves() {
        let pose = MotionTransform {
            scale: 1.2,
            rotation_y: 12.0,
            pos_x: -0.6,
            pos_y: 0.1,
            ..MotionTransform::default()
        };
        let canvas = MotionTextCoordinateSpace::Canvas;
        let still = title_motion(canvas, MotionTransform::default(), (0.2, 0.5), |_| {});
        let moved = title_motion(canvas, pose, (0.2, 0.5), |_| {});

        let (mut still_frame, mut moved_frame) = (
            render_composition(&still, 600, 400, 0.5),
            render_composition(&moved, 600, 400, 0.5),
        );
        let still_box = bright_bbox(&mut still_frame).expect("canvas title ink");
        let moved_box = bright_bbox(&mut moved_frame).expect("canvas title ink");
        assert_eq!(
            still_box, moved_box,
            "a Canvas title must not travel with the image card"
        );
        assert_ne!(
            still_frame.data().unwrap().to_vec(),
            moved_frame.data().unwrap().to_vec(),
            "the moved card must change the composition around the still title"
        );

        let card = black_card(200, 150);
        let stage = MotionStage::frame(600.0, 400.0);
        let segment = &still.text_segments[0];
        let still_anchor = motion_text_anchor_view_point(
            card_layout(
                &card,
                1.0,
                stage,
                &MotionAppearance {
                    background_padding: 0.0,
                    background_fill_type: MotionBackgroundFillType::Color,
                    ..MotionAppearance::default()
                },
                None,
                MotionTransform::default(),
                (0.5, 0.5),
            ),
            segment,
        )
        .expect("anchor");
        let moved_anchor = motion_text_anchor_view_point(
            card_layout(
                &card,
                1.0,
                stage,
                &MotionAppearance {
                    background_padding: 0.0,
                    background_fill_type: MotionBackgroundFillType::Color,
                    ..MotionAppearance::default()
                },
                None,
                pose,
                (0.5, 0.5),
            ),
            segment,
        )
        .expect("anchor");
        assert_eq!(still_anchor, moved_anchor);
    }

    #[test]
    fn card_title_still_follows_the_moving_image() {
        let pose = MotionTransform {
            scale: 1.2,
            rotation_y: 12.0,
            pos_x: -0.6,
            pos_y: 0.1,
            ..MotionTransform::default()
        };
        let image = MotionTextCoordinateSpace::MotionCanvasLocal;
        let still = title_motion(image, MotionTransform::default(), (0.2, 0.5), |_| {});
        let moved = title_motion(image, pose, (0.2, 0.5), |_| {});

        let (mut still_frame, mut moved_frame) = (
            render_composition(&still, 600, 400, 0.5),
            render_composition(&moved, 600, 400, 0.5),
        );
        let still_box = bright_bbox(&mut still_frame).expect("card title ink");
        let moved_box = bright_bbox(&mut moved_frame).expect("card title ink");
        assert_ne!(
            still_box, moved_box,
            "an image-attached title must follow the card"
        );

        let card = black_card(200, 150);
        let stage = MotionStage::frame(600.0, 400.0);
        let segment = &still.text_segments[0];
        let still_anchor = motion_text_anchor_view_point(
            card_layout(
                &card,
                1.0,
                stage,
                &MotionAppearance {
                    background_padding: 0.0,
                    background_fill_type: MotionBackgroundFillType::Color,
                    ..MotionAppearance::default()
                },
                None,
                MotionTransform::default(),
                (0.5, 0.5),
            ),
            segment,
        )
        .expect("anchor");
        let moved_anchor = motion_text_anchor_view_point(
            card_layout(
                &card,
                1.0,
                stage,
                &MotionAppearance {
                    background_padding: 0.0,
                    background_fill_type: MotionBackgroundFillType::Color,
                    ..MotionAppearance::default()
                },
                None,
                pose,
                (0.5, 0.5),
            ),
            segment,
        )
        .expect("anchor");
        assert_ne!(still_anchor, moved_anchor);
    }

    #[test]
    fn canvas_title_hit_test_inverts_its_own_rotation() {
        let canvas = MotionTextCoordinateSpace::Canvas;
        let flat = title_motion(canvas, MotionTransform::default(), (0.5, 0.5), |motion| {
            motion.set_selected_text_value("HEADING".into());
            motion.set_selected_text_wrap_width(0.0);
        });
        let mut rotated = flat.clone();
        rotated.set_selected_text_rotation(90.0);

        let (mut flat_frame, mut rotated_frame) = (
            render_composition(&flat, 600, 400, 0.5),
            render_composition(&rotated, 600, 400, 0.5),
        );
        let (fx0, fy0, fx1, fy1) = bright_bbox(&mut flat_frame).expect("flat ink");
        let (rx0, ry0, rx1, ry1) = bright_bbox(&mut rotated_frame).expect("rotated ink");
        let (flat_w, flat_h) = ((fx1 - fx0) as f64, (fy1 - fy0) as f64);
        let (rotated_w, rotated_h) = ((rx1 - rx0) as f64, (ry1 - ry0) as f64);
        assert!(
            rotated_h > rotated_w * 2.0 && flat_w > flat_h * 2.0,
            "a 90 degree title must stand upright: flat {flat_w}x{flat_h}, rotated {rotated_w}x{rotated_h}"
        );

        let card = black_card(200, 150);
        let stage = MotionStage::frame(600.0, 400.0);
        let segment = &flat.text_segments[0];
        let rotated_segment = &rotated.text_segments[0];
        let center = (
            (fx0 as f64 + fx1 as f64) * 0.5,
            (fy0 as f64 + fy1 as f64) * 0.5,
        );
        assert!(canvas_contains(&card, stage, segment, 0.5, center));
        assert!(canvas_contains(&card, stage, rotated_segment, 0.5, center));
        let along = (center.0 + flat_w * 0.4, center.1);
        let across = (center.0, center.1 + flat_w * 0.4);
        assert!(
            canvas_contains(&card, stage, segment, 0.5, along),
            "the flat title covers its own baseline"
        );
        assert!(
            !canvas_contains(&card, stage, rotated_segment, 0.5, along),
            "the rotated title must not cover the flat run's far end"
        );
        assert!(
            !canvas_contains(&card, stage, segment, 0.5, across),
            "the flat title must not cover a point below its line"
        );
        assert!(
            canvas_contains(&card, stage, rotated_segment, 0.5, across),
            "the rotated title covers what the flat one did not"
        );
        assert!(!canvas_contains(&card, stage, segment, 0.5, (60.0, 380.0)));
        assert!(!canvas_contains(
            &card,
            stage,
            rotated_segment,
            0.5,
            (60.0, 380.0)
        ));
    }

    #[test]
    fn wrapped_canvas_title_breaks_lines_and_hits_the_second_one() {
        let canvas = MotionTextCoordinateSpace::Canvas;
        let mut motion = title_motion(canvas, MotionTransform::default(), (0.5, 0.5), |_| {});
        motion.set_selected_text_value("a background headline wrapping over lines".into());
        motion.set_selected_text_wrap_width(0.45);

        let mut frame = render_composition(&motion, 600, 400, 0.5);
        let (left, top, right, bottom) = bright_bbox(&mut frame).expect("wrapped ink");
        let font = 0.06 * 400.0;
        let box_width = 0.45 * 600.0;
        assert!(
            (bottom - top) as f64 > font * 1.8,
            "wrapped copy must occupy more than one line: {:?}",
            (left, top, right, bottom)
        );
        assert!(
            (right - left) as f64 <= box_width + 2.0,
            "the paragraph box must bound the wrapped ink: {}",
            right - left
        );

        let card = black_card(200, 150);
        let stage = MotionStage::frame(600.0, 400.0);
        let segment = &motion.text_segments[0];
        let second_line = (
            (left as f64 + right as f64) * 0.5,
            top as f64 + (bottom - top) as f64 * 0.75,
        );
        assert!(canvas_contains(&card, stage, segment, 0.5, second_line));
        assert!(!canvas_contains(
            &card,
            stage,
            segment,
            0.5,
            (second_line.0, bottom as f64 + 12.0)
        ));
    }

    #[test]
    fn canvas_title_scales_with_the_composition_not_the_viewport() {
        let canvas = MotionTextCoordinateSpace::Canvas;
        let motion = title_motion(canvas, MotionTransform::default(), (0.3, 0.4), |_| {});
        let mut small = render_composition(&motion, 600, 400, 0.5);
        let mut large = render_composition(&motion, 1200, 800, 0.5);
        let (sx0, sy0, sx1, sy1) = bright_bbox(&mut small).expect("small ink");
        let (lx0, ly0, lx1, ly1) = bright_bbox(&mut large).expect("large ink");
        let height_ratio = (ly1 - ly0) as f64 / (sy1 - sy0) as f64;
        assert!(
            (height_ratio - 2.0).abs() < 0.2,
            "the title must scale with the composition, got {height_ratio}"
        );
        let small_center = (
            (sx0 + sx1) as f64 / 2.0 / 600.0,
            (sy0 + sy1) as f64 / 2.0 / 400.0,
        );
        let large_center = (
            (lx0 + lx1) as f64 / 2.0 / 1200.0,
            (ly0 + ly1) as f64 / 2.0 / 800.0,
        );
        assert!(
            (small_center.0 - large_center.0).abs() < 0.01
                && (small_center.1 - large_center.1).abs() < 0.01,
            "the title keeps its composition position: {small_center:?} vs {large_center:?}"
        );

        let card = black_card(200, 150);
        let segment = &motion.text_segments[0];
        let preview_stage = preview_stage(&card, 600.0, 400.0, &motion.appearance, &motion.frame);
        let (px, py) = motion_text_anchor_view_point(
            card_layout(
                &card,
                1.0,
                preview_stage,
                &MotionAppearance {
                    background_padding: 0.0,
                    background_fill_type: MotionBackgroundFillType::Color,
                    ..MotionAppearance::default()
                },
                None,
                MotionTransform::default(),
                (0.5, 0.5),
            ),
            segment,
        )
        .expect("preview anchor");
        let frame_stage = MotionStage::frame(600.0, 400.0);
        let (fx, fy) = motion_text_anchor_view_point(
            card_layout(
                &card,
                1.0,
                frame_stage,
                &MotionAppearance {
                    background_padding: 0.0,
                    background_fill_type: MotionBackgroundFillType::Color,
                    ..MotionAppearance::default()
                },
                None,
                MotionTransform::default(),
                (0.5, 0.5),
            ),
            segment,
        )
        .expect("frame anchor");
        let normalized = |stage: MotionStage, x: f64, y: f64| {
            (
                (x - (stage.center_x - stage.bounds_w / 2.0)) / stage.bounds_w,
                (y - (stage.center_y - stage.bounds_h / 2.0)) / stage.bounds_h,
            )
        };
        let preview_norm = normalized(preview_stage, px, py);
        let frame_norm = normalized(frame_stage, fx, fy);
        assert!((preview_norm.0 - frame_norm.0).abs() < 1e-9);
        assert!((preview_norm.1 - frame_norm.1).abs() < 1e-9);
    }

    #[test]
    fn typewriter_reveal_keeps_the_paragraph_metrics_stable() {
        let canvas = MotionTextCoordinateSpace::Canvas;
        let mut motion = title_motion(canvas, MotionTransform::default(), (0.5, 0.5), |motion| {
            motion.set_selected_text_value("AB\nCD".into());
            motion.set_selected_text_wrap_width(0.0);
            motion.set_selected_text_animation(MotionTextAnimation::Typewriter);
            motion.set_selected_text_scope(MotionTextScope::Character);
            motion.set_selected_text_typewriter_time(1.0);
        });
        motion.set_selected_text_alignment(MotionTextAlignment::Left);
        let font = 0.06 * 400.0;

        let mut first = render_composition(&motion, 600, 400, 0.25);
        let mut all = render_composition(&motion, 600, 400, 1.0);
        let (fx0, fy0, fx1, fy1) = bright_bbox(&mut first).expect("one revealed grapheme");
        let (ax0, ay0, ax1, ay1) = bright_bbox(&mut all).expect("full paragraph");
        assert!(
            ((fy1 - fy0) as f64) < font * 1.6,
            "one revealed grapheme draws one line, got {:?}",
            (fx0, fy0, fx1, fy1)
        );
        assert!(
            ((ay1 - ay0) as f64) > font * 1.8,
            "the full paragraph keeps both lines, got {:?}",
            (ax0, ay0, ax1, ay1)
        );
        assert_eq!(
            (fx0, fy0),
            (ax0, ay0),
            "revealing more text must not move the paragraph's first line"
        );
    }

    #[test]
    fn word_and_line_reveals_preserve_newlines_and_reveal_row_by_row() {
        let canvas = MotionTextCoordinateSpace::Canvas;
        let font = 0.06 * 400.0;

        let mut words = title_motion(canvas, MotionTransform::default(), (0.5, 0.5), |motion| {
            motion.set_selected_text_value("one two\nthree four".into());
            motion.set_selected_text_wrap_width(0.0);
            motion.set_selected_text_animation(MotionTextAnimation::Typewriter);
            motion.set_selected_text_scope(MotionTextScope::Word);
            motion.set_selected_text_typewriter_time(1.0);
        });
        words.set_selected_text_alignment(MotionTextAlignment::Left);
        let mut half = render_composition(&words, 600, 400, 0.5);
        let mut whole = render_composition(&words, 600, 400, 1.0);
        let (_, hy0, _, hy1) = bright_bbox(&mut half).expect("two revealed words");
        let (_, wy0, _, wy1) = bright_bbox(&mut whole).expect("four revealed words");
        assert!(
            ((hy1 - hy0) as f64) < font * 1.6,
            "two words stop at the newline, got {:?}",
            (hy0, hy1)
        );
        assert!(
            ((wy1 - wy0) as f64) > font * 1.8,
            "four words keep both lines, got {:?}",
            (wy0, wy1)
        );

        let mut lines = title_motion(canvas, MotionTransform::default(), (0.5, 0.5), |motion| {
            motion.set_selected_text_value("first line\nsecond line".into());
            motion.set_selected_text_wrap_width(0.0);
            motion.set_selected_text_animation(MotionTextAnimation::Typewriter);
            motion.set_selected_text_scope(MotionTextScope::Line);
            motion.set_selected_text_typewriter_time(1.0);
        });
        lines.set_selected_text_alignment(MotionTextAlignment::Left);
        let mut one_line = render_composition(&lines, 600, 400, 0.5);
        let mut both_lines = render_composition(&lines, 600, 400, 0.6);
        let (_, oy0, _, oy1) = bright_bbox(&mut one_line).expect("first line");
        let (_, by0, _, by1) = bright_bbox(&mut both_lines).expect("both lines");
        assert!(
            ((oy1 - oy0) as f64) < font * 1.6,
            "half the lines reveal the first row only, got {:?}",
            (oy0, oy1)
        );
        assert!(
            ((by1 - by0) as f64) > font * 1.8,
            "the last line lands with its own reveal step, got {:?}",
            (by0, by1)
        );
    }

    #[test]
    fn character_reveal_counts_graphemes_not_chars() {
        let text = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}x";
        assert_eq!(text.chars().count(), 6);
        let cluster = motion_text_revealed_bytes(text, MotionTextScope::Character, 0.5)
            .expect("half the graphemes");
        assert_eq!(
            cluster,
            "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}".len(),
            "a half reveal must not cut inside a grapheme cluster"
        );
        assert_eq!(
            motion_text_revealed_bytes(text, MotionTextScope::Character, 1.0),
            Some(text.len())
        );
        assert_eq!(
            motion_text_revealed_bytes(text, MotionTextScope::Character, 0.0),
            None
        );
    }

    #[test]
    fn typewriter_prefix_keeps_completed_lines_intact() {
        let canvas = MotionTextCoordinateSpace::Canvas;
        let text = "LINE ONE IS QUITE LONG\nshort";
        let motion = title_motion(canvas, MotionTransform::default(), (0.5, 0.5), |motion| {
            motion.set_selected_text_value(text.into());
            motion.set_selected_text_wrap_width(0.0);
            motion.set_selected_text_alignment(MotionTextAlignment::Left);
            motion.set_selected_text_animation(MotionTextAnimation::Typewriter);
            motion.set_selected_text_scope(MotionTextScope::Character);
            motion.set_selected_text_typewriter_time(1.0);
        });
        let revealed = 25.0 / 29.0;
        assert_eq!(
            motion_text_revealed_bytes(text, MotionTextScope::Character, revealed),
            Some(25),
            "the prefix must land one grapheme into the second line"
        );

        let mut prefix = render_composition(&motion, 600, 400, revealed);
        let mut whole = render_composition(&motion, 600, 400, 1.0);
        let prefix_lines = ink_lines(&mut prefix);
        let whole_lines = ink_lines(&mut whole);
        assert_eq!(
            prefix_lines.len(),
            2,
            "both lines stay painted: {prefix_lines:?}"
        );
        assert_eq!(whole_lines.len(), 2, "{whole_lines:?}");
        assert_eq!(
            (prefix_lines[0].0, prefix_lines[0].1),
            (whole_lines[0].0, whole_lines[0].1),
            "the finished first line must keep its full width while the short second line types"
        );
        assert!(
            prefix_lines[1].1 - prefix_lines[1].0 < whole_lines[1].1 - whole_lines[1].0,
            "the second line is still only a prefix: {:?} vs {:?}",
            prefix_lines[1],
            whole_lines[1]
        );
    }

    #[test]
    fn partial_opacity_attenuates_the_shadow() {
        let canvas = MotionTextCoordinateSpace::Canvas;
        let posed = MotionTransform {
            pos_x: -1.0,
            ..MotionTransform::default()
        };
        let darkest = |opacity: f64, shadow: bool| {
            let motion = title_motion(canvas, posed, (0.35, 0.4), |motion| {
                motion.set_selected_text_shadow(shadow);
                motion.set_selected_text_opacity(opacity);
            });
            let mut frame = render_composition(&motion, 600, 400, 0.5);
            frame.flush();
            let stride = frame.stride() as usize;
            let data = frame.data().unwrap();
            (0..400usize)
                .flat_map(|y| (0..300usize).map(move |x| (x, y)))
                .map(|(x, y)| data[y * stride + x * 4 + 2])
                .min()
                .expect("pixels")
        };
        let full = darkest(1.0, true);
        let half = darkest(0.5, true);
        let none = darkest(1.0, false);
        assert!(
            full < half && half < none,
            "shadow strength must follow the title's opacity: full {full}, half {half}, none {none}"
        );
    }

    #[test]
    fn invisible_and_fading_titles_are_not_drag_targets() {
        let card = black_card(200, 150);
        let stage = MotionStage::frame(600.0, 400.0);
        let canvas = MotionTextCoordinateSpace::Canvas;
        let mut motion = title_motion(canvas, MotionTransform::default(), (0.5, 0.5), |_| {});
        motion.set_selected_text_opacity(0.0);
        let mut frame = render_composition(&motion, 600, 400, 1.0);
        assert!(bright_bbox(&mut frame).is_none());
        let segment = &motion.text_segments[0];
        assert!(
            !motion_text_contains_view_point(
                card_layout(
                    &card,
                    1.0,
                    stage,
                    &MotionAppearance {
                        background_padding: 0.0,
                        background_fill_type: MotionBackgroundFillType::Color,
                        ..MotionAppearance::default()
                    },
                    None,
                    MotionTransform::default(),
                    (0.5, 0.5)
                ),
                segment,
                0.5,
                300.0,
                200.0,
            ),
            "a fully transparent title is not grabbable"
        );

        motion.set_selected_text_opacity(1.0);
        motion.set_selected_text_animation(MotionTextAnimation::Fade);
        motion.set_selected_text_transition_duration(1.0);
        motion.set_text_range(0, 0.0, 3.0);
        assert!(
            !canvas_contains(&card, stage, &motion.text_segments[0], 0.0, (300.0, 200.0)),
            "a fade that has not started is not grabbable"
        );
        assert!(
            canvas_contains(&card, stage, &motion.text_segments[0], 0.5, (300.0, 200.0)),
            "the title is grabbable once its fade is visible"
        );
    }

    #[test]
    fn auto_width_multiline_honours_alignment() {
        let canvas = MotionTextCoordinateSpace::Canvas;
        let line_extents = |alignment: MotionTextAlignment| {
            let motion = title_motion(canvas, MotionTransform::default(), (0.5, 0.5), |motion| {
                motion.set_selected_text_value("a much longer line\nshort".into());
                motion.set_selected_text_wrap_width(0.0);
                motion.set_selected_text_alignment(alignment);
            });
            let mut frame = render_composition(&motion, 600, 400, 0.5);
            let lines = ink_lines(&mut frame);
            assert_eq!(lines.len(), 2, "{alignment:?}: {lines:?}");
            lines
        };
        let left = line_extents(MotionTextAlignment::Left);
        let center = line_extents(MotionTextAlignment::Center);
        let right = line_extents(MotionTextAlignment::Right);

        assert!(
            (left[1].0 as f64 - left[0].0 as f64).abs() < 4.0,
            "left keeps the short line at the paragraph's left edge: {left:?}"
        );
        assert!(
            (right[1].1 as f64 - right[0].1 as f64).abs() < 4.0,
            "right puts the short line on the paragraph's right edge: {right:?}"
        );
        let centred = left[0].0 as f64 + (left[0].1 - left[0].0) as f64 / 2.0
            - (center[1].1 - center[1].0) as f64 / 2.0;
        assert!(
            (center[1].0 as f64 - centred).abs() < 4.0,
            "center straddles the paragraph: {center:?}"
        );
        assert!(
            left[1].0 < center[1].0 && center[1].0 < right[1].0,
            "the short line walks left, centre, right: {left:?} {center:?} {right:?}"
        );
    }

    #[test]
    fn reveal_follows_the_text_direction() {
        let canvas = MotionTextCoordinateSpace::Canvas;
        let reveal = |text: &str, family: Option<&str>| {
            let motion = title_motion(canvas, MotionTransform::default(), (0.5, 0.5), |motion| {
                motion.set_selected_text_value(text.into());
                if let Some(family) = family {
                    motion.set_selected_text_font_family(family.into());
                }
                motion.set_selected_text_wrap_width(0.0);
                motion.set_selected_text_alignment(MotionTextAlignment::Left);
                motion.set_selected_text_animation(MotionTextAnimation::Typewriter);
                motion.set_selected_text_scope(MotionTextScope::Character);
                motion.set_selected_text_typewriter_time(1.0);
            });
            let mut half = render_composition(&motion, 600, 400, 0.5);
            let mut whole = render_composition(&motion, 600, 400, 1.0);
            (
                bright_bbox(&mut half).expect("half revealed"),
                bright_bbox(&mut whole).expect("full text"),
            )
        };

        let (half, whole) = reveal("revealing in reading order", None);
        assert_eq!(
            half.0, whole.0,
            "a left-to-right title keeps its left edge: {half:?} vs {whole:?}"
        );
        assert!(
            half.2 < whole.2,
            "and grows to the right: {half:?} {whole:?}"
        );

        let arabic = "\u{645}\u{631}\u{62d}\u{628}\u{627}\u{20}\u{628}\u{627}\u{644}\u{639}\u{627}\u{644}\u{645}";
        let (half, whole) = reveal(arabic, Some("DejaVu Sans"));
        assert_eq!(
            half.2, whole.2,
            "a right-to-left title keeps its right edge: {half:?} vs {whole:?}"
        );
        assert!(
            half.0 > whole.0 + 8,
            "and grows to the left instead of always rightwards: {half:?} {whole:?}"
        );
    }

    #[test]
    fn invisible_title_leaks_no_shadow_or_outline() {
        let canvas = MotionTextCoordinateSpace::Canvas;
        let posed = MotionTransform {
            pos_x: -1.0,
            ..MotionTransform::default()
        };
        let transparent = title_motion(canvas, posed, (0.35, 0.4), |motion| {
            motion.set_selected_text_opacity(0.0);
        });
        let adorned = title_motion(canvas, posed, (0.35, 0.4), |motion| {
            motion.set_selected_text_shadow(true);
            motion.set_selected_text_outline_width(0.12);
            motion.set_selected_text_opacity(0.0);
        });
        let mut empty = title_motion(canvas, posed, (0.35, 0.4), |_| {});
        empty.text_segments.clear();

        let pixels = |motion: &MotionState| {
            let mut frame = render_composition(motion, 600, 400, 0.5);
            let bytes = frame.data().unwrap().to_vec();
            (frame, bytes)
        };
        let (mut adorned_frame, adorned_pixels) = pixels(&adorned);
        let (_, transparent_pixels) = pixels(&transparent);
        let (_, empty_pixels) = pixels(&empty);
        assert_eq!(
            adorned_pixels, transparent_pixels,
            "a transparent title must not paint its shadow or outline"
        );
        assert_eq!(
            adorned_pixels, empty_pixels,
            "a transparent title must leave the composition untouched"
        );
        assert!(bright_bbox(&mut adorned_frame).is_none());

        let mut typed = title_motion(canvas, posed, (0.35, 0.4), |motion| {
            motion.set_selected_text_shadow(true);
            motion.set_selected_text_outline_width(0.12);
            motion.set_selected_text_animation(MotionTextAnimation::Typewriter);
            motion.set_selected_text_scope(MotionTextScope::Character);
            motion.set_selected_text_typewriter_time(1.0);
        });
        typed.set_selected_text_value("HEADLINE".into());
        typed.set_selected_text_wrap_width(0.0);
        typed.set_selected_text_alignment(MotionTextAlignment::Left);
        let mut partial = render_composition(&typed, 600, 400, 0.3);
        let mut full = render_composition(&typed, 600, 400, 1.0);
        let (_, _, partial_right, _) = bright_bbox(&mut partial).expect("revealed prefix");
        let (fx0, fy0, full_right, fy1) = bright_bbox(&mut full).expect("full title");
        assert!(
            partial_right < full_right,
            "the unrevealed tail must not be painted: {partial_right} vs {full_right}"
        );
        let _ = fx0;
        let probe = (full_right as f64 - 4.0, (fy0 + fy1) as f64 * 0.5);
        let card = black_card(200, 150);
        let stage = MotionStage::frame(600.0, 400.0);
        let segment = &typed.text_segments[0];
        assert!(
            canvas_contains(&card, stage, segment, 1.0, probe),
            "the full title covers the tail of its own line"
        );
        assert!(
            !canvas_contains(&card, stage, segment, 0.3, probe),
            "an unrevealed glyph is not a drag target"
        );
    }

    #[test]
    fn text_entrance_duration_drives_slides_and_fades() {
        let mut motion = MotionState::default();
        let index = motion.add_text_at(0.0).expect("title clip");
        motion.set_text_range(index, 0.0, 3.0);
        motion.set_selected_text_animation(MotionTextAnimation::Fade);
        motion.set_selected_text_transition_duration(1.0);
        let slow = motion.text_segments[index].clone();
        let early = slow.sample(0.1).expect("entrance").alpha;
        let mid = slow.sample(0.5).expect("entrance").alpha;
        assert!(early > 0.0 && early < mid && mid < 1.0, "{early} {mid}");
        assert!((slow.sample(1.0).expect("entrance").alpha - 1.0).abs() < 1e-9);
        let fade = slow.sample(0.5).expect("entrance");
        assert!(fade.offset_x.abs() < 1e-9 && fade.offset_y.abs() < 1e-9);

        motion.set_selected_text_transition_duration(0.2);
        assert!(
            (motion.text_segments[index]
                .sample(0.5)
                .expect("entrance")
                .alpha
                - 1.0)
                .abs()
                < 1e-9,
            "a shorter entrance is already done at the same time"
        );

        motion.set_selected_text_animation(MotionTextAnimation::SlideFromLeft);
        motion.set_selected_text_transition_duration(1.0);
        let sliding = motion.text_segments[index].sample(0.5).expect("entrance");
        assert!(
            sliding.alpha > 0.0 && sliding.alpha < 1.0 && sliding.offset_x < 0.0,
            "the slide uses the same configured duration"
        );

        motion.set_selected_text_transition_duration(DEFAULT_MOTION_TEXT_TRANSITION_SECONDS);
        assert!(
            (motion.text_segments[index].transition_duration - 0.28).abs() < f64::EPSILON,
            "the recovered default entrance is unchanged"
        );

        // A configured entrance plays for its whole requested length inside a
        // clip that is just as long, instead of being cut to a third of it.
        let mut full_clip = MotionState::default();
        let index = full_clip.add_text_at(0.0).expect("title clip");
        full_clip.set_selected_text_animation(MotionTextAnimation::Fade);
        full_clip.set_selected_text_transition_duration(1.0);
        let segment = &full_clip.text_segments[index];
        assert!((segment.duration() - 1.0).abs() < 1e-9);
        let during = segment.sample(0.5).expect("entrance").alpha;
        assert!(
            during > 0.0 && during < 0.99,
            "the entrance is still running at half a second: {during}"
        );
        assert!(
            (segment.sample(1.0).expect("entrance").alpha - 1.0).abs() < 1e-9,
            "and has just finished by the end of the clip"
        );
    }
}
