//! The first-run "Try a styled look" chip in the image editor header.
//!
//! The chip walks a short curated list of starting points for an unstyled
//! still: a wallpaper, the Frame panel's matching preset, a card shadow, and
//! where the look calls for it a 3D pose or an off-canvas bleed. The Appearance
//! fields are applied as one Motion-history step and the pose as one still
//! step, exactly the way the Background/Appearance and Transform panels record
//! their own edits.
//!
//! Those two halves live in different histories, so the chip never relies on a
//! blind Undo to remove its look: it saves the values it is about to touch
//! (appearance fill, frame, radius, shadow and the stored pose) and writes them
//! back itself. That keeps "Next look" and "Undo look" atomic — cycling never
//! stacks two looks and removing one never leaves a tilted card on the previous
//! wallpaper.
//!
//! The wallpaper pixels are part of that snapshot. Applying a look shows the
//! bundled thumbnail and queues the full-size decode on a worker thread, and
//! restoring puts the saved surface back, exactly the way the Background
//! panel's wallpaper tiles work: no click here decodes a multi-megapixel
//! wallpaper on the GTK main thread.
//!
//! It is a local nudge only: the persisted open counter lives in
//! [`crate::studio_nudges`], and the chip disappears once Studio is unlocked
//! or the document already carries a Studio style.

use gtk4::{prelude::*, Align, Box as GtkBox, Button, DrawingArea, Image, Label, Orientation};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::capture::editor::image_transform::StaticImageTransform;
use crate::capture::editor::state::EditorState;
use crate::capture::editor::types::FrameStyle;
use crate::i18n::{t, tfmt};
use crate::recording::editor::model::{
    MotionAppearance, MotionBackgroundFillType, MotionSceneShadow, MotionSceneShadowPreset,
    DEFAULT_MOTION_END_PERSPECTIVE,
};

use super::background_panel;
use super::icon_names;
use super::motion_mode::{appearance, MotionSession};

/// Image-editor opens that offer the chip.
const NUDGE_OPENS: u64 = 3;
/// Smallest fill band a look keeps, so its wallpaper is actually visible.
const LOOK_PADDING: f64 = 48.0;
/// Card short edge used to derive shadow fields before the source is readable.
const FALLBACK_SHORT_EDGE: f64 = 400.0;

/// A still pose that leaves the camera at rest.
const RESTING_POSE: StaticImageTransform = StaticImageTransform {
    yaw: 0.0,
    pitch: 0.0,
    roll: 0.0,
    perspective: DEFAULT_MOTION_END_PERSPECTIVE,
    pos_x: 0.0,
    pos_y: 0.0,
};

/// One curated starting point. Every field maps to a control the inspector
/// panels already expose, so a look never invents a style the editor cannot
/// reproduce or edit afterwards.
struct Look {
    /// Translation key for the look's name.
    name_key: &'static str,
    /// Bundled wallpaper file name; resolved through the Background panel.
    wallpaper_file: &'static str,
    frame_style: FrameStyle,
    transform: StaticImageTransform,
    /// Fill band around the card. A larger band the user already set is kept.
    padding: f64,
    /// Corner radius to set, or `None` to keep the card's current radius (the
    /// glass presets still lift a sharp card onto their minimum).
    radius: Option<f64>,
    shadow: MotionSceneShadow,
}

