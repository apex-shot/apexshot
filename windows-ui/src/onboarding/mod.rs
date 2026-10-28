//! First-run onboarding for the Windows build.
//!
//! The same surface the Linux build shows (`src/onboarding`): one undecorated
//! window with the step caption in its title bar, a progress rail, a mascot,
//! and a page per step. The cloud page is shared with the Linux flow verbatim,
//! because signing in is the same browser device-authorization handoff on both
//! platforms and it stays optional. Finishing writes
//! `%APPDATA%\apexshot\.windows-onboarding-complete`, which the entry point
//! checks before it opens Settings.

mod cloud;
mod complete;
mod howto;
mod mascot;
mod motion;
mod ui;
mod welcome;

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Application, ApplicationWindow, Box as GtkBox, CssProvider, Orientation, Stack,
    StackTransitionType,
};

use crate::config::config_path;
use crate::i18n::{t, tfmt};
use crate::settings::ui_support::{window_control_button, WindowControl, TITLE_BAR_HEIGHT};

/// Written only after the final page finishes.
const FLAG_FILE: &str = ".windows-onboarding-complete";

/// The window box at 100% display scaling, the size the Linux flow uses.
const BASE_WIDTH: f64 = 960.0;
const BASE_HEIGHT: f64 = 640.0;

/// Inset of the page column and the footer from the window edges.
const PAGE_INSET: f64 = 140.0;

/// Width the page column keeps inside that inset.
const PAGE_WIDTH: f64 = 680.0;

/// What the caller should do after the onboarding window closes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub finished: bool,
    pub open_settings: bool,
}

/// One page of the flow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OnboardingStep {
    Welcome,
    HowToUse,
    Cloud,
    Complete,
}

impl OnboardingStep {
    fn all() -> [Self; 4] {
        [Self::Welcome, Self::HowToUse, Self::Cloud, Self::Complete]
    }

    fn label(self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::HowToUse => "Capturing",
            Self::Cloud => "Cloud sharing",
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
            Self::Cloud => Some(Self::Complete),
            Self::Complete => None,
        }
    }

    fn prev(self) -> Option<Self> {
        match self {
            Self::Welcome => None,
            Self::HowToUse => Some(Self::Welcome),
            Self::Cloud => Some(Self::HowToUse),
            Self::Complete => Some(Self::Cloud),
        }
    }
}

/// A design value at 100% scaling, in this display's logical pixels.
pub(super) fn px(scale: f64, value: f64) -> i32 {
    (value * scale).round() as i32
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

/// What a step builder can do besides drawing itself. `scale` is the display
/// scale every page lays itself out against.
#[derive(Clone)]
pub(crate) struct Nav {
    scope: StepScope,
    scale: f64,
    go: Rc<dyn Fn(OnboardingStep)>,
    advance: Rc<dyn Fn()>,
    finish: Rc<dyn Fn()>,
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
    step_caption: gtk4::Label,
    back_slot: GtkBox,
    left_actions: GtkBox,
    actions: GtkBox,
    status: gtk4::Label,
    scale: f64,
    generation: Rc<Cell<u64>>,
    animations: Rc<RefCell<Vec<gtk4::TickCallbackId>>>,
    outcome: Rc<RefCell<Outcome>>,
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

fn flag_path() -> Option<PathBuf> {
    let path = config_path()?;
    path.parent()
        .map(|parent| parent.join(FLAG_FILE))
        .filter(|path| !path.as_os_str().is_empty())
}

/// True when this Windows install has finished onboarding.
pub fn is_complete() -> bool {
    flag_path().is_some_and(|path| path.exists())
}

/// Mark this Windows install as onboarded.
pub fn mark_complete() -> std::io::Result<()> {
    let Some(path) = flag_path() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no config directory",
        ));
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, b"1\n")
}

/// Open the onboarding window and wait for it to close.
pub fn run() -> anyhow::Result<Outcome> {
    let app = Application::builder()
        .application_id(format!("{}.setup", crate::app_identity::app_id()))
        .build();
    let outcome = Rc::new(RefCell::new(Outcome::default()));
    let outcome_for_app = Rc::clone(&outcome);

    app.connect_activate(move |application| {
        if let Some(existing) = application.windows().first() {
            existing.present();
            return;
        }
        build_window(application, Rc::clone(&outcome_for_app));
    });

    let _ = app.run_with_args::<String>(&[]);
    let result = *outcome.borrow();
    Ok(result)
}

