#[cfg(test)]
mod tests {
    use super::{
        draw_motion_backdrop, draw_motion_foreground, draw_motion_frame,
        motion_text_contains_view_point, paint_card_shadow, paint_image_background,
        view_point_to_motion_text_position, CardLayout, MotionStage,
    };
    use crate::recording::editor::model::{
        project_card_corners, MotionBackgroundFillType, MotionEffectTransformTiming, MotionState,
        MotionTransform, DEFAULT_MOTION_ZOOM,
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

    /// Motion starts with an empty track; tests add their own clip.
    fn motion_with_first_clip() -> MotionState {
        let mut motion = MotionState::default();
        motion
            .add_segment_at(0.0)
            .expect("a fresh track accepts a first move");
        motion
    }

    /// Export-style layout: the full frame with default padding.
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
            super::draw_transformed_card(
                &context,
                &card,
                MotionStage::frame(400.0, 300.0),
                MotionTransform::default(),
                (0.5, 0.5),
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
        let fit = super::motion_canvas_fit(100.0, 50.0, 0.0, stage.bounds_w, stage.bounds_h);
        let (cx, cy) = super::motion_card_center(100.0, 50.0, fit, stage, transform, (0.5, 0.5));
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
                super::draw_transformed_card(
                    &context,
                    &card,
                    stage,
                    transform,
                    (0.5, 0.5),
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
        let fit = super::motion_canvas_fit(100.0, 50.0, 0.0, stage.bounds_w, stage.bounds_h);
        let (cx, cy) = super::motion_card_center(100.0, 50.0, fit, stage, transform, (0.5, 0.5));
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
            super::draw_transformed_card(
                &context,
                &rounded,
                stage,
                transform,
                (0.5, 0.5),
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

        let layout = CardLayout::with_padding(
            &card,
            MotionStage::frame(200.0, 150.0),
            motion.sample(time),
            motion.zoom_anchor_at(time),
            0.0,
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
}
