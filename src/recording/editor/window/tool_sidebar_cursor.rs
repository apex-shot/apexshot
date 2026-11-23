struct CursorPanel {
    widget: GtkBox,
    refresh: Rc<dyn Fn()>,
}

fn build_cursor_panel(
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
    pause_playback: PausePlayback,
) -> CursorPanel {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("recording-editor-zoom-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("recording-editor-zoom-header");
    header.set_hexpand(true);
    let title = Label::new(Some(&t("Cursor")));
    title.add_css_class("recording-editor-zoom-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);

    let tabs = GtkBox::new(Orientation::Horizontal, 0);
    tabs.add_css_class("recording-editor-cursor-tabs");
    tabs.set_hexpand(true);
    tabs.set_homogeneous(true);
    let style_tab = cursor_tab_button(&t("Style"));
    let motion_tab = cursor_tab_button(&t("Follow"));
    let effects_tab = cursor_tab_button(&t("Effects"));
    motion_tab.set_group(Some(&style_tab));
    effects_tab.set_group(Some(&style_tab));
    style_tab.set_active(true);
    tabs.append(&style_tab);
    tabs.append(&motion_tab);
    tabs.append(&effects_tab);

    let style = build_cursor_style_tab(state.clone(), on_change.clone(), pause_playback.clone());
    let motion = build_cursor_motion_tab(state.clone(), on_change.clone(), pause_playback.clone());
    let effects = build_cursor_effects_tab(state.clone(), on_change.clone());
    style.widget.set_visible(true);
    motion.widget.set_visible(false);
    effects.widget.set_visible(false);

    let pages = GtkBox::new(Orientation::Vertical, 0);
    pages.set_hexpand(true);
    pages.append(&style.widget);
    pages.append(&motion.widget);
    pages.append(&effects.widget);

    let scroll = ScrolledWindow::new();
    scroll.add_css_class("recording-editor-zoom-scroll");
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&pages));

    let unavailable = GtkBox::new(Orientation::Vertical, 10);
    unavailable.add_css_class("recording-editor-cursor-unavailable");
    unavailable.set_halign(Align::Fill);
    unavailable.set_valign(Align::Fill);
    unavailable.set_hexpand(true);
    unavailable.set_vexpand(true);
    unavailable.set_visible(false);

    let unavailable_title = Label::new(Some(&t("Cursor unavailable")));
    unavailable_title.add_css_class("recording-editor-cursor-unavailable-title");
    unavailable_title.set_xalign(0.0);
    unavailable_title.set_wrap(true);

    let unavailable_detail = Label::new(None);
    unavailable_detail.add_css_class("recording-editor-cursor-unavailable-detail");
    unavailable_detail.set_xalign(0.0);
    unavailable_detail.set_wrap(true);
    unavailable_detail.set_max_width_chars(30);

    unavailable.append(&unavailable_title);
    unavailable.append(&unavailable_detail);

    let pointer_content = GtkBox::new(Orientation::Vertical, 0);
    pointer_content.set_hexpand(true);
    pointer_content.set_vexpand(true);
    pointer_content.append(&tabs);
    pointer_content.append(&scroll);

    let pointer_overlay = Overlay::new();
    pointer_overlay.add_css_class("recording-editor-cursor-overlay");
    pointer_overlay.set_hexpand(true);
    pointer_overlay.set_vexpand(true);
    pointer_overlay.set_child(Some(&pointer_content));
    pointer_overlay.add_overlay(&unavailable);

    let block_unavailable_input = GestureClick::new();
    block_unavailable_input.set_propagation_limit(gtk4::PropagationLimit::None);
    block_unavailable_input.set_propagation_phase(gtk4::PropagationPhase::Capture);
    unavailable.add_controller(block_unavailable_input);

    panel.append(&header);
    panel.append(&pointer_overlay);

    style_tab.connect_toggled({
        let style = style.widget.clone();
        let motion = motion.widget.clone();
        let effects = effects.widget.clone();
        move |button| {
            if button.is_active() {
                style.set_visible(true);
                motion.set_visible(false);
                effects.set_visible(false);
            }
        }
    });
    motion_tab.connect_toggled({
        let style = style.widget.clone();
        let motion = motion.widget.clone();
        let effects = effects.widget.clone();
        move |button| {
            if button.is_active() {
                style.set_visible(false);
                motion.set_visible(true);
                effects.set_visible(false);
            }
        }
    });
    effects_tab.connect_toggled({
        let style = style.widget.clone();
        let motion = motion.widget.clone();
        let effects = effects.widget.clone();
        move |button| {
            if button.is_active() {
                style.set_visible(false);
                motion.set_visible(false);
                effects.set_visible(true);
            }
        }
    });

    let refresh = {
        let refresh_style = style.refresh;
        let refresh_motion = motion.refresh;
        let refresh_effects = effects.refresh;
        let state = state.clone();
        let tabs = tabs.clone();
        let scroll = scroll.clone();
        let unavailable = unavailable.clone();
        let unavailable_detail = unavailable_detail.clone();
        Rc::new(move || {
            let guard = state.lock().unwrap();
            let can_style = guard
                .sidecar
                .as_ref()
                .is_some_and(|sidecar| sidecar.can_render_cursor_overlay());
            let inferred = guard.sidecar.as_ref().is_some_and(|sidecar| {
                sidecar.source
                    == crate::recording::editor::sidecar::PointerDataSource::InferredFromVideo
            });
            drop(guard);
            tabs.set_sensitive(can_style);
            scroll.set_sensitive(can_style);
            unavailable.set_visible(!can_style);
            let (message, tooltip) = if inferred {
                (
                    t("The cursor is baked into this imported video."),
                    t("Cursor replacement is unavailable. Its inferred path can still guide Auto Zoom."),
                )
            } else {
                (
                    t("No editable cursor data was found."),
                    t("Cursor styling requires pointer data from an ApexShot recording."),
                )
            };
            unavailable_detail.set_text(&message);
            unavailable.set_tooltip_text(Some(&tooltip));
            refresh_style();
            refresh_motion();
            refresh_effects();
        }) as Rc<dyn Fn()>
    };

    CursorPanel {
        widget: panel,
        refresh,
    }
}

