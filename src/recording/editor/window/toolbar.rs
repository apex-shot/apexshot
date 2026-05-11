use gtk4::{prelude::*, Align, ApplicationWindow, Box as GtkBox, CenterBox, Label, Orientation};

pub(super) fn build_toolbar(window: &ApplicationWindow, file_stem: &str) -> CenterBox {
    let controls = CenterBox::new();
    controls.add_css_class("recording-editor-window-controls");
    controls.set_can_target(true);
    controls.set_size_request(-1, 30);

    let title = Label::new(Some(file_stem));
    title.add_css_class("recording-editor-title");
    title.set_can_target(false);
    title.set_hexpand(false);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_width_request(1);
    title.set_size_request(-1, 30);
    controls.set_center_widget(Some(&title));

    let close =
        crate::capture::editor::ui_support::traffic_light_button("traffic-light-red", "Close");
    close.remove_css_class("recent-captures-wm-btn");
    close.remove_css_class("recent-captures-wm-close");
    close.add_css_class("recording-editor-traffic-btn");
    let minimize = crate::capture::editor::ui_support::traffic_light_button(
        "traffic-light-yellow",
        "Minimize",
    );
    minimize.remove_css_class("recent-captures-wm-btn");
    minimize.add_css_class("recording-editor-traffic-btn");
    let zoom =
        crate::capture::editor::ui_support::traffic_light_button("traffic-light-green", "Zoom");
    zoom.remove_css_class("recent-captures-wm-btn");
    zoom.add_css_class("recording-editor-traffic-btn");

    for button in [&close, &minimize, &zoom] {
        button.set_size_request(24, 24);
        button.set_valign(Align::Center);
    }

    let right_box = GtkBox::new(Orientation::Horizontal, 6);
    right_box.set_halign(Align::End);
    right_box.set_margin_end(4);
    right_box.append(&minimize);
    right_box.append(&zoom);
    right_box.append(&close);
    controls.set_end_widget(Some(&right_box));

    let window_close = window.clone();
    close.connect_clicked(move |_| window_close.close());

    let window_minimize = window.clone();
    minimize.connect_clicked(move |_| window_minimize.minimize());

    let window_zoom = window.clone();
    zoom.connect_clicked(move |_| {
        if window_zoom.is_maximized() {
            window_zoom.unmaximize();
        } else {
            window_zoom.maximize();
        }
    });

    controls
}