/// The looks, in "Next look" order.
const LOOKS: [Look; 4] = [
    Look {
        name_key: "Frost",
        wallpaper_file: "wallpaper-001.jpg",
        frame_style: FrameStyle::GlassLight,
        transform: RESTING_POSE,
        padding: LOOK_PADDING,
        radius: None,
        shadow: MotionSceneShadow {
            preset: MotionSceneShadowPreset::None,
            opacity: 1.0,
            light: (0.65, 0.65),
        },
    },
    Look {
        name_key: "Tilt",
        wallpaper_file: "gradient-08.jpg",
        frame_style: FrameStyle::Default,
        transform: StaticImageTransform {
            yaw: -18.0,
            pitch: 10.0,
            ..RESTING_POSE
        },
        padding: 56.0,
        radius: Some(18.0),
        shadow: MotionSceneShadow {
            preset: MotionSceneShadowPreset::Hug,
            opacity: 0.9,
            light: (0.65, 0.65),
        },
    },
    Look {
        name_key: "Bleed",
        wallpaper_file: "wallpaper-021.jpg",
        frame_style: FrameStyle::Default,
        // Pushed down and right so the card runs off the canvas: the camera
        // looks up and left. The pad scales with the canvas (pos 1 moves the
        // card half a canvas), so these keep the card's top-left ~55-60% in
        // frame for both landscape and portrait stills.
        transform: StaticImageTransform {
            pos_x: -0.33,
            pos_y: -0.40,
            ..RESTING_POSE
        },
        padding: 56.0,
        radius: Some(18.0),
        shadow: MotionSceneShadow {
            preset: MotionSceneShadowPreset::None,
            opacity: 1.0,
            light: (0.65, 0.65),
        },
    },
    Look {
        name_key: "Float",
        wallpaper_file: "gradient-09.jpg",
        frame_style: FrameStyle::Default,
        transform: RESTING_POSE,
        padding: 64.0,
        radius: Some(26.0),
        shadow: MotionSceneShadow {
            preset: MotionSceneShadowPreset::Hug,
            opacity: 0.85,
            light: (0.65, 0.65),
        },
    },
];

/// The wallpaper pixels the canvas was showing when the snapshot was taken.
/// Cloning a `gtk4::cairo::ImageSurface` is a refcount, not a decode, so
/// saving and restoring one never touches the image files.
#[derive(Clone, Default)]
struct LookSurface {
    surface: Option<gtk4::cairo::ImageSurface>,
    /// Path whose pixels `surface` holds.
    path: Option<String>,
    /// True while `surface` only holds the cached thumbnail, which the restore
    /// still has to replace with the full-size decode.
    is_preview: bool,
}

/// The values a look overwrites, saved before it is applied so the chip can put
/// the document back exactly as it found it.
#[derive(Clone)]
struct LookSnapshot {
    fill_type: MotionBackgroundFillType,
    wallpaper_image_name: Option<String>,
    custom_background_image: Option<String>,
    padding: f64,
    radius: f64,
    frame_style: FrameStyle,
    border_thickness: f64,
    border_fill_color: [f64; 4],
    shadow: MotionSceneShadow,
    transform: StaticImageTransform,
    surface: LookSurface,
}

impl LookSnapshot {
    fn capture(
        appearance: &MotionAppearance,
        shadow: &MotionSceneShadow,
        transform: StaticImageTransform,
        surface: LookSurface,
    ) -> Self {
        Self {
            fill_type: appearance.background_fill_type.clone(),
            wallpaper_image_name: appearance.wallpaper_image_name.clone(),
            custom_background_image: appearance.custom_background_image.clone(),
            padding: appearance.background_padding,
            radius: appearance.border_radius,
            frame_style: appearance.frame_style,
            border_thickness: appearance.border_thickness,
            border_fill_color: appearance.border_fill_color,
            shadow: shadow.clone(),
            transform,
            surface,
        }
    }

    /// Write the saved values back. Returns the pose the still had before the
    /// look, for the caller to store.
    fn restore_into(
        &self,
        appearance: &mut MotionAppearance,
        shadow: &mut MotionSceneShadow,
        short: f64,
    ) -> StaticImageTransform {
        appearance.background_fill_type = self.fill_type.clone();
        appearance.wallpaper_image_name = self.wallpaper_image_name.clone();
        appearance.custom_background_image = self.custom_background_image.clone();
        appearance.background_padding = self.padding;
        appearance.border_radius = self.radius;
        appearance.frame_style = self.frame_style;
        appearance.border_thickness = self.border_thickness;
        appearance.border_fill_color = self.border_fill_color;
        *shadow = self.shadow.clone();
        apply_shadow_fields(appearance, shadow, short);
        self.transform
    }
}

