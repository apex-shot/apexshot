//! Shared widgets for the per-tool floating bars (text, obfuscate, number, highlighter, pen, arrow, shape).
//!
//! Each bar is a small dark bar of pills that either follows the element being
//! edited or docks in the chrome band above the canvas. The pill, popover row, and
//! state-sync plumbing is identical across tools, so it lives here rather than in
//! every bar module — including [`dock_position`], which keeps a docked bar off the
//! image so it cannot steal a press meant for the canvas.

use std::rc::Rc;

use gtk4::{
    glib, prelude::*, Align, Box as GtkBox, Button, DrawingArea, Image, Label, Orientation,
    Popover, PositionType, ScrolledWindow,
};

use crate::capture::editor::ui_support::{DockedBarInset, EDITOR_TOP_CHROME_HEIGHT};
use crate::i18n::t;

use super::icon_names;

/// Padding kept between a docked bar and the toolbar above it, or the canvas below it.
pub(super) const DOCK_PAD: f64 = 8.0;
/// Minimum margin from the viewport edges.
const DOCK_EDGE: f64 = 12.0;

/// A rectangle as `(x, y, width, height)`.
pub(super) type Rect4 = (f64, f64, f64, f64);

/// Widgets a docked bar measures against: the canvas scroller, so it stays centered in
/// the viewport it was docked into.
pub(super) struct DockRefs {
    pub scroller: ScrolledWindow,
    pub drawing_area: DrawingArea,
}

/// Shortest/longest real bar height the reserve will believe (see [`dock_reserve`]).
const MIN_BAR_HEIGHT: f64 = 26.0;
const MAX_BAR_HEIGHT: f64 = 64.0;

/// Space the canvas has to give up so a docked bar of this height sits clear of it.
///
/// The image is what makes room: the layout and the draw transform both add this, so
/// docking pushes the image down instead of floating a bar over the top of it.
///
/// The height is clamped because a bar reports `0` until GTK has allocated it. Taking
/// that literally would shrink the reserve for a frame, and switching between two
/// tools that both dock a bar would bounce the image up and straight back down.
pub(super) fn dock_reserve(bar_h: f64) -> f64 {
    bar_h.clamp(MIN_BAR_HEIGHT, MAX_BAR_HEIGHT) + 2.0 * DOCK_PAD
}

/// Where a docked bar goes: centered, in the band the canvas just gave up.
///
/// Nothing to measure: the bar sits one padding below the reserved chrome strip and
/// the canvas starts `dock_reserve` lower, so the two can never meet.
pub(super) fn dock_position(refs: &DockRefs, bar_w: f64) -> (f64, f64) {
    let viewport = refs.scroller.compute_bounds(&refs.drawing_area);
    let Some(viewport) = viewport else {
        return (DOCK_EDGE, f64::from(EDITOR_TOP_CHROME_HEIGHT) + DOCK_PAD);
    };
    plan_docked_bar(
        (
            viewport.x() as f64,
            viewport.y() as f64,
            viewport.width() as f64,
            viewport.height() as f64,
        ),
        f64::from(EDITOR_TOP_CHROME_HEIGHT),
        bar_w,
    )
}

/// Show or hide a floating bar without unmapping it.
///
/// A widget that is `set_visible(false)` gets no allocation, so `width()`/`height()`
/// read 0 on the frame it comes back — which parks it off-centre and briefly shrinks
/// the band the canvas reserved. Staying mapped and merely transparent keeps its size
/// answerable at all times, so a tool switch is a straight swap with no reflow bounce.
pub(super) fn set_bar_shown(root: &GtkBox, shown: bool) {
    let opacity = if shown { 1.0 } else { 0.0 };
    if (root.opacity() - opacity).abs() > f64::EPSILON {
        root.set_opacity(opacity);
    }
    if root.can_target() != shown {
        root.set_can_target(shown);
    }
    if !shown && root.is_sensitive() {
        if let Some(window) = root
            .root()
            .and_then(|root| root.downcast::<gtk4::Window>().ok())
        {
            if let Some(focus) = gtk4::prelude::RootExt::focus(&window) {
                if focus.is_ancestor(root) {
                    gtk4::prelude::GtkWindowExt::set_focus(&window, Option::<&gtk4::Widget>::None);
                }
            }
        }
    }
    if root.is_sensitive() != shown {
        root.set_sensitive(shown);
    }
}

