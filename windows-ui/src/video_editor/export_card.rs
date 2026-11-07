//! A quiet, full-window export screen with live progress and completion actions.

use super::export::{start_export_with_settings, ExportModel, ExportSettings, ExportUpdate};
use crate::i18n::t;
use gtk4::prelude::*;
use gtk4::{gdk, glib, Align, Box as GtkBox, Button, Label, Orientation, Overlay, ProgressBar};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

pub(super) fn show(
    shell: &Overlay,
    model: ExportModel,
    output: PathBuf,
    settings: ExportSettings,
    clipboard: bool,
) {
    let scale = crate::platform::ui_scale();
    let px = |n: f64| (n * scale).round() as i32;
    let screen = gtk4::CenterBox::new();
    screen.set_orientation(Orientation::Vertical);
    screen.add_css_class("video-export-screen");
    screen.set_hexpand(true);
    screen.set_vexpand(true);
    let content = GtkBox::new(Orientation::Vertical, px(10.0));
    content.set_halign(Align::Center);
    content.set_width_request(px(330.0));
    content.set_margin_start(px(30.0));
    content.set_margin_end(px(30.0));
    let destination = Label::new(Some(&output.to_string_lossy()));
    destination.add_css_class("video-export-destination");
    destination.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    destination.set_max_width_chars(52);
    destination.set_tooltip_text(Some(&output.to_string_lossy()));
    let title = Label::new(Some(&t("Preparing export…")));
    title.add_css_class("video-export-progress-title");
    let bar = ProgressBar::new();
    bar.add_css_class("video-export-progress");
    let timing = Label::new(Some("00:00 · —"));
    timing.add_css_class("video-export-muted");
    let detail = Label::new(None);
    detail.add_css_class("video-export-muted");
    detail.set_wrap(true);
    detail.set_max_width_chars(60);
    detail.set_selectable(true);
    detail.set_visible(false);
    let stop = Button::with_label(&t("Stop export"));
    stop.add_css_class("video-export-stop");
    stop.set_has_frame(false);
    stop.set_halign(Align::Center);
    stop.set_margin_top(px(24.0));
    let actions = GtkBox::new(Orientation::Horizontal, px(8.0));
    actions.set_halign(Align::Center);
    actions.set_margin_top(px(18.0));
    actions.set_visible(false);
    let back = Button::with_label(&t("Back to editor"));
    let reveal = Button::with_label(&t("Show in Explorer"));
    reveal.add_css_class("video-export-primary");
    actions.append(&back);
    actions.append(&reveal);
    for widget in [
        destination.upcast_ref::<gtk4::Widget>(),
        title.upcast_ref(),
        bar.upcast_ref(),
        timing.upcast_ref(),
        detail.upcast_ref(),
        stop.upcast_ref(),
        actions.upcast_ref(),
    ] {
        content.append(widget);
    }
    screen.set_center_widget(Some(&content));
    present_screen(shell, &screen);
    stop.grab_focus();

    let handle = Rc::new(start_export_with_settings(model, output, settings));
    let cancelling = Rc::new(Cell::new(false));
    stop.connect_clicked({
        let handle = handle.clone();
        let cancelling = cancelling.clone();
        let title = title.clone();
        move |button| {
            cancelling.set(true);
            handle.cancel();
            button.set_sensitive(false);
            title.set_text(&t("Stopping export…"));
        }
    });
    back.connect_clicked({
        let shell = shell.downgrade();
        let screen = screen.downgrade();
        move |_| {
            if let (Some(shell), Some(screen)) = (shell.upgrade(), screen.upgrade()) {
                shell.remove_overlay(&screen);
                restore_editor(&shell);
            }
        }
    });

    let started = Instant::now();
    let mut last_sample: Option<(Instant, u64)> = None;
    let mut per_frame: Option<f64> = None;
    let mut fraction = 0.0_f64;
    let mut remaining = None;
    glib::timeout_add_local(Duration::from_millis(100), {
        let shell = shell.downgrade();
        let screen = screen.downgrade();
        move || {
            let (Some(shell), Some(screen)) = (shell.upgrade(), screen.upgrade()) else {
                handle.cancel();
                return glib::ControlFlow::Break;
            };
            if shell.root().is_none() {
                handle.cancel();
                return glib::ControlFlow::Break;
            }
            let mut update = handle.receiver.try_recv();
            let mut latest = None;
            while let Ok(ExportUpdate::Progress { done, total }) = update {
                latest = Some((done, total));
                update = handle.receiver.try_recv();
            }
            if let Some((done, total)) = latest {
                if let Some((since, previous)) = last_sample {
                    if done > previous {
                        let sample = since.elapsed().as_secs_f64() / (done - previous) as f64;
                        per_frame = Some(
                            per_frame.map_or(sample, |previous| previous * 0.6 + sample * 0.4),
                        );
                    } else if done < previous {
                        // A hardware-encoder fallback starts progress from the beginning.
                        per_frame = None;
                    }
                }
                last_sample = Some((Instant::now(), done));
                fraction = if total == 0 {
                    0.0
                } else {
                    done as f64 / total as f64
                };
                remaining = per_frame.map(|seconds| seconds * total.saturating_sub(done) as f64);
                bar.set_fraction(fraction.clamp(0.0, 1.0));
                if !cancelling.get() {
                    title.set_text(&format!(
                        "{} · {:.0}%",
                        t("Exporting video"),
                        fraction * 100.0
                    ));
                }
            }
            let eta = if fraction >= 1.0 {
                t("Finishing…")
            } else if let Some(seconds) = remaining {
                let since = last_sample.map_or(0.0, |(instant, _)| instant.elapsed().as_secs_f64());
                format!("{} {}", clock((seconds - since).max(0.0)), t("left"))
            } else {
                t("Estimating…")
            };
            timing.set_text(&format!(
                "{} · {eta}",
                clock(started.elapsed().as_secs_f64())
            ));
            let failure = match update {
                Ok(ExportUpdate::Progress { .. }) | Err(TryRecvError::Empty) => {
                    return glib::ControlFlow::Continue
                }
                Ok(ExportUpdate::Cancelled) => {
                    shell.remove_overlay(&screen);
                    restore_editor(&shell);
                    return glib::ControlFlow::Break;
                }
                Ok(ExportUpdate::Finished(path)) => {
                    title.set_text(&t("Exported successfully"));
                    timing.set_text(&format!(
                        "{} · {}",
                        clock(started.elapsed().as_secs_f64()),
                        t("Complete")
                    ));
                    if clipboard {
                        match copy_file(&path) {
                            Ok(()) => detail.set_text(&t(
                                "File copied to clipboard. Ready to paste into other apps.",
                            )),
                            Err(error) => detail.set_text(&format!(
                                "{} {error}",
                                t("Video exported, but could not copy to clipboard:")
                            )),
                        }
                        detail.set_visible(true);
                    }
                    reveal.connect_clicked({
                        let detail = detail.clone();
                        move |_| {
                            if let Err(error) = crate::utils::open::reveal_in_file_manager(&path) {
                                detail.set_text(&error.to_string());
                                detail.set_visible(true);
                            }
                        }
                    });
                    None
                }
                Ok(ExportUpdate::Failed(message)) => Some(message),
                Err(TryRecvError::Disconnected) => Some(t("The export stopped unexpectedly.")),
            };
            if let Some(message) = failure {
                title.set_text(&t("Export failed"));
                detail.set_text(&message);
                detail.add_css_class("video-export-error");
                detail.set_visible(true);
                timing.set_visible(false);
                reveal.set_visible(false);
            }
            bar.set_visible(false);
            stop.set_visible(false);
            actions.set_visible(true);
            back.grab_focus();
            glib::ControlFlow::Break
        }
    });
}

