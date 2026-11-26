//! First-run onboarding for the Linux build.
//!
//! The steps live in one undecorated window: the step's page in a centred
//! column under the app mark, with Back and the window controls floating over
//! the pane and the progress rail at its foot. Finishing writes
//! `~/.config/apexshot/.onboarding_complete`, which the entry point checks
//! before it opens Settings.

use gtk4::{
    glib, prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button, Label,
    Orientation, Overlay, Stack, StackTransitionType, Widget,
};
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

mod blur_text;
mod cloud;
mod complete;
pub mod extensions;
mod howto;
mod motion;
pub(crate) mod showcase;
pub(crate) mod ui;
mod welcome;

use crate::capture::editor::window::icon_names::custom;
use crate::i18n::{self, t, tfmt};
use crate::settings::ui_support::{install_settings_css, traffic_light_button};

const ONBOARDING_FLAG_FILE: &str = ".onboarding_complete";

/// The window box.
const BASE_WIDTH: i32 = 820;
const BASE_HEIGHT: i32 = 760;

/// Width of the pane that holds the step's page.
const PANE_WIDTH: i32 = 480;

/// Inset of the page from the pane's edges.
const PAGE_INSET: i32 = 48;

/// The one gap between a page's sections, so every step reads with the same
/// rhythm. Smaller sub-gaps inside a section stay local to that section.
pub(super) const SECTION_GAP: i32 = 18;

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
    /// Re-apply the actions-column layout after a view re-renders its own
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
    status: Label,
    dark: bool,
    generation: Rc<Cell<u64>>,
    animations: Rc<RefCell<Vec<gtk4::TickCallbackId>>>,
    current_step: Rc<Cell<Option<OnboardingStep>>>,
}

thread_local! {
    /// The open onboarding window, so a `cloud/connect` link can switch it to
    /// the Cloud step instead of opening a second window.
    static ONBOARDING_WIDGETS: RefCell<Option<OnboardingWidgets>> = const { RefCell::new(None) };
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
    run_onboarding(false)
}

/// Show onboarding at the Cloud step, with its device-code view already open
/// when no account is linked. The website's Cloud connect link lands here.
pub fn show_onboarding_for_cloud_connect() -> anyhow::Result<()> {
    run_onboarding(true)
}

fn run_onboarding(cloud_connect: bool) -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id(crate::app_identity::app_id())
        .build();

    install_cloud_connect_action(&app);
    let pending_cloud_connect = Rc::new(Cell::new(cloud_connect));

    app.connect_activate(move |application| {
        let wants_connect = pending_cloud_connect.replace(false);
        let windows = application.windows();
        if let Some(existing_window) = windows.first() {
            existing_window.present();
            if wants_connect {
                switch_to_cloud_connect();
            }
            return;
        }

        build_onboarding_window_at(application, wants_connect);
    });

    if cloud_connect && hand_cloud_connect_to_primary(&app) {
        return Ok(());
    }

    crate::graceful_close::close_windows_on_sigterm(&app, || true);
    let _ = app.run_with_args::<String>(&[]);
    Ok(())
}

fn install_cloud_connect_action(app: &Application) {
    let action = gtk4::gio::SimpleAction::new("cloud-connect", None);
    let app_weak = app.downgrade();
    action.connect_activate(move |_, _| {
        if let Some(app) = app_weak.upgrade() {
            open_onboarding_for_cloud_connect(&app);
        }
    });
    app.add_action(&action);
}

fn open_onboarding_for_cloud_connect(app: &Application) {
    if app.windows().first().is_some() {
        switch_to_cloud_connect();
    } else {
        build_onboarding_window_at(app, true);
    }
}

fn switch_to_cloud_connect() {
    let widgets = ONBOARDING_WIDGETS.with(|slot| slot.borrow().clone());
    let Some(widgets) = widgets else {
        return;
    };
    widgets.window.present();
    // A live device-code view is already what the link asks for.
    if widgets.current_step.get() == Some(OnboardingStep::Cloud) && cloud::connect_in_progress() {
        return;
    }
    show_step_inner(&widgets, OnboardingStep::Cloud, true);
}

/// Hand the action to an instance that already owns the application id.
/// Returns true when this process can exit.
fn hand_cloud_connect_to_primary(app: &Application) -> bool {
    if app.register(None::<&gtk4::gio::Cancellable>).is_err() {
        return false;
    }
    if !app.is_remote() {
        return false;
    }
    app.activate_action("cloud-connect", None);
    if let Some(connection) = app.dbus_connection() {
        let _ = connection.flush_sync(None::<&gtk4::gio::Cancellable>);
    }
    true
}

