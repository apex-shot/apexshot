//! First-run onboarding for the Windows build.
//!
//! The Windows package requires an account, so the flow starts with a real
//! browser device-authorization handoff and stores the same account cache the
//! rest of the app reads. After the account is linked, four short setup pages
//! (appearance, screenshot format, recording defaults, and plan) lead to a
//! final checklist. Every choice writes the shared [`AppConfig`]; nothing here
//! invents a setting the Windows shell cannot honour.

mod account;
mod hero;
mod setup;
mod widgets;

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Application, ApplicationWindow, Box as GtkBox, CssProvider, Orientation, Overlay,
    Stack, StackTransitionType,
};

use crate::config::{config_path, load_config, save_config, AppConfig};
use crate::i18n::t;

use self::widgets::px;

/// Written only after the account is linked and the final page is completed.
const FLAG_FILE: &str = ".windows-onboarding-complete";

/// The window's design size at 100% display scaling.
const BASE_WIDTH: f64 = 1080.0;
const BASE_HEIGHT: f64 = 700.0;

/// What the caller should do after the onboarding window closes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub finished: bool,
    pub open_settings: bool,
}

/// One page of the flow. The account pages have no progress dots; the four
/// setup pages map to the four dots, and the final checklist closes the flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Account,
    Authorize,
    Appearance,
    Formats,
    Recording,
    Plan,
    Finish,
}

impl Step {
    fn name(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Authorize => "authorize",
            Self::Appearance => "appearance",
            Self::Formats => "formats",
            Self::Recording => "recording",
            Self::Plan => "plan",
            Self::Finish => "finish",
        }
    }

    fn order(self) -> usize {
        match self {
            Self::Account => 0,
            Self::Authorize => 1,
            Self::Appearance => 2,
            Self::Formats => 3,
            Self::Recording => 4,
            Self::Plan => 5,
            Self::Finish => 6,
        }
    }

    /// Zero-based dot index, or `None` for the account pages and the final
    /// page, which draw no progress indicator.
    fn dot_index(self) -> Option<usize> {
        match self {
            Self::Appearance => Some(0),
            Self::Formats => Some(1),
            Self::Recording => Some(2),
            Self::Plan => Some(3),
            _ => None,
        }
    }

    fn next(self) -> Option<Self> {
        match self {
            Self::Account => Some(Self::Authorize),
            Self::Authorize => Some(Self::Appearance),
            Self::Appearance => Some(Self::Formats),
            Self::Formats => Some(Self::Recording),
            Self::Recording => Some(Self::Plan),
            Self::Plan => Some(Self::Finish),
            Self::Finish => None,
        }
    }

    fn prev(self) -> Option<Self> {
        match self {
            Self::Account => None,
            Self::Authorize => Some(Self::Account),
            Self::Appearance => None,
            Self::Formats => Some(Self::Appearance),
            Self::Recording => Some(Self::Formats),
            Self::Plan => Some(Self::Recording),
            Self::Finish => Some(Self::Plan),
        }
    }
}

/// Shared state behind one onboarding window. Cloneable; every clone points at
/// the same window.
#[derive(Clone)]
pub(crate) struct Shell {
    inner: Rc<ShellInner>,
}

