// Background panel for the recording (video) editor.
//
// Accessed through the left tool rail next to Cursor; opens on the right
// just like the Cursor panel. The fill is picked from one of three sources —
// a bundled wallpaper, a hand-drawn custom fill, or an image from disk —
// above the Padding and Radius controls.
//
// Included into `tool_sidebar.rs`, so parent imports (gtk, state, FillSlider,
// color dots, `t`, ..) are already in scope; only truly new items are
// imported here.

use std::path::PathBuf;

use crate::capture::editor::window::icon_names;

fn video_wallpaper_files() -> Vec<&'static str> {
    crate::capture::editor::window::background_panel::MOTION_WALLPAPER_FILES
        .iter()
        .copied()
        .filter(|name| name.starts_with("wallpaper-"))
        .collect()
}

fn wallpaper_full_path(file_name: &str) -> PathBuf {
    crate::capture::editor::window::background_panel::background_gradient_asset_path(file_name)
}

fn wallpaper_thumb_path(file_name: &str) -> PathBuf {
    crate::capture::editor::window::background_panel::motion_wallpaper_preview_asset_path(file_name)
}

fn wallpaper_file_name(path: &PathBuf) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_owned())
}

struct BackgroundPanel {
    widget: GtkBox,
    refresh: Rc<dyn Fn()>,
}

/// Which source page the panel is showing. This is view state, deliberately
/// separate from the model's fill: picking the Wallpaper tab should reveal
/// the grid even when no wallpaper is selected yet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BgPage {
    Wallpaper,
    Custom,
    Image,
}

