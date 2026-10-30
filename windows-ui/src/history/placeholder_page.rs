//! Placeholder pages for Windows History features whose backends are not ported.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Image, Label, Orientation, ScrolledWindow};

use crate::i18n::t;

use super::HistoryPage;

pub fn build_placeholder_page(icon_name: &str, detail: &str) -> HistoryPage {
    let scroller = ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_hexpand(true);

    // The same metrics the other pages use: the Settings page padding plus
    // the card inset on the edges the card leaves.
    let scale = super::window_scale().min(1.0);
    let column = GtkBox::new(Orientation::Vertical, 0);
    column.set_margin_top((12.0 * scale).round() as i32);
    column.set_margin_bottom((32.0 * scale).round() as i32);
    column.set_margin_start((20.0 * scale).round() as i32);
    column.set_margin_end((28.0 * scale).round() as i32);

    let empty = GtkBox::new(Orientation::Vertical, 8);
    empty.add_css_class("recent-captures-empty-state");
    empty.set_halign(Align::Center);
    empty.set_valign(Align::Center);
    empty.set_vexpand(true);

    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(48);
    icon.add_css_class("history-empty-icon");
    icon.set_halign(Align::Center);
    empty.append(&icon);

    let detail = Label::new(Some(detail));
    detail.add_css_class("recent-captures-empty-detail");
    detail.set_halign(Align::Center);
    detail.set_wrap(true);
    detail.set_max_width_chars(52);
    detail.set_justify(gtk4::Justification::Center);
    empty.append(&detail);

    column.append(&empty);
    scroller.set_child(Some(&column));

    HistoryPage {
        widget: scroller.upcast(),
        refresh: std::rc::Rc::new(|| {}),
        search_placeholder: t("Search isn't available on this page"),
        searchable: false,
    }
}