/// Build the header chip, or `None` when this window should not show it.
///
/// `on_look_changed` runs after the chip applies, cycles or removes a look. The
/// window uses it to rebuild the docked Background inspector from the shared
/// runtime, hand the Background tool back the canvas, and re-seat the Transform
/// sliders — the same end state a panel interaction leaves behind.
pub(super) fn build_look_chip(
    session: &MotionSession,
    state: &Arc<Mutex<EditorState>>,
    preview: &DrawingArea,
    editor_open_count: u64,
    on_look_changed: Rc<dyn Fn()>,
) -> Option<GtkBox> {
    if editor_open_count > NUDGE_OPENS {
        return None;
    }
    if crate::studio_entitlement::studio_unlocked() {
        return None;
    }
    let already_styled = state
        .lock()
        .map(|state| state.needs_studio_preview_mark())
        .unwrap_or(true);
    if already_styled {
        return None;
    }

    let chip = GtkBox::new(Orientation::Horizontal, 2);
    chip.add_css_class("editor-look-chip");
    chip.set_halign(Align::Center);
    chip.set_valign(Align::Center);

    let next = Button::new();
    next.set_has_frame(false);
    next.set_focusable(false);
    next.add_css_class("editor-look-chip-action");
    next.set_visible(false);
    {
        let row = GtkBox::new(Orientation::Horizontal, 4);
        row.set_halign(Align::Center);
        row.set_valign(Align::Center);
        let icon = Image::from_icon_name(icon_names::ARROW_REDO_REGULAR);
        icon.set_pixel_size(12);
        row.append(&icon);
        row.append(&Label::new(Some(&t("Next look"))));
        next.set_child(Some(&row));
    }

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
    let dismiss_icon = Image::from_icon_name(icon_names::DISMISS_REGULAR);
    dismiss_icon.set_pixel_size(12);
    dismiss.set_child(Some(&dismiss_icon));

    chip.append(&next);
    chip.append(&action);
    chip.append(&dismiss);

    dismiss.connect_clicked({
        let chip = chip.downgrade();
        move |_| {
            if let Some(chip) = chip.upgrade() {
                // The look, if one is applied, stays: only the suggestion goes.
                chip.set_visible(false);
            }
        }
    });

    let look_index: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
    let snapshot: Rc<RefCell<Option<LookSnapshot>>> = Rc::new(RefCell::new(None));

    let show_state: Rc<dyn Fn(Option<usize>)> = {
        let action = action.clone();
        let next = next.clone();
        Rc::new(move |index| match index {
            Some(index) => {
                action.set_label(&t("Undo look"));
                action.set_tooltip_text(Some(&t("Undo look")));
                let upcoming = &LOOKS[next_look_index(index)];
                let name = t(upcoming.name_key);
                next.set_visible(true);
                next.set_tooltip_text(Some(&tfmt("Next look: {name}", &[("name", name.as_str())])));
            }
            None => {
                action.set_label(&t("Try a styled look"));
                action.set_tooltip_text(Some(&t("Try a styled look")));
                next.set_visible(false);
            }
        })
    };

    next.connect_clicked({
        let session = (*session).clone();
        let state = state.clone();
        let preview = (*preview).clone();
        let look_index = look_index.clone();
        let snapshot = snapshot.clone();
        let show_state = show_state.clone();
        let on_look_changed = on_look_changed.clone();
        move |_| {
            let Some(current) = look_index.get() else {
                return;
            };
            // Restore before applying the next one: looks replace each other
            // instead of stacking, and the replacement is still one step.
            remove_look(&session, &state, &preview, &snapshot);
            let upcoming = next_look_index(current);
            if start_look(&session, &state, &preview, &snapshot, upcoming) {
                look_index.set(Some(upcoming));
                show_state(Some(upcoming));
            } else {
                look_index.set(None);
                show_state(None);
            }
            on_look_changed();
        }
    });

    action.connect_clicked({
        let session = (*session).clone();
        let state = state.clone();
        let preview = (*preview).clone();
        let look_index = look_index.clone();
        let snapshot = snapshot.clone();
        let show_state = show_state.clone();
        let on_look_changed = on_look_changed.clone();
        move |_| {
            if look_index.get().is_some() {
                remove_look(&session, &state, &preview, &snapshot);
                look_index.set(None);
                show_state(None);
                on_look_changed();
                return;
            }
            if start_look(&session, &state, &preview, &snapshot, 0) {
                look_index.set(Some(0));
                show_state(Some(0));
                on_look_changed();
            }
        }
    });

    Some(chip)
}

/// The look "Next look" moves to, wrapping back to the first.
fn next_look_index(current: usize) -> usize {
    (current + 1) % LOOKS.len()
}

/// Save the pre-look values, then apply `index` as the chip's single look.
fn start_look(
    session: &MotionSession,
    state: &Arc<Mutex<EditorState>>,
    preview: &DrawingArea,
    snapshot: &Rc<RefCell<Option<LookSnapshot>>>,
    index: usize,
) -> bool {
    let saved = capture_look_snapshot(session, state);
    if !apply_look(session, state, preview, index) {
        return false;
    }
    *snapshot.borrow_mut() = Some(saved);
    true
}

