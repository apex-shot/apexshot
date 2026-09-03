const CLICK_COLOR_PRESETS: [(u8, u8, u8); 7] = [
    (255, 255, 255),
    (24, 24, 28),
    (176, 92, 56),
    (80, 160, 255),
    (72, 210, 140),
    (255, 200, 80),
    (240, 80, 110),
];

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
    let title = Label::new(Some("Cursor"));
    title.add_css_class("recording-editor-zoom-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);

    let tabs = GtkBox::new(Orientation::Horizontal, 0);
    tabs.add_css_class("recording-editor-cursor-tabs");
    tabs.set_hexpand(true);
    tabs.set_homogeneous(true);
    let style_tab = cursor_tab_button("Style");
    let motion_tab = cursor_tab_button("Motion");
    let effects_tab = cursor_tab_button("Effects");
    motion_tab.set_group(Some(&style_tab));
    effects_tab.set_group(Some(&style_tab));
    style_tab.set_active(true);
    tabs.append(&style_tab);
    tabs.append(&motion_tab);
    tabs.append(&effects_tab);

    let style = build_cursor_style_tab(state.clone(), on_change.clone());
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

    let unavailable_title = Label::new(Some("Cursor unavailable"));
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
                    "The cursor is baked into this imported video.",
                    "Cursor replacement is unavailable. Its inferred path can still guide Auto Zoom.",
                )
            } else {
                (
                    "No editable cursor data was found.",
                    "Cursor styling requires pointer data from an ApexShot recording.",
                )
            };
            unavailable_detail.set_text(message);
            unavailable.set_tooltip_text(Some(tooltip));
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
        write(&mut state.lock().unwrap().cursor, scale.value());
        on_change();
    });
}