fn present_screen(shell: &Overlay, screen: &impl IsA<gtk4::Widget>) {
    if let Some(editor) = shell.child() {
        editor.set_sensitive(false);
        // Preserve the editor's allocation and state, without showing any of it.
        editor.set_opacity(0.0);
    }
    shell.add_overlay(screen);
}

fn restore_editor(shell: &Overlay) {
    if let Some(editor) = shell.child() {
        editor.set_opacity(1.0);
        editor.set_sensitive(true);
    }
}

fn copy_file(path: &std::path::Path) -> Result<(), String> {
    let display = gdk::Display::default().ok_or_else(|| t("Clipboard is unavailable."))?;
    let list = gdk::FileList::from_array(&[gtk4::gio::File::for_path(path)]);
    display
        .clipboard()
        .set_content(Some(&gdk::ContentProvider::for_value(&list.to_value())))
        .map_err(|error| error.to_string())
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

pub(super) fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let scale = crate::platform::ui_scale();
    let px = |n: f64| (n * scale).round().max(1.0) as i32;
    let css = format!(
        r#"
        .video-export-scrim {{ background: alpha(#05070c, 0.64); }}
        .video-export-options, .video-export-screen {{
            color: #e8e8e8;
            font-family: 'DM Sans', 'Noto Sans', system-ui, sans-serif;
            font-size: {font}px;
        }}
        .video-export-options {{
            background: #171717;
            border: none;
            border-radius: {radius}px;
            padding: {pad}px;
            box-shadow: 0 {shadow_y}px {shadow_blur}px alpha(black, 0.3);
        }}
        .video-export-screen {{ background: #0f0f0f; border-radius: 11px; }}
        .video-export-heading {{ font-weight: 500; }}
        .video-export-dialog-title {{ font-size: {dialog_title}px; font-weight: 600; }}
        .video-export-muted {{ color: #858585; font-size: {small}px; }}
        .video-export-options button.video-export-action,
        .video-export-options button.video-export-preset,
        .video-export-options button.video-export-close,
        .video-export-screen button {{
            background: #202020;
            background-image: none;
            color: #c6c6c6;
            border: none;
            border-radius: {button_radius}px;
            box-shadow: none;
            text-shadow: none;
            min-height: {button_height}px;
            padding: 0 {button_pad}px;
            font-size: {font}px;
            font-weight: 500;
        }}
        .video-export-options button.video-export-action:hover,
        .video-export-options button.video-export-close:hover,
        .video-export-screen button:hover {{ background: #292929; }}
        .video-export-options button:focus-visible, .video-export-screen button:focus-visible {{ outline: 2px solid #ff7c2f; outline-offset: 2px; }}
        .video-export-options button.video-export-action {{ min-height: {action_height}px; }}
        .video-export-options button.video-export-preset {{ background: alpha(white, 0.04); }}
        .video-export-options button.video-export-preset:hover {{ background: alpha(white, 0.08); }}
        .video-export-options button.video-export-preset:checked {{ color: #ff995c; background: alpha(#ff670d, 0.16); }}
        .video-export-options button.video-export-close {{
            background: transparent;
            min-width: {close_size}px;
            min-height: {close_size}px;
            padding: 0;
            color: #999999;
        }}
        .video-export-options button.settings-select {{
            background: alpha(white, 0.06);
            color: #f1f1f3;
            border: none;
            border-radius: {button_radius}px;
            box-shadow: none;
            min-height: {select_height}px;
            padding: {select_pad}px 10px;
        }}
        .video-export-options button.settings-select label {{ font-size: {font}px; }}
        .video-export-options button.settings-select:hover {{ background: alpha(white, 0.10); }}
        .video-export-options button.settings-select:active {{ background: alpha(white, 0.12); }}
        .video-export-options button.video-export-primary,
        .video-export-screen button.video-export-primary {{ background: #ff670d; color: #ffffff; }}
        .video-export-options button.video-export-primary:hover,
        .video-export-screen button.video-export-primary:hover {{ background: #ff7c2f; }}
        .video-export-destination {{
            background: #202020;
            border: 1px solid alpha(white, 0.08);
            border-radius: {pill_radius}px;
            padding: {pill_y}px {button_pad}px;
            color: #a4a4a4;
            font-size: {small}px;
        }}
        .video-export-progress-title {{ font-weight: 600; font-size: {title}px; }}
        progressbar.video-export-progress trough {{ min-height: {bar}px; background: #292929; border: none; border-radius: {bar}px; }}
        progressbar.video-export-progress progress {{ min-height: {bar}px; background: #ff670d; border: none; border-radius: {bar}px; }}
        .video-export-screen button.video-export-stop {{
            background: #242424;
            color: #e8e8e8;
            min-height: {stop_height}px;
            padding: 0 {stop_pad}px;
            border: none;
            border-radius: {stop_radius}px;
            box-shadow: none;
            outline: none;
            font-size: {font}px;
        }}
        .video-export-screen button.video-export-stop:hover {{ background: #30251f; color: #ff995c; }}
        .video-export-screen button.video-export-stop:active {{ background: #3b291f; color: #ffb184; }}
        .video-export-screen button.video-export-stop:disabled {{ background: #1b1b1b; color: #777777; }}
        .video-export-screen button.video-export-stop:focus-visible {{ outline: 2px solid #ff7c2f; outline-offset: 3px; }}
        .video-export-error {{ color: #ed786a; }}
        .editor-theme-light .video-export-options {{ background: #ffffff; color: #202020; }}
        .editor-theme-light .video-export-screen {{ background: #f6f6f6; color: #202020; }}
        .editor-theme-light .video-export-muted {{ color: #737373; }}
        .editor-theme-light .video-export-options button.video-export-action,
        .editor-theme-light .video-export-screen button {{ background: #eeeeee; color: #454545; }}
        .editor-theme-light .video-export-options button.video-export-action:hover,
        .editor-theme-light .video-export-screen button:hover {{ background: #e5e5e5; }}
        .editor-theme-light .video-export-options button.video-export-preset {{ background: alpha(#111827, 0.04); color: #454545; }}
        .editor-theme-light .video-export-options button.video-export-preset:hover {{ background: alpha(#111827, 0.08); }}
        .editor-theme-light .video-export-options button.video-export-preset:checked {{ color: #b74809; background: alpha(#ff670d, 0.12); }}
        .editor-theme-light .video-export-options button.video-export-close {{ background: transparent; color: #737373; }}
        .editor-theme-light .video-export-options button.video-export-close:hover {{ background: alpha(#111827, 0.06); }}
        .editor-theme-light .video-export-options button.settings-select {{ background: #ffffff; color: #17202a; }}
        .editor-theme-light .video-export-options button.settings-select:hover {{ background: #f8fafc; }}
        .editor-theme-light .video-export-options button.settings-select:active {{ background: #f1f5f9; }}
        .editor-theme-light .video-export-options button.video-export-primary,
        .editor-theme-light .video-export-screen button.video-export-primary {{ background: #ff670d; color: white; }}
        .editor-theme-light .video-export-destination {{ background: #eeeeee; color: #666666; border-color: alpha(black, 0.08); }}
        .editor-theme-light progressbar.video-export-progress trough {{ background: #dddddd; }}
        .editor-theme-light .video-export-screen button.video-export-stop {{ background: #e8e8e8; color: #333333; }}
        .editor-theme-light .video-export-screen button.video-export-stop:hover {{ background: #f5e5da; color: #a94008; }}
        .editor-theme-light .video-export-screen button.video-export-stop:active {{ background: #efd5c3; color: #8a3508; }}
        .editor-theme-light .video-export-screen button.video-export-stop:disabled {{ background: #eeeeee; color: #999999; }}
        "#,
        font = 14.0 * scale,
        small = 13.0 * scale,
        title = 15.0 * scale,
        dialog_title = 18.0 * scale,
        radius = px(14.0),
        pad = px(26.0),
        close_size = px(30.0),
        action_height = px(36.0),
        select_height = px(26.0),
        select_pad = px(1.0),
        shadow_y = px(12.0),
        shadow_blur = px(36.0),
        button_radius = px(5.0),
        button_height = px(32.0),
        stop_height = px(34.0),
        stop_pad = px(18.0),
        stop_radius = px(7.0),
        button_pad = px(11.0),
        pill_radius = px(4.0),
        pill_y = px(5.0),
        bar = px(3.0),
    );
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(&css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 3,
    );
    crate::settings::shell::install_select_menu_css();
}

#[cfg(test)]
mod tests {
    use super::clock;

    #[test]
    fn export_hides_and_restores_the_whole_editor_without_changing_its_layout() {
        let Some(()) = crate::test_support::with_gtk(|| {
            use gtk4::prelude::*;
            use gtk4::{Box as GtkBox, Orientation, Overlay};

            let shell = Overlay::new();
            let editor = GtkBox::new(Orientation::Vertical, 0);
            let title = GtkBox::new(Orientation::Horizontal, 0);
            editor.append(&title);
            shell.set_child(Some(&editor));
            let screen = gtk4::CenterBox::new();
            super::present_screen(&shell, &screen);
            assert_eq!(shell.child(), Some(editor.clone().upcast()));
            assert!(editor.is_visible());
            assert_eq!(editor.opacity(), 0.0);
            assert!(!title.is_sensitive());
            assert_eq!(shell.last_child(), Some(screen.clone().upcast()));

            shell.remove_overlay(&screen);
            super::restore_editor(&shell);
            assert_eq!(editor.opacity(), 1.0);
            assert!(title.is_sensitive());
            assert_eq!(shell.last_child(), Some(editor.upcast()));
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn export_clock_supports_long_recordings() {
        assert_eq!(clock(0.0), "00:00");
        assert_eq!(clock(67.0), "01:07");
        assert_eq!(clock(3661.0), "1:01:01");
    }
}
