use gtk4::{
    gdk, glib, prelude::*, ApplicationWindow, DrawingArea, EventControllerFocus,
    EventControllerKey, IMMulticontext,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::capture::editor::{
    state::{normalize_pasted_line_endings, EditorState},
    types::{Point, Rect, ViewTransform},
    ui_support::text_input_has_focus,
};

fn text_input_cursor_rect(t: &ViewTransform, bounds: &Rect) -> gdk::Rectangle {
    let bottom_left = t.image_to_view(Point {
        x: bounds.x as f64,
        y: (bounds.y + bounds.height) as f64,
    });
    let bottom_right = t.image_to_view(Point {
        x: (bounds.x + bounds.width) as f64,
        y: (bounds.y + bounds.height) as f64,
    });
    gdk::Rectangle::new(
        bottom_left.x.round() as i32,
        bottom_left.y.round() as i32,
        ((bottom_right.x - bottom_left.x).round() as i32).max(1),
        1,
    )
}

fn deliver_clipboard_text(
    drawing_area_weak: &glib::WeakRef<DrawingArea>,
    in_motion: &Rc<Cell<bool>>,
    state: &Arc<Mutex<EditorState>>,
    bound_session: &Rc<Cell<Option<u64>>>,
    session_id: u64,
    text: &str,
) -> bool {
    if in_motion.get() {
        return false;
    }
    let Some(area) = drawing_area_weak.upgrade() else {
        return false;
    };
    if !area.is_realized() || area.root().is_none() {
        return false;
    }
    let text = normalize_pasted_line_endings(text);
    if text.is_empty() {
        return false;
    }
    {
        let mut st = state.lock().unwrap();
        if bound_session.get() != Some(session_id)
            || st.active_text_input.is_none()
            || st.text_input_session_id() != session_id
        {
            return false;
        }
        st.insert_text_input_str(&text);
        st.fit_active_text_to_layout();
        st.reset_text_cursor_blink();
    }
    area.queue_draw();
    true
}

fn start_clipboard_paste(
    drawing_area: &DrawingArea,
    in_motion: &Rc<Cell<bool>>,
    state: &Arc<Mutex<EditorState>>,
    bound_session: &Rc<Cell<Option<u64>>>,
    session_id: u64,
) {
    let clipboard = drawing_area.display().clipboard();
    let drawing_area_weak = drawing_area.downgrade();
    let in_motion = in_motion.clone();
    let state = state.clone();
    let bound_session = bound_session.clone();
    clipboard.read_text_async(None::<&gtk4::gio::Cancellable>, move |result| {
        let Ok(Some(text)) = result else {
            return;
        };
        deliver_clipboard_text(
            &drawing_area_weak,
            &in_motion,
            &state,
            &bound_session,
            session_id,
            &text,
        );
    });
}

fn wire_text_input_inner(
    window: &ApplicationWindow,
    in_motion: &Rc<Cell<bool>>,
    state: &Arc<Mutex<EditorState>>,
    transform: &Arc<Mutex<ViewTransform>>,
    drawing_area: &DrawingArea,
    im: &IMMulticontext,
    eligible: Rc<dyn Fn() -> bool>,
) -> EventControllerKey {
    im.set_use_preedit(false);
    im.set_input_purpose(gtk4::InputPurpose::FreeForm);

    let bound_session = Rc::new(Cell::new(None::<u64>));
    let im_focused = Rc::new(Cell::new(false));
    let composing = Rc::new(Cell::new(false));
    let last_surrounding = Rc::new(RefCell::new((String::new(), -1_i32)));
    let last_cursor_rect = Rc::new(RefCell::new(gdk::Rectangle::new(0, 0, 1, 1)));

    let sync_im: Rc<dyn Fn()> = {
        let im = im.clone();
        let in_motion = in_motion.clone();
        let state = state.clone();
        let transform = transform.clone();
        let drawing_area_weak = drawing_area.downgrade();
        let bound_session = bound_session.clone();
        let im_focused = im_focused.clone();
        let composing = composing.clone();
        let last_surrounding = last_surrounding.clone();
        let last_cursor_rect = last_cursor_rect.clone();
        let eligible = eligible.clone();
        Rc::new(move || {
            if drawing_area_weak.upgrade().is_none() {
                return;
            }
            let eligible = !in_motion.get() && eligible();
            let snapshot = if eligible {
                let st = state.lock().unwrap();
                if st.active_text_input.is_some() {
                    Some((
                        st.text_input_session_id(),
                        st.text_input_surrounding(),
                        st.active_text_bounds.clone(),
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            match snapshot {
                Some((session_id, surrounding, bounds)) => {
                    if bound_session.get() != Some(session_id) {
                        bound_session.set(None);
                        composing.set(false);
                        im.reset();
                        *last_surrounding.borrow_mut() = (String::new(), -1);
                        *last_cursor_rect.borrow_mut() = gdk::Rectangle::new(0, 0, 1, 1);
                        bound_session.set(Some(session_id));
                    }
                    if !im_focused.replace(true) {
                        im.focus_in();
                    }
                    if let Some((text, caret_byte)) = surrounding {
                        let mut cached = last_surrounding.borrow_mut();
                        if *cached != (text.clone(), caret_byte) {
                            *cached = (text.clone(), caret_byte);
                            drop(cached);
                            im.set_surrounding_with_selection(&text, caret_byte, caret_byte);
                        }
                    }
                    if let Some(bounds) = bounds {
                        let rect = {
                            let t = *transform.lock().unwrap();
                            text_input_cursor_rect(&t, &bounds.rect)
                        };
                        let mut cached = last_cursor_rect.borrow_mut();
                        if *cached != rect {
                            *cached = rect;
                            drop(cached);
                            im.set_cursor_location(&rect);
                        }
                    }
                }
                None => {
                    let was_bound = bound_session.replace(None).is_some();
                    let was_focused = im_focused.replace(false);
                    composing.set(false);
                    if was_bound || was_focused {
                        im.reset();
                        im.focus_out();
                        *last_surrounding.borrow_mut() = (String::new(), -1);
                        *last_cursor_rect.borrow_mut() = gdk::Rectangle::new(0, 0, 1, 1);
                    }
                }
            }
        })
    };

    {
        let im = im.clone();
        let sync_im = sync_im.clone();
        drawing_area.connect_realize(move |area| {
            im.set_client_widget(Some(area));
            sync_im();
        });
    }
    {
        let im = im.clone();
        let bound_session = bound_session.clone();
        let im_focused = im_focused.clone();
        let composing = composing.clone();
        let last_surrounding = last_surrounding.clone();
        let last_cursor_rect = last_cursor_rect.clone();
        drawing_area.connect_unrealize(move |_| {
            bound_session.set(None);
            im_focused.set(false);
            composing.set(false);
            *last_surrounding.borrow_mut() = (String::new(), -1);
            *last_cursor_rect.borrow_mut() = gdk::Rectangle::new(0, 0, 1, 1);
            im.reset();
            im.focus_out();
            im.set_client_widget(None::<&gtk4::Widget>);
        });
    }
    if drawing_area.is_realized() {
        im.set_client_widget(Some(drawing_area));
    }

    {
        let state = state.clone();
        let in_motion = in_motion.clone();
        let drawing_area_weak = drawing_area.downgrade();
        let bound_session = bound_session.clone();
        let eligible = eligible.clone();
        im.connect_commit(move |_ctx, text| {
            if in_motion.get() || !eligible() || drawing_area_weak.upgrade().is_none() {
                return;
            }
            let text = normalize_pasted_line_endings(text);
            if text.is_empty() {
                return;
            }
            {
                let mut st = state.lock().unwrap();
                let live = st
                    .active_text_input
                    .is_some()
                    .then(|| st.text_input_session_id());
                if live.is_none() || bound_session.get() != live {
                    return;
                }
                st.insert_text_input_str(&text);
                st.fit_active_text_to_layout();
                st.reset_text_cursor_blink();
            }
            if let Some(area) = drawing_area_weak.upgrade() {
                area.queue_draw();
            }
        });
    }
    {
        let state = state.clone();
        let bound_session = bound_session.clone();
        let eligible = eligible.clone();
        im.connect_retrieve_surrounding(move |ctx| {
            if !eligible() {
                return false;
            }
            let surrounding = {
                let st = state.lock().unwrap();
                let live = st
                    .active_text_input
                    .is_some()
                    .then(|| st.text_input_session_id());
                if live.is_none() || bound_session.get() != live {
                    None
                } else {
                    st.text_input_surrounding()
                }
            };
            match surrounding {
                Some((text, caret_byte)) => {
                    ctx.set_surrounding_with_selection(&text, caret_byte, caret_byte);
                    true
                }
                None => false,
            }
        });
    }
    {
        let state = state.clone();
        let in_motion = in_motion.clone();
        let drawing_area_weak = drawing_area.downgrade();
        let bound_session = bound_session.clone();
        let eligible = eligible.clone();
        im.connect_delete_surrounding(move |_ctx, offset, n_chars| {
            if in_motion.get() || !eligible() || drawing_area_weak.upgrade().is_none() {
                return false;
            }
            let deleted = {
                let mut st = state.lock().unwrap();
                let live = st
                    .active_text_input
                    .is_some()
                    .then(|| st.text_input_session_id());
                if live.is_none() || bound_session.get() != live {
                    return false;
                }
                let deleted = st.delete_surrounding_text_input(offset, n_chars);
                if deleted {
                    st.fit_active_text_to_layout();
                    st.reset_text_cursor_blink();
                }
                deleted
            };
            if deleted {
                if let Some(area) = drawing_area_weak.upgrade() {
                    area.queue_draw();
                }
            }
            deleted
        });
    }
    {
        let composing = composing.clone();
        im.connect_preedit_start(move |_ctx| {
            composing.set(true);
        });
    }
    {
        let composing = composing.clone();
        im.connect_preedit_end(move |_ctx| {
            composing.set(false);
        });
    }

    let focus_controller = EventControllerFocus::new();
    {
        let sync_im = sync_im.clone();
        focus_controller.connect_enter(move |_| sync_im());
    }
    {
        let sync_im = sync_im.clone();
        focus_controller.connect_leave(move |_| sync_im());
    }
    drawing_area.add_controller(focus_controller);
    {
        let sync_im = sync_im.clone();
        window.connect_is_active_notify(move |_| sync_im());
    }

    {
        let sync_im = sync_im.clone();
        let drawing_area_weak = drawing_area.downgrade();
        drawing_area.add_tick_callback(move |_, _| {
            if drawing_area_weak.upgrade().is_none() {
                return glib::ControlFlow::Break;
            }
            sync_im();
            glib::ControlFlow::Continue
        });
    }

    let key_controller = EventControllerKey::new();
    key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let in_motion = in_motion.clone();
        let state = state.clone();
        let window_weak = window.downgrade();
        let drawing_area_weak = drawing_area.downgrade();
        let im = im.clone();
        let bound_session = bound_session.clone();
        let composing = composing.clone();
        let sync_im = sync_im.clone();
        let eligible = eligible.clone();
        key_controller.connect_key_pressed(move |controller, key, _, modifiers| {
            if in_motion.get() {
                return glib::Propagation::Proceed;
            }
            let (Some(window), Some(area)) = (window_weak.upgrade(), drawing_area_weak.upgrade())
            else {
                return glib::Propagation::Proceed;
            };
            if text_input_has_focus(&window) || !eligible() {
                return glib::Propagation::Proceed;
            }
            sync_im();
            let session_id = {
                let st = state.lock().unwrap();
                if st.active_text_input.is_none() {
                    return glib::Propagation::Proceed;
                }
                st.text_input_session_id()
            };
            if bound_session.get() != Some(session_id) {
                return glib::Propagation::Proceed;
            }

            let was_composing = composing.get();
            let filtered = controller
                .current_event()
                .is_some_and(|event| im.filter_keypress(&event));
            if filtered || was_composing || composing.get() {
                return glib::Propagation::Stop;
            }

            let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
            let alt = modifiers.contains(gdk::ModifierType::ALT_MASK);
            let super_ = modifiers.contains(gdk::ModifierType::SUPER_MASK);

            if (ctrl && !alt && !super_ && matches!(key, gdk::Key::v | gdk::Key::V))
                || (shift && !ctrl && !alt && !super_ && key == gdk::Key::Insert)
            {
                start_clipboard_paste(&area, &in_motion, &state, &bound_session, session_id);
                return glib::Propagation::Stop;
            }

            let mut cancel = false;
            let mut handled = true;
            {
                let mut st = state.lock().unwrap();
                match key {
                    gdk::Key::Escape => cancel = true,
                    gdk::Key::Return | gdk::Key::KP_Enter => st.add_text_input_char('\n'),
                    gdk::Key::BackSpace => st.delete_text_input_char(),
                    gdk::Key::Delete => st.delete_text_input_char_forward(),
                    gdk::Key::Left => st.move_cursor_left(),
                    gdk::Key::Right => st.move_cursor_right(),
                    gdk::Key::Home => st.move_cursor_to_line_start(),
                    gdk::Key::End => st.move_cursor_to_line_end(),
                    _ => {
                        if ctrl || alt || super_ {
                            handled = false;
                        } else if let Some(ch) = key.to_unicode() {
                            if ch.is_control() {
                                handled = false;
                            } else {
                                st.add_text_input_char(ch);
                            }
                        } else {
                            handled = false;
                        }
                    }
                }

                if cancel {
                    st.cancel_text_input();
                }

                if handled && st.active_text_input.is_some() {
                    st.fit_active_text_to_layout();
                    st.reset_text_cursor_blink();
                }
            }

            if handled || cancel {
                sync_im();
                area.queue_draw();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    {
        let in_motion = in_motion.clone();
        let state = state.clone();
        let window_weak = window.downgrade();
        let drawing_area_weak = drawing_area.downgrade();
        let im = im.clone();
        let bound_session = bound_session.clone();
        let sync_im = sync_im.clone();
        let eligible = eligible.clone();
        key_controller.connect_key_released(move |controller, _, _, _| {
            if in_motion.get() {
                return;
            }
            let (Some(window), Some(_area)) = (window_weak.upgrade(), drawing_area_weak.upgrade())
            else {
                return;
            };
            if text_input_has_focus(&window) || !eligible() {
                return;
            }
            sync_im();
            let session_id = {
                let st = state.lock().unwrap();
                if st.active_text_input.is_none() {
                    return;
                }
                st.text_input_session_id()
            };
            if bound_session.get() != Some(session_id) {
                return;
            }
            if let Some(event) = controller.current_event() {
                im.filter_keypress(&event);
            }
        });
    }
    window.add_controller(key_controller.clone());
    key_controller
}

pub(super) fn wire_text_input(
    window: &ApplicationWindow,
    in_motion: &Rc<Cell<bool>>,
    state: &Arc<Mutex<EditorState>>,
    transform: &Arc<Mutex<ViewTransform>>,
    drawing_area: &DrawingArea,
) {
    let window_weak = window.downgrade();
    let area_weak = drawing_area.downgrade();
    let eligible = Rc::new(move || {
        let (Some(window), Some(area)) = (window_weak.upgrade(), area_weak.upgrade()) else {
            return false;
        };
        area.is_realized() && area.root().is_some() && area.has_focus() && window.is_active()
    });
    wire_text_input_inner(
        window,
        in_motion,
        state,
        transform,
        drawing_area,
        &IMMulticontext::new(),
        eligible,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::types::Tool;
    use gtk4::glib::translate::IntoGlib;
    use image::RgbaImage;

    fn pump() {
        let ctx = glib::MainContext::default();
        while ctx.iteration(false) {}
    }

    fn keyval(key: gdk::Key) -> u32 {
        key.into_glib() as u32
    }

    fn press(controller: &EventControllerKey, key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
        controller.emit_by_name::<bool>("key-pressed", &[&keyval(key), &0u32, &modifiers])
    }

    fn fixture(
        with_entry: bool,
    ) -> (
        gtk4::Application,
        ApplicationWindow,
        DrawingArea,
        Option<gtk4::Entry>,
        Rc<Cell<bool>>,
        Arc<Mutex<EditorState>>,
        Arc<Mutex<ViewTransform>>,
    ) {
        thread_local! {
            static TEST_APP: gtk4::Application = {
                let app = gtk4::Application::new(
                    Some("dev.apexshot.tests.textinput"),
                    Default::default(),
                );
                app.register(None::<&gtk4::gio::Cancellable>)
                    .expect("register test app");
                app
            };
        }
        let app = TEST_APP.with(|app| app.clone());
        let window = ApplicationWindow::new(&app);
        let area = DrawingArea::new();
        area.set_focusable(true);
        area.set_hexpand(true);
        area.set_vexpand(true);
        let entry = if with_entry {
            let entry = gtk4::Entry::new();
            let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            vbox.append(&area);
            vbox.append(&entry);
            window.set_child(Some(&vbox));
            Some(entry)
        } else {
            window.set_child(Some(&area));
            None
        };
        window.set_default_size(400, 300);
        window.present();
        area.grab_focus();
        pump();
        assert!(
            area.is_realized() && area.root().is_some(),
            "test drawing area failed to realize"
        );
        (
            app,
            window,
            area,
            entry,
            Rc::new(Cell::new(false)),
            Arc::new(Mutex::new(EditorState::new(RgbaImage::new(400, 300)))),
            Arc::new(Mutex::new(ViewTransform::for_image(400.0, 300.0))),
        )
    }

    fn test_eligibility(
        window: &ApplicationWindow,
        area: &DrawingArea,
        active: Rc<Cell<bool>>,
    ) -> Rc<dyn Fn() -> bool> {
        let window_weak = window.downgrade();
        let area_weak = area.downgrade();
        Rc::new(move || {
            let (Some(window), Some(area)) = (window_weak.upgrade(), area_weak.upgrade()) else {
                return false;
            };
            let area_widget: gtk4::Widget = area.clone().upcast();
            active.get()
                && area.is_realized()
                && area.root().is_some()
                && gtk4::prelude::GtkWindowExt::focus(&window)
                    .is_some_and(|focused| focused == area_widget)
        })
    }

    fn begin_session(state: &Arc<Mutex<EditorState>>) {
        let mut st = state.lock().unwrap();
        st.set_tool(Tool::Text);
        st.begin_text_input(Point { x: 20.0, y: 80.0 }, 200.0, 60.0);
    }

    #[test]
    fn cursor_rect_anchors_transformed_bounds_bottom() {
        let mut t = ViewTransform::for_image(400.0, 300.0);
        t.scale = 0.5;
        t.offset_x = 100.0;
        t.offset_y = 50.0;
        let rect = text_input_cursor_rect(
            &t,
            &Rect {
                x: 10,
                y: 20,
                width: 40,
                height: 10,
            },
        );
        assert_eq!(rect.x(), 105);
        assert_eq!(rect.y(), 65);
        assert_eq!(rect.width(), 20);
        assert_eq!(rect.height(), 1);
    }

    #[test]
    fn cursor_rect_follows_zoom_and_scroll() {
        let mut t = ViewTransform::for_image(400.0, 300.0);
        t.scale = 2.0;
        t.offset_x = -30.0;
        t.offset_y = 12.0;
        let rect = text_input_cursor_rect(
            &t,
            &Rect {
                x: 10,
                y: 20,
                width: 40,
                height: 10,
            },
        );
        assert_eq!(rect.x(), -10);
        assert_eq!(rect.y(), 72);
        assert_eq!(rect.width(), 80);
        assert_eq!(rect.height(), 1);
    }

    #[test]
    fn cursor_rect_minimum_width() {
        let t = ViewTransform::for_image(400.0, 300.0);
        let rect = text_input_cursor_rect(
            &t,
            &Rect {
                x: 10,
                y: 20,
                width: 0,
                height: 4,
            },
        );
        assert!(rect.width() >= 1);
        assert_eq!(rect.y(), 24);
    }

    #[test]
    fn commit_inserts_only_in_live_bound_session() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (_app, window, area, _entry, in_motion, state, transform) = fixture(false);
            let im = IMMulticontext::new();
            let active = Rc::new(Cell::new(true));
            let eligible = test_eligibility(&window, &area, active.clone());
            let controller = wire_text_input_inner(
                &window, &in_motion, &state, &transform, &area, &im, eligible,
            );
            pump();

            im.emit_by_name::<()>("commit", &[&"dropped"]);
            assert!(state.lock().unwrap().active_text_input.is_none());

            begin_session(&state);
            assert!(press(&controller, gdk::Key::x, gdk::ModifierType::empty()));
            pump();

            im.emit_by_name::<()>("commit", &[&"Tiếng"]);
            assert_eq!(
                state.lock().unwrap().get_text_input().unwrap().text,
                "xTiếng"
            );

            active.set(false);
            im.emit_by_name::<()>("commit", &[&"blocked"]);
            assert!(!press(&controller, gdk::Key::z, gdk::ModifierType::empty()));
            assert_eq!(
                state.lock().unwrap().get_text_input().unwrap().text,
                "xTiếng"
            );
            active.set(true);
            pump();

            {
                let mut st = state.lock().unwrap();
                st.cancel_text_input();
                st.begin_text_input(Point { x: 20.0, y: 80.0 }, 200.0, 60.0);
            }
            im.emit_by_name::<()>("commit", &[&"stale"]);
            assert_eq!(state.lock().unwrap().get_text_input().unwrap().text, "");

            assert!(press(&controller, gdk::Key::q, gdk::ModifierType::empty()));
            pump();
            im.emit_by_name::<()>("commit", &[&"ok"]);
            assert_eq!(state.lock().unwrap().get_text_input().unwrap().text, "qok");
            window.destroy();
            pump();
        }) else {
            return;
        };
    }

    #[test]
    fn retrieve_and_delete_surrounding_track_bound_session() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (_app, window, area, _entry, in_motion, state, transform) = fixture(false);
            let im = IMMulticontext::new();
            let active = Rc::new(Cell::new(true));
            let eligible = test_eligibility(&window, &area, active.clone());
            let controller = wire_text_input_inner(
                &window, &in_motion, &state, &transform, &area, &im, eligible,
            );
            pump();

            let handled = im.emit_by_name::<bool>("retrieve-surrounding", &[]);
            assert!(!handled);

            begin_session(&state);
            assert!(press(&controller, gdk::Key::a, gdk::ModifierType::empty()));
            state.lock().unwrap().insert_text_input_str("iế");
            press(&controller, gdk::Key::a, gdk::ModifierType::empty());
            pump();

            let handled = im.emit_by_name::<bool>("retrieve-surrounding", &[]);
            assert!(handled);
            let (text, caret) = state
                .lock()
                .unwrap()
                .text_input_surrounding()
                .expect("surrounding supplied to IM");
            assert_eq!(text, "aiếa");
            assert_eq!(caret, "aiếa".len() as i32);

            let deleted = im.emit_by_name::<bool>("delete-surrounding", &[&-3_i32, &2_i32]);
            assert!(deleted);
            assert_eq!(state.lock().unwrap().get_text_input().unwrap().text, "aa");

            {
                let mut st = state.lock().unwrap();
                st.cancel_text_input();
            }
            let handled = im.emit_by_name::<bool>("retrieve-surrounding", &[]);
            assert!(!handled);
            let deleted = im.emit_by_name::<bool>("delete-surrounding", &[&-1_i32, &1_i32]);
            assert!(!deleted);
            window.destroy();
            pump();
        }) else {
            return;
        };
    }

    #[test]
    fn composition_swallows_escape_and_editing_keys() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (_app, window, area, _entry, in_motion, state, transform) = fixture(false);
            let im = IMMulticontext::new();
            let active = Rc::new(Cell::new(true));
            let eligible = test_eligibility(&window, &area, active.clone());
            let controller = wire_text_input_inner(
                &window, &in_motion, &state, &transform, &area, &im, eligible,
            );
            pump();

            begin_session(&state);
            assert!(press(&controller, gdk::Key::x, gdk::ModifierType::empty()));
            pump();

            im.emit_by_name::<()>("preedit-start", &[]);
            assert!(press(
                &controller,
                gdk::Key::Escape,
                gdk::ModifierType::empty()
            ));
            assert!(state.lock().unwrap().active_text_input.is_some());
            assert!(press(&controller, gdk::Key::b, gdk::ModifierType::empty()));
            assert_eq!(state.lock().unwrap().get_text_input().unwrap().text, "x");

            im.emit_by_name::<()>("preedit-end", &[]);
            assert!(press(
                &controller,
                gdk::Key::Escape,
                gdk::ModifierType::empty()
            ));
            assert!(state.lock().unwrap().active_text_input.is_none());
            window.destroy();
            pump();
        }) else {
            return;
        };
    }

    #[test]
    fn paste_delivery_guards_session_focus_motion_and_lifetime() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (_app, window, area, _entry, in_motion, state, _transform) = fixture(false);
            let bound = Rc::new(Cell::new(None::<u64>));
            let area_weak = area.downgrade();

            begin_session(&state);
            let id = state.lock().unwrap().text_input_session_id();
            assert!(!deliver_clipboard_text(
                &area_weak, &in_motion, &state, &bound, id, "x"
            ));
            bound.set(Some(id));
            assert!(deliver_clipboard_text(
                &area_weak, &in_motion, &state, &bound, id, "ab"
            ));
            assert_eq!(state.lock().unwrap().get_text_input().unwrap().text, "ab");

            {
                let mut st = state.lock().unwrap();
                st.cancel_text_input();
                st.begin_text_input(Point { x: 20.0, y: 80.0 }, 200.0, 60.0);
            }
            let new_id = state.lock().unwrap().text_input_session_id();
            assert_ne!(id, new_id);
            assert!(!deliver_clipboard_text(
                &area_weak, &in_motion, &state, &bound, id, "stale"
            ));
            assert_eq!(state.lock().unwrap().get_text_input().unwrap().text, "");
            bound.set(Some(new_id));
            assert!(deliver_clipboard_text(
                &area_weak, &in_motion, &state, &bound, new_id, "p\r\nq"
            ));
            assert_eq!(state.lock().unwrap().get_text_input().unwrap().text, "p\nq");

            in_motion.set(true);
            assert!(!deliver_clipboard_text(
                &area_weak, &in_motion, &state, &bound, new_id, "nope"
            ));
            in_motion.set(false);

            let orphan = DrawingArea::new();
            let orphan_weak = orphan.downgrade();
            assert!(!deliver_clipboard_text(
                &orphan_weak,
                &in_motion,
                &state,
                &bound,
                new_id,
                "nope"
            ));

            window.destroy();
            pump();
            assert!(!deliver_clipboard_text(
                &area_weak, &in_motion, &state, &bound, new_id, "late"
            ));
        }) else {
            return;
        };
    }

    #[test]
    fn paste_shortcut_modifiers_and_native_field_bypass() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (_app, window, area, entry, in_motion, state, transform) = fixture(true);
            let im = IMMulticontext::new();
            let active = Rc::new(Cell::new(true));
            let eligible = test_eligibility(&window, &area, active.clone());
            let controller = wire_text_input_inner(
                &window, &in_motion, &state, &transform, &area, &im, eligible,
            );
            pump();

            begin_session(&state);
            assert!(press(&controller, gdk::Key::x, gdk::ModifierType::empty()));

            assert!(!press(
                &controller,
                gdk::Key::v,
                gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK
            ));
            assert!(!press(
                &controller,
                gdk::Key::Insert,
                gdk::ModifierType::SHIFT_MASK | gdk::ModifierType::SUPER_MASK
            ));
            assert!(press(
                &controller,
                gdk::Key::v,
                gdk::ModifierType::CONTROL_MASK
            ));
            assert!(press(
                &controller,
                gdk::Key::Insert,
                gdk::ModifierType::SHIFT_MASK
            ));

            let entry = entry.expect("entry fixture");
            entry.grab_focus();
            pump();
            assert!(!press(&controller, gdk::Key::x, gdk::ModifierType::empty()));
            assert!(!press(
                &controller,
                gdk::Key::v,
                gdk::ModifierType::CONTROL_MASK
            ));
            assert_eq!(state.lock().unwrap().get_text_input().unwrap().text, "x");

            window.destroy();
            pump();
        }) else {
            return;
        };
    }

    #[test]
    fn unrealize_unbinds_and_realize_rebinds() {
        let Some(()) = crate::test_support::with_gtk(|| {
            let (_app, window, area, _entry, in_motion, state, transform) = fixture(false);
            let im = IMMulticontext::new();
            let active = Rc::new(Cell::new(true));
            let eligible = test_eligibility(&window, &area, active.clone());
            let controller = wire_text_input_inner(
                &window, &in_motion, &state, &transform, &area, &im, eligible,
            );
            pump();

            begin_session(&state);
            assert!(press(&controller, gdk::Key::x, gdk::ModifierType::empty()));
            assert_eq!(state.lock().unwrap().get_text_input().unwrap().text, "x");

            window.set_child(None::<&gtk4::Widget>);
            pump();
            assert!(!area.is_realized());
            im.emit_by_name::<()>("commit", &[&"stale"]);
            assert_eq!(state.lock().unwrap().get_text_input().unwrap().text, "x");

            window.set_child(Some(&area));
            area.grab_focus();
            pump();
            assert!(press(&controller, gdk::Key::y, gdk::ModifierType::empty()));
            assert_eq!(state.lock().unwrap().get_text_input().unwrap().text, "xy");
            window.destroy();
            pump();
        }) else {
            return;
        };
    }
}
