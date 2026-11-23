/// Right-click menu for a clip on the video or Zoom track.
///
/// This replaces the sidebar's footer Delete button: a clip is acted on where
/// it sits instead of being selected and then deleted from across the window.
/// Every item works on one clip index on one track, so the track kind is data
/// (`ClipMenuTarget`) rather than a second menu implementation.
#[derive(Clone, Copy)]
pub enum ClipMenuTarget {
    Video(usize),
    Zoom(usize),
}

impl ClipMenuTarget {
    fn index(self) -> usize {
        match self {
            ClipMenuTarget::Video(index) | ClipMenuTarget::Zoom(index) => index,
        }
    }
}

/// One row in the menu. Every row, Delete included, is drawn the same way.
struct MenuItem {
    icon: &'static str,
    label: String,
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

    let card = GtkBox::new(Orientation::Vertical, 2);
    card.add_css_class("recording-editor-clip-menu");
    card.set_size_request(160, -1);

    let index = target.index();
    let is_video = matches!(target, ClipMenuTarget::Video(_));

    // The hide item names what the click will do, so a clip that is already
    // disabled offers "Show" and the eye matches. Paste is offered only when
    // the model says a paste would actually land.
    let (hidden_now, can_paste) = {
        let guard = state.lock().unwrap();
        let hidden = guard.zoom_clips.get(index).is_some_and(|clip| clip.hidden);
        (hidden, guard.can_paste_clipboard_at_playhead())
    };

    let (muted_now, can_mute_video) = {
        let guard = state.lock().unwrap();
        (
            guard.segment_is_muted(index),
            !guard.video_locked && guard.has_audio_track() && !guard.audio_locked,
        )
    };

