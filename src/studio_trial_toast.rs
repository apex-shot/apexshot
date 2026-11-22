//! The floating card that confirms a free Studio export and counts the rest.
//!
//! It floats over the editor's own overlay instead of living in a layout, so
//! showing the trial count can never widen a sidebar or push a control. The
//! card leaves after a few seconds, and only the card itself claims input, so
//! the canvas underneath stays usable.
//!
//! Editors with a floating top toolbar hang the card just below it through
//! [`show_anchored`]; the editors without one keep the bottom-centre card from
//! [`show`].

use crate::i18n::{t, tfmt};
use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, EventControllerMotion, Label, Orientation, Overlay,
    Revealer, RevealerTransitionType, Window,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

const TOAST_NAME: &str = "apexshot-studio-toast";
const TOAST_WIDTH: f64 = 440.0;
const TOAST_MARGIN: f64 = 24.0;
/// Smallest gap the anchored card keeps from the overlay's side edges.
const TOAST_EDGE: f64 = 8.0;
const DITHER_HEIGHT: f64 = 30.0;
const VISIBLE_FOR: Duration = Duration::from_secs(8);
const HOVER_RESUME: Duration = Duration::from_secs(4);

/// A toolbar-relative home for the trial card.
///
/// `anchor` reports `(center_x, top_y)` in `overlay` coordinates: the centre
/// of the floating toolbar and the top of the free band under it (and under a
/// docked tool bar when one is up). Returning `None` keeps the card at the
/// overlay's bottom centre for that frame.
#[derive(Clone)]
pub(crate) struct Anchor {
    overlay: Overlay,
    anchor: Rc<dyn Fn() -> Option<(f64, f64)>>,
}

impl Anchor {
    pub(crate) fn new(overlay: Overlay, anchor: Rc<dyn Fn() -> Option<(f64, f64)>>) -> Self {
        Self { overlay, anchor }
    }
}

/// The bold line and the muted line the card shows for `remaining`.
pub(crate) fn toast_lines(remaining: u32) -> (String, String) {
    match remaining {
        0 => (
            t("That was your last free Studio export."),
            t("Get Studio to keep these styles."),
        ),
        1 => (t("Exported with Studio."), t("1 free Studio export left.")),
        count => {
            let count = count.to_string();
            (
                t("Exported with Studio."),
                tfmt("{count} free Studio exports left.", &[("count", &count)]),
            )
        }
    }
}

/// Raise the card under `anchor`'s band when that surface is on screen, and
/// fall back to the window's editor overlay otherwise (for example while the
/// Motion page covers the static canvas).
///
/// Returns false when neither surface could host the card, so callers can fall
/// back to a desktop notification.
pub(crate) fn show_for(window: &impl IsA<Window>, anchor: Option<&Anchor>, remaining: u32) -> bool {
    match anchor {
        Some(anchor) if anchor.overlay.is_mapped() => {
            show_anchored(&anchor.overlay, anchor.anchor.clone(), remaining)
        }
        _ => show(window, remaining),
    }
}

/// Raise the trial toast over `window`'s editor overlay, bottom centre.
///
/// Returns false when the window has no overlay to host it, so callers can
/// fall back to a desktop notification.
pub(crate) fn show(window: &impl IsA<Window>, remaining: u32) -> bool {
    let Some(overlay) = crate::studio_upsell::overlay_for(window) else {
        return false;
    };
    raise(&overlay, None, remaining);
    true
}

/// Raise the trial toast on `overlay`, centred on the band `anchor` reports.
///
/// The card follows the anchor every frame, so it moves when a tool's
/// secondary bar appears or disappears under the toolbar. It never takes part
/// in a layout, so nothing else shifts.
pub(crate) fn show_anchored(
    overlay: &Overlay,
    anchor: Rc<dyn Fn() -> Option<(f64, f64)>>,
    remaining: u32,
) -> bool {
    raise(overlay, Some(anchor), remaining);
    true
}

