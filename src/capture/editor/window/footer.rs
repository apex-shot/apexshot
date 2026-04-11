use gtk4::{prelude::*, Box as GtkBox, Button, Label, Orientation};

use super::super::ui_support::footer_icon_button;

pub(super) struct FooterParts {
    pub root: GtkBox,
    pub zoom_button: Button,
    pub zoom_label: Label,
    pub zoom_header_label: Label,
    pub zoom_popup: GtkBox,
    pub zoom_minus_btn: Button,
    pub zoom_plus_btn: Button,
    pub zoom_in_btn: Button,
    pub zoom_out_btn: Button,
    pub fit_to_screen_btn: Button,
    pub zoom_to_selection_btn: Button,
    pub copy_btn: Button,
    pub upload_btn: Button,
}

fn build_zoom_row(label: &str, shortcut: &str) -> (Button, GtkBox) {
    let row = GtkBox::new(Orientation::Horizontal, 0);
    row.add_css_class("editor-footer-zoom-row");

    let label_widget = Label::new(Some(label));
    label_widget.set_hexpand(true);
    label_widget.set_xalign(0.0);

    let shortcut_box = GtkBox::new(Orientation::Horizontal, 4);
    shortcut_box.add_css_class("editor-footer-zoom-shortcut-box");

    for part in shortcut.split(' ') {
        let part_label = Label::new(Some(part));
        part_label.add_css_class("editor-footer-zoom-shortcut-part");
        shortcut_box.append(&part_label);
    }

    row.append(&label_widget);
    row.append(&shortcut_box);

    let btn = Button::builder()
        .has_frame(false)
        .css_classes(["flat", "editor-footer-zoom-action-btn"])
        .child(&row)
        .build();

    (btn, row)
}

fn build_mouse_hints() -> GtkBox {
    let container = GtkBox::new(Orientation::Horizontal, 0);
    container.add_css_class("editor-footer-zoom-mouse-hints");
    container.set_halign(gtk4::Align::Center);

    let left_text = Label::new(Some("Zoom with\nthe scroll\nwheel"));
    left_text.set_justify(gtk4::Justification::Right);
    left_text.add_css_class("editor-footer-zoom-mouse-hint-text");

    let drawing = gtk4::DrawingArea::new();
    drawing.add_css_class("editor-footer-zoom-mouse-drawing");
    drawing.set_content_width(60);
    drawing.set_content_height(60);
    drawing.set_draw_func(move |_, cr, width, height| {
        let w = f64::from(width);
        let h = f64::from(height);

        // Draw mouse body (simple rounded rect)
        let mouse_w = 24.0;
        let mouse_h = 40.0;
        let mouse_x = (w - mouse_w) / 2.0;
        let mouse_y = h - mouse_h - 5.0;
        let radius = 10.0;

        cr.set_source_rgba(1.0, 1.0, 1.0, 0.1);
        cr.set_line_width(1.0);
        cr.new_sub_path();
        cr.arc(
            mouse_x + radius,
            mouse_y + radius,
            radius,
            180.0 * std::f64::consts::PI / 180.0,
            270.0 * std::f64::consts::PI / 180.0,
        );
        cr.arc(
            mouse_x + mouse_w - radius,
            mouse_y + radius,
            radius,
            270.0 * std::f64::consts::PI / 180.0,
            360.0 * std::f64::consts::PI / 180.0,
        );
        cr.line_to(mouse_x + mouse_w, mouse_y + mouse_h);
        cr.line_to(mouse_x, mouse_y + mouse_h);
        cr.close_path();
        let _ = cr.stroke();

        // Draw middle line for buttons
        cr.move_to(mouse_x + mouse_w / 2.0, mouse_y);
        cr.line_to(mouse_x + mouse_w / 2.0, mouse_y + 15.0);
        let _ = cr.stroke();

        // Draw scroll wheel (highlighted blue)
        let wheel_w = 4.0;
        let wheel_h = 10.0;
        let wheel_x = (w - wheel_w) / 2.0;
        let wheel_y = mouse_y + 8.0;
        cr.set_source_rgba(0.0, 0.5, 1.0, 0.8);
        cr.new_sub_path();
        cr.arc(
            wheel_x + wheel_w / 2.0,
            wheel_y + wheel_w / 2.0,
            wheel_w / 2.0,
            180.0 * std::f64::consts::PI / 180.0,
            360.0 * std::f64::consts::PI / 180.0,
        );
        cr.arc(
            wheel_x + wheel_w / 2.0,
            wheel_y + wheel_h - wheel_w / 2.0,
            wheel_w / 2.0,
            0.0,
            180.0 * std::f64::consts::PI / 180.0,
        );
        cr.close_path();
        let _ = cr.fill();

        // Draw lines pointing to hints
        cr.set_source_rgba(0.0, 0.5, 1.0, 0.4);
        cr.set_line_width(0.8);

        // To scroll wheel
        cr.move_to(mouse_x - 5.0, wheel_y + wheel_h / 2.0);
        cr.curve_to(
            mouse_x - 15.0,
            wheel_y + wheel_h / 2.0,
            wheel_x - 5.0,
            wheel_y + wheel_h / 2.0,
            wheel_x - 2.0,
            wheel_y + wheel_h / 2.0,
        );
        let _ = cr.stroke();

        // To right button
        cr.move_to(w - (mouse_x - 5.0), wheel_y + wheel_h / 2.0);
        cr.curve_to(
            w - (mouse_x - 15.0),
            wheel_y + wheel_h / 2.0,
            wheel_x + wheel_w + 5.0,
            wheel_y + wheel_h / 2.0,
            wheel_x + wheel_w + 2.0,
            wheel_y + wheel_h / 2.0,
        );
        let _ = cr.stroke();
    });

    let right_text = Label::new(Some("Pan with\nthe right\nbutton"));
    right_text.set_justify(gtk4::Justification::Left);
    right_text.add_css_class("editor-footer-zoom-mouse-hint-text");

    container.append(&left_text);
    container.append(&drawing);
    container.append(&right_text);
    container
}

