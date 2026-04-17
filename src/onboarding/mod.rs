use gtk4::{prelude::*, Align, Application, ApplicationWindow, Box as GtkBox, Button, Orientation};
use std::fs;
use std::path::PathBuf;

mod welcome;
mod extensions;
mod cloud;
mod complete;

use crate::settings::ui_support::install_settings_css;

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
    GnomeExtension,
    ChromeExtension,
    Cloud,
    Complete,
}

impl OnboardingStep {
    fn next(self) -> Option<Self> {
        match self {
            Self::Welcome => Some(Self::GnomeExtension),
            Self::GnomeExtension => Some(Self::ChromeExtension),
            Self::ChromeExtension => Some(Self::Cloud),
            Self::Cloud => Some(Self::Complete),
            Self::Complete => None,
        }
    }

    fn prev(self) -> Option<Self> {
        match self {
            Self::Welcome => None,
            Self::GnomeExtension => Some(Self::Welcome),
            Self::ChromeExtension => Some(Self::GnomeExtension),
            Self::Cloud => Some(Self::ChromeExtension),
            Self::Complete => Some(Self::Cloud),
        }
    }
}

pub struct OnboardingWidgets {
    pub window: ApplicationWindow,
    content_box: GtkBox,
    nav_box: GtkBox,
}

pub fn show_onboarding_window() -> anyhow::Result<()> {
    let app = Application::builder()
        .application_id("io.github.codegoddy.apexshot")
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
        .default_width(1020)
        .default_height(840)
        .decorated(false)
        .build();

    window.add_css_class("editor-window");

    let root_box = GtkBox::new(Orientation::Vertical, 0);
    root_box.add_css_class("editor-root");
    root_box.set_margin_top(24);
    root_box.set_margin_bottom(24);
    root_box.set_margin_start(32);
    root_box.set_margin_end(32);

    // Content area (will be swapped per step)
    let content_box = GtkBox::new(Orientation::Vertical, 16);
    content_box.set_vexpand(true);
    content_box.set_halign(Align::Center);
    content_box.set_valign(Align::Center);

    // Navigation buttons
    let nav_box = GtkBox::new(Orientation::Horizontal, 12);
    nav_box.set_halign(Align::End);
    nav_box.set_margin_top(24);

    root_box.append(&content_box);
    root_box.append(&nav_box);
    window.set_child(Some(&root_box));

    // Store state
    let widgets = OnboardingWidgets {
        window: window.clone(),
        content_box: content_box.clone(),
        nav_box: nav_box.clone(),
    };

    show_step(&widgets, OnboardingStep::Welcome);

    window.present();
}

fn show_step(widgets: &OnboardingWidgets, step: OnboardingStep) {
    // Clear content
    while let Some(child) = widgets.content_box.first_child() {
        widgets.content_box.remove(&child);
    }

    // Clear nav
    while let Some(child) = widgets.nav_box.first_child() {
        widgets.nav_box.remove(&child);
    }

    // Build step content
    match step {
        OnboardingStep::Welcome => {
            welcome::build(&widgets.content_box);
        }
        OnboardingStep::GnomeExtension => {
            extensions::build_gnome(&widgets.content_box);
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
    // Skip button (only on welcome step)
    if step == OnboardingStep::Welcome {
        let skip_btn = Button::with_label("Skip Setup");
        skip_btn.add_css_class("secondary-settings-button");
        let window = widgets.window.clone();
        skip_btn.connect_clicked(move |_| {
            let _ = mark_onboarding_complete();
            window.close();
        });
        widgets.nav_box.append(&skip_btn);
    }

    // Spacer
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    widgets.nav_box.append(&spacer);

    // Back button
    if let Some(prev_step) = step.prev() {
        let back_btn = Button::with_label("← Back");
        back_btn.add_css_class("secondary-settings-button");
        let widgets_clone = OnboardingWidgets {
            window: widgets.window.clone(),
            content_box: widgets.content_box.clone(),
            nav_box: widgets.nav_box.clone(),
        };
        back_btn.connect_clicked(move |_| {
            show_step(&widgets_clone, prev_step);
        });
        widgets.nav_box.append(&back_btn);
    }

    // Next/Finish button
    if let Some(next_step) = step.next() {
        let next_btn = Button::with_label("Next →");
        next_btn.add_css_class("settings-primary-btn");
        let widgets_clone = OnboardingWidgets {
            window: widgets.window.clone(),
            content_box: widgets.content_box.clone(),
            nav_box: widgets.nav_box.clone(),
        };
        next_btn.connect_clicked(move |_| {
            show_step(&widgets_clone, next_step);
        });
        widgets.nav_box.append(&next_btn);
    } else {
        // Final step - "Start Using ApexShot"
        let finish_btn = Button::with_label("Start Using ApexShot");
        finish_btn.add_css_class("settings-primary-btn");
        let window = widgets.window.clone();
        finish_btn.connect_clicked(move |_| {
            let _ = mark_onboarding_complete();
            window.close();
        });
        widgets.nav_box.append(&finish_btn);
    }
}
