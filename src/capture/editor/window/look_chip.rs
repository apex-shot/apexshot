//! The first-run "Try a styled look" chip in the image editor header.
//!
//! The chip offers one undoable starting point for an unstyled still: the
//! light blue wallpaper plus the Frame panel's Frost preset. Both edits go
//! through the shared Motion history, so the editor's ordinary Undo button
//! and the chip's own "Undo look" state undo the whole look in one step.
//!
//! It is a local nudge only: the persisted open counter lives in
//! [`crate::studio_nudges`], and the chip disappears once Studio is unlocked
//! or the document already carries a Studio style.

use gtk4::{prelude::*, Align, Box as GtkBox, Button, DrawingArea, Orientation};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::capture::editor::state::EditorState;
use crate::capture::editor::types::FrameStyle;
use crate::i18n::t;
use crate::recording::editor::model::MotionBackgroundFillType;

use super::background_panel;
use super::icon_names;
use super::motion_mode::MotionSession;

/// Image-editor opens that offer the chip.
const NUDGE_OPENS: u64 = 3;
/// Fill band around the card so the chosen wallpaper is actually visible.
const LOOK_PADDING: f64 = 48.0;

/// Build the header chip, or `None` when this window should not show it.
///
/// `on_applied` runs after the look is applied or undone so the Background
/// inspector can be rebuilt from the shared runtime, exactly as a panel
/// interaction would leave it.
pub(super) fn build_look_chip(
    session: &MotionSession,
    state: &Arc<Mutex<EditorState>>,
    preview: &DrawingArea,
    editor_open_count: u64,
    on_applied: Rc<dyn Fn()>,
) -> Option<GtkBox> {
    if editor_open_count > NUDGE_OPENS {
        return None;
    }
    if crate::studio_entitlement::studio_unlocked() {
        return None;
    }
    let already_styled = state
        .lock()
        .map(|state| state.uses_studio_style())
        .unwrap_or(true);
    if already_styled {
        return None;
    }

    let chip = GtkBox::new(Orientation::Horizontal, 2);
    chip.add_css_class("editor-look-chip");
    chip.set_halign(Align::Center);
    chip.set_valign(Align::Center);

    let action = Button::with_label(&t("Try a styled look"));
    action.set_has_frame(false);
    action.set_focusable(false);
    action.add_css_class("editor-look-chip-action");
    action.set_tooltip_text(Some(&t("Try a styled look")));

    let dismiss = Button::new();
    dismiss.set_has_frame(false);
    dismiss.set_focusable(false);
    dismiss.add_css_class("editor-look-chip-dismiss");
    dismiss.set_tooltip_text(Some(&t("Hide suggestion")));
    let dismiss_icon = gtk4::Image::from_icon_name(icon_names::DISMISS_REGULAR);
    dismiss_icon.set_pixel_size(12);
    dismiss.set_child(Some(&dismiss_icon));

    chip.append(&action);
    chip.append(&dismiss);

    dismiss.connect_clicked({
        let chip = chip.downgrade();
        move |_| {
            if let Some(chip) = chip.upgrade() {
                chip.set_visible(false);
            }
        }
    });

    let applied = Rc::new(Cell::new(false));
    action.connect_clicked({
        let session = (*session).clone();
        let preview = (*preview).clone();
        let applied = applied.clone();
        let action = action.downgrade();
        let on_applied = on_applied.clone();
        move |_| {
            if applied.get() {
                let changed = session.runtime.borrow_mut().undo_motion();
                applied.set(false);
                if let Some(action) = action.upgrade() {
                    action.set_label(&t("Try a styled look"));
                    action.set_tooltip_text(Some(&t("Try a styled look")));
                }
                if changed {
                    on_applied();
                    preview.queue_draw();
                }
                return;
            }
            if apply_styled_look(&session, &preview) {
                applied.set(true);
                if let Some(action) = action.upgrade() {
                    action.set_label(&t("Undo look"));
                    action.set_tooltip_text(Some(&t("Undo look")));
                }
                on_applied();
            }
        }
    });

    Some(chip)
}

/// Apply the look as one Motion-history step.
///
/// The setters are the same ones the Background and Style inspectors use:
/// `background_fill_type` / `wallpaper_image_name` / `background_padding`
/// for the fill, and `frame_style` plus its spec's border values for the
/// preset. `begin_motion_command` clears the coalesce window first, so the
/// following panel edit is a separate Undo step.
fn apply_styled_look(session: &MotionSession, preview: &DrawingArea) -> bool {
    let Some(wallpaper) = look_wallpaper() else {
        return false;
    };
    let path = PathBuf::from(&wallpaper);
    let surface = background_panel::load_background_preview_image(
        &path,
        background_panel::PREVIEW_BACKGROUND_MAX_EDGE,
    )
    .and_then(|image| crate::capture::editor::render::rgba_image_to_surface(&image));

    let style = FrameStyle::GlassLight;
    let spec = style.spec();

    let mut runtime = session.runtime.borrow_mut();
    runtime.begin_motion_command();
    {
        let appearance = &mut runtime.motion.appearance;
        appearance.background_fill_type = MotionBackgroundFillType::Wallpaper;
        appearance.wallpaper_image_name = Some(wallpaper.clone());
        // Replace, never stack: a stale custom image must not survive behind
        // the new fill.
        appearance.custom_background_image = None;
        appearance.background_padding = appearance.background_padding.max(LOOK_PADDING);
        appearance.frame_style = style;
        appearance.border_thickness = spec.border_thickness;
        appearance.border_fill_color = [
            spec.border_color.r,
            spec.border_color.g,
            spec.border_color.b,
            spec.border_color.a,
        ];
        if spec.liquid && appearance.border_radius < 12.0 {
            appearance.border_radius = 20.0;
        }
    }
    runtime.set_background_surface(Some(wallpaper), surface, false);
    runtime.preview_frame = None;
    runtime.backdrop_cache = None;
    drop(runtime);

    preview.queue_draw();
    true
}

/// The look's wallpaper: the light blue one, falling back to the first bundled
/// wallpaper when an install ships without it.
fn look_wallpaper() -> Option<String> {
    let path = background_panel::background_gradient_asset_path(LOOK_WALLPAPER_FILE);
    if path.is_file() {
        return Some(path.to_string_lossy().into_owned());
    }
    background_panel::default_motion_wallpaper()
}

/// File name of the wallpaper the look applies.
const LOOK_WALLPAPER_FILE: &str = "wallpaper-001.jpg";