fn cursor_tab_button(label: &str) -> ToggleButton {
    let button = ToggleButton::with_label(label);
    button.add_css_class("recording-editor-cursor-tab");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button
}

/// Let a segmented button's label wrap instead of setting the width floor.
///
/// A label that does not wrap reports its whole text as its minimum width, and
/// the segmented rows are homogeneous, so the longest segment would make both
/// buttons as wide as its text and push the sidebar wider than its column.
fn wrap_segment_label(button: &ToggleButton) {
    let Some(child) = button.child() else {
        return;
    };
    let Ok(label) = child.downcast::<Label>() else {
        return;
    };
    label.set_wrap(true);
    label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    label.set_justify(Justification::Center);
    label.set_xalign(0.5);
    label.set_max_width_chars(1);
}

fn bind_cursor_f64(
    slider: &FillSlider,
    syncing: Rc<Cell<bool>>,
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
    write: impl Fn(&mut crate::recording::editor::model::CursorSettings, f64) + 'static,
) {
    slider.connect_value_changed(move |scale| {
        if syncing.get() {
            return;
        }
        {
            let mut guard = state.lock().unwrap();
            write(&mut guard.cursor, scale.value());
            guard.sync_sidecar_cursor_filters();
        }
        on_change();
    });
}

/// Write the cursor settings once and push the shake filter and tail values
/// into the pointer sidecar before the editor refreshes.
fn write_cursor(
    state: &Arc<Mutex<VideoEditState>>,
    on_change: &Rc<dyn Fn()>,
    write: impl FnOnce(&mut crate::recording::editor::model::CursorSettings),
) {
    {
        let mut guard = state.lock().unwrap();
        write(&mut guard.cursor);
        guard.sync_sidecar_cursor_filters();
    }
    on_change();
}

