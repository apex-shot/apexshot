use gtk4::{
    prelude::*, Align, Box as GtkBox, Button, DrawingArea, Image, Label, Orientation, PolicyType,
    Popover, ScrolledWindow, SearchEntry,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::i18n::t;
use crate::recording::editor::window::custom_wallpaper_popover::{build_color_picker, ColorPicker};

type FontChanged = Rc<RefCell<Option<Rc<dyn Fn(String)>>>>;
type ColorChanged = Rc<RefCell<Option<Rc<dyn Fn((u8, u8, u8))>>>>;

#[derive(Clone)]
pub(in crate::capture::editor::window) struct MotionFontPicker {
    button: Button,
    label: Label,
    list: GtkBox,
    families: Rc<Vec<String>>,
    selected: Rc<RefCell<String>>,
    changed: FontChanged,
}

impl MotionFontPicker {
    pub(super) fn new() -> Self {
        use pango::prelude::FontMapExt;
        let mut families = pangocairo::FontMap::default()
            .list_families()
            .iter()
            .map(|family| family.name().to_string())
            .collect::<Vec<_>>();
        families.sort();
        families.dedup();
        let families = Rc::new(families);
        let button = Button::new();
        button.set_has_frame(false);
        button.set_hexpand(true);
        button.add_css_class("editor-tool-button");
        button.add_css_class("editor-motion-font-button");
        button.set_tooltip_text(Some(&t("Font family")));
        let content = GtkBox::new(Orientation::Horizontal, 8);
        let label = Label::new(Some(crate::typography::UI_FONT_FAMILY));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_max_width_chars(24);
        label.set_ellipsize(pango::EllipsizeMode::End);
        let arrow = Image::from_icon_name(super::super::icon_names::CHEVRON_DOWN_REGULAR);
        arrow.set_pixel_size(12);
        content.append(&label);
        content.append(&arrow);
        button.set_child(Some(&content));

        let popover = Popover::new();
        popover.add_css_class("editor-popover");
        popover.add_css_class("editor-motion-font-popover");
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        popover.set_parent(&button);
        let body = GtkBox::new(Orientation::Vertical, 6);
        body.set_width_request(260);
        let search = SearchEntry::new();
        search.add_css_class("editor-motion-font-search");
        search.set_placeholder_text(Some(&t("Search fonts…")));
        search.set_search_delay(0);
        body.append(&search);
        let list = GtkBox::new(Orientation::Vertical, 0);
        list.add_css_class("editor-popover-list");
        let empty = Label::new(Some(&t("No fonts found")));
        empty.add_css_class("editor-inspector-placeholder");
        empty.set_margin_top(12);
        empty.set_margin_bottom(12);
        empty.set_visible(false);
        let scroll = ScrolledWindow::new();
        scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
        scroll.set_max_content_height(280);
        scroll.set_propagate_natural_height(true);
        scroll.set_child(Some(&list));
        body.append(&scroll);
        body.append(&empty);
        popover.set_child(Some(&body));

        let selected = Rc::new(RefCell::new(crate::typography::UI_FONT_FAMILY.to_string()));
        let changed: FontChanged = Rc::new(RefCell::new(None));
        let choose: Rc<dyn Fn(String)> = {
            let selected = selected.clone();
            let changed = changed.clone();
            let label = label.downgrade();
            let list = list.downgrade();
            let families = families.clone();
            Rc::new(move |family| {
                if *selected.borrow() == family {
                    return;
                }
                *selected.borrow_mut() = family.clone();
                if let Some(label) = label.upgrade() {
                    label.set_text(&family);
                }
                if let Some(list) = list.upgrade() {
                    super::super::sync_text_option_selection(
                        &list,
                        families.iter().position(|item| item == &family),
                    );
                }
                if let Some(callback) = changed.borrow().as_ref() {
                    callback(family);
                }
            })
        };
        let mut rows = Vec::new();
        for family in families.iter() {
            let row = GtkBox::new(Orientation::Horizontal, 8);
            let name = Label::new(Some(family));
            name.set_xalign(0.0);
            name.set_hexpand(true);
            name.set_max_width_chars(26);
            name.set_ellipsize(pango::EllipsizeMode::End);
            let check = Label::new(Some("✓"));
            check.add_css_class("editor-text-inspector-check");
            check.set_visible(family == crate::typography::UI_FONT_FAMILY);
            row.append(&name);
            row.append(&check);
            let option = Button::builder()
                .has_frame(false)
                .css_classes([
                    "editor-popover-list-item",
                    "flat",
                    "editor-text-inspector-option",
                ])
                .child(&row)
                .build();
            option.set_tooltip_text(Some(family));
            option.connect_clicked({
                let choose = choose.clone();
                let family = family.clone();
                let popover = popover.downgrade();
                move |_| {
                    choose(family.clone());
                    if let Some(popover) = popover.upgrade() {
                        popover.popdown();
                    }
                }
            });
            rows.push((family.clone(), option.downgrade()));
            list.append(&option);
        }
        search.connect_search_changed({
            let empty = empty.downgrade();
            move |entry| {
                let query = entry.text();
                let mut found = false;
                for (family, row) in &rows {
                    if let Some(row) = row.upgrade() {
                        let matches = font_matches_query(family, &query);
                        row.set_visible(matches);
                        found |= matches;
                    }
                }
                if let Some(empty) = empty.upgrade() {
                    empty.set_visible(!found);
                }
            }
        });
        search.connect_activate({
            let choose = choose.clone();
            let popover = popover.downgrade();
            let families = families.clone();
            move |entry| {
                let query = entry.text();
                if query.trim().is_empty() {
                    return;
                }
                if let Some(family) = families
                    .iter()
                    .find(|family| font_matches_query(family, &query))
                {
                    choose(family.clone());
                    if let Some(popover) = popover.upgrade() {
                        popover.popdown();
                    }
                }
            }
        });
        popover.connect_show({
            let search = search.downgrade();
            move |_| {
                if let Some(search) = search.upgrade() {
                    search.set_text("");
                }
            }
        });
        popover.connect_map({
            let search = search.downgrade();
            move |_| {
                if let Some(search) = search.upgrade() {
                    search.grab_focus();
                }
            }
        });
        button.connect_clicked(move |_| popover.popup());
        super::super::sync_text_option_selection(
            &list,
            families
                .iter()
                .position(|family| family == crate::typography::UI_FONT_FAMILY),
        );
        Self {
            button,
            label,
            list,
            families,
            selected,
            changed,
        }
    }

    pub(super) fn widget(&self) -> Button {
        self.button.clone()
    }

    pub(super) fn set_family(&self, family: &str) {
        if *self.selected.borrow() == family {
            return;
        }
        *self.selected.borrow_mut() = family.to_string();
        self.label.set_text(family);
        super::super::sync_text_option_selection(
            &self.list,
            self.families.iter().position(|item| item == family),
        );
    }

    pub(super) fn connect_changed(&self, callback: impl Fn(String) + 'static) {
        *self.changed.borrow_mut() = Some(Rc::new(callback));
    }
}

fn font_matches_query(family: &str, query: &str) -> bool {
    let family = family.to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| family.contains(word))
}

