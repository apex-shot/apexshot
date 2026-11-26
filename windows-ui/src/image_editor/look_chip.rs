//! The first-run "Try a styled look" chip in the image editor header.
//!
//! The chip walks a short curated list of starting points for an unstyled
//! still: a wallpaper, the Frame panel's matching preset, a card shadow, and
//! where the look calls for it a 3D pose or an off-canvas bleed. The fields are
//! written into the Windows document as one undo step, the same way the
//! Background and Transform panels record their own edits.
//!
//! The chip never relies on a blind Undo to remove its look: it saves the
//! values it is about to touch (the whole background appearance, the scene
//! shadow and the stored pose) and writes them back itself. That keeps "Next
//! look" and "Undo look" atomic, so cycling never stacks two looks and removing
//! one never leaves a tilted card on the previous wallpaper.
//!
//! The wallpaper pixels are part of that snapshot. Applying a look hands the
//! bundled thumbnail and the full-size file to the Background panel's own
//! loader, which decodes off the main thread, and restoring puts the saved
//! pixels back without a decode.
//!
//! It is a local nudge only: the persisted open counter lives beside the app
//! config, and the chip disappears once Studio is unlocked or the document
//! already carries a Studio style.

use gtk4::prelude::*;
use gtk4::{glib, Align, Box as GtkBox, Button, Label, Orientation};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Duration;

use crate::capture::editor::image_transform::StaticImageTransform;
use crate::capture::editor::state::background::Appearance;
use crate::capture::editor::types::FrameStyle;
use crate::i18n::{t, tfmt};
use crate::recording::editor::model::{
    MotionSceneShadow, MotionSceneShadowPreset, DEFAULT_MOTION_END_PERSPECTIVE,
};

use super::annotate::EditorHost;
use super::ui_support::px;

/// Image-editor opens that offer the chip.
const NUDGE_OPENS: u64 = 3;
/// Smallest fill band a look keeps, so its wallpaper is actually visible.
const LOOK_PADDING: f64 = 48.0;
/// How often the chip checks whether it should still be on screen.
const VISIBILITY_TICK: Duration = Duration::from_millis(250);

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
    /// Bundled wallpaper file name; resolved through the Background assets.
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

/// The values a look overwrites, saved before it is applied so the chip can
/// put the document back exactly as it found it.
#[derive(Clone)]
struct LookSnapshot {
    background: Appearance,
    shadow: MotionSceneShadow,
    transform: StaticImageTransform,
}

/// The persisted nudge counters. They carry no identity and never leave the
/// machine; a missing or unreadable file behaves like a fresh install.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct NudgeState {
    editor_opens: u64,
}

fn nudges_path() -> Option<PathBuf> {
    let mut path = crate::config::config_path()?;
    path.set_file_name("studio_nudges.json");
    Some(path)
}

fn load_state(path: &Path) -> NudgeState {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_state(path: &Path, state: &NudgeState) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    let Ok(bytes) = serde_json::to_vec(state) else {
        return false;
    };
    std::fs::write(path, bytes).is_ok()
}

/// Persist one new editor-open count and return it. Counts one image editor
/// window opened with an image; a storage failure returns 0 so the nudge still
/// appears on a machine that cannot write the file.
pub(super) fn bump_editor_open() -> u64 {
    let Some(path) = nudges_path() else {
        return 0;
    };
    bump_at_path(&path)
}

fn bump_at_path(path: &Path) -> u64 {
    // The editor path can run in several threads of one process; serialize the
    // read-modify-write so two near-simultaneous bumps cannot drop one.
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());

    let mut state = load_state(path);
    state.editor_opens = state.editor_opens.saturating_add(1);
    if save_state(path, &state) {
        state.editor_opens
    } else {
        0
    }
}

pub(super) struct LookChip {
    pub(super) root: GtkBox,
}