fn build_cursor_style_tab(
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
    pause_playback: PausePlayback,
) -> CursorPanel {
    let body = GtkBox::new(Orientation::Vertical, 8);
    body.add_css_class("recording-editor-zoom-body");
    body.add_css_class("recording-editor-cursor-tab-body");
    body.set_hexpand(true);

    let hint = Label::new(Some(&t("Shown over the recording in preview and export")));
    hint.add_css_class("recording-editor-zoom-hint");
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    body.append(&hint);

    let style_section = sidebar_section(&t("Style"), true, false);

    let grid = Grid::new();
    grid.add_css_class("recording-editor-cursor-grid");
    grid.set_column_spacing(8);
    grid.set_row_spacing(8);
    grid.set_column_homogeneous(true);
    grid.set_hexpand(true);

    let cards: Vec<(CursorTheme, ToggleButton, DrawingArea)> = CursorTheme::ALL
        .iter()
        .enumerate()
        .map(|(index, &theme)| {
            let card = ToggleButton::new();
            card.add_css_class("recording-editor-cursor-card");
            card.set_has_frame(false);
            card.set_hexpand(true);
            let column = GtkBox::new(Orientation::Vertical, 6);
            column.set_halign(Align::Fill);
            let preview = DrawingArea::new();
            preview.add_css_class("recording-editor-cursor-preview");
            preview.set_content_width(72);
            preview.set_content_height(56);
            preview.set_hexpand(true);
            let preview_state = state.clone();
            preview.set_draw_func(move |_, cr, width, height| {
                let mut settings = preview_state.lock().unwrap().cursor;
                settings.theme = theme;
                cursor_sprite::draw_centered(cr, width as f64, height as f64, settings, 1.0);
            });
            let name = Label::new(Some(&t(theme.label())));
            name.add_css_class("recording-editor-cursor-card-label");
            name.set_xalign(0.5);
            column.append(&preview);
            column.append(&name);
            card.set_child(Some(&column));
            card.connect_clicked({
                let state = state.clone();
                let on_change = on_change.clone();
                move |button| {
                    if !button.is_active() {
                        return;
                    }
                    state.lock().unwrap().cursor.theme = theme;
                    on_change();
                }
            });
            grid.attach(&card, (index % 2) as i32, (index / 2) as i32, 1, 1);
            (theme, card, preview)
        })
        .collect();

    let first = cards[0].1.clone();
    for (index, (_, card, _)) in cards.iter().enumerate() {
        if index > 0 {
            card.set_group(Some(&first));
        }
    }
    let size_row = cursor_slider_row(&t("Size"));
    let shadow_row = cursor_slider_row(&t("Shadow"));
    size_row.scale.set_range(MIN_CURSOR_SIZE, MAX_CURSOR_SIZE);
    size_row.scale.set_increments(0.05, 0.25);
    shadow_row.scale.set_range(0.0, 1.0);
    shadow_row.scale.set_increments(0.05, 0.1);

    let default_row = GtkBox::new(Orientation::Horizontal, 8);
    default_row.add_css_class("recording-editor-zoom-classic");
    default_row.set_hexpand(true);
    let default_label = Label::new(Some(&t("Always use the default cursor")));
    default_label.add_css_class("recording-editor-zoom-classic-label");
    default_label.set_xalign(0.0);
    default_label.set_hexpand(true);
    default_label.set_valign(Align::Center);
    let always_default = Switch::new();
    always_default.add_css_class("recording-editor-zoom-switch");
    always_default.set_valign(Align::Center);
    always_default.set_halign(Align::End);
    default_row.append(&default_label);
    default_row.append(&always_default);

    style_section.body.append(&grid);
    style_section.body.append(&size_row.widget);
    style_section.body.append(&shadow_row.widget);
    style_section.body.append(&default_row);
    body.append(&style_section.root);

    let syncing = Rc::new(Cell::new(false));
    bind_cursor_f64(
        &size_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.size = value,
    );
    bind_cursor_f64(
        &shadow_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.shadow = value,
    );
    bind_switch(
        &always_default,
        syncing.clone(),
        pause_playback,
        on_change,
        {
            let state = state.clone();
            move |active| state.lock().unwrap().cursor.always_use_default = active
        },
    );

    let refresh = {
        let cards = cards;
        let size_scale = size_row.scale.clone();
        let shadow_scale = shadow_row.scale.clone();
        let always_default = always_default.clone();
        let syncing = syncing.clone();
        Rc::new(move || {
            let cursor = state.lock().unwrap().cursor;
            for (item, card, preview) in &cards {
                card.set_active(*item == cursor.theme);
                preview.queue_draw();
            }
            syncing.set(true);
            size_scale.set_value(cursor.size);
            shadow_scale.set_value(cursor.shadow);
            sync_switch(&always_default, cursor.always_use_default);
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };

    CursorPanel {
        widget: body,
        refresh,
    }
}

fn build_cursor_motion_tab(
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
    pause_playback: PausePlayback,
) -> CursorPanel {
    let body = GtkBox::new(Orientation::Vertical, 8);
    body.add_css_class("recording-editor-zoom-body");
    body.add_css_class("recording-editor-cursor-tab-body");
    body.set_hexpand(true);

    let movement_section = sidebar_section(&t("Movement"), true, false);

    let presets = GtkBox::new(Orientation::Horizontal, 8);
    presets.add_css_class("recording-editor-motion-preset-row");
    presets.set_hexpand(true);
    presets.set_homogeneous(true);
    let focused_btn = motion_preset_card(&t("Focused"), &t("Tighter, faster"));
    let smooth_btn = motion_preset_card(&t("Smooth"), &t("Softer follow"));
    presets.append(&focused_btn);
    presets.append(&smooth_btn);

    let smooth_row = cursor_slider_row(&t("Smoothing"));
    let speed_row = cursor_slider_row(&t("Speed"));
    smooth_row.scale.set_range(0.0, 1.0);
    smooth_row.scale.set_increments(0.05, 0.1);
    speed_row
        .scale
        .set_range(MIN_CURSOR_SPEED, MAX_CURSOR_SPEED);
    speed_row.scale.set_increments(0.05, 0.25);
    movement_section.body.append(&presets);
    movement_section.body.append(&smooth_row.widget);
    movement_section.body.append(&speed_row.widget);

    let shake_row = GtkBox::new(Orientation::Horizontal, 8);
    shake_row.add_css_class("recording-editor-zoom-classic");
    shake_row.set_hexpand(true);
    let shake_label = Label::new(Some(&t("Remove cursor shake")));
    shake_label.add_css_class("recording-editor-zoom-classic-label");
    shake_label.set_xalign(0.0);
    shake_label.set_hexpand(true);
    let shake_switch = Switch::new();
    shake_switch.add_css_class("recording-editor-zoom-switch");
    shake_switch.set_valign(Align::Center);
    shake_switch.set_halign(Align::End);
    shake_row.append(&shake_label);
    shake_row.append(&shake_switch);
    movement_section.body.append(&shake_row);

    let rotation_section = sidebar_section(&t("Rotation"), true, false);

    let rotation_modes = GtkBox::new(Orientation::Horizontal, 0);
    rotation_modes.add_css_class("recording-editor-cursor-segments");
    rotation_modes.set_hexpand(true);
    rotation_modes.set_homogeneous(true);
    let velocity_btn = ToggleButton::with_label(&t("Tilt with motion"));
    velocity_btn.add_css_class("recording-editor-cursor-segment");
    velocity_btn.set_has_frame(false);
    velocity_btn.set_hexpand(true);
    wrap_segment_label(&velocity_btn);
    let directional_btn = ToggleButton::with_label(&t("Point along movement"));
    directional_btn.add_css_class("recording-editor-cursor-segment");
    directional_btn.set_has_frame(false);
    directional_btn.set_hexpand(true);
    wrap_segment_label(&directional_btn);
    directional_btn.set_group(Some(&velocity_btn));
    rotation_modes.append(&velocity_btn);
    rotation_modes.append(&directional_btn);
    rotation_section.body.append(&rotation_modes);

    let tilt_row = cursor_slider_row(&t("Tilt amount"));
    tilt_row.scale.set_range(0.0, 1.0);
    tilt_row.scale.set_increments(0.05, 0.1);
    rotation_section.body.append(&tilt_row.widget);

    let angle_slider =
        FillSlider::new_with_value_text(&t("Base angle"), |value, _, _| format!("{value:.0}°"));
    angle_slider.set_range(-180.0, 180.0);
    angle_slider.set_increments(5.0, 45.0);
    rotation_section.body.append(&angle_slider.widget());

    let visibility_section = sidebar_section(&t("Visibility"), true, false);

    // The whole-edit hide: one switch that takes the rendered cursor out of
    // the preview and the export, with no timeline range to place or manage.
    let hidden_row = GtkBox::new(Orientation::Horizontal, 8);
    hidden_row.add_css_class("recording-editor-zoom-classic");
    hidden_row.set_hexpand(true);
    let hidden_label = Label::new(Some(&t("Hide cursor")));
    hidden_label.add_css_class("recording-editor-zoom-classic-label");
    hidden_label.set_xalign(0.0);
    hidden_label.set_hexpand(true);
    let hidden_switch = Switch::new();
    hidden_switch.add_css_class("recording-editor-zoom-switch");
    hidden_switch.set_valign(Align::Center);
    hidden_switch.set_halign(Align::End);
    hidden_row.append(&hidden_label);
    hidden_row.append(&hidden_switch);

    let idle_row = GtkBox::new(Orientation::Horizontal, 8);
    idle_row.add_css_class("recording-editor-zoom-classic");
    idle_row.set_hexpand(true);
    let idle_label = Label::new(Some(&t("Hide when idle")));
    idle_label.add_css_class("recording-editor-zoom-classic-label");
    idle_label.set_xalign(0.0);
    idle_label.set_hexpand(true);
    let idle_switch = Switch::new();
    idle_switch.add_css_class("recording-editor-zoom-switch");
    idle_switch.set_valign(Align::Center);
    idle_switch.set_halign(Align::End);
    idle_row.append(&idle_label);
    idle_row.append(&idle_switch);

    let idle_delay_row = cursor_slider_row(&t("Idle delay"));
    idle_delay_row.scale.set_range(120.0, 5000.0);
    idle_delay_row.scale.set_increments(40.0, 200.0);
    visibility_section.body.append(&hidden_row);
    visibility_section.body.append(&idle_row);
    visibility_section.body.append(&idle_delay_row.widget);

    let loop_row = GtkBox::new(Orientation::Horizontal, 8);
    loop_row.add_css_class("recording-editor-zoom-classic");
    loop_row.set_hexpand(true);
    let loop_label = Label::new(Some(&t("Loop cursor position")));
    loop_label.add_css_class("recording-editor-zoom-classic-label");
    loop_label.set_xalign(0.0);
    loop_label.set_hexpand(true);
    let loop_switch = Switch::new();
    loop_switch.add_css_class("recording-editor-zoom-switch");
    loop_switch.set_valign(Align::Center);
    loop_switch.set_halign(Align::End);
    loop_row.append(&loop_label);
    loop_row.append(&loop_switch);
    visibility_section.body.append(&loop_row);

    let loop_delay_row = FillSlider::new_with_value_text(&t("Loop before end"), |value, _, _| {
        format!("{:.1}s", value / 1000.0)
    });
    loop_delay_row.set_range(1000.0, 4000.0);
    loop_delay_row.set_increments(50.0, 250.0);
    visibility_section.body.append(&loop_delay_row.widget());

    let stop_row = FillSlider::new_with_value_text(&t("Stop at the end"), |value, _, _| {
        format!("{:.1}s", value / 1000.0)
    });
    stop_row.set_range(0.0, 5000.0);
    stop_row.set_increments(50.0, 250.0);
    visibility_section.body.append(&stop_row.widget());

    body.append(&movement_section.root);
    body.append(&rotation_section.root);
    body.append(&visibility_section.root);

    let syncing = Rc::new(Cell::new(false));
    focused_btn.connect_clicked({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |_| {
            if syncing.get() {
                return;
            }
            state
                .lock()
                .unwrap()
                .cursor
                .apply_motion_preset(CursorMotionStyle::Focused);
            on_change();
        }
    });
    smooth_btn.connect_clicked({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |_| {
            if syncing.get() {
                return;
            }
            state
                .lock()
                .unwrap()
                .cursor
                .apply_motion_preset(CursorMotionStyle::Smooth);
            on_change();
        }
    });
    bind_cursor_f64(
        &smooth_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.smooth = value,
    );
    bind_cursor_f64(
        &speed_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.speed = value,
    );
    bind_cursor_f64(
        &idle_delay_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.idle_ms = value,
    );
    bind_switch(
        &hidden_switch,
        syncing.clone(),
        pause_playback.clone(),
        on_change.clone(),
        {
            let state = state.clone();
            move |active| state.lock().unwrap().cursor.hidden = active
        },
    );
    bind_switch(
        &idle_switch,
        syncing.clone(),
        pause_playback.clone(),
        on_change.clone(),
        {
            let state = state.clone();
            move |active| state.lock().unwrap().cursor.hide_idle = active
        },
    );
    velocity_btn.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            write_cursor(&state, &on_change, |cursor| {
                cursor.rotation_mode = CursorRotationMode::Velocity
            });
        }
    });
    directional_btn.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            write_cursor(&state, &on_change, |cursor| {
                cursor.rotation_mode = CursorRotationMode::Directional
            });
        }
    });
    bind_cursor_f64(
        &tilt_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.rotate_on_x_movement_ratio = value,
    );
    bind_cursor_f64(
        &angle_slider,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.base_rotation_deg = value,
    );
    bind_switch(
        &shake_switch,
        syncing.clone(),
        pause_playback.clone(),
        on_change.clone(),
        {
            let state = state.clone();
            // The studied panel turns the filter off by writing zero and back
            // on at its default threshold.
            move |active| {
                let mut guard = state.lock().unwrap();
                guard.cursor.remove_shake_threshold = if active {
                    crate::recording::editor::cursor_shake::DEFAULT_REMOVE_SHAKE_THRESHOLD
                } else {
                    0.0
                };
                guard.sync_sidecar_cursor_filters();
            }
        },
    );
    bind_switch(
        &loop_switch,
        syncing.clone(),
        pause_playback.clone(),
        on_change.clone(),
        {
            let state = state.clone();
            // The studied panel turns the loop on one second before the end.
            move |active| {
                let mut guard = state.lock().unwrap();
                guard.cursor.loop_position_before_end_ms = if active { Some(1000.0) } else { None };
                guard.sync_sidecar_cursor_filters();
            }
        },
    );
    bind_cursor_f64(
        &loop_delay_row,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| {
            if cursor.loop_position_before_end_ms.is_some() {
                cursor.loop_position_before_end_ms = Some(value);
            }
        },
    );
    bind_cursor_f64(
        &stop_row,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.stop_movement_in_last_part_ms = value,
    );

    let refresh = {
        let focused_btn = focused_btn.clone();
        let smooth_btn = smooth_btn.clone();
        let smooth_scale = smooth_row.scale.clone();
        let speed_scale = speed_row.scale.clone();
        let hidden_switch = hidden_switch.clone();
        let idle_switch = idle_switch.clone();
        let idle_delay_scale = idle_delay_row.scale.clone();
        let velocity_btn = velocity_btn.clone();
        let directional_btn = directional_btn.clone();
        let tilt_scale = tilt_row.scale.clone();
        let angle_slider = angle_slider.clone();
        let shake_switch = shake_switch.clone();
        let loop_switch = loop_switch.clone();
        let loop_delay_scale = loop_delay_row.clone();
        let stop_scale = stop_row.clone();
        let syncing = syncing.clone();
        Rc::new(move || {
            let cursor = state.lock().unwrap().cursor;
            syncing.set(true);
            match cursor.matching_motion_preset() {
                Some(CursorMotionStyle::Focused) => {
                    focused_btn.set_active(true);
                    smooth_btn.set_active(false);
                }
                Some(CursorMotionStyle::Smooth) => {
                    focused_btn.set_active(false);
                    smooth_btn.set_active(true);
                }
                None => {
                    focused_btn.set_active(false);
                    smooth_btn.set_active(false);
                }
            }
            smooth_scale.set_value(cursor.smooth);
            speed_scale.set_value(cursor.speed);
            sync_switch(&hidden_switch, cursor.hidden);
            sync_switch(&idle_switch, cursor.hide_idle);
            idle_delay_scale.set_value(cursor.idle_ms);
            idle_delay_scale.set_sensitive(cursor.hide_idle);
            velocity_btn.set_active(cursor.rotation_mode == CursorRotationMode::Velocity);
            directional_btn.set_active(cursor.rotation_mode == CursorRotationMode::Directional);
            tilt_scale.set_value(cursor.rotate_on_x_movement_ratio);
            // Tilt is the velocity mode's setting; pointing along the movement
            // derives its angle from the heading instead.
            tilt_scale.set_sensitive(cursor.rotation_mode == CursorRotationMode::Velocity);
            angle_slider.set_value(cursor.base_rotation_deg);
            sync_switch(&shake_switch, cursor.remove_shake_threshold > 0.0);
            sync_switch(&loop_switch, cursor.loop_position_before_end_ms.is_some());
            loop_delay_scale.set_value(cursor.loop_position_before_end_ms.unwrap_or(1000.0));
            loop_delay_scale.set_sensitive(cursor.loop_position_before_end_ms.is_some());
            stop_scale.set_value(cursor.stop_movement_in_last_part_ms);
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };

    CursorPanel {
        widget: body,
        refresh,
    }
}

fn motion_preset_card(title: &str, hint: &str) -> ToggleButton {
    let card = ToggleButton::new();
    card.add_css_class("recording-editor-motion-preset");
    card.set_has_frame(false);
    card.set_hexpand(true);
    let column = GtkBox::new(Orientation::Vertical, 2);
    column.set_halign(Align::Center);
    column.set_valign(Align::Center);
    let name = Label::new(Some(title));
    name.add_css_class("recording-editor-motion-preset-label");
    name.set_xalign(0.5);
    let detail = Label::new(Some(hint));
    detail.add_css_class("recording-editor-motion-preset-hint");
    detail.set_xalign(0.5);
    column.append(&name);
    column.append(&detail);
    card.set_child(Some(&column));
    card
}

fn build_cursor_effects_tab(
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) -> CursorPanel {
    let body = GtkBox::new(Orientation::Vertical, 8);
    body.add_css_class("recording-editor-zoom-body");
    body.add_css_class("recording-editor-cursor-tab-body");
    body.set_hexpand(true);

    let live = DrawingArea::new();
    live.add_css_class("recording-editor-click-live-preview");
    live.set_content_width(240);
    live.set_content_height(88);
    live.set_hexpand(true);
    let anim_t = Rc::new(Cell::new(0.0));
    live.set_draw_func({
        let state = state.clone();
        let anim_t = anim_t.clone();
        move |widget, cr, width, height| {
            let cursor = state.lock().unwrap().cursor;
            draw_click_live_preview(
                widget,
                cr,
                width as f64,
                height as f64,
                cursor,
                anim_t.get(),
            );
        }
    });
    {
        let preview = live.downgrade();
        let anim_t = anim_t.clone();
        let state = state.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            let Some(preview) = preview.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if preview.is_mapped() {
                let duration = state.lock().unwrap().cursor.click_window_seconds().max(0.2);
                anim_t.set((anim_t.get() + 0.016) % (duration * 1.35));
                preview.queue_draw();
            }
            glib::ControlFlow::Continue
        });
    }
    body.append(&live);

    let clicks_section = sidebar_section(&t("Clicks"), true, false);
    let click_row = GtkBox::new(Orientation::Horizontal, 6);
    click_row.add_css_class("recording-editor-click-effect-row");
    click_row.set_hexpand(true);
    click_row.set_homogeneous(true);
    let none_btn = click_effect_card(ClickEffect::None);
    let ripple_btn = click_effect_card(ClickEffect::Ripple);
    let circle_btn = click_effect_card(ClickEffect::Circle);
    ripple_btn.set_group(Some(&none_btn));
    circle_btn.set_group(Some(&none_btn));
    click_row.append(&none_btn);
    click_row.append(&ripple_btn);
    click_row.append(&circle_btn);
    clicks_section.body.append(&click_row);

    let syncing = Rc::new(Cell::new(false));
    none_btn.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            state.lock().unwrap().cursor.click_effect = ClickEffect::None;
            on_change();
        }
    });
    ripple_btn.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            state.lock().unwrap().cursor.click_effect = ClickEffect::Ripple;
            on_change();
        }
    });
    circle_btn.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            state.lock().unwrap().cursor.click_effect = ClickEffect::Circle;
            on_change();
        }
    });

    let sound_section = sidebar_section(&t("Sound"), true, false);

    let sound_row = GtkBox::new(Orientation::Horizontal, 6);
    sound_row.add_css_class("recording-editor-click-effect-row");
    sound_row.set_hexpand(true);
    sound_row.set_homogeneous(true);
    let sound_off = click_sound_card(&t("Off"));
    let sound_soft = click_sound_card(&t("Soft"));
    sound_soft.set_group(Some(&sound_off));
    sound_row.append(&sound_off);
    sound_row.append(&sound_soft);
    sound_section.body.append(&sound_row);

    sound_off.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |card| {
            if syncing.get() || !card.is_active() {
                return;
            }
            state.lock().unwrap().sound.click_sound = None;
            on_change();
        }
    });
    sound_soft.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |card| {
            if syncing.get() || !card.is_active() {
                return;
            }
            state.lock().unwrap().sound.click_sound = Some("soft".to_owned());
            on_change();
        }
    });

    let volume =
        FillSlider::new_with_value_text(&t("Volume"), |value, _, _| format!("{value:.0}%"));
    volume.set_range(0.0, 100.0);
    volume.set_increments(1.0, 5.0);
    volume.connect_value_changed({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |slider| {
            if syncing.get() {
                return;
            }
            state.lock().unwrap().sound.click_sound_volume = slider.value() / 100.0;
            on_change();
        }
    });

    let preview = Button::with_label(&t("Preview"));
    preview.add_css_class("recording-editor-secondary-button");
    preview.set_valign(Align::Center);
    preview.set_halign(Align::End);
    preview.connect_clicked({
        let state = state.clone();
        move |_| {
            let volume = state.lock().unwrap().sound.click_sound_volume;
            crate::recording::editor::sound::preview_click(volume);
        }
    });

    let volume_row = GtkBox::new(Orientation::Horizontal, 8);
    volume_row.set_hexpand(true);
    volume.widget().set_hexpand(true);
    volume_row.append(&volume.widget());
    volume_row.append(&preview);
    sound_section.body.append(&volume_row);

    body.append(&clicks_section.root);
    body.append(&sound_section.root);

    let refresh = {
        let live = live.clone();
        let none_btn = none_btn.clone();
        let ripple_btn = ripple_btn.clone();
        let circle_btn = circle_btn.clone();
        let sound_off = sound_off.clone();
        let sound_soft = sound_soft.clone();
        let volume = volume.clone();
        let preview = preview.clone();
        let syncing = syncing.clone();
        Rc::new(move || {
            let guard = state.lock().unwrap();
            let cursor = guard.cursor;
            let click_sound = guard.sound.click_sound.clone();
            let click_volume = guard.sound.click_sound_volume;
            drop(guard);
            syncing.set(true);
            match cursor.click_effect {
                ClickEffect::None => none_btn.set_active(true),
                ClickEffect::Ripple => ripple_btn.set_active(true),
                ClickEffect::Circle => circle_btn.set_active(true),
                // Legacy Spotlight/Echo are no longer offered. Leave the
                // picker unselected so the stored value is not silently
                // changed; the effect still renders from the saved value.
                ClickEffect::Spotlight | ClickEffect::Echo => {}
            }
            let click_on = click_sound.is_some();
            if click_on {
                sound_soft.set_active(true);
            } else {
                sound_off.set_active(true);
            }
            volume.set_value(click_volume * 100.0);
            volume.set_sensitive(click_on);
            preview.set_sensitive(click_on);
            live.queue_draw();
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };

    CursorPanel {
        widget: body,
        refresh,
    }
}

fn draw_color_chip(
    cr: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    color: (u8, u8, u8),
    r: f64,
) {
    fill_slider_rounded_rect(
        cr,
        0.5,
        0.5,
        (width - 1.0).max(1.0),
        (height - 1.0).max(1.0),
        r,
    );
    cr.set_source_rgb(
        color.0 as f64 / 255.0,
        color.1 as f64 / 255.0,
        color.2 as f64 / 255.0,
    );
    let _ = cr.fill_preserve();
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.28);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
}