/// Put the document back where it was before the chip applied a look.
fn remove_look(
    session: &MotionSession,
    state: &Arc<Mutex<EditorState>>,
    preview: &DrawingArea,
    snapshot: &Rc<RefCell<Option<LookSnapshot>>>,
) {
    if let Some(saved) = snapshot.borrow_mut().take() {
        restore_look(session, state, preview, &saved);
    }
}

fn capture_look_snapshot(session: &MotionSession, state: &Arc<Mutex<EditorState>>) -> LookSnapshot {
    let transform = state
        .lock()
        .map(|state| state.static_image_transform)
        .unwrap_or_default();
    let runtime = session.runtime.borrow();
    LookSnapshot::capture(
        &runtime.motion.appearance,
        &runtime.motion.scene_shadow,
        transform,
        LookSurface {
            surface: runtime.background_surface.clone(),
            path: runtime.background_surface_path.clone(),
            is_preview: runtime.background_surface_is_preview,
        },
    )
}

/// Apply `index` as one Motion-history step plus, when it moves the card, one
/// still step for the pose.
fn apply_look(
    session: &MotionSession,
    state: &Arc<Mutex<EditorState>>,
    preview: &DrawingArea,
    index: usize,
) -> bool {
    let Some(look) = LOOKS.get(index) else {
        return false;
    };
    let Some(wallpaper) = look_wallpaper(look.wallpaper_file) else {
        return false;
    };
    // The tile recipe: the bundled, cached thumbnail paints immediately and the
    // full-size wallpaper decodes on a worker thread. The catalog wallpapers are
    // up to 6000x4000, so decoding one here would freeze the window.
    let thumbnail = appearance::cached_wallpaper_preview_surface(
        &background_panel::motion_wallpaper_preview_asset_path(
            // The resolved file, so a fallback wallpaper gets its own thumbnail.
            std::path::Path::new(&wallpaper)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(look.wallpaper_file),
        )
        .to_string_lossy(),
    );
    let short = card_short_edge(state);

    {
        let mut runtime = session.runtime.borrow_mut();
        // `begin_motion_command` clears the coalesce window first, so every
        // look is its own Undo step and a following panel edit is another.
        runtime.begin_motion_command();
        let motion = &mut runtime.motion;
        apply_look_fields(
            &mut motion.appearance,
            &mut motion.scene_shadow,
            look,
            &wallpaper,
            short,
        );
        runtime.set_background_surface(Some(wallpaper.clone()), thumbnail, true);
        runtime.preview_frame = None;
        runtime.backdrop_cache = None;
    }

    // The pose is stored on the still itself, so it goes through the same
    // setter the Transform panel uses; an identity pose records nothing.
    if let Ok(mut editor) = state.lock() {
        editor.set_static_image_transform(look.transform);
    }

    preview.queue_draw();
    appearance::load_motion_wallpaper_asynchronously(
        PathBuf::from(wallpaper),
        session.clone(),
        preview.clone(),
    );
    true
}

/// Write one look's appearance fields. The transform is stored separately; the
/// caller applies it through [`EditorState::set_static_image_transform`].
fn apply_look_fields(
    appearance: &mut MotionAppearance,
    shadow: &mut MotionSceneShadow,
    look: &Look,
    wallpaper: &str,
    short: f64,
) {
    let spec = look.frame_style.spec();
    appearance.background_fill_type = MotionBackgroundFillType::Wallpaper;
    appearance.wallpaper_image_name = Some(wallpaper.to_owned());
    // Replace, never stack: a stale custom image must not survive behind the
    // new fill.
    appearance.custom_background_image = None;
    appearance.background_padding = appearance.background_padding.max(look.padding);
    appearance.frame_style = look.frame_style;
    appearance.border_thickness = spec.border_thickness;
    appearance.border_fill_color = [
        spec.border_color.r,
        spec.border_color.g,
        spec.border_color.b,
        spec.border_color.a,
    ];
    if let Some(radius) = look.radius {
        appearance.border_radius = radius;
    }
    // Glass needs corners to catch the light: lift a sharp card onto the glass
    // minimum the Frame panel uses, but leave a radius the user already set.
    if spec.liquid && appearance.border_radius < 12.0 {
        appearance.border_radius = 20.0;
    }
    *shadow = look.shadow.clone();
    apply_shadow_fields(appearance, shadow, short);
}

