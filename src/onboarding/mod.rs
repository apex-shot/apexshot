use gtk4::{prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button, Orientation};
use std::fs;
use std::path::PathBuf;

mod cloud;
mod complete;
pub mod extensions;
mod welcome;

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

#[derive(Clone, Copy, PartialEq)]
pub enum OnboardingStep {
    Welcome,
    ChromeExtension,
    Cloud,
    Complete,
}

impl OnboardingStep {
    fn next(self) -> Option<Self> {
        match self {
            Self::Welcome => Some(Self::ChromeExtension),
            Self::ChromeExtension => Some(Self::Cloud),
            Self::Cloud => Some(Self::Complete),
            Self::Complete => None,
        }
    }

    fn prev(self) -> Option<Self> {
        match self {
            Self::Welcome => None,
            Self::ChromeExtension => Some(Self::Welcome),
            Self::Cloud => Some(Self::ChromeExtension),
            Self::Complete => Some(Self::Cloud),
        }
    }
}

pub struct OnboardingWidgets {
    pub window: ApplicationWindow,
    content_box: GtkBox,
    nav_box: GtkBox,
    top_nav_box: GtkBox,
    progress_box: GtkBox,
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
    install_settings_css();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("ApexShot Setup")
        .default_width(720)
        .default_height(560)
        .decorated(false)
        .build();

    window.add_css_class("editor-window");

    let root_box = GtkBox::new(Orientation::Vertical, 0);
    root_box.add_css_class("editor-root");
    let prefers_dark = crate::capture::editor::ui_support::prefers_dark_glass_theme();
    if prefers_dark {
        root_box.add_css_class("editor-theme-dark");
    } else {
        root_box.add_css_class("editor-theme-light");
    }

    // --- TOOLBAR ---
    let toolbar = GtkBox::new(Orientation::Horizontal, 0);
    toolbar.add_css_class("settings-window-controls");
    toolbar.set_size_request(-1, 30);

    let drag_handle = GtkBox::new(Orientation::Horizontal, 0);
    drag_handle.set_hexpand(true);
    drag_handle.set_halign(Align::Fill);
    drag_handle.set_vexpand(false);
    toolbar.append(&drag_handle);

    let close_btn = traffic_light_button("traffic-light-red", "Close");
    close_btn.remove_css_class("recent-captures-wm-btn");
    close_btn.remove_css_class("recent-captures-wm-close");
    close_btn.add_css_class("recording-editor-traffic-btn");
    let win_clone = window.clone();
    close_btn.connect_clicked(move |_| win_clone.close());

    let min_btn = traffic_light_button("traffic-light-yellow", "Minimize");
    min_btn.remove_css_class("recent-captures-wm-btn");
    min_btn.add_css_class("recording-editor-traffic-btn");
    let win_clone = window.clone();
    min_btn.connect_clicked(move |_| win_clone.minimize());

    for button in [&close_btn, &min_btn] {
        button.set_size_request(24, 24);
        button.set_valign(Align::Center);
    }

    let right_box = GtkBox::new(Orientation::Horizontal, 6);
    right_box.set_halign(Align::End);
    right_box.append(&min_btn);
    right_box.append(&close_btn);
    toolbar.append(&right_box);

    root_box.append(&toolbar);

    install_window_drag(&drag_handle, &window);

    // Top navigation (back button + progress indicator)
    let top_nav_box = GtkBox::new(Orientation::Horizontal, 12);
    top_nav_box.set_halign(Align::Start);
    top_nav_box.set_margin_top(16);
    top_nav_box.set_margin_bottom(8);
    top_nav_box.set_margin_start(24);

    // Progress indicator
    let progress_box = GtkBox::new(Orientation::Horizontal, 6);
    progress_box.set_halign(Align::Center);
    progress_box.set_margin_top(8);
    progress_box.set_margin_bottom(16);

    // Content area (will be swapped per step)
    let content_box = GtkBox::new(Orientation::Vertical, 12);
    content_box.set_vexpand(true);
    content_box.set_halign(Align::Center);
    content_box.set_valign(Align::Center);

    // Bottom navigation buttons
    let nav_box = GtkBox::new(Orientation::Horizontal, 12);
    nav_box.set_halign(Align::End);
    nav_box.set_margin_top(20);
    nav_box.set_margin_bottom(16);
    nav_box.set_margin_end(24);

    root_box.append(&top_nav_box);
    root_box.append(&progress_box);
    root_box.append(&content_box);
    root_box.append(&nav_box);
    window.set_child(Some(&root_box));

