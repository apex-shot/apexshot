use super::keystrokes::{self, Caption};
use crate::recording::editor::click_effect::{ripple_bounce_at_ms, RIPPLE_VISIBLE_DURATION_MS};
use crate::recording::editor::model::{ClickEffect, CursorSettings};
use crate::recording::editor::ripple_preview::{PreviewRipple, PreviewRippleClick};
use crate::recording::editor::sidecar::{CursorMotion, PointerSidecar};
use gtk4::cairo::Context;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

#[derive(Clone)]
pub(super) struct CursorModel {
    state: Rc<RefCell<State>>,
}

struct State {
    sidecar: Option<PointerSidecar>,
    video_path: Option<std::path::PathBuf>,
    settings: CursorSettings,
    end_seconds: Option<f64>,
    caption_settings: keystrokes::Settings,
    captions: Vec<Caption>,
    captions_active: bool,
    /// How much of the cursor the hide track leaves visible at the playhead:
    /// 0 while a hide clip covers it, 1 otherwise.
    hide_alpha: f64,
}

fn rebuild_captions(state: &mut State) {
    let show_single_letters = state.caption_settings.show_single_letters;
    state.captions = match state.sidecar.as_ref() {
        Some(sidecar) => keystrokes::captions(&sidecar.keys, show_single_letters),
        None => Vec::new(),
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct VideoRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl CursorModel {
    pub fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(State {
                sidecar: None,
                video_path: None,
                settings: CursorSettings::default(),
                end_seconds: None,
                caption_settings: keystrokes::Settings::default(),
                captions: Vec::new(),
                captions_active: true,
                hide_alpha: 1.0,
            })),
        }
    }

    pub fn load(&self, path: Option<&Path>) {
        let mut state = self.state.borrow_mut();
        state.sidecar = path.and_then(PointerSidecar::load_next_to_video);
        state.video_path = path.map(Path::to_path_buf);
        rebuild_captions(&mut state);
        let threshold = state.settings.remove_shake_threshold;
        let settings = state.settings;
        let end = state.end_seconds;
        if let Some(sidecar) = state.sidecar.as_mut() {
            sidecar.set_shake_threshold(threshold);
            sidecar.set_cursor_tail(
                settings.stop_movement_in_last_part_ms,
                settings.loop_position_before_end_ms,
                end,
            );
        }
    }

    pub fn settings(&self) -> CursorSettings {
        self.state.borrow().settings
    }

    pub fn sidecar(&self) -> Option<PointerSidecar> {
        self.state.borrow().sidecar.clone()
    }

    /// Give a test cursor a recording with one key press and nothing else.
    #[cfg(test)]
    pub(super) fn set_keys_for_test(&self) {
        let mut sidecar = PointerSidecar::new(
            0,
            crate::recording::editor::sidecar::CaptureRegion {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
        );
        sidecar
            .keys
            .push(crate::recording::editor::sidecar::KeySample {
                t: 1.0,
                kind: "key_down".to_owned(),
                key: 46,
                scan_code: 0,
                modifiers: Vec::new(),
                cursor_id: None,
            });
        self.state.borrow_mut().sidecar = Some(sidecar);
    }

    /// Whether the open recording carries key presses, without cloning it.
    pub fn has_keys(&self) -> bool {
        self.state
            .borrow()
            .sidecar
            .as_ref()
            .is_some_and(|sidecar| !sidecar.keys.is_empty())
    }

    pub fn set_settings(&self, settings: CursorSettings) {
        let mut state = self.state.borrow_mut();
        state.settings = settings.clamped();
        let threshold = state.settings.remove_shake_threshold;
        let settings = state.settings;
        let end = state.end_seconds;
        if let Some(sidecar) = state.sidecar.as_mut() {
            sidecar.set_shake_threshold(threshold);
            sidecar.set_cursor_tail(
                settings.stop_movement_in_last_part_ms,
                settings.loop_position_before_end_ms,
                end,
            );
        }
    }

    pub fn set_caption_settings(&self, settings: keystrokes::Settings) {
        let mut state = self.state.borrow_mut();
        if state.caption_settings == settings {
            return;
        }
        state.caption_settings = settings;
        rebuild_captions(&mut state);
    }

    /// Where the edit ends, in source seconds. The cursor's tail behaviours
    /// measure back from it.
    pub fn set_end(&self, end_seconds: f64) {
        let mut state = self.state.borrow_mut();
        state.end_seconds = (end_seconds.is_finite() && end_seconds > 0.0).then_some(end_seconds);
        let settings = state.settings;
        let end = state.end_seconds;
        if let Some(sidecar) = state.sidecar.as_mut() {
            sidecar.set_cursor_tail(
                settings.stop_movement_in_last_part_ms,
                settings.loop_position_before_end_ms,
                end,
            );
        }
    }

    /// Where the timeline cuts the footage. The cursor plan leads the cursor
    /// but stops at the next cut, so the preview matches the export.
    pub fn set_cuts(&self, cuts: &[f64]) {
        if let Some(sidecar) = self.state.borrow_mut().sidecar.as_mut() {
            sidecar.set_cut_times(cuts);
        }
    }

    /// The click ripple the current frame should warp, if any.
    ///
    /// The decoded texture holds the whole encoded frame, so the ripple's view
    /// spans the source: the camera is a transform on the picture and magnifies
    /// the warp together with the footage, keeping the wave on its source pixel.
    pub fn ripple_at(
        &self,
        source_seconds: f64,
        source_size: Option<(u32, u32)>,
    ) -> Option<PreviewRipple> {
        let state = self.state.borrow();
        let (width, height) = source_size?;
        preview_ripple(
            state.sidecar.as_ref()?,
            state.settings,
            source_seconds,
            (width, height),
            (0.0, 0.0, f64::from(width), f64::from(height)),
        )
    }

    /// Whether the frame at the playhead is footage the edit keeps.
    ///
    /// Captions are placed by the source clock, and that clock holds its last
    /// value across a cut, a removed span or the freeze tail, so the caller
    /// says when there is no picture for them to decorate. Loading a recording
    /// leaves the flag alone: only the playhead moving decides.
    pub fn set_captions_active(&self, active: bool) {
        self.state.borrow_mut().captions_active = active;
    }

    /// How much of the cursor the hide track leaves visible at the playhead,
    /// as the shared editor's hide coverage reports it.
    pub fn set_hide_alpha(&self, alpha: f64) {
        self.state.borrow_mut().hide_alpha = if alpha.is_finite() {
            alpha.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }

    /// Keyboard shortcut captions over the whole stage, outside the video
    /// transform, the way the export decorates the final picture.
    pub fn draw_captions(&self, cr: &Context, width: i32, height: i32, source_seconds: f64) {
        let state = self.state.borrow();
        if state.caption_settings.shortcuts && state.captions_active {
            keystrokes::draw(
                cr,
                f64::from(width),
                f64::from(height),
                &state.captions,
                source_seconds,
            );
        }
    }

    pub fn draw(
        &self,
        cr: &Context,
        allocation_width: i32,
        allocation_height: i32,
        source_seconds: f64,
        source_size: Option<(u32, u32)>,
        view: Option<super::zoom::ViewRect>,
    ) {
        let state = self.state.borrow();
        let Some(sidecar) = state
            .sidecar
            .as_ref()
            .filter(|sidecar| sidecar.can_render_cursor_overlay())
        else {
            return;
        };
        let typing_alpha = super::typing::typing_alpha(
            &sidecar.keys,
            &sidecar.pointer,
            &sidecar.presses,
            source_seconds,
        );
        if typing_alpha <= 0.001 {
            return;
        }
        let settings = state.settings;
        let Some((video_width, video_height)) = source_size else {
            return;
        };
        let Some(view) = view else {
            return;
        };
        let video_width = f64::from(video_width);
        let video_height = f64::from(video_height);
        let Some(rect) = video_rect_for(
            f64::from(allocation_width),
            f64::from(allocation_height),
            video_width,
            video_height,
        ) else {
            return;
        };
        let motion = CursorMotion {
            smooth: settings.smooth,
            hide_idle: settings.hide_idle,
            idle_ms: settings.idle_ms,
            speed: settings.speed,
            rotation_mode: settings.rotation_mode,
            rotation_ratio: settings.rotate_on_x_movement_ratio,
            base_rotation_deg: settings.base_rotation_deg,
        };
        let Some(mut frame) =
            sidecar.presented_in_video_at(source_seconds, motion, video_width, video_height)
        else {
            return;
        };
        frame.alpha *= typing_alpha;
        // A hide clip takes the sprite and its click rings out together, so
        // the coverage multiplies the one alpha both are drawn with.
        frame.alpha *= state.hide_alpha;
        let map_point = |point| {
            let (x, y) = super::zoom::source_to_stage(point, view, rect.width, rect.height);
            (rect.x + x, rect.y + y)
        };

        let _ = cr.save();
        cr.rectangle(rect.x, rect.y, rect.width, rect.height);
        cr.clip();

        // Ripple is a footage warp in the preview and the export alike, so the
        // drawn ring is never used for it; every other effect is drawn.
        if settings.click_effect != ClickEffect::Ripple {
            for (x, y, progress) in sidecar.click_ripples_in_video_at(
                source_seconds,
                settings.click_window_seconds(),
                video_width,
                video_height,
            ) {
                let (x, y) = map_point((x, y));
                crate::recording::editor::cursor_sprite::draw_click(
                    cr,
                    x,
                    y,
                    progress,
                    settings,
                    frame.alpha,
                    rect.width,
                );
            }
        }

        let (x, y) = map_point((frame.x, frame.y));
        // The click effect is the feedback: the shared preview passes a flat
        // pulse and lets the ripple warp or the drawn circle carry the click,
        // so the sprite never paints its own halo on top of either.
        let capture = state
            .video_path
            .as_deref()
            .and_then(|path| sidecar.cursor_capture(path, &frame));
        crate::recording::editor::cursor_sprite::draw_recorded(
            cr,
            x,
            y,
            1.0,
            super::press::press_cursor_scale(sidecar, source_seconds),
            &frame,
            capture.as_ref(),
            settings,
            frame.alpha,
        );
        let _ = cr.restore();
    }
}

/// Build the warp for a live click, shared by the preview and the export.
///
/// `view` is the source rectangle the texture shows: the whole frame for the
/// preview (its camera is a CSS transform) and the camera crop for the export.
pub(super) fn preview_ripple(
    sidecar: &PointerSidecar,
    settings: CursorSettings,
    source_seconds: f64,
    source_size: (u32, u32),
    view: super::zoom::ViewRect,
) -> Option<PreviewRipple> {
    if settings.click_effect != ClickEffect::Ripple
        || !source_seconds.is_finite()
        || !sidecar.can_render_cursor_overlay()
    {
        return None;
    }
    let width = f64::from(source_size.0);
    let height = f64::from(source_size.1);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let mut clicks: Vec<_> = sidecar
        .clicks
        .iter()
        .filter_map(|click| {
            let age_ms = (source_seconds - click.t) * 1000.0;
            if !(0.0..RIPPLE_VISIBLE_DURATION_MS).contains(&age_ms) {
                return None;
            }
            let (x, y) = sidecar.map_to_video(click.x, click.y, width, height);
            Some(PreviewRippleClick { x, y, age_ms })
        })
        .collect();
    clicks.sort_by(|left, right| left.age_ms.total_cmp(&right.age_ms));
    clicks.truncate(3);
    clicks.reverse();
    clicks
        .iter()
        .any(|click| ripple_bounce_at_ms(click.age_ms).abs() > 1e-9)
        .then_some(PreviewRipple {
            view,
            source_width: width,
            source_height: height,
            clicks,
        })
}

pub(super) fn video_rect_for(
    allocation_width: f64,
    allocation_height: f64,
    video_width: f64,
    video_height: f64,
) -> Option<VideoRect> {
    if !allocation_width.is_finite()
        || !allocation_height.is_finite()
        || !video_width.is_finite()
        || !video_height.is_finite()
        || allocation_width <= 0.0
        || allocation_height <= 0.0
        || video_width <= 0.0
        || video_height <= 0.0
    {
        return None;
    }

    let scale = (allocation_width / video_width).min(allocation_height / video_height);
    let width = video_width * scale;
    let height = video_height * scale;
    Some(VideoRect {
        x: (allocation_width - width) / 2.0,
        y: (allocation_height - height) / 2.0,
        width,
        height,
    })
}

#[cfg(test)]
fn source_to_stage(
    point: (f64, f64),
    rect: VideoRect,
    video_width: f64,
    video_height: f64,
) -> (f64, f64) {
    (
        rect.x + point.0 * rect.width / video_width,
        rect.y + point.1 * rect.height / video_height,
    )
}

#[cfg(test)]
mod tests {
    use super::{source_to_stage, video_rect_for, CursorModel, VideoRect};
    use crate::recording::editor::model::{ClickEffect, CursorSettings, CursorTheme};
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, CursorKind, KeySample, PointerSample, PointerSidecar,
    };

    fn key_down(t: f64, key: i32, modifiers: &[&str]) -> KeySample {
        KeySample {
            t,
            kind: "key_down".to_owned(),
            key,
            scan_code: 0,
            modifiers: modifiers.iter().map(|name| (*name).to_owned()).collect(),
            cursor_id: None,
        }
    }

    fn cursor_with_keys(keys: Vec<KeySample>) -> CursorModel {
        let cursor = CursorModel::new();
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
        );
        sidecar.keys = keys;
        cursor.state.borrow_mut().sidecar = Some(sidecar);
        cursor
    }

    #[test]
    fn caption_settings_rebuild_the_captions_only_when_they_change() {
        let cursor = cursor_with_keys(vec![key_down(1.0, 65, &[]), key_down(3.0, 46, &[])]);
        cursor.set_caption_settings(crate::video_editor::keystrokes::Settings {
            shortcuts: true,
            show_single_letters: false,
        });
        assert_eq!(cursor.state.borrow().captions.len(), 1);

        cursor.set_caption_settings(crate::video_editor::keystrokes::Settings {
            shortcuts: true,
            show_single_letters: true,
        });
        assert_eq!(cursor.state.borrow().captions.len(), 2);
    }

    #[test]
    fn shortcut_captions_paint_without_pointer_samples_or_a_video() {
        let Some((with_keys, without_keys)) = crate::test_support::with_gtk(|| {
            use gtk4::cairo::{Context, Format, ImageSurface};

            let paints = |cursor: &CursorModel| {
                let mut surface = ImageSurface::create(Format::ARgb32, 320, 180).unwrap();
                {
                    let cr = Context::new(&surface).unwrap();
                    cursor.draw_captions(&cr, 320, 180, 1.5);
                }
                surface.flush();
                let pixels = surface.data().unwrap().to_vec();
                pixels.iter().any(|byte| *byte != 0)
            };
            let settings = crate::video_editor::keystrokes::Settings {
                shortcuts: true,
                show_single_letters: false,
            };
            let keyed = cursor_with_keys(vec![key_down(1.0, 75, &["control"])]);
            keyed.set_caption_settings(settings);
            let bare = cursor_with_keys(Vec::new());
            bare.set_caption_settings(settings);
            (paints(&keyed), paints(&bare))
        }) else {
            return;
        };
        assert!(with_keys);
        assert!(!without_keys);
    }

    #[test]
    fn captions_paint_only_while_the_source_clock_holds_footage() {
        let Some((showing, hidden)) = crate::test_support::with_gtk(|| {
            use gtk4::cairo::{Context, Format, ImageSurface};

            let paints = |cursor: &CursorModel| {
                let mut surface = ImageSurface::create(Format::ARgb32, 320, 180).unwrap();
                {
                    let cr = Context::new(&surface).unwrap();
                    cursor.draw_captions(&cr, 320, 180, 1.5);
                }
                surface.flush();
                let pixels = surface.data().unwrap().to_vec();
                pixels.iter().any(|byte| *byte != 0)
            };
            let cursor = cursor_with_keys(vec![key_down(1.0, 75, &["control"])]);
            cursor.set_caption_settings(crate::video_editor::keystrokes::Settings {
                shortcuts: true,
                show_single_letters: false,
            });
            let showing = paints(&cursor);
            cursor.set_captions_active(false);
            let hidden = paints(&cursor);
            (showing, hidden)
        }) else {
            return;
        };
        assert!(showing);
        assert!(!hidden);
    }

    #[test]
    fn the_hide_track_takes_the_cursor_and_its_clicks_out_of_the_preview() {
        let Some((showing, hidden)) = crate::test_support::with_gtk(|| {
            use gtk4::cairo::{Context, Format, ImageSurface};

            let paints = |cursor: &CursorModel| {
                let mut surface = ImageSurface::create(Format::ARgb32, 320, 180).unwrap();
                {
                    let cr = Context::new(&surface).unwrap();
                    cursor.draw(
                        &cr,
                        320,
                        180,
                        1.4,
                        Some((640, 480)),
                        Some((0.0, 0.0, 640.0, 480.0)),
                    );
                }
                surface.flush();
                let painted = surface.data().unwrap().iter().any(|byte| *byte != 0);
                painted
            };
            let cursor = rippling_cursor();
            let showing = paints(&cursor);
            cursor.set_hide_alpha(0.0);
            let hidden = paints(&cursor);
            (showing, hidden)
        }) else {
            return;
        };
        assert!(showing);
        assert!(!hidden);
    }

    fn rippling_cursor() -> CursorModel {
        let cursor = CursorModel::new();
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
        );
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 300.0,
            y: 220.0,
            kind: CursorKind::Default,
            cursor_id: None,
        });
        sidecar.clicks.push(ClickSample {
            t: 1.0,
            x: 320.0,
            y: 240.0,
            button: 1,
            cursor_id: None,
        });
        cursor.state.borrow_mut().sidecar = Some(sidecar);
        cursor.set_settings(CursorSettings {
            click_effect: ClickEffect::Ripple,
            ..CursorSettings::default()
        });
        cursor
    }

    #[test]
    fn only_a_live_click_under_the_ripple_effect_warps_the_frame() {
        let cursor = rippling_cursor();
        let ripple = cursor
            .ripple_at(1.4, Some((640, 480)))
            .expect("a live click should warp");
        assert_eq!(ripple.view, (0.0, 0.0, 640.0, 480.0));
        assert_eq!(ripple.clicks.len(), 1);
        assert_eq!((ripple.clicks[0].x, ripple.clicks[0].y), (320.0, 240.0));

        assert!(cursor.ripple_at(4.0, Some((640, 480))).is_none());
        assert!(cursor.ripple_at(1.4, None).is_none());

        cursor.set_settings(CursorSettings {
            click_effect: ClickEffect::Circle,
            ..CursorSettings::default()
        });
        assert!(cursor.ripple_at(1.4, Some((640, 480))).is_none());
    }

    #[test]
    fn matching_aspects_fill_the_allocation() {
        let rect = video_rect_for(1280.0, 720.0, 1920.0, 1080.0).unwrap();
        assert_eq!(
            rect,
            VideoRect {
                x: 0.0,
                y: 0.0,
                width: 1280.0,
                height: 720.0,
            }
        );
        assert_eq!(
            source_to_stage((960.0, 540.0), rect, 1920.0, 1080.0),
            (640.0, 360.0)
        );
    }

    #[test]
    fn wider_allocation_centres_letterboxed_video() {
        let rect = video_rect_for(1600.0, 1200.0, 1000.0, 800.0).unwrap();
        assert_eq!(
            rect,
            VideoRect {
                x: 50.0,
                y: 0.0,
                width: 1500.0,
                height: 1200.0,
            }
        );
        assert_eq!(
            source_to_stage((0.0, 0.0), rect, 1000.0, 800.0),
            (50.0, 0.0)
        );
    }

    fn alpha_bounds(
        pixels: &[u8],
        width: usize,
        height: usize,
    ) -> Option<(usize, usize, usize, usize)> {
        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for y in 0..height {
            for x in 0..width {
                if pixels[(y * width + x) * 4 + 3] == 0 {
                    continue;
                }
                bounds = Some(match bounds {
                    Some((min_x, min_y, max_x, max_y)) => {
                        (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                    }
                    None => (x, y, x, y),
                });
            }
        }
        bounds
    }

    #[test]
    fn the_preview_paints_a_captured_png_at_its_recorded_hotspot() {
        use crate::recording::editor::sidecar::{
            CaptureRegion, CursorAsset, CursorKind, PointerSample, PointerSidecar,
        };
        use gtk4::cairo::{Context, Format, ImageSurface};

        let video = std::env::temp_dir().join(format!(
            "apexshot-preview-cursor-{}-{:?}.mp4",
            std::process::id(),
            std::thread::current().id()
        ));
        let sidecar_path = PointerSidecar::sidecar_path(&video);
        let folder_name = format!(
            "{}.cursors",
            sidecar_path
                .file_name()
                .expect("the sidecar has a file name")
                .to_string_lossy()
        );
        let folder = sidecar_path.parent().unwrap().join(&folder_name);
        std::fs::create_dir_all(&folder).unwrap();
        let mut image = image::RgbaImage::new(32, 32);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgba([255, 0, 0, 255]);
        }
        image.save(folder.join("hand.png")).unwrap();

        let cursor = CursorModel::new();
        let mut sidecar = PointerSidecar::new(
            0,
            CaptureRegion {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
        );
        sidecar.cursor_images = Some(folder_name);
        sidecar.cursors.push(CursorAsset {
            id: "hand-id".to_string(),
            file: "hand.png".to_string(),
            width: 32,
            height: 32,
            hotspot_x: 1,
            hotspot_y: 1,
            kind: CursorKind::Hand,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 50.0,
            y: 40.0,
            kind: CursorKind::Hand,
            cursor_id: Some("hand-id".to_string()),
        });
        cursor.state.borrow_mut().sidecar = Some(sidecar);
        cursor.state.borrow_mut().video_path = Some(video);
        cursor.set_settings(CursorSettings {
            smooth: 0.0,
            theme: CursorTheme::Macos,
            ..CursorSettings::default()
        });

        let mut surface = ImageSurface::create(Format::ARgb32, 100, 100).unwrap();
        {
            let cr = Context::new(&surface).unwrap();
            cursor.draw(
                &cr,
                100,
                100,
                0.0,
                Some((100, 100)),
                Some((0.0, 0.0, 100.0, 100.0)),
            );
        }
        surface.flush();
        let pixels = surface.data().unwrap().to_vec();
        assert_eq!(alpha_bounds(&pixels, 100, 100), Some((48, 38, 96, 86)));

        let _ = std::fs::remove_dir_all(&folder);
    }
}