#[derive(Clone)]
pub(in crate::capture::editor::window) struct MotionTextColorPicker {
    row: GtkBox,
    swatch: DrawingArea,
    value: Rc<Cell<(u8, u8, u8)>>,
    picker: ColorPicker,
    changed: ColorChanged,
}

impl MotionTextColorPicker {
    pub(super) fn new() -> Self {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.add_css_class("recording-editor-bg-custom-row");
        row.set_hexpand(true);
        let swatch = DrawingArea::new();
        swatch.add_css_class("recording-editor-bg-custom-swatch");
        swatch.set_content_width(28);
        swatch.set_content_height(28);
        swatch.set_valign(Align::Center);
        swatch.set_can_target(false);
        let value = Rc::new(Cell::new((255, 255, 255)));
        swatch.set_draw_func({
            let value = value.clone();
            move |_, context, width, height| {
                let (red, green, blue) = value.get();
                crate::capture::editor::render::rounded_rect_path(
                    context,
                    0.5,
                    0.5,
                    f64::from(width) - 1.0,
                    f64::from(height) - 1.0,
                    7.0,
                );
                context.set_source_rgb(
                    f64::from(red) / 255.0,
                    f64::from(green) / 255.0,
                    f64::from(blue) / 255.0,
                );
                context.fill_preserve().ok();
                context.set_source_rgba(0.0, 0.0, 0.0, 0.28);
                context.set_line_width(1.0);
                context.stroke().ok();
            }
        });
        let label = Label::new(Some(&t("Color")));
        label.set_hexpand(true);
        label.set_xalign(0.0);
        let edit = Button::with_label(&t("Edit"));
        edit.add_css_class("recording-editor-bg-custom-edit");
        edit.set_has_frame(false);
        edit.set_valign(Align::Center);
        edit.set_tooltip_text(Some(&t("Edit text color")));
        row.append(&swatch);
        row.append(&label);
        row.append(&edit);

        let popover = Popover::new();
        popover.add_css_class("recording-editor-custom-popover");
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        popover.set_position(gtk4::PositionType::Left);
        popover.set_valign(Align::Start);
        popover.set_parent(&row);
        popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(-8, 0, 1, 40)));
        let body = GtkBox::new(Orientation::Vertical, 0);
        body.add_css_class("recording-editor-custom-body");
        let header = GtkBox::new(Orientation::Horizontal, 8);
        header.add_css_class("recording-editor-custom-header");
        let title = Label::new(Some(&t("Text color")));
        title.add_css_class("recording-editor-custom-title");
        title.set_hexpand(true);
        title.set_xalign(0.0);
        let close = Button::new();
        close.add_css_class("recording-editor-custom-close");
        close.set_has_frame(false);
        close.set_tooltip_text(Some(&t("Close")));
        let close_icon = Image::from_icon_name("window-close-symbolic");
        close_icon.set_pixel_size(13);
        close.set_child(Some(&close_icon));
        close.connect_clicked({
            let popover = popover.downgrade();
            move |_| {
                if let Some(popover) = popover.upgrade() {
                    popover.popdown();
                }
            }
        });
        header.append(&title);
        header.append(&close);
        body.append(&header);

        let changed: ColorChanged = Rc::new(RefCell::new(None));
        let edited = Rc::new(Cell::new(false));
        let repaint: Rc<RefCell<Option<std::rc::Weak<dyn Fn()>>>> = Rc::new(RefCell::new(None));
        let picker = build_color_picker(
            {
                let value = value.clone();
                Rc::new(move || value.get())
            },
            {
                let value = value.clone();
                let edited = edited.clone();
                Rc::new(move |rgb| edited.set(value.replace(rgb) != rgb))
            },
            {
                let value = value.clone();
                let edited = edited.clone();
                let changed = changed.clone();
                let swatch = swatch.downgrade();
                let repaint = repaint.clone();
                Rc::new(move || {
                    if edited.replace(false) {
                        if let Some(callback) = changed.borrow().as_ref() {
                            callback(value.get());
                        }
                    }
                    if let Some(swatch) = swatch.upgrade() {
                        swatch.queue_draw();
                    }
                    if let Some(repaint) =
                        repaint.borrow().as_ref().and_then(std::rc::Weak::upgrade)
                    {
                        repaint();
                    }
                })
            },
        );
        *repaint.borrow_mut() = Some(Rc::downgrade(&picker.repaint));
        body.append(&picker.widget);
        popover.set_child(Some(&body));
        popover.connect_show({
            let repaint = picker.repaint.clone();
            move |_| repaint()
        });
        edit.connect_clicked(move |_| popover.popup());
        Self {
            row,
            swatch,
            value,
            picker,
            changed,
        }
    }

    pub(super) fn widget(&self) -> GtkBox {
        self.row.clone()
    }

    pub(super) fn set_color(&self, color: [f64; 4]) {
        let rgb = super::widgets::motion_rgb888(super::widgets::motion_rgba(color));
        if self.value.replace(rgb) != rgb {
            self.swatch.queue_draw();
            (self.picker.repaint)();
        }
    }

    pub(super) fn connect_changed(&self, callback: impl Fn((u8, u8, u8)) + 'static) {
        *self.changed.borrow_mut() = Some(Rc::new(callback));
    }
}

#[cfg(test)]
mod tests {
    use super::font_matches_query;

    #[test]
    fn font_search_matches_case_insensitive_name_fragments() {
        assert!(font_matches_query("DejaVu Sans Mono", "MONO"));
        assert!(font_matches_query("DejaVu Sans Mono", "  dej mono  "));
        assert!(font_matches_query("École Sans", "éCOLE"));
        assert!(font_matches_query("Inter", ""));
        assert!(!font_matches_query("Inter", "mono"));
    }

    #[test]
    fn font_and_color_controls_reuse_the_existing_popover_components() {
        let source = include_str!("typography.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        assert!(production.contains("popover.add_css_class(\"editor-popover\")"));
        assert!(production.contains("editor-popover-list-item"));
        assert!(production.contains("SearchEntry::new()"));
        assert!(production.contains("connect_search_changed"));
        assert!(production.contains("No fonts found"));
        assert!(production.contains("build_color_picker("));
        assert!(production.contains("recording-editor-bg-custom-row"));
        assert!(production.contains("recording-editor-bg-custom-edit"));
        assert!(production.contains("recording-editor-custom-popover"));
        assert!(!production.contains("ColorDialog"));
        assert!(!production.contains("DropDown::"));
    }
}
