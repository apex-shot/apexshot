use gtk4::{
    glib, prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button, Label,
    Orientation, Stack, StackTransitionType, Widget,
};
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

mod cloud;
mod complete;
pub mod extensions;
mod howto;
mod mascot;
mod motion;
mod ui;
mod welcome;

use crate::i18n::{self, t, tfmt};
use crate::settings::ui_support::{install_settings_css, traffic_light_button};
use crate::settings::windowing::install_window_drag;

const ONBOARDING_FLAG_FILE: &str = ".onboarding_complete";

fn get_onboarding_flag_path() -> PathBuf {
    let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    config_dir.join("apexshot").join(ONBOARDING_FLAG_FILE)
}

pub fn is_onboarding_complete() -> bool {
    get_onboarding_flag_path().exists()
}

pub fn mark_onboarding_complete() -> std::io::Result<()> {
    let path = get_onboarding_flag_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::File::create(path)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OnboardingStep {
    Welcome,
    HowToUse,
    Cloud,
    ChromeExtension,
    Complete,
}

impl OnboardingStep {
    fn all() -> [Self; 5] {
        [
            Self::Welcome,
            Self::HowToUse,
            Self::Cloud,
            Self::ChromeExtension,
            Self::Complete,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::HowToUse => "Capturing",
            Self::Cloud => "Cloud sharing",
            Self::ChromeExtension => "Browser extension",
            Self::Complete => "Finish",
        }
    }

    fn position(self) -> usize {
        Self::all()
            .iter()
            .position(|step| *step == self)
            .unwrap_or(0)
    }

    fn next(self) -> Option<Self> {
        match self {
            Self::Welcome => Some(Self::HowToUse),
            Self::HowToUse => Some(Self::Cloud),
            Self::Cloud => Some(Self::ChromeExtension),
            Self::ChromeExtension => Some(Self::Complete),
            Self::Complete => None,
        }
    }

    fn prev(self) -> Option<Self> {
        match self {
            Self::Welcome => None,
            Self::HowToUse => Some(Self::Welcome),
            Self::Cloud => Some(Self::HowToUse),
            Self::ChromeExtension => Some(Self::Cloud),
            Self::Complete => Some(Self::ChromeExtension),
        }
    }
}

/// Identifies one rendered step. A step's background work checks `is_current`
/// so it stops as soon as the user navigates away or the window rebuilds.
#[derive(Clone)]
struct StepScope {
    generation: Rc<Cell<u64>>,
    id: u64,
}

impl StepScope {
    fn is_current(&self) -> bool {
        self.generation.get() == self.id
    }
}

/// What a step builder can do besides drawing itself.
#[derive(Clone)]
struct Nav {
    scope: StepScope,
    go: Rc<dyn Fn(OnboardingStep)>,
    advance: Rc<dyn Fn()>,
    finish: Rc<dyn Fn()>,
    rebuild: Rc<dyn Fn()>,
    /// Re-apply the footer button layout after a view re-renders its own
    /// actions in place without going through `show_step`.
    layout_footer: Rc<dyn Fn()>,
}

impl Nav {
    fn layout_footer(&self) {
        (self.layout_footer)();
    }
}

#[derive(Clone)]
struct OnboardingWidgets {
    window: ApplicationWindow,
    pages: Stack,
    progress: GtkBox,
    step_caption: Label,
    back_slot: GtkBox,
    left_actions: GtkBox,
    actions: GtkBox,
    status: Label,
    generation: Rc<Cell<u64>>,
    animations: Rc<RefCell<Vec<gtk4::TickCallbackId>>>,
}

impl OnboardingWidgets {
    fn cancel_animations(&self) {
        cancel_animations(&self.animations);
    }
}

fn cancel_animations(animations: &Rc<RefCell<Vec<gtk4::TickCallbackId>>>) {
    for callback in animations.borrow_mut().drain(..) {
        callback.remove();
    }
}

pub fn show_onboarding_window() -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .build();

    app.connect_activate(|application| {
        let windows = application.windows();
        if let Some(existing_window) = windows.first() {
            existing_window.present();
            return;
        }

        build_onboarding_window(application);
    });

    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn build_onboarding_window(app: &Application) {
    use std::sync::Once;
    static INIT_ICONS: Once = Once::new();
    INIT_ICONS.call_once(|| {
        relm4_icons::initialize_icons(
            crate::capture::editor::window::icon_names::GRESOURCE_BYTES,
            crate::capture::editor::window::icon_names::RESOURCE_PREFIX,
        );
    });

    install_settings_css();
    let language = crate::config::load_config().ui_language;
    i18n::apply_gtk_direction(&language);

    let animations = Rc::new(RefCell::new(Vec::new()));
    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("ApexShot Setup"))
        .default_width(960)
        .default_height(640)
        .decorated(false)
        .build();
    window.add_css_class("editor-window");

    let close_animations = Rc::clone(&animations);
    window.connect_close_request(move |_| {
        cancel_animations(&close_animations);
        glib::Propagation::Proceed
    });

    install(&window, animations);
    window.present();
}

