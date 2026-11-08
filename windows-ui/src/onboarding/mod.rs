//! First-run onboarding for the Windows build.
//!
//! The same steps the Linux build shows (`src/onboarding`) in one undecorated
//! window: the step's page in a centred column under the app mark, with the
//! progress rail at its foot. The cloud page is shared
//! with the Linux flow verbatim, because signing in is the same browser
//! device-authorization handoff on both platforms and it stays optional.
//! Finishing writes `%APPDATA%\apexshot\.windows-onboarding-complete`, which
//! the entry point checks before it opens Settings.

mod blur_text;
mod cloud;
mod complete;
mod howto;
mod motion;
pub(crate) mod showcase;
pub(crate) mod ui;
mod welcome;

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Application, ApplicationWindow, Box as GtkBox, CssProvider, Orientation, Overlay,
    Stack, StackTransitionType,
};

use crate::capture::editor::window::icon_names::custom;
use crate::config::config_path;
use crate::i18n::{t, tfmt};
use crate::settings::ui_support::{window_control_button, WindowControl};

/// Written only after the final page finishes.
const FLAG_FILE: &str = ".windows-onboarding-complete";

/// The window box at 100% display scaling.
const BASE_WIDTH: f64 = 820.0;
const BASE_HEIGHT: f64 = 620.0;

/// Width of the pane that holds the step's page.
const PANE_WIDTH: f64 = 480.0;

/// Inset of the page from the pane's edges.
const PAGE_INSET: f64 = 48.0;

/// The one gap between a page's sections, so every step reads with the same
/// rhythm. Smaller sub-gaps inside a section stay local to that section.
pub(super) const SECTION_GAP: f64 = 20.0;

/// What happened when the onboarding window closed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// True only when the last page finished; a window the user closed early
    /// leaves it false so the caller starts nothing.
    pub finished: bool,
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
    back_slot: GtkBox,
    actions: GtkBox,
    status: gtk4::Label,
    dark: bool,
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
    root.add_css_class("onboarding-root");
    if !dark {
        root.add_css_class("editor-theme-light");
    }

    let pane = GtkBox::new(Orientation::Vertical, 0);
    pane.add_css_class("onboarding-pane");
    pane.set_size_request(px(scale, PANE_WIDTH), -1);
    pane.set_hexpand(false);
    pane.set_halign(Align::Center);
    crate::settings::windowing::install_window_drag(&pane, window);

    // Back floats in the window's top-left corner, over the centred pane;
    // the pane itself is the drag handle.
    let back_slot = GtkBox::new(Orientation::Horizontal, 0);
    back_slot.set_halign(Align::Start);
    back_slot.set_valign(Align::Start);

    let pages = crossfade_stack();
    pages.connect_notify_local(Some("transition-running"), |pages, _| prune_pages(pages));
    // Each form fits the pane; setup has no scrolling container.
    pane.append(&pages);

    // Actions belong to the form, not the window's foot.
    let actions = GtkBox::new(Orientation::Vertical, px(scale, 10.0));
    actions.add_css_class("onboarding-actions");
    actions.set_margin_top(px(scale, 28.0));

    let status = gtk4::Label::new(None);
    status.add_css_class("onboarding-status-error");
    status.set_wrap(true);
    status.set_max_width_chars(44);
    status.set_justify(gtk4::Justification::Center);
    status.set_visible(false);

    let progress = GtkBox::new(Orientation::Horizontal, 0);
    progress.add_css_class("onboarding-progress");
    progress.set_halign(Align::Center);

    let footer = GtkBox::new(Orientation::Vertical, px(scale, 10.0));
    footer.add_css_class("onboarding-footer");
    footer.set_margin_start(px(scale, PAGE_INSET));
    footer.set_margin_end(px(scale, PAGE_INSET));
    footer.set_margin_top(px(scale, 18.0));
    footer.set_margin_bottom(px(scale, 28.0));
    footer.append(&status);
    footer.append(&progress);
    pane.append(&footer);

    let overlay = Overlay::new();
    overlay.set_vexpand(true);
    overlay.set_child(Some(&pane));
    overlay.add_overlay(&back_slot);
    overlay.add_overlay(&window_controls(window, scale));
    root.append(&overlay);
    window.set_child(Some(&root));

    let widgets = OnboardingWidgets {
        window: window.clone(),
        pages,
        progress,
        back_slot,
        actions,
        status,
        dark,
        scale,
        generation: Rc::new(Cell::new(0)),
        animations,
        outcome,
    };
    show_step(&widgets, OnboardingStep::Welcome);
}