fn draw_click_live_preview(
    widget: &DrawingArea,
    cr: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    settings: crate::recording::editor::model::CursorSettings,
    t: f64,
) {
    let settings = settings.clamped();
    let light = widget_is_light(widget);
    fill_slider_rounded_rect(cr, 0.0, 0.0, width, height, 10.0);
    if light {
        cr.set_source_rgba(0.11, 0.13, 0.16, 0.08);
    } else {
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.05);
    }
    let _ = cr.fill();
    let (hotspot_x, hotspot_y) = cursor_sprite::centered_preview_hotspot(width, height, settings);
    let duration = settings.click_window_seconds().max(0.2);
    let local = t % (duration * 1.35);
    if local <= duration {
        cursor_sprite::draw_click(
            cr,
            hotspot_x,
            hotspot_y,
            (local / duration).clamp(0.0, 1.0),
            settings,
            1.0,
            width,
        );
    }
    cursor_sprite::draw_centered(cr, width, height, settings, 0.95);
}

struct CursorSliderRow {
    widget: DrawingArea,
    scale: FillSlider,
}

fn click_effect_card(effect: ClickEffect) -> ToggleButton {
    let card = ToggleButton::new();
    card.add_css_class("recording-editor-click-effect-card");
    card.set_has_frame(false);
    card.set_hexpand(true);
    let column = GtkBox::new(Orientation::Vertical, 4);
    column.set_halign(Align::Center);
    column.set_valign(Align::Center);
    let preview = DrawingArea::new();
    preview.set_content_width(40);
    preview.set_content_height(30);
    preview.set_can_target(false);
    preview.set_draw_func(move |widget, cr, width, height| {
        draw_click_effect_icon(widget, cr, width as f64, height as f64, effect);
    });
    column.append(&preview);
    let name = Label::new(Some(&t(effect.label())));
    name.add_css_class("recording-editor-click-effect-label");
    name.set_xalign(0.5);
    column.append(&name);
    card.set_child(Some(&column));
    card
}

