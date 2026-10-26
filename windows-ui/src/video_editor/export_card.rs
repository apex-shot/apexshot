//! The export card: a floating panel over the editor that reports an export.
//!
//! It replaces the old separate progress window. The card is laid out in three
//! bands — a header that names the export and its settings, a body that holds
//! either the progress bar with its percentage pill and estimate or the result,
//! and a footer of actions once the file exists.

use super::export::{start_export, ExportModel, ExportUpdate};
use crate::i18n::t;
use gtk4::cairo::{Context, FontSlant, FontWeight, LinearGradient};
use gtk4::prelude::*;
use gtk4::{
    gdk, glib, Align, Box as GtkBox, Button, DrawingArea, Label, Orientation, Overlay, Separator,
};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

/// Show the card and drive it until the export ends or the user closes it.
pub(super) fn show(shell: &Overlay, model: ExportModel, output: PathBuf) {
    let scale = crate::platform::ui_scale();
    let px = |value: f64| (value * scale).round().max(1.0) as i32;
    let dark = crate::settings::windowing::prefers_dark_glass_theme();
    let size = model.source_size;
    let fps = crate::recording::editor::ffmpeg::probe_frame_rate(&model.source)
        .filter(|fps| fps.is_finite() && *fps > 0.0)
        .unwrap_or(30.0);

    let scrim = GtkBox::new(Orientation::Vertical, 0);
    scrim.add_css_class("video-editor-export-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_can_target(true);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("video-editor-export-card");
    card.add_css_class(if dark {
        "editor-theme-dark"
    } else {
        "editor-theme-light"
    });
    card.set_halign(Align::Center);
    card.set_valign(Align::Center);
    card.set_width_request(px(320.0));
    card.set_can_target(true);

    let head = GtkBox::new(Orientation::Horizontal, 0);
    head.add_css_class("video-editor-export-head");
    let title = Label::new(Some(&t("Export")));
    title.add_css_class("video-editor-export-title");
    head.append(&title);
    for text in [format!("{}FPS", fps.round() as i32), format!("{}p", size.1)] {
        let dot = Label::new(Some("•"));
        dot.add_css_class("video-editor-export-dot");
        dot.set_margin_start(px(6.0));
        dot.set_margin_end(px(6.0));
        let meta = Label::new(Some(&text));
        meta.add_css_class("video-editor-export-meta");
        head.append(&dot);
        head.append(&meta);
    }
    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let close = Button::from_icon_name("window-close-symbolic");
    close.add_css_class("video-editor-export-close");
    close.set_valign(Align::Center);
    head.append(&spacer);
    head.append(&close);

    let result = Label::new(None);
    result.add_css_class("video-editor-export-result");
    result.set_xalign(0.0);
    result.set_visible(false);
    let detail = Label::new(None);
    detail.add_css_class("video-editor-export-detail");
    detail.set_xalign(0.0);
    detail.set_wrap(true);
    detail.set_visible(false);

    let fraction = Rc::new(Cell::new(0.0_f64));
    let bar = DrawingArea::new();
    bar.add_css_class("video-editor-export-bar");
    bar.set_content_height(px(26.0));
    bar.set_hexpand(true);
    bar.set_draw_func({
        let fraction = fraction.clone();
        move |_area, context, width, height| {
            draw_bar(
                context,
                f64::from(width),
                f64::from(height),
                fraction.get(),
                scale,
                dark,
            );
        }
    });

    let estimate_row = GtkBox::new(Orientation::Horizontal, 0);
    estimate_row.add_css_class("video-editor-export-estimate");
    let estimate_label = Label::new(Some(&t("estimated:")));
    estimate_label.add_css_class("video-editor-export-estimate-label");
    estimate_label.set_xalign(1.0);
    estimate_label.set_hexpand(true);
    let estimate_value = Label::new(Some("—"));
    estimate_value.add_css_class("video-editor-export-estimate-value");
    estimate_value.set_xalign(1.0);
    estimate_row.append(&estimate_label);
    estimate_row.append(&estimate_value);
    estimate_row.set_margin_start(px(4.0));
    estimate_value.set_margin_start(px(6.0));

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("video-editor-export-body");
    body.append(&result);
    body.append(&detail);
    body.append(&bar);
    body.append(&estimate_row);

    let lower_rule = Separator::new(Orientation::Horizontal);
    lower_rule.add_css_class("video-editor-export-rule");
    lower_rule.set_visible(false);

    let actions = GtkBox::new(Orientation::Horizontal, px(7.0));
    actions.add_css_class("video-editor-export-actions");
    actions.set_homogeneous(true);
    actions.set_visible(false);
    let dismiss = Button::with_label(&t("Close"));
    dismiss.add_css_class("video-editor-export-dismiss");
    let reveal = Button::with_label(&t("Show in Explorer"));
    reveal.add_css_class("video-editor-export-reveal");
    actions.append(&dismiss);
    actions.append(&reveal);

    let upper_rule = Separator::new(Orientation::Horizontal);
    upper_rule.add_css_class("video-editor-export-rule");

    card.append(&head);
    card.append(&upper_rule);
    card.append(&body);
    card.append(&lower_rule);
    card.append(&actions);
    shell.add_overlay(&scrim);
    shell.add_overlay(&card);

    let handle = Rc::new(start_export(model, output));
    let finished = Rc::new(Cell::new(false));
    // The estimate follows the export's own throughput: a smoothed per-frame
    // cost built from consecutive progress samples, so encoder start-up and a
    // first slow frame never leak into the number the user reads.
    let last_sample: Rc<Cell<Option<(Instant, u64)>>> = Rc::new(Cell::new(None));
    let per_frame_ms: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));

    close.connect_clicked({
        let handle = handle.clone();
        let finished = finished.clone();
        let estimate_value = estimate_value.clone();
        let shell = shell.clone();
        let card = card.clone();
        let scrim = scrim.clone();
        move |button| {
            if !finished.get() {
                handle.cancel();
                button.set_sensitive(false);
                estimate_value.set_text(&t("cancelling…"));
                return;
            }
            shell.remove_overlay(&card);
            shell.remove_overlay(&scrim);
        }
    });

    dismiss.connect_clicked({
        let shell = shell.clone();
        let card = card.clone();
        let scrim = scrim.clone();
        move |_| {
            shell.remove_overlay(&card);
            shell.remove_overlay(&scrim);
        }
    });

    glib::timeout_add_local(Duration::from_millis(100), {
        let handle = handle.clone();
        let fraction = fraction.clone();
        let bar = bar.clone();
        let estimate_row = estimate_row.clone();
        let estimate_value = estimate_value.clone();
        let result = result.clone();
        let detail = detail.clone();
        let content = card.clone();
        let lower_rule = lower_rule.clone();
        let actions = actions.clone();
        let reveal = reveal.clone();
        let finished = finished.clone();
        let shell = shell.clone();
        let card = card.clone();
        let scrim = scrim.clone();
        let last_sample = last_sample.clone();
        let per_frame_ms = per_frame_ms.clone();
        move || {
            let mut update = handle.receiver.try_recv();
            let mut latest = None;
            while let Ok(ExportUpdate::Progress { done, total }) = update {
                latest = Some((done, total));
                update = handle.receiver.try_recv();
            }
            if let Some((done, total)) = latest {
                if let Some((since, previous)) = last_sample.get() {
                    if done > previous {
                        let millis = since.elapsed().as_secs_f64() * 1000.0;
                        let sample = millis / (done - previous) as f64;
                        let smoothed = match per_frame_ms.get() {
                            Some(previous) => previous * 0.6 + sample * 0.4,
                            None => sample,
                        };
                        per_frame_ms.set(Some(smoothed));
                    }
                }
                last_sample.set(Some((Instant::now(), done)));
                let value = if total == 0 {
                    0.0
                } else {
                    done as f64 / total as f64
                };
                fraction.set(value);
                bar.queue_draw();
                let estimate = match per_frame_ms.get() {
                    _ if total > 0 && done >= total => t("finishing…"),
                    Some(millis) => {
                        duration_words(millis * total.saturating_sub(done) as f64 / 1000.0)
                    }
                    None => "—".to_string(),
                };
                estimate_value.set_markup(&estimate_markup(&estimate, dark));
            }
            match update {
                Ok(ExportUpdate::Progress { .. }) | Err(TryRecvError::Empty) => {
                    glib::ControlFlow::Continue
                }
                Ok(ExportUpdate::Finished(path)) => {
                    fraction.set(1.0);
                    bar.queue_draw();
                    result.set_text(&t("Exported successfully."));
                    result.set_visible(true);
                    reveal.connect_clicked({
                        let detail = detail.clone();
                        move |_| {
                            if let Err(error) = crate::utils::open::reveal_in_file_manager(&path) {
                                detail.set_text(&error.to_string());
                                detail.set_visible(true);
                            }
                        }
                    });
                    bar.set_visible(false);
                    estimate_row.set_visible(false);
                    lower_rule.set_visible(true);
                    content.add_css_class("video-editor-export-done");
                    actions.set_visible(true);
                    content.set_height_request(-1);
                    finished.set(true);
                    glib::ControlFlow::Break
                }
                Ok(ExportUpdate::Failed(message)) => {
                    result.set_text(&t("Export failed."));
                    result.set_visible(true);
                    detail.set_text(&message);
                    detail.set_visible(true);
                    detail.add_css_class("video-editor-export-failed");
                    bar.set_visible(false);
                    estimate_row.set_visible(false);
                    lower_rule.set_visible(true);
                    content.add_css_class("video-editor-export-done");
                    actions.set_visible(true);
                    finished.set(true);
                    actions.remove(&reveal);
                    glib::ControlFlow::Break
                }
                Ok(ExportUpdate::Cancelled) => {
                    shell.remove_overlay(&card);
                    shell.remove_overlay(&scrim);
                    glib::ControlFlow::Break
                }
                Err(TryRecvError::Disconnected) => {
                    result.set_text(&t("Export failed."));
                    result.set_visible(true);
                    detail.set_text(&t("The export stopped unexpectedly."));
                    detail.set_visible(true);
                    detail.add_css_class("video-editor-export-failed");
                    bar.set_visible(false);
                    estimate_row.set_visible(false);
                    lower_rule.set_visible(true);
                    content.add_css_class("video-editor-export-done");
                    actions.set_visible(true);
                    actions.remove(&reveal);
                    finished.set(true);
                    glib::ControlFlow::Break
                }
            }
        }
    });
}

