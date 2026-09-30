//! Light / Dark / System theme picker for Settings → General.
//!
//! A short row of checkbox options that read like the native controls around
//! them (the shared `checkbutton check` tokens in 04-native-widgets.css).
//! A theme is single-select, but grouping would swap GTK's indicator to the
//! round `radio` node, so exclusion is enforced here instead.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, CheckButton, Orientation};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::config::DEFAULT_UI_THEME;
use crate::i18n::t;

/// Stored ids in display order. Captions come from [`theme_caption`] so the
/// strings stay literal for the catalog coverage checker.
const THEME_IDS: [&str; 3] = ["system", "light", "dark"];

fn theme_caption(theme_id: &str) -> String {
    match theme_id {
        "light" => t("Light"),
        "dark" => t("Dark"),
        _ => t("System"),
    }
}

#[derive(Clone)]
pub struct ThemePicker {
    root: GtkBox,
    ids: Rc<Vec<String>>,
    selected: Rc<Cell<usize>>,
    on_changed: Rc<RefCell<Vec<Rc<dyn Fn()>>>>,
}

impl ThemePicker {
    pub fn new(current: &str) -> Self {
        let ids: Vec<String> = THEME_IDS.iter().map(|id| (*id).to_string()).collect();
        let selected_index = ids
            .iter()
            .position(|id| id == current)
            .or_else(|| ids.iter().position(|id| id == DEFAULT_UI_THEME))
            .unwrap_or(0);
        let ids = Rc::new(ids);
        let selected = Rc::new(Cell::new(selected_index));
        let on_changed: Rc<RefCell<Vec<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(Vec::new()));

        let root = GtkBox::new(Orientation::Horizontal, 16);
        root.add_css_class("settings-theme-picker");
        root.set_halign(Align::End);
        root.set_valign(Align::Center);

        let mut options = Vec::with_capacity(THEME_IDS.len());
        for id in THEME_IDS.iter() {
            let option = CheckButton::with_label(&theme_caption(id));
            option.set_focus_on_click(false);
            root.append(&option);
            options.push(option);
        }
        options[selected_index].set_active(true);
        let options = Rc::new(options);

        // Keep the checkboxes mutually exclusive by hand: only one theme can be
        // active, and the active one can never be cleared.
        let updating = Rc::new(Cell::new(false));
        for (index, option) in options.iter().enumerate() {
            let options = Rc::clone(&options);
            let selected = Rc::clone(&selected);
            let on_changed = Rc::clone(&on_changed);
            let updating = Rc::clone(&updating);
            option.connect_toggled(move |button| {
                if updating.get() {
                    return;
                }
                if !button.is_active() {
                    updating.set(true);
                    button.set_active(true);
                    updating.set(false);
                    return;
                }
                updating.set(true);
                for (other_index, other) in options.iter().enumerate() {
                    if other_index != index {
                        other.set_active(false);
                    }
                }
                updating.set(false);
                if selected.get() == index {
                    return;
                }
                selected.set(index);
                for callback in on_changed.borrow().iter() {
                    callback();
                }
            });
        }

        Self {
            root,
            ids,
            selected,
            on_changed,
        }
    }

    pub fn widget(&self) -> &GtkBox {
        &self.root
    }

    pub fn active_id(&self) -> Option<String> {
        self.ids.get(self.selected.get()).cloned()
    }

    pub fn connect_changed<F: Fn() + 'static>(&self, callback: F) {
        self.on_changed.borrow_mut().push(Rc::new(callback));
    }
}