/// Whether a floating bar currently occupies its docked band.
///
/// [`set_bar_shown`] keeps bars mapped and only drops their opacity, so a
/// plain visibility check would count every hidden bar. Allocated height is
/// the other half: a bar reports `0` until GTK has laid it out.
pub(super) fn bar_is_shown(root: &GtkBox) -> bool {
    root.opacity() > 0.0 && root.is_mapped() && root.height() > 0
}

/// Claim (or give back) the canvas space above a docked bar.
///
/// The layout and the draw transform both read the inset, so the canvas has to repaint
/// when it changes — otherwise the image would not visibly move out of the way.
pub(super) fn set_dock_reserve(
    inset: &DockedBarInset,
    bar: &'static str,
    reserve: f64,
    area: &DrawingArea,
) {
    if inset.set(bar, reserve) {
        area.queue_draw();
    }
}

/// Pure placement math for [`dock_position`], split out so the cases are testable.
///
/// `viewport` is the visible canvas rect in the drawing area's coordinates, so `y` is
/// the scroll offset: adding it keeps the bar pinned to the chrome instead of
/// scrolling away with the image. The horizontal position is always the centered one —
/// a docked bar that jumps next to the toolbar is disorienting.
pub(super) fn plan_docked_bar(viewport: Rect4, strip_bottom: f64, bar_w: f64) -> (f64, f64) {
    let (vx, vy, vw, _vh) = viewport;
    let left = vx + ((vw - bar_w) / 2.0).max(DOCK_EDGE);
    let top = vy + strip_bottom + DOCK_PAD;
    (left, top)
}

pub(super) struct Pill {
    pub button: Button,
    pub label: Label,
    pub list: GtkBox,
}

/// Text-only pill: its label is the value it shows, so a leading glyph would only
/// add width to a bar that floats over the canvas.
pub(super) fn build_pill(tooltip: &str) -> Pill {
    let button = Button::new();
    button.set_has_frame(false);
    button.set_focusable(false);
    button.add_css_class("editor-tool-button");
    button.add_css_class("flat");
    button.set_tooltip_text(Some(&t(tooltip)));

    let row = GtkBox::new(Orientation::Horizontal, 6);
    row.set_halign(Align::Center);
    row.set_valign(Align::Center);
    let label = Label::new(None);
    let chevron = Image::from_icon_name(icon_names::CHEVRON_DOWN_REGULAR);
    chevron.set_pixel_size(10);
    row.append(&label);
    row.append(&chevron);
    button.set_child(Some(&row));

    let popover = Popover::new();
    popover.set_has_arrow(false);
    popover.set_autohide(true);
    popover.set_position(PositionType::Bottom);
    popover.set_offset(0, 8);
    popover.add_css_class("editor-popover");
    popover.set_parent(&button);
    let list = GtkBox::new(Orientation::Vertical, 0);
    list.add_css_class("editor-popover-list");
    popover.set_child(Some(&list));
    button.connect_clicked(move |_| popover.popup());

    Pill {
        button,
        label,
        list,
    }
}

/// Popover row: label on the left, tick on the right, revealed when selected.
pub(super) fn build_option_row(label: &str, check_class: &str) -> Button {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.set_margin_start(0);
    row.set_margin_end(0);
    row.set_margin_top(0);
    row.set_margin_bottom(0);

    let text = Label::new(Some(label));
    text.set_hexpand(true);
    text.set_xalign(0.0);

    let check = Image::from_icon_name(icon_names::custom::CHECK_SYMBOLIC);
    check.set_pixel_size(12);
    check.set_visible(false);
    check.add_css_class(check_class);

    row.append(&text);
    row.append(&check);

    let option = Button::builder()
        .has_frame(false)
        .css_classes(["editor-popover-list-item", "flat"])
        .child(&row)
        .build();
    option.set_size_request(-1, 32);
    option
}

/// Wire every option row in `list`, handing the row index and its button to `on_click`.
pub(super) fn wire_option_rows(
    list: &GtkBox,
    row_class: &'static str,
    on_click: impl Fn(usize, &Button) + 'static,
) {
    let on_click = Rc::new(on_click);
    let mut child_opt = list.first_child();
    let mut index = 0usize;
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();

        let Ok(button) = child.clone().downcast::<Button>() else {
            continue;
        };
        if !button.css_classes().iter().any(|class| class == row_class) {
            continue;
        }
        let on_click = on_click.clone();
        button.connect_clicked(move |button| on_click(index, button));
        index += 1;
    }
}