/// Build the whole onboarding surface into `window` and show the first step.
/// Language changes call this again so every string and direction is rebuilt.
fn install(window: &ApplicationWindow, animations: Rc<RefCell<Vec<gtk4::TickCallbackId>>>) {
    cancel_animations(&animations);
    let dark = crate::capture::editor::ui_support::prefers_dark_glass_theme();
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class(if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });

    let step_caption = Label::new(None);
    step_caption.add_css_class("onboarding-progress-caption");
    step_caption.set_valign(Align::Center);
    // Back lives in the title bar's top-left corner rather than the footer.
    let back_slot = GtkBox::new(Orientation::Horizontal, 0);
    back_slot.set_halign(Align::Start);
    root.append(&build_toolbar(window, &step_caption, &back_slot));

    let content = GtkBox::new(Orientation::Vertical, 0);
    content.set_hexpand(true);
    content.set_vexpand(true);

    let progress = GtkBox::new(Orientation::Horizontal, 6);
    progress.add_css_class("onboarding-progress");
    progress.set_halign(Align::Center);
    progress.set_margin_top(24);
    content.append(&progress);

    let pages = Stack::new();
    pages.set_hexpand(true);
    pages.set_vexpand(true);
    pages.set_hhomogeneous(false);
    pages.set_vhomogeneous(false);
    pages.set_transition_duration(320);
    pages.set_transition_type(if motion::enabled() {
        StackTransitionType::Crossfade
    } else {
        StackTransitionType::None
    });
    pages.connect_notify_local(Some("transition-running"), |pages, _| prune_pages(pages));
    pages.set_margin_top(22);
    content.append(&pages);

    let status = Label::new(None);
    status.add_css_class("onboarding-status-error");
    status.set_wrap(true);
    status.set_max_width_chars(44);
    status.set_xalign(1.0);
    status.set_visible(false);
    // A step with one action centers it; with two, the secondary one moves to
    // this far-left slot and the primary stays at the end. show_step fills it.
    let left_actions = GtkBox::new(Orientation::Horizontal, 10);
    left_actions.set_halign(Align::Start);
    let actions = GtkBox::new(Orientation::Horizontal, 10);
    actions.set_hexpand(true);

    let footer = GtkBox::new(Orientation::Horizontal, 12);
    footer.add_css_class("onboarding-footer");
    footer.set_margin_start(140);
    footer.set_margin_end(140);
    footer.set_margin_top(14);
    footer.set_margin_bottom(24);
    footer.append(&left_actions);
    footer.append(&status);
    footer.append(&actions);
    content.append(&footer);
    root.append(&content);
    window.set_child(Some(&root));

    let widgets = OnboardingWidgets {
        window: window.clone(),
        pages,
        progress,
        step_caption,
        back_slot,
        left_actions,
        actions,
        status,
        generation: Rc::new(Cell::new(0)),
        animations,
    };
    show_step(&widgets, OnboardingStep::Welcome);
}

