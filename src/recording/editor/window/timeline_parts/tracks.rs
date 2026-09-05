pub fn rail_divider() -> GtkBox {
    let divider = GtkBox::new(Orientation::Vertical, 0);
    divider.add_css_class("recording-editor-rail-divider");
    divider.set_halign(Align::Start);
    divider.set_valign(Align::Fill);
    divider.set_hexpand(false);
    divider.set_vexpand(true);
    divider.set_can_target(false);
    divider.set_size_request(1, -1);
    divider.set_margin_start(RAIL_HEADER_WIDTH);
    divider
}

#[derive(Clone)]
pub struct RailChrome {
    pub row: GtkBox,
    pub lock: Button,
    pub hide: Option<Button>,
    pub mute: Option<Button>,
    pub delete: Button,
}

pub fn track_row(header: Option<&RailChrome>, body: &impl IsA<gtk4::Widget>) -> (GtkBox, GtkBox) {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("recording-editor-track-row");
    row.set_hexpand(true);

    let header_box = if let Some(chrome) = header {
        chrome.row.clone()
    } else {
        let spacer = GtkBox::new(Orientation::Horizontal, 0);
        spacer.add_css_class("recording-editor-track-header");
        spacer.set_size_request(RAIL_HEADER_WIDTH, 16);
        spacer
    };
    header_box.set_hexpand(false);
    header_box.set_halign(Align::Start);
    header_box.set_valign(Align::Center);
    header_box.set_size_request(RAIL_HEADER_WIDTH, -1);
    row.append(&header_box);

    let body_wrap = GtkBox::new(Orientation::Horizontal, 0);
    body_wrap.add_css_class("recording-editor-track-body");
    body_wrap.set_hexpand(true);
    body_wrap.set_size_request(0, -1);
    body_wrap.set_overflow(gtk4::Overflow::Hidden);
    body_wrap.append(body);
    row.append(&body_wrap);
    (row, body_wrap)
}

pub fn queue_draw_tree(widget: &impl IsA<gtk4::Widget>) {
    widget.queue_draw();
    let mut child = widget.first_child();
    while let Some(next) = child {
        queue_draw_tree(&next);
        child = next.next_sibling();
    }
}

pub fn video_track_signature(state: &Arc<Mutex<VideoEditState>>) -> u64 {
    let guard = state.lock().unwrap();
    let mut hash = guard.video_tracks().len() as u64;
    for item in guard.video_tracks() {
        hash = hash
            .wrapping_mul(33)
            .wrapping_add(item.path.as_os_str().len() as u64);
    }
    hash
}

pub fn rebuild_extra_video_tracks(
    host: &GtkBox,
    extras: &[ProjectMedia],
    state: &Arc<Mutex<VideoEditState>>,
    estimate_label: &Label,
) {
    while let Some(child) = host.first_child() {
        host.remove(&child);
    }
    for item in extras {
        let header = build_rail_header(RailKind::Video);
        let strip = extra_video_strip(state, item);
        let (row, _) = track_row(Some(&header), &strip);
        row.add_css_class("recording-editor-empty-track-row");
        header.delete.set_sensitive(true);
        header.delete.set_tooltip_text(Some(&t("Remove video track")));
        header.delete.connect_clicked({
            let state = state.clone();
            let estimate_label = estimate_label.clone();
            let path = item.path.clone();
            let host = host.clone();
            move |_| {
                state
                    .lock()
                    .unwrap()
                    .remove_project_media(&path, ProjectMediaKind::Video);
                let extras: Vec<ProjectMedia> = state
                    .lock()
                    .unwrap()
                    .extra_video_tracks()
                    .into_iter()
                    .cloned()
                    .collect();
                rebuild_extra_video_tracks(&host, &extras, &state, &estimate_label);
                footer::update_estimate(&estimate_label, &state, false);
            }
        });
        if let Some(hide) = &header.hide {
            hide.set_sensitive(false);
        }
        if let Some(mute) = &header.mute {
            mute.set_sensitive(false);
        }
        header.lock.set_sensitive(false);
        host.append(&row);
    }
}