fn build_background_panel(
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) -> BackgroundPanel {
    let panel = GtkBox::new(Orientation::Vertical, 0);
    panel.add_css_class("recording-editor-zoom-panel");
    panel.set_hexpand(true);
    panel.set_vexpand(true);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("recording-editor-zoom-header");
    header.set_hexpand(true);
    let title = Label::new(Some(&t("Background")));
    title.add_css_class("recording-editor-zoom-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    header.append(&title);
    panel.append(&header);

    // Which source the fill comes from. `None` still lives in the model as
    // the absence of a fill, so it is reached by clearing rather than by
    // holding a tab — the tabs pick *what* fills, and a "No background"
    // action below turns it off.
    let source_row = GtkBox::new(Orientation::Horizontal, 0);
    source_row.add_css_class("recording-editor-bg-tabs");
    source_row.set_hexpand(true);
    source_row.set_homogeneous(true);
    let wallpaper_tab = bg_tab_button(&t("Wallpaper"));
    let custom_tab = bg_tab_button(&t("Custom"));
    let image_tab = bg_tab_button(&t("Image"));
    custom_tab.set_group(Some(&wallpaper_tab));
    image_tab.set_group(Some(&wallpaper_tab));
    wallpaper_tab.set_active(true);
    source_row.append(&wallpaper_tab);
    source_row.append(&custom_tab);
    source_row.append(&image_tab);

    let body = GtkBox::new(Orientation::Vertical, 8);
    body.add_css_class("recording-editor-zoom-body");
    body.add_css_class("recording-editor-cursor-tab-body");
    body.set_hexpand(true);
    body.append(&source_row);

    // --- Wallpaper source: the bundled grid, four across like the mock. ---
    let wallpaper_page = GtkBox::new(Orientation::Vertical, 0);
    wallpaper_page.set_hexpand(true);

    let grid = Grid::new();
    grid.add_css_class("editor-motion-wallpaper-grid");
    grid.add_css_class("recording-editor-bg-wallpaper-grid");
    grid.set_column_spacing(8);
    grid.set_row_spacing(8);
    grid.set_column_homogeneous(true);
    grid.set_hexpand(false);
    grid.set_halign(Align::Fill);

    let syncing = Rc::new(Cell::new(false));
    let files = video_wallpaper_files();
    let cards: Vec<(String, ToggleButton)> = files
        .iter()
        .enumerate()
        .map(|(index, &file_name)| {
            let card = ToggleButton::new();
            card.add_css_class("editor-background-gradient-button");
            card.add_css_class("editor-background-preview-size-regular");
            card.add_css_class("editor-motion-wallpaper-thumbnail");
            card.set_has_frame(false);
            card.set_size_request(56, 56);
            card.set_hexpand(false);
            card.set_halign(Align::Center);
            card.set_valign(Align::Start);
            card.set_tooltip_text(Some(file_name));
            let thumb = DrawingArea::new();
            thumb.set_content_width(56);
            thumb.set_content_height(56);
            card.set_child(Some(&thumb));
            // Decode the small bundled thumb off the critical path so
            // opening the panel stays instant with 60 tiles.
            {
                let thumb = thumb.clone();
                let thumb_path = wallpaper_thumb_path(file_name);
                glib::idle_add_local_once(move || {
                    let Some(surface) = decode_wallpaper_thumb(&thumb_path) else {
                        return;
                    };
                    thumb.set_draw_func(move |_, cr, width, height| {
                        paint_wallpaper_thumb(cr, &surface, width, height);
                    });
                    thumb.queue_draw();
                });
            }
            {
                let state = state.clone();
                let on_change = on_change.clone();
                let syncing = syncing.clone();
                let file_name = file_name.to_string();
                card.connect_clicked(move |button| {
                    if syncing.get() || !button.is_active() {
                        return;
                    }
                    let full = wallpaper_full_path(&file_name);
                    let mut guard = state.lock().unwrap();
                    // Re-clicking the active wallpaper must not recomposite.
                    if guard.background == VideoBackground::Wallpaper(full.clone()) {
                        return;
                    }
                    guard.background = VideoBackground::Wallpaper(full);
                    drop(guard);
                    on_change();
                });
            }
            grid.attach(&card, (index % 4) as i32, (index / 4) as i32, 1, 1);
            (file_name.to_string(), card)
        })
        .collect();
    if let Some(first) = cards.first().map(|(_, card)| card.clone()) {
        for (_, card) in cards.iter().skip(1) {
            card.set_group(Some(&first));
        }
    }
    wallpaper_page.append(&grid);

    // --- Custom source: one row that opens the Custom Wallpaper dialog. ---
    let custom_page = GtkBox::new(Orientation::Vertical, 0);
    custom_page.set_hexpand(true);
    let custom_row = Button::new();
    custom_row.add_css_class("recording-editor-bg-custom-row");
    custom_row.set_has_frame(false);
    custom_row.set_hexpand(true);
    let custom_inner = GtkBox::new(Orientation::Horizontal, 10);
    let custom_swatch = DrawingArea::new();
    custom_swatch.add_css_class("recording-editor-bg-custom-swatch");
    custom_swatch.set_content_width(20);
    custom_swatch.set_content_height(20);
    custom_swatch.set_valign(Align::Center);
    custom_swatch.set_can_target(false);
    let custom_label = Label::new(Some(&t("Edit custom fill")));
    custom_label.set_hexpand(true);
    custom_label.set_xalign(0.0);
    custom_label.set_valign(Align::Center);
    let custom_edit = Image::from_icon_name("document-edit-symbolic");
    custom_edit.set_pixel_size(13);
    custom_edit.set_valign(Align::Center);
    custom_inner.append(&custom_swatch);
    custom_inner.append(&custom_label);
    custom_inner.append(&custom_edit);
    custom_row.set_child(Some(&custom_inner));
    custom_page.append(&custom_row);

    // --- Image source: pick any file on disk. ---
    let image_page = GtkBox::new(Orientation::Vertical, 0);
    image_page.set_hexpand(true);
    let image_row = Button::new();
    image_row.add_css_class("recording-editor-bg-custom-row");
    image_row.set_has_frame(false);
    image_row.set_hexpand(true);
    let image_inner = GtkBox::new(Orientation::Horizontal, 10);
    let image_thumb = DrawingArea::new();
    image_thumb.add_css_class("recording-editor-bg-custom-swatch");
    image_thumb.set_content_width(20);
    image_thumb.set_content_height(20);
    image_thumb.set_valign(Align::Center);
    image_thumb.set_can_target(false);
    let image_label = Label::new(Some(&t("Select image...")));
    image_label.set_hexpand(true);
    image_label.set_xalign(0.0);
    image_label.set_valign(Align::Center);
    let image_edit = Image::from_icon_name(icon_names::shipped::FOLDER_OPEN_REGULAR);
    image_edit.set_pixel_size(13);
    image_edit.set_valign(Align::Center);
    image_inner.append(&image_thumb);
    image_inner.append(&image_label);
    image_inner.append(&image_edit);
    image_row.set_child(Some(&image_inner));
    image_page.append(&image_row);

    // --- Padding and Radius: number pill + slider, per the mock. ---
    let padding_row = bg_value_row(&t("Padding"));
    padding_row.scale.set_range(0.0, 80.0);
    padding_row.scale.set_increments(1.0, 4.0);
    bind_bg_value(
        &padding_row,
        &syncing,
        &state,
        &on_change,
        |guard, value| guard.background_padding = value,
    );

    // Radius follows padding's units: both are slider values against a 400px
    // long edge, so the number in the pill matches what export draws.
    let radius_row = bg_value_row(&t("Radius"));
    radius_row.scale.set_range(0.0, 80.0);
    radius_row.scale.set_increments(1.0, 4.0);
    bind_bg_value(
        &radius_row,
        &syncing,
        &state,
        &on_change,
        |guard, value| guard.background_corner_radius = value,
    );

    let pages = GtkBox::new(Orientation::Vertical, 0);
    pages.set_hexpand(true);
    pages.append(&wallpaper_page);
    pages.append(&custom_page);
    pages.append(&image_page);
    body.append(&padding_row.widget);
    body.append(&radius_row.widget);
    body.append(&pages);

    // --- Stroke and Shadow: expandable, inert for now. ---
    let stroke_section = bg_placeholder_section(&t("Stroke"));
    let shadow_section = bg_placeholder_section(&t("Shadow"));
    body.append(&stroke_section.widget);
    body.append(&shadow_section.widget);

    let scroll = ScrolledWindow::new();
    scroll.add_css_class("recording-editor-zoom-scroll");
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&body));
    panel.append(&scroll);

    // Picking a custom source without an editable fill would leave the panel
    // Which tab is open, independent of the fill the model holds. The pages
    // follow this, so opening Wallpaper shows the grid whether or not a
    // wallpaper happens to be picked yet.
    let active_page = Rc::new(Cell::new(BgPage::Wallpaper));

    // Picking a source that has nothing behind it yet falls back to a
    // sensible default rather than showing a page that cannot do anything.
    let custom_tab_activate = {
        let state = state.clone();
        let on_change = on_change.clone();
        let syncing = syncing.clone();
        let active_page = active_page.clone();
        move |button: &ToggleButton| {
            if !button.is_active() {
                return;
            }
            active_page.set(BgPage::Custom);
            let mut guard = state.lock().unwrap();
            if matches!(
                guard.background,
                VideoBackground::Plain { .. } | VideoBackground::Gradient(_)
            ) {
                return;
            }
            guard.background = VideoBackground::Plain {
                r: 17,
                g: 17,
                b: 17,
            };
            drop(guard);
            syncing.set(false);
            on_change();
        }
    };
    let image_tab_activate = {
        let state = state.clone();
        let syncing = syncing.clone();
        let active_page = active_page.clone();
        move |button: &ToggleButton| {
            if !button.is_active() {
                return;
            }
            active_page.set(BgPage::Image);
            let _ = &state;
            let _ = &syncing;
        }
    };
    wallpaper_tab.connect_toggled({
        let active_page = active_page.clone();
        move |button: &ToggleButton| {
            if button.is_active() {
                active_page.set(BgPage::Wallpaper);
            }
        }
    });
    custom_tab.connect_toggled(custom_tab_activate);
    image_tab.connect_toggled(image_tab_activate);

    custom_row.connect_clicked({
        let state = state.clone();
        let on_change = on_change.clone();
        move |button| open_custom_wallpaper_dialog(button, state.clone(), on_change.clone())
    });

    image_row.connect_clicked({
        let state = state.clone();
        let on_change = on_change.clone();
        move |button| pick_background_image(button, state.clone(), on_change.clone())
    });

    let refresh = {
        let wallpaper_tab = wallpaper_tab.clone();
        let custom_tab = custom_tab.clone();
        let image_tab = image_tab.clone();
        let wallpaper_page = wallpaper_page.clone();
        let custom_page = custom_page.clone();
        let image_page = image_page.clone();
        let custom_swatch = custom_swatch.clone();
        let image_thumb = image_thumb.clone();
        let padding_row_value = padding_row.clone();
        let radius_row_value = radius_row.clone();
        let cards = cards.clone();
        let syncing = syncing.clone();
        let active_page = active_page.clone();
        Rc::new(move || {
            let (background, padding, radius) = {
                let guard = state.lock().unwrap();
                (
                    guard.background.clone(),
                    guard.background_padding,
                    guard.background_corner_radius,
                )
            };
            syncing.set(true);
            let is_wallpaper = matches!(background, VideoBackground::Wallpaper(_));
            let is_plain = matches!(background, VideoBackground::Plain { .. });
            let is_gradient = matches!(background, VideoBackground::Gradient(_));
            let has_fill = is_wallpaper || is_plain || is_gradient;

            // A user-picked image and a bundled wallpaper share a variant, so
            // the Image tab is only implied when the chosen file is not one of
            // the bundled assets. The open page otherwise stays wherever the
            // user left it, so the wallpaper grid is not blank on first open.
            let picked = match &background {
                VideoBackground::Wallpaper(path) => wallpaper_file_name(path)
                    .map(|name| !video_wallpaper_files().contains(&name.as_str()))
                    .unwrap_or(false),
                _ => false,
            };
            // The open tab wins. An earlier version inferred the page from
            // the fill, which fought the user: switching to Custom wrote a
            // plain fill, refresh then pinned the page to Custom, and
            // returning to Wallpaper snapped straight back. The open tab is
            // the only thing that moves the page now; the fill follows it.
            match active_page.get() {
                BgPage::Wallpaper => wallpaper_tab.set_active(true),
                BgPage::Custom => custom_tab.set_active(true),
                BgPage::Image => image_tab.set_active(true),
            }

            wallpaper_page.set_visible(matches!(active_page.get(), BgPage::Wallpaper));
            custom_page.set_visible(matches!(active_page.get(), BgPage::Custom));
            image_page.set_visible(matches!(active_page.get(), BgPage::Image));

            // Padding only means something once something fills the canvas.
            padding_row_value.widget.set_visible(has_fill);
            padding_row_value.sync_value(padding);
            // Radius rounds the video card itself, so it stays available with
            // no fill at all.
            radius_row_value.sync_value(radius);

            // The custom swatch previews whatever the dialog last applied.
            {
                let preview: Option<(u8, u8, u8)> = match &background {
                    VideoBackground::Plain { r, g, b } => Some((*r, *g, *b)),
                    VideoBackground::Gradient(gradient) => {
                        let stops = gradient.draw_stops();
                        // Average the ends so a multi-stop ramp reads as a
                        // representative color at 20px.
                        stops.first().zip(stops.last()).map(|(a, b)| {
                            (
                                ((a.r as u32 + b.r as u32) / 2) as u8,
                                ((a.g as u32 + b.g as u32) / 2) as u8,
                                ((a.b as u32 + b.b as u32) / 2) as u8,
                            )
                        })
                    }
                    _ => None,
                };
                custom_swatch.set_draw_func(move |_, cr, width, height| {
                    if let Some((r, g, b)) = preview {
                        draw_color_chip(cr, width as f64, height as f64, (r, g, b), 5.0);
                    }
                });
                custom_swatch.queue_draw();
            }

            // The Image tab shows the chosen file, or an empty slot.
            {
                let path = match &background {
                    VideoBackground::Wallpaper(path) if picked => Some(path.clone()),
                    _ => None,
                };
                let surface = path.as_ref().and_then(|p| decode_wallpaper_thumb(p));
                image_thumb.set_draw_func(move |_, cr, width, height| {
                    if let Some(surface) = &surface {
                        paint_wallpaper_thumb(cr, surface, width, height);
                    }
                });
                image_thumb.queue_draw();
            }

            if is_wallpaper {
                let active = wallpaper_file_name_from_background(&background);
                for (file_name, card) in &cards {
                    let on = active.as_deref() == Some(file_name.as_str());
                    card.set_active(on);
                    if on {
                        card.add_css_class("active-background-option");
                    } else {
                        card.remove_css_class("active-background-option");
                    }
                }
            } else {
                for (_, card) in &cards {
                    card.set_active(false);
                    card.remove_css_class("active-background-option");
                }
            }
            syncing.set(false);
        }) as Rc<dyn Fn()>
    };

    BackgroundPanel {
        widget: panel,
        refresh,
    }
}