fn build_onboarding_window_at(app: &Application, cloud_connect: bool) {
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
        .default_width(BASE_WIDTH)
        .default_height(BASE_HEIGHT)
        .decorated(false)
        .build();
    window.add_css_class("editor-window");
    window.add_css_class("onboarding-window");

    let close_animations = Rc::clone(&animations);
    window.connect_close_request(move |_| {
        cancel_animations(&close_animations);
        glib::Propagation::Proceed
    });

    window.connect_close_request(|_| {
        ONBOARDING_WIDGETS.with(|slot| *slot.borrow_mut() = None);
        glib::Propagation::Proceed
    });

    let initial_step = if cloud_connect {
        OnboardingStep::Cloud
    } else {
        OnboardingStep::Welcome
    };
    let widgets = install_at(&window, animations, initial_step, cloud_connect);
    ONBOARDING_WIDGETS.with(|slot| *slot.borrow_mut() = Some(widgets));
    window.present();
}

#[cfg(test)]
fn install(
    window: &ApplicationWindow,
    animations: Rc<RefCell<Vec<gtk4::TickCallbackId>>>,
) -> OnboardingWidgets {
    install_at(window, animations, OnboardingStep::Welcome, false)
}

/// Build the whole onboarding surface into `window` and show `initial_step`.
fn install_at(
    window: &ApplicationWindow,
    animations: Rc<RefCell<Vec<gtk4::TickCallbackId>>>,
    initial_step: OnboardingStep,
    start_cloud_connect: bool,
) -> OnboardingWidgets {
    cancel_animations(&animations);
    let dark = crate::capture::editor::ui_support::prefers_dark_glass_theme();
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.add_css_class("editor-root");
    root.add_css_class("onboarding-root");
    root.add_css_class(if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });

    // The pane is the drag handle: the page inside it carries every control,
    // and Back and the window controls float beside it.
    let pane = GtkBox::new(Orientation::Vertical, 0);
    pane.add_css_class("onboarding-pane");
    pane.set_size_request(PANE_WIDTH, -1);
    pane.set_hexpand(false);
    pane.set_vexpand(true);
    pane.set_halign(Align::Center);
    install_pane_drag(&pane, window);

    let back_slot = GtkBox::new(Orientation::Horizontal, 0);
    back_slot.set_halign(Align::Start);
    back_slot.set_valign(Align::Start);

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
    pane.append(&pages);

    // Actions belong to the form, not the window's foot; show_step fills the
    // box after each page builds its own buttons.
    let actions = GtkBox::new(Orientation::Vertical, 10);
    actions.add_css_class("onboarding-actions");
    actions.set_margin_top(28);

    let status = Label::new(None);
    status.add_css_class("onboarding-status-error");
    status.set_wrap(true);
    status.set_max_width_chars(44);
    status.set_halign(Align::Center);
    status.set_xalign(0.5);
    status.set_justify(gtk4::Justification::Center);
    status.set_visible(false);

    let progress = GtkBox::new(Orientation::Horizontal, 0);
    progress.add_css_class("onboarding-progress");
    progress.set_halign(Align::Center);

    let footer = GtkBox::new(Orientation::Vertical, 10);
    footer.add_css_class("onboarding-footer");
    footer.set_margin_start(PAGE_INSET);
    footer.set_margin_end(PAGE_INSET);
    footer.set_margin_top(16);
    footer.set_margin_bottom(24);
    footer.append(&status);
    footer.append(&progress);
    pane.append(&footer);

    let overlay = Overlay::new();
    overlay.set_vexpand(true);
    overlay.set_child(Some(&pane));
    overlay.add_overlay(&back_slot);
    overlay.add_overlay(&window_controls(window));
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
        generation: Rc::new(Cell::new(0)),
        animations,
        current_step: Rc::new(Cell::new(None)),
    };
    show_step_inner(&widgets, initial_step, start_cloud_connect);
    widgets
}