struct ShellInner {
    window: ApplicationWindow,
    stack: Stack,
    root: GtkBox,
    scale: f64,
    config: RefCell<AppConfig>,
    step: Cell<Step>,
    outcome: Rc<RefCell<Outcome>>,
    /// Bumped on every page change; background work compares it and stops.
    generation: Arc<AtomicU64>,
    /// The action Enter should run on the current page.
    primary: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Shell {
    pub(crate) fn window(&self) -> &ApplicationWindow {
        &self.inner.window
    }

    pub(crate) fn scale(&self) -> f64 {
        self.inner.scale
    }

    pub(crate) fn config(&self) -> AppConfig {
        self.inner.config.borrow().clone()
    }

    pub(crate) fn update_config(&self, update: impl FnOnce(&mut AppConfig)) {
        update(&mut self.inner.config.borrow_mut());
        self.persist();
    }

    pub(crate) fn step(&self) -> Step {
        self.inner.step.get()
    }

    /// The generation a page's background work belongs to.
    pub(crate) fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::SeqCst)
    }

    /// A thread-safe handle to the page generation, for workers that must stop
    /// when the person navigates away.
    pub(crate) fn generation_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.inner.generation)
    }

    pub(crate) fn set_primary(&self, action: Rc<dyn Fn()>) {
        *self.inner.primary.borrow_mut() = Some(action);
    }

    pub(crate) fn primary(&self) -> Option<Rc<dyn Fn()>> {
        self.inner.primary.borrow().clone()
    }

    /// Persist the shared config and its shell mirror. A write failure is
    /// reported but never blocks the flow: the next page still works.
    pub(crate) fn persist(&self) {
        let config = self.config();
        if let Err(error) = save_config(&config) {
            eprintln!("[onboarding] could not save config: {error}");
        }
        if let Err(error) = crate::settings::actions::write_shell_settings(&config) {
            eprintln!("[onboarding] could not write shell settings: {error}");
        }
    }

    /// Reload the config after a worker changed it (a completed device login
    /// stores the tokens and account cache).
    pub(crate) fn reload_config(&self) {
        *self.inner.config.borrow_mut() = load_config().sanitized();
    }

    /// Re-apply the selected theme to the live window.
    pub(crate) fn apply_theme(&self) {
        let theme = self.config().ui_theme;
        let dark = match theme.as_str() {
            "dark" => true,
            "light" => false,
            _ => crate::settings::windowing::prefers_dark_glass_theme(),
        };
        if dark {
            self.inner.root.remove_css_class("editor-theme-light");
        } else {
            self.inner.root.add_css_class("editor-theme-light");
        }
    }

    pub(crate) fn go(&self, step: Step) {
        let forward = step.order() > self.step().order();
        self.inner.step.set(step);
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        self.inner.primary.replace(None);

        let page = match step {
            Step::Account => account::build_account(self),
            Step::Authorize => account::build_authorize(self),
            Step::Appearance => setup::build_appearance(self),
            Step::Formats => setup::build_formats(self),
            Step::Recording => setup::build_recording(self),
            Step::Plan => setup::build_plan(self),
            Step::Finish => setup::build_finish(self),
        };
        page.set_hexpand(true);
        page.set_vexpand(true);

        let name = step.name();
        if let Some(previous) = self.inner.stack.child_by_name(name) {
            self.inner.stack.remove(&previous);
        }
        self.inner.stack.set_transition_type(if forward {
            StackTransitionType::SlideLeft
        } else {
            StackTransitionType::SlideRight
        });
        self.inner.stack.add_named(&page, Some(name));
        self.inner.stack.set_visible_child_name(name);

        // Drop pages the transition has finished with, unless they became the
        // current page again in the meantime.
        let stack = self.inner.stack.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(360), move || {
            let visible = stack
                .visible_child_name()
                .map(|name| name.to_string())
                .unwrap_or_default();
            let mut child = stack.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                let own_name = stack
                    .page(&widget)
                    .name()
                    .map(|name| name.to_string())
                    .unwrap_or_default();
                if own_name != visible {
                    stack.remove(&widget);
                }
            }
        });
    }

    /// Advance to the next page, if there is one.
    pub(crate) fn advance(&self) {
        if let Some(next) = self.step().next() {
            self.go(next);
        }
    }

    /// Complete the flow. The flag is best-effort: a failed write must not
    /// trap the user in an onboarding that cannot end.
    pub(crate) fn finish(&self, open_settings: bool) {
        if let Err(error) = mark_complete() {
            eprintln!("[onboarding] could not write the completion flag: {error}");
        }
        let mut outcome = self.inner.outcome.borrow_mut();
        outcome.finished = true;
        outcome.open_settings = open_settings;
        drop(outcome);
        self.inner.window.close();
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
    install_css();

    let config = load_config().sanitized();
    let scale = crate::platform::ui_scale();
    let dark = match config.ui_theme.as_str() {
        "dark" => true,
        "light" => false,
        _ => crate::settings::windowing::prefers_dark_glass_theme(),
    };

    let window = ApplicationWindow::builder()
        .application(app)
        .title(t("Set up ApexShot"))
        .icon_name(crate::app_identity::icon_name())
        .default_width((BASE_WIDTH * scale).round() as i32)
        .default_height((BASE_HEIGHT * scale).round() as i32)
        .build();
    window.set_decorated(false);
    window.add_css_class("editor-window");

    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class("onboarding-root");
    if !dark {
        root.add_css_class("editor-theme-light");
    }

    let stack = Stack::new();
    stack.add_css_class("onboarding-stack");
    stack.set_transition_type(StackTransitionType::SlideLeft);
    stack.set_transition_duration(280);
    stack.set_vexpand(true);
    stack.set_hexpand(true);
    root.append(&stack);

    let shell = Shell {
        inner: Rc::new(ShellInner {
            window: window.clone(),
            stack,
            root: root.clone(),
            scale,
            config: RefCell::new(config),
            step: Cell::new(Step::Account),
            outcome,
            generation: Arc::new(AtomicU64::new(0)),
            primary: RefCell::new(None),
        }),
    };

    let overlay = Overlay::new();
    overlay.set_child(Some(&root));
    if !dark {
        overlay.add_css_class("editor-theme-light");
    }
    overlay.add_overlay(&build_title_bar(&shell));
    window.set_child(Some(&overlay));

    let keys = gtk4::EventControllerKey::new();
    {
        let shell = shell.clone();
        keys.connect_key_pressed(move |_, key, _, _| match key {
            gtk4::gdk::Key::Escape => {
                shell.window().close();
                glib::Propagation::Stop
            }
            gtk4::gdk::Key::Return | gtk4::gdk::Key::KP_Enter => {
                if let Some(action) = shell.primary() {
                    action();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
            _ => glib::Propagation::Proceed,
        });
    }
    window.add_controller(keys);

    shell.go(Step::Account);
    window.present();
}

/// The flat caption row the Windows windows use, overlaid on the content so the
/// close and minimise targets sit at the same top-right corner as Settings.
fn build_title_bar(shell: &Shell) -> GtkBox {
    let bar = GtkBox::new(Orientation::Horizontal, 0);
    bar.add_css_class("onboarding-titlebar");
    bar.set_size_request(-1, px(shell.scale(), 36.0));
    bar.set_halign(Align::Fill);
    bar.set_valign(Align::Start);

    let drag = GtkBox::new(Orientation::Horizontal, 0);
    drag.set_hexpand(true);
    drag.set_halign(Align::Fill);
    bar.append(&drag);

    let minimize = crate::settings::ui_support::window_control_button(
        crate::settings::ui_support::WindowControl::Minimize,
        &t("Minimize"),
        shell.scale(),
    );
    let window = shell.window().clone();
    minimize.connect_clicked(move |_| window.minimize());
    minimize.set_valign(Align::Center);
    bar.append(&minimize);

    let close = crate::settings::ui_support::window_control_button(
        crate::settings::ui_support::WindowControl::Close,
        &t("Close"),
        shell.scale(),
    );
    let window = shell.window().clone();
    close.connect_clicked(move |_| window.close());
    close.set_valign(Align::Center);
    bar.append(&close);

    crate::settings::windowing::install_window_drag(&drag, shell.window());
    bar
}

/// Load the onboarding stylesheet: fixed colours and layout, plus the sizes
/// that scale with the display.
fn install_css() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_data(&format!("{ONBOARDING_CSS}\n{}", scaled_css()));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

const ONBOARDING_CSS: &str = include_str!("style.css");

/// Font sizes are the one thing GTK CSS cannot express relative to the manual
/// Windows DPI scale, so every text class gets its size injected here.
fn scaled_css() -> String {
    let scale = crate::platform::ui_scale();
    let px = |value: f64| (value * scale).round() as i32;
    let type_scale: &[(&str, f64)] = &[
        ("ob-brand-name", 14.5),
        ("ob-headline", 39.0),
        ("ob-lead", 14.5),
        ("ob-title", 31.0),
        ("ob-subtitle", 14.5),
        ("ob-card-title", 14.5),
        ("ob-card-caption", 12.5),
        ("ob-choice-title", 14.0),
        ("ob-choice-caption", 12.0),
        ("ob-feature", 12.5),
        ("ob-button", 13.5),
        ("ob-button-small", 12.5),
        ("ob-fineprint", 11.5),
        ("ob-code", 21.0),
        ("ob-step-caption", 11.0),
        ("ob-plan-name", 15.0),
        ("ob-plan-price", 34.0),
        ("ob-plan-unit", 12.0),
        ("ob-plan-feature", 12.5),
        ("ob-status", 12.5),
        ("ob-email", 12.0),
        ("ob-badge", 10.5),
        ("ob-check", 12.0),
        ("ob-back-glyph", 22.0),
        ("ob-return-glyph", 12.0),
    ];
    let mut css = String::new();
    for (class, size) in type_scale {
        css.push_str(&format!(".{class} {{ font-size:{}px; }}\n", px(*size)));
    }
    let rule = |selector: &str, body: String| format!(".{selector} {{ {body} }}\n");
    css.push_str(&rule(
        "ob-primary",
        format!(
            "min-height:{}px; padding:0 {}px; border-radius:{}px;",
            px(46.0),
            px(22.0),
            px(10.0)
        ),
    ));
    css.push_str(&rule(
        "ob-secondary",
        format!(
            "min-height:{}px; padding:0 {}px; border-radius:{}px;",
            px(42.0),
            px(20.0),
            px(10.0)
        ),
    ));
    css.push_str(&rule(
        "ob-ghost",
        format!(
            "min-height:{}px; padding:0 {}px; border-radius:{}px;",
            px(36.0),
            px(16.0),
            px(9.0)
        ),
    ));
    css.push_str(&rule(
        "ob-next",
        format!(
            "min-height:{}px; min-width:{}px; padding:0 {}px; border-radius:{}px;",
            px(42.0),
            px(96.0),
            px(20.0),
            px(10.0)
        ),
    ));
    css.push_str(&rule(
        "ob-back",
        format!(
            "min-height:{}px; padding:0 {}px; border-radius:{}px;",
            px(38.0),
            px(14.0),
            px(9.0)
        ),
    ));
    css.push_str(&rule(
        "ob-code-box",
        format!(
            "min-width:{}px; min-height:{}px; border-radius:{}px;",
            px(44.0),
            px(44.0),
            px(12.0)
        ),
    ));
    css.push_str(&rule(
        "ob-choice-card",
        format!("padding:{}px; border-radius:{}px;", px(16.0), px(14.0)),
    ));
    css.push_str(&rule(
        "ob-feature-panel",
        format!(
            "padding:{}px {}px; border-radius:{}px;",
            px(16.0),
            px(18.0),
            px(14.0)
        ),
    ));
    css.push_str(&rule(
        "ob-settings-card",
        format!(
            "padding:{}px {}px; border-radius:{}px;",
            px(16.0),
            px(18.0),
            px(14.0)
        ),
    ));
    css.push_str(&rule(
        "ob-plan-card",
        format!(
            "padding:{}px {}px; border-radius:{}px; min-width:{}px;",
            px(26.0),
            px(22.0),
            px(18.0),
            px(340.0)
        ),
    ));
    css.push_str(&rule(
        "ob-split-left",
        format!(
            "padding:{}px {}px {}px {}px;",
            px(42.0),
            px(44.0),
            px(40.0),
            px(54.0)
        ),
    ));
    css.push_str(&rule("ob-centered", format!("padding-top:{}px;", px(58.0))));
    css.push_str(&rule(
        "ob-footer",
        format!(
            "padding:{}px {}px {}px {}px; min-height:{}px;",
            px(18.0),
            px(28.0),
            px(24.0),
            px(28.0),
            px(76.0)
        ),
    ));
    css.push_str(&rule("ob-code-row", format!("margin-top:{}px;", px(26.0))));
    css.push_str(&rule("ob-copy-row", format!("margin-top:{}px;", px(19.0))));
    css.push_str(&rule(
        "ob-plan-price-row",
        format!("margin-top:{}px;", px(22.0)),
    ));
    css
}

/// A state line used by the account pages and the final checklist.
pub(crate) fn status_line() -> gtk4::Label {
    let label = gtk4::Label::new(None);
    label.add_css_class("ob-status");
    label.set_halign(Align::Start);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label
}

pub(crate) fn report_error(label: &gtk4::Label, message: &str) {
    label.remove_css_class("ob-status-success");
    label.add_css_class("ob-status-error");
    label.set_text(message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::hero::HeroScene;

    #[test]
    fn the_setup_pages_map_to_the_four_progress_dots() {
        assert_eq!(Step::Appearance.dot_index(), Some(0));
        assert_eq!(Step::Formats.dot_index(), Some(1));
        assert_eq!(Step::Recording.dot_index(), Some(2));
        assert_eq!(Step::Plan.dot_index(), Some(3));
        assert_eq!(Step::Account.dot_index(), None);
        assert_eq!(Step::Authorize.dot_index(), None);
        assert_eq!(Step::Finish.dot_index(), None);
    }

    #[test]
    fn the_flow_advances_in_order_and_never_goes_back_into_sign_in() {
        let mut step = Step::Account;
        while let Some(next) = step.next() {
            if step == Step::Authorize {
                // Appearance is the first page after the account is linked;
                // Back must not reopen the browser handoff.
                assert_eq!(next.prev(), None);
            } else {
                assert_eq!(next.prev(), Some(step));
            }
            step = next;
        }
        assert_eq!(step, Step::Finish);
    }

    #[test]
    fn onboarding_css_parses_without_gtk_errors() {
        let Some((errors, bogus_errors)) = crate::test_support::with_gtk(|| {
            let provider = CssProvider::new();
            let errors = std::rc::Rc::new(RefCell::new(Vec::new()));
            {
                let errors = std::rc::Rc::clone(&errors);
                provider.connect_parsing_error(move |_, section, error| {
                    errors
                        .borrow_mut()
                        .push(format!("{}: {}", section.to_str(), error.message()));
                });
            }
            provider.load_from_data(&format!("{ONBOARDING_CSS}\n{}", scaled_css()));

            let bogus = CssProvider::new();
            let bogus_errors = std::rc::Rc::new(RefCell::new(Vec::new()));
            {
                let bogus_errors = std::rc::Rc::clone(&bogus_errors);
                bogus.connect_parsing_error(move |_, _, error| {
                    bogus_errors.borrow_mut().push(error.message().to_string());
                });
            }
            bogus.load_from_data(".bogus { definitely-not-a-property: 1; }");

            let parsed: Vec<String> = errors.borrow().clone();
            let reported: Vec<String> = bogus_errors.borrow().clone();
            (parsed, reported)
        }) else {
            eprintln!("skipping: no display available");
            return;
        };

        assert!(
            errors.is_empty(),
            "onboarding CSS failed to parse: {errors:#?}"
        );
        assert!(
            !bogus_errors.is_empty(),
            "parsing-error signal is not firing; the CSS check is vacuous"
        );
    }

    #[test]
    fn hero_scenes_start_from_the_stored_recording_defaults() {
        let mut config = AppConfig {
            rec_webcam_enabled: false,
            rec_mic: true,
            ..AppConfig::default()
        };
        assert_eq!(HeroScene::for_config(&config), HeroScene::ScreenAndVoice);
        config.rec_webcam_enabled = true;
        assert_eq!(HeroScene::for_config(&config), HeroScene::CameraAndVoice);
        config.rec_webcam_enabled = false;
        config.rec_mic = false;
        assert_eq!(HeroScene::for_config(&config), HeroScene::ScreenOnly);
    }
}