fn build_toolbar(
    window: &ApplicationWindow,
    step_caption: &Label,
    back_slot: &GtkBox,
) -> gtk4::CenterBox {
    let toolbar = gtk4::CenterBox::new();
    toolbar.add_css_class("settings-window-controls");
    toolbar.add_css_class("onboarding-toolbar");
    toolbar.set_size_request(-1, 36);

    // Centering the caption in the title bar keeps it lined up with the
    // progress rail below. The Back button sits at the far left, and the drag
    // handle fills the rest of the start side so that area still moves the
    // window.
    let start_box = GtkBox::new(Orientation::Horizontal, 6);
    start_box.set_halign(Align::Fill);
    start_box.append(back_slot);
    let drag_handle = GtkBox::new(Orientation::Horizontal, 0);
    drag_handle.set_hexpand(true);
    drag_handle.set_halign(Align::Fill);
    drag_handle.set_vexpand(false);
    start_box.append(&drag_handle);
    toolbar.set_start_widget(Some(&start_box));

    let close_btn = traffic_light_button("traffic-light-red", &t("Close"));
    close_btn.remove_css_class("recent-captures-wm-btn");
    close_btn.remove_css_class("recent-captures-wm-close");
    close_btn.add_css_class("recording-editor-traffic-btn");
    let win_clone = window.clone();
    close_btn.connect_clicked(move |_| win_clone.close());

    let min_btn = traffic_light_button("traffic-light-yellow", &t("Minimize"));
    min_btn.remove_css_class("recent-captures-wm-btn");
    min_btn.add_css_class("recording-editor-traffic-btn");
    let win_clone = window.clone();
    min_btn.connect_clicked(move |_| win_clone.minimize());

    for button in [&close_btn, &min_btn] {
        button.set_size_request(28, 28);
        button.set_valign(Align::Center);
    }

    let right_box = GtkBox::new(Orientation::Horizontal, 6);
    right_box.set_halign(Align::End);
    right_box.append(&min_btn);
    right_box.append(&close_btn);
    toolbar.set_end_widget(Some(&right_box));
    toolbar.set_center_widget(Some(step_caption));

    install_window_drag(&drag_handle, window);
    toolbar
}

fn show_step(widgets: &OnboardingWidgets, step: OnboardingStep) {
    widgets.cancel_animations();
    let id = widgets.generation.get() + 1;
    widgets.generation.set(id);
    widgets.window.set_title(Some(&t("ApexShot Setup")));
    widgets.status.set_visible(false);
    clear(&widgets.actions);
    clear(&widgets.back_slot);
    clear(&widgets.left_actions);
    // With no Back button the slot is hidden so the drag handle starts at the
    // window edge instead of leaving a gap.
    widgets.back_slot.set_visible(step.prev().is_some());
    let caption = step_caption(step);
    widgets.step_caption.set_text(&caption);
    build_progress(&widgets.progress, step);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("onboarding-page");
    body.set_width_request(680);
    body.set_hexpand(true);
    body.set_halign(Align::Fill);
    body.set_margin_start(140);
    body.set_margin_end(140);
    body.set_vexpand(true);
    let mut animations = Vec::new();
    let nav = Nav {
        scope: StepScope {
            generation: Rc::clone(&widgets.generation),
            id,
        },
        go: {
            let widgets = widgets.clone();
            Rc::new(move |target| show_step(&widgets, target))
        },
        advance: {
            let widgets = widgets.clone();
            Rc::new(move || {
                if let Some(next) = step.next() {
                    show_step(&widgets, next);
                }
            })
        },
        finish: {
            let widgets = widgets.clone();
            Rc::new(move || finish(&widgets))
        },
        rebuild: {
            let window = widgets.window.clone();
            let animations = Rc::clone(&widgets.animations);
            Rc::new(move || {
                let window = window.clone();
                let animations = Rc::clone(&animations);
                glib::idle_add_local_once(move || install(&window, animations));
            })
        },
        layout_footer: {
            let actions = widgets.actions.clone();
            let left_actions = widgets.left_actions.clone();
            Rc::new(move || layout_footer_actions(&actions, &left_actions))
        },
    };

    let mascot = mascot::Mascot::new(step);
    match step {
        OnboardingStep::Welcome => welcome::build(&body, &widgets.actions, &nav, mascot.widget()),
        OnboardingStep::HowToUse => howto::build(&body, &widgets.actions, &nav),
        OnboardingStep::Cloud => cloud::build(&body, &widgets.actions, &nav),
        OnboardingStep::ChromeExtension => extensions::build_chrome(&body, &widgets.actions, &nav),
        OnboardingStep::Complete => complete::build(&body, &widgets.actions, &nav),
    }
    layout_footer_actions(&widgets.actions, &widgets.left_actions);

    if step != OnboardingStep::Welcome {
        body.prepend(mascot.widget());
    }
    if let Some(callback) = mascot.animate() {
        animations.push(callback);
    }

    if let Some(previous) = step.prev() {
        let back_btn = Button::with_label(&t("← Back"));
        back_btn.add_css_class("onboarding-text-button");
        back_btn.set_valign(Align::Center);
        back_btn.set_margin_start(12);
        back_btn.set_margin_top(6);
        let back_slot = widgets.back_slot.clone();
        let widgets_c = widgets.clone();
        back_btn.connect_clicked(move |_| show_step(&widgets_c, previous));
        back_slot.append(&back_btn);
    }

    show_page(&widgets.pages, &body, &format!("step-{id}"));

    let mut entrance = if step == OnboardingStep::Welcome {
        body.first_child()
            .and_then(|content| content.downcast::<GtkBox>().ok())
            .map(|content| children(&content))
            .unwrap_or_default()
    } else {
        children(&body)
    };
    entrance.retain(|widget| !widget.has_css_class("onboarding-mascot"));
    entrance.push(widgets.actions.clone().upcast());
    if let Some(callback) = motion::reveal(&entrance, 70.0, 420.0) {
        animations.push(callback);
    }
    widgets.animations.replace(animations);
}

