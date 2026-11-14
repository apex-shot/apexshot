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
use super::{canvas, mode_confirm, motion};

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
    let motion = Rc::new(motion::build(scale, dark, inspector.clone(), &canvas));
    let mode_confirm = Rc::new(mode_confirm::build(scale, dark));
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

    // Upload keeps its own busy state so a document change mid-request cannot
    // re-enable the button.
    let uploading = Rc::new(Cell::new(false));
    // Copy, Upload and Export act on the Motion composition while that page
    // is up; video exports disable the button until the encoder finishes.
    let in_motion = Rc::new(Cell::new(false));
    let exporting = Rc::new(Cell::new(false));
    let motion_export_name = initial
        .and_then(|path| path.file_stem())
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(|stem| format!("{stem} Motion.mp4"))
        .unwrap_or_else(|| "ApexShot Motion.mp4".to_string());

    // Model -> chrome: bars, history sensitivity and action buttons refresh
    // after every document or selection mutation.
    let sync_all = {
        let sub_bars = sub_bars.clone();
        let host = canvas.host.clone();
        let undo = toolbar.undo.clone();
        let redo = toolbar.redo.clone();
        let delete = toolbar.delete.clone();
        let copy = inspector.copy_button.clone();
        let upload = inspector.upload_button.clone();
        let uploading = uploading.clone();
        let export = inspector.export_button.clone();
        let exporting = exporting.clone();
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
                canvas.set_crop_mode(st.selected_tool == crate::capture::editor::types::Tool::Crop);
                effect_control.sync(st, &canvas);
                undo.set_sensitive(st.can_undo());
                redo.set_sensitive(st.can_redo());
                delete.set_sensitive(st.selected_action_index.is_some());
            });
            let has_doc = host.has_document();
            inspector.sync_background();
            copy.set_sensitive(has_doc && !host.background_loading());
            upload.set_sensitive(has_doc && !host.background_loading() && !uploading.get());
            export.set_sensitive(has_doc && !host.background_loading() && !exporting.get());
            canvas.refresh_cursor();
        })
    };
    canvas.add_change_listener(sync_all.clone());
    sync_all();

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
        let window = window.clone();
        let motion = motion.clone();
        let in_motion = in_motion.clone();
        let button = inspector.copy_button.downgrade();
        move |_| {
            if !in_motion.get() {
                copy_image(&window, &host);
                return;
            }
            // A Motion frame of a Studio-styled composition is a styled still.
            let Some(capabilities) =
                host.read(|st| crate::studio_entitlement::StudioCapabilities {
                    studio_style: st.uses_studio_style(),
                    ..Default::default()
                })
            else {
                return;
            };
            let retry_host = Rc::downgrade(&host);
            let retry_button = button.clone();
            if !crate::studio_upsell::gate(&window, capabilities, move || {
                if let Some(host) = retry_host.upgrade() {
                    host.invalidate_studio_cache();
                }
                if let Some(button) = retry_button.upgrade() {
                    button.emit_clicked();
                }
            }) {
                return;
            }
            match motion.current_frame_image() {
                Some(frame) => copy_frame_image(&host, frame),
                None => {
                    // The Motion page covers the canvas toast, so the failure
                    // also goes to a system notification.
                    let message = t("Could not render the Motion frame.");
                    crate::utils::notify::desktop_notification_important(
                        &t("Copy failed"),
                        &message,
                    );
                    host.report_error(message);
                }
            }
        }
    });
    inspector.upload_button.connect_clicked({
        let host = canvas.host.clone();
        let window = window.clone();
        let button = inspector.upload_button.clone();
        let uploading = uploading.clone();
        let motion = motion.clone();
        let in_motion = in_motion.clone();
        let name = motion_export_name.clone();
        move |_| {
            if !in_motion.get() {
                upload_image(&window, &host, &button, &uploading);
                return;
            }
            upload_motion_video(&window, &host, &motion, &button, &uploading, &name);
        }
    });
    inspector.export_button.connect_clicked({
        let host = canvas.host.clone();
        let window = window.clone();
        let motion = motion.clone();
        let in_motion = in_motion.clone();
        let exporting = exporting.clone();
        let name = motion_export_name.clone();
        let sync_all = sync_all.clone();
        move |_| {
            if in_motion.get() {
                let sync_all: Rc<dyn Fn()> = sync_all.clone();
                save_motion_video_dialog(&window, &host, &motion, &exporting, &name, sync_all);
            } else {
                save_as_dialog(&window, &host);
            }
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
    // returns. Commit any inline text before showing the motion source.
    let switch_mode = {
        let workspace_stack = workspace_stack.clone();
        let toolbar_stack = toolbar_stack.clone();
        let inspector = inspector.clone();
        let motion = motion.clone();
        let host = canvas.host.clone();
        let in_motion = in_motion.clone();
        Rc::new(move |motion_now: bool| {
            in_motion.set(motion_now);
            if motion_now {
                host.commit_text_if_active();
                host.set_space_pan(false);
                host.changed();
            } else {
                // Leaving Motion drops its clips, the way the Linux session
                // clears the snapshot; the warning below is what tells the
                // user that. Shared appearance settings survive.
                motion.timeline.clear_clips();
            }
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
    // Both mode buttons ask first when the switch would drop work the other
    // mode cannot show.
    let request_mode = {
        let switch_mode = switch_mode.clone();
        let confirm = mode_confirm.clone();
        let host = canvas.host.clone();
        let timeline = motion.timeline.clone();
        Rc::new(move |motion_now: bool| {
            let switch = if motion_now {
                mode_confirm::ModeSwitch::ToMotion
            } else {
                mode_confirm::ModeSwitch::ToStatic
            };
            let config = crate::config::load_config();
            // Pending text counts as an annotation: entering Motion commits
            // it before the snapshot is taken.
            let has_static_annotations = host
                .read(|st| {
                    !st.actions.is_empty()
                        || st
                            .active_text_input
                            .as_ref()
                            .is_some_and(|input| !input.text.trim().is_empty())
                })
                .unwrap_or(false);
            let has_motion_segments = timeline.motion_state().has_segments();
            if mode_confirm::needs_confirmation(
                switch,
                has_static_annotations,
                has_motion_segments,
                &config,
            ) {
                let switch_mode = switch_mode.clone();
                confirm.ask(switch, Rc::new(move || switch_mode(motion_now)));
                return;
            }
            switch_mode(motion_now);
        })
    };
    canvas.motion_button.connect_clicked({
        let request_mode = request_mode.clone();
        move |_| request_mode(true)
    });
    motion.static_button.connect_clicked({
        let request_mode = request_mode.clone();
        move |_| request_mode(false)
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

    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(&root));
    overlay.add_overlay(mode_confirm.overlay());
    window.set_child(Some(&overlay));

    canvas.open_button.connect_clicked({
        let canvas = canvas.clone();
        let window = window.clone();
        move |_| canvas.choose_image(&window)
    });

    install_keys(
        &window,
        &canvas,
        &sub_bars,
        &workspace_stack,
        &mode_confirm,
        &motion,
        &inspector.copy_button,
        &exporting,
        &motion_export_name,
        &sync_all,
    );

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
    let retry_window = window.downgrade();
    let retry_host = Rc::downgrade(host);
    if !gate_image_output(window, host, move || {
        if let (Some(window), Some(host)) = (retry_window.upgrade(), retry_host.upgrade()) {
            save_as_dialog(&window, &host);
        }
    }) {
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

/// Export the whole Motion composition as an MP4. The gate matches Motion's
/// video export entitlement; the encoder renders a frame per main-loop tick
/// so the dialog and window stay usable while it works.
fn save_motion_video_dialog(
    window: &ApplicationWindow,
    host: &Rc<super::annotate::EditorHost>,
    motion: &Rc<motion::MotionMode>,
    exporting: &Rc<Cell<bool>>,
    name: &str,
    sync_all: Rc<dyn Fn()>,
) {
    if exporting.get() || !host.has_document() || host.background_loading() {
        return;
    }
    let retry_window = window.downgrade();
    let retry_host = Rc::downgrade(host);
    let retry_motion = Rc::downgrade(motion);
    let retry_exporting = exporting.clone();
    let retry_name = name.to_string();
    let retry_sync = sync_all.clone();
    let retry_host_for_cache = Rc::downgrade(host);
    if !crate::studio_upsell::gate(
        window,
        crate::studio_entitlement::capabilities_for_motion_export(),
        move || {
            if let Some(host) = retry_host_for_cache.upgrade() {
                host.invalidate_studio_cache();
            }
            if let (Some(window), Some(host), Some(motion)) = (
                retry_window.upgrade(),
                retry_host.upgrade(),
                retry_motion.upgrade(),
            ) {
                save_motion_video_dialog(
                    &window,
                    &host,
                    &motion,
                    &retry_exporting,
                    &retry_name,
                    retry_sync.clone(),
                );
            }
        },
    ) {
        return;
    }

    let dialog = gtk4::FileDialog::new();
    dialog.set_title(&t("Export Motion video"));
    let config = crate::config::load_config().sanitized();
    let folder = config.motion_export_dir(&crate::recording::recording_output_dir(&config));
    dialog.set_initial_folder(Some(&gtk4::gio::File::for_path(&folder)));
    dialog.set_initial_name(Some(name));
    let filter = gtk4::FileFilter::new();
    filter.set_name(Some(&t("MP4 video")));
    filter.add_mime_type("video/mp4");
    filter.add_pattern("*.mp4");
    filter.add_pattern("*.MP4");
    let filters = gtk4::gio::ListStore::new::<gtk4::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    dialog.set_default_filter(Some(&filter));

    let motion = motion.clone();
    let host = host.clone();
    let exporting = exporting.clone();
    let sync_all = sync_all.clone();
    dialog.save(Some(window), gtk4::gio::Cancellable::NONE, move |result| {
        if let Ok(file) = result {
            if let Some(path) = file.path() {
                let path = with_mp4_extension(path);
                exporting.set(true);
                sync_all();
                let result = motion.export_video(path, {
                    let exporting = exporting.clone();
                    let sync_all = sync_all.clone();
                    let host = host.clone();
                    move |result| {
                        exporting.set(false);
                        sync_all();
                        match result {
                            Ok(path) => {
                                let mut config = crate::config::load_config();
                                if config.remember_video_export_dir(&path) {
                                    let _ = crate::config::save_config(&config);
                                }
                                crate::utils::notify::desktop_notification(
                                    &t("Export complete"),
                                    &path.display().to_string(),
                                );
                            }
                            Err(error) => {
                                // The Motion page hides the canvas toast, so
                                // the export failure also goes to the desktop.
                                crate::utils::notify::desktop_notification_important(
                                    &t("Export failed"),
                                    &error,
                                );
                                host.report_error(error);
                            }
                        }
                    }
                });
                if let Err(error) = result {
                    exporting.set(false);
                    sync_all();
                    crate::utils::notify::desktop_notification_important(
                        &t("Export failed"),
                        &error,
                    );
                    host.report_error(error);
                }
            }
        }
    });
}

/// Motion is always encoded as MP4, so an extension-less or mis-typed choice
/// from the Save chooser is corrected rather than written as-is.
fn with_mp4_extension(mut destination: std::path::PathBuf) -> std::path::PathBuf {
    let is_mp4 = destination
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"));
    if !is_mp4 {
        destination.set_extension("mp4");
    }
    destination
}

fn gate_image_output(
    window: &ApplicationWindow,
    host: &Rc<super::annotate::EditorHost>,
    retry: impl Fn() + 'static,
) -> bool {
    let Some(capabilities) = host.read(|st| st.studio_capabilities()) else {
        return false;
    };
    let retry_host = Rc::downgrade(host);
    crate::studio_upsell::gate(window, capabilities, move || {
        if let Some(host) = retry_host.upgrade() {
            host.invalidate_studio_cache();
        }
        retry();
    })
}

fn copy_image(window: &ApplicationWindow, host: &Rc<super::annotate::EditorHost>) {
    if !host.has_document() || host.background_loading() {
        return;
    }
    let retry_window = window.downgrade();
    let retry_host = Rc::downgrade(host);
    if gate_image_output(window, host, move || {
        if let (Some(window), Some(host)) = (retry_window.upgrade(), retry_host.upgrade()) {
            copy_image(&window, &host);
        }
    }) {
        host.copy_to_clipboard();
    }
}

/// Copy the already rendered Motion pose. The caller runs the Studio style
/// gate first; the static pose gate does not apply to Motion poses.
fn copy_frame_image(host: &Rc<super::annotate::EditorHost>, frame: image::RgbaImage) {
    if !host.has_document() || host.background_loading() {
        return;
    }
    host.copy_frame_to_clipboard(frame);
}

/// Upload the flattened document as a temporary PNG. The temp file lives only
/// for the request: Cloud responds with the share URL, which goes to the
/// clipboard so the next paste carries the link.
fn upload_image(
    window: &ApplicationWindow,
    host: &Rc<super::annotate::EditorHost>,
    button: &gtk4::Button,
    uploading: &Rc<Cell<bool>>,
) {
    if uploading.get() || !host.has_document() || host.background_loading() {
        return;
    }
    let retry_window = window.downgrade();
    let retry_host = Rc::downgrade(host);
    let retry_button = button.downgrade();
    let retry_uploading = uploading.clone();
    if !gate_image_output(window, host, move || {
        if let (Some(window), Some(host), Some(button)) = (
            retry_window.upgrade(),
            retry_host.upgrade(),
            retry_button.upgrade(),
        ) {
            upload_image(&window, &host, &button, &retry_uploading);
        }
    }) {
        return;
    }

    let Some(config) = configured_cloud() else {
        return;
    };
    let snapshot = match host.upload_snapshot() {
        Ok(snapshot) => snapshot,
        Err(message) => {
            // A render failure must not look like a no-op: report it the way
            // Copy reports its own flatten failure.
            crate::utils::notify::desktop_notification_important(&t("Upload failed"), &message);
            host.report_error(message);
            return;
        }
    };
    upload_rendered_image(window, button, uploading, config, snapshot);
}

fn configured_cloud() -> Option<crate::config::AppConfig> {
    let config = crate::config::load_config().sanitized();
    if !crate::cloud::upload::is_configured(&config) {
        let (title, body) = crate::cloud::upload::not_configured_notification(&config);
        crate::utils::notify::desktop_notification_important(&title, &body);
        return None;
    }
    Some(config)
}

/// Upload the Motion composition as an MP4. Rendering happens first, one
/// frame per main-loop tick; the finished file goes through the same Cloud
/// request and share-URL clipboard as a still.
fn upload_motion_video(
    window: &ApplicationWindow,
    host: &Rc<super::annotate::EditorHost>,
    motion: &Rc<motion::MotionMode>,
    button: &gtk4::Button,
    uploading: &Rc<Cell<bool>>,
    name: &str,
) {
    if uploading.get() || !host.has_document() || host.background_loading() {
        return;
    }
    let retry_window = window.downgrade();
    let retry_host = Rc::downgrade(host);
    let retry_motion = Rc::downgrade(motion);
    let retry_button = button.downgrade();
    let retry_uploading = uploading.clone();
    let retry_name = name.to_string();
    let retry_host_for_cache = Rc::downgrade(host);
    if !crate::studio_upsell::gate(
        window,
        crate::studio_entitlement::capabilities_for_motion_export(),
        move || {
            if let Some(host) = retry_host_for_cache.upgrade() {
                host.invalidate_studio_cache();
            }
            if let (Some(window), Some(host), Some(motion), Some(button)) = (
                retry_window.upgrade(),
                retry_host.upgrade(),
                retry_motion.upgrade(),
                retry_button.upgrade(),
            ) {
                upload_motion_video(
                    &window,
                    &host,
                    &motion,
                    &button,
                    &retry_uploading,
                    &retry_name,
                );
            }
        },
    ) {
        return;
    }

    let Some(config) = configured_cloud() else {
        return;
    };
    let directory = upload_temp_dir();
    if let Err(error) = std::fs::create_dir_all(&directory) {
        let message = format!("Could not prepare the Motion upload: {error}");
        crate::utils::notify::desktop_notification_important(&t("Upload failed"), &message);
        host.report_error(message);
        return;
    }
    let path = directory.join(name);
    let cleanup = directory.clone();
    uploading.set(true);
    button.set_sensitive(false);
    let result = motion.export_video(path, {
        let window = window.downgrade();
        let button = button.downgrade();
        let uploading = uploading.clone();
        let host = host.clone();
        move |result| match result {
            Ok(path) => {
                if let (Some(window), Some(button)) = (window.upgrade(), button.upgrade()) {
                    upload_path(&window, &button, &uploading, config, path, Some(cleanup));
                } else {
                    uploading.set(false);
                    let _ = std::fs::remove_dir_all(&cleanup);
                }
            }
            Err(error) => {
                uploading.set(false);
                if let Some(button) = button.upgrade() {
                    button.set_sensitive(true);
                }
                let _ = std::fs::remove_dir_all(&cleanup);
                crate::utils::notify::desktop_notification_important(&t("Upload failed"), &error);
                host.report_error(error);
            }
        }
    });
    if let Err(error) = result {
        uploading.set(false);
        button.set_sensitive(true);
        let _ = std::fs::remove_dir_all(&directory);
        crate::utils::notify::desktop_notification_important(&t("Upload failed"), &error);
        host.report_error(error);
    }
}

fn upload_temp_dir() -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("apexshot")
        .join("upload-snapshots")
        .join(format!("{}-{stamp}", std::process::id()))
}

/// Upload a prepared file and put the share URL on the clipboard. `cleanup`
/// removes the temporary folder once the request finishes.
fn upload_path(
    window: &ApplicationWindow,
    button: &gtk4::Button,
    uploading: &Rc<Cell<bool>>,
    config: crate::config::AppConfig,
    path: std::path::PathBuf,
    cleanup: Option<std::path::PathBuf>,
) {
    uploading.set(true);
    button.set_sensitive(false);
    let (sender, receiver) =
        std::sync::mpsc::channel::<Result<String, crate::cloud::upload::UploadError>>();
    std::thread::spawn(move || {
        let result =
            crate::cloud::upload::upload_file_with_notifications_without_clipboard(&config, &path)
                .map(|result| result.share_url);
        if let Some(directory) = cleanup {
            let _ = std::fs::remove_dir_all(directory);
        }
        let _ = sender.send(result);
    });
    poll_upload(window, button, uploading, receiver);
}

fn poll_upload(
    window: &ApplicationWindow,
    button: &gtk4::Button,
    uploading: &Rc<Cell<bool>>,
    receiver: std::sync::mpsc::Receiver<Result<String, crate::cloud::upload::UploadError>>,
) {
    let window = window.downgrade();
    let button = button.downgrade();
    let uploading = uploading.clone();
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match receiver.try_recv() {
            Ok(result) => {
                uploading.set(false);
                if let Some(button) = button.upgrade() {
                    button.set_sensitive(true);
                }
                match result {
                    Ok(share_url) => {
                        if let Some(display) = gdk::Display::default() {
                            display.clipboard().set_text(&share_url);
                        }
                    }
                    Err(crate::cloud::upload::UploadError::UpgradeRequired(message)) => {
                        if let Some(window) = window.upgrade() {
                            crate::cloud::upgrade::show_prompt(&window, &message);
                        }
                    }
                    // The upload helper already reported the failure.
                    Err(_) => {}
                }
                gtk4::glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => gtk4::glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                uploading.set(false);
                if let Some(button) = button.upgrade() {
                    button.set_sensitive(true);
                }
                gtk4::glib::ControlFlow::Break
            }
        }
    });
}

/// Upload an already rendered frame. Static output ends here so the temporary
/// file, widget busy state and share-URL clipboard behave the same.
fn upload_rendered_image(
    window: &ApplicationWindow,
    button: &gtk4::Button,
    uploading: &Rc<Cell<bool>>,
    config: crate::config::AppConfig,
    snapshot: image::RgbaImage,
) {
    uploading.set(true);
    button.set_sensitive(false);
    let directory = upload_temp_dir();
    let (sender, receiver) =
        std::sync::mpsc::channel::<Result<String, crate::cloud::upload::UploadError>>();
    std::thread::spawn(move || {
        let result = std::fs::create_dir_all(&directory)
            .map_err(|error| {
                crate::cloud::upload::UploadError::Server(format!("Could not save edits: {error}"))
            })
            .and_then(|()| {
                let path = directory.join("upload.png");
                crate::capture::editor::io_ops::save_edited_image_snapshot(&path, snapshot)
                    .map_err(|error| {
                        crate::cloud::upload::UploadError::Server(format!(
                            "Could not save edits: {error}"
                        ))
                    })?;
                crate::cloud::upload::upload_file_with_notifications_without_clipboard(
                    &config, &path,
                )
                .map(|result| result.share_url)
            });
        let _ = std::fs::remove_dir_all(&directory);
        let _ = sender.send(result);
    });
    poll_upload(window, button, uploading, receiver);
}

fn install_keys(
    window: &ApplicationWindow,
    canvas: &Rc<canvas::Canvas>,
    sub_bars: &Rc<super::sub_bar::SubBars>,
    workspace: &gtk4::Stack,
    mode_confirm: &Rc<mode_confirm::ModeConfirm>,
    motion: &Rc<motion::MotionMode>,
    copy_button: &gtk4::Button,
    exporting: &Rc<Cell<bool>>,
    export_name: &str,
    sync_all: &Rc<dyn Fn()>,
) {
    // Space held = pan mode. Capture phase so it wins over the canvas, but a
    // focused text entry still gets its literal space character.
    let space = gtk4::EventControllerKey::new();
    space.set_propagation_phase(gtk4::PropagationPhase::Capture);
    space.connect_key_pressed({
        let canvas = canvas.clone();
        let window = window.clone();
        let sub_bars = sub_bars.clone();
        let workspace = workspace.clone();
        let mode_confirm = mode_confirm.clone();
        move |controller, key, _, _| {
            if mode_confirm.visible() {
                if key == gdk::Key::Escape {
                    mode_confirm.cancel();
                    return Propagation::Stop;
                }
                // Tab, Enter and Space belong to the card's buttons, so every
                // other key must reach them instead of stopping here.
                return Propagation::Proceed;
            }
            if workspace.visible_child_name().as_deref() == Some("motion") {
                return Propagation::Proceed;
            }
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
        let workspace = workspace.clone();
        let mode_confirm = mode_confirm.clone();
        let motion = motion.clone();
        let copy_button = copy_button.clone();
        let exporting = exporting.clone();
        let export_name = export_name.to_string();
        let sync_all = sync_all.clone();
        move |controller, key, _, modifiers| {
            if mode_confirm.visible() {
                if key == gdk::Key::Escape {
                    mode_confirm.cancel();
                }
                return Propagation::Stop;
            }
            if workspace.visible_child_name().as_deref() == Some("motion") {
                let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
                let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
                // A focused text field keeps its own undo, redo and copy keys
                // while Motion is open.
                let text_focused =
                    gtk4::prelude::RootExt::focus(&window).is_some_and(|w| {
                        w.is::<gtk4::Text>() || w.is::<gtk4::Entry>() || w.is::<gtk4::TextView>()
                    });
                if text_focused
                    && ctrl
                    && matches!(
                        key,
                        gdk::Key::z
                            | gdk::Key::Z
                            | gdk::Key::y
                            | gdk::Key::Y
                            | gdk::Key::c
                            | gdk::Key::C
                    )
                {
                    return Propagation::Proceed;
                }
                return match key {
                    gdk::Key::Escape => {
                        window.close();
                        Propagation::Stop
                    }
                    gdk::Key::s | gdk::Key::S if ctrl => {
                        let sync_all: Rc<dyn Fn()> = sync_all.clone();
                        save_motion_video_dialog(
                            &window,
                            &canvas.host,
                            &motion,
                            &exporting,
                            &export_name,
                            sync_all,
                        );
                        Propagation::Stop
                    }
                    gdk::Key::c | gdk::Key::C if ctrl => {
                        // The Copy button already runs Motion's Studio gate
                        // and frame render; the shortcut takes that path.
                        copy_button.emit_clicked();
                        Propagation::Stop
                    }
                    gdk::Key::z | gdk::Key::Z if ctrl && shift => {
                        motion.timeline.redo();
                        Propagation::Stop
                    }
                    gdk::Key::z | gdk::Key::Z if ctrl => {
                        motion.timeline.undo();
                        Propagation::Stop
                    }
                    gdk::Key::y | gdk::Key::Y if ctrl => {
                        motion.timeline.redo();
                        Propagation::Stop
                    }
                    gdk::Key::w | gdk::Key::W if ctrl => {
                        window.close();
                        Propagation::Stop
                    }
                    _ => Propagation::Proceed,
                };
            }
            let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
            // A focused editable owns its keys: entries keep characters and
            // their own shortcuts instead of triggering editor commands.
            if gtk4::prelude::RootExt::focus(&window)
                .is_some_and(|w| w.is::<gtk4::Editable>() || w.is::<gtk4::TextView>())
            {
                return Propagation::Proceed;
            }
            // The input method composes and commits text for the live text
            // box; keys it keeps never reach the editor commands.
            if controller
                .current_event()
                .is_some_and(|event| canvas.host.filter_text_input_key(&event))
            {
                return Propagation::Stop;
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
                    copy_image(&window, &canvas.host);
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
    keys.connect_key_released({
        let canvas = canvas.clone();
        let window = window.clone();
        move |controller, _, _, _| {
            if gtk4::prelude::RootExt::focus(&window)
                .is_some_and(|w| w.is::<gtk4::Editable>() || w.is::<gtk4::TextView>())
            {
                return;
            }
            if let Some(event) = controller.current_event() {
                canvas.host.filter_text_input_key(&event);
            }
        }
    });
    window.add_controller(keys);
}
