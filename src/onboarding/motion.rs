use gtk4::prelude::*;
use gtk4::{glib, Widget};

pub fn enabled() -> bool {
    gtk4::Settings::default().is_none_or(|settings| settings.is_gtk_enable_animations())
}

pub fn ease_out_cubic(progress: f64) -> f64 {
    1.0 - (1.0 - progress.clamp(0.0, 1.0)).powi(3)
}

pub fn reveal(
    widgets: &[Widget],
    stagger_ms: f64,
    duration_ms: f64,
) -> Option<gtk4::TickCallbackId> {
    if widgets.is_empty() || !enabled() {
        return None;
    }
    for widget in widgets {
        widget.set_opacity(0.0);
    }
    let anchor = widgets[0].clone();
    let widgets = widgets.to_vec();
    let started = std::cell::Cell::new(None::<i64>);
    Some(anchor.add_tick_callback(move |anchor, clock| {
        if !anchor.is_mapped() {
            for widget in &widgets {
                widget.set_opacity(1.0);
            }
            return glib::ControlFlow::Break;
        }
        let now = clock.frame_time();
        let start = started.get().unwrap_or(now);
        started.set(Some(start));
        let elapsed_ms = (now - start) as f64 / 1000.0;
        let mut pending = false;
        for (index, widget) in widgets.iter().enumerate() {
            let progress = (elapsed_ms - index as f64 * stagger_ms) / duration_ms;
            widget.set_opacity(ease_out_cubic(progress));
            pending |= progress < 1.0;
        }
        if pending {
            glib::ControlFlow::Continue
        } else {
            glib::ControlFlow::Break
        }
    }))
}