fn duration_words(seconds: f64) -> String {
    let seconds = seconds.round().max(0.0) as u64;
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60),
    }
}

/// Pango markup for the estimate: its figures in the accent, its units in the
/// label's own grey.
fn estimate_markup(text: &str, dark: bool) -> String {
    let figure = if dark { "#ff7c2f" } else { "#cf530b" };
    let mut markup = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(|c: char| c.is_ascii_digit()) {
        let end = rest[start..]
            .find(|c: char| !c.is_ascii_digit())
            .map_or(rest.len(), |offset| start + offset);
        markup.push_str(&glib::markup_escape_text(&rest[..start]));
        markup.push_str(&format!(
            "<span foreground=\"{figure}\">{}</span>",
            &rest[start..end]
        ));
        rest = &rest[end..];
    }
    markup.push_str(&glib::markup_escape_text(rest));
    markup
}

/// Draw the track, the filled portion, and the percentage pill riding its
/// leading edge.
fn draw_bar(context: &Context, width: f64, height: f64, fraction: f64, scale: f64, dark: bool) {
    let fraction = fraction.clamp(0.0, 1.0);
    let track_height = 7.0 * scale;
    let track_y = (height - track_height) / 2.0;
    let radius = track_height / 2.0;

    rounded_rect(context, 0.0, track_y, width, track_height, radius);
    if dark {
        context.set_source_rgb(0.22, 0.22, 0.22);
    } else {
        context.set_source_rgba(0.0, 0.0, 0.0, 0.08);
    }
    let _ = context.fill();

    let filled = (width * fraction).max(0.0);
    if filled > 0.5 {
        rounded_rect(context, 0.0, track_y, filled, track_height, radius);
        if dark {
            context.set_source_rgb(0.561, 0.227, 0.027);
        } else {
            context.set_source_rgb(1.0, 0.749, 0.6);
        }
        let _ = context.fill();
    }

    let text = format!("{}%", (fraction * 100.0).round() as i32);
    context.select_font_face("Inter", FontSlant::Normal, FontWeight::Bold);
    context.set_font_size(13.0 * scale);
    let Ok(extents) = context.text_extents(&text) else {
        return;
    };
    let pill_height = 19.0 * scale;
    let pill_width = extents.width() + 17.0 * scale;
    let pill_y = (height - pill_height) / 2.0;
    let pill_x = (filled - 3.5 * scale).clamp(0.0, (width - pill_width).max(0.0));

    // A glow trails the pill back along the fill. Stacked bands of the same
    // gradient soften its top and bottom edges, which cairo cannot blur.
    let tail = 31.0 * scale;
    let glow_start = pill_x - tail;
    let glow_width = tail + pill_width * 0.85;
    let glow = LinearGradient::new(glow_start, 0.0, glow_start + glow_width, 0.0);
    glow.add_color_stop_rgba(0.0, 1.0, 0.404, 0.051, 0.0);
    glow.add_color_stop_rgba(0.5 * tail / glow_width, 1.0, 0.404, 0.051, 0.06);
    glow.add_color_stop_rgba(tail / glow_width, 1.0, 0.404, 0.051, 0.25);
    glow.add_color_stop_rgba(1.0, 1.0, 0.404, 0.051, 0.0);
    for inflate in [3.5, 2.0, 0.5] {
        let inflate = inflate * scale;
        rounded_rect(
            context,
            glow_start,
            pill_y - inflate,
            glow_width,
            pill_height + inflate * 2.0,
            pill_height / 2.0 + inflate,
        );
        let _ = context.set_source(&glow);
        let _ = context.fill();
    }

    rounded_rect(
        context,
        pill_x,
        pill_y,
        pill_width,
        pill_height,
        pill_height / 2.0,
    );
    context.set_source_rgb(1.0, 0.404, 0.051);
    let _ = context.fill();

    context.set_source_rgb(1.0, 1.0, 1.0);
    let text_x = pill_x + (pill_width - extents.width()) / 2.0 - extents.x_bearing();
    let text_y = pill_y + (pill_height - extents.height()) / 2.0 - extents.y_bearing();
    context.move_to(text_x, text_y);
    let _ = context.show_text(&text);
}