fn build_window(app: &Application, outcome: Rc<RefCell<Outcome>>) {
    crate::icons::install();
    crate::settings::ui_support::install_settings_css();
    crate::settings::install_window_css();

    let scale = crate::platform::ui_scale();
    install_css(scale);

    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("ApexShot Setup"))
        .icon_name(crate::app_identity::icon_name())
        .default_width(px(scale, BASE_WIDTH))
        .default_height(px(scale, BASE_HEIGHT))
        .build();
    window.set_decorated(false);
    window.add_css_class("editor-window");

    let animations = Rc::new(RefCell::new(Vec::new()));
    let close_animations = Rc::clone(&animations);
    window.connect_close_request(move |_| {
        cancel_animations(&close_animations);
        glib::Propagation::Proceed
    });

    install(&window, scale, animations, outcome);
    window.present();
}

/// Build the whole onboarding surface into `window` and show the first step.
fn install(
    window: &ApplicationWindow,
    scale: f64,
    animations: Rc<RefCell<Vec<gtk4::TickCallbackId>>>,
    outcome: Rc<RefCell<Outcome>>,
) {
    cancel_animations(&animations);
    let dark = crate::settings::windowing::prefers_dark_glass_theme();
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    if !dark {
        root.add_css_class("editor-theme-light");
    }

    let step_caption = gtk4::Label::new(None);
    step_caption.add_css_class("onboarding-progress-caption");
    step_caption.set_valign(Align::Center);
    // Back lives in the title bar's top-left corner rather than the footer.
    let back_slot = GtkBox::new(Orientation::Horizontal, 0);
    back_slot.set_halign(Align::Start);
    root.append(&build_toolbar(window, &step_caption, &back_slot, scale));

    let content = GtkBox::new(Orientation::Vertical, 0);
    content.set_hexpand(true);
    content.set_vexpand(true);

    let progress = GtkBox::new(Orientation::Horizontal, px(scale, 6.0));
    progress.add_css_class("onboarding-progress");
    progress.set_halign(Align::Center);
    progress.set_margin_top(px(scale, 24.0));
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
    pages.set_margin_top(px(scale, 22.0));
    content.append(&pages);

    let status = gtk4::Label::new(None);
    status.add_css_class("onboarding-status-error");
    status.set_wrap(true);
    status.set_max_width_chars(44);
    status.set_xalign(1.0);
    status.set_visible(false);
    // A step with one action centers it; with two, the secondary one moves to
    // this far-left slot and the primary stays at the end. show_step fills it.
    let left_actions = GtkBox::new(Orientation::Horizontal, px(scale, 10.0));
    left_actions.set_halign(Align::Start);
    let actions = GtkBox::new(Orientation::Horizontal, px(scale, 10.0));
    actions.set_hexpand(true);

    let footer = GtkBox::new(Orientation::Horizontal, px(scale, 12.0));
    footer.add_css_class("onboarding-footer");
    footer.set_margin_start(px(scale, PAGE_INSET));
    footer.set_margin_end(px(scale, PAGE_INSET));
    footer.set_margin_top(px(scale, 14.0));
    footer.set_margin_bottom(px(scale, 24.0));
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
        scale,
        generation: Rc::new(Cell::new(0)),
        animations,
        outcome,
    };
    show_step(&widgets, OnboardingStep::Welcome);
}

