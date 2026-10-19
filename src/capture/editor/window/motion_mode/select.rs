use gtk4::{prelude::*, Box as GtkBox, Button, Image, Label, MenuButton, Orientation, Popover};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type ChangeHandlers = Rc<RefCell<Vec<Box<dyn Fn(usize)>>>>;

#[derive(Clone)]
pub(super) struct MotionSelect {
    button: MenuButton,
    selected: Rc<Cell<usize>>,
    options: Vec<Button>,
    apply: Rc<dyn Fn(usize)>,
    changed: ChangeHandlers,
}

impl MotionSelect {
    pub(super) fn new(labels: &[&str], selected_index: usize) -> Self {
        let button = MenuButton::new();
        button.set_has_frame(false);
        button.add_css_class("editor-motion-select");
        if let Some(trigger) = button.first_child() {
            trigger.add_css_class("recording-editor-dropdown");
        }
        let row = GtkBox::new(Orientation::Horizontal, 8);
        let label = Label::new(Some(labels[selected_index]));
        label.add_css_class("recording-editor-dropdown-label");
        label.set_xalign(0.0);
        label.set_hexpand(true);
        let arrow = Image::from_icon_name("pan-down-symbolic");
        arrow.add_css_class("recording-editor-dropdown-arrow");
        arrow.set_pixel_size(10);
        row.append(&label);
        row.append(&arrow);
        button.set_child(Some(&row));

        let popover = Popover::new();
        popover.set_has_arrow(false);
        popover.add_css_class("recording-editor-dropdown-popover");
        let list = GtkBox::new(Orientation::Vertical, 0);
        list.add_css_class("recording-editor-dropdown-list");
        let mut options = Vec::new();
        let mut checks = Vec::new();
        for text in labels {
            let option = Button::new();
            option.set_has_frame(false);
            option.add_css_class("recording-editor-dropdown-item");
            let row = GtkBox::new(Orientation::Horizontal, 8);
            let check = Image::from_icon_name("object-select-symbolic");
            check.set_pixel_size(12);
            check.set_opacity(0.0);
            let text = Label::new(Some(text));
            text.set_xalign(0.0);
            row.append(&check);
            row.append(&text);
            option.set_child(Some(&row));
            list.append(&option);
            checks.push(check);
            options.push(option);
        }
        popover.set_child(Some(&list));
        button.set_popover(Some(&popover));
        popover.connect_show({
            let button = button.downgrade();
            move |popover| {
                if let Some(button) = button.upgrade() {
                    popover.set_size_request(button.width(), -1);
                }
            }
        });

        let selected = Rc::new(Cell::new(selected_index));
        let changed: ChangeHandlers = Rc::new(RefCell::new(Vec::new()));
        let apply: Rc<dyn Fn(usize)> = Rc::new({
            let selected = selected.clone();
            let labels = labels
                .iter()
                .map(|label| label.to_string())
                .collect::<Vec<_>>();
            move |index| {
                let Some(text) = labels.get(index) else {
                    return;
                };
                selected.set(index);
                label.set_text(text);
                for (position, check) in checks.iter().enumerate() {
                    check.set_opacity(if position == index { 1.0 } else { 0.0 });
                }
            }
        });
        apply(selected_index);
        for (index, option) in options.iter().enumerate() {
            let apply = apply.clone();
            let changed = changed.clone();
            let selected = selected.clone();
            let popover = popover.downgrade();
            option.connect_clicked(move |option| {
                if !option.is_sensitive() {
                    return;
                }
                if selected.get() != index {
                    apply(index);
                    for callback in changed.borrow().iter() {
                        callback(index);
                    }
                }
                if let Some(popover) = popover.upgrade() {
                    popover.popdown();
                }
            });
        }
        Self {
            button,
            selected,
            options,
            apply,
            changed,
        }
    }

    pub(super) fn widget(&self) -> &MenuButton {
        &self.button
    }

    pub(super) fn selected(&self) -> usize {
        self.selected.get()
    }

    pub(super) fn set_selected(&self, index: usize) {
        (self.apply)(index);
    }

    pub(super) fn set_option_sensitive(&self, index: usize, sensitive: bool) {
        if let Some(option) = self.options.get(index) {
            option.set_sensitive(sensitive);
            option.set_opacity(if sensitive { 1.0 } else { 0.4 });
        }
    }

    pub(super) fn connect_changed(&self, callback: impl Fn(usize) + 'static) {
        self.changed.borrow_mut().push(Box::new(callback));
    }
}
