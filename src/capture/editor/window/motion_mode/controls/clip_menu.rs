use gtk4::{
    gdk, prelude::*, Align, Box as GtkBox, Button, Image, Label, Orientation, Popover,
    PositionType, Widget,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::capture::editor::window::icon_names;
use crate::i18n::t;

use super::super::MotionRuntime;
use super::Redraw;

#[derive(Clone, Copy)]
pub(super) enum ClipMenuTarget {
    Motion(usize),
    Text(usize),
}

struct MenuItem {
    icon: &'static str,
    label: String,
    danger: bool,
    sensitive: bool,
    action: Rc<dyn Fn()>,
}

pub(super) fn show_clip_menu(
    anchor: &impl IsA<Widget>,
    target: ClipMenuTarget,
    x: f64,
    y: f64,
    runtime: Rc<RefCell<MotionRuntime>>,
    redraw: Redraw,
) -> Popover {
    let popover = Popover::new();
    popover.add_css_class("recording-editor-clip-menu-popover");
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.set_position(PositionType::Top);

    let card = GtkBox::new(Orientation::Vertical, 2);
    card.add_css_class("recording-editor-clip-menu");
    card.set_size_request(160, -1);
    let (index, is_text, disabled, can_duplicate) = {
        let runtime = runtime.borrow();
        match target {
            ClipMenuTarget::Motion(index) => {
                let Some(segment) = runtime.motion.segments.get(index) else {
                    return popover;
                };
                (
                    index,
                    false,
                    segment.is_disabled,
                    runtime.motion.motion_duplicate_span(index).is_some(),
                )
            }
            ClipMenuTarget::Text(index) => {
                let Some(segment) = runtime.motion.text_segments.get(index) else {
                    return popover;
                };
                (
                    index,
                    true,
                    segment.is_disabled,
                    runtime.motion.text_duplicate_span(index).is_some(),
                )
            }
        }
    };

    popover.set_parent(anchor);
    let items = [
        MenuItem {
            icon: icon_names::shipped::COPY_ARROW_RIGHT_REGULAR,
            label: t("Duplicate"),
            danger: false,
            sensitive: can_duplicate,
            action: {
                let runtime = runtime.clone();
                let redraw = redraw.clone();
                Rc::new(move || {
                    let mut runtime = runtime.borrow_mut();
                    runtime.begin_motion_command();
                    if is_text {
                        runtime.motion.selected_text = Some(index);
                        runtime.motion.selected = None;
                        runtime.motion.duplicate_text_segment(index);
                    } else {
                        runtime.motion.selected = Some(index);
                        runtime.motion.selected_text = None;
                        runtime.motion.duplicate_motion_segment(index);
                    }
                    drop(runtime);
                    redraw();
                })
            },
        },
        MenuItem {
            icon: if disabled {
                icon_names::shipped::EYE_REGULAR
            } else {
                icon_names::shipped::EYE_OFF_REGULAR
            },
            label: if disabled { t("Show") } else { t("Hide") },
            danger: false,
            sensitive: true,
            action: {
                let runtime = runtime.clone();
                let redraw = redraw.clone();
                Rc::new(move || {
                    let mut runtime = runtime.borrow_mut();
                    runtime.begin_motion_command();
                    if is_text {
                        runtime.motion.selected_text = Some(index);
                        runtime.motion.selected = None;
                        runtime.motion.set_selected_text_disabled(!disabled);
                    } else {
                        runtime.motion.selected = Some(index);
                        runtime.motion.selected_text = None;
                        runtime.motion.set_selected_disabled(!disabled);
                    }
                    drop(runtime);
                    redraw();
                })
            },
        },
        MenuItem {
            icon: icon_names::custom::USER_TRASH_SYMBOLIC,
            label: t("Delete"),
            danger: true,
            sensitive: true,
            action: {
                let runtime = runtime.clone();
                let redraw = redraw.clone();
                Rc::new(move || {
                    let mut runtime = runtime.borrow_mut();
                    runtime.begin_motion_command();
                    if is_text {
                        runtime.motion.selected_text = Some(index);
                        runtime.motion.selected = None;
                    } else {
                        runtime.motion.selected = Some(index);
                        runtime.motion.selected_text = None;
                    }
                    runtime.motion.remove_selected();
                    drop(runtime);
                    redraw();
                })
            },
        },
    ];

    for (position, item) in items.into_iter().enumerate() {
        if item.danger && position > 0 {
            card.append(&menu_separator());
        }
        card.append(&menu_row(item, &popover));
    }

    popover.set_child(Some(&card));
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
    popover
}

fn menu_separator() -> GtkBox {
    let separator = GtkBox::new(Orientation::Vertical, 0);
    separator.add_css_class("recording-editor-clip-menu-separator");
    separator.set_size_request(-1, 1);
    separator
}

fn menu_row(item: MenuItem, popover: &Popover) -> Button {
    let button = Button::new();
    button.add_css_class("recording-editor-clip-menu-row");
    if item.danger {
        button.add_css_class("recording-editor-clip-menu-row-danger");
    }
    button.set_has_frame(false);
    button.set_focus_on_click(false);
    button.set_size_request(-1, 32);
    button.set_sensitive(item.sensitive);

    let row = GtkBox::new(Orientation::Horizontal, 10);
    row.add_css_class("recording-editor-clip-menu-row-inner");
    row.set_valign(Align::Center);
    let icon = Image::from_icon_name(item.icon);
    icon.set_pixel_size(15);
    let label = Label::new(Some(&item.label));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&icon);
    row.append(&label);
    button.set_child(Some(&row));

    let popover = popover.clone();
    button.connect_clicked(move |_| {
        popover.popdown();
        (item.action)();
    });
    button
}