/// The title bar: the step caption centered over the progress rail, Back at the
/// far left, and the caption targets in the corner Settings puts them in.
fn build_toolbar(
    window: &ApplicationWindow,
    step_caption: &gtk4::Label,
    back_slot: &GtkBox,
    scale: f64,
) -> gtk4::CenterBox {
    let toolbar = gtk4::CenterBox::new();
    toolbar.add_css_class("settings-window-controls");
    toolbar.add_css_class("onboarding-toolbar");
    toolbar.set_size_request(-1, px(scale, TITLE_BAR_HEIGHT));

    let start_box = GtkBox::new(Orientation::Horizontal, px(scale, 6.0));
    start_box.set_halign(Align::Fill);
    start_box.append(back_slot);
    toolbar.set_start_widget(Some(&start_box));

    let minimize = window_control_button(WindowControl::Minimize, &t("Minimize"), scale);
    let window_c = window.clone();
    minimize.connect_clicked(move |_| window_c.minimize());

    let close = window_control_button(WindowControl::Close, &t("Close"), scale);
    let window_c = window.clone();
    close.connect_clicked(move |_| window_c.close());

    let right_box = GtkBox::new(Orientation::Horizontal, 0);
    right_box.set_halign(Align::End);
    right_box.append(&minimize);
    right_box.append(&close);
    toolbar.set_end_widget(Some(&right_box));
    toolbar.set_center_widget(Some(step_caption));

    // The whole bar moves the window, not just its left corner: the buttons
    // and Back claim their own presses, everything else drags.
    crate::settings::windowing::install_window_drag(&toolbar, window);
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
    // With no Back button the slot is hidden so the bar starts at the window
    // edge instead of leaving a gap.
    widgets.back_slot.set_visible(step.prev().is_some());
    widgets.step_caption.set_text(&step_caption(step));
    build_progress(&widgets.progress, step, widgets.scale);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("onboarding-page");
    body.set_width_request(px(widgets.scale, PAGE_WIDTH));
    body.set_hexpand(true);
    body.set_halign(Align::Fill);
    body.set_margin_start(px(widgets.scale, PAGE_INSET));
    body.set_margin_end(px(widgets.scale, PAGE_INSET));
    body.set_vexpand(true);
    let mut animations = Vec::new();
    let nav = Nav {
        scope: StepScope {
            generation: Rc::clone(&widgets.generation),
            id,
        },
        scale: widgets.scale,
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
        layout_footer: {
            let actions = widgets.actions.clone();
            let left_actions = widgets.left_actions.clone();
            Rc::new(move || layout_footer_actions(&actions, &left_actions))
        },
    };

    let mascot = mascot::Mascot::new(step, widgets.scale);
    match step {
        OnboardingStep::Welcome => welcome::build(&body, &widgets.actions, &nav, mascot.widget()),
        OnboardingStep::HowToUse => howto::build(&body, &widgets.actions, &nav),
        OnboardingStep::Cloud => cloud::build(&body, &widgets.actions, &nav),
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
        let back_btn = gtk4::Button::with_label(&t("← Back"));
        back_btn.add_css_class("onboarding-text-button");
        back_btn.set_valign(Align::Center);
        back_btn.set_margin_start(px(widgets.scale, 12.0));
        back_btn.set_margin_top(px(widgets.scale, 6.0));
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

fn build_progress(progress: &GtkBox, step: OnboardingStep, scale: f64) {
    clear(progress);
    let steps = OnboardingStep::all();
    let current = step.position();
    for (index, item) in steps.iter().enumerate() {
        let dot = GtkBox::new(Orientation::Horizontal, 0);
        dot.add_css_class("onboarding-progress-dot");
        if index == current {
            dot.add_css_class("onboarding-progress-dot-active");
            dot.set_size_request(px(scale, 22.0), px(scale, 6.0));
        } else {
            dot.set_size_request(px(scale, 6.0), px(scale, 6.0));
        }
        if index < current {
            dot.add_css_class("onboarding-progress-dot-done");
        }
        dot.set_tooltip_text(Some(&t(item.label())));
        progress.append(&dot);
    }
}

fn finish(widgets: &OnboardingWidgets) {
    if let Err(error) = mark_complete() {
        eprintln!("[onboarding] failed to save setup state: {error}");
        widgets.status.set_text(&t(
            "Couldn't save your setup. Check that your config folder is writable, then try again.",
        ));
        widgets.status.set_visible(true);
        return;
    }
    {
        let mut outcome = widgets.outcome.borrow_mut();
        outcome.finished = true;
        outcome.open_settings = true;
    }
    widgets.window.close();
}

pub(crate) fn clear(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn children(container: &GtkBox) -> Vec<gtk4::Widget> {
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

/// Load the onboarding metrics that follow the display scale. The colours,
/// radii, and paddings come from the shared settings stylesheet; only the
/// sizes GTK CSS cannot express relative to the manual Windows scale are here.
fn install_css(scale: f64) {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&scaled_css(scale));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn scaled_css(scale: f64) -> String {
    format!(
        ".onboarding-progress-caption {{ font-size:{}px; }} \
         .onboarding-headline {{ font-size:{}px; }} \
         .onboarding-copy {{ font-size:{}px; }} \
         .onboarding-text-button {{ font-size:{}px; padding:{}px {}px; }} \
         .onboarding-text-button:focus, .onboarding-text-button:focus-visible {{ \
             outline: none; box-shadow: none; background: transparent; }} \
         .onboarding-hotkey-table .shortcut-capture-keycap {{ \
             font-size:{}px; padding:{}px {}px; }} \
         .onboarding-connect-code {{ font-size:{}px; }} \
         .onboarding-status-error {{ font-size:{}px; }}",
        px(scale, 12.0),
        px(scale, 26.0),
        px(scale, 14.0),
        px(scale, 13.0),
        px(scale, 6.0),
        px(scale, 12.0),
        px(scale, 12.0),
        px(scale, 4.0),
        px(scale, 10.0),
        px(scale, 26.0),
        px(scale, 12.0),
    )
}

#[cfg(test)]
mod tests {
    use super::OnboardingStep;

    #[test]
    fn step_chain_matches_the_rail_order() {
        let steps = OnboardingStep::all();
        assert_eq!(steps.len(), 4);

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