/// Minimize and Close in the window's top-right corner, floating over the
/// pane so the form itself stays free of chrome.
pub(crate) fn window_controls(window: &ApplicationWindow) -> GtkBox {
    let min_btn = traffic_light_button("traffic-light-yellow", &t("Minimize"));
    min_btn.remove_css_class("recent-captures-wm-btn");
    min_btn.add_css_class("recording-editor-traffic-btn");
    min_btn.set_valign(Align::Center);
    let win_clone = window.clone();
    min_btn.connect_clicked(move |_| win_clone.minimize());

    let close_btn = traffic_light_button("traffic-light-red", &t("Close"));
    close_btn.remove_css_class("recent-captures-wm-btn");
    close_btn.remove_css_class("recent-captures-wm-close");
    close_btn.add_css_class("recording-editor-traffic-btn");
    close_btn.set_valign(Align::Center);
    let win_clone = window.clone();
    close_btn.connect_clicked(move |_| win_clone.close());

    let controls = GtkBox::new(Orientation::Horizontal, 6);
    controls.add_css_class("settings-window-controls");
    controls.add_css_class("onboarding-toolbar");
    controls.set_halign(Align::End);
    controls.set_valign(Align::Start);
    controls.set_margin_top(6);
    controls.set_margin_end(8);
    controls.append(&min_btn);
    controls.append(&close_btn);
    controls
}

/// Move the window from a press on the pane's empty space. A press that lands
/// on a control (or anything inside one) is left alone: starting a move on
/// press takes the pointer grab, so the control would never see its release.
pub(crate) fn install_pane_drag(pane: &GtkBox, window: &ApplicationWindow) {
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(1);
    let window = window.downgrade();
    gesture.connect_pressed(move |gesture, _, x, y| {
        let Some(pane) = gesture.widget() else {
            return;
        };
        if press_hits_control(&pane, x, y) {
            return;
        }
        let Some(window) = window.upgrade() else {
            return;
        };
        let Some(event) = gesture.current_event() else {
            return;
        };
        let Some(device) = event.device() else {
            return;
        };
        let Some(toplevel) = window
            .surface()
            .and_then(|surface| surface.downcast::<gtk4::gdk::Toplevel>().ok())
        else {
            return;
        };
        toplevel.begin_move(&device, gesture.current_button() as i32, x, y, event.time());
    });
    pane.add_controller(gesture);
}

/// True when the widget under (`x`, `y`) in `pane` is, or sits inside, a
/// control that handles its own clicks.
fn press_hits_control(pane: &Widget, x: f64, y: f64) -> bool {
    let mut current = pane.pick(x, y, gtk4::PickFlags::DEFAULT);
    while let Some(widget) = current {
        if &widget == pane {
            return false;
        }
        if widget.is::<Button>()
            || widget.is::<gtk4::Editable>()
            || widget.is::<gtk4::DropDown>()
            || widget.is::<gtk4::Scrollbar>()
            || (widget.is::<Label>()
                && widget
                    .downcast_ref::<Label>()
                    .is_some_and(|l| l.is_selectable()))
        {
            return true;
        }
        current = widget.parent();
    }
    false
}

fn show_step(widgets: &OnboardingWidgets, step: OnboardingStep) {
    show_step_inner(widgets, step, false);
}

fn show_step_inner(widgets: &OnboardingWidgets, step: OnboardingStep, start_cloud_connect: bool) {
    widgets.current_step.set(Some(step));
    widgets.cancel_animations();
    let id = widgets.generation.get() + 1;
    widgets.generation.set(id);
    widgets.window.set_title(Some(&t("ApexShot Setup")));
    widgets.status.set_visible(false);
    clear(&widgets.back_slot);
    // With no Back button the slot is hidden so the controls start at the
    // window edge instead of leaving a gap.
    widgets.back_slot.set_visible(step.prev().is_some());
    build_progress(&widgets.progress, step);

    // A short form sits at the pane's optical centre.
    let page = GtkBox::new(Orientation::Vertical, 0);
    page.add_css_class("onboarding-page");
    page.set_hexpand(true);
    page.set_halign(Align::Fill);
    page.set_margin_start(PAGE_INSET);
    page.set_margin_end(PAGE_INSET);
    page.set_valign(Align::Center);
    // Clear the floating Back button and window controls.
    page.set_margin_top(48);
    page.set_margin_bottom(24);

    let brand = showcase::step_mark(step, widgets.dark);
    brand.set_margin_bottom(20);
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
        OnboardingStep::Cloud => {
            if start_cloud_connect {
                cloud::build_connecting(&body, &actions, &nav);
            } else {
                cloud::build(&body, &actions, &nav);
            }
        }
        OnboardingStep::ChromeExtension => extensions::build_chrome(&body, &actions, &nav),
        OnboardingStep::Complete => complete::build(&body, &actions, &nav),
    }
    layout_footer_actions(&actions);
    page.append(&actions);

    if let Some(previous) = step.prev() {
        let back_btn = Button::new();
        back_btn.add_css_class("onboarding-text-button");
        let back_content = GtkBox::new(Orientation::Horizontal, 4);
        let back_icon = gtk4::Image::from_icon_name(custom::CHEVRON_LEFT_SYMBOLIC);
        back_icon.set_pixel_size(14);
        back_content.append(&back_icon);
        back_content.append(&Label::new(Some(&t("Back"))));
        back_btn.set_child(Some(&back_content));
        back_btn.set_valign(Align::Center);
        back_btn.set_margin_start(12);
        back_btn.set_margin_top(8);
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
        let mut entrance: Vec<Widget> = vec![brand.upcast()];
        entrance.extend(children(&body));
        entrance.push(actions.upcast());
        if let Some(callback) = motion::reveal(&entrance, 70.0, 420.0) {
            animations.push(callback);
        }
    }
    widgets.animations.replace(animations);
}