fn bg_tab_button(label: &str) -> ToggleButton {
    let button = ToggleButton::with_label(label);
    button.add_css_class("recording-editor-bg-tab");
    button.set_has_frame(false);
    button.set_hexpand(true);
    button
}

fn wallpaper_file_name_from_background(background: &VideoBackground) -> Option<String> {
    match background {
        VideoBackground::Wallpaper(path) => wallpaper_file_name(path),
        _ => None,
    }
}

/// A labelled slider row. The filled slider draws its own name and value,
/// so the row is just the track — no separate number field to keep in sync.
#[derive(Clone)]
struct BgValueRow {
    widget: GtkBox,
    scale: FillSlider,
    syncing: Rc<Cell<bool>>,
}

fn bg_value_row(label: &str) -> BgValueRow {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.add_css_class("recording-editor-bg-value-row");
    row.set_hexpand(true);

    // `FillSlider::new` shows a 0-100 percentage, which would render a 24px
    // padding on an 0-80 range as "30". Show the real value in px instead.
    let scale = FillSlider::new_with_value_text(label, |value, _, _| format!("{value:.0}px"));
    let track = scale.widget();
    track.set_hexpand(true);
    track.set_size_request(-1, 32);
    row.append(&track);

    BgValueRow {
        widget: row,
        scale,
        syncing: Rc::new(Cell::new(false)),
    }
}