/// Derive the compositor's card-shadow fields from the shadow layer, the same
/// way the Appearance panel's shadow controls do.
fn apply_shadow_fields(appearance: &mut MotionAppearance, shadow: &MotionSceneShadow, short: f64) {
    let (opacity, blur, offset) = shadow.clamped().resolved(short);
    appearance.shadow_opacity = opacity;
    appearance.shadow_blur = blur;
    appearance.shadow_position = offset;
}

/// Restore a saved snapshot and drop the history steps the look pushed, so the
/// editor's own Undo keeps pointing at the user's previous edit.
fn restore_look(
    session: &MotionSession,
    state: &Arc<Mutex<EditorState>>,
    preview: &DrawingArea,
    snapshot: &LookSnapshot,
) {
    let short = card_short_edge(state);
    let pending_wallpaper = {
        let mut runtime = session.runtime.borrow_mut();
        let motion = &mut runtime.motion;
        snapshot.restore_into(&mut motion.appearance, &mut motion.scene_shadow, short);
        runtime.preview_content_gen = runtime.preview_content_gen.wrapping_add(1);
        // Put the pixels the canvas already had back with the fill: a surface
        // clone is a refcount, so removing a look never re-decodes a wallpaper.
        runtime.set_background_surface(
            snapshot.surface.path.clone(),
            snapshot.surface.surface.clone(),
            snapshot.surface.is_preview,
        );
        runtime.preview_frame = None;
        runtime.discard_current_motion_checkpoint();
        // A snapshot taken while only the thumbnail was on screen still owes
        // the full-size decode the Background panel's tiles would have queued.
        let is_wallpaper =
            runtime.motion.appearance.background_fill_type == MotionBackgroundFillType::Wallpaper;
        let needs_full = snapshot.surface.is_preview || snapshot.surface.surface.is_none();
        if is_wallpaper && needs_full {
            runtime.motion.appearance.wallpaper_image_name.clone()
        } else {
            None
        }
    };
    if let Some(wallpaper) = pending_wallpaper {
        appearance::load_motion_wallpaper_asynchronously(
            PathBuf::from(wallpaper),
            session.clone(),
            preview.clone(),
        );
    }
    if let Ok(mut editor) = state.lock() {
        editor.static_image_transform = snapshot.transform;
        editor.discard_reverted_static_transform_step();
    }
    preview.queue_draw();
}

/// The card's short edge in canvas units, which every shadow style is a
/// fraction of. Mirrors the Appearance panel's own measurement.
fn card_short_edge(state: &Arc<Mutex<EditorState>>) -> f64 {
    state
        .lock()
        .ok()
        .map(|state| f64::from(state.base_image.width().min(state.base_image.height())))
        .filter(|short| *short > 1.0)
        .unwrap_or(FALLBACK_SHORT_EDGE)
}