fn crossfade_stack() -> Stack {
    let stack = Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_transition_duration(320);
    stack.set_transition_type(if motion::enabled() {
        StackTransitionType::Crossfade
    } else {
        StackTransitionType::None
    });
    stack
}

/// Minimize and Close in the window's top-right corner, where Settings puts
/// them, floating over the showcase card.
fn window_controls(window: &ApplicationWindow, scale: f64) -> GtkBox {
    let minimize = window_control_button(WindowControl::Minimize, &t("Minimize"), scale);
    let window_c = window.clone();
    minimize.connect_clicked(move |_| window_c.minimize());

    let close = window_control_button(WindowControl::Close, &t("Close"), scale);
    let window_c = window.clone();
    close.connect_clicked(move |_| window_c.close());

    let controls = GtkBox::new(Orientation::Horizontal, 0);
    controls.add_css_class("settings-window-controls");
    controls.add_css_class("onboarding-toolbar");
    controls.set_halign(Align::End);
    controls.set_valign(Align::Start);
    controls.append(&minimize);
    controls.append(&close);
    controls
}

fn show_step(widgets: &OnboardingWidgets, step: OnboardingStep) {
    widgets.cancel_animations();
    let id = widgets.generation.get() + 1;
    widgets.generation.set(id);
    widgets.window.set_title(Some(&t("ApexShot Setup")));
    widgets.status.set_visible(false);
    clear(&widgets.back_slot);
    // With no Back button the slot is hidden so the bar starts at the window
    // edge instead of leaving a gap.
    widgets.back_slot.set_visible(step.prev().is_some());
    build_progress(&widgets.progress, step, widgets.scale);

    let scale = widgets.scale;
    // A short form sits at the pane's optical centre.
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.add_css_class("onboarding-page");
    page.set_hexpand(true);
    page.set_halign(Align::Fill);
    page.set_margin_start(px(scale, PAGE_INSET));
    page.set_margin_end(px(scale, PAGE_INSET));
    page.set_valign(Align::Center);
    page.set_margin_top(px(scale, 24.0));
    page.set_margin_bottom(px(scale, 40.0));

    let brand = showcase::step_mark(step, scale, widgets.dark);
    brand.set_margin_bottom(px(scale, 24.0));
    page.append(&brand);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.set_hexpand(true);
    page.append(&body);

    let actions = widgets.actions.clone();
    if let Some(parent) = actions.parent().and_downcast::<GtkBox>() {
        parent.remove(&actions);
    }
    clear(&actions);

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
            let actions = actions.clone();
            Rc::new(move || layout_footer_actions(&actions))
        },
    };

    match step {
        OnboardingStep::Welcome => {
            welcome::build(&body, &actions, &nav, &mut animations);
        }
        OnboardingStep::HowToUse => howto::build(&body, &actions, &nav),
        OnboardingStep::Cloud => cloud::build(&body, &actions, &nav),
        OnboardingStep::Complete => complete::build(&body, &actions, &nav),
    }
    layout_footer_actions(&actions);
    page.append(&actions);

    if let Some(previous) = step.prev() {
        let back_btn = gtk4::Button::new();
        back_btn.add_css_class("onboarding-text-button");
        let back_content = GtkBox::new(Orientation::Horizontal, px(scale, 4.0));
        let back_icon = gtk4::Image::from_icon_name(custom::CHEVRON_LEFT_SYMBOLIC);
        back_icon.set_pixel_size(px(scale, 14.0));
        back_content.append(&back_icon);
        back_content.append(&gtk4::Label::new(Some(&t("Back"))));
        back_btn.set_child(Some(&back_content));
        back_btn.set_valign(Align::Center);
        back_btn.set_margin_start(px(scale, 12.0));
        back_btn.set_margin_top(px(scale, 8.0));
        let back_slot = widgets.back_slot.clone();
        let widgets_c = widgets.clone();
        back_btn.connect_clicked(move |_| show_step(&widgets_c, previous));
        back_slot.append(&back_btn);
    }

    let name = format!("step-{id}");
    show_page(&widgets.pages, &page, &name);

    if step == OnboardingStep::Welcome {
        if let Some(callback) = motion::fade_in(brand.upcast_ref(), 80.0, 480.0) {
            animations.push(callback);
        }
        if let Some(callback) = motion::fade_in(widgets.progress.upcast_ref(), 1550.0, 400.0) {
            animations.push(callback);
        }
    } else {
        widgets.progress.set_opacity(1.0);
        let mut entrance: Vec<gtk4::Widget> = vec![brand.upcast()];
        entrance.extend(children(&body));
        entrance.push(actions.upcast());
        if let Some(callback) = motion::reveal(&entrance, 70.0, 420.0) {
            animations.push(callback);
        }
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
        "{current} / {total}",
        &[
            ("current", &(step.position() + 1).to_string()),
            ("total", &steps.len().to_string()),
        ],
    )
}