fn show_page(pages: &Stack, page: &GtkBox, name: &str) {
    pages.add_named(page, Some(name));
    pages.set_visible_child_name(name);
    prune_pages(pages);
}

/// Drop superseded pages once the crossfade has finished. Removing a page
/// unmaps it, which also ends its entrance animation.
fn prune_pages(pages: &Stack) {
    if pages.is_transition_running() {
        return;
    }
    let visible = pages.visible_child();
    let mut child = pages.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if visible.as_ref() != Some(&widget) {
            pages.remove(&widget);
        }
    }
}

fn step_caption(step: OnboardingStep) -> String {
    let steps = OnboardingStep::all();
    tfmt(
        "Step {current} of {total}",
        &[
            ("current", &(step.position() + 1).to_string()),
            ("total", &steps.len().to_string()),
        ],
    )
}

fn build_progress(progress: &GtkBox, step: OnboardingStep) {
    clear(progress);
    let steps = OnboardingStep::all();
    let current = step.position();
    for (index, item) in steps.iter().enumerate() {
        let dot = GtkBox::new(Orientation::Horizontal, 0);
        dot.add_css_class("onboarding-progress-dot");
        if index == current {
            dot.add_css_class("onboarding-progress-dot-active");
            dot.set_size_request(22, 6);
        } else {
            dot.set_size_request(6, 6);
        }
        if index < current {
            dot.add_css_class("onboarding-progress-dot-done");
        }
        dot.set_tooltip_text(Some(&t(item.label())));
        progress.append(&dot);
    }
}

fn finish(widgets: &OnboardingWidgets) {
    if let Err(error) = mark_onboarding_complete() {
        eprintln!("[onboarding] Failed to save setup state: {error}");
        widgets.status.set_text(&t(
            "Couldn't save your setup. Check that your config folder is writable, then try again.",
        ));
        widgets.status.set_visible(true);
        return;
    }
    // Tray + hotkeys live in the daemon; Settings alone is not enough.
    std::thread::spawn(|| {
        let _ = crate::daemon::ensure_daemon_running();
    });
    // Spawn the main app (settings UI) now that onboarding is complete
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("apexshot"));
    if let Err(e) = std::process::Command::new(&exe).spawn() {
        eprintln!("Failed to launch settings window: {e}");
    }
    widgets.window.close();
}

fn clear(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn children(container: &GtkBox) -> Vec<Widget> {
    let mut out = Vec::new();
    let mut child = container.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        out.push(widget);
    }
    out
}

/// Lay out a step's footer actions: a single action is centered (in line with
/// the caption and rail), while two or more spread out, with the secondary one
/// in the far-left slot and the primary at the end.
fn layout_footer_actions(actions: &GtkBox, left_slot: &GtkBox) {
    // Drop whatever a previous render split off, so re-rendering in place
    // cannot leave a stale button in the left slot.
    clear(left_slot);
    if children(actions).len() < 2 {
        actions.set_halign(Align::Center);
        return;
    }
    if let Some(first) = actions.first_child() {
        actions.remove(&first);
        left_slot.append(&first);
    }
    actions.set_halign(Align::End);
}

#[cfg(test)]
mod tests {
    use super::OnboardingStep;

    #[test]
    fn step_chain_matches_the_rail_order() {
        let steps = OnboardingStep::all();
        assert_eq!(steps.len(), 5);

        // next() walks the same order as the rail, prev() walks it back.
        for pair in steps.windows(2) {
            assert_eq!(pair[0].next(), Some(pair[1]));
            assert_eq!(pair[1].prev(), Some(pair[0]));
        }
        assert_eq!(steps[0].prev(), None);
        assert_eq!(steps[steps.len() - 1].next(), None);
    }

    #[test]
    fn every_step_has_a_label() {
        for step in OnboardingStep::all() {
            assert!(!step.label().is_empty());
        }
    }
}