fn build_cursor_style_tab(
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) -> CursorPanel {
    let body = GtkBox::new(Orientation::Vertical, 8);
    body.add_css_class("recording-editor-zoom-body");
    body.add_css_class("recording-editor-cursor-tab-body");
    body.set_hexpand(true);

    let hint = Label::new(Some("Shown over the recording in preview and export"));
    hint.add_css_class("recording-editor-zoom-hint");
    hint.set_wrap(true);
    hint.set_xalign(0.0);
    body.append(&hint);

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
            preview.set_draw_func(move |_, cr, width, height| {
                cursor_sprite::draw_centered(cr, width as f64, height as f64, theme);
            });
            let name = Label::new(Some(theme.label()));
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
    body.append(&grid);

    let size_row = cursor_slider_row("Size");
    let shadow_row = cursor_slider_row("Shadow");
    size_row.scale.set_range(MIN_CURSOR_SIZE, MAX_CURSOR_SIZE);
    size_row.scale.set_increments(0.05, 0.25);
    shadow_row.scale.set_range(0.0, 1.0);
    shadow_row.scale.set_increments(0.05, 0.1);
    body.append(&size_row.widget);
    body.append(&shadow_row.widget);

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
        on_change,
        |cursor, value| cursor.shadow = value,
    );

    let refresh = {
        let cards = cards;
        let size_scale = size_row.scale.clone();
        let shadow_scale = shadow_row.scale.clone();
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

    let preset_label = Label::new(Some("PRESET"));
    preset_label.add_css_class("recording-editor-zoom-kicker");
    preset_label.set_xalign(0.0);
    body.append(&preset_label);

    let presets = GtkBox::new(Orientation::Horizontal, 8);
    presets.add_css_class("recording-editor-motion-preset-row");
    presets.set_hexpand(true);
    presets.set_homogeneous(true);
    let focused_btn = motion_preset_card("Focused", "Tighter, faster");
    let smooth_btn = motion_preset_card("Smooth", "Softer follow");
    presets.append(&focused_btn);
    presets.append(&smooth_btn);
    body.append(&presets);

    let smooth_row = cursor_slider_row("Smoothing");
    let speed_row = cursor_slider_row("Speed");
    let trail_row = cursor_slider_row("Trail");
    let tilt_row = cursor_slider_row("Tilt");
    let sway_row = cursor_slider_row("Sway");
    smooth_row.scale.set_range(0.0, 1.0);
    smooth_row.scale.set_increments(0.05, 0.1);
    speed_row
        .scale
        .set_range(MIN_CURSOR_SPEED, MAX_CURSOR_SPEED);
    speed_row.scale.set_increments(0.05, 0.25);
    trail_row.scale.set_range(0.0, 1.0);
    trail_row.scale.set_increments(0.05, 0.1);
    tilt_row.scale.set_range(0.0, 1.0);
    tilt_row.scale.set_increments(0.05, 0.1);
    sway_row.scale.set_range(0.0, 1.0);
    sway_row.scale.set_increments(0.05, 0.1);
    body.append(&smooth_row.widget);
    body.append(&speed_row.widget);
    body.append(&trail_row.widget);
    body.append(&tilt_row.widget);
    body.append(&sway_row.widget);

    let idle_row = GtkBox::new(Orientation::Horizontal, 8);
    idle_row.add_css_class("recording-editor-zoom-classic");
    idle_row.set_hexpand(true);
    let idle_label = Label::new(Some("Hide when idle"));
    idle_label.add_css_class("recording-editor-zoom-classic-label");
    idle_label.set_xalign(0.0);
    idle_label.set_hexpand(true);
    let idle_switch = Switch::new();
    idle_switch.add_css_class("recording-editor-zoom-switch");
    idle_switch.set_valign(Align::Center);
    idle_switch.set_halign(Align::End);
    idle_row.append(&idle_label);
    idle_row.append(&idle_switch);
    body.append(&idle_row);

    let idle_delay_row = cursor_slider_row("Idle delay");
    idle_delay_row.scale.set_range(120.0, 4000.0);
    idle_delay_row.scale.set_increments(40.0, 200.0);
    body.append(&idle_delay_row.widget);

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
        &trail_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.trail = value,
    );
    bind_cursor_f64(
        &tilt_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.tilt = value,
    );
    bind_cursor_f64(
        &sway_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.sway = value,
    );
    bind_cursor_f64(
        &idle_delay_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.idle_ms = value,
    );
    idle_switch.connect_state_set({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        let pause_playback = pause_playback.clone();
        move |_, active| {
            if !syncing.get() {
                pause_playback();
                state.lock().unwrap().cursor.hide_idle = active;
                on_change();
            }
            glib::Propagation::Proceed
        }
    });

    let refresh = {
        let focused_btn = focused_btn.clone();
        let smooth_btn = smooth_btn.clone();
        let smooth_scale = smooth_row.scale.clone();
        let speed_scale = speed_row.scale.clone();
        let trail_scale = trail_row.scale.clone();
        let tilt_scale = tilt_row.scale.clone();
        let sway_scale = sway_row.scale.clone();
        let idle_switch = idle_switch.clone();
        let idle_delay_scale = idle_delay_row.scale.clone();
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
            trail_scale.set_value(cursor.trail);
            tilt_scale.set_value(cursor.tilt);
            sway_scale.set_value(cursor.sway);
            idle_switch.set_active(cursor.hide_idle);
            idle_delay_scale.set_value(cursor.idle_ms);
            idle_delay_scale.set_sensitive(cursor.hide_idle);
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

    let click_label = Label::new(Some("CLICK EFFECT"));
    click_label.add_css_class("recording-editor-zoom-kicker");
    click_label.set_xalign(0.0);
    body.append(&click_label);
    let click_row = GtkBox::new(Orientation::Horizontal, 6);
    click_row.add_css_class("recording-editor-click-effect-row");
    click_row.set_hexpand(true);
    click_row.set_homogeneous(true);
    let none_btn = click_effect_card(ClickEffect::None);
    let spotlight_btn = click_effect_card(ClickEffect::Spotlight);
    let ripple_btn = click_effect_card(ClickEffect::Ripple);
    let echo_btn = click_effect_card(ClickEffect::Echo);
    spotlight_btn.set_group(Some(&none_btn));
    ripple_btn.set_group(Some(&none_btn));
    echo_btn.set_group(Some(&none_btn));
    click_row.append(&none_btn);
    click_row.append(&spotlight_btn);
    click_row.append(&ripple_btn);
    click_row.append(&echo_btn);
    body.append(&click_row);

    let color_row = GtkBox::new(Orientation::Horizontal, 8);
    color_row.add_css_class("recording-editor-click-color-row");
    color_row.set_hexpand(true);
    let color_label = Label::new(Some("Color"));
    color_label.add_css_class("recording-editor-zoom-classic-label");
    color_label.set_xalign(0.0);
    color_label.set_hexpand(true);
    color_label.set_valign(Align::Center);
    let swatch = Button::new();
    swatch.add_css_class("recording-editor-click-color-swatch");
    swatch.set_has_frame(false);
    swatch.set_tooltip_text(Some("Choose click color"));
    let swatch_paint = DrawingArea::new();
    swatch_paint.set_content_width(28);
    swatch_paint.set_content_height(22);
    swatch_paint.set_can_target(false);
    swatch_paint.set_draw_func({
        let state = state.clone();
        move |_, cr, width, height| {
            let (r, g, b) = state.lock().unwrap().cursor.click_color;
            draw_color_chip(cr, width as f64, height as f64, (r, g, b), 6.0);
        }
    });
    swatch.set_child(Some(&swatch_paint));
    swatch.connect_clicked({
        let state = state.clone();
        let on_change = on_change.clone();
        move |button| open_click_color_dialog(button, state.clone(), on_change.clone())
    });
    let hex = Label::new(Some("#FFFFFF"));
    hex.add_css_class("recording-editor-click-color-hex");
    hex.set_xalign(0.0);
    hex.set_valign(Align::Center);
    color_row.append(&color_label);
    color_row.append(&swatch);
    color_row.append(&hex);
    body.append(&color_row);

    let dots = GtkBox::new(Orientation::Horizontal, 6);
    dots.add_css_class("recording-editor-click-color-dots");
    dots.set_halign(Align::End);
    for color in CLICK_COLOR_PRESETS {
        let dot = color_dot_button(color);
        dot.connect_clicked({
            let state = state.clone();
            let on_change = on_change.clone();
            move |_| {
                state.lock().unwrap().cursor.click_color = color;
                on_change();
            }
        });
        dots.append(&dot);
    }
    body.append(&dots);

    let size_row = cursor_slider_row("Size");
    let opacity_row = cursor_slider_row("Opacity");
    let duration_row = cursor_slider_row("Duration");
    let intensity_row = cursor_slider_row("Intensity");
    size_row.scale.set_range(MIN_CLICK_SCALE, MAX_CLICK_SCALE);
    size_row.scale.set_increments(0.05, 0.1);
    opacity_row.scale.set_range(0.0, 1.0);
    opacity_row.scale.set_increments(0.05, 0.1);
    duration_row
        .scale
        .set_range(MIN_CLICK_DURATION_MS as f64, MAX_CLICK_DURATION_MS as f64);
    duration_row.scale.set_increments(20.0, 100.0);
    intensity_row.scale.set_range(0.0, 1.0);
    intensity_row.scale.set_increments(0.05, 0.1);
    body.append(&size_row.widget);
    body.append(&opacity_row.widget);
    body.append(&duration_row.widget);
    body.append(&intensity_row.widget);

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
    spotlight_btn.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            state.lock().unwrap().cursor.click_effect = ClickEffect::Spotlight;
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
    echo_btn.connect_toggled({
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        move |button| {
            if syncing.get() || !button.is_active() {
                return;
            }
            state.lock().unwrap().cursor.click_effect = ClickEffect::Echo;
            on_change();
        }
    });
    bind_cursor_f64(
        &size_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.click_scale = value,
    );
    bind_cursor_f64(
        &opacity_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.click_opacity = value,
    );
    bind_cursor_f64(
        &duration_row.scale,
        syncing.clone(),
        state.clone(),
        on_change.clone(),
        |cursor, value| cursor.click_duration_ms = value.round() as u32,
    );
    bind_cursor_f64(
        &intensity_row.scale,
        syncing.clone(),
        state.clone(),
        on_change,
        |cursor, value| cursor.click_intensity = value,
    );

    let refresh = {
        let live = live.clone();
        let swatch_paint = swatch_paint.clone();
        let hex = hex.clone();
        let none_btn = none_btn.clone();
        let spotlight_btn = spotlight_btn.clone();
        let ripple_btn = ripple_btn.clone();
        let echo_btn = echo_btn.clone();
        let size_scale = size_row.scale.clone();
        let opacity_scale = opacity_row.scale.clone();
        let duration_scale = duration_row.scale.clone();
        let intensity_scale = intensity_row.scale.clone();
        let swatch = swatch.clone();
        let dots = dots.clone();
        let syncing = syncing.clone();
        Rc::new(move || {
            let cursor = state.lock().unwrap().cursor;
            syncing.set(true);
            match cursor.click_effect {
                ClickEffect::None => none_btn.set_active(true),
                ClickEffect::Spotlight => spotlight_btn.set_active(true),
                ClickEffect::Ripple => ripple_btn.set_active(true),
                ClickEffect::Echo => echo_btn.set_active(true),
            }
            size_scale.set_value(cursor.click_scale);
            opacity_scale.set_value(cursor.click_opacity);
            duration_scale.set_value(cursor.click_duration_ms as f64);
            intensity_scale.set_value(cursor.click_intensity);
            let enabled = cursor.click_effect != ClickEffect::None;
            size_scale.set_sensitive(enabled);
            opacity_scale.set_sensitive(enabled);
            duration_scale.set_sensitive(enabled);
            intensity_scale.set_sensitive(enabled);
            swatch.set_sensitive(enabled);
            dots.set_sensitive(enabled);
            hex.set_text(&format!(
                "#{:02X}{:02X}{:02X}",
                cursor.click_color.0, cursor.click_color.1, cursor.click_color.2
            ));
            swatch_paint.queue_draw();
            live.queue_draw();
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };

    CursorPanel {
        widget: body,
        refresh,
    }
}

fn color_dot_button(color: (u8, u8, u8)) -> Button {
    let button = Button::new();
    button.add_css_class("recording-editor-click-color-dot");
    button.set_has_frame(false);
    let paint = DrawingArea::new();
    paint.set_content_width(16);
    paint.set_content_height(16);
    paint.set_can_target(false);
    paint.set_draw_func(move |_, cr, width, height| {
        let cx = width as f64 / 2.0;
        let cy = height as f64 / 2.0;
        cr.set_source_rgb(
            color.0 as f64 / 255.0,
            color.1 as f64 / 255.0,
            color.2 as f64 / 255.0,
        );
        cr.arc(cx, cy, 6.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    });
    button.set_child(Some(&paint));
    button
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

fn open_click_color_dialog(
    widget: &impl IsA<Widget>,
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) {
    let (r, g, b) = state.lock().unwrap().cursor.click_color;
    let initial = gdk::RGBA::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0);
    let parent = widget
        .root()
        .and_then(|root| root.downcast::<Window>().ok());
    let dialog = ColorChooserDialog::new(Some("Click color"), parent.as_ref());
    dialog.set_modal(true);
    dialog.set_use_alpha(false);
    dialog.set_rgba(&initial);
    dialog.connect_response(move |dialog, response| {
        if response == gtk4::ResponseType::Ok {
            let color = dialog.rgba();
            state.lock().unwrap().cursor.click_color = (
                (color.red() * 255.0).round().clamp(0.0, 255.0) as u8,
                (color.green() * 255.0).round().clamp(0.0, 255.0) as u8,
                (color.blue() * 255.0).round().clamp(0.0, 255.0) as u8,
            );
            on_change();
        }
        dialog.close();
    });
    dialog.present();
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
    let cx = width / 2.0;
    let cy = height / 2.0;
    let duration = settings.click_window_seconds().max(0.2);
    let local = t % (duration * 1.35);
    if local <= duration {
        cursor_sprite::draw_click(
            cr,
            cx,
            cy,
            (local / duration).clamp(0.0, 1.0),
            settings,
            1.0,
        );
    }
    cursor_sprite::draw(cr, cx, cy, 1.0, "default", settings, 0.95);
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
    let name = Label::new(Some(effect.label()));
    name.add_css_class("recording-editor-click-effect-label");
    name.set_xalign(0.5);
    column.append(&name);
    card.set_child(Some(&column));
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
struct FillSlider {
    area: DrawingArea,
    value: Rc<Cell<f64>>,
    min: Rc<Cell<f64>>,
    max: Rc<Cell<f64>>,
    step: Rc<Cell<f64>>,
    enabled: Rc<Cell<bool>>,
    listeners: Rc<RefCell<Vec<Rc<dyn Fn(&FillSlider)>>>>,
}

impl FillSlider {
    fn new(label: &str) -> Self {
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
            listeners: Rc::new(RefCell::new(Vec::new())),
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
            move |gesture, x, _| slider.apply_x(gesture, x)
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
        area.add_controller(drag);
        area.set_cursor(gdk::Cursor::from_name("ew-resize", None).as_ref());
        slider
    }

    fn value(&self) -> f64 {
        self.value.get()
    }

    fn set_value(&self, value: f64) {
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

    fn set_range(&self, min: f64, max: f64) {
        self.min.set(min);
        self.max.set(max.max(min + 1e-9));
        self.set_value(self.value.get());
    }

    fn set_increments(&self, step: f64, _page: f64) {
        self.step.set(step.max(0.0));
    }

    fn set_sensitive(&self, sensitive: bool) {
        self.enabled.set(sensitive);
        self.area.set_sensitive(sensitive);
        self.area.queue_draw();
    }

    fn connect_value_changed<F>(&self, f: F)
    where
        F: Fn(&FillSlider) + 'static,
    {
        self.listeners.borrow_mut().push(Rc::new(f));
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
        let mut value = min + t * (max - min);
        let step = self.step.get();
        if step > 1e-9 {
            value = ((value - min) / step).round() * step + min;
        }
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
        let min = self.min.get();
        let max = self.max.get();
        let progress = ((self.value.get() - min) / (max - min).max(1e-9)).clamp(0.0, 1.0);
        let radius = 8.0;
        fill_slider_rounded_rect(cr, 0.0, 0.0, w, h, radius);
        if light {
            cr.set_source_rgba(0.11, 0.13, 0.16, 0.10 * enabled);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.07 * enabled);
        }
        let _ = cr.fill();

        // Allow the marker to overlay the value text, but stop at its right
        // edge rather than running into the slider's rounded outer edge.
        let tick_x = 1.5 + progress * (w - 15.5);
        let fill_w = tick_x;
        let show_fill = progress > 0.0;
        if show_fill {
            fill_slider_rounded_rect(cr, 0.0, 0.0, fill_w, h, radius);
            if light {
                cr.set_source_rgba(0.11, 0.13, 0.16, 0.16 * enabled);
            } else {
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.12 * enabled);
            }
            let _ = cr.fill();
        }

        if light {
            cr.set_source_rgba(0.15, 0.16, 0.18, 0.72 * enabled);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.72 * enabled);
        }
        cr.select_font_face(
            crate::typography::UI_FONT_FAMILY,
            gtk4::cairo::FontSlant::Normal,
            gtk4::cairo::FontWeight::Normal,
        );
        cr.set_font_size(12.0);
        cr.move_to(14.0, h * 0.66);
        let _ = cr.show_text(label);
        let display = (progress * 100.0).round() as i32;
        let text = display.to_string();
        if let Ok(ext) = cr.text_extents(&text) {
            cr.move_to(w - 14.0 - ext.width(), h * 0.66);
            let _ = cr.show_text(&text);
        }

        // Draw the marker after text so it can travel over the label/value
        // instead of being trapped inside a reserved text gutter.
        cr.set_line_width(1.5);
        cr.set_line_cap(gtk4::cairo::LineCap::Round);
        if light {
            cr.set_source_rgba(0.15, 0.16, 0.18, 0.82 * enabled);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.82 * enabled);
        }
        cr.move_to(tick_x, h * 0.28);
        cr.line_to(tick_x, h * 0.72);
        let _ = cr.stroke();
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
