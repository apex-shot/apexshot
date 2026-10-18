#[cfg(test)]
pub fn export_motion_mp4(
    snapshot: &RgbaImage,
    motion: &MotionState,
    prefers_dark: bool,
    destination: &Path,
) -> Result<PathBuf, String> {
    let card =
        crate::capture::editor::upscale::enhance(snapshot, motion.appearance.source_upscale)?;
    export_motion_card_mp4(
        &card,
        f64::from(motion.appearance.source_upscale.factor()),
        motion,
        prefers_dark,
        destination,
    )
}

pub(super) fn export_motion_card_mp4(
    snapshot: &RgbaImage,
    card_scale: f64,
    motion: &MotionState,
    prefers_dark: bool,
    destination: &Path,
) -> Result<PathBuf, String> {
    use std::io::Write;
    use std::process::Stdio;

    crate::studio_entitlement::ensure_export_allowed(
        crate::studio_entitlement::capabilities_for_motion_export(),
    )
    .map_err(|error| error.to_string())?;
    crate::recording::editor::ffmpeg::ensure_tools_available()
        .map_err(|error| error.to_string())?;

    if !card_scale.is_finite() || card_scale <= 0.0 {
        return Err("Motion has an invalid source pixel scale".into());
    }
    let source_w = f64::from(snapshot.width().max(1)) / card_scale;
    let source_h = f64::from(snapshot.height().max(1)) / card_scale;
    let (out_w, out_h) = motion_frame_output_size(source_w, source_h, motion);
    let Some(card) = crate::capture::editor::render::rgba_image_to_surface(snapshot) else {
        return Err("could not prepare the Motion still".into());
    };
    let background_surface = match motion.appearance.background_fill_type {
        MotionBackgroundFillType::Wallpaper => motion
            .appearance
            .wallpaper_image_name
            .as_deref()
            .and_then(load_motion_background_surface),
        MotionBackgroundFillType::Image => motion
            .appearance
            .custom_background_image
            .as_deref()
            .and_then(load_motion_background_surface),
        _ => None,
    };
    let watermark_surface = motion
        .watermark
        .image_file_name
        .as_deref()
        .and_then(load_motion_background_surface);

    // The caller owns destination choice (Save chooser), so the export writes
    // exactly the confirmed path instead of guessing a folder from the still.
    let output = destination.to_path_buf();
    if let Some(parent) = output.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        let _ = fs::create_dir_all(parent);
    }

    let backdrop =
        ImageSurface::create(Format::ARgb32, out_w, out_h).map_err(|error| error.to_string())?;
    {
        let context = Context::new(&backdrop).map_err(|error| error.to_string())?;
        draw_motion_backdrop(
            &context,
            out_w,
            out_h,
            motion,
            background_surface.as_ref(),
            false,
            prefers_dark,
            None,
        );
    }
    backdrop.flush();
    let mut surface =
        ImageSurface::create(Format::ARgb32, out_w, out_h).map_err(|error| error.to_string())?;
    let mut encoder = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &format!("{out_w}x{out_h}"),
            "-framerate",
            &MOTION_EXPORT_FPS.to_string(),
            "-i",
            "pipe:0",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "18",
            "-movflags",
            "+faststart",
        ])
        .arg(&output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let rendered = (|| -> Result<(), String> {
        let mut input = encoder
            .stdin
            .take()
            .ok_or_else(|| "could not open the Motion encoder input".to_string())?;
        let frame_count = ((motion.duration * f64::from(MOTION_EXPORT_FPS)).round() as u32).max(1);
        for index in 0..frame_count {
            let time = f64::from(index) / f64::from(MOTION_EXPORT_FPS);
            {
                let context = Context::new(&surface).map_err(|error| error.to_string())?;
                context.set_operator(Operator::Source);
                context
                    .set_source_surface(&backdrop, 0.0, 0.0)
                    .map_err(|error| error.to_string())?;
                context.paint().map_err(|error| error.to_string())?;
                context.set_operator(Operator::Over);
                draw_motion_output_foreground(
                    &context,
                    out_w,
                    out_h,
                    &card,
                    motion,
                    watermark_surface.as_ref(),
                    time,
                    card_scale,
                );
            }
            surface.flush();
            let stride = surface.stride() as usize;
            let data = surface.data().map_err(|error| error.to_string())?;
            let image = crate::capture::editor::render::cairo_argb_to_rgba_image(
                out_w as u32,
                out_h as u32,
                stride,
                data.as_ref(),
            );
            input
                .write_all(image.as_raw())
                .map_err(|error| format!("could not stream the Motion frame: {error}"))?;
        }
        Ok(())
    })();
    if let Err(error) = rendered {
        let _ = encoder.kill();
        let _ = encoder.wait();
        return Err(error);
    }
    let status = encoder.wait().map_err(|error| error.to_string())?;
    if !status.success() {
        return Err("ffmpeg failed to encode the Motion video".into());
    }
    Ok(output)
}

/// Output canvas shared by the Motion Frame controls and MP4 encoder.
pub(super) fn motion_frame_output_size(
    source_w: f64,
    source_h: f64,
    motion: &MotionState,
) -> (i32, i32) {
    let canvas = motion_canvas(source_w, source_h, &motion.appearance, &motion.frame);
    motion
        .frame
        .output_size_for(canvas.canvas_width / canvas.canvas_height)
}