impl BgValueRow {
    /// Push state into the widget without re-entering the write path.
    fn sync_value(&self, value: f64) {
        self.syncing.set(true);
        self.scale.set_value(value);
        self.syncing.set(false);
    }
}

/// Wire a row's slider to one state field.
fn bind_bg_value(
    row: &BgValueRow,
    syncing: &Rc<Cell<bool>>,
    state: &Arc<Mutex<VideoEditState>>,
    on_change: &Rc<dyn Fn()>,
    write: impl Fn(&mut VideoEditState, f64) + Clone + 'static,
) {
    let scale_row = row.clone();
    let scale_state = state.clone();
    let scale_change = on_change.clone();
    let scale_syncing = syncing.clone();
    let slider_write = write;
    row.scale.connect_value_changed(move |slider| {
        if scale_syncing.get() || scale_row.syncing.get() {
            return;
        }
        slider_write(&mut scale_state.lock().unwrap(), slider.value());
        scale_change();
    });
}

struct BgSection {
    widget: GtkBox,
}

/// An expandable row with a "+" affordance. Stroke and Shadow ship this way:
/// the control is there, but nothing renders it until it is implemented.
fn bg_placeholder_section(label: &str) -> BgSection {
    let section = GtkBox::new(Orientation::Vertical, 0);
    section.add_css_class("recording-editor-bg-section");
    section.set_hexpand(true);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.set_hexpand(true);

    let title = Button::new();
    title.add_css_class("recording-editor-bg-section-toggle");
    title.set_has_frame(false);
    title.set_hexpand(true);
    let title_text = Label::new(Some(label));
    title_text.set_xalign(0.0);
    title.set_child(Some(&title_text));
    header.append(&title);

    let add = Button::new();
    add.add_css_class("recording-editor-bg-section-add");
    add.set_has_frame(false);
    add.set_valign(Align::Center);
    add.set_tooltip_text(Some(&t("Add")));
    let plus = Label::new(Some("+"));
    plus.set_valign(Align::Center);
    add.set_child(Some(&plus));
    header.append(&add);
    section.append(&header);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("recording-editor-bg-section-body");
    let hint = Label::new(Some(&t("Coming soon")));
    hint.add_css_class("recording-editor-zoom-hint");
    hint.set_xalign(0.0);
    body.append(&hint);

    let revealer = Revealer::new();
    revealer.set_reveal_child(false);
    revealer.set_transition_type(RevealerTransitionType::SlideDown);
    revealer.set_transition_duration(180);
    revealer.set_child(Some(&body));
    section.append(&revealer);

    title.connect_clicked({
        let revealer = revealer.clone();
        let plus = plus.clone();
        move |_| {
            let revealed = revealer.reveals_child();
            revealer.set_reveal_child(!revealed);
            plus.set_text(if revealed { "+" } else { "\u{2212}" });
        }
    });

    BgSection { widget: section }
}