#[cfg(test)]
mod tests {
    use super::{menu_row, MenuItem};
    use gtk4::{prelude::*, Box as GtkBox, Image, Label, Popover};
    use std::rc::Rc;

    fn production_source() -> &'static str {
        let source = include_str!("clip_menu.rs");
        source.split("#[cfg(test)]").next().unwrap_or(source)
    }

    #[test]
    fn clip_menu_rows_match_the_shared_menu_metrics() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let popover = Popover::new();
            let row = menu_row(
                MenuItem {
                    icon: crate::capture::editor::window::icon_names::shipped::COPY_REGULAR,
                    label: "Duplicate".to_string(),
                    danger: false,
                    sensitive: true,
                    action: Rc::new(|| {}),
                },
                &popover,
            );
            assert_eq!(row.size_request(), (-1, 32));
            assert!(!row.has_frame());
            assert!(!row.gets_focus_on_click());
            let inner = row
                .child()
                .and_then(|child| child.downcast::<GtkBox>().ok())
                .expect("the row content is a box");
            assert_eq!(inner.spacing(), 10);
            let icon = inner
                .first_child()
                .and_then(|icon| icon.downcast::<Image>().ok())
                .expect("the row leads with the item icon");
            assert_eq!(icon.pixel_size(), 15);
            let label = icon
                .next_sibling()
                .and_then(|label| label.downcast::<Label>().ok())
                .expect("the label follows the icon");
            assert!(label.hexpands());
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn a_disabled_menu_item_stays_insensitive() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let popover = Popover::new();
            let row = menu_row(
                MenuItem {
                    icon: crate::capture::editor::window::icon_names::shipped::COPY_ARROW_RIGHT_REGULAR,
                    label: "Duplicate".to_string(),
                    danger: false,
                    sensitive: false,
                    action: Rc::new(|| {}),
                },
                &popover,
            );
            assert!(!row.is_sensitive());
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn the_menu_card_matches_the_shared_menu_metrics() {
        let source = production_source();
        assert!(
            source.contains("GtkBox::new(Orientation::Vertical, 2)")
                && source.contains("card.set_size_request(160, -1);"),
            "the clip menu card keeps the shared menu's 160px width and 2px row spacing"
        );
    }
}
