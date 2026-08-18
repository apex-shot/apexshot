use crate::recording::editor::model::{sanitize_title, VideoEditState};
use gtk4::gdk;
use gtk4::{
    glib, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, Entry, EventControllerFocus,
    EventControllerKey, Image, Label, Orientation,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[allow(dead_code)]
pub(super) fn build_titlebar(
    window: &ApplicationWindow,
    state: Option<Arc<Mutex<VideoEditState>>>,
) -> GtkBox {
    let bar = GtkBox::new(Orientation::Horizontal, 8);
    bar.add_css_class("recording-editor-window-controls");
    bar.set_hexpand(true);
    bar.set_vexpand(false);
    bar.set_valign(Align::Start);

    let name = state
        .as_ref()
        .map(|state| state.lock().unwrap().title.clone())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "Untitled".to_string());
    let current_name = Rc::new(RefCell::new(name.clone()));
    let editing = Rc::new(Cell::new(false));
    let can_rename = state.is_some();

    let title_row = GtkBox::new(Orientation::Horizontal, 4);
    title_row.add_css_class("recording-editor-title-row");
    title_row.set_hexpand(false);
    title_row.set_halign(Align::Start);
    title_row.set_valign(Align::Center);

    let title = Label::new(Some(&name));
    title.add_css_class("recording-editor-title");
    title.set_xalign(0.0);
    title.set_hexpand(false);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(42);
    title.set_halign(Align::Start);
    title.set_valign(Align::Center);
    title.set_can_target(false);
    title_row.append(&title);

    let edit = Button::new();
    edit.set_has_frame(false);
    edit.add_css_class("recording-editor-title-edit");
    edit.set_tooltip_text(Some("Rename video"));
    edit.set_sensitive(can_rename);
    edit.set_halign(Align::Center);
    edit.set_valign(Align::Center);
    let edit_icon = Image::from_icon_name("document-edit-symbolic");
    edit_icon.set_pixel_size(12);
    edit.set_child(Some(&edit_icon));
    title_row.append(&edit);

    let entry = Entry::new();
    entry.add_css_class("recording-editor-title-entry");
    entry.set_text(&name);
    entry.set_hexpand(false);
    entry.set_width_chars(18);
    entry.set_max_width_chars(42);
    entry.set_valign(Align::Center);
    entry.set_visible(false);

    bar.append(&title_row);
    bar.append(&entry);

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    bar.append(&spacer);

    let lights = build_traffic_lights(window);
    lights.set_halign(Align::End);
    lights.set_valign(Align::Center);
    bar.append(&lights);

    let begin_edit = {
        let title_row = title_row.clone();
        let entry = entry.clone();
        let current_name = current_name.clone();
        let editing = editing.clone();
        Rc::new(move || {
            if editing.get() {
                return;
            }
            editing.set(true);
            entry.set_text(&current_name.borrow());
            title_row.set_visible(false);
            entry.set_visible(true);
            entry.grab_focus();
            entry.select_region(0, -1);
        })
    };

    let end_edit = {
        let title = title.clone();
        let title_row = title_row.clone();
        let entry = entry.clone();
        let current_name = current_name.clone();
        let editing = editing.clone();
        let state = state.clone();
        Rc::new(move |commit: bool| {
            if !editing.get() {
                return;
            }
            if commit {
                let next = sanitize_title(&entry.text());
                if let Some(state) = &state {
                    state.lock().unwrap().set_title(&next);
                    let saved = state.lock().unwrap().title.clone();
                    *current_name.borrow_mut() = saved.clone();
                    title.set_text(&saved);
                    title.set_tooltip_text(Some(&saved));
                } else {
                    *current_name.borrow_mut() = next.clone();
                    title.set_text(&next);
                    title.set_tooltip_text(Some(&next));
                }
            } else {
                entry.set_text(&current_name.borrow());
            }
            editing.set(false);
            entry.set_visible(false);
            title_row.set_visible(true);
        })
    };

    edit.connect_clicked({
        let begin_edit = begin_edit.clone();
        move |_| begin_edit()
    });

    entry.connect_activate({
        let end_edit = end_edit.clone();
        move |_| end_edit(true)
    });

    let focus = EventControllerFocus::new();
    focus.connect_leave({
        let end_edit = end_edit.clone();
        move |_| end_edit(true)
    });
    entry.add_controller(focus);

    let keys = EventControllerKey::new();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            end_edit(false);
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    entry.add_controller(keys);

    title.set_tooltip_text(Some(&name));
    crate::capture::editor::ui_support::install_window_drag(&bar, window);
    bar
}

pub(super) fn build_traffic_lights(window: &ApplicationWindow) -> GtkBox {
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
    right_box.add_css_class("recording-editor-traffic-lights");
    right_box.set_halign(Align::End);
    right_box.append(&minimize);
    right_box.append(&zoom);
    right_box.append(&close);

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

    right_box
}