/// A click-sound card: the same surface as an effect card, with the label
/// doing the work because the sound has no shape to draw.
fn click_sound_card(label: &str) -> ToggleButton {
    let card = ToggleButton::new();
    card.add_css_class("recording-editor-click-effect-card");
    card.set_has_frame(false);
    card.set_hexpand(true);
    let name = Label::new(Some(label));
    name.add_css_class("recording-editor-click-effect-label");
    name.set_xalign(0.5);
    name.set_valign(Align::Center);
    card.set_child(Some(&name));
    card
}

fn draw_click_effect_icon(
    widget: &DrawingArea,
    cr: &gtk4::cairo::Context,
    width: f64,
    height: f64,
    effect: ClickEffect,
) {
    let cx = width / 2.0;
    let cy = height / 2.0;
    let light = widget_is_light(widget);
    let (r, g, b) = if light {
        (0.15, 0.16, 0.18)
    } else {
        (1.0, 1.0, 1.0)
    };
    cr.set_line_cap(gtk4::cairo::LineCap::Round);
    cr.set_line_join(gtk4::cairo::LineJoin::Round);
    let s = width.min(height) / 48.0;
    match effect {
        ClickEffect::None => {
            let s = width.min(height) / 40.0;
            cr.set_line_width(1.8 * s);
            cr.set_source_rgba(r, g, b, 0.75);
            cr.arc(cx, cy, 11.5 * s, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            cr.set_line_width(2.2 * s);
            cr.set_source_rgba(r, g, b, 0.92);
            cr.move_to(cx - 7.5 * s, cy + 7.5 * s);
            cr.line_to(cx + 7.5 * s, cy - 7.5 * s);
            let _ = cr.stroke();
        }
        ClickEffect::Spotlight => {
            cr.set_line_width(1.5 * s);
            cr.set_source_rgba(r, g, b, 0.3);
            cr.arc(cx, cy, 13.5 * s, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            cr.set_line_width(1.7 * s);
            cr.set_source_rgba(r, g, b, 0.56);
            cr.arc(cx, cy, 9.75 * s, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
        }
        ClickEffect::Ripple => {
            cr.set_line_width(2.0 * s);
            cr.set_source_rgba(r, g, b, 0.72);
            cr.arc(cx, cy, 13.0 * s, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
        }
        ClickEffect::Circle => {
            cr.set_source_rgba(r, g, b, 0.22);
            cr.arc(cx, cy, 12.0 * s, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            cr.set_line_width(1.6 * s);
            cr.set_source_rgba(r, g, b, 0.72);
            cr.arc(cx, cy, 12.0 * s, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
        }
        ClickEffect::Echo => {
            cr.set_line_width(1.8 * s);
            cr.set_source_rgba(r, g, b, 0.72);
            cr.arc(cx, cy, 9.0 * s, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            cr.set_line_width(1.5 * s);
            cr.set_source_rgba(r, g, b, 0.4);
            cr.arc(cx, cy, 14.5 * s, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            cr.set_source_rgba(r, g, b, 0.22);
            cr.arc(cx, cy, 4.25 * s, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }
    }
}

fn cursor_slider_row(label: &str) -> CursorSliderRow {
    let scale = FillSlider::new(label);
    CursorSliderRow {
        widget: scale.area.clone(),
        scale,
    }
}

#[derive(Clone)]
pub(crate) struct FillSlider {
    area: DrawingArea,
    value: Rc<Cell<f64>>,
    min: Rc<Cell<f64>>,
    max: Rc<Cell<f64>>,
    step: Rc<Cell<f64>>,
    enabled: Rc<Cell<bool>>,
    hovered: Rc<Cell<bool>>,
    dragging: Rc<Cell<bool>>,
    logarithmic: Rc<Cell<bool>>,
    listeners: Rc<RefCell<Vec<Rc<dyn Fn(&FillSlider)>>>>,
    drag_started_listeners: Rc<RefCell<Vec<Rc<dyn Fn()>>>>,
    drag_ended_listeners: Rc<RefCell<Vec<Rc<dyn Fn()>>>>,
    value_text: Rc<dyn Fn(f64, f64, f64) -> String>,
}

impl FillSlider {
    pub(crate) fn new(label: &str) -> Self {
        Self::new_with_value_text(label, |value, min, max| {
            format!(
                "{:.0}",
                ((value - min) / (max - min).max(1e-9) * 100.0).round()
            )
        })
    }

    pub(crate) fn new_with_value_text<F>(label: &str, value_text: F) -> Self
    where
        F: Fn(f64, f64, f64) -> String + 'static,
    {
        let area = DrawingArea::new();
        area.add_css_class("recording-editor-fill-slider");
        area.set_hexpand(true);
        area.set_size_request(-1, 32);
        let slider = Self {
            area: area.clone(),
            value: Rc::new(Cell::new(0.0)),
            min: Rc::new(Cell::new(0.0)),
            max: Rc::new(Cell::new(1.0)),
            step: Rc::new(Cell::new(0.05)),
            enabled: Rc::new(Cell::new(true)),
            hovered: Rc::new(Cell::new(false)),
            dragging: Rc::new(Cell::new(false)),
            logarithmic: Rc::new(Cell::new(false)),
            listeners: Rc::new(RefCell::new(Vec::new())),
            drag_started_listeners: Rc::new(RefCell::new(Vec::new())),
            drag_ended_listeners: Rc::new(RefCell::new(Vec::new())),
            value_text: Rc::new(value_text),
        };
        area.set_draw_func({
            let slider = slider.clone();
            let label = label.to_string();
            move |widget, cr, width, height| {
                slider.draw(widget, cr, width, height, &label);
            }
        });
        let drag = GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin({
            let slider = slider.clone();
            move |gesture, x, _| {
                slider.dragging.set(true);
                slider.area.queue_draw();
                for listener in slider.drag_started_listeners.borrow().iter().cloned() {
                    listener();
                }
                slider.apply_x(gesture, x);
            }
        });
        drag.connect_drag_update({
            let slider = slider.clone();
            move |gesture, dx, _| {
                let Some((start, _)) = gesture.start_point() else {
                    return;
                };
                slider.apply_x(gesture, start + dx);
            }
        });
        drag.connect_drag_end({
            let slider = slider.clone();
            move |_, _, _| {
                slider.dragging.set(false);
                slider.area.queue_draw();
                for listener in slider.drag_ended_listeners.borrow().iter().cloned() {
                    listener();
                }
            }
        });
        area.add_controller(drag);
        let motion = EventControllerMotion::new();
        motion.connect_enter({
            let slider = slider.clone();
            move |_, _, _| {
                slider.hovered.set(true);
                slider.area.queue_draw();
            }
        });
        motion.connect_leave({
            let slider = slider.clone();
            move |_| {
                slider.hovered.set(false);
                slider.area.queue_draw();
            }
        });
        area.add_controller(motion);
        area.set_cursor(gdk::Cursor::from_name("ew-resize", None).as_ref());
        slider
    }

    pub(crate) fn widget(&self) -> DrawingArea {
        self.area.clone()
    }

    pub(crate) fn value(&self) -> f64 {
        self.value.get()
    }

    pub(crate) fn set_value(&self, value: f64) {
        let min = self.min.get();
        let max = self.max.get();
        let value = value.clamp(min.min(max), min.max(max));
        self.value.set(value);
        self.area.queue_draw();
        let listeners = self.listeners.borrow().clone();
        for listener in listeners {
            listener(self);
        }
    }

    pub(crate) fn set_range(&self, min: f64, max: f64) {
        self.min.set(min);
        self.max.set(max.max(min + 1e-9));
        self.set_value(self.value.get());
    }

    pub(crate) fn set_increments(&self, step: f64, _page: f64) {
        self.step.set(step.max(0.0));
    }

    /// Space the track logarithmically instead of linearly. Use it for
    /// multiplicative quantities whose range spans an order of magnitude or
    /// more (playback speed, for one): equal pixels then mean equal *ratios*,
    /// so a 0.25x-30x range does not crush every slow-motion value into a few
    /// pixels at the left edge. Call after [`Self::set_range`] so a positive
    /// minimum is already in place.
    pub(crate) fn set_logarithmic(&self, logarithmic: bool) {
        self.logarithmic.set(logarithmic);
        self.area.queue_draw();
    }

    pub(crate) fn set_sensitive(&self, sensitive: bool) {
        self.enabled.set(sensitive);
        self.area.set_sensitive(sensitive);
        if !sensitive {
            self.hovered.set(false);
            self.dragging.set(false);
        }
        self.area.queue_draw();
    }

    pub(crate) fn connect_value_changed<F>(&self, f: F)
    where
        F: Fn(&FillSlider) + 'static,
    {
        self.listeners.borrow_mut().push(Rc::new(f));
    }

    pub(crate) fn connect_drag_started(&self, listener: impl Fn() + 'static) {
        self.drag_started_listeners
            .borrow_mut()
            .push(Rc::new(listener));
    }

    pub(crate) fn connect_drag_ended(&self, listener: impl Fn() + 'static) {
        self.drag_ended_listeners
            .borrow_mut()
            .push(Rc::new(listener));
    }

    fn apply_x(&self, gesture: &GestureDrag, x: f64) {
        if !self.enabled.get() {
            return;
        }
        let width = gesture
            .widget()
            .map(|widget| widget.allocated_width().max(1) as f64)
            .unwrap_or(1.0);
        let min = self.min.get();
        let max = self.max.get();
        let t = (x / width).clamp(0.0, 1.0);
        let value = fill_slider_value_from_t(t, min, max, self.logarithmic.get(), self.step.get());
        self.set_value(value);
    }

    fn draw(
        &self,
        widget: &DrawingArea,
        cr: &gtk4::cairo::Context,
        width: i32,
        height: i32,
        label: &str,
    ) {
        let w = width as f64;
        let h = height as f64;
        if w < 8.0 || h < 8.0 {
            return;
        }
        let light = widget_is_light(widget);
        let enabled = if self.enabled.get() { 1.0 } else { 0.42 };
        let hot = self.enabled.get() && (self.hovered.get() || self.dragging.get());
        let active = self.enabled.get() && self.dragging.get();
        let min = self.min.get();
        let max = self.max.get();
        let progress = fill_slider_progress(self.value.get(), min, max, self.logarithmic.get());
        let radius = 8.0;
        let (track_a, fill_a, text_a, handle_a) = if light {
            if active {
                (0.16, 0.28, 0.88, 0.95)
            } else if hot {
                (0.13, 0.22, 0.82, 0.92)
            } else {
                (0.10, 0.16, 0.72, 0.82)
            }
        } else if active {
            (0.12, 0.22, 0.92, 1.0)
        } else if hot {
            (0.10, 0.18, 0.86, 0.95)
        } else {
            (0.07, 0.12, 0.72, 0.82)
        };
        fill_slider_rounded_rect(cr, 0.0, 0.0, w, h, radius);
        if light {
            cr.set_source_rgba(0.11, 0.13, 0.16, track_a * enabled);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, track_a * enabled);
        }
        let _ = cr.fill();

        // Fill covers the track; the handle sits inside it, inset from the
        // trailing rounded edge so it reads as a grabber, not a border.
        let handle_inset = radius;
        let fill_w = progress * w;
        let tick_x =
            (fill_w - handle_inset).clamp(handle_inset, (w - handle_inset).max(handle_inset));
        let show_fill = progress > 0.0;
        if show_fill {
            fill_slider_rounded_rect(cr, 0.0, 0.0, fill_w, h, radius);
            if light {
                cr.set_source_rgba(0.11, 0.13, 0.16, fill_a * enabled);
            } else {
                cr.set_source_rgba(1.0, 1.0, 1.0, fill_a * enabled);
            }
            let _ = cr.fill();
        }

        if light {
            cr.set_source_rgba(0.15, 0.16, 0.18, text_a * enabled);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, text_a * enabled);
        }
        cr.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(12.0);
        cr.move_to(14.0, h * 0.66);
        let _ = cr.show_text(label);
        let text = (self.value_text)(self.value.get(), min, max);
        if let Ok(ext) = cr.text_extents(&text) {
            cr.move_to(w - 14.0 - ext.width(), h * 0.66);
            let _ = cr.show_text(&text);
        }

        // Draw the marker after text so it can travel over the label/value
        // instead of being trapped inside a reserved text gutter.
        cr.set_line_width(if hot { 2.0 } else { 1.5 });
        cr.set_line_cap(gtk4::cairo::LineCap::Round);
        if light {
            cr.set_source_rgba(0.15, 0.16, 0.18, handle_a * enabled);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, handle_a * enabled);
        }
        cr.move_to(tick_x, h * 0.28);
        cr.line_to(tick_x, h * 0.72);
        let _ = cr.stroke();
    }
}

/// Position of `value` along the track as 0..1. Logarithmic sliders space
/// equal ratios evenly; linear sliders space equal differences evenly.
fn fill_slider_progress(value: f64, min: f64, max: f64, logarithmic: bool) -> f64 {
    if logarithmic && min > 0.0 && max > 0.0 {
        let span = (max.ln() - min.ln()).abs().max(1e-9);
        ((value.max(1e-9).ln() - min.ln()) / span).clamp(0.0, 1.0)
    } else {
        ((value - min) / (max - min).max(1e-9)).clamp(0.0, 1.0)
    }
}

/// Value at track position `t` (0..1), snapped to `step` where one is set.
fn fill_slider_value_from_t(t: f64, min: f64, max: f64, logarithmic: bool, step: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let value = if logarithmic && min > 0.0 && max > 0.0 {
        (min.ln() + t * (max.ln() - min.ln())).exp()
    } else {
        min + t * (max - min)
    };
    if step > 1e-9 {
        ((value - min) / step).round() * step + min
    } else {
        value
    }
}

fn fill_slider_rounded_rect(cr: &gtk4::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}

fn widget_is_light(widget: &impl gtk4::glib::object::IsA<Widget>) -> bool {
    let mut current = Some(widget.clone().upcast::<Widget>());
    while let Some(node) = current {
        if node.has_css_class("editor-theme-light") {
            return true;
        }
        current = node.parent();
    }
    false
}