fn raise(overlay: &Overlay, anchor: Option<Rc<dyn Fn() -> Option<(f64, f64)>>>, remaining: u32) {
    crate::studio_upsell::install_css();
    remove_existing(overlay);

    let anchored = anchor.is_some();
    // The theme class sits on the window's root overlay, above the canvas
    // overlay the anchored card rides, so judge from the toplevel.
    let dark = match overlay.root().and_downcast::<gtk4::Window>() {
        Some(window) => crate::studio_upsell::theme_is_dark(&window),
        None => crate::studio_upsell::theme_is_dark(overlay),
    };
    let (title_text, count_text) = toast_lines(remaining);

    let card = GtkBox::new(Orientation::Vertical, 0);
    card.add_css_class("apexshot-studio-toast");
    card.set_overflow(gtk4::Overflow::Hidden);
    card.set_size_request(crate::studio_upsell::px(TOAST_WIDTH), -1);
    card.set_halign(if anchored {
        Align::Start
    } else {
        Align::Center
    });
    card.set_valign(if anchored { Align::Start } else { Align::End });
    if !anchored {
        card.set_margin_bottom(crate::studio_upsell::px(TOAST_MARGIN));
    }
    card.set_can_target(true);
    card.set_can_focus(false);
    if !dark {
        card.add_css_class(crate::studio_upsell::LIGHT_CLASS);
    }

    // A thin shimmering strip carries the same accent as the Studio sheet.
    card.append(&crate::dither_field::dither_field(dark, DITHER_HEIGHT));

    let row = GtkBox::new(Orientation::Horizontal, crate::studio_upsell::px(12.0));
    row.add_css_class("apexshot-studio-toast-row");
    row.set_valign(Align::Center);

    let copy = GtkBox::new(Orientation::Vertical, 2);
    copy.set_hexpand(true);
    copy.set_valign(Align::Center);
    let title = Label::new(Some(&title_text));
    title.add_css_class("apexshot-studio-toast-title");
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.set_max_width_chars(34);
    let count = Label::new(Some(&count_text));
    count.add_css_class("apexshot-studio-toast-subtitle");
    count.set_xalign(0.0);
    count.set_wrap(true);
    count.set_max_width_chars(34);
    copy.append(&title);
    copy.append(&count);
    row.append(&copy);

    let get_studio = crate::studio_upsell::primary_button(&t("Get Studio"));
    get_studio.add_css_class("apexshot-studio-toast-action");
    get_studio.set_valign(Align::Center);
    row.append(&get_studio);

    let close = crate::studio_upsell::close_button();
    close.add_css_class("apexshot-studio-toast-close");
    close.set_valign(Align::Center);
    row.append(&close);

    card.append(&row);

    let revealer = Revealer::new();
    revealer.set_widget_name(TOAST_NAME);
    // The anchored card drops out from under the toolbar; the bottom one
    // still rises from the edge.
    revealer.set_transition_type(if anchored {
        RevealerTransitionType::SlideDown
    } else {
        RevealerTransitionType::SlideUp
    });
    revealer.set_transition_duration(250);
    revealer.set_halign(if anchored {
        Align::Start
    } else {
        Align::Center
    });
    revealer.set_valign(if anchored { Align::Start } else { Align::End });
    revealer.set_can_target(false);
    revealer.set_child(Some(&card));

    // While mapped, the anchored card re-reads the band each frame. Margins
    // are the only thing it writes, so it never reflows the editor.
    let tick: Rc<RefCell<Option<gtk4::TickCallbackId>>> = Rc::new(RefCell::new(None));
    if let Some(anchor) = anchor {
        let overlay_weak = overlay.downgrade();
        let id = revealer.add_tick_callback(move |revealer, _| {
            let Some(overlay) = overlay_weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let overlay_w = f64::from(overlay.width());
            if overlay_w <= 1.0 {
                return glib::ControlFlow::Continue;
            }
            let measured = f64::from(revealer.width());
            let card_w = if measured > 1.0 {
                measured
            } else {
                f64::from(crate::studio_upsell::px(TOAST_WIDTH))
            };
            let (center_x, top_y) = anchor().unwrap_or_else(|| {
                // No toolbar in reach: keep the familiar bottom-centre spot.
                let card_h = f64::from(revealer.height());
                let bottom_top = f64::from(overlay.height())
                    - card_h
                    - f64::from(crate::studio_upsell::px(TOAST_MARGIN));
                (overlay_w / 2.0, bottom_top)
            });
            let (left, top) = anchor_margins(center_x, top_y, card_w, overlay_w);
            if revealer.margin_start() != left || revealer.margin_top() != top {
                revealer.set_margin_start(left);
                revealer.set_margin_top(top);
            }
            glib::ControlFlow::Continue
        });
        *tick.borrow_mut() = Some(id);
    }

    let detach: Rc<dyn Fn()> = Rc::new({
        let tick = tick.clone();
        move || {
            if let Some(id) = tick.borrow_mut().take() {
                id.remove();
            }
        }
    });

    let timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let revealed = Rc::new(Cell::new(false));
    let hide: Rc<dyn Fn()> = Rc::new({
        let overlay = overlay.downgrade();
        let revealer = revealer.downgrade();
        let revealed = revealed.clone();
        let detach = detach.clone();
        move || {
            let Some(revealer) = revealer.upgrade() else {
                return;
            };
            // Closing before the entrance ran leaves nothing to slide out, so
            // there is no transition notification to remove it later.
            if !revealed.replace(false) {
                detach();
                if let Some(overlay) = overlay.upgrade() {
                    if revealer.parent().is_some() {
                        overlay.remove_overlay(&revealer);
                    }
                }
                return;
            }
            revealer.set_reveal_child(false);
        }
    });

    // The revealer leaves the overlay only once its slide-out finishes, so a
    // dismissed card never pops off mid-animation.
    revealer.connect_child_revealed_notify({
        let overlay = overlay.downgrade();
        let detach = detach.clone();
        move |revealer| {
            if revealer.is_child_revealed() {
                return;
            }
            detach();
            if let Some(overlay) = overlay.upgrade() {
                if revealer.parent().is_some() {
                    overlay.remove_overlay(revealer);
                }
            }
        }
    });

    close.connect_clicked({
        let hide = hide.clone();
        let timer = timer.clone();
        move |_| {
            cancel(&timer);
            hide()
        }
    });
    get_studio.connect_clicked(|_| {
        let _ = crate::utils::open::open_url(&crate::studio_entitlement::studio_page_url());
    });

    // Hovering holds the card open; leaving gives it a shorter grace period.
    let motion = EventControllerMotion::new();
    motion.connect_enter({
        let timer = timer.clone();
        move |_, _, _| cancel(&timer)
    });
    motion.connect_leave({
        let timer = timer.clone();
        let hide = hide.clone();
        move |_| schedule(&timer, HOVER_RESUME, hide.clone())
    });
    card.add_controller(motion);

    overlay.add_overlay(&revealer);
    glib::idle_add_local_once({
        let revealer = revealer.downgrade();
        move || {
            if let Some(revealer) = revealer.upgrade() {
                revealed.set(true);
                revealer.set_reveal_child(true);
            }
        }
    });
    schedule(&timer, VISIBLE_FOR, hide);
}