/// Build the header chip, or `None` when this window should not show it.
pub(super) fn build(
    scale: f64,
    host: &Rc<EditorHost>,
    editor_open_count: Option<u64>,
) -> Option<LookChip> {
    let editor_open_count = editor_open_count?;
    if editor_open_count > NUDGE_OPENS {
        return None;
    }
    if crate::studio_entitlement::studio_unlocked() {
        return None;
    }

    let chip = GtkBox::new(Orientation::Horizontal, px(2.0, scale));
    chip.add_css_class("image-editor-look-chip");
    chip.set_halign(Align::Center);
    chip.set_valign(Align::Start);
    chip.set_margin_top(px(8.0, scale));
    chip.set_margin_start(px(8.0, scale));
    // The document loads asynchronously; stay hidden until one is present, so
    // the chip never flashes over an empty canvas or a styled saved session.
    chip.set_visible(false);

    let next = Button::with_label(&t("Next look"));
    next.set_has_frame(false);
    next.set_focusable(false);
    next.add_css_class("image-editor-look-chip-action");
    next.set_visible(false);

    let action = Button::with_label(&t("Try a styled look"));
    action.set_has_frame(false);
    action.set_focusable(false);
    action.add_css_class("image-editor-look-chip-action");
    action.set_tooltip_text(Some(&t("Try a styled look")));

    let dismiss = Button::new();
    dismiss.set_has_frame(false);
    dismiss.set_focusable(false);
    dismiss.add_css_class("image-editor-look-chip-dismiss");
    dismiss.set_tooltip_text(Some(&t("Hide suggestion")));
    let dismiss_label = Label::new(Some("\u{00d7}"));
    dismiss_label.add_css_class("image-editor-look-chip-dismiss-label");
    dismiss.set_child(Some(&dismiss_label));

    chip.append(&next);
    chip.append(&action);
    chip.append(&dismiss);

    let hidden = Rc::new(Cell::new(false));
    let checked = Rc::new(Cell::new(false));
    dismiss.connect_clicked({
        let chip = chip.downgrade();
        let hidden = hidden.clone();
        move |_| {
            // The look, if one is applied, stays: only the suggestion goes.
            hidden.set(true);
            if let Some(chip) = chip.upgrade() {
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
        }) as Rc<dyn Fn(Option<usize>)>
    };

    let failure = failure_callback(host, &snapshot, &look_index, &show_state);

    next.connect_clicked({
        let host = host.clone();
        let snapshot = snapshot.clone();
        let look_index = look_index.clone();
        let show_state = show_state.clone();
        let failure = failure.clone();
        move |_| {
            let Some(current) = look_index.get() else {
                return;
            };
            // Restore before applying the next one: looks replace each other
            // instead of stacking, and the replacement is still one step.
            restore_look(&host, &snapshot);
            let upcoming = next_look_index(current);
            if start_look(&host, &snapshot, upcoming, failure.clone()) {
                look_index.set(Some(upcoming));
                show_state(Some(upcoming));
            } else {
                look_index.set(None);
                show_state(None);
            }
        }
    });

    action.connect_clicked({
        let host = host.clone();
        let snapshot = snapshot.clone();
        let look_index = look_index.clone();
        let show_state = show_state.clone();
        let failure = failure.clone();
        move |_| {
            if look_index.get().is_some() {
                restore_look(&host, &snapshot);
                look_index.set(None);
                show_state(None);
                return;
            }
            if start_look(&host, &snapshot, 0, failure.clone()) {
                look_index.set(Some(0));
                show_state(Some(0));
            }
        }
    });

    // Reveal the chip once a document is up, keep it off a document that
    // already carries Studio content, and drop it when Studio is activated.
    {
        let host = host.clone();
        let chip = chip.downgrade();
        let hidden = hidden.clone();
        let checked = checked.clone();
        glib::timeout_add_local(VISIBILITY_TICK, move || {
            let Some(chip) = chip.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if hidden.get() || !host.has_document() {
                chip.set_visible(false);
                return glib::ControlFlow::Continue;
            }
            if !checked.replace(true) {
                let already_styled = host
                    .read(|st| st.needs_studio_preview_mark())
                    .unwrap_or(true);
                if already_styled {
                    hidden.set(true);
                    chip.set_visible(false);
                    return glib::ControlFlow::Continue;
                }
            }
            chip.set_visible(host.studio_preview_locked());
            glib::ControlFlow::Continue
        });
    }

    Some(LookChip { root: chip })
}

/// The look "Next look" moves to, wrapping back to the first.
fn next_look_index(current: usize) -> usize {
    (current + 1) % LOOKS.len()
}

fn failure_callback(
    host: &Rc<EditorHost>,
    snapshot: &Rc<RefCell<Option<LookSnapshot>>>,
    look_index: &Rc<Cell<Option<usize>>>,
    show_state: &Rc<dyn Fn(Option<usize>)>,
) -> Rc<dyn Fn()> {
    let host = host.clone();
    let snapshot = snapshot.clone();
    let look_index = look_index.clone();
    let show_state = show_state.clone();
    Rc::new(move || {
        restore_look(&host, &snapshot);
        look_index.set(None);
        show_state(None);
    })
}

fn capture_snapshot(host: &Rc<EditorHost>) -> LookSnapshot {
    host.read(|st| LookSnapshot {
        background: st.background.clone(),
        shadow: st.scene_shadow.clone(),
        transform: st.static_image_transform,
    })
    .unwrap_or_else(|| LookSnapshot {
        background: Appearance::default(),
        shadow: MotionSceneShadow::default(),
        transform: StaticImageTransform::default(),
    })
}

/// Save the pre-look values, then apply `index` as the chip's single look.
fn start_look(
    host: &Rc<EditorHost>,
    snapshot: &Rc<RefCell<Option<LookSnapshot>>>,
    index: usize,
    on_failure: Rc<dyn Fn()>,
) -> bool {
    let Some(look) = LOOKS.get(index) else {
        return false;
    };
    let Some(wallpaper) = look_wallpaper(look.wallpaper_file) else {
        return false;
    };
    let thumbnail = look_thumbnail(&wallpaper);
    // The canvas renders the frozen pose only under the Background tool, the
    // same place the Transform and Frame controls live. Switch before opening
    // the history interaction so the tool change cannot close it early.
    host.set_tool(crate::capture::editor::types::Tool::Background);
    let saved = capture_snapshot(host);
    let applied = host
        .mutate(|st| {
            // One interaction spans the appearance, the pose and the
            // wallpaper the loader is about to put in, so the whole look is a
            // single Undo step.
            st.begin_history_interaction();
            apply_look_fields(&mut st.background, &mut st.scene_shadow, look);
            st.set_static_image_transform(look.transform);
        })
        .is_some();
    if !applied {
        return false;
    }
    *snapshot.borrow_mut() = Some(saved);
    // The tile recipe: the bundled, cached thumbnail paints immediately and
    // the full-size wallpaper decodes on a worker thread, never here on the
    // window thread.
    host.load_background(wallpaper, thumbnail, |_| {}, move || on_failure());
    true
}

/// Put the document back where it was before the chip applied a look.
fn restore_look(host: &Rc<EditorHost>, snapshot: &Rc<RefCell<Option<LookSnapshot>>>) {
    let Some(saved) = snapshot.borrow_mut().take() else {
        return;
    };
    // Drop the wallpaper this look may still be decoding, then remove the
    // step the look recorded and write the saved values back without adding
    // one of its own.
    host.cancel_background_load();
    host.mutate(|st| {
        st.discard_last_history_step_if_current();
        st.restore_look_snapshot(
            saved.background.clone(),
            saved.shadow.clone(),
            saved.transform,
        );
    });
}

/// Write one look's appearance fields. The transform is stored separately; the
/// caller applies it through [`EditorState::set_static_image_transform`].
fn apply_look_fields(background: &mut Appearance, shadow: &mut MotionSceneShadow, look: &Look) {
    background.set_padding(background.padding.max(look.padding));
    background.frame_style = look.frame_style;
    if let Some(radius) = look.radius {
        background.radius = radius;
    }
    // Glass needs corners to catch the light: lift a sharp card onto the glass
    // minimum the Frame panel uses, but leave a radius the user already set.
    if look.frame_style.spec().liquid && background.radius < 12.0 {
        background.radius = 20.0;
    }
    background.changed();
    *shadow = look.shadow.clamped();
}

/// Resolve a look's wallpaper, falling back to the first bundled wallpaper when
/// an install ships without the chosen one.
fn look_wallpaper(file_name: &str) -> Option<PathBuf> {
    let directory = crate::video_editor::background::asset_directory()?;
    let chosen = directory.join(file_name);
    if chosen.is_file() {
        return Some(chosen);
    }
    let fallback = directory.join("wallpaper-001.jpg");
    fallback.is_file().then_some(fallback)
}

/// The bundled thumbnail for a wallpaper, if the install ships one.
fn look_thumbnail(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let thumbnail = if let Some(suffix) = name.strip_prefix("wallpaper-") {
        format!("wallpaper-thumb-{suffix}")
    } else if let Some(suffix) = name.strip_prefix("gradient-") {
        format!("gradient-thumb-{suffix}")
    } else {
        return None;
    };
    let thumbnail = path.with_file_name(thumbnail);
    thumbnail.is_file().then_some(thumbnail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::editor::state::background::Fill;
    use crate::capture::editor::state::EditorState;
    use image::{Rgba, RgbaImage};
    use std::sync::Arc;

    fn scratch_path(case: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "apexshot-windows-look-nudges-{}-{case}",
                std::process::id()
            ))
            .join("studio_nudges.json")
    }

    #[test]
    fn the_chip_offers_four_looks_over_four_wallpapers() {
        assert_eq!(LOOKS.len(), 4);
        let mut wallpapers: Vec<&str> = LOOKS.iter().map(|look| look.wallpaper_file).collect();
        wallpapers.sort_unstable();
        wallpapers.dedup();
        assert_eq!(wallpapers.len(), LOOKS.len(), "one wallpaper per look");
        assert_eq!(
            LOOKS
                .iter()
                .map(|look| look.wallpaper_file)
                .collect::<Vec<_>>(),
            vec![
                "wallpaper-001.jpg",
                "gradient-08.jpg",
                "wallpaper-021.jpg",
                "gradient-09.jpg",
            ]
        );
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
        assert_eq!(tilt.shadow.preset, MotionSceneShadowPreset::Hug);
        assert!((tilt.shadow.opacity - 0.9).abs() < 1e-9);
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
        assert_eq!(bleed.shadow.preset, MotionSceneShadowPreset::None);
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
        let mut styled = Appearance::default();
        styled.fill = Fill::Image(Arc::new(RgbaImage::from_pixel(1, 1, Rgba([7, 9, 11, 255]))));
        styled.padding = 20.0;
        styled.radius = 12.0;
        styled.frame_style = FrameStyle::Border;
        let original = LookSnapshot {
            background: styled,
            shadow: MotionSceneShadow {
                preset: MotionSceneShadowPreset::Hug,
                opacity: 0.5,
                light: (0.2, -0.4),
            },
            transform: StaticImageTransform {
                yaw: 6.0,
                pos_x: 0.2,
                ..RESTING_POSE
            },
        };
        let mut background = original.background.clone();
        let mut shadow = original.shadow.clone();
        // Two looks in a row: the second replaces the first, and the snapshot
        // must still undo both.
        apply_look_fields(&mut background, &mut shadow, &LOOKS[1]);
        apply_look_fields(&mut background, &mut shadow, &LOOKS[2]);
        assert_ne!(background.padding, original.background.padding);
        assert_ne!(background.frame_style, original.background.frame_style);
        assert_ne!(shadow, original.shadow);

        // The snapshot is written back exactly, pose included.
        let mut state = EditorState::new(RgbaImage::new(40, 30));
        state.background = background;
        state.scene_shadow = shadow;
        state.static_image_transform = StaticImageTransform {
            yaw: -8.0,
            ..RESTING_POSE
        };
        state.restore_look_snapshot(
            original.background.clone(),
            original.shadow.clone(),
            original.transform,
        );
        assert_eq!(state.background.padding, original.background.padding);
        assert_eq!(state.background.radius, original.background.radius);
        assert_eq!(
            state.background.frame_style,
            original.background.frame_style
        );
        assert!(matches!(&state.background.fill, Fill::Image(_)));
        assert_eq!(state.scene_shadow, original.shadow);
        assert_eq!(state.static_image_transform, original.transform);
    }

    #[test]
    fn every_look_sets_a_frame_shadow_and_pose_explicitly() {
        for look in &LOOKS {
            let mut background = Appearance::default();
            let mut shadow = MotionSceneShadow::default();
            apply_look_fields(&mut background, &mut shadow, look);
            assert!(background.padding >= LOOK_PADDING);
            assert_eq!(background.frame_style, look.frame_style);
            assert_eq!(shadow, look.shadow);
            if let Some(radius) = look.radius {
                assert_eq!(background.radius, radius);
            }
        }
        // Glass keeps a radius the user already set but lifts a sharp card.
        let frost = LOOKS
            .iter()
            .find(|look| look.name_key == "Frost")
            .expect("a frost look");
        let mut background = Appearance::default();
        let mut shadow = MotionSceneShadow::default();
        apply_look_fields(&mut background, &mut shadow, frost);
        assert_eq!(background.radius, 20.0);
        background.radius = 16.0;
        apply_look_fields(&mut background, &mut shadow, frost);
        assert_eq!(background.radius, 16.0);
    }

    #[test]
    fn the_open_counter_persists_and_survives_a_corrupt_file() {
        let path = scratch_path("counter");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        assert_eq!(load_state(&path).editor_opens, 0);
        assert_eq!(bump_at_path(&path), 1);
        assert_eq!(bump_at_path(&path), 2);
        assert_eq!(load_state(&path).editor_opens, 2);

        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(bump_at_path(&path), 1);

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }
}
