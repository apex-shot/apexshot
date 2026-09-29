#[allow(dead_code)]
mod crop_dialog;
pub(crate) mod custom_wallpaper_popover;
#[allow(dead_code)]
mod dialogs;
#[allow(dead_code)]
mod footer;
#[allow(dead_code)]
mod inspector;
#[allow(dead_code)]
mod media_library;
#[allow(dead_code)]
mod preview;
#[allow(dead_code)]
mod rail;
mod timeline_card;
mod tool_section;
pub(crate) mod tool_sidebar;
mod toolbar;

use super::ffmpeg;
use super::model::{EditorTool, VideoEditState, VideoMetadata, DEFAULT_FRAME_RATE};
use super::project::{self, persist_video_session};
use super::ui_support::install_recording_editor_css;
use gtk4::{
    gdk, gio, glib, prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button,
    DropTarget, FileChooserAction, FileChooserNative, FileFilter, GestureClick, Label, MediaFile,
    Orientation, Overlay, Popover, ResponseType, Widget,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::i18n::t;

pub fn open_empty() -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let initial = InitialVideo::None;
    app.connect_activate(move |application| {
        install_recording_editor_icons();
        crate::capture::editor::ui_support::install_editor_css();
        install_recording_editor_css();
        build_window(application, initial.clone());
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn install_recording_editor_icons() {
    use crate::capture::editor::window::icon_names;
    use std::sync::Once;
    static INIT_ICONS: Once = Once::new();
    INIT_ICONS.call_once(|| {
        relm4_icons::initialize_icons(icon_names::GRESOURCE_BYTES, icon_names::RESOURCE_PREFIX);
    });
}

/// Open the editor with an empty window and load the video asynchronously,
/// showing a loading spinner while ffprobe + thumbnail generation run in
/// the background. This avoids a long frozen gap before the window appears
/// for large recordings.
pub fn open_with_path(path: PathBuf) -> anyhow::Result<()> {
    let thumbnail_dir = ffmpeg::thumbnail_cache_dir(&path);

    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let thumbnail_dir_for_cleanup = thumbnail_dir.clone();
    app.connect_shutdown(move |_| {
        let _ = std::fs::remove_dir_all(&thumbnail_dir_for_cleanup);
    });

    let initial = InitialVideo::AsyncLoad(path);
    app.connect_activate(move |application| {
        install_recording_editor_icons();
        crate::capture::editor::ui_support::install_editor_css();
        install_recording_editor_css();
        build_window(application, initial.clone());
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

#[derive(Clone)]
enum InitialVideo {
    None,
    AsyncLoad(PathBuf),
}

fn build_window(application: &Application, initial_video: InitialVideo) {
    let window = ApplicationWindow::builder()
        .application(application)
        .title(t("ApexShot Recording Editor"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(1400)
        .default_height(900)
        .decorated(false)
        .build();
    window.set_size_request(1400, 900);
    window.add_css_class("editor-window");

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class("recording-editor-root");
    let prefers_dark = crate::capture::editor::ui_support::prefers_dark_glass_theme();
    if prefers_dark {
        root.add_css_class("editor-theme-dark");
    } else {
        root.add_css_class("editor-theme-light");
    }
    if crate::capture::editor::ui_support::prefers_reduced_transparency() {
        root.add_css_class("editor-reduced-transparency");
    }

    let state = match &initial_video {
        InitialVideo::AsyncLoad(path) => match ffmpeg::probe_metadata(path) {
            Ok(metadata) => {
                let mut state = VideoEditState::new(metadata);
                project::restore_into(&mut state);
                if state.zoom_clips.is_empty() && state.suggest_zoom_clips() > 0 {
                    state.selected_tool = EditorTool::Timeline;
                }
                Some(Arc::new(Mutex::new(state)))
            }
            Err(err) => {
                eprintln!(
                    "[recording-editor] failed to probe {}: {err}",
                    path.display()
                );
                None
            }
        },
        InitialVideo::None => None,
    };
    let state = state.unwrap_or_else(|| Arc::new(Mutex::new(placeholder_edit_state())));
    let filmstrip: Rc<RefCell<Vec<gtk4::gdk_pixbuf::Pixbuf>>> = Rc::new(RefCell::new(Vec::new()));
    let media = Rc::new(RefCell::new(match &initial_video {
        InitialVideo::AsyncLoad(path) => Some(MediaFile::for_filename(path)),
        InitialVideo::None => Some(MediaFile::new()),
    }));
    let exporting = Rc::new(Cell::new(false));
    let (title_bar, title_label, upload_btn, export_btn) =
        build_window_controls(&window, state.clone(), exporting.clone());
    root.append(&title_bar);

    let paint_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let refresh_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let ping = {
        let paint_slot = paint_slot.clone();
        let refresh_slot = refresh_slot.clone();
        let state = state.clone();
        let title_label = title_label.clone();
        let upload_btn = upload_btn.clone();
        let export_btn = export_btn.clone();
        let exporting = exporting.clone();
        Rc::new(move || {
            let (name, has_video) = {
                let state = state.lock().unwrap();
                (state.title.clone(), state.has_source_video())
            };
            title_label.set_text(&name);
            title_label.set_tooltip_text(Some(&name));
            let enabled = has_video && !exporting.get();
            upload_btn.set_sensitive(enabled);
            export_btn.set_sensitive(enabled);
            if let Some(paint) = paint_slot.borrow().clone() {
                paint();
            }
            if let Some(refresh) = refresh_slot.borrow().clone() {
                refresh();
            }
        }) as Rc<dyn Fn()>
    };

    let workspace = GtkBox::new(Orientation::Horizontal, 0);
    workspace.add_css_class("recording-editor-workspace");
    workspace.set_hexpand(true);
    workspace.set_vexpand(true);

    let stage = GtkBox::new(Orientation::Vertical, 0);
    stage.add_css_class("recording-editor-stage");
    stage.set_hexpand(true);
    stage.set_vexpand(true);
    let estimate_label = Label::new(None);
    estimate_label.add_css_class("recording-editor-estimate");
    let preview_media = media.borrow().clone();
    let (preview_widget, _, _) =
        preview::build_preview_with_media(state.clone(), estimate_label.clone(), preview_media);
    stage.append(&preview_widget);
    stage.append(&preview::build_stage_tools(
        &window,
        state.clone(),
        estimate_label,
        ping.clone(),
    ));
    workspace.append(&stage);

    // Tools sit on the right: a compact icon bar picks the tool, and the
    // selected tool's controls stack directly beneath it.
    let tools = tool_section::build_tool_section(state.clone(), ping.clone());

    let pause_playback_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let sidebar_pause_playback = {
        let pause_playback_slot = pause_playback_slot.clone();
        Rc::new(move || {
            if let Some(pause_playback) = pause_playback_slot.borrow().as_ref() {
                pause_playback();
            }
        }) as Rc<dyn Fn()>
    };
    let sidebar =
        tool_sidebar::build_tool_sidebar(state.clone(), ping.clone(), sidebar_pause_playback);

    let tools_column = GtkBox::new(Orientation::Vertical, 0);
    tools_column.add_css_class("recording-editor-tools-column");
    tools_column.set_hexpand(false);
    tools_column.set_vexpand(true);
    tools_column.append(&tools.widget);
    tools_column.append(&sidebar.widget);
    workspace.append(&tools_column);
    root.append(&workspace);

    let (timeline, paint, pause_playback) = timeline_card::build_timeline_card(
        state.clone(),
        media.clone(),
        filmstrip.clone(),
        ping.clone(),
    );
    root.append(&timeline);
    *paint_slot.borrow_mut() = Some(paint.clone());
    if state.lock().unwrap().has_source_video() {
        let metadata = state.lock().unwrap().metadata.clone();
        spawn_filmstrip_job(metadata, filmstrip.clone(), state.clone(), paint);
    }
    *pause_playback_slot.borrow_mut() = Some(pause_playback);
    *refresh_slot.borrow_mut() = Some({
        let refresh_tools = tools.refresh;
        let refresh_sidebar = sidebar.refresh;
        Rc::new(move || {
            refresh_tools();
            refresh_sidebar();
        }) as Rc<dyn Fn()>
    });
    let drop_target = DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
    drop_target.connect_drop({
        let state = state.clone();
        let media = media.clone();
        let filmstrip = filmstrip.clone();
        let window = window.clone();
        let ping = ping.clone();
        move |_, value, _, _| {
            let Ok(file) = value.get::<gio::File>() else {
                return false;
            };
            let Some(path) = file.path() else {
                return false;
            };
            load_preview_video(path, &state, &media, &filmstrip, &window, &ping)
        }
    });
    preview_widget.add_controller(drop_target);

    let open_click = GestureClick::new();
    open_click.set_button(1);
    open_click.connect_released({
        let state = state.clone();
        let media = media.clone();
        let filmstrip = filmstrip.clone();
        let window = window.clone();
        let ping = ping.clone();
        move |_, _, _, _| {
            if state.lock().unwrap().metadata.duration_seconds > 0.0 {
                return;
            }
            show_open_preview_video(
                &window,
                state.clone(),
                media.clone(),
                filmstrip.clone(),
                ping.clone(),
            );
        }
    });
    preview_widget.add_controller(open_click);
    ping();

    let shell = Overlay::new();
    shell.add_css_class("recording-editor-shell");
    shell.set_hexpand(true);
    shell.set_vexpand(true);
    if root.has_css_class("editor-theme-light") {
        shell.add_css_class("editor-theme-light");
    }
    let scrim = GtkBox::new(Orientation::Vertical, 0);
    scrim.add_css_class("recording-editor-modal-scrim");
    scrim.set_halign(Align::Fill);
    scrim.set_valign(Align::Fill);
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_visible(false);
    scrim.set_can_target(true);
    shell.set_child(Some(&root));
    shell.add_overlay(&scrim);
    shell.set_clip_overlay(&scrim, true);
    crate::capture::editor::ui_support::install_edge_resize(&shell, &window);
    window.set_child(Some(&shell));
    wire_close_persist(&window, state.clone(), exporting.clone());
    sweep_popovers_on_deactivate(&window);
    window.present();
    crate::update_ui::present_if_needed(&shell);
}

/// Pop down `window`'s popovers when it stops being the active window.
///
/// A popover must not outlive its window's activation. GTK leaves that to the
/// app, and an autohiding popover holds a grab for as long as it is up: leave
/// the Custom Wallpaper popover open while another window takes focus — a
/// screenshot overlay, say — and that grab is still live while the overlay
/// tries to take the pointer. See `popdown_popovers`.
///
/// The decision is deferred to the main loop's idle, because `is-active` does
/// not mean what it says at the moment it notifies. Mapping an autohiding
/// popover takes its grab, and GTK blips the toplevel inactive and back while
/// that grab lands — measured on the Edit pill's click: `is-active` ran false,
/// true, false, true inside a single dispatch, ~100µs apart. Sweeping on the
/// blip took the popover down in the same breath as it opened, so clicking
/// Edit looked like a dead button. Re-checking at idle reads the settled
/// value instead: the blip is long over, while a real deactivation — the
/// capture overlay taking focus — is not.
pub(crate) fn sweep_popovers_on_deactivate(window: &(impl IsA<gtk4::Window> + IsA<Widget>)) {
    window.connect_is_active_notify(|window| {
        if window.is_active() {
            return;
        }
        let window = window.clone();
        glib::idle_add_local_once(move || {
            if !window.is_active() {
                popdown_popovers(&window);
            }
        });
    });
}

/// Pop down every popover in `root`'s tree that is still up.
///
/// Popovers are widgets in the tree, so a plain walk finds them, nested ones
/// included. This runs when the editor window's loss of activation has
/// settled: an autohiding popover holds a grab until it is dismissed, and
/// dismissing it is what GTK expects the app to do on focus-out (`GtkWindow`
/// does not do it for you). Leaving one up while a capture overlay takes focus
/// is how a popover ends up fighting the overlay for the pointer — on Wayland
/// that lands as a protocol error, and a protocol error takes the whole
/// connection down with it, this window included.
fn popdown_popovers(root: &impl IsA<Widget>) {
    let mut stack = vec![root.clone().upcast::<Widget>()];
    while let Some(widget) = stack.pop() {
        if let Ok(popover) = widget.clone().downcast::<Popover>() {
            if popover.is_visible() {
                popover.popdown();
            }
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            stack.push(current.clone());
            child = current.next_sibling();
        }
    }
}

fn wire_close_persist(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    exporting: Rc<Cell<bool>>,
) {
    let force_close = Rc::new(Cell::new(false));
    window.connect_close_request(move |window| {
        if force_close.get() {
            persist_video_session(&state.lock().unwrap());
            return glib::Propagation::Proceed;
        }
        if !state.lock().unwrap().has_source_video() {
            return glib::Propagation::Proceed;
        }
        if exporting.get() {
            let window = window.clone();
            let state = state.clone();
            let force_close = force_close.clone();
            dialogs::show_export_in_progress_close(&window, {
                let window = window.clone();
                move || {
                    persist_video_session(&state.lock().unwrap());
                    force_close.set(true);
                    window.close();
                }
            });
            return glib::Propagation::Stop;
        }
        persist_video_session(&state.lock().unwrap());
        glib::Propagation::Proceed
    });
}

fn load_preview_video(
    path: PathBuf,
    state: &Arc<Mutex<VideoEditState>>,
    media: &Rc<RefCell<Option<MediaFile>>>,
    filmstrip: &Rc<RefCell<Vec<gtk4::gdk_pixbuf::Pixbuf>>>,
    window: &ApplicationWindow,
    ping: &Rc<dyn Fn()>,
) -> bool {
    let Ok(metadata) = ffmpeg::probe_metadata(&path) else {
        return false;
    };
    let mut next = VideoEditState::new(metadata.clone());
    project::restore_into(&mut next);
    *state.lock().unwrap() = next;
    filmstrip.borrow_mut().clear();
    spawn_filmstrip_job(metadata, filmstrip.clone(), state.clone(), ping.clone());
    let has_mouse_data = state.lock().unwrap().supports_auto_zoom();
    if let Some(player) = media.borrow().as_ref() {
        player.set_file(Some(&gio::File::for_path(&path)));
    }
    ping();
    if !has_mouse_data {
        dialogs::show_manual_zoom_notice(window);
    }
    true
}

/// Decodes the timeline filmstrip for `metadata` on a worker thread, then
/// swaps the frames into `filmstrip` and repaints. A stale job (a newer video
/// was loaded meanwhile) is discarded. On failure the slot keeps whatever it
/// held — usually empty — and the clip still draws its orange fill.
///
/// Pixbufs cross into the UI on the main thread: `Pixbuf` is neither `Send`
/// nor `Sync`, and the cache dir is keyed per source path, so a superseded
/// job's files are either still equivalent tiles or gone — `from_file` failure
/// just drops that tile.
fn spawn_filmstrip_job(
    metadata: VideoMetadata,
    filmstrip: Rc<RefCell<Vec<gtk4::gdk_pixbuf::Pixbuf>>>,
    state: Arc<Mutex<VideoEditState>>,
    ping: Rc<dyn Fn()>,
) {
    let source = metadata.path.clone();
    let (sender, receiver) = mpsc::channel::<Vec<PathBuf>>();
    std::thread::spawn(move || {
        let thumbnails = ffmpeg::generate_thumbnails(&metadata).unwrap_or_default();
        let _ = sender.send(thumbnails);
    });
    glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(thumbnails) => {
                if state.lock().unwrap().metadata.path == source {
                    let frames = thumbnails
                        .iter()
                        .filter_map(|thumb| gtk4::gdk_pixbuf::Pixbuf::from_file(thumb).ok())
                        .collect();
                    *filmstrip.borrow_mut() = frames;
                    ping();
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

fn show_open_preview_video(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    media: Rc<RefCell<Option<MediaFile>>>,
    filmstrip: Rc<RefCell<Vec<gtk4::gdk_pixbuf::Pixbuf>>>,
    ping: Rc<dyn Fn()>,
) {
    let title = t("Open video");
    let open = t("Open");
    let cancel = t("Cancel");
    let chooser = FileChooserNative::new(
        Some(&title),
        Some(window),
        FileChooserAction::Open,
        Some(&open),
        Some(&cancel),
    );
    let filter = FileFilter::new();
    filter.set_name(Some(&t("Videos")));
    filter.add_mime_type("video/mp4");
    filter.add_pattern("*.mp4");
    chooser.add_filter(&filter);
    let window = window.clone();
    chooser.connect_response(move |dialog, response| {
        if response == ResponseType::Accept {
            if let Some(path) = dialog.file().and_then(|file| file.path()) {
                load_preview_video(path, &state, &media, &filmstrip, &window, &ping);
            }
        }
        dialog.hide();
    });
    chooser.show();
}

fn placeholder_edit_state() -> VideoEditState {
    let mut state = VideoEditState::new(VideoMetadata {
        path: PathBuf::from("Screen Recording.mp4"),
        duration_seconds: 0.0,
        width: 1920,
        height: 1080,
        file_size_bytes: 0,
        has_audio: false,
        frame_rate: DEFAULT_FRAME_RATE,
    });
    state.title = t("Drop a recording to begin");
    state
}

fn build_window_controls(
    window: &ApplicationWindow,
    state: Arc<Mutex<VideoEditState>>,
    exporting: Rc<Cell<bool>>,
) -> (GtkBox, Label, Button, Button) {
    const TRAFFIC_LIGHTS_WIDTH: i32 = 100;
    let bar = GtkBox::new(Orientation::Horizontal, 8);
    bar.add_css_class("recording-editor-window-controls");
    bar.set_hexpand(true);
    bar.set_vexpand(false);
    bar.set_valign(Align::Start);

    let left_balance = GtkBox::new(Orientation::Horizontal, 0);
    left_balance.set_size_request(TRAFFIC_LIGHTS_WIDTH, -1);
    bar.append(&left_balance);

    let title_text = state.lock().unwrap().title.clone();
    let title = Label::new(Some(&title_text));
    title.add_css_class("recording-editor-title");
    title.set_hexpand(true);
    title.set_halign(Align::Center);
    title.set_valign(Align::Center);
    title.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title.set_max_width_chars(64);
    title.set_can_target(false);
    bar.append(&title);

    let (export, export_spinner) =
        footer::build_export_action(window, state.clone(), exporting.clone());
    let (upload, upload_spinner) = footer::build_upload_action(state, exporting);
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.add_css_class("recording-editor-title-actions");
    actions.set_valign(Align::Center);
    actions.append(&upload_spinner);
    actions.append(&upload);
    actions.append(&export_spinner);
    actions.append(&export);
    let lights = toolbar::build_traffic_lights(window);
    lights.set_size_request(TRAFFIC_LIGHTS_WIDTH, -1);
    lights.set_halign(Align::End);
    lights.set_valign(Align::Center);
    // The tools column is as wide as the actions and traffic lights beside
    // it, so the title stays optically centred across the window.
    let right_balance = GtkBox::new(Orientation::Horizontal, 16);
    right_balance.set_size_request(tool_sidebar::TOOL_SIDEBAR_WIDTH + TRAFFIC_LIGHTS_WIDTH, -1);
    right_balance.set_hexpand(false);
    let right_spacer = GtkBox::new(Orientation::Horizontal, 0);
    right_spacer.set_hexpand(true);
    right_balance.append(&right_spacer);
    right_balance.append(&actions);
    right_balance.append(&lights);
    bar.append(&right_balance);

    crate::capture::editor::ui_support::install_window_drag(&bar, window);
    (bar, title, upload, export)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_window_takes_its_popovers_down_when_it_deactivates() {
        // A popover that stays up past its window's activation keeps a live
        // pointer grab, which is what let an open Custom Wallpaper popover fight
        // a capture overlay for the pointer — and a Wayland protocol error there
        // kills the whole connection, this window included. Both the wiring and
        // the walk (it has to find nested popovers too) are pinned here.
        let source = include_str!("mod.rs");
        let production = &source[..source.find("\n#[cfg(test)]").expect("tests module")];
        assert!(
            production.contains("connect_is_active_notify"),
            "the editor must react to its own activation"
        );
        assert!(
            production.contains("!window.is_active()"),
            "popovers must come down when the window loses activation"
        );
        assert!(
            production.contains("popdown_popovers(&window)"),
            "the window must sweep its popovers on focus-out"
        );
        assert!(
            production.contains("idle_add_local_once"),
            "the sweep must read settled activation: mapping a popover blips is-active off and back"
        );

        let Some(count) = crate::test_support::with_gtk(|| {
            use gtk4::prelude::*;
            use gtk4::{glib as gtk_glib, Box as GtkBox, Orientation, Popover};

            let window = gtk4::Window::new();
            let root = GtkBox::new(Orientation::Vertical, 0);
            window.set_child(Some(&root));

            let outer = Popover::new();
            outer.set_has_arrow(false);
            outer.set_parent(&root);
            let body = GtkBox::new(Orientation::Vertical, 0);
            outer.set_child(Some(&body));

            let nested = Popover::new();
            nested.set_has_arrow(false);
            nested.set_parent(&body);
            let nested_body = GtkBox::new(Orientation::Vertical, 0);
            nested.set_child(Some(&nested_body));

            // Surfaces are positioned by the compositor, so the loop has to run
            // in real time before visibility means anything.
            window.present();
            let pump = || {
                let ctx = gtk_glib::MainContext::default();
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(150);
                while std::time::Instant::now() < deadline {
                    while ctx.iteration(false) {}
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            };
            pump();
            outer.popup();
            nested.popup();
            pump();
            let up = usize::from(outer.is_visible()) + usize::from(nested.is_visible());
            super::popdown_popovers(&window);
            pump();
            let down = usize::from(outer.is_visible()) + usize::from(nested.is_visible());

            nested.unparent();
            outer.unparent();
            window.destroy();
            (up, down)
        }) else {
            eprintln!("skipping: no display available");
            return;
        };

        assert_eq!(count.0, 2, "both popovers must be up before the sweep");
        assert_eq!(count.1, 0, "the sweep must take nested popovers down too");
    }

    #[test]
    fn clicking_edit_opens_the_custom_wallpaper_popover() {
        // The regression this pins: the popover's own opener was fine, but the
        // deactivation sweep ate the card in the same breath it opened. Mapping
        // an autohiding popover takes its grab, and GTK blips the toplevel's
        // `is-active` off and back while the grab lands — measured on this very
        // click: false, true, false, true inside one dispatch, ~100µs apart.
        // The sweep read that blip as "the window lost activation", popped the
        // card straight back down, and from the Background panel the Edit pill
        // looked like a dead button. Built through the real opener and the real
        // sweep wiring, so the settle the wiring depends on is in play: the card
        // must still be up once the blip has passed.
        use crate::recording::editor::model::{VideoEditState, VideoMetadata};
        use crate::recording::editor::window::custom_wallpaper_popover::{
            build_custom_fill_popover, FillOps,
        };
        use gtk4::prelude::*;
        use gtk4::{glib as gtk_glib, Box as GtkBox, Button, Orientation};
        use std::path::PathBuf;
        use std::rc::Rc;
        use std::sync::{Arc, Mutex};

        let Some(opened) = crate::test_support::with_gtk(|| {
            let state = Arc::new(Mutex::new(VideoEditState::new(VideoMetadata {
                path: PathBuf::from("/tmp/input.mp4"),
                duration_seconds: 10.0,
                width: 1920,
                height: 1080,
                file_size_bytes: 1024,
                has_audio: false,
                frame_rate: 30.0,
            })));
            let window = gtk4::Window::new();
            window.set_default_size(1200, 800);
            let sidebar = GtkBox::new(Orientation::Vertical, 0);
            window.set_child(Some(&sidebar));
            let edit = Button::with_label("Edit");
            sidebar.append(&edit);
            let popover = build_custom_fill_popover(
                &sidebar,
                &edit,
                "Custom Wallpaper",
                FillOps::for_video(state),
                Rc::new(|| {}),
            );
            super::sweep_popovers_on_deactivate(&window);

            // Surfaces are positioned by the compositor, so the loop has to run
            // in real time before visibility means anything.
            window.present();
            let pump = || {
                let ctx = gtk_glib::MainContext::default();
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
                while std::time::Instant::now() < deadline {
                    while ctx.iteration(false) {}
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            };
            pump();
            edit.emit_clicked();
            pump();
            let opened = popover.is_visible();
            popover.popdown();
            popover.unparent();
            window.destroy();
            opened
        }) else {
            eprintln!("skipping: no display available");
            return;
        };

        assert!(
            opened,
            "clicking Edit must open the Custom Wallpaper popover and leave it up"
        );
    }

    #[test]
    fn close_persists_project_and_does_not_export() {
        let source = include_str!("mod.rs");
        let start = source
            .find("fn wire_close_persist(")
            .expect("video close persist handler");
        let rest = &source[start + 1..];
        let end = rest
            .find("\nfn ")
            .map(|i| start + 1 + i)
            .unwrap_or(source.len());
        let handler = &source[start..end];
        assert!(
            handler.contains("persist_video_session"),
            "Video close handler must persist the project sidecar"
        );
        assert!(
            !handler.contains("export_edited_to"),
            "Video close must not encode an MP4"
        );
    }

    #[test]
    fn light_theme_playhead_is_black_and_clips_have_no_lift_shadow() {
        let painting = include_str!("timeline_card_parts/painting.rs");
        assert!(
            painting.contains("(0.07, 0.08, 0.09)"),
            "Light-theme playhead stem and capsule outline must paint black"
        );
        assert!(
            painting.contains("cr.set_source_rgba(0.80, 0.22, 0.20, 0.95)"),
            "Hover read-out must paint the Motion red hairline"
        );
        assert!(
            !painting.contains("x - 5.0, 20.0"),
            "Playhead head must be a capsule, not a triangle"
        );
        assert!(
            !painting.contains("0.0, 0.0, 0.0, 0.38"),
            "Lifted clips must not draw a drop shadow"
        );
        assert!(
            painting.contains("fn widget_is_light"),
            "Timeline painting must detect the light theme"
        );
    }

    #[test]
    fn video_lane_is_empty_before_upload_and_dark_while_frames_load() {
        let painting = include_str!("timeline_card_parts/painting.rs");
        assert!(
            painting.contains("if !state.has_source_video()"),
            "no clip may be drawn before a video is loaded"
        );
        assert!(
            painting.contains("fill: (0.10, 0.11, 0.13, 0.9)"),
            "while frames load the lane must read as an empty media strip, not orange"
        );
        let shell = include_str!("timeline_card_parts/shell.rs");
        assert!(
            shell.contains("video_track.set_size_request(-1, 56)"),
            "video track must match Motion's 56px source lane"
        );
        let css = include_str!("../ui_support_css/07.css");
        assert!(
            css.contains(".recording-editor-card-video-track {\n                min-height: 56px;"),
            "CSS floor must match the 56px video track"
        );
        assert!(
            css.contains(".editor-motion-timeline-dock .recording-editor-card-board"),
            "the Motion board must keep its own taller floor"
        );
    }

    #[test]
    fn zoom_and_hide_clips_outline_only_the_selected_clip() {
        let painting = include_str!("timeline_card_parts/painting.rs");
        assert!(
            painting.contains("edge: (f64, f64, f64, f64)"),
            "ClipTone must carry a selection outline color like the Motion tones"
        );
        assert!(
            painting.contains("let (r, g, b, a) = blue.edge;"),
            "Selected zoom clips must stroke their outline"
        );
        assert!(
            painting.contains("let (r, g, b, a) = rose.edge;"),
            "Selected hide clips must stroke their outline"
        );
        assert!(
            painting.contains("let (r, g, b, a) = tone.edge;"),
            "Selected video clips must stroke their outline"
        );
    }
}
