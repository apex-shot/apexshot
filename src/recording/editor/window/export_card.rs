use super::dialogs;
use super::export_flow;
use crate::i18n::{t, tfmt};
use crate::recording::editor::export_settings::{ExportPhase, ExportProgress};
use crate::recording::editor::ffmpeg;
use crate::recording::editor::model::{format_size, VideoEditState};
use gtk4::{
    gdk, glib, prelude::*, Align, ApplicationWindow, Box as GtkBox, Button, EventControllerKey,
    Label, Orientation, ProgressBar,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn encoding_percent(fraction: f64) -> u32 {
    (fraction * 100.0).floor().min(99.0) as u32
}

/// Integer upload progress, held below 100 until the upload reports it is done.
fn upload_percent(sent: u64, total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    ((sent as u128 * 100) / total as u128).min(99) as u32
}

fn progress_title(label: &str, percent: u32) -> String {
    format!("{label} · {percent}%")
}

fn format_elapsed(seconds: u64) -> String {
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

const STALL: Duration = Duration::from_secs(3);
const MIN_WINDOW: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Eta {
    start: Option<(Instant, f64)>,
    last: Option<(Instant, f64)>,
}

impl Eta {
    fn reset(&mut self) {
        self.start = None;
        self.last = None;
    }

    fn sample(&mut self, at: Instant, progress: f64) {
        if !progress.is_finite() || progress <= 0.0 {
            return;
        }
        match self.last {
            Some((_, previous)) if progress < previous - 1e-9 => {
                self.start = Some((at, progress));
                self.last = Some((at, progress));
            }
            Some((when, previous)) if progress > previous + 1e-9 => {
                if at.saturating_duration_since(when) >= STALL || self.start.is_none() {
                    self.start = Some((at, progress));
                }
                self.last = Some((at, progress));
            }
            Some(_) => {}
            None => {
                self.start = Some((at, progress));
                self.last = Some((at, progress));
            }
        }
    }

    fn remaining(&self, now: Instant) -> Option<f64> {
        let (start_at, start_progress) = self.start?;
        let (last_at, last_progress) = self.last?;
        if now.saturating_duration_since(last_at) >= STALL {
            return None;
        }
        let window = last_at.saturating_duration_since(start_at);
        if window < MIN_WINDOW || last_progress <= start_progress {
            return None;
        }
        let per_unit = window.as_secs_f64() / (last_progress - start_progress);
        let since = now.saturating_duration_since(last_at).as_secs_f64();
        Some(((1.0 - last_progress) * per_unit - since).max(0.0))
    }
}

fn clock(seconds: f64) -> String {
    let seconds = seconds.round().max(0.0) as u64;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            (seconds % 3600) / 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UploadPhase {
    Ready,
    Uploading,
    Complete,
    Failed,
}

enum UploadEvent {
    Progress {
        sent: u64,
        total: u64,
    },
    Done {
        result: Result<String, crate::cloud::upload::UploadError>,
        heads_up: Option<crate::cloud::limit_prompt::UploadHeadsUp>,
    },
}

/// The export screen widgets the upload flow rewrites, cloned for its poll.
#[derive(Clone)]
struct UploadUi {
    path: PathBuf,
    destination: Label,
    title: Label,
    bar: ProgressBar,
    timing: Label,
    detail: Label,
    actions: GtkBox,
    extra: GtkBox,
    upload: Button,
    reveal: Button,
    open_link: Button,
    complete_detail: Rc<RefCell<String>>,
    share_url: Rc<RefCell<Option<String>>>,
    editor_window: glib::WeakRef<ApplicationWindow>,
}

impl UploadUi {
    fn window(&self) -> Option<gtk4::Window> {
        self.upload
            .root()
            .and_then(|root| root.downcast::<gtk4::Window>().ok())
            .or_else(|| {
                self.editor_window
                    .upgrade()
                    .map(|window| window.upcast::<gtk4::Window>())
            })
    }

    fn window_is_active(&self) -> bool {
        self.window().is_some_and(|window| window.is_active())
    }

    fn path_text(&self) -> String {
        self.path.to_string_lossy().to_string()
    }

    fn show_progress(&self, sent: u64, total: u64) {
        let fraction = if total == 0 {
            0.0
        } else {
            (sent as f64 / total as f64).clamp(0.0, 1.0)
        };
        self.bar.set_fraction(fraction);
        self.title.set_text(&progress_title(
            &t("Uploading video"),
            upload_percent(sent, total),
        ));
        self.timing.set_text(&tfmt(
            "{sent} of {total}",
            &[("sent", &format_size(sent)), ("total", &format_size(total))],
        ));
    }

    fn show_uploading(&self, phase: &Rc<Cell<UploadPhase>>) {
        phase.set(UploadPhase::Uploading);
        self.upload.set_sensitive(false);
        self.actions.set_visible(false);
        self.extra.set_visible(false);
        self.detail.set_visible(false);
        self.detail
            .remove_css_class("recording-editor-export-error");
        self.bar.set_visible(true);
        self.timing.set_visible(true);
        let total = std::fs::metadata(&self.path)
            .map(|meta| meta.len())
            .unwrap_or(0);
        self.show_progress(0, total);
    }

    fn show_complete(
        &self,
        phase: &Rc<Cell<UploadPhase>>,
        url: String,
        heads_up: Option<&crate::cloud::limit_prompt::UploadHeadsUp>,
    ) {
        phase.set(UploadPhase::Complete);
        *self.share_url.borrow_mut() = Some(url.clone());
        self.title.set_text(&t("Upload complete"));
        self.bar.set_visible(false);
        self.timing.set_visible(false);
        self.destination.set_text(&url);
        self.destination.set_tooltip_text(Some(&url));
        self.detail
            .remove_css_class("recording-editor-export-error");
        self.detail.set_visible(true);

        let copied_now = self.window().is_some_and(|window| {
            let detail = self.detail.clone();
            crate::utils::clipboard::copy_text_when_window_active_then(&window, &url, move || {
                detail.set_text(&t("Link copied to clipboard"));
            })
        });
        let mut detail_text = if copied_now {
            t("Link copied to clipboard")
        } else {
            t("The link will be copied when you return to this window")
        };
        if let Some(heads_up) = heads_up {
            detail_text = heads_up.append_to(&detail_text);
        }
        self.detail.set_text(&detail_text);

        if !self.window_is_active() {
            let mut body = t("Switch back to the editor to copy the link");
            if let Some(heads_up) = heads_up {
                body = heads_up.append_to(&body);
            }
            crate::utils::notify::desktop_notification(&t("Upload complete"), &body);
        }

        self.upload.set_label(&t("Copy link"));
        self.upload.set_sensitive(true);
        self.upload.add_css_class("recording-editor-export-primary");
        self.reveal
            .remove_css_class("recording-editor-export-primary");
        self.actions.set_visible(true);
        self.extra.set_visible(true);
        self.open_link.set_visible(true);
    }

    fn show_complete_state(&self) {
        phase_neutral_layout(self);
        self.detail
            .remove_css_class("recording-editor-export-error");
        self.detail.set_text(self.complete_detail.borrow().as_str());
        self.detail.set_visible(true);
    }

    fn show_error(&self, phase: &Rc<Cell<UploadPhase>>, message: &str) {
        phase.set(UploadPhase::Failed);
        self.title.set_text(&t("Upload failed"));
        self.bar.set_visible(false);
        self.timing.set_visible(false);
        self.path_back();
        self.detail.add_css_class("recording-editor-export-error");
        self.detail.set_text(message);
        self.detail.set_visible(true);
        self.actions.set_visible(true);
        self.extra.set_visible(true);
        self.open_link.set_visible(false);
        self.upload.set_label(&t("Try again"));
        self.upload.set_sensitive(true);
        self.upload
            .remove_css_class("recording-editor-export-primary");
        self.reveal.add_css_class("recording-editor-export-primary");
    }

    fn path_back(&self) {
        let path = self.path_text();
        self.destination.set_text(&path);
        self.destination.set_tooltip_text(Some(&path));
    }

    fn copy_share_link(&self) {
        let url = self.share_url.borrow().clone();
        let Some(url) = url else {
            return;
        };
        match crate::utils::clipboard::copy_text_to_gtk_clipboard(&url) {
            Ok(()) => {
                self.detail
                    .remove_css_class("recording-editor-export-error");
                self.detail.set_text(&t("Link copied to clipboard"));
                self.detail.set_visible(true);
            }
            Err(err) => {
                self.detail.add_css_class("recording-editor-export-error");
                self.detail.set_text(&err);
                self.detail.set_visible(true);
            }
        }
    }
}

fn phase_neutral_layout(ui: &UploadUi) {
    ui.title.set_text(&t("Export complete"));
    ui.bar.set_visible(false);
    ui.timing.set_visible(false);
    ui.path_back();
    ui.actions.set_visible(true);
    ui.extra.set_visible(true);
    ui.open_link.set_visible(false);
    ui.upload.set_label(&t("Upload"));
    ui.upload.set_sensitive(true);
    ui.upload
        .remove_css_class("recording-editor-export-primary");
    ui.reveal.add_css_class("recording-editor-export-primary");
}

#[derive(Clone)]
pub(super) struct ExportScreen {
    shell: glib::WeakRef<gtk4::Overlay>,
    screen: glib::WeakRef<gtk4::CenterBox>,
    editor: glib::WeakRef<gtk4::Widget>,
    previous_opacity: f64,
    detached: Rc<Cell<bool>>,
}

impl ExportScreen {
    fn attach(window: &ApplicationWindow, screen: &gtk4::CenterBox) -> Option<Self> {
        let shell = window.child()?.downcast::<gtk4::Overlay>().ok()?;
        let editor = shell.child()?;
        let previous_opacity = editor.opacity();
        dialogs::set_editor_modal_blur(window, false);
        editor.set_opacity(0.0);
        shell.add_overlay(screen);
        Some(Self {
            shell: shell.downgrade(),
            screen: screen.downgrade(),
            editor: editor.downgrade(),
            previous_opacity,
            detached: Rc::new(Cell::new(false)),
        })
    }

    pub(super) fn dismiss(&self) -> bool {
        if self.detached.replace(true) {
            return false;
        }
        if let Some(editor) = self.editor.upgrade() {
            editor.set_opacity(self.previous_opacity);
        }
        if let (Some(shell), Some(screen)) = (self.shell.upgrade(), self.screen.upgrade()) {
            if screen
                .parent()
                .is_some_and(|parent| parent == shell.clone().upcast::<gtk4::Widget>())
            {
                shell.remove_overlay(&screen);
            }
        }
        true
    }

    fn is_attached(&self) -> bool {
        !self.detached.get()
            && self
                .screen
                .upgrade()
                .is_some_and(|screen| screen.root().is_some())
    }
}

fn hand_back_to_editor(
    screen: &ExportScreen,
    restore: &Rc<dyn Fn()>,
    flow: &export_flow::FlowSlot,
) {
    if screen.dismiss() {
        restore();
        export_flow::release(flow);
    }
}

fn report_clipboard_result(result: Result<(), String>) {
    match result {
        Ok(()) => crate::utils::notify::desktop_notification(
            &t("Export complete"),
            &t("A link to the exported file was copied"),
        ),
        Err(err) => crate::utils::notify::desktop_notification(&t("Copy failed"), &err.to_string()),
    }
}

pub(super) fn start(
    window: &ApplicationWindow,
    mut snapshot: VideoEditState,
    output_path: PathBuf,
    copy_to_clipboard: bool,
    flow: export_flow::FlowSlot,
    restore: Rc<dyn Fn()>,
    exporting: Rc<Cell<bool>>,
    export_cancellation: Arc<AtomicBool>,
) {
    exporting.set(true);
    // Captured before encoding: the finished file consumes the trial inside
    // the renderer, and this drives the completion copy.
    let was_trial = crate::studio_entitlement::studio_access(
        crate::studio_entitlement::capabilities_for_video(&snapshot),
    )
    .is_trial();

    let progress = Arc::new(ExportProgress::new(0));
    snapshot.export_progress = Some(progress.clone());

    let (sender, receiver) = std::sync::mpsc::channel::<Result<PathBuf, String>>();

    let screen = gtk4::CenterBox::new();
    screen.set_orientation(Orientation::Vertical);
    screen.add_css_class("recording-editor-export-screen");
    screen.set_hexpand(true);
    screen.set_vexpand(true);

    let content = GtkBox::new(Orientation::Vertical, 10);
    content.set_halign(Align::Center);
    content.set_valign(Align::Center);
    content.set_width_request(330);
    content.set_margin_start(30);
    content.set_margin_end(30);

    let destination = Label::new(Some(&output_path.to_string_lossy()));
    destination.add_css_class("recording-editor-export-destination");
    destination.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    destination.set_max_width_chars(52);
    destination.set_tooltip_text(Some(&output_path.to_string_lossy()));
    content.append(&destination);

    let title = Label::new(Some(&t("Preparing export…")));
    title.add_css_class("recording-editor-export-screen-title");
    content.append(&title);

    let bar = ProgressBar::new();
    bar.add_css_class("recording-editor-export-progress");
    bar.set_fraction(0.0);
    content.append(&bar);

    let timing = Label::new(Some("00:00 · —"));
    timing.add_css_class("recording-editor-export-muted");
    content.append(&timing);

    let detail = Label::new(None);
    detail.add_css_class("recording-editor-export-muted");
    detail.set_wrap(true);
    detail.set_max_width_chars(60);
    detail.set_selectable(true);
    detail.set_visible(false);
    content.append(&detail);

    let stop = Button::with_label(&t("Stop export"));
    stop.add_css_class("recording-editor-export-stop");
    stop.set_halign(Align::Center);
    stop.set_margin_top(24);
    content.append(&stop);

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::Center);
    actions.set_margin_top(18);
    actions.set_visible(false);
    let back = Button::with_label(&t("Back to editor"));
    back.add_css_class("recording-editor-export-action");
    let upload = Button::with_label(&t("Upload"));
    upload.add_css_class("recording-editor-export-action");
    upload.set_visible(false);
    let reveal = Button::with_label(&t("Show in Files"));
    reveal.add_css_class("recording-editor-export-action");
    reveal.add_css_class("recording-editor-export-primary");
    reveal.set_visible(false);
    actions.append(&back);
    actions.append(&upload);
    actions.append(&reveal);
    content.append(&actions);

    let extra = GtkBox::new(Orientation::Horizontal, 8);
    extra.set_halign(Align::Center);
    extra.set_margin_top(8);
    extra.set_visible(false);
    let open = Button::with_label(&t("Open video"));
    open.add_css_class("recording-editor-export-action");
    let copy_file_button = Button::with_label(&t("Copy exported file"));
    copy_file_button.add_css_class("recording-editor-export-action");
    let open_link = Button::with_label(&t("Open link"));
    open_link.add_css_class("recording-editor-export-action");
    open_link.set_visible(false);
    extra.append(&open);
    extra.append(&copy_file_button);
    extra.append(&open_link);
    content.append(&extra);

    let upload_phase = Rc::new(Cell::new(UploadPhase::Ready));
    let share_url = Rc::new(RefCell::new(None::<String>));
    let complete_detail = Rc::new(RefCell::new(String::new()));

    open_link.connect_clicked({
        let share_url = share_url.clone();
        move |_| {
            let url = share_url.borrow().clone();
            let Some(url) = url else {
                return;
            };
            if let Err(err) = crate::utils::open::open_url(&url) {
                crate::utils::notify::desktop_notification(&t("Open failed"), &err);
            }
        }
    });

    screen.set_center_widget(Some(&content));

    let Some(handle) = ExportScreen::attach(window, &screen) else {
        restore();
        export_flow::release(&flow);
        exporting.set(false);
        export_cancellation.store(true, Ordering::Release);
        return;
    };

    let upload_ui = UploadUi {
        path: output_path.clone(),
        destination: destination.clone(),
        title: title.clone(),
        bar: bar.clone(),
        timing: timing.clone(),
        detail: detail.clone(),
        actions: actions.clone(),
        extra: extra.clone(),
        upload: upload.clone(),
        reveal: reveal.clone(),
        open_link: open_link.clone(),
        complete_detail: complete_detail.clone(),
        share_url: share_url.clone(),
        editor_window: window.downgrade(),
    };

    upload.connect_clicked({
        let ui = upload_ui.clone();
        let phase = upload_phase.clone();
        move |button| match phase.get() {
            UploadPhase::Uploading => {}
            UploadPhase::Complete => ui.copy_share_link(),
            UploadPhase::Ready | UploadPhase::Failed => {
                if !button.is_sensitive() {
                    return;
                }
                let config = crate::config::load_config();
                if crate::cloud::sign_in::needs_apexshot_sign_in(&config) {
                    let parent = ui.window();
                    let retry = button.downgrade();
                    crate::cloud::sign_in::prompt_for_upload(parent.as_ref(), move || {
                        if let Some(button) = retry.upgrade() {
                            button.emit_clicked();
                        }
                    });
                    return;
                }
                if !crate::cloud::upload::is_configured(&config) {
                    let (title, body) = crate::cloud::upload::not_configured_notification(&config);
                    crate::utils::notify::desktop_notification(&title, &body);
                    return;
                }
                start_upload(&ui, &phase);
            }
        }
    });

    {
        let output_path = output_path.clone();
        let cancellation = export_cancellation.clone();
        std::thread::spawn(move || {
            let result = ffmpeg::export_edited_to_cancellable(
                &snapshot,
                output_path.clone(),
                Some(&cancellation),
            );
            if result.is_err() && copy_to_clipboard {
                let _ = std::fs::remove_file(&output_path);
            }
            let _ = sender.send(result.map_err(|err| err.to_string()));
        });
    }

    let terminal = Rc::new(Cell::new(false));
    let cancelling = Rc::new(Cell::new(false));

    stop.connect_clicked({
        let export_cancellation = export_cancellation.clone();
        let cancelling = cancelling.clone();
        let title = title.clone();
        move |stop| {
            export_cancellation.store(true, Ordering::Release);
            cancelling.set(true);
            stop.set_sensitive(false);
            title.set_text(&t("Stopping export…"));
        }
    });

    back.connect_clicked({
        let handle = handle.clone();
        let restore = restore.clone();
        let flow = flow.clone();
        move |_| hand_back_to_editor(&handle, &restore, &flow)
    });

    let keys = EventControllerKey::new();
    keys.connect_key_pressed({
        let terminal = terminal.clone();
        let cancelling = cancelling.clone();
        let export_cancellation = export_cancellation.clone();
        let title = title.clone();
        let stop = stop.clone();
        let handle = handle.clone();
        let restore = restore.clone();
        let flow = flow.clone();
        move |_, key, _, _| {
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            if terminal.get() {
                hand_back_to_editor(&handle, &restore, &flow);
            } else if !cancelling.replace(true) {
                export_cancellation.store(true, Ordering::Release);
                stop.set_sensitive(false);
                title.set_text(&t("Stopping export…"));
            }
            glib::Propagation::Stop
        }
    });
    screen.add_controller(keys);
    stop.grab_focus();

    screen.connect_parent_notify({
        let terminal = terminal.clone();
        let export_cancellation = export_cancellation.clone();
        let handle = handle.clone();
        let restore = restore.clone();
        let flow = flow.clone();
        move |screen| {
            if screen.parent().is_some() {
                return;
            }
            if terminal.get() {
                hand_back_to_editor(&handle, &restore, &flow);
            } else {
                export_cancellation.store(true, Ordering::Release);
            }
        }
    });

    let started = Instant::now();
    let toast_window = window.downgrade();
    glib::timeout_add_local(Duration::from_millis(120), {
        let handle = handle.clone();
        let toast_window = toast_window.clone();
        let progress = progress.clone();
        let restore = restore.clone();
        let exporting = exporting.clone();
        let export_cancellation = export_cancellation.clone();
        let flow = flow.clone();
        let terminal = terminal.clone();
        let cancelling = cancelling.clone();
        let title = title.clone();
        let timing = timing.clone();
        let detail = detail.clone();
        let bar = bar.clone();
        let stop = stop.clone();
        let actions = actions.clone();
        let extra = extra.clone();
        let back = back.clone();
        let upload = upload.clone();
        let reveal = reveal.clone();
        let open = open.clone();
        let copy_file_button = copy_file_button.clone();
        let complete_detail = complete_detail.clone();
        let mut eta = Eta::default();
        let mut last_phase = ExportPhase::Preparing;
        move || {
            if !handle.is_attached() {
                if terminal.get() {
                    restore();
                    export_flow::release(&flow);
                    return glib::ControlFlow::Break;
                }
                export_cancellation.store(true, Ordering::Release);
            }

            if !terminal.get() {
                let phase = progress.phase();
                match phase {
                    ExportPhase::Preparing => {
                        if last_phase != ExportPhase::Preparing {
                            eta.reset();
                        }
                        if !cancelling.get() {
                            bar.pulse();
                            title.set_text(&t("Preparing export…"));
                        }
                    }
                    ExportPhase::Finishing => {
                        if !cancelling.get() {
                            bar.pulse();
                            title.set_text(&t("Finishing…"));
                        }
                    }
                    ExportPhase::RetryingSoftware => {
                        if last_phase != ExportPhase::RetryingSoftware {
                            eta.reset();
                        }
                        if !cancelling.get() {
                            bar.pulse();
                            title.set_text(&t("Retrying on the software encoder…"));
                            detail.set_text(&t(
                                "Hardware encoding failed; the export restarted on the software encoder",
                            ));
                            detail.set_visible(true);
                        }
                    }
                    ExportPhase::Encoding | ExportPhase::Finished => {
                        let fraction = progress.fraction();
                        eta.sample(Instant::now(), fraction);
                        bar.set_fraction(fraction.clamp(0.0, 1.0));
                        if !cancelling.get() {
                            title.set_text(&format!(
                                "{} · {}%",
                                t("Exporting video"),
                                encoding_percent(fraction)
                            ));
                            if progress.retried() {
                                detail.set_text(&t(
                                    "Hardware encoding failed; the export restarted on the software encoder",
                                ));
                                detail.set_visible(true);
                            }
                        }
                    }
                }
                last_phase = phase;
                if !cancelling.get() {
                    let left = if progress.phase() == ExportPhase::Finished
                        || progress.fraction() >= 1.0
                    {
                        t("Finishing…")
                    } else if let Some(seconds) = eta.remaining(Instant::now()) {
                        format!("{} {}", clock(seconds), t("left"))
                    } else {
                        t("Estimating…")
                    };
                    timing.set_text(&format!(
                        "{} · {left}",
                        clock(started.elapsed().as_secs_f64())
                    ));
                }
                if export_cancellation.load(Ordering::Acquire) && !cancelling.get() {
                    cancelling.set(true);
                    stop.set_sensitive(false);
                    title.set_text(&t("Stopping export…"));
                }
            }

            match receiver.try_recv() {
                Ok(result) => {
                    terminal.set(true);
                    exporting.set(false);
                    let cancelled = export_cancellation.load(Ordering::Acquire);
                    if cancelled {
                        if let Ok(path) = &result {
                            let _ = std::fs::remove_file(path);
                        }
                        handle.dismiss();
                        restore();
                        export_flow::release(&flow);
                        crate::utils::notify::desktop_notification(
                            &t("Export stopped"),
                            &t("The partial file was removed"),
                        );
                        return glib::ControlFlow::Break;
                    }
                    if !handle.is_attached() {
                        restore();
                        export_flow::release(&flow);
                        match &result {
                            Ok(path) => {
                                let size =
                                    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
                                // The overlay's toast carries the trial count;
                                // the notification keeps it only when no card
                                // could be raised.
                                let shown = was_trial
                                    && toast_window.upgrade().is_some_and(|window| {
                                        crate::studio_trial_toast::show(
                                            &window,
                                            crate::studio_trial::trial_remaining(),
                                        )
                                    });
                                let mut body = format_size(size);
                                if !shown {
                                    if let Some(text) =
                                        crate::studio_trial::finished_feedback(was_trial)
                                    {
                                        body = format!("{body}\n\n{text}");
                                    }
                                }
                                crate::utils::notify::desktop_notification(
                                    &t("Export complete"),
                                    &body,
                                );
                            }
                            Err(err) => {
                                crate::utils::notify::desktop_notification(
                                    &t("Export failed"),
                                    err,
                                );
                            }
                        }
                        return glib::ControlFlow::Break;
                    }
                    match result {
                        Ok(path) => {
                            let mut config = crate::config::load_config();
                            if config.remember_video_export_dir(&path) {
                                let _ = crate::config::save_config(&config);
                            }
                            title.set_text(&t("Export complete"));
                            let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                            let mut notes = vec![format!(
                                "{} · {}",
                                format_size(size),
                                format_elapsed(started.elapsed().as_secs())
                            )];
                            if copy_to_clipboard {
                                match crate::utils::clipboard::copy_uri_to_clipboard(&path) {
                                    Ok(()) => notes.push(t(
                                        "File copied to clipboard. Ready to paste into other apps.",
                                    )),
                                    Err(error) => notes.push(format!(
                                        "{} {error}",
                                        t("Video exported, but could not copy to clipboard:")
                                    )),
                                }
                            }
                            // A finished trial export says how many remain, and
                            // when the last one is gone. The toast carries that
                            // copy, so the card only repeats it as a fallback.
                            let shown = was_trial
                                && toast_window.upgrade().is_some_and(|window| {
                                    crate::studio_trial_toast::show(
                                        &window,
                                        crate::studio_trial::trial_remaining(),
                                    )
                                });
                            if !shown {
                                if let Some(text) =
                                    crate::studio_trial::finished_feedback(was_trial)
                                {
                                    notes.push(text);
                                }
                            }
                            let notes_text = notes.join("\n");
                            detail.remove_css_class("recording-editor-export-error");
                            detail.set_text(&notes_text);
                            *complete_detail.borrow_mut() = notes_text;
                            detail.set_visible(true);
                            bar.set_visible(false);
                            stop.set_visible(false);
                            timing.set_visible(false);
                            actions.set_visible(true);
                            upload.set_visible(true);
                            upload.set_label(&t("Upload"));
                            upload.set_sensitive(true);
                            upload.remove_css_class("recording-editor-export-primary");
                            reveal.set_visible(true);
                            extra.set_visible(true);
                            back.grab_focus();

                            reveal.connect_clicked({
                                let path = path.clone();
                                move |_| {
                                    let folder = path
                                        .parent()
                                        .map(std::path::Path::to_path_buf)
                                        .unwrap_or_else(|| path.clone());
                                    if let Err(err) = crate::utils::open::open_path(&folder) {
                                        crate::utils::notify::desktop_notification(
                                            &t("Open failed"),
                                            &err,
                                        );
                                    }
                                }
                            });
                            open.connect_clicked({
                                let path = path.clone();
                                move |_| {
                                    if let Err(err) = crate::utils::open::open_path(&path) {
                                        crate::utils::notify::desktop_notification(
                                            &t("Open failed"),
                                            &err,
                                        );
                                    }
                                }
                            });
                            copy_file_button.connect_clicked({
                                let path = path.clone();
                                move |_| {
                                    report_clipboard_result(
                                        crate::utils::clipboard::copy_uri_to_clipboard(&path),
                                    )
                                }
                            });
                        }
                        Err(err) => {
                            title.set_text(&t("Export failed"));
                            timing.set_visible(false);
                            detail.add_css_class("recording-editor-export-error");
                            detail.set_text(&err);
                            detail.set_visible(true);
                            bar.set_visible(false);
                            stop.set_visible(false);
                            actions.set_visible(true);
                            back.grab_focus();
                        }
                    }
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    terminal.set(true);
                    exporting.set(false);
                    if handle.is_attached() {
                        title.set_text(&t("Export failed"));
                        timing.set_visible(false);
                        detail.add_css_class("recording-editor-export-error");
                        detail.set_text(&t("ApexShot lost contact with the export worker."));
                        detail.set_visible(true);
                        bar.set_visible(false);
                        stop.set_visible(false);
                        actions.set_visible(true);
                        back.grab_focus();
                    } else {
                        restore();
                        export_flow::release(&flow);
                        crate::utils::notify::desktop_notification(
                            &t("Export failed"),
                            &t("ApexShot lost contact with the export worker."),
                        );
                    }
                    glib::ControlFlow::Break
                }
            }
        }
    });
}

fn start_upload(ui: &UploadUi, phase: &Rc<Cell<UploadPhase>>) {
    ui.show_uploading(phase);

    let config = crate::config::load_config();
    let path = ui.path.clone();
    let (sender, receiver) = std::sync::mpsc::channel::<UploadEvent>();
    let _ = std::thread::spawn({
        let sender = sender.clone();
        move || {
            let progress_sender = sender.clone();
            let last_percent = Cell::new(None::<u32>);
            let outcome = crate::cloud::upload::upload_file_with_progress_and_heads_up(
                &config,
                &path,
                &move |sent, total| {
                    let percent = upload_percent(sent, total);
                    if last_percent.get() != Some(percent) {
                        last_percent.set(Some(percent));
                        let _ = progress_sender.send(UploadEvent::Progress { sent, total });
                    }
                },
            );
            let _ = sender.send(UploadEvent::Done {
                result: outcome.result.map(|result| result.share_url),
                heads_up: outcome.heads_up,
            });
        }
    });

    let ui = ui.clone();
    let phase = phase.clone();
    glib::timeout_add_local(Duration::from_millis(200), move || {
        let mut latest = None;
        let mut done = None;
        let mut disconnected = false;
        loop {
            match receiver.try_recv() {
                Ok(UploadEvent::Progress { sent, total }) => latest = Some((sent, total)),
                Ok(UploadEvent::Done { result, heads_up }) => {
                    done = Some((result, heads_up));
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        if let Some((sent, total)) = latest {
            ui.show_progress(sent, total);
        }

        if let Some((result, heads_up)) = done {
            match result {
                Ok(url) => {
                    ui.show_complete(&phase, url, heads_up.as_ref());
                    if let Some(window) = ui.window() {
                        if let Some(heads_up) = &heads_up {
                            for limit in &heads_up.cards {
                                crate::cloud::upgrade::show_prompt_almost(&window, limit);
                            }
                        }
                    }
                }
                Err(crate::cloud::upload::UploadError::UpgradeRequired(limit)) => {
                    phase.set(UploadPhase::Ready);
                    ui.show_complete_state();
                    if let Some(window) = ui.window() {
                        crate::cloud::upgrade::show_prompt(&window, &limit);
                    }
                }
                Err(err) => {
                    let message = err.to_string();
                    if !ui.window_is_active() {
                        crate::utils::notify::desktop_notification(&t("Upload failed"), &message);
                    }
                    ui.show_error(&phase, &message);
                }
            }
            return glib::ControlFlow::Break;
        }

        if disconnected {
            let message = t("ApexShot lost contact with the upload worker.");
            if !ui.window_is_active() {
                crate::utils::notify::desktop_notification(&t("Upload failed"), &message);
            }
            ui.show_error(&phase, &message);
            return glib::ControlFlow::Break;
        }

        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_percent_stays_below_a_hundred_until_finished() {
        assert_eq!(encoding_percent(0.0), 0);
        assert_eq!(encoding_percent(0.5), 50);
        assert_eq!(encoding_percent(0.994), 99);
        assert_eq!(encoding_percent(0.999), 99);
        assert_eq!(encoding_percent(1.0), 99);
    }

    #[test]
    fn upload_percent_stays_below_a_hundred_until_the_upload_finishes() {
        assert_eq!(upload_percent(0, 0), 0);
        assert_eq!(upload_percent(0, 1000), 0);
        assert_eq!(upload_percent(500, 1000), 50);
        assert_eq!(upload_percent(994, 1000), 99);
        assert_eq!(upload_percent(1000, 1000), 99);
    }

    #[test]
    fn progress_title_matches_the_encoding_pattern() {
        assert_eq!(progress_title("Uploading video", 0), "Uploading video · 0%");
        assert_eq!(
            progress_title("Uploading video", 42),
            "Uploading video · 42%"
        );
    }

    #[test]
    fn the_upload_flow_reports_progress_and_copies_when_the_window_is_active() {
        let source = include_str!("export_card.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production.contains("upload_file_with_progress"),
            "the upload must report real progress"
        );
        assert!(
            production.contains("copy_text_when_window_active"),
            "the share link must wait for window focus before copying"
        );
    }

    #[test]
    fn the_export_screen_covers_the_editor_and_hands_back_cleanly() {
        let Some((fills_shell, editor_hidden, editor_back, active_after, sensitive_after)) =
            crate::test_support::with_gtk(|| {
                let app = crate::test_support::test_app();
                let window = ApplicationWindow::new(&app);
                window.set_default_size(1400, 900);
                let shell = gtk4::Overlay::new();
                let root = GtkBox::new(Orientation::Vertical, 0);
                root.add_css_class("recording-editor-root");
                root.set_opacity(0.85);
                shell.set_child(Some(&root));
                window.set_child(Some(&shell));
                window.present();
                let screen = gtk4::CenterBox::new();
                screen.set_orientation(Orientation::Vertical);
                screen.set_hexpand(true);
                screen.set_vexpand(true);
                let handle = ExportScreen::attach(&window, &screen).expect("screen attaches");
                let editor_hidden = root.opacity() < 0.01;
                let active: export_flow::FlowActive = Rc::new(Cell::new(false));
                let flow = export_flow::ActiveFlow::begin(&window, &active).unwrap();
                let restored = Rc::new(Cell::new(false));
                let restore: Rc<dyn Fn()> = {
                    let restored = restored.clone();
                    Rc::new(move || restored.set(true))
                };
                let context = glib::MainContext::default();
                while context.pending() {
                    context.iteration(false);
                }
                let shell_bounds = screen.compute_bounds(&shell);
                let fills_shell = shell_bounds.is_some_and(|bounds| {
                    bounds.width() >= shell.width() as f32 - 1.0
                        && bounds.height() >= shell.height() as f32 - 1.0
                });
                hand_back_to_editor(&handle, &restore, &flow);
                (
                    fills_shell,
                    editor_hidden,
                    (root.opacity() - 0.85).abs() < 0.001 && restored.get(),
                    active.get(),
                    root.is_sensitive(),
                )
            })
        else {
            return;
        };
        assert!(fills_shell, "the export screen must fill the shell");
        assert!(
            editor_hidden,
            "the editor must be hidden while the screen is up"
        );
        assert!(editor_back, "handback must restore the exact prior opacity");
        assert!(!active_after);
        assert!(sensitive_after);
    }

    #[test]
    fn an_externally_detached_terminal_screen_restores_exactly_once() {
        let Some((second, restores, editor_back)) = crate::test_support::with_gtk(|| {
            let app = crate::test_support::test_app();
            let window = ApplicationWindow::new(&app);
            window.set_default_size(1400, 900);
            let shell = gtk4::Overlay::new();
            let root = GtkBox::new(Orientation::Vertical, 0);
            root.add_css_class("recording-editor-root");
            root.set_opacity(0.85);
            shell.set_child(Some(&root));
            window.set_child(Some(&shell));
            window.present();
            let screen = gtk4::CenterBox::new();
            screen.set_orientation(Orientation::Vertical);
            screen.set_hexpand(true);
            screen.set_vexpand(true);
            let handle = ExportScreen::attach(&window, &screen).expect("screen attaches");
            let active: export_flow::FlowActive = Rc::new(Cell::new(false));
            let flow = export_flow::ActiveFlow::begin(&window, &active).unwrap();
            let restored = Rc::new(Cell::new(0u32));
            let restore: Rc<dyn Fn()> = {
                let restored = restored.clone();
                Rc::new(move || restored.set(restored.get() + 1))
            };
            shell.remove_overlay(&screen);
            hand_back_to_editor(&handle, &restore, &flow);
            hand_back_to_editor(&handle, &restore, &flow);
            let second = handle.dismiss();
            (
                second,
                restored.get(),
                (root.opacity() - 0.85).abs() < 0.001,
            )
        }) else {
            return;
        };
        assert!(!second, "dismiss must be idempotent");
        assert_eq!(restores, 1, "restore must run exactly once");
        assert!(editor_back, "the exact prior opacity must be restored");
    }
}