fn rounded_rect(context: &Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    context.new_sub_path();
    context.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    context.close_path();
}

/// Styles for the card, sized for the display scale like the rest of the
/// Windows editor's sheets.
pub(super) fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let scale = crate::platform::ui_scale();
    let px = |value: f64| (value * scale).round().max(1.0) as i32;
    let font = |value: f64| value * scale;
    let css = format!(
        r#"
        .video-editor-export-scrim {{
            background: alpha(#05070c, 0.55);
        }}
        .video-editor-export-card {{
            background: #0f0f0f;
            border: 1px solid #2c2c2c;
            border-radius: {card_radius}px;
            padding: 0;
        }}
        .video-editor-export-card.editor-theme-light {{
            background: #ffffff;
            border-color: #e2e2e2;
        }}
        .video-editor-export-head {{
            padding: {head_y}px {pad_h}px;
        }}
        .video-editor-export-title,
        .video-editor-export-dot,
        .video-editor-export-meta {{
            font-size: {title_size}px;
            font-weight: 600;
        }}
        .video-editor-export-title {{
            color: #ffffff;
        }}
        .video-editor-export-dot {{
            color: #5b5b5b;
        }}
        .video-editor-export-meta {{
            color: #8a8a8a;
        }}
        .video-editor-export-card.editor-theme-light .video-editor-export-title {{
            color: #141414;
        }}
        .video-editor-export-card.editor-theme-light .video-editor-export-dot {{
            color: #bdbdbd;
        }}
        .video-editor-export-card.editor-theme-light .video-editor-export-meta {{
            color: #7a7a7a;
        }}
        button.video-editor-export-close {{
            min-width: {close_size}px;
            min-height: {close_size}px;
            padding: 0;
            border: none;
            border-radius: {close_radius}px;
            background: transparent;
            box-shadow: none;
            color: #ffffff;
        }}
        button.video-editor-export-close image {{
            -gtk-icon-size: {close_size}px;
        }}
        button.video-editor-export-close:hover {{
            background: alpha(white, 0.08);
        }}
        .video-editor-export-card.editor-theme-light button.video-editor-export-close {{
            color: #141414;
        }}
        .video-editor-export-card.editor-theme-light button.video-editor-export-close:hover {{
            background: alpha(black, 0.06);
        }}
        separator.video-editor-export-rule {{
            min-height: 1px;
            background: #1e1e1e;
        }}
        .video-editor-export-done separator.video-editor-export-rule {{
            margin: 0 {pad_h}px;
        }}
        .video-editor-export-card.editor-theme-light separator.video-editor-export-rule {{
            background: #ececec;
        }}
        .video-editor-export-body {{
            padding: {body_top}px {pad_h}px {body_bottom}px {pad_h}px;
        }}
        .video-editor-export-done .video-editor-export-body {{
            padding: {done_body_y}px {pad_h}px;
        }}
        .video-editor-export-result {{
            color: #d9d9d9;
            font-size: {result_size}px;
            font-weight: 600;
        }}
        .video-editor-export-card.editor-theme-light .video-editor-export-result {{
            color: #262626;
        }}
        .video-editor-export-detail {{
            color: #8a8a8a;
            font-size: {detail_size}px;
            margin-top: {detail_gap}px;
        }}
        .video-editor-export-card.editor-theme-light .video-editor-export-detail {{
            color: #7a7a7a;
        }}
        .video-editor-export-detail.video-editor-export-failed {{
            color: #ff7a6b;
        }}
        .video-editor-export-estimate {{
            margin-top: {estimate_gap}px;
        }}
        .video-editor-export-estimate-label,
        .video-editor-export-estimate-value {{
            font-size: {estimate_size}px;
            font-weight: 600;
        }}
        .video-editor-export-estimate-label {{
            color: #6c6c6c;
        }}
        .video-editor-export-estimate-value {{
            color: #999999;
            font-variant-numeric: tabular-nums;
        }}
        .video-editor-export-card.editor-theme-light .video-editor-export-estimate-label {{
            color: #8a8a8a;
        }}
        .video-editor-export-card.editor-theme-light .video-editor-export-estimate-value {{
            color: #6b6b6b;
        }}
        .video-editor-export-actions {{
            padding: {actions_top}px {pad_h}px {pad_h}px {pad_h}px;
        }}
        button.video-editor-export-dismiss,
        button.video-editor-export-reveal {{
            min-height: {button_height}px;
            padding: 0 {button_pad}px;
            border-radius: {button_radius}px;
            border: none;
            box-shadow: none;
            background-image: none;
            font-size: {button_text}px;
            font-weight: 600;
        }}
        button.video-editor-export-dismiss {{
            background: #1f1f1f;
            color: #bcbcbc;
        }}
        button.video-editor-export-dismiss:hover {{
            background: #262626;
            color: #d9d9d9;
        }}
        button.video-editor-export-reveal {{
            background: #ff670d;
            color: #ffffff;
        }}
        button.video-editor-export-reveal:hover {{
            background: #ff7c2f;
        }}
        button.video-editor-export-reveal:active {{
            background: #cf530b;
        }}
        .video-editor-export-card.editor-theme-light button.video-editor-export-dismiss {{
            background: #f0f0f0;
            color: #5c5c5c;
        }}
        .video-editor-export-card.editor-theme-light button.video-editor-export-dismiss:hover {{
            background: #e8e8e8;
            color: #3a3a3a;
        }}
        "#,
        card_radius = px(12.0),
        pad_h = px(14.0),
        head_y = px(17.0),
        close_size = px(18.0),
        close_radius = px(5.0),
        body_top = px(8.0),
        body_bottom = px(13.0),
        done_body_y = px(15.0),
        detail_gap = px(4.0),
        estimate_gap = px(5.0),
        actions_top = px(15.0),
        button_height = px(30.0),
        button_pad = px(12.0),
        button_radius = px(8.0),
        title_size = font(14.0),
        result_size = font(15.0),
        detail_size = font(11.0),
        estimate_size = font(13.0),
        button_text = font(11.0),
    );
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(&css);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 3,
    );
}