/// Resolve a look's wallpaper, falling back to the first bundled wallpaper when
/// an install ships without the chosen one.
fn look_wallpaper(file_name: &str) -> Option<String> {
    let path = background_panel::background_gradient_asset_path(file_name);
    if path.is_file() {
        return Some(path.to_string_lossy().into_owned());
    }
    background_panel::default_motion_wallpaper()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const SHORT: f64 = 800.0;

    /// An appearance that differs from every look in every field the chip
    /// touches, so a restore that misses one is visible.
    fn styled_appearance() -> (MotionAppearance, MotionSceneShadow) {
        let mut appearance = MotionAppearance {
            background_fill_type: MotionBackgroundFillType::Color,
            background_color: [0.1, 0.2, 0.3, 1.0],
            background_padding: 80.0,
            border_radius: 12.0,
            frame_style: FrameStyle::Border,
            border_thickness: 3.0,
            border_fill_color: [0.0, 0.0, 0.0, 1.0],
            ..MotionAppearance::default()
        };
        let shadow = MotionSceneShadow {
            preset: MotionSceneShadowPreset::Hug,
            opacity: 0.5,
            light: (0.2, -0.4),
        };
        apply_shadow_fields(&mut appearance, &shadow, SHORT);
        (appearance, shadow)
    }

    #[test]
    fn the_chip_offers_four_looks_over_four_wallpapers() {
        assert_eq!(LOOKS.len(), 4);
        let wallpapers: HashSet<&str> = LOOKS.iter().map(|look| look.wallpaper_file).collect();
        assert_eq!(
            wallpapers.len(),
            LOOKS.len(),
            "every look should use its own wallpaper"
        );
        for look in &LOOKS {
            assert!(
                background_panel::MOTION_WALLPAPER_FILES.contains(&look.wallpaper_file),
                "{} must name a bundled wallpaper",
                look.wallpaper_file
            );
        }
    }

    #[test]
    fn the_tilt_look_carries_a_3d_pose_and_stays_centred() {
        let tilt = LOOKS
            .iter()
            .find(|look| look.name_key == "Tilt")
            .expect("a tilt look");
        assert!(tilt.transform.yaw.abs() > 1.0);
        assert!(tilt.transform.pitch.abs() > 1.0);
        assert_eq!(tilt.transform.roll, 0.0);
        assert_eq!(tilt.transform.pos_x, 0.0);
        assert_eq!(tilt.transform.pos_y, 0.0);
        assert_ne!(
            tilt.shadow.preset,
            MotionSceneShadowPreset::None,
            "the product-shot pose wants a cast shadow"
        );
    }

    #[test]
    fn the_bleed_look_runs_off_the_lower_right_edge() {
        let bleed = LOOKS
            .iter()
            .find(|look| look.name_key == "Bleed")
            .expect("a bleed look");
        // The Position pad is camera framing: negative X/Y push the card down
        // and right, out of the frame, leaving its top-left corner visible.
        assert!(bleed.transform.pos_x < 0.0);
        assert!(bleed.transform.pos_y < 0.0);
        assert_eq!(bleed.transform.yaw, 0.0);
        assert_eq!(bleed.transform.pitch, 0.0);
        assert_eq!(bleed.frame_style, FrameStyle::Default);
        assert_eq!(bleed.radius, Some(18.0));
    }

    #[test]
    fn next_look_wraps_from_the_last_look_to_the_first() {
        assert_eq!(next_look_index(0), 1);
        for index in 1..LOOKS.len() - 1 {
            assert_eq!(next_look_index(index), index + 1);
        }
        assert_eq!(next_look_index(LOOKS.len() - 1), 0);
    }

    #[test]
    fn a_snapshot_restores_every_field_a_look_touches() {
        let (original_appearance, original_shadow) = styled_appearance();
        let original_transform = StaticImageTransform {
            yaw: 6.0,
            pos_x: 0.2,
            ..RESTING_POSE
        };
        let snapshot = LookSnapshot::capture(
            &original_appearance,
            &original_shadow,
            original_transform,
            LookSurface::default(),
        );

        let mut appearance = original_appearance.clone();
        let mut shadow = original_shadow.clone();
        // Two looks in a row: the second replaces the first, and the snapshot
        // must still undo both.
        apply_look_fields(
            &mut appearance,
            &mut shadow,
            &LOOKS[1],
            "/tmp/gradient-08.jpg",
            SHORT,
        );
        apply_look_fields(
            &mut appearance,
            &mut shadow,
            &LOOKS[2],
            "/tmp/gradient-01.jpg",
            SHORT,
        );
        assert_ne!(appearance, original_appearance);
        assert_ne!(shadow, original_shadow);

        let restored = snapshot.restore_into(&mut appearance, &mut shadow, SHORT);
        assert_eq!(restored, original_transform);
        assert_eq!(appearance, original_appearance);
        assert_eq!(shadow, original_shadow);
    }

    #[test]
    fn every_look_sets_a_wallpaper_frame_and_shadow_explicitly() {
        for look in &LOOKS {
            let mut appearance = MotionAppearance::default();
            let mut shadow = MotionSceneShadow::default();
            apply_look_fields(&mut appearance, &mut shadow, look, "/tmp/look.jpg", SHORT);
            assert_eq!(
                appearance.background_fill_type,
                MotionBackgroundFillType::Wallpaper
            );
            assert_eq!(
                appearance.wallpaper_image_name.as_deref(),
                Some("/tmp/look.jpg")
            );
            assert_eq!(appearance.custom_background_image, None);
            assert!(appearance.background_padding >= LOOK_PADDING);
            assert_eq!(appearance.frame_style, look.frame_style);
            assert_eq!(shadow, look.shadow);
        }
    }
}