/// Open the Custom Wallpaper dialog. The dialog itself lands with the
/// gradient editor; until then the color path is the one that works.
fn open_custom_wallpaper_dialog(
    widget: &impl IsA<Widget>,
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) {
    open_background_color_dialog(widget, state, on_change)
}

/// Pick any image from disk to sit behind the video.
fn pick_background_image(
    widget: &impl IsA<Widget>,
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) {
    let Some(chooser) = pick_image_chooser(widget) else {
        return;
    };
    chooser.connect_response(move |dialog, response| {
        if response == gtk4::ResponseType::Accept {
            if let Some(path) = dialog.file().and_then(|file| file.path()) {
                state.lock().unwrap().background = VideoBackground::Wallpaper(path);
                on_change();
            }
        }
    });
    chooser.show();
}

fn pick_image_chooser(widget: &impl IsA<Widget>) -> Option<gtk4::FileChooserNative> {
    let root = widget.root()?;
    let window = root.downcast::<Window>().ok();
    let cancel = t("Cancel");
    let chooser = gtk4::FileChooserNative::new(
        Some(&t("Select background image")),
        window.as_ref(),
        gtk4::FileChooserAction::Open,
        Some(&t("Select")),
        Some(&cancel),
    );
    let filter = gtk4::FileFilter::new();
    filter.set_name(Some(&t("Images")));
    for mime in ["image/png", "image/jpeg", "image/webp"] {
        filter.add_mime_type(mime);
    }
    for pattern in ["*.png", "*.jpg", "*.jpeg", "*.webp"] {
        filter.add_pattern(pattern);
    }
    chooser.add_filter(&filter);
    Some(chooser)
}


