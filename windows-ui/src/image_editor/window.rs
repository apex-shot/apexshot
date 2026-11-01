//! The Windows image-editor window.
//!
//! The frame follows the Linux editor: no caption bar, the checkerboard canvas
//! fills the window, and the floating tool bar rides in a transparent top
//! chrome strip that doubles as the drag handle. The caption controls sit at
//! the top of the inspector, the same place the Linux editor puts its window
//! controls.

use crate::capture::editor::window::icon_names;
use crate::i18n::t;
use crate::settings::ui_support::{caption_button, CaptionGlyph};
use crate::settings::windowing;
use gtk4::gdk;
use gtk4::glib::Propagation;
use gtk4::prelude::*;
use gtk4::{Align, Application, ApplicationWindow, Box as GtkBox};
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Once;

use super::inspector;
use super::toolbar::{self, Tool};
use super::ui_support::{self, px, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, TOP_CHROME_HEIGHT};
use super::{canvas, motion};

pub(super) fn build_window(app: &Application, initial: Option<&Path>) {
    static INIT_ICONS: Once = Once::new();
    INIT_ICONS.call_once(|| {
        relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);
    });

    ui_support::install_css();

    let scale = crate::platform::ui_scale();
    let (default_width, default_height) = ui_support::default_window_size(scale);
    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("ApexShot Editor"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(default_width)
        .default_height(default_height)
        .build();
    window.set_decorated(false);
    window.add_css_class("editor-window");
    window.add_css_class("image-editor-window");
    window.set_size_request(px(MIN_WINDOW_WIDTH, scale), px(MIN_WINDOW_HEIGHT, scale));

    let dark = windowing::prefers_dark_glass_theme();
    let root = GtkBox::new(gtk4::Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class("image-editor-root");
    root.add_css_class(if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });
    if windowing::prefers_reduced_transparency() {
        root.add_css_class("editor-reduced-transparency");
    }
    // Keep the canvas inside the shell's rounded corners.
    root.set_overflow(gtk4::Overflow::Hidden);

    let canvas = canvas::build(scale, dark);
    let effect_control = Rc::new(super::effect_control::EffectControl::new(
        scale,
        &canvas.host,
    ));
    canvas.root.add_overlay(&effect_control.root);
    let inspector = Rc::new(inspector::build(scale, &canvas.host, &window));
    let motion = Rc::new(motion::build(scale, dark, inspector.clone()));
    let sub_bars = Rc::new(super::sub_bar::build(
        scale,
        canvas.host.clone(),
        {
            let canvas = canvas.clone();
            Rc::new(move |docked| canvas.set_docked(docked))
        },
        Some(Rc::downgrade(&canvas)),
    ));
    let toolbar = Rc::new(toolbar::build(scale, Tool::Select, {
        let inspector = inspector.clone();
        let host = canvas.host.clone();
        Rc::new(move |tool| {
            host.activate_tool(tool.to_model());
            inspector.show_tool(tool);
        })
    }));
    canvas.root.add_overlay(&sub_bars.root);
    canvas.root.add_overlay(&toolbar.history);

    // Model -> chrome: bars, history sensitivity and action buttons refresh
    // after every document or selection mutation.
    {
        let sync_all = {
            let sub_bars = sub_bars.clone();
            let host = canvas.host.clone();
            let undo = toolbar.undo.clone();
            let redo = toolbar.redo.clone();
            let delete = toolbar.delete.clone();
            let copy = inspector.copy_button.clone();
            let export = inspector.export_button.clone();
            let toolbar = toolbar.clone();
            let canvas = canvas.clone();
            let effect_control = effect_control.clone();
            let inspector = inspector.clone();
            Rc::new(move || {
                // View math follows the document so crop/undo re-fit and
                // hit-testing tracks the real pixels.
                canvas.sync_view_to_document();
                host.read(|st| {
                    toolbar.set_active_tool(Tool::from_model(st.selected_tool));
                    sub_bars.sync(Some(st));
                    canvas.set_crop_mode(
                        st.selected_tool == crate::capture::editor::types::Tool::Crop,
                    );
                    effect_control.sync(st, &canvas);
                    undo.set_sensitive(st.can_undo());
                    redo.set_sensitive(st.can_redo());
                    delete.set_sensitive(st.selected_action_index.is_some());
                });
                let has_doc = host.has_document();
                inspector.sync_background();
                copy.set_sensitive(has_doc && !host.background_loading());
                export.set_sensitive(has_doc && !host.background_loading());
            })
        };
        canvas.add_change_listener(sync_all.clone());
        sync_all();
    }

    toolbar.undo.connect_clicked({
        let host = canvas.host.clone();
        move |_| host.undo()
    });
    toolbar.redo.connect_clicked({
        let host = canvas.host.clone();
        move |_| host.redo()
    });
    toolbar.delete.connect_clicked({
        let host = canvas.host.clone();
        move |_| host.mutate_effects(|st| st.remove_selected_action())
    });

    inspector.copy_button.connect_clicked({
        let host = canvas.host.clone();
        move |_| host.copy_to_clipboard()
    });
    inspector.export_button.connect_clicked({
        let host = canvas.host.clone();
        let window = window.clone();
        move |_| {
            save_as_dialog(&window, &host);
        }
    });

    // The workspace holds the two mode pages: the annotate canvas and the
    // Motion page with its preview and timeline dock.
    let workspace_stack = gtk4::Stack::new();
    workspace_stack.set_hhomogeneous(false);
    workspace_stack.set_vhomogeneous(false);
    workspace_stack.set_hexpand(true);
    workspace_stack.set_vexpand(true);
    workspace_stack.add_named(&canvas.root, Some("static"));
    workspace_stack.add_named(&motion.page, Some("motion"));
    workspace_stack.set_visible_child_name("static");

    // The top toolbar swaps with the mode: the annotation rail for Static,
    // the Static button for Motion.
    let toolbar_stack = gtk4::Stack::new();
    toolbar_stack.set_hhomogeneous(false);
    toolbar_stack.set_vhomogeneous(false);
    toolbar_stack.set_halign(Align::Center);
    toolbar_stack.set_valign(Align::Start);
    toolbar_stack.add_named(&toolbar.root, Some("static"));
    toolbar_stack.add_named(&motion.toolbar, Some("motion"));
    toolbar_stack.set_visible_child_name("static");

    inspector.set_motion_duration_listener({
        let timeline = motion.timeline.clone();
        Rc::new(move |seconds| timeline.set_duration(seconds))
    });

    // Motion button opens Motion mode; the Motion toolbar's Static button
    // returns. Neither asks for confirmation here — the confirm dialog asks
    // about annotations, and the shell carries none yet.
    let switch_mode = {
        let workspace_stack = workspace_stack.clone();
        let toolbar_stack = toolbar_stack.clone();
        let inspector = inspector.clone();
        let motion = motion.clone();
        Rc::new(move |motion_now: bool| {
            let page = if motion_now { "motion" } else { "static" };
            for stack in [&workspace_stack, &toolbar_stack] {
                stack.set_visible_child_name(page);
            }
            inspector.set_mode(motion_now);
            if motion_now {
                motion.set_active_tab("motion");
            }
        })
    };
    canvas.motion_button.connect_clicked({
        let switch_mode = switch_mode.clone();
        move |_| switch_mode(true)
    });
    motion.static_button.connect_clicked({
        let switch_mode = switch_mode.clone();
        move |_| switch_mode(false)
    });

    // Transparent strip over the top edge of both mode pages: it centres the
    // tool bar stacks and is the strip the window drags from, like the Linux
    // top chrome. The Motion toolbar's Static button lives in the same slot.
    let top_chrome = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    top_chrome.add_css_class("image-editor-top-chrome");
    top_chrome.set_halign(Align::Fill);
    top_chrome.set_valign(Align::Start);
    top_chrome.set_hexpand(true);
    top_chrome.set_size_request(-1, px(TOP_CHROME_HEIGHT, scale));
    let top_left = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    top_left.set_hexpand(true);
    let top_right = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    top_right.set_hexpand(true);
    top_chrome.append(&top_left);
    top_chrome.append(&toolbar_stack);
    top_chrome.append(&top_right);
    windowing::install_window_drag(&top_chrome, &window);

    // The pages live under an overlay host so the top chrome covers both.
    let pages_overlay = ui_support::editor_overlay();
    pages_overlay.set_hexpand(true);
    pages_overlay.set_vexpand(true);
    pages_overlay.set_child(Some(&workspace_stack));
    pages_overlay.add_overlay(&top_chrome);

    let utility = build_utility_strip(&window, scale);
    inspector.root.prepend(&utility);

    let workspace = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    workspace.add_css_class("image-editor-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);
    workspace.append(&pages_overlay);
    workspace.append(&inspector.root);
    root.append(&workspace);

    window.set_child(Some(&root));

    canvas.open_button.connect_clicked({
        let canvas = canvas.clone();
        let window = window.clone();
        move |_| canvas.choose_image(&window)
    });

    install_keys(&window, &canvas, &sub_bars);

    window.present();

    if let Some(path) = initial {
        canvas.load(path);
    }
}

/// The caption controls at the top of the inspector: an empty drag area on the
/// left, then minimize, maximize and close hugging the window's top-right
/// corner, where the Linux utility strip keeps its window buttons.
fn build_utility_strip(window: &ApplicationWindow, scale: f64) -> GtkBox {
    let strip = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    strip.add_css_class("image-editor-utility-controls");
    strip.set_halign(Align::Fill);

    let drag_area = GtkBox::new(gtk4::Orientation::Horizontal, 0);
    drag_area.set_hexpand(true);
    strip.append(&drag_area);
    windowing::install_window_drag(&drag_area, window);

    let (minimize, _) = caption_button(
        Rc::new(Cell::new(CaptionGlyph::Minimize)),
        &t("Minimize"),
        scale,
    );
    let minimize_window = window.clone();
    minimize.connect_clicked(move |_| minimize_window.minimize());

    let maximize_state = Rc::new(Cell::new(CaptionGlyph::Maximize));
    let (maximize, maximize_glyph) = caption_button(maximize_state.clone(), &t("Maximize"), scale);
    let maximize_window = window.clone();
    maximize.connect_clicked(move |_| {
        if maximize_window.is_maximized() {
            maximize_window.unmaximize();
        } else {
            maximize_window.maximize();
        }
    });
    window.connect_maximized_notify({
        let state = maximize_state;
        move |window| {
            state.set(if window.is_maximized() {
                CaptionGlyph::Restore
            } else {
                CaptionGlyph::Maximize
            });
            maximize_glyph.queue_draw();
        }
    });

    let (close, _) = caption_button(Rc::new(Cell::new(CaptionGlyph::Close)), &t("Close"), scale);
    let close_window = window.clone();
    close.connect_clicked(move |_| close_window.close());

    strip.append(&minimize);
    strip.append(&maximize);
    strip.append(&close);
    strip
}

/// Save As through the platform dialog; the filter set is the formats the
/// encoder writes (PNG, JPEG, WebP). The dialog handles overwrite prompts.
fn save_as_dialog(window: &ApplicationWindow, host: &Rc<super::annotate::EditorHost>) {
    if !host.has_document() || host.background_loading() {
        return;
    }
    let dialog = gtk4::FileDialog::new();
    dialog.set_title(&t("Save annotated image"));
    dialog.set_initial_name(Some("annotated.png"));
    let filters = gtk4::gio::ListStore::new::<gtk4::FileFilter>();
    let images = gtk4::FileFilter::new();
    images.set_name(Some(&t("Images")));
    for mime in ["image/png", "image/jpeg", "image/webp"] {
        images.add_mime_type(mime);
    }
    filters.append(&images);
    dialog.set_filters(Some(&filters));
    dialog.set_default_filter(Some(&images));
    let host = host.clone();
    let parent = window.clone();
    dialog.save(Some(window), gtk4::gio::Cancellable::NONE, move |result| {
        if let Ok(file) = result {
            if let Some(path) = file.path() {
                host.save_as(&parent, &path);
            }
        }
    });
}

fn install_keys(
    window: &ApplicationWindow,
    canvas: &Rc<canvas::Canvas>,
    sub_bars: &Rc<super::sub_bar::SubBars>,
) {
    // Space held = pan mode. Capture phase so it wins over the canvas, but a
    // focused text entry still gets its literal space character.
    let space = gtk4::EventControllerKey::new();
    space.set_propagation_phase(gtk4::PropagationPhase::Capture);
    space.connect_key_pressed({
        let canvas = canvas.clone();
        let window = window.clone();
        let sub_bars = sub_bars.clone();
        move |controller, key, _, _| {
            if key == gdk::Key::Escape && sub_bars.close_color_panels() {
                return Propagation::Stop;
            }
            // A focused editable (sub-bar entry, motion fields) owns its keys.
            let editable_focused = controller
                .current_event()
                .and_then(|_| gtk4::prelude::RootExt::focus(&window))
                .is_some_and(|w| w.is::<gtk4::Editable>() || w.is::<gtk4::TextView>());
            if editable_focused {
                return Propagation::Proceed;
            }
            if key == gdk::Key::space && canvas.host.set_space_pan(true) {
                canvas.area.set_cursor_from_name(Some("grab"));
                return Propagation::Stop;
            }
            Propagation::Proceed
        }
    });
    space.connect_key_released({
        let canvas = canvas.clone();
        move |_, key, _, _| {
            if key == gdk::Key::space && canvas.host.space_pan_active() {
                canvas.host.set_space_pan(false);
                canvas.area.set_cursor_from_name(None);
            }
        }
    });
    window.add_controller(space);

    // Releasing focus cancels any held pan state.
    let focus = gtk4::EventControllerFocus::new();
    focus.connect_leave({
        let host = canvas.host.clone();
        move |_| {
            host.set_space_pan(false);
        }
    });
    window.add_controller(focus);

    let keys = gtk4::EventControllerKey::new();
    keys.connect_key_pressed({
        let canvas = canvas.clone();
        let window = window.clone();
        move |_, key, _, modifiers| {
            let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
            // A focused editable owns its keys: entries keep characters and
            // their own shortcuts instead of triggering editor commands.
            if gtk4::prelude::RootExt::focus(&window)
                .is_some_and(|w| w.is::<gtk4::Editable>() || w.is::<gtk4::TextView>())
            {
                return Propagation::Proceed;
            }
            // The document handles editing keys first (text input, tools,
            // history, delete, cancel cascades); what it passes on falls
            // through to the window shortcuts below.
            if canvas.host.handle_key(key, modifiers) == Propagation::Stop {
                return Propagation::Stop;
            }
            match key {
                gdk::Key::Escape => {
                    window.close();
                    Propagation::Stop
                }
                gdk::Key::s | gdk::Key::S if ctrl => {
                    save_as_dialog(&window, &canvas.host);
                    Propagation::Stop
                }
                gdk::Key::c | gdk::Key::C if ctrl => {
                    canvas.host.copy_to_clipboard();
                    Propagation::Stop
                }
                gdk::Key::_0 | gdk::Key::KP_0 if ctrl => {
                    canvas.fit();
                    Propagation::Stop
                }
                gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add if ctrl => {
                    canvas.zoom_in();
                    Propagation::Stop
                }
                gdk::Key::minus | gdk::Key::underscore | gdk::Key::KP_Subtract if ctrl => {
                    canvas.zoom_out();
                    Propagation::Stop
                }
                gdk::Key::o | gdk::Key::O if ctrl => {
                    canvas.choose_image(&window);
                    Propagation::Stop
                }
                gdk::Key::w | gdk::Key::W if ctrl => {
                    window.close();
                    Propagation::Stop
                }
                _ => Propagation::Proceed,
            }
        }
    });
    window.add_controller(keys);
}