/// Only one toast exists per overlay: clear any previous one first.
fn remove_existing(overlay: &Overlay) {
    let mut child = overlay.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        if widget.widget_name().as_str() == TOAST_NAME {
            overlay.remove_overlay(&widget);
        }
        child = next;
    }
}

/// Margins that centre a card of `card_w` on `center_x` while keeping it at
/// least [`TOAST_EDGE`] inside an overlay `overlay_w` wide, plus the rounded
/// top. Split out so the edge cases are testable without a toolkit.
fn anchor_margins(center_x: f64, top_y: f64, card_w: f64, overlay_w: f64) -> (i32, i32) {
    let max_left = (overlay_w - card_w - TOAST_EDGE).max(TOAST_EDGE);
    let left = (center_x - card_w / 2.0)
        .round()
        .clamp(TOAST_EDGE, max_left);
    (left as i32, top_y.round() as i32)
}

fn schedule(timer: &Rc<RefCell<Option<glib::SourceId>>>, delay: Duration, hide: Rc<dyn Fn()>) {
    cancel(timer);
    let slot = timer.clone();
    let source = glib::timeout_add_local_once(delay, move || {
        slot.borrow_mut().take();
        hide();
    });
    *timer.borrow_mut() = Some(source);
}

fn cancel(timer: &Rc<RefCell<Option<glib::SourceId>>>) {
    if let Some(source) = timer.borrow_mut().take() {
        source.remove();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_names_the_remaining_exports() {
        assert_eq!(
            toast_lines(3),
            (
                t("Exported with Studio."),
                tfmt("{count} free Studio exports left.", &[("count", "3")]),
            )
        );
        assert_eq!(
            toast_lines(1),
            (t("Exported with Studio."), t("1 free Studio export left."),)
        );
        assert_eq!(
            toast_lines(0),
            (
                t("That was your last free Studio export."),
                t("Get Studio to keep these styles."),
            )
        );
    }

    #[test]
    fn the_anchored_card_centres_under_its_anchor() {
        assert_eq!(anchor_margins(500.0, 120.0, 440.0, 1000.0), (280, 120));
    }

    #[test]
    fn the_anchored_card_clamps_to_the_left_edge() {
        assert_eq!(anchor_margins(100.0, 120.0, 440.0, 1000.0), (8, 120));
    }

    #[test]
    fn the_anchored_card_clamps_to_the_right_edge() {
        // 990 - 220 = 770, but the overlay only allows 1000 - 440 - 8 = 552.
        assert_eq!(anchor_margins(990.0, 120.0, 440.0, 1000.0), (552, 120));
    }

    #[test]
    fn the_anchored_card_never_leaves_a_narrow_overlay() {
        assert_eq!(anchor_margins(500.0, 120.0, 440.0, 300.0), (8, 120));
    }
}