fn build_progress(progress: &GtkBox, step: OnboardingStep, scale: f64) {
    clear(progress);
    progress.update_property(&[
        gtk4::accessible::Property::Label(&step_caption(step)),
        gtk4::accessible::Property::Description(&t(step.label())),
    ]);
    let steps = OnboardingStep::all();
    let current = step.position();
    for (index, item) in steps.iter().enumerate() {
        let node = GtkBox::new(Orientation::Horizontal, 0);
        node.add_css_class("onboarding-progress-node");
        node.set_size_request(px(scale, 22.0), px(scale, 22.0));
        node.set_valign(Align::Center);
        let name = t(item.label());
        node.set_tooltip_text(Some(&name));
        node.update_property(&[gtk4::accessible::Property::Label(&name)]);

        if index < current {
            node.add_css_class("onboarding-progress-node-done");
            let check = gtk4::Image::from_icon_name(custom::CHECK_SYMBOLIC);
            check.set_pixel_size(px(scale, 12.0));
            check.set_hexpand(true);
            check.set_valign(Align::Center);
            node.append(&check);
        } else {
            let number = gtk4::Label::new(Some(&(index + 1).to_string()));
            number.set_hexpand(true);
            number.set_valign(Align::Center);
            node.append(&number);
        }
        if index == current {
            node.add_css_class("onboarding-progress-node-active");
        }
        progress.append(&node);

        if index + 1 < steps.len() {
            let connector = GtkBox::new(Orientation::Horizontal, 0);
            connector.add_css_class("onboarding-progress-connector");
            connector.set_size_request(px(scale, 18.0), px(scale, 1.0));
            connector.set_valign(Align::Center);
            if index < current {
                connector.add_css_class("onboarding-progress-connector-done");
            }
            progress.append(&connector);
        }
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

/// Lay out a step's actions: primary buttons first, then the secondary ones
/// beneath them in the order the step added them. Primary actions fill the form.
/// Running it again on an already laid-out row leaves it unchanged.
fn layout_footer_actions(actions: &GtkBox) {
    for child in children(actions) {
        child.set_halign(if child.has_css_class("settings-primary-btn") {
            Align::Fill
        } else {
            Align::Center
        });
        if !child.has_css_class("settings-primary-btn") {
            let last = actions.last_child();
            if last.as_ref() != Some(&child) {
                actions.reorder_child_after(&child, last.as_ref());
            }
        }
    }
}

/// Load the setup-specific surfaces after the editor's styles, with the
/// same DPI scaling used by the real editor controls in the preview.
pub(crate) fn install_css(scale: f64) {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&crate::css_scale::scale_px_lengths(
        include_str!("onboarding.css"),
        scale,
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 5,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduce the form's spacing without presenting or driving a window.
    pub(super) fn test_form(scale: f64, step: OnboardingStep) -> (GtkBox, GtkBox, GtkBox, Nav) {
        // Measure the form with the faces the app registers before it opens
        // onboarding. Without them the fixture falls back to whatever the
        // machine has installed, and a wider fallback can fail the fit checks
        // on a runner that has no DM Sans.
        static FONTS: std::sync::Once = std::sync::Once::new();
        FONTS.call_once(crate::platform::prepare_process);

        // Keep this fixture's CSS scoped to onboarding. Reinstalling the
        // shared Settings providers changes other GTK tests' cascade.
        install_css(scale);
        let page = GtkBox::new(Orientation::Vertical, 0);
        page.set_margin_top(px(scale, 24.0));
        page.set_margin_bottom(px(scale, 40.0));
        let brand = showcase::step_mark(step, scale, true);
        brand.set_margin_bottom(px(scale, 24.0));
        page.append(&brand);
        let body = GtkBox::new(Orientation::Vertical, 0);
        let actions = GtkBox::new(Orientation::Vertical, px(scale, 10.0));
        actions.add_css_class("onboarding-actions");
        actions.set_margin_top(px(scale, 28.0));
        page.append(&body);
        page.append(&actions);
        let root = GtkBox::new(Orientation::Vertical, 0);
        root.add_css_class("editor-root");
        root.add_css_class("onboarding-root");
        root.append(&page);
        let nav = Nav {
            scale,
            scope: StepScope {
                generation: Rc::new(Cell::new(0)),
                id: 0,
            },
            go: Rc::new(|_| {}),
            advance: Rc::new(|| {}),
            finish: Rc::new(|| {}),
            layout_footer: {
                let actions = actions.clone();
                Rc::new(move || layout_footer_actions(&actions))
            },
        };
        (root, body, actions, nav)
    }

    pub(super) fn assert_form_fits(root: &GtkBox, scale: f64) {
        let width = px(scale, PANE_WIDTH - 2.0 * PAGE_INSET);
        let minimum_width = root.measure(Orientation::Horizontal, -1).0;
        assert!(
            minimum_width <= width,
            "form needs {minimum_width}px width, only {width}px available"
        );
        let height = root.measure(Orientation::Vertical, width).0;
        // Leave the header, progress row and its margins outside the form.
        let available = px(
            scale,
            BASE_HEIGHT - crate::settings::ui_support::TITLE_BAR_HEIGHT - 8.0 - 64.0,
        );
        assert!(
            height <= available,
            "form needs {height}px, only {available}px available"
        );
    }

    #[test]
    fn every_step_fits_without_a_scrolling_form() {
        let _ = crate::test_support::with_gtk(|| {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                for step in OnboardingStep::all() {
                    let (root, body, actions, nav) = test_form(scale, step);
                    match step {
                        OnboardingStep::Welcome => {
                            let mut animations = Vec::new();
                            welcome::build(&body, &actions, &nav, &mut animations);
                        }
                        OnboardingStep::HowToUse => howto::build(&body, &actions, &nav),
                        OnboardingStep::Cloud => cloud::build(&body, &actions, &nav),
                        OnboardingStep::Complete => complete::build(&body, &actions, &nav),
                    }
                    layout_footer_actions(&actions);
                    assert_form_fits(&root, scale);
                }
            }
        });
    }

    #[test]
    fn typography_keeps_distinct_dpi_scaled_roles() {
        let _ = crate::test_support::with_gtk(|| {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let (root, body, actions, _) = test_form(scale, OnboardingStep::Welcome);
                let heading = ui::heading_parts(scale, "Welcome to ApexShot", "Supporting copy");
                body.append(&heading.block);
                let hint = gtk4::Label::new(Some("Helpful detail"));
                hint.add_css_class("settings-sub-option");
                body.append(&hint);
                let note = gtk4::Label::new(Some("No account needed."));
                note.add_css_class("onboarding-action-note");
                actions.append(&note);
                assert_form_fits(&root, scale);
                for (label, size) in [
                    (&heading.title, 28.0),
                    (&heading.copy, 15.0),
                    (&hint, 13.0),
                    (&note, 12.0),
                ] {
                    let font = label
                        .pango_context()
                        .font_description()
                        .expect("label font");
                    assert_eq!(
                        font.size(),
                        (size * scale * f64::from(gtk4::pango::SCALE)).round() as i32
                    );
                }
            }
        });
    }

    #[test]
    fn progress_tracks_forward_and_back_navigation_without_stale_states() {
        let _ = crate::test_support::with_gtk(|| {
            crate::icons::install();
            let display = gtk4::gdk::Display::default().expect("GTK display");
            let theme = gtk4::IconTheme::for_display(&display);
            for icon in [custom::CHECK_SYMBOLIC, custom::CHEVRON_LEFT_SYMBOLIC] {
                assert!(theme.has_icon(icon), "missing bundled icon: {icon}");
            }
            for scale in [1.0, 1.25, 1.5, 2.0] {
                for dark in [true, false] {
                    let (root, _, _, _) = test_form(scale, OnboardingStep::Welcome);
                    if !dark {
                        root.add_css_class("editor-theme-light");
                    }
                    let progress = GtkBox::new(Orientation::Horizontal, 0);
                    root.append(&progress);
                    for step in OnboardingStep::all()
                        .into_iter()
                        .chain(OnboardingStep::all().into_iter().rev())
                    {
                        build_progress(&progress, step, scale);
                        let parts = children(&progress);
                        assert_eq!(parts.len(), 7);
                        for (index, item) in OnboardingStep::all().iter().enumerate() {
                            let node = &parts[index * 2];
                            assert!(node.has_css_class("onboarding-progress-node"));
                            assert_eq!(
                                node.has_css_class("onboarding-progress-node-active"),
                                index == step.position()
                            );
                            assert_eq!(
                                node.has_css_class("onboarding-progress-node-done"),
                                index < step.position()
                            );
                            assert_eq!(
                                node.tooltip_text().as_deref(),
                                Some(t(item.label()).as_str())
                            );
                            assert!(!node.is_focusable());
                            let content = node.first_child().expect("step marker");
                            if index < step.position() {
                                let check =
                                    content.downcast::<gtk4::Image>().expect("completed check");
                                assert_eq!(
                                    check.icon_name().as_deref(),
                                    Some(custom::CHECK_SYMBOLIC)
                                );
                            } else {
                                let number =
                                    content.downcast::<gtk4::Label>().expect("step number");
                                assert_eq!(number.text().as_str(), (index + 1).to_string());
                            }
                            if index < 3 {
                                let connector = &parts[index * 2 + 1];
                                assert!(connector.has_css_class("onboarding-progress-connector"));
                                assert_eq!(
                                    connector.has_css_class("onboarding-progress-connector-done"),
                                    index < step.position()
                                );
                            }
                        }
                        assert!(
                            progress.measure(Orientation::Horizontal, -1).0 <= px(scale, 200.0)
                        );
                        assert!(progress.measure(Orientation::Vertical, -1).0 <= px(scale, 32.0));
                    }
                }
            }
        });
    }

    #[test]
    fn navigation_glyphs_keep_symbolic_filled_outlines() {
        for svg in [
            include_str!("../../../data/icons/check-symbolic.svg"),
            include_str!("../../../data/icons/chevron-left-symbolic.svg"),
        ] {
            assert!(svg.contains("fill=\"#222222\""));
            assert!(!svg.contains("stroke=") && !svg.contains("fill=\"none\""));
        }
    }

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

    #[test]
    fn form_actions_keep_primary_first_and_layout_is_idempotent() {
        use gtk4::prelude::*;
        use gtk4::{Align, Box as GtkBox, Button, Orientation};

        let _ = crate::test_support::with_gtk(|| {
            let actions = GtkBox::new(Orientation::Vertical, 0);
            let skip = Button::with_label("Skip");
            let primary = Button::with_label("Continue");
            primary.add_css_class("settings-primary-btn");
            let secondary = Button::with_label("Another action");
            actions.append(&skip);
            actions.append(&primary);
            actions.append(&secondary);

            for _ in 0..2 {
                super::layout_footer_actions(&actions);
                assert_eq!(actions.first_child(), Some(primary.clone().upcast()));
                assert_eq!(primary.next_sibling(), Some(skip.clone().upcast()));
                assert_eq!(actions.last_child(), Some(secondary.clone().upcast()));
                assert_eq!(primary.halign(), Align::Fill);
                assert_eq!(skip.halign(), Align::Center);
            }
        });
    }
}