fn show_page(pages: &Stack, page: &impl IsA<Widget>, name: &str) {
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
    progress.update_property(&[
        gtk4::accessible::Property::Label(&step_caption(step)),
        gtk4::accessible::Property::Description(&t(step.label())),
    ]);
    let steps = OnboardingStep::all();
    let current = step.position();
    for (index, item) in steps.iter().enumerate() {
        let node = GtkBox::new(Orientation::Horizontal, 0);
        node.add_css_class("onboarding-progress-node");
        node.set_size_request(22, 22);
        node.set_valign(Align::Center);
        let name = t(item.label());
        node.set_tooltip_text(Some(&name));
        node.update_property(&[gtk4::accessible::Property::Label(&name)]);

        if index < current {
            node.add_css_class("onboarding-progress-node-done");
            let check = gtk4::Image::from_icon_name(custom::CHECK_SYMBOLIC);
            check.set_pixel_size(12);
            check.set_hexpand(true);
            check.set_valign(Align::Center);
            node.append(&ringed(&check, false));
        } else {
            let number = Label::new(Some(&(index + 1).to_string()));
            number.set_halign(Align::Center);
            number.set_xalign(0.5);
            number.set_hexpand(true);
            number.set_valign(Align::Center);
            node.append(&ringed(&number, index == current));
        }
        if index == current {
            node.add_css_class("onboarding-progress-node-active");
        }
        progress.append(&node);

        if index + 1 < steps.len() {
            let connector = GtkBox::new(Orientation::Horizontal, 0);
            connector.add_css_class("onboarding-progress-connector");
            connector.set_size_request(18, 1);
            connector.set_valign(Align::Center);
            if index < current {
                connector.add_css_class("onboarding-progress-connector-done");
            }
            progress.append(&connector);
        }
    }
}