    // Store state
    let widgets = OnboardingWidgets {
        window: window.clone(),
        content_box: content_box.clone(),
        nav_box: nav_box.clone(),
        top_nav_box: top_nav_box.clone(),
        progress_box: progress_box.clone(),
    };

    show_step(&widgets, OnboardingStep::Welcome);

    window.present();
}

fn show_step(widgets: &OnboardingWidgets, step: OnboardingStep) {
    // Clear content
    while let Some(child) = widgets.content_box.first_child() {
        widgets.content_box.remove(&child);
    }

    // Clear nav boxes
    while let Some(child) = widgets.nav_box.first_child() {
        widgets.nav_box.remove(&child);
    }
    while let Some(child) = widgets.top_nav_box.first_child() {
        widgets.top_nav_box.remove(&child);
    }
    while let Some(child) = widgets.progress_box.first_child() {
        widgets.progress_box.remove(&child);
    }

    // Build step content
    match step {
        OnboardingStep::Welcome => {
            welcome::build(&widgets.content_box);
        }
        OnboardingStep::ChromeExtension => {
            extensions::build_chrome(&widgets.content_box);
        }
        OnboardingStep::Cloud => {
            cloud::build(&widgets.content_box);
        }
        OnboardingStep::Complete => {
            complete::build(&widgets.content_box);
        }
    }

    // Build navigation
    build_navigation(widgets, step);
}

fn build_navigation(widgets: &OnboardingWidgets, step: OnboardingStep) {
    // Build progress dots
    let steps = [
        OnboardingStep::Welcome,
        OnboardingStep::ChromeExtension,
        OnboardingStep::Cloud,
        OnboardingStep::Complete,
    ];
    let current_idx = steps.iter().position(|s| *s == step).unwrap_or(0);

    for (idx, _) in steps.iter().enumerate() {
        let dot = GtkBox::new(Orientation::Horizontal, 0);
        dot.add_css_class("onboarding-dot");
        if idx == current_idx {
            dot.add_css_class("onboarding-dot-active");
        }
        dot.set_size_request(if idx == current_idx { 18 } else { 6 }, 6);
        dot.set_valign(Align::Center);
        widgets.progress_box.append(&dot);
    }

    // Top navigation - Back button (left side)
    if let Some(prev_step) = step.prev() {
        let back_btn = Button::with_label("← Back");
        back_btn.add_css_class("secondary-settings-button");
        back_btn.add_css_class("onboarding-back-button");
        back_btn.set_margin_start(16);
        let widgets_clone = OnboardingWidgets {
            window: widgets.window.clone(),
            content_box: widgets.content_box.clone(),
            nav_box: widgets.nav_box.clone(),
            top_nav_box: widgets.top_nav_box.clone(),
            progress_box: widgets.progress_box.clone(),
        };
        back_btn.connect_clicked(move |_| {
            show_step(&widgets_clone, prev_step);
        });
        widgets.top_nav_box.append(&back_btn);
    }

    // Bottom navigation - Spacer
    let spacer_bottom = GtkBox::new(Orientation::Horizontal, 0);
    spacer_bottom.set_hexpand(true);
    widgets.nav_box.append(&spacer_bottom);

    // Next/Finish button (bottom right)
    if let Some(next_step) = step.next() {
        let next_btn = Button::with_label("Next →");
        next_btn.add_css_class("settings-primary-btn");
        next_btn.set_margin_end(16);
        let widgets_clone = OnboardingWidgets {
            window: widgets.window.clone(),
            content_box: widgets.content_box.clone(),
            nav_box: widgets.nav_box.clone(),
            top_nav_box: widgets.top_nav_box.clone(),
            progress_box: widgets.progress_box.clone(),
        };
        next_btn.connect_clicked(move |_| {
            show_step(&widgets_clone, next_step);
        });
        widgets.nav_box.append(&next_btn);
    } else {
        // Final step - "Start Using ApexShot"
        let finish_btn = Button::with_label("Start Using ApexShot");
        finish_btn.add_css_class("settings-primary-btn");
        finish_btn.set_margin_end(16);
        let window = widgets.window.clone();
        finish_btn.connect_clicked(move |_| {
            let _ = mark_onboarding_complete();
            // Spawn the main app (settings UI) now that onboarding is complete
            let exe =
                std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("apexshot"));
            if let Err(e) = std::process::Command::new(&exe).spawn() {
                eprintln!("Failed to launch settings window: {e}");
            }
            window.close();
        });
        widgets.nav_box.append(&finish_btn);
    }
}