// Small bundled thumbs decode fast; a missing thumb leaves the tile empty
// rather than decoding a multi-megapixel wallpaper on the UI thread.
fn decode_wallpaper_thumb(path: &PathBuf) -> Option<gtk4::cairo::ImageSurface> {
    let image = image::open(path).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let stride = gtk4::cairo::Format::ARgb32.stride_for_width(width).ok()?;
    // Premultiplied ARgb32 bytes, matching the image editor's conversion.
    let data: Vec<u8> = image
        .pixels()
        .flat_map(|pixel| {
            let [r, g, b, a] = pixel.0;
            let a = a as u32;
            let pr = ((r as u32 * a + 127) / 255) as u8;
            let pg = ((g as u32 * a + 127) / 255) as u8;
            let pb = ((b as u32 * a + 127) / 255) as u8;
            [pb, pg, pr, a as u8]
        })
        .collect();
    gtk4::cairo::ImageSurface::create_for_data(
        data,
        gtk4::cairo::Format::ARgb32,
        width as i32,
        height as i32,
        stride,
    )
    .ok()
}

// Cover-fit paint with the same rounded corners as the image editor tiles.
fn paint_wallpaper_thumb(
    cr: &gtk4::cairo::Context,
    surface: &gtk4::cairo::ImageSurface,
    width: i32,
    height: i32,
) {
    let source_w = surface.width().max(1) as f64;
    let source_h = surface.height().max(1) as f64;
    let scale = (width as f64 / source_w).max(height as f64 / source_h);
    let _ = cr.save();
    wallpaper_thumb_rounded_rect(cr, 0.0, 0.0, width as f64, height as f64, 11.0);
    cr.clip();
    cr.translate(
        (width as f64 - source_w * scale) * 0.5,
        (height as f64 - source_h * scale) * 0.5,
    );
    cr.scale(scale, scale);
    let _ = cr.set_source_surface(surface, 0.0, 0.0);
    let _ = cr.paint();
    let _ = cr.restore();
}

fn wallpaper_thumb_rounded_rect(
    cr: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    r: f64,
) {
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

fn open_background_color_dialog(
    widget: &impl IsA<Widget>,
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) {
    let (r, g, b) = match &state.lock().unwrap().background {
        VideoBackground::Plain { r, g, b } => (*r, *g, *b),
        _ => (17, 17, 17),
    };
    let initial = gdk::RGBA::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0);
    let parent = widget
        .root()
        .and_then(|root| root.downcast::<Window>().ok());
    let dialog = ColorChooserDialog::new(Some(&t("Background color")), parent.as_ref());
    dialog.set_modal(true);
    dialog.set_use_alpha(false);
    dialog.set_rgba(&initial);
    // Custom colors in the chooser use the shared image-editor slots; video
    // intentionally keeps only its own presets plus this dialog.
    let _ = Image::from_icon_name("dialog-cancel-symbolic");
    dialog.connect_response(move |dialog, response| {
        if response == gtk4::ResponseType::Ok {
            let color = dialog.rgba();
            state.lock().unwrap().background = VideoBackground::Plain {
                r: (color.red() * 255.0).round().clamp(0.0, 255.0) as u8,
                g: (color.green() * 255.0).round().clamp(0.0, 255.0) as u8,
                b: (color.blue() * 255.0).round().clamp(0.0, 255.0) as u8,
            };
            on_change();
        }
        dialog.close();
    });
    dialog.present();
}
