/// Right-click menu for a clip on the Zoom or Hide track.
///
/// This replaces the sidebar's footer Delete button: a clip is acted on where
/// it sits instead of being selected and then deleted from across the window.
/// Every item works on one clip index on one track, so the track kind is data
/// (`ClipMenuTarget`) rather than a second menu implementation.
#[derive(Clone, Copy)]
pub enum ClipMenuTarget {
    Zoom(usize),
    Hide(usize),
}

impl ClipMenuTarget {
    fn index(self) -> usize {
        match self {
            ClipMenuTarget::Zoom(index) | ClipMenuTarget::Hide(index) => index,
        }
    }

    fn is_zoom(self) -> bool {
        matches!(self, ClipMenuTarget::Zoom(_))
    }
}

/// One row in the menu. `danger` paints the row red, which is how Delete reads.
struct MenuItem {
    icon: &'static str,
    label: String,
    danger: bool,
    action: Rc<dyn Fn()>,
}

/// Build and pop up the clip menu at `(x, y)` in `anchor`'s coordinate space.
pub fn show_clip_menu(
    anchor: &impl IsA<Widget>,
    target: ClipMenuTarget,
    x: f64,
    y: f64,
    state: Arc<Mutex<VideoEditState>>,
    on_change: Rc<dyn Fn()>,
) -> Popover {
    let popover = Popover::new();
    popover.add_css_class("recording-editor-clip-menu-popover");
    popover.set_parent(anchor);
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    // The menu opens upward: the tracks sit at the bottom of the editor, so a
    // downward menu would land off the window edge.
    popover.set_position(PositionType::Top);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("recording-editor-clip-menu");

    let index = target.index();
    let is_zoom = target.is_zoom();

    // The hide item names what the click will do, so a clip that is already
    // disabled offers "Show" and the eye matches. Paste is offered only when
    // the model says a paste would actually land.
    let (hidden_now, can_paste) = {
        let guard = state.lock().unwrap();
        let hidden = if is_zoom {
            guard.zoom_clips.get(index).is_some_and(|clip| clip.hidden)
        } else {
            guard
                .cursor_hide_clips
                .get(index)
                .is_some_and(|clip| clip.hidden)
        };
        (hidden, guard.can_paste_clipboard_at_playhead())
    };

    let mut items: Vec<MenuItem> = vec![
        MenuItem {
            icon: icon_names::shipped::COPY_ARROW_RIGHT_REGULAR,
            label: t("Duplicate"),
            danger: false,
            action: {
                let state = state.clone();
                let on_change = on_change.clone();
                Rc::new(move || {
                    {
                        let mut guard = state.lock().unwrap();
                        let start = duplicate_start(&guard, index, is_zoom);
                        if is_zoom {
                            let _ = guard.duplicate_zoom_clip(index, start);
                        } else {
                            let _ = guard.duplicate_cursor_hide_clip(index, start);
                        }
                    }
                    on_change();
                })
            },
        },
        MenuItem {
            icon: icon_names::shipped::COPY_REGULAR,
            label: t("Copy"),
            danger: false,
            action: {
                let state = state.clone();
                let on_change = on_change.clone();
                Rc::new(move || {
                    {
                        let mut guard = state.lock().unwrap();
                        if is_zoom {
                            guard.copy_zoom_clip(index);
                        } else {
                            guard.copy_cursor_hide_clip(index);
                        }
                    }
                    // Copy changes nothing on the timeline, so without this the
                    // dim and the ghost would not appear until some later
                    // repaint — the copy would look like a no-op again.
                    on_change();
                })
            },
        },
        MenuItem {
            icon: icon_names::shipped::CUT_REGULAR,
            label: t("Cut"),
            danger: false,
            action: {
                let state = state.clone();
                let on_change = on_change.clone();
                Rc::new(move || {
                    {
                        let mut guard = state.lock().unwrap();
                        if is_zoom {
                            guard.cut_zoom_clip(index);
                        } else {
                            guard.cut_cursor_hide_clip(index);
                        }
                    }
                    on_change();
                })
            },
        },
        MenuItem {
            icon: if hidden_now {
                icon_names::shipped::EYE_REGULAR
            } else {
                icon_names::shipped::EYE_OFF_REGULAR
            },
            label: if hidden_now { t("Show") } else { t("Hide") },
            danger: false,
            action: {
                let state = state.clone();
                let on_change = on_change.clone();
                Rc::new(move || {
                    {
                        let mut guard = state.lock().unwrap();
                        if is_zoom {
                            guard.set_zoom_hidden(index, !hidden_now);
                        } else {
                            guard.set_cursor_hide_hidden(index, !hidden_now);
                        }
                    }
                    on_change();
                })
            },
        },
        MenuItem {
            icon: icon_names::custom::USER_TRASH_SYMBOLIC,
            label: t("Delete"),
            danger: true,
            action: {
                let state = state.clone();
                let on_change = on_change.clone();
                Rc::new(move || {
                    {
                        let mut guard = state.lock().unwrap();
                        if is_zoom {
                            guard.remove_zoom_clip(index);
                        } else {
                            guard.remove_cursor_hide_clip(index);
                        }
                    }
                    on_change();
                })
            },
        },
    ];

    // Paste is offered only when a paste would actually land: something is on
    // the clipboard and the playhead is clear of the clip it would insert. It
    // is inserted before Delete so the destructive row stays last.
    if can_paste {
        let state = state.clone();
        let on_change = on_change.clone();
        items.insert(
            items.len() - 1,
            MenuItem {
                icon: icon_names::shipped::CLIPBOARD_PASTE_REGULAR,
                label: t("Paste"),
                danger: false,
                action: Rc::new(move || {
                    state.lock().unwrap().paste_clipboard_at_playhead();
                    on_change();
                }),
            },
        );
    }

    for item in items {
        // The reference separates the destructive row from the rest.
        if item.danger {
            card.append(&menu_separator());
        }
        card.append(&menu_row(item, &popover));
    }

    popover.set_child(Some(&card));
    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    // `set_parent` hands the popover to the anchor for its lifetime, so it has
    // to be handed back when the menu closes. Without this, every right-click
    // leaves another popover parented to the track.
    popover.connect_closed(|popover| {
        popover.unparent();
    });
    popover.popup();
    popover
}

/// Where a duplicate of `index` should land: directly after the clip it copies,
/// so the pair read as one run.
fn duplicate_start(state: &VideoEditState, index: usize, is_zoom: bool) -> f64 {
    if is_zoom {
        state
            .zoom_clips
            .get(index)
            .map(|clip| clip.end)
            .unwrap_or(0.0)
    } else {
        state
            .cursor_hide_clips
            .get(index)
            .map(|clip| clip.end)
            .unwrap_or(0.0)
    }
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

    let row = GtkBox::new(Orientation::Horizontal, 10);
    row.add_css_class("recording-editor-clip-menu-row-inner");
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
        (item.action)();
        popover.popdown();
    });
    button
}