/// Put a step's marker inside its ring. The ring is stroked with cairo as a
/// true circle: a CSS border or inset shadow on a 22px pill leaves brighter
/// specks where GTK joins the rounded sides. Its colour comes from CSS.
fn ringed(content: &impl IsA<Widget>, active: bool) -> Overlay {
    let ring = gtk4::DrawingArea::new();
    ring.add_css_class("onboarding-progress-ring");
    ring.set_can_target(false);
    ring.set_draw_func(move |widget, cr, width, height| {
        let color = widget.color();
        let (r, g, b, a) = (
            f64::from(color.red()),
            f64::from(color.green()),
            f64::from(color.blue()),
            f64::from(color.alpha()),
        );
        let (cx, cy) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
        let radius = cx.min(cy) - 0.5;
        cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
        if active {
            cr.set_source_rgba(r, g, b, a * 0.11);
            let _ = cr.fill_preserve();
        }
        cr.set_source_rgba(r, g, b, a);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
    });
    let overlay = Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_child(Some(content));
    overlay.add_overlay(&ring);
    overlay
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

/// Lay out a step's actions: primary buttons first, filling the form; then
/// every secondary button side by side on one centred row of quiet text
/// buttons; then any notes. Running it again leaves the column unchanged.
fn layout_footer_actions(actions: &GtkBox) {
    // Unpack a row left by an earlier pass so the order is rebuilt from scratch.
    for child in children(actions) {
        if child.has_css_class("onboarding-secondary-row") {
            if let Ok(row) = child.clone().downcast::<GtkBox>() {
                let mut anchor = child.clone();
                for item in children(&row) {
                    row.remove(&item);
                    if item.has_css_class("onboarding-secondary-dot") {
                        continue;
                    }
                    actions.insert_child_after(&item, Some(&anchor));
                    anchor = item;
                }
            }
            actions.remove(&child);
        }
    }

    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    let mut notes = Vec::new();
    for child in children(actions) {
        actions.remove(&child);
        if child.has_css_class("settings-primary-btn") {
            primary.push(child);
        } else if child.is::<Button>() {
            secondary.push(child);
        } else {
            notes.push(child);
        }
    }

    for button in &primary {
        button.set_halign(Align::Fill);
        actions.append(button);
    }
    if !secondary.is_empty() {
        let row = GtkBox::new(Orientation::Horizontal, 6);
        row.add_css_class("onboarding-secondary-row");
        row.set_halign(Align::Center);
        for (index, button) in secondary.iter().enumerate() {
            if index > 0 {
                let dot = Label::new(Some("·"));
                dot.add_css_class("onboarding-secondary-dot");
                dot.add_css_class("onboarding-action-note");
                row.append(&dot);
            }
            button.remove_css_class("secondary-settings-button");
            button.add_css_class("onboarding-text-button");
            button.set_halign(Align::Center);
            row.append(button);
        }
        actions.append(&row);
    }
    for note in &notes {
        note.set_halign(Align::Center);
        actions.append(note);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every step fits the fixed window: setup never scrolls or grows.
    #[test]
    fn every_step_fits_the_window_without_scrolling() {
        let _ = crate::test_support::with_gtk(|| {
            crate::settings::ui_support::install_settings_css();
            let window = ApplicationWindow::builder().build();
            let widgets = install(&window, Rc::new(RefCell::new(Vec::new())));
            let root = window.child().expect("onboarding root");
            for step in OnboardingStep::all() {
                show_step(&widgets, step);
                prune_pages(&widgets.pages);
                let needed = root.measure(Orientation::Vertical, BASE_WIDTH).0;
                eprintln!("[fit] {step:?} needs {needed}px");
                assert!(
                    needed <= BASE_HEIGHT,
                    "{step:?} needs {needed}px, the window is {BASE_HEIGHT}px"
                );
            }
        });
    }

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

    #[test]
    fn progress_tracks_forward_and_back_navigation_without_stale_states() {
        let _ = crate::test_support::with_gtk(|| {
            let progress = GtkBox::new(Orientation::Horizontal, 0);
            for step in OnboardingStep::all()
                .into_iter()
                .chain(OnboardingStep::all().into_iter().rev())
            {
                build_progress(&progress, step);
                let parts = children(&progress);
                let steps = OnboardingStep::all();
                assert_eq!(parts.len(), steps.len() * 2 - 1);
                for (index, item) in steps.iter().enumerate() {
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
                    let content = node
                        .first_child()
                        .and_downcast::<Overlay>()
                        .and_then(|ring| ring.child())
                        .expect("step marker");
                    if index < step.position() {
                        let check = content.downcast::<gtk4::Image>().expect("completed check");
                        assert_eq!(check.icon_name().as_deref(), Some(custom::CHECK_SYMBOLIC));
                    } else {
                        let number = content.downcast::<Label>().expect("step number");
                        assert_eq!(number.text().as_str(), (index + 1).to_string());
                    }
                    if index + 1 < steps.len() {
                        let connector = &parts[index * 2 + 1];
                        assert!(connector.has_css_class("onboarding-progress-connector"));
                        assert_eq!(
                            connector.has_css_class("onboarding-progress-connector-done"),
                            index < step.position()
                        );
                    }
                }
                assert!(progress.measure(Orientation::Horizontal, -1).0 <= 200);
                assert!(progress.measure(Orientation::Vertical, -1).0 <= 32);
            }
        });
    }

    #[test]
    fn form_actions_keep_primary_first_and_layout_is_idempotent() {
        let _ = crate::test_support::with_gtk(|| {
            let actions = GtkBox::new(Orientation::Vertical, 0);
            let skip = Button::with_label("Skip");
            let primary = Button::with_label("Continue");
            primary.add_css_class("settings-primary-btn");
            let secondary = Button::with_label("Another action");
            actions.append(&skip);
            actions.append(&primary);
            actions.append(&secondary);

            let note = Label::new(Some("No account needed."));
            actions.append(&note);

            for _ in 0..2 {
                layout_footer_actions(&actions);
                assert_eq!(actions.first_child(), Some(primary.clone().upcast()));
                let row = primary
                    .next_sibling()
                    .and_downcast::<GtkBox>()
                    .expect("secondary row");
                assert!(row.has_css_class("onboarding-secondary-row"));
                assert_eq!(row.next_sibling(), Some(note.clone().upcast()));
                assert_eq!(row.first_child(), Some(skip.clone().upcast()));
                assert_eq!(row.last_child(), Some(secondary.clone().upcast()));
                assert_eq!(children(&row).len(), 3);
                assert_eq!(primary.halign(), Align::Fill);
                assert!(skip.has_css_class("onboarding-text-button"));
                assert!(secondary.has_css_class("onboarding-text-button"));
            }
        });
    }
}
