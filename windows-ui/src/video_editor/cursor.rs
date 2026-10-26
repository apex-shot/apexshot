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
    settings: CursorSettings,
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
                settings: CursorSettings::default(),
            })),
        }
    }

    pub fn load(&self, path: Option<&Path>) {
        self.state.borrow_mut().sidecar = path.and_then(PointerSidecar::load_next_to_video);
    }

    pub fn settings(&self) -> CursorSettings {
        self.state.borrow().settings
    }

    pub fn sidecar(&self) -> Option<PointerSidecar> {
        self.state.borrow().sidecar.clone()
    }

    pub fn set_settings(&self, settings: CursorSettings) {
        self.state.borrow_mut().settings = settings.clamped();
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
        let Some((width, height)) = source_size else {
            return None;
        };
        preview_ripple(
            state.sidecar.as_ref()?,
            state.settings,
            source_seconds,
            (width, height),
            (0.0, 0.0, f64::from(width), f64::from(height)),
        )
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
        };
        let Some(frame) =
            sidecar.presented_in_video_at(source_seconds, motion, video_width, video_height)
        else {
            return;
        };
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
        crate::recording::editor::cursor_sprite::draw(
            cr,
            x,
            y,
            1.0,
            super::press::press_cursor_scale(sidecar, source_seconds),
            frame.kind.as_str(),
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
    use crate::recording::editor::model::{ClickEffect, CursorSettings};
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, CursorKind, PointerSample, PointerSidecar,
    };

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
        });
        sidecar.clicks.push(ClickSample {
            t: 1.0,
            x: 320.0,
            y: 240.0,
            button: 1,
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
}