pub(super) fn build_footer(copy_icon_name: &str, upload_icon_name: &str) -> FooterParts {
    let zoom_button = Button::new();
    zoom_button.set_has_frame(false);
    zoom_button.set_tooltip_text(Some("Zoom controls"));
    zoom_button.add_css_class("editor-footer-zoom-button");

    let zoom_label = Label::new(Some("100%"));
    zoom_label.add_css_class("editor-footer-zoom-label");
    zoom_button.set_child(Some(&zoom_label));

    let zoom_popup = GtkBox::new(Orientation::Vertical, 0);
    zoom_popup.add_css_class("editor-footer-zoom-popup");
    zoom_popup.set_halign(gtk4::Align::Start);
    zoom_popup.set_valign(gtk4::Align::End);
    zoom_popup.set_margin_start(16);
    zoom_popup.set_margin_bottom(16);
    zoom_popup.set_visible(false);

    // Header
    let zoom_header = GtkBox::new(Orientation::Horizontal, 8);
    zoom_header.add_css_class("editor-footer-zoom-header");

    let zoom_minus_btn = Button::with_label("-");
    zoom_minus_btn.add_css_class("editor-footer-zoom-header-btn");
    zoom_minus_btn.add_css_class("flat");

    let zoom_header_label = Label::new(Some("100%"));
    zoom_header_label.set_hexpand(true);
    zoom_header_label.add_css_class("editor-footer-zoom-header-label");

    let zoom_plus_btn = Button::with_label("+");
    zoom_plus_btn.add_css_class("editor-footer-zoom-header-btn");
    zoom_plus_btn.add_css_class("orange-btn");
    zoom_plus_btn.add_css_class("flat");

    zoom_header.append(&zoom_minus_btn);
    zoom_header.append(&zoom_header_label);
    zoom_header.append(&zoom_plus_btn);

    // List of actions
    let zoom_list = GtkBox::new(Orientation::Vertical, 0);
    zoom_list.add_css_class("editor-footer-zoom-list");

    let (zoom_in_btn, _) = build_zoom_row("Zoom In", "Ctrl +");
    let (zoom_out_btn, _) = build_zoom_row("Zoom Out", "Ctrl -");
    let (fit_to_screen_btn, _) = build_zoom_row("Fit to Screen", "Ctrl 0");
    let (zoom_to_selection_btn, _) = build_zoom_row("Zoom to Selection", "Ctrl 2");

    let sep1 = GtkBox::new(Orientation::Horizontal, 0);
    sep1.add_css_class("editor-footer-zoom-separator");

    let sep2 = GtkBox::new(Orientation::Horizontal, 0);
    sep2.add_css_class("editor-footer-zoom-separator");

    let mouse_hints = build_mouse_hints();

    zoom_list.append(&zoom_in_btn);
    zoom_list.append(&zoom_out_btn);
    zoom_list.append(&fit_to_screen_btn);
    zoom_list.append(&zoom_to_selection_btn);

    zoom_popup.append(&zoom_header);
    zoom_popup.append(&sep1);
    zoom_popup.append(&zoom_list);
    zoom_popup.append(&sep2);
    zoom_popup.append(&mouse_hints);

    let (copy_btn, _) = footer_icon_button(copy_icon_name, "Copy file URI");
    let (upload_btn, _) = footer_icon_button(upload_icon_name, "Upload");

    let root = GtkBox::new(Orientation::Horizontal, 0);
    root.add_css_class("editor-footer");

    let footer_left = GtkBox::new(Orientation::Horizontal, 0);
    footer_left.set_hexpand(true);
    footer_left.set_halign(gtk4::Align::Start);
    footer_left.append(&zoom_button);

    let footer_right = GtkBox::new(Orientation::Horizontal, 6);
    footer_right.set_hexpand(true);
    footer_right.set_halign(gtk4::Align::End);
    footer_right.append(&copy_btn);
    footer_right.append(&upload_btn);

    root.append(&footer_left);
    root.append(&footer_right);

    FooterParts {
        root,
        zoom_button,
        zoom_label,
        zoom_header_label,
        zoom_popup,
        zoom_minus_btn,
        zoom_plus_btn,
        zoom_in_btn,
        zoom_out_btn,
        fit_to_screen_btn,
        zoom_to_selection_btn,
        copy_btn,
        upload_btn,
    }
}
