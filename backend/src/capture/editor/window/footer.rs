use gtk4::{prelude::*, Box as GtkBox, Button, Image, Orientation};

use super::super::ui_support::footer_icon_button;

pub(super) struct FooterParts {
    pub root: GtkBox,
    pub pin_btn: Button,
    pub pin_icon: Image,
    pub drag_btn: Button,
    pub copy_btn: Button,
    pub upload_btn: Button,
}

pub(super) fn build_footer(
    pin_icon_name: &str,
    copy_icon_name: &str,
    upload_icon_name: &str,
) -> FooterParts {
    let (pin_btn, pin_icon) = footer_icon_button(pin_icon_name, "Pin window");
    let drag_btn = Button::with_label("Drag me");
    drag_btn.set_has_frame(false);
    drag_btn.set_tooltip_text(Some("Drag to move editor window"));
    drag_btn.add_css_class("editor-footer-drag-button");
    drag_btn.add_css_class("body");
    let (copy_btn, _) = footer_icon_button(copy_icon_name, "Copy file URI");
    let (upload_btn, _) = footer_icon_button(upload_icon_name, "Upload");

    let root = GtkBox::new(Orientation::Horizontal, 0);
    root.add_css_class("editor-footer");

    let footer_left = GtkBox::new(Orientation::Horizontal, 0);
    footer_left.set_hexpand(true);
    footer_left.set_halign(gtk4::Align::Start);
    footer_left.append(&pin_btn);

    let footer_center = GtkBox::new(Orientation::Horizontal, 0);
    footer_center.set_hexpand(true);
    footer_center.set_halign(gtk4::Align::Center);
    footer_center.append(&drag_btn);

    let footer_right = GtkBox::new(Orientation::Horizontal, 6);
    footer_right.set_hexpand(true);
    footer_right.set_halign(gtk4::Align::End);
    footer_right.append(&copy_btn);
    footer_right.append(&upload_btn);

    root.append(&footer_left);
    root.append(&footer_center);
    root.append(&footer_right);

    FooterParts {
        root,
        pin_btn,
        pin_icon,
        drag_btn,
        copy_btn,
        upload_btn,
    }
}