    let mut items: Vec<MenuItem> = if is_video {
        let mut items = Vec::new();
        if can_mute_video {
            items.push(MenuItem {
                icon: if muted_now {
                    "audio-volume-high-symbolic"
                } else {
                    "audio-volume-muted-symbolic"
                },
                label: if muted_now { t("Unmute") } else { t("Mute") },
                action: {
                    let state = state.clone();
                    let on_change = on_change.clone();
                    Rc::new(move || {
                        {
                            let mut guard = state.lock().unwrap();
                            if !guard.video_locked {
                                select_video(&mut guard, Some(index));
                                guard.set_selected_clip_muted(!muted_now);
                            }
                        }
                        on_change();
                    })
                },
            });
        }
        items.push(MenuItem {
            icon: icon_names::custom::USER_TRASH_SYMBOLIC,
            label: t("Delete"),
            action: {
                let state = state.clone();
                let on_change = on_change.clone();
                Rc::new(move || {
                    {
                        let mut guard = state.lock().unwrap();
                        select_video(&mut guard, Some(index));
                        guard.remove_selected_clip();
                    }
                    on_change();
                })
            },
        });
        items
    } else {
        vec![
            MenuItem {
                icon: icon_names::shipped::COPY_ARROW_RIGHT_REGULAR,
                label: t("Duplicate"),
                action: {
                    let state = state.clone();
                    let on_change = on_change.clone();
                    Rc::new(move || {
                        {
                            let mut guard = state.lock().unwrap();
                            let start = duplicate_start(&guard, index);
                            let _ = guard.duplicate_zoom_clip(index, start);
                        }
                        on_change();
                    })
                },
            },
            MenuItem {
                icon: icon_names::shipped::COPY_REGULAR,
                label: t("Copy"),
                action: {
                    let state = state.clone();
                    let on_change = on_change.clone();
                    Rc::new(move || {
                        {
                            let mut guard = state.lock().unwrap();
                            guard.copy_zoom_clip(index);
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
                action: {
                    let state = state.clone();
                    let on_change = on_change.clone();
                    Rc::new(move || {
                        {
                            let mut guard = state.lock().unwrap();
                            guard.cut_zoom_clip(index);
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
                action: {
                    let state = state.clone();
                    let on_change = on_change.clone();
                    Rc::new(move || {
                        {
                            let mut guard = state.lock().unwrap();
                            guard.set_zoom_hidden(index, !hidden_now);
                        }
                        on_change();
                    })
                },
            },
            MenuItem {
                icon: icon_names::custom::USER_TRASH_SYMBOLIC,
                label: t("Delete"),
                action: {
                    let state = state.clone();
                    let on_change = on_change.clone();
                    Rc::new(move || {
                        {
                            let mut guard = state.lock().unwrap();
                            guard.remove_zoom_clip(index);
                        }
                        on_change();
                    })
                },
            },
        ]
    };

    // Paste is offered only when a paste would actually land: something is on
    // the clipboard and the playhead is clear of the clip it would insert. It
    // is inserted before Delete so the destructive row stays last.
    if can_paste && !is_video {
        let state = state.clone();
        let on_change = on_change.clone();
        items.insert(
            items.len() - 1,
            MenuItem {
                icon: icon_names::shipped::CLIPBOARD_PASTE_REGULAR,
                label: t("Paste"),
                action: Rc::new(move || {
                    state.lock().unwrap().paste_clipboard_at_playhead();
                    on_change();
                }),
            },
        );
    }

    for item in items {
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
fn duplicate_start(state: &VideoEditState, index: usize) -> f64 {
    state
        .zoom_clips
        .get(index)
        .map(|clip| clip.end)
        .unwrap_or(0.0)
}

fn menu_row(item: MenuItem, popover: &Popover) -> Button {
    let button = Button::new();
    button.add_css_class("recording-editor-clip-menu-row");
    button.set_has_frame(false);
    button.set_focus_on_click(false);
    button.set_size_request(-1, 32);

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

#[cfg(test)]
mod tests {
    #[test]
    fn a_video_clip_is_offered_its_own_edits_only() {
        let menu = include_str!("clip_menu.rs");
        let source = menu
            .split("#[cfg(test)]")
            .next()
            .expect("the menu's production source");
        let start = source
            .find("let mut items: Vec<MenuItem> = if is_video {")
            .expect("the video branch of the item list");
        let video = {
            let rest = &source[start + 1..];
            let end = rest
                .find("\n    } else {")
                .expect("the overlay branch follows the video branch");
            &source[start..start + 1 + end]
        };
        assert!(
            video.contains("select_video(&mut guard, Some(index));"),
            "a video action must target the clicked segment, not whatever was selected"
        );
        assert!(
            video.contains("guard.remove_selected_clip();"),
            "the video menu must delete the clicked clip"
        );
        assert!(
            video.contains("guard.set_selected_clip_muted(!muted_now);"),
            "the video menu must mute and unmute the clicked clip"
        );
        assert!(
            video.contains("if !guard.video_locked {"),
            "the model setter leaves the video lock to the caller, so the menu rechecks it"
        );
        assert!(
            !video.contains("paste_clipboard_at_playhead"),
            "the overlay clipboard paste must not be offered on a video clip"
        );
        assert!(
            !video.contains("label: if hidden_now"),
            "a video clip has no hide flag"
        );
        assert!(
            source.contains("if can_paste && !is_video {"),
            "paste must stay out of the video menu while the overlays keep it"
        );
    }

    #[test]
    fn the_menu_rows_share_one_treatment() {
        let menu = include_str!("clip_menu.rs");
        let source = menu
            .split("#[cfg(test)]")
            .next()
            .expect("the menu's production source");
        assert!(
            !source.contains("menu_separator")
                && !source.contains("recording-editor-clip-menu-separator"),
            "the menu has no separators: every row is drawn the same way"
        );
        assert!(
            !source.contains("recording-editor-clip-menu-row-danger"),
            "the destructive row keeps the same ink as every other row"
        );
        assert!(
            source.contains("card.append(&menu_row(item, &popover));"),
            "every item becomes one row, in order"
        );
    }

    #[test]
    fn clip_menu_rows_match_the_shared_menu_metrics() {
        use super::*;
        let Some(()) = crate::test_support::with_gtk(|| {
            let popover = Popover::new();
            let row = menu_row(
                MenuItem {
                    icon: icon_names::shipped::COPY_REGULAR,
                    label: "Copy".to_string(),
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
    fn the_menu_card_matches_the_shared_menu_metrics() {
        let menu = include_str!("clip_menu.rs");
        let source = menu
            .split("#[cfg(test)]")
            .next()
            .expect("the menu's production source");
        assert!(
            source.contains("GtkBox::new(Orientation::Vertical, 2)")
                && source.contains("card.set_size_request(160, -1);"),
            "the clip menu card keeps the shared menu's 160px width and 2px row spacing"
        );
    }
}
