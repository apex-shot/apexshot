use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::settings::windowing;
use gtk4::cairo::{FontSlant, FontWeight};
use gtk4::gdk;
use gtk4::gio;
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, DrawingArea,
    DropTarget, FileChooserAction, FileChooserNative, FileFilter, Image, Label, Orientation,
    ResponseType,
};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

const TIMELINE_HEIGHT: i32 = 226;

type UpdateDocument = Rc<dyn Fn(Option<&Path>)>;

pub(super) fn build_window(app: &Application, initial: Option<&Path>) {
    install_css();

    let scale = crate::platform::ui_scale();
    let (default_width, default_height) = default_window_size(scale);
    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("ApexShot Video Editor"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(default_width)
        .default_height(default_height)
        .build();
    window.set_decorated(false);
    window.add_css_class("editor-window");
    window.add_css_class("video-editor-window");
    let minimum = scaled_pair(920.0, 620.0, scale);
    window.set_size_request(minimum.0, minimum.1);

    let prefers_dark = windowing::prefers_dark_glass_theme();
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class("recording-editor-root");
    root.add_css_class("video-editor-root");
    root.add_css_class(if prefers_dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });
    if !prefers_dark {
        window.add_css_class("editor-theme-light");
    }
    if windowing::prefers_reduced_transparency() {
        root.add_css_class("editor-reduced-transparency");
    }

    let document_title = Rc::new(RefCell::new(document_title(initial)));
    let title_bar = build_title_bar(&window, document_title.clone(), scale);
    root.append(&title_bar);

    let workspace = GtkBox::new(Orientation::Horizontal, 0);
    workspace.add_css_class("recording-editor-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);

    let (stage, stage_title, stage_detail, open_button) =
        build_stage(scale, document_title.borrow().clone());
    workspace.append(&stage);

    let tools_column = super::side_panel::build_tools_column(scale, &window);
    workspace.append(&tools_column);
    root.append(&workspace);

    let timeline = build_timeline(scale, prefers_dark);
    root.append(&timeline);

    window.set_child(Some(&root));

    let update_document: UpdateDocument = {
        let document_title = document_title.clone();
        let title_bar = title_bar.clone();
        let stage_title = stage_title.clone();
        let stage_detail = stage_detail.clone();
        Rc::new(move |path| {
            let title = document_title_for_path(path);
            *document_title.borrow_mut() = title.clone();
            set_title_bar_text(&title_bar, &title);
            stage_title.set_text(&title);
            stage_detail.set_text(&if path.is_some() {
                t(
                    "This recording is selected. Preview, editing, and export are coming in the next Windows editor pass.",
                )
            } else {
                t(
                    "Choose an MP4 recording to begin. Preview, editing, and export are not connected on Windows yet.",
                )
            });
        })
    };
    update_document(initial);

    open_button.connect_clicked({
        let window = window.clone();
        let update_document = update_document.clone();
        move |_| choose_recording(&window, update_document.clone())
    });

    let drop_target = DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    drop_target.connect_drop({
        let update_document = update_document.clone();
        move |_, value, _, _| {
            let Ok(file) = value.get::<gio::File>() else {
                return false;
            };
            let Some(path) = file.path() else {
                return false;
            };
            if !is_supported_video(&path) {
                return false;
            }
            update_document(Some(&path));
            true
        }
    });
    stage.add_controller(drop_target);

    window.present();
}

fn build_title_bar(
    window: &ApplicationWindow,
    document_title: Rc<RefCell<String>>,
    scale: f64,
) -> GtkBox {
    let bar = GtkBox::new(Orientation::Horizontal, 0);
    bar.add_css_class("settings-window-controls");
    bar.add_css_class("video-editor-titlebar");
    bar.set_size_request(-1, (52.0 * scale).round() as i32);

    let left_spacer = GtkBox::new(Orientation::Horizontal, 0);
    left_spacer.set_hexpand(true);
    left_spacer.set_halign(Align::Fill);
    bar.append(&left_spacer);

    let title = Label::new(Some(&document_title.borrow()));
    title.add_css_class("recording-editor-title");
    title.add_css_class("video-editor-document-title");
    title.set_hexpand(true);
    title.set_halign(Align::Center);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(72);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    title.set_can_target(false);
    bar.append(&title);

    let right_spacer = GtkBox::new(Orientation::Horizontal, 0);
    right_spacer.set_hexpand(true);
    right_spacer.set_halign(Align::Fill);
    bar.append(&right_spacer);

    let close = crate::settings::ui_support::traffic_light_button("traffic-light-red", &t("Close"));
    close.remove_css_class("recent-captures-wm-btn");
    close.remove_css_class("recent-captures-wm-close");
    close.add_css_class("recording-editor-traffic-btn");
    let close_window = window.clone();
    close.connect_clicked(move |_| close_window.close());

    let minimize =
        crate::settings::ui_support::traffic_light_button("traffic-light-yellow", &t("Minimize"));
    minimize.remove_css_class("recent-captures-wm-btn");
    minimize.add_css_class("recording-editor-traffic-btn");
    let minimize_window = window.clone();
    minimize.connect_clicked(move |_| minimize_window.minimize());

    let maximize =
        crate::settings::ui_support::traffic_light_button("traffic-light-green", &t("Maximize"));
    maximize.remove_css_class("recent-captures-wm-btn");
    maximize.add_css_class("recording-editor-traffic-btn");
    let maximize_window = window.clone();
    maximize.connect_clicked(move |_| {
        if maximize_window.is_maximized() {
            maximize_window.unmaximize();
        } else {
            maximize_window.maximize();
        }
    });

    for button in [&close, &minimize, &maximize] {
        let size = (28.0 * scale).round() as i32;
        button.set_size_request(size, size);
        button.set_valign(Align::Center);
    }

    let actions = GtkBox::new(Orientation::Horizontal, 6);
    actions.set_halign(Align::End);
    actions.set_valign(Align::Center);
    actions.append(&minimize);
    actions.append(&maximize);
    actions.append(&close);
    bar.append(&actions);

    windowing::install_window_drag(&left_spacer, window);
    windowing::install_window_drag(&right_spacer, window);
    bar
}

fn set_title_bar_text(bar: &GtkBox, title: &str) {
    let mut child = bar.first_child();
    while let Some(widget) = child {
        if widget.has_css_class("video-editor-document-title") {
            if let Ok(label) = widget.downcast::<Label>() {
                label.set_text(title);
                label.set_tooltip_text(Some(title));
            }
            return;
        }
        child = widget.next_sibling();
    }
}

fn build_stage(scale: f64, initial_title: String) -> (GtkBox, Label, Label, Button) {
    let stage = GtkBox::new(Orientation::Vertical, 0);
    stage.add_css_class("recording-editor-stage");
    stage.set_hexpand(true);
    stage.set_vexpand(true);

    let empty = GtkBox::new(Orientation::Vertical, 12);
    empty.add_css_class("video-editor-empty");
    empty.set_halign(Align::Center);
    empty.set_valign(Align::Center);
    empty.set_hexpand(true);
    empty.set_vexpand(true);

    let icon_shell = GtkBox::new(Orientation::Vertical, 0);
    icon_shell.add_css_class("video-editor-empty-icon");
    icon_shell.set_halign(Align::Center);
    icon_shell.set_valign(Align::Center);
    icon_shell.set_size_request((64.0 * scale).round() as i32, (64.0 * scale).round() as i32);
    let icon = Image::from_icon_name(icon_names::custom::CLAPPERBOARD_SYMBOLIC);
    icon.set_pixel_size((28.0 * scale).round() as i32);
    icon.set_halign(Align::Center);
    icon.set_valign(Align::Center);
    icon_shell.append(&icon);
    empty.append(&icon_shell);

    let title = Label::new(Some(&initial_title));
    title.add_css_class("video-editor-empty-title");
    title.set_halign(Align::Center);
    empty.append(&title);

    let detail = Label::new(Some(&t(
        "Choose an MP4 recording to begin. Preview, editing, and export are not connected on Windows yet.",
    )));
    detail.add_css_class("video-editor-empty-detail");
    detail.set_justify(gtk4::Justification::Center);
    detail.set_wrap(true);
    detail.set_max_width_chars(52);
    detail.set_halign(Align::Center);
    empty.append(&detail);

    let open_button = Button::with_label(&t("Open recording…"));
    open_button.add_css_class("video-editor-open-button");
    open_button.set_halign(Align::Center);
    open_button.set_margin_top((4.0 * scale).round() as i32);
    empty.append(&open_button);
    stage.append(&empty);

    let stage_tools = GtkBox::new(Orientation::Horizontal, 8);
    stage_tools.add_css_class("recording-editor-stage-tools");
    stage_tools.set_halign(Align::Fill);
    stage_tools.set_valign(Align::End);
    stage_tools.set_margin_start((10.0 * scale).round() as i32);
    stage_tools.set_margin_end((10.0 * scale).round() as i32);
    let source_label = Label::new(Some(&t("Source")));
    source_label.add_css_class("video-editor-stage-meta");
    source_label.set_xalign(0.0);
    stage_tools.append(&source_label);
    let source_value = Label::new(Some(&t("Not loaded")));
    source_value.add_css_class("video-editor-stage-meta-value");
    source_value.set_xalign(0.0);
    stage_tools.append(&source_value);
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    stage_tools.append(&spacer);
    let original = Button::with_label(&t("Original"));
    original.add_css_class("recording-editor-stage-chip");
    original.set_sensitive(false);
    stage_tools.append(&original);
    stage.append(&stage_tools);

    (stage, title, detail, open_button)
}

fn build_timeline(scale: f64, prefers_dark: bool) -> GtkBox {
    let dock = GtkBox::new(Orientation::Vertical, 0);
    dock.add_css_class("recording-editor-timeline-dock");
    dock.add_css_class("video-editor-timeline-dock");
    dock.set_valign(Align::End);

    let toolbar = GtkBox::new(Orientation::Horizontal, 8);
    toolbar.add_css_class("recording-editor-timeline-toolbar");
    toolbar.set_size_request(-1, (38.0 * scale).round() as i32);

    let rewind = timeline_icon_button("media-skip-backward-symbolic", &t("Back"));
    rewind.set_sensitive(false);
    toolbar.append(&rewind);
    let play = timeline_icon_button("media-playback-start-symbolic", &t("Play"));
    play.add_css_class("recording-editor-play-button");
    play.set_sensitive(false);
    toolbar.append(&play);
    let forward = timeline_icon_button("media-skip-forward-symbolic", &t("Forward"));
    forward.set_sensitive(false);
    toolbar.append(&forward);

    let clock = Label::new(Some("00:00.00  /  --:--.--"));
    clock.add_css_class("recording-editor-timeline-clock");
    clock.set_margin_start((6.0 * scale).round() as i32);
    toolbar.append(&clock);

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    toolbar.append(&spacer);

    let mode = Label::new(Some(&t("Timeline")));
    mode.add_css_class("video-editor-timeline-mode");
    toolbar.append(&mode);

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    toolbar.append(&spacer);

    let zoom_icon = Image::from_icon_name("zoom-in-symbolic");
    zoom_icon.set_pixel_size((14.0 * scale).round() as i32);
    zoom_icon.add_css_class("video-editor-timeline-zoom");
    toolbar.append(&zoom_icon);
    let zoom = gtk4::Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
    zoom.set_value(28.0);
    zoom.set_sensitive(false);
    zoom.set_size_request((110.0 * scale).round() as i32, -1);
    zoom.add_css_class("recording-editor-timeline-scale");
    toolbar.append(&zoom);
    dock.append(&toolbar);

    let board = DrawingArea::new();
    board.add_css_class("video-editor-timeline-board");
    board.set_hexpand(true);
    board.set_content_height((TIMELINE_HEIGHT as f64 * scale).round() as i32);
    board.set_draw_func(move |_, context, width, height| {
        draw_timeline(context, width, height, scale, prefers_dark);
    });
    dock.append(&board);
    dock
}

fn timeline_icon_button(icon_name: &str, tooltip: &str) -> Button {
    let button = Button::builder()
        .icon_name(icon_name)
        .has_frame(false)
        .focusable(false)
        .tooltip_text(tooltip)
        .build();
    button.add_css_class("recording-editor-timeline-icon");
    if let Some(image) = button.child().and_downcast::<Image>() {
        image.set_pixel_size(14);
    }
    button
}

fn draw_timeline(
    context: &gtk4::cairo::Context,
    width: i32,
    height: i32,
    scale: f64,
    prefers_dark: bool,
) {
    let width = f64::from(width);
    let height = f64::from(height);
    let foreground = if prefers_dark {
        (0.88, 0.88, 0.91)
    } else {
        (0.12, 0.15, 0.20)
    };
    let muted = if prefers_dark {
        (0.58, 0.59, 0.63)
    } else {
        (0.43, 0.46, 0.52)
    };
    let line = if prefers_dark {
        (1.0, 1.0, 1.0, 0.10)
    } else {
        (0.07, 0.09, 0.13, 0.11)
    };

    let left = (118.0 * scale).round();
    let ruler = (34.0 * scale).round();
    let row_height = ((height - ruler) / 3.0).min(58.0 * scale);
    let track_top = ruler;

    context.set_line_width(1.0);
    context.set_source_rgba(1.0, 1.0, 1.0, 0.0);
    context.paint().ok();

    context.set_source_rgba(line.0, line.1, line.2, line.3);
    context.move_to(left, ruler);
    context.line_to(width, ruler);
    context.stroke().ok();

    context.select_font_face("Inter", FontSlant::Normal, FontWeight::Normal);
    context.set_font_size((10.0 * scale).round());
    for (index, label) in ["0:00", "0:10", "0:20", "0:30", "0:40"].iter().enumerate() {
        let x = left + (width - left) * index as f64 / 4.0;
        context.set_source_rgba(muted.0, muted.1, muted.2, 0.90);
        let _ = context.show_text(label);
        context.set_source_rgba(line.0, line.1, line.2, line.3);
        context.move_to(x, ruler - (5.0 * scale).round());
        context.line_to(x, height);
        context.stroke().ok();
    }

    let rows = [
        (t("Video"), t("Drop a recording here")),
        (t("Zoom"), t("No zoom clips")),
        (t("Cursor"), t("No cursor clips")),
    ];
    for (index, (name, hint)) in rows.into_iter().enumerate() {
        let y = track_top + index as f64 * row_height;
        context.set_source_rgba(line.0, line.1, line.2, line.3);
        context.move_to(0.0, y);
        context.line_to(width, y);
        context.stroke().ok();

        context.set_source_rgba(foreground.0, foreground.1, foreground.2, 0.82);
        context.set_font_size((11.0 * scale).round());
        let _ = context.show_text(&name);

        let body_x = left + (10.0 * scale).round();
        let body_y = y + (11.0 * scale).round();
        let body_w = (width - left - (20.0 * scale).round()).max(1.0);
        let body_h = (row_height - (22.0 * scale).round()).max(14.0);
        rounded_rect(context, body_x, body_y, body_w, body_h, 6.0 * scale);
        context.set_source_rgba(line.0, line.1, line.2, line.3 * 0.8);
        context.fill().ok();

        context.set_source_rgba(muted.0, muted.1, muted.2, 0.62);
        context.set_font_size((10.0 * scale).round());
        context.move_to(body_x + (10.0 * scale).round(), body_y + body_h * 0.62);
        let _ = context.show_text(&hint);
    }
}

fn rounded_rect(
    context: &gtk4::cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) {
    let right = x + width;
    let bottom = y + height;
    context.new_sub_path();
    context.arc(
        right - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        right - radius,
        bottom - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        x + radius,
        bottom - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    context.close_path();
}

fn choose_recording(window: &ApplicationWindow, update_document: UpdateDocument) {
    let chooser = FileChooserNative::new(
        Some(&t("Open recording")),
        Some(window),
        FileChooserAction::Open,
        Some(&t("Open")),
        Some(&t("Cancel")),
    );
    let filter = FileFilter::new();
    filter.set_name(Some(&t("MP4 recordings")));
    filter.add_pattern("*.mp4");
    filter.add_pattern("*.MP4");
    chooser.set_filter(&filter);
    chooser.connect_response(move |chooser, response| {
        if response == ResponseType::Accept {
            if let Some(path) = chooser.file().and_then(|file| file.path()) {
                if is_supported_video(&path) {
                    update_document(Some(&path));
                }
            }
        }
        chooser.hide();
    });
    chooser.show();
}

fn document_title(path: Option<&Path>) -> String {
    path.map(|path| document_title_for_path(Some(path)))
        .unwrap_or_else(|| t("Untitled recording"))
}

fn document_title_for_path(path: Option<&Path>) -> String {
    path.and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| t("Untitled recording"))
}

fn is_supported_video(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"))
}

fn default_window_size(scale: f64) -> (i32, i32) {
    let requested = scaled_pair(1280.0, 860.0, scale);
    let Some(display) = gdk::Display::default() else {
        return requested;
    };
    let Some(monitor) = display.monitors().item(0).and_downcast::<gdk::Monitor>() else {
        return requested;
    };
    let geometry = monitor.geometry();
    let max_width = (f64::from(geometry.width()) * 0.92).round() as i32;
    let max_height = (f64::from(geometry.height()) * 0.90).round() as i32;
    (
        requested.0.min(max_width).max(1),
        requested.1.min(max_height).max(1),
    )
}

fn scaled_pair(width: f64, height: f64, scale: f64) -> (i32, i32) {
    (
        (width * scale).round() as i32,
        (height * scale).round() as i32,
    )
}

fn install_css() {
    crate::settings::install_window_css();
    super::ui_support::install_recording_editor_css();
    super::side_panel::install_css();

    let Some(display) = gdk::Display::default() else {
        return;
    };
    let scale = crate::platform::ui_scale();
    let px = |value: f64| (value * scale).round() as i32;
    let css = format!(
        r#"
        window.video-editor-window {{
            background: transparent;
        }}
        .video-editor-root {{
            border: 1px solid rgba(255,255,255,0.12);
            border-radius: 12px;
        }}
        .video-editor-root.editor-theme-light {{
            border: 1px solid rgba(17,24,39,0.12);
        }}
        .video-editor-titlebar {{
            min-height: {titlebar}px;
            padding: 0 0 0 0;
            border-radius: 12px 12px 0 0;
        }}
        .video-editor-document-title {{
            font-size: {title}px;
            font-weight: 500;
        }}
        .video-editor-empty {{
            padding: {empty_pad}px;
        }}
        .video-editor-empty-icon {{
            background: alpha(white,0.06);
            border: 1px solid alpha(white,0.08);
            border-radius: {icon_radius}px;
            color: alpha(white,0.82);
        }}
        .editor-theme-light .video-editor-empty-icon {{
            background: alpha(#111827,0.05);
            border-color: alpha(#111827,0.08);
            color: alpha(#111827,0.76);
        }}
        .video-editor-empty-title {{
            color: alpha(white,0.94);
            font-size: {empty_title}px;
            font-weight: 600;
        }}
        .editor-theme-light .video-editor-empty-title {{
            color: alpha(#111827,0.92);
        }}
        .video-editor-empty-detail {{
            color: alpha(white,0.54);
            font-size: {empty_detail}px;
        }}
        .editor-theme-light .video-editor-empty-detail {{
            color: alpha(#111827,0.56);
        }}
        button.video-editor-open-button {{
            min-height: {button_h}px;
            padding: 0 {button_x}px;
            border-radius: {button_radius}px;
            background: #ff670d;
            color: #ffffff;
            font-size: {button_text}px;
            font-weight: 600;
        }}
        button.video-editor-open-button:hover {{
            background: #ff7c2f;
        }}
        button.video-editor-open-button:active {{
            background: #dd5909;
        }}
        .video-editor-stage-meta {{
            color: alpha(white,0.40);
            font-size: {meta}px;
        }}
        .video-editor-stage-meta-value {{
            color: alpha(white,0.68);
            font-size: {meta}px;
        }}
        .editor-theme-light .video-editor-stage-meta {{
            color: alpha(#111827,0.44);
        }}
        .editor-theme-light .video-editor-stage-meta-value {{
            color: alpha(#111827,0.68);
        }}
        .recording-editor-tools-column {{
            min-width: {tools_w}px;
            max-width: {tools_w}px;
        }}
        .video-editor-tool-panel {{
            padding: {panel_pad}px {panel_side}px;
        }}
        .video-editor-panel-head {{
            padding: {panel_head_y}px 0 {panel_head_bottom}px 0;
        }}
        .video-editor-panel-title {{
            color: alpha(white,0.92);
            font-size: {panel_title}px;
            font-weight: 600;
        }}
        .editor-theme-light .video-editor-panel-title {{
            color: alpha(#111827,0.92);
        }}
        .video-editor-panel-detail {{
            color: alpha(white,0.48);
            font-size: {panel_detail}px;
        }}
        .editor-theme-light .video-editor-panel-detail {{
            color: alpha(#111827,0.54);
        }}
        .video-editor-panel-rule {{
            min-height: 1px;
            background: alpha(white,0.08);
        }}
        .editor-theme-light .video-editor-panel-rule {{
            background: alpha(#111827,0.09);
        }}
        .video-editor-panel-placeholder {{
            margin-top: {placeholder_top}px;
            color: alpha(white,0.38);
            font-size: {placeholder}px;
        }}
        .editor-theme-light .video-editor-panel-placeholder {{
            color: alpha(#111827,0.46);
        }}
        .video-editor-timeline-dock {{
            min-height: {timeline_min}px;
        }}
        .video-editor-timeline-board {{
            min-height: {timeline_h}px;
        }}
        .video-editor-timeline-mode {{
            color: alpha(white,0.52);
            font-size: {mode}px;
            font-weight: 600;
        }}
        .editor-theme-light .video-editor-timeline-mode {{
            color: alpha(#111827,0.52);
        }}
        .video-editor-timeline-zoom {{
            color: alpha(white,0.46);
        }}
        .editor-theme-light .video-editor-timeline-zoom {{
            color: alpha(#111827,0.46);
        }}
        "#,
        titlebar = px(52.0),
        title = px(12.0),
        empty_pad = px(24.0),
        icon_radius = px(18.0),
        empty_title = px(20.0),
        empty_detail = px(13.0),
        button_h = px(34.0),
        button_x = px(18.0),
        button_radius = px(9.0),
        button_text = px(12.0),
        meta = px(11.0),
        tools_w = px(super::side_panel::TOOL_COLUMN_WIDTH as f64),
        panel_pad = px(10.0),
        panel_side = px(16.0),
        panel_head_y = px(14.0),
        panel_head_bottom = px(12.0),
        panel_title = px(15.0),
        panel_detail = px(12.0),
        placeholder_top = px(16.0),
        placeholder = px(12.0),
        timeline_min = px((TIMELINE_HEIGHT + 40) as f64),
        timeline_h = px(TIMELINE_HEIGHT as f64),
        mode = px(11.0),
    );

    let provider = CssProvider::new();
    provider.load_from_data(&css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 4,
    );
}