pub(super) fn sync_option_selection(list: &GtkBox, selected_index: usize, active_class: &str) {
    let mut child_opt = list.first_child();
    let mut index = 0usize;
    while let Some(child) = child_opt {
        child_opt = child.next_sibling();

        let Ok(button) = child.downcast::<Button>() else {
            continue;
        };

        let is_active = index == selected_index;
        if is_active {
            button.add_css_class(active_class);
        } else {
            button.remove_css_class(active_class);
        }
        if let Some(content) = button.child() {
            if let Ok(row) = content.downcast::<GtkBox>() {
                if let Some(check) = row.last_child() {
                    check.set_visible(is_active);
                }
            }
        }

        index += 1;
    }
}

pub(super) fn popdown_for(button: &Button) {
    if let Some(popover) = button.ancestor(Popover::static_type()) {
        if let Ok(popover) = popover.downcast::<Popover>() {
            popover.popdown();
        }
    }
}

pub(super) fn queue_draw(area: &glib::object::WeakRef<DrawingArea>) {
    if let Some(area) = area.upgrade() {
        area.queue_draw();
    }
}

/// Move an anchored bar, skipping writes that would not change the layout.
///
/// Visibility is owned by [`set_bar_shown`], so this only ever places the bar.
pub(super) fn move_bar(bar: &GtkBox, left: f64, top: f64) {
    if (bar.margin_start() as f64 - left).abs() >= 1.0
        || (bar.margin_top() as f64 - top).abs() >= 1.0
    {
        bar.set_margin_start(left as i32);
        bar.set_margin_top(top as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        dock_reserve, plan_docked_bar, DOCK_EDGE, DOCK_PAD, MAX_BAR_HEIGHT, MIN_BAR_HEIGHT,
    };
    use crate::capture::editor::ui_support::DockedBarInset;

    fn production_source() -> &'static str {
        let source = include_str!("floating_bar.rs");
        source.split("#[cfg(test)]").next().unwrap_or(source)
    }

    #[test]
    fn pills_are_text_only() {
        let source = production_source();
        assert!(
            !source.contains("NUMBER_CIRCLE_1_REGULAR")
                && source.contains("let label = Label::new(None);")
                && source.contains("row.append(&label);")
                && source.contains("row.append(&chevron);"),
            "Pills show their value plus a chevron, with no leading glyph"
        );
    }

    #[test]
    fn option_rows_reuse_the_shared_popover_styling() {
        let source = production_source();
        assert!(
            source.contains("popover.add_css_class(\"editor-popover\");")
                && source.contains("list.add_css_class(\"editor-popover-list\");")
                && source.contains("\"editor-popover-list-item\""),
            "Floating-bar popovers should reuse the shared editor popover styling"
        );
    }

    /// The reserved chrome strip the toolbar floats in.
    const STRIP_BOTTOM: f64 = 56.0;

    #[test]
    fn bars_toggle_opacity_instead_of_being_unmapped() {
        let source = production_source();
        assert!(
            source.contains("pub(super) fn set_bar_shown(")
                && source.contains("root.set_opacity(opacity);")
                && source.contains("root.set_can_target(shown);"),
            "Hiding a bar must leave it mapped so its measured size stays available"
        );
        for bar in [
            include_str!("number_bar.rs"),
            include_str!("highlighter_bar.rs"),
            include_str!("arrow_bar.rs"),
            include_str!("stroke_bar.rs"),
            include_str!("text_bar.rs"),
        ] {
            let production = bar.split("#[cfg(test)]").next().unwrap_or(bar);
            assert!(
                !production.contains("set_visible(false)") && production.contains("set_bar_shown("),
                "Docked bars must be shown/hidden through set_bar_shown, never set_visible"
            );
        }
    }

    #[test]
    fn every_bar_reveals_itself_before_it_can_bail_out() {
        // The bars are opacity-hidden, so a tick that returns before revealing leaves
        // the bar invisible for good — exactly how the number bar went missing once.
        // Docked bars only: the anchored ones (text, obfuscate, focus) come and go with
        // the element they follow and are allowed to map/unmap.
        for (name, bar) in [
            ("number_bar.rs", include_str!("number_bar.rs")),
            ("highlighter_bar.rs", include_str!("highlighter_bar.rs")),
            ("arrow_bar.rs", include_str!("arrow_bar.rs")),
            ("stroke_bar.rs", include_str!("stroke_bar.rs")),
            ("text_bar.rs", include_str!("text_bar.rs")),
        ] {
            let production = bar.split("#[cfg(test)]").next().unwrap_or(bar);
            let reveal = production
                .find("set_bar_shown(&root, show);")
                .unwrap_or_else(|| panic!("{name}: the bar never reveals itself"));
            let bail_out = production
                .find("return glib::ControlFlow::Continue;")
                .unwrap_or_else(|| panic!("{name}: tick has no early return to compare"));
            assert!(
                reveal < bail_out,
                "{name}: the bar must be revealed before the tick can return early"
            );
        }
    }

    #[test]
    fn docked_bars_reserve_the_space_they_actually_need() {
        // A remembered or guessed bar size (the number bar once kept a 320x44 default)
        // makes one tool push the image down further than another with the same CSS.
        for (name, bar) in [
            ("number_bar.rs", include_str!("number_bar.rs")),
            ("highlighter_bar.rs", include_str!("highlighter_bar.rs")),
            ("arrow_bar.rs", include_str!("arrow_bar.rs")),
            ("stroke_bar.rs", include_str!("stroke_bar.rs")),
            ("text_bar.rs", include_str!("text_bar.rs")),
        ] {
            let production = bar.split("#[cfg(test)]").next().unwrap_or(bar);
            assert!(
                production
                    .contains("let (bar_w, bar_h) = (root.width() as f64, inner.height() as f64);")
                    && !production.contains("Cell::new(("),
                "{name}: reserve from the measured bar, not from a remembered guess"
            );
            let claimed = production.matches("dock_reserve(bar_h)").count();
            let reserved = production.matches("dock_reserve(").count();
            // `set_dock_reserve(` contains the same substring, so the claim is the one
            // occurrence beyond the release calls.
            let releases = production.matches("set_dock_reserve(").count();
            assert_eq!(
                claimed,
                reserved - releases,
                "{name}: the reserve must always come from the measured height"
            );
        }
    }

    #[test]
    fn a_hidden_bar_never_clears_another_bars_reserve() {
        // Every bar ticks independently, so releasing one must not wipe out the claim
        // of the bar that is actually showing.
        let inset = DockedBarInset::new();
        assert!(inset.set("pen", dock_reserve(30.0)));
        assert!(
            !inset.set("pen", dock_reserve(30.0)),
            "same claim is a no-op"
        );
        assert!(
            !inset.set("highlighter", 0.0),
            "an idle bar cannot change it"
        );

        assert_eq!(inset.px(), dock_reserve(30.0));
        assert!(inset.set("pen", 0.0));
        assert_eq!(inset.px(), 0.0, "the canvas gets its space back");
    }

    #[test]
    fn the_canvas_reserves_the_tallest_docked_bar() {
        let inset = DockedBarInset::new();
        inset.set("pen", 26.0);
        inset.set("highlighter", 44.0);
        assert_eq!(inset.px(), 44.0);
    }

    #[test]
    fn dock_reserve_leaves_padding_above_and_below_the_bar() {
        assert_eq!(dock_reserve(30.0), 30.0 + 2.0 * DOCK_PAD);
    }

    #[test]
    fn dock_reserve_never_dips_for_an_unallocated_bar() {
        // `0` means "not measured yet": believing it would collapse the reserve and
        // bounce the image every time the tool changes.
        assert_eq!(dock_reserve(0.0), MIN_BAR_HEIGHT + 2.0 * DOCK_PAD);
        assert_eq!(dock_reserve(30.0), 30.0 + 2.0 * DOCK_PAD);
        assert_eq!(dock_reserve(500.0), MAX_BAR_HEIGHT + 2.0 * DOCK_PAD);
    }

    #[test]
    fn switching_between_docked_bars_never_gives_the_space_back() {
        // Pen hands over to the highlighter: the pen releases on the same frame the
        // highlighter (still unmeasured) claims, so the reserve must not dip.
        let inset = DockedBarInset::new();
        inset.set("pen", dock_reserve(30.0));
        let settled = inset.px();

        inset.set("pen", 0.0);
        inset.set("highlighter", dock_reserve(0.0));
        assert!(
            inset.px() >= settled - 4.0,
            "handover kept the band, got {} from {settled}",
            inset.px()
        );
    }

    #[test]
    fn docked_bar_is_centered_just_below_the_chrome_strip() {
        let (left, top) = plan_docked_bar((0.0, 0.0, 1000.0, 800.0), STRIP_BOTTOM, 220.0);

        assert_eq!(left, (1000.0 - 220.0) / 2.0, "centered in the viewport");
        assert_eq!(top, STRIP_BOTTOM + DOCK_PAD, "one padding below the strip");
    }

    #[test]
    fn docked_bar_stays_pinned_to_the_chrome_when_the_canvas_is_scrolled() {
        // Scrolled 120px down: the bar tracks the viewport, not the image.
        let (left, top) = plan_docked_bar((0.0, 120.0, 1000.0, 800.0), STRIP_BOTTOM, 220.0);

        assert_eq!(top, 120.0 + STRIP_BOTTOM + DOCK_PAD);
        assert_eq!(left, (1000.0 - 220.0) / 2.0);
    }

    #[test]
    fn docked_bar_never_covers_the_canvas_that_gave_up_the_space() {
        // The canvas starts at `canvas padding + strip + reserve` — the bar of any
        // height ends at least one padding above it, so it can never eat a press.
        for bar_h in [26.0, 30.0, 44.0] {
            let (_left, top) = plan_docked_bar((0.0, 0.0, 1000.0, 800.0), STRIP_BOTTOM, 220.0);
            let canvas_top = 24.0 + STRIP_BOTTOM + dock_reserve(bar_h);
            assert!(
                top + bar_h + DOCK_PAD <= canvas_top,
                "a {bar_h}px bar must clear the canvas, got top {top} of {canvas_top}"
            );
        }
    }

    #[test]
    fn docked_bar_never_moves_sideways() {
        // Zoomed, narrow, scrolled: the horizontal position is always the centered one.
        for viewport in [
            (0.0, 0.0, 1000.0, 800.0),
            (0.0, 120.0, 1400.0, 800.0),
            (0.0, 400.0, 700.0, 500.0),
            (0.0, 0.0, 200.0, 500.0),
        ] {
            let (left, _top) = plan_docked_bar(viewport, STRIP_BOTTOM, 220.0);
            assert_eq!(
                left,
                (viewport.0 + (viewport.2 - 220.0) / 2.0).max(viewport.0 + DOCK_EDGE),
                "docked bars stay centered for viewport {viewport:?}"
            );
        }
    }

    #[test]
    fn hiding_a_bar_releases_descendant_focus_and_tab_traversal() {
        use super::set_bar_shown;
        use gtk4::prelude::*;
        let Some(()) = crate::test_support::with_gtk(|| {
            let window = gtk4::Window::new();
            let root = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
            let entry = gtk4::Entry::new();
            root.append(&entry);
            window.set_child(Some(&root));
            window.present();
            while gtk4::glib::MainContext::default().iteration(false) {}
            entry.grab_focus();
            set_bar_shown(&root, false);
            assert!(gtk4::prelude::RootExt::focus(&window)
                .is_none_or(|focus| !focus.is_ancestor(&root)));
            assert!(!entry.is_sensitive());
            set_bar_shown(&root, true);
            assert!(entry.is_sensitive());
            window.close();
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }

    #[test]
    fn option_rows_carry_a_trailing_bundled_check_that_selection_reveals() {
        use super::{build_option_row, sync_option_selection};
        use gtk4::prelude::*;
        let Some(()) = crate::test_support::with_gtk(|| {
            let list = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            for label in ["One", "Two"] {
                let row = build_option_row(label, "editor-crop-ratio-check");
                row.add_css_class("editor-crop-ratio-option");
                list.append(&row);
            }
            let check_of = |index: usize| {
                let mut child = list.first_child();
                for _ in 0..index {
                    child = child.and_then(|widget| widget.next_sibling());
                }
                let button = child
                    .and_then(|widget| widget.downcast::<gtk4::Button>().ok())
                    .expect("an option button");
                button
                    .child()
                    .and_then(|child| child.downcast::<gtk4::Box>().ok())
                    .and_then(|row| row.last_child())
                    .and_then(|check| check.downcast::<gtk4::Image>().ok())
                    .expect("the row's trailing child must be the bundled check image")
            };
            for index in 0..2 {
                let check = check_of(index);
                assert_eq!(
                    check.icon_name().as_deref(),
                    Some(super::icon_names::custom::CHECK_SYMBOLIC)
                );
                assert_eq!(check.pixel_size(), 12);
                assert!(!check.get_visible());
            }
            sync_option_selection(&list, 1, "editor-crop-ratio-option-active");
            assert!(!check_of(0).get_visible());
            assert!(check_of(1).get_visible());
            sync_option_selection(&list, 0, "editor-crop-ratio-option-active");
            assert!(check_of(0).get_visible());
            assert!(!check_of(1).get_visible());
        }) else {
            eprintln!("skipping: GTK unavailable");
            return;
        };
    }
}