pub fn extra_video_strip(state: &Arc<Mutex<VideoEditState>>, item: &ProjectMedia) -> Overlay {
    let strip = Overlay::new();
    strip.add_css_class("recording-editor-thumbnail-strip");
    strip.add_css_class("recording-editor-empty-thumbnail-strip");
    strip.set_hexpand(true);
    strip.set_size_request(-1, 64);
    let clip = DrawingArea::new();
    clip.set_hexpand(true);
    clip.set_vexpand(true);
    let title = if item.display_name.trim().is_empty() {
        t("Video")
    } else {
        item.display_name.clone()
    };
    let duration = item.duration_seconds.unwrap_or(0.0);
    clip.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| {
            draw_extra_video_clip(&state, cr, width, height, duration, &title);
        }
    });
    strip.set_child(Some(&clip));
    strip
}

pub fn draw_extra_video_clip(
    state: &Arc<Mutex<VideoEditState>>,
    cr: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    duration: f64,
    title: &str,
) {
    let state = state.lock().unwrap();
    let w = width as f64;
    let h = height as f64;
    let end = if duration > 0.0 {
        duration
    } else {
        state.metadata.duration_seconds.max(0.001)
    };
    let x0 = state.time_to_x(0.0, w);
    let x1 = state.time_to_x(end, w);
    let clip_w = (x1 - x0).abs().max(72.0).min(w);
    cr.set_source_rgb(0.22, 0.22, 0.22);
    cr.rectangle(x0.max(0.0), 0.0, clip_w, h);
    let _ = cr.fill();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.42);
    cr.select_font_face(
        crate::typography::UI_FONT_FAMILY,
        gtk4::cairo::FontSlant::Normal,
        gtk4::cairo::FontWeight::Normal,
    );
    cr.set_font_size(11.0);
    if let Ok(ext) = cr.text_extents(title) {
        cr.move_to(x0.max(0.0) + 12.0, h / 2.0 + ext.height() / 2.0);
        let _ = cr.show_text(title);
    }
}

pub fn set_track_body_look(body: &GtkBox, faded: bool, locked: bool) {
    body.set_opacity(if faded { 0.3 } else { 1.0 });
    if faded {
        body.add_css_class("recording-editor-track-faded");
    } else {
        body.remove_css_class("recording-editor-track-faded");
    }
    if locked {
        body.add_css_class("recording-editor-track-locked");
    } else {
        body.remove_css_class("recording-editor-track-locked");
    }
}

pub fn rail_icon_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::new();
    button.set_has_frame(false);
    button.add_css_class("recording-editor-rail-action");
    button.set_tooltip_text(Some(tooltip));
    button.set_halign(Align::Center);
    button.set_valign(Align::Center);
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    button.set_child(Some(&icon));
    button
}

pub fn set_rail_icon(button: &Button, icon_name: &str) {
    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    button.set_child(Some(&icon));
}

pub fn build_rail_header(kind: RailKind) -> RailChrome {
    let row = GtkBox::new(Orientation::Horizontal, 4);
    row.add_css_class("recording-editor-track-header");
    row.set_hexpand(false);
    row.set_halign(Align::Start);
    row.set_valign(Align::Center);
    row.set_size_request(RAIL_HEADER_WIDTH, -1);

    let type_name = match kind {
        RailKind::Video => "camera-video-symbolic",
        RailKind::Audio => "audio-volume-high-symbolic",
        RailKind::Zoom => "zoom-in-symbolic",
    };
    let type_icon = Image::from_icon_name(type_name);
    type_icon.add_css_class("recording-editor-track-icon");
    type_icon.set_pixel_size(14);
    type_icon.set_halign(Align::Start);
    type_icon.set_valign(Align::Center);
    row.append(&type_icon);

    let lock = rail_icon_button("changes-allow-symbolic", &t("Lock track"));
    row.append(&lock);

    let hide = if matches!(kind, RailKind::Video | RailKind::Zoom) {
        let hide = rail_icon_button("view-reveal-symbolic", &t("Hide track"));
        row.append(&hide);
        Some(hide)
    } else {
        None
    };

    let mute = if matches!(kind, RailKind::Video | RailKind::Audio) {
        let mute = rail_icon_button("audio-volume-high-symbolic", &t("Mute"));
        row.append(&mute);
        Some(mute)
    } else {
        None
    };

    let delete = rail_icon_button("user-trash-symbolic", &t("Remove track"));
    row.append(&delete);

    RailChrome {
        row,
        lock,
        hide,
        mute,
        delete,
    }
}

