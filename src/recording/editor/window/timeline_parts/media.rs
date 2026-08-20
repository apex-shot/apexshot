pub fn icon_tool_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-tool-icon");
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(16);
    button.set_child(Some(&icon));
    button.set_valign(Align::Center);
    button.set_tooltip_text(Some(tooltip));
    button
}

pub fn build_waveform_body(
    state: &Arc<Mutex<VideoEditState>>,
    waveform: Option<PathBuf>,
) -> DrawingArea {
    let area = DrawingArea::new();
    area.add_css_class("recording-editor-waveform");
    area.set_hexpand(true);
    area.set_size_request(0, 36);
    let pixbuf = waveform.and_then(|path| gtk4::gdk_pixbuf::Pixbuf::from_file(path).ok());
    let empty_label = if state.lock().unwrap().metadata.has_audio {
        "Waveform unavailable"
    } else {
        "No audio"
    };
    area.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| {
            draw_waveform_image(&state, pixbuf.as_ref(), empty_label, cr, width, height);
        }
    });
    area
}

pub fn load_track_pixbufs(paths: &[PathBuf]) -> Vec<gtk4::gdk_pixbuf::Pixbuf> {
    paths
        .iter()
        .filter_map(|path| gtk4::gdk_pixbuf::Pixbuf::from_file(path).ok())
        .collect()
}

pub fn paint_pixbuf(
    cr: &gtk4::cairo::Context,
    pixbuf: &gtk4::gdk_pixbuf::Pixbuf,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    let src_w = f64::from(pixbuf.width()).max(1.0);
    let src_h = f64::from(pixbuf.height()).max(1.0);
    cr.save().ok();
    cr.rectangle(x, y, width, height);
    cr.clip();
    cr.translate(x, y);
    cr.scale(width / src_w, height / src_h);
    cr.set_source_pixbuf(pixbuf, 0.0, 0.0);
    let _ = cr.paint();
    cr.restore().ok();
}

pub fn draw_filmstrip(
    state: &Arc<Mutex<VideoEditState>>,
    pixbufs: &[gtk4::gdk_pixbuf::Pixbuf],
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    let (clip_x0, clip_x1) = trim_span_x(&state, w);
    let clip_w = (clip_x1 - clip_x0).max(1.0);

    // Frames stay locked to the ruler. Only the kept range is painted, so
    // dragging a trim handle shrinks the clip instead of sliding the strip.
    cr.save().ok();
    rounded_rect(cr, clip_x0, 0.0, clip_w, h, 10.0);
    cr.clip();
    cr.set_source_rgb(0.12, 0.12, 0.12);
    cr.rectangle(clip_x0, 0.0, clip_w, h);
    let _ = cr.fill();

    let duration = state.metadata.duration_seconds.max(0.001);
    let count = pixbufs.len().max(12);
    let slice = duration / count as f64;
    for index in 0..count {
        let x0 = state.source_to_x(index as f64 * slice, w);
        let x1 = state.source_to_x((index + 1) as f64 * slice, w);
        if x1 < clip_x0 || x0 > clip_x1 {
            continue;
        }
        let dest_w = (x1 - x0).max(1.0);
        if let Some(pixbuf) = pixbufs.get(index) {
            paint_pixbuf(cr, pixbuf, x0, 0.0, dest_w, h);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.04);
            cr.rectangle(x0, 0.0, dest_w, h);
            let _ = cr.fill();
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.06);
            cr.rectangle(x0 + 0.5, 0.5, dest_w - 1.0, h - 1.0);
            let _ = cr.stroke();
        }
    }
    cr.restore().ok();
}

pub fn draw_waveform_image(
    state: &Arc<Mutex<VideoEditState>>,
    pixbuf: Option<&gtk4::gdk_pixbuf::Pixbuf>,
    empty_label: &str,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    cr.set_source_rgb(0.08, 0.08, 0.08);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();
    let (clip_x0, clip_x1) = trim_span_x(&state, w);
    let duration = state.metadata.duration_seconds.max(0.001);
    let x0 = state.source_to_x(0.0, w);
    let x1 = state.source_to_x(duration, w);
    let dest_w = (x1 - x0).max(1.0);
    cr.save().ok();
    rounded_rect(cr, clip_x0, 0.0, (clip_x1 - clip_x0).max(1.0), h, 8.0);
    cr.clip();
    if let Some(pixbuf) = pixbuf {
        paint_pixbuf(cr, pixbuf, x0, 0.0, dest_w, h);
        cr.restore().ok();
        return;
    }
    cr.restore().ok();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.38);
    cr.select_font_face(
        "sans-serif",
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(10.0);
    if let Ok(ext) = cr.text_extents(empty_label) {
        cr.move_to(
            ((w - ext.width()) / 2.0).max(4.0),
            h / 2.0 + ext.height() / 2.0,
        );
        let _ = cr.show_text(empty_label);
    }
}

