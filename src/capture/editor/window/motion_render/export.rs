pub fn export_motion_mp4(
    snapshot: &RgbaImage,
    motion: &MotionState,
    prefers_dark: bool,
    source_image: &Path,
) -> Result<PathBuf, String> {
    crate::recording::editor::ffmpeg::ensure_tools_available()
        .map_err(|error| error.to_string())?;

    let source_w = f64::from(snapshot.width().max(1));
    let source_h = f64::from(snapshot.height().max(1));
    let canvas = motion_canvas(source_w, source_h, &motion.appearance, &motion.frame);
    let (out_w, out_h) = motion_frame_output_size(
        &motion.frame,
        canvas.canvas_width / canvas.canvas_height,
    );
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

    let config = crate::config::load_config().sanitized();
    let fallback = source_image
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = config.video_editor_export_dir(&fallback);
    let _ = fs::create_dir_all(&dir);
    let stem = source_image
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("ApexShot");
    let output = unique_motion_path(&dir, stem);

    let work = std::env::temp_dir().join(format!(
        "apexshot-motion-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&work).map_err(|error| error.to_string())?;

    let frame_count = ((motion.duration * f64::from(MOTION_EXPORT_FPS)).round() as u32).max(1);
    for index in 0..frame_count {
        let time = f64::from(index) / f64::from(MOTION_EXPORT_FPS);
        let mut surface = ImageSurface::create(Format::ARgb32, out_w, out_h)
            .map_err(|error| error.to_string())?;
        {
            let context = Context::new(&surface).map_err(|error| error.to_string())?;
            draw_motion_frame(
                &context,
                out_w,
                out_h,
                &card,
                motion,
                background_surface.as_ref(),
                watermark_surface.as_ref(),
                time,
                false,
                prefers_dark,
                false,
                1.0,
            );
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let image = {
            let data = surface.data().map_err(|error| error.to_string())?;
            crate::capture::editor::render::cairo_argb_to_rgba_image(
                out_w as u32,
                out_h as u32,
                stride,
                data.as_ref(),
            )
        };
        let frame_path = work.join(format!("frame_{index:04}.png"));
        image.save(&frame_path).map_err(|error| error.to_string())?;
    }

    let status = Command::new("ffmpeg")
        .args(["-y", "-framerate", &MOTION_EXPORT_FPS.to_string(), "-i"])
        .arg(work.join("frame_%04d.png"))
        .args([
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
        .status()
        .map_err(|error| error.to_string())?;
    let _ = fs::remove_dir_all(&work);
    if !status.success() {
        return Err("ffmpeg failed to encode the Motion video".into());
    }
    Ok(output)
}

/// Output canvas for a Frame preset. Delegates to the frame's own sizing so
/// manual W/H (Custom) and the full ratio grid share one budget rule.
fn motion_frame_output_size(
    frame: &crate::recording::editor::model::MotionFrame,
    source_aspect: f64,
) -> (i32, i32) {
    frame.output_size_for(source_aspect)
}

fn unique_motion_path(dir: &Path, stem: &str) -> PathBuf {
    let mut n = 0u32;
    loop {
        let name = if n == 0 {
            format!("{stem} Motion.mp4")
        } else {
            format!("{stem} Motion {n}.mp4")
        };
        let path = dir.join(name);
        if !path.exists() {
            return path;
        }
        n += 1;
    }
}
