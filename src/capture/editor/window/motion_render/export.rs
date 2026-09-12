pub fn export_motion_mp4(
    snapshot: &RgbaImage,
    motion: &MotionState,
    prefers_dark: bool,
    source_image: &Path,
) -> Result<PathBuf, String> {
    crate::recording::editor::ffmpeg::ensure_tools_available()
        .map_err(|error| error.to_string())?;

    let (out_w, out_h) = motion_frame_output_size(motion.frame.preset);
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
        let time = if frame_count <= 1 {
            0.0
        } else {
            motion.duration * f64::from(index) / f64::from(frame_count - 1)
        };
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

/// Output canvas for a Frame preset. The long edge keeps the established
/// 1920px budget; dimensions are rounded to even values because the MP4
/// encoder's yuv420p pixel format requires even sizes.
fn motion_frame_output_size(preset: MotionFramePreset) -> (i32, i32) {
    fn even(value: f64) -> i32 {
        ((value.round() as i32) / 2 * 2).max(2)
    }
    match preset.aspect() {
        None => (1920, 1080),
        Some(aspect) if aspect >= 1.0 => (1920, even(1920.0 / aspect)),
        Some(aspect) => (even(1080.0 * aspect), 1080),
    }
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
