//! Settings dropdowns, matching the History window action popover.
//!
//! Native `ComboBoxText` still looks like a GTK combobox. History menus use a
//! frameless button list in a rounded popover — this widget is the same
//! control, wired as a labeled picker.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation, Popover};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::capture::editor::window::icon_names;

#[derive(Clone)]
pub struct SettingsSelect {
    button: Button,
    ids: Rc<Vec<String>>,
    selected: Rc<Cell<usize>>,
    apply: Rc<dyn Fn(usize, bool)>,
    on_changed: Rc<RefCell<Vec<Rc<dyn Fn()>>>>,
}

impl SettingsSelect {
    pub fn new<I, Id, L>(items: I, current: &str) -> Self
    where
        I: IntoIterator<Item = (Id, L)>,
        Id: AsRef<str>,
        L: AsRef<str>,
    {
        let mut ids = Vec::new();
        let mut labels = Vec::new();
        for (id, label) in items {
            ids.push(id.as_ref().to_string());
            labels.push(label.as_ref().to_string());
        }
        debug_assert!(
            !ids.is_empty(),
            "SettingsSelect requires at least one option"
        );

        let selected_index = ids.iter().position(|id| id == current).unwrap_or(0);
        let ids = Rc::new(ids);
        let labels = Rc::new(labels);
        let selected = Rc::new(Cell::new(selected_index));
        let on_changed: Rc<RefCell<Vec<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(Vec::new()));

        let button = Button::new();
        button.add_css_class("settings-select");
        button.set_valign(Align::Center);
        button.set_vexpand(false);
        button.set_hexpand(false);
        button.set_has_frame(false);

        let row = GtkBox::new(Orientation::Horizontal, 8);
        let label = Label::new(Some(&labels[selected_index]));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        let arrow = Image::from_icon_name(icon_names::CHEVRON_DOWN_REGULAR);
        arrow.add_css_class("settings-select-arrow");
        arrow.set_pixel_size(12);
        row.append(&label);
        row.append(&arrow);
        button.set_child(Some(&row));

        let popover = Popover::new();
        popover.add_css_class("history-action-popover");
        popover.add_css_class("settings-select-popover");
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        popover.set_position(gtk4::PositionType::Bottom);
        popover.set_parent(&button);

        let menu = GtkBox::new(Orientation::Vertical, 2);
        let mut option_buttons = Vec::with_capacity(labels.len());
        for (index, option_label) in labels.iter().enumerate() {
            let option = Button::new();
            option.add_css_class("history-action-btn");
            option.set_focus_on_click(false);
            if index == selected_index {
                option.add_css_class("settings-select-option-active");
            }
            let option_text = Label::new(Some(option_label));
            option_text.set_halign(Align::Start);
            option_text.set_xalign(0.0);
            option.set_child(Some(&option_text));
            menu.append(&option);
            option_buttons.push(option);
        }
        popover.set_child(Some(&menu));
        let option_buttons = Rc::new(option_buttons);

        let apply = {
            let label = label.clone();
            let labels = Rc::clone(&labels);
            let option_buttons = Rc::clone(&option_buttons);
            let selected = Rc::clone(&selected);
            let on_changed = Rc::clone(&on_changed);
            Rc::new(move |index: usize, notify: bool| {
                selected.set(index);
                if let Some(text) = labels.get(index) {
                    label.set_text(text);
                }
                for (i, option) in option_buttons.iter().enumerate() {
                    if i == index {
                        option.add_css_class("settings-select-option-active");
                    } else {
                        option.remove_css_class("settings-select-option-active");
                    }
                }
                if notify {
                    for callback in on_changed.borrow().iter() {
                        callback();
                    }
                }
            }) as Rc<dyn Fn(usize, bool)>
        };

        for (index, option) in option_buttons.iter().enumerate() {
            let apply = Rc::clone(&apply);
            let popover = popover.clone();
            option.connect_clicked(move |_| {
                apply(index, true);
                popover.popdown();
            });
        }

        {
            let popover = popover.clone();
            let button_for_width = button.clone();
            button.connect_clicked(move |_| {
                // The menu is an extension of its trigger, so keep their
                // outside widths aligned instead of letting the shared
                // History popover surface choose a wider minimum.
                let width = button_for_width.width();
                popover.set_size_request(width, -1);
                popover.popup();
            });
        }

        Self {
            button,
            ids,
            selected,
            apply,
            on_changed,
        }
    }

    pub fn widget(&self) -> &Button {
        &self.button
    }

    pub fn active_id(&self) -> Option<String> {
        self.ids.get(self.selected.get()).cloned()
    }

    pub fn set_active_id(&self, id: &str) -> bool {
        let Some(index) = self.ids.iter().position(|item| item == id) else {
            return false;
        };
        (self.apply)(index, false);
        true
    }

    pub fn set_sensitive(&self, sensitive: bool) {
        self.button.set_sensitive(sensitive);
    }

    pub fn connect_changed<F: Fn() + 'static>(&self, callback: F) {
        self.on_changed.borrow_mut().push(Rc::new(callback));
    }
}

pub fn language_combo(current: &str) -> SettingsSelect {
    let items = crate::i18n::UI_LANGUAGES.iter().map(|lang| {
        let label = if lang.code == crate::i18n::SYSTEM_LANGUAGE {
            crate::i18n::t("System default")
        } else {
            lang.native_name.to_string()
        };
        (lang.code, label)
    });
    SettingsSelect::new(items, &crate::i18n::sanitize_ui_language(current))
}

#[cfg(test)]
mod tests {
    #[test]
    fn settings_select_reuses_history_popover_classes() {
        let source = include_str!("select.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("history-action-popover"),
            "settings dropdown popover no longer reuses the History menu surface"
        );
        assert!(
            production_source.contains("history-action-btn"),
            "settings dropdown options no longer reuse History menu rows"
        );
        assert!(
            production_source.contains("set_valign(Align::Center)"),
            "settings dropdown still stretches with the row"
        );
        assert!(
            production_source.contains("button_for_width.width()"),
            "settings dropdown menu should match its trigger width"
        );
    }
}