pub fn apply_rail_state(
    state: &VideoEditState,
    media: &MediaFile,
    video: &RailChrome,
    audio: &RailChrome,
    zoom: &RailChrome,
    audio_row: &GtkBox,
    zoom_row: &GtkBox,
    video_body: &GtkBox,
    audio_body: &GtkBox,
    zoom_body: &GtkBox,
) {
    set_rail_icon(
        &video.lock,
        if state.video_locked {
            "changes-prevent-symbolic"
        } else {
            "changes-allow-symbolic"
        },
    );
    video.lock.set_tooltip_text(Some(&if state.video_locked {
        t("Unlock video")
    } else {
        t("Lock video")
    }));
    if let Some(hide) = &video.hide {
        set_rail_icon(
            hide,
            if state.video_hidden {
                "view-conceal-symbolic"
            } else {
                "view-reveal-symbolic"
            },
        );
        hide.set_tooltip_text(Some(&if state.video_hidden {
            t("Show video")
        } else {
            t("Hide video")
        }));
    }
    if let Some(mute) = &video.mute {
        let muted = state.is_muted();
        set_rail_icon(
            mute,
            if muted {
                "audio-volume-muted-symbolic"
            } else {
                "audio-volume-high-symbolic"
            },
        );
        mute.set_sensitive(state.has_audio_track() && !state.audio_locked);
        mute.set_tooltip_text(Some(&if muted { t("Unmute") } else { t("Mute") }));
    }
    video
        .delete
        .set_sensitive(!state.video_locked && state.video_has_edits());
    video
        .delete
        .set_tooltip_text(Some(&if state.video_has_edits() {
            t("Reset video edits")
        } else {
            t("No video edits to remove")
        }));

    audio_row.set_visible(state.has_audio_track());
    set_rail_icon(
        &audio.lock,
        if state.audio_locked {
            "changes-prevent-symbolic"
        } else {
            "changes-allow-symbolic"
        },
    );
    audio.lock.set_tooltip_text(Some(&if state.audio_locked {
        t("Unlock audio")
    } else {
        t("Lock audio")
    }));
    if let Some(mute) = &audio.mute {
        let muted = state.is_muted();
        set_rail_icon(
            mute,
            if muted {
                "audio-volume-muted-symbolic"
            } else {
                "audio-volume-high-symbolic"
            },
        );
        mute.set_sensitive(!state.audio_locked);
        mute.set_tooltip_text(Some(&if muted { t("Unmute") } else { t("Mute") }));
    }
    audio.delete.set_sensitive(!state.audio_locked);
    audio.delete.set_tooltip_text(Some(&t("Remove audio track")));

    zoom_row.set_visible(state.has_zoom_track());
    set_rail_icon(
        &zoom.lock,
        if state.zoom_locked {
            "changes-prevent-symbolic"
        } else {
            "changes-allow-symbolic"
        },
    );
    zoom.lock.set_tooltip_text(Some(&if state.zoom_locked {
        t("Unlock zoom")
    } else {
        t("Lock zoom")
    }));
    if let Some(hide) = &zoom.hide {
        set_rail_icon(
            hide,
            if state.zoom_hidden {
                "view-conceal-symbolic"
            } else {
                "view-reveal-symbolic"
            },
        );
        hide.set_tooltip_text(Some(&if state.zoom_hidden {
            t("Show zoom")
        } else {
            t("Hide zoom")
        }));
    }
    zoom.delete
        .set_sensitive(!state.zoom_locked && state.has_zoom_track());
    zoom.delete.set_tooltip_text(Some(&t("Remove zoom track")));

    set_track_body_look(video_body, state.video_hidden, state.video_locked);
    set_track_body_look(audio_body, state.is_muted(), state.audio_locked);
    set_track_body_look(zoom_body, state.zoom_hidden, state.zoom_locked);

    media.set_muted(state.is_muted() || !state.has_audio_track());
}

