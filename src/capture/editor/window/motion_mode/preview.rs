use gtk4::cairo::{Context, Format, ImageSurface};
use std::cell::RefCell;
use std::rc::Rc;

use super::{MotionHoverTrack, MotionRuntime};
use crate::recording::editor::model::{MotionAppearance, MotionState};

pub(super) fn draw_motion_preview(
    context: &Context,
    width: i32,
    height: i32,
    runtime: &Rc<RefCell<MotionRuntime>>,
    prefers_dark: bool,
) {
    let backdrop = {
        let mut runtime = runtime.borrow_mut();
        if runtime.card.is_none() {
            crate::capture::editor::render::draw_canvas_checkerboard_background(
                context,
                width,
                height,
                None,
                !prefers_dark,
            );
            return;
        }
        cached_backdrop(&mut runtime, width, height, prefers_dark)
    };
    // The cache build needs mutable access, but rendering itself only reads
    // state. Avoid cloning the complete Motion state (and title strings) on
    // every playback frame.
    let runtime = runtime.borrow();
    let (card, card_scale) = match runtime.card_preview.as_ref() {
        Some(preview) => (preview, runtime.card_scale),
        None => (
            runtime
                .card
                .as_ref()
                .expect("Motion preview card was checked"),
            1.0,
        ),
    };
    // Hover scrub: the red hover line drives the preview only while the
    // pointer is directly over a clip on its own lane and nothing is
    // playing. The playhead itself never moves — it only advances on Play
    // or a handle/ruler scrub.
    let time = hover_preview_frame(
        &runtime.motion,
        runtime.hover_time,
        runtime.hover_track,
        runtime.playing,
    )
    .unwrap_or(runtime.motion.playhead);
    let live_preview = runtime.live_preview || runtime.playing;

    if let Some(backdrop) = backdrop {
        context.set_source_surface(&backdrop, 0.0, 0.0).ok();
        context.paint().ok();
        super::super::motion_render::draw_motion_foreground(
            context,
            width,
            height,
            card,
            &runtime.motion,
            runtime.watermark_surface.as_ref(),
            time,
            true,
            live_preview,
            card_scale,
        );
    } else {
        // Preserve a correct first frame while GTK is still assigning a
        // drawable allocation.
        super::super::motion_render::draw_motion_frame(
            context,
            width,
            height,
            card,
            &runtime.motion,
            runtime.background_surface.as_ref(),
            runtime.watermark_surface.as_ref(),
            time,
            true,
            prefers_dark,
            live_preview,
            card_scale,
        );
    }
}

fn cached_backdrop(
    runtime: &mut MotionRuntime,
    width: i32,
    height: i32,
    prefers_dark: bool,
) -> Option<ImageSurface> {
    let width = width.max(1);
    let height = height.max(1);
    let stale = runtime.backdrop_cache.as_ref().is_none_or(|cache| {
        cache.width != width
            || cache.height != height
            || cache.prefers_dark != prefers_dark
            || !same_backdrop_appearance(&cache.appearance, &runtime.motion.appearance)
    });
    if stale {
        let surface = ImageSurface::create(Format::ARgb32, width, height).ok()?;
        let context = Context::new(&surface).ok()?;
        super::super::motion_render::draw_motion_backdrop(
            &context,
            width,
            height,
            &runtime.motion,
            runtime.background_surface.as_ref(),
            true,
            prefers_dark,
        );
        surface.flush();
        runtime.backdrop_cache = Some(super::session::MotionBackdropCache {
            width,
            height,
            prefers_dark,
            appearance: runtime.motion.appearance.clone(),
            surface,
        });
    }
    runtime
        .backdrop_cache
        .as_ref()
        .map(|cache| cache.surface.clone())
}

/// Card styling (padding, border, and shadow) is composited above the scene.
/// Do not evict a costly blurred background just because one of those controls
/// changes while it is being dragged.
fn same_backdrop_appearance(a: &MotionAppearance, b: &MotionAppearance) -> bool {
    a.background_fill_type == b.background_fill_type
        && a.background_color == b.background_color
        && a.gradient_color_1 == b.gradient_color_1
        && a.gradient_color_2 == b.gradient_color_2
        && a.wallpaper_image_name == b.wallpaper_image_name
        && a.custom_background_image == b.custom_background_image
        && a.background_blur == b.background_blur
        && a.background_noise == b.background_noise
}

/// Hover frame for the preview, or `None` to stay on the playhead.
/// Motions play on their allocated clip span only: hovering empty lane space
/// next to a clip (or the other lane's gap) never fakes a motion preview.
pub(super) fn hover_preview_frame(
    motion: &MotionState,
    hover_time: Option<f64>,
    hover_track: Option<MotionHoverTrack>,
    playing: bool,
) -> Option<f64> {
    if playing {
        return None;
    }
    let hover = hover_time?;
    let over_clip = match hover_track? {
        MotionHoverTrack::Motion => motion.segment_index_at(hover).is_some(),
        MotionHoverTrack::Text => motion.text_index_at(hover).is_some(),
    };
    over_clip.then_some(hover)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_preview_only_plays_directly_over_a_clip() {
        let mut motion = MotionState::default();
        motion.add_segment_at(0.0).expect("motion clip");
        // Directly over the motion clip: hover frame wins, playhead untouched.
        assert_eq!(
            hover_preview_frame(&motion, Some(0.5), Some(MotionHoverTrack::Motion), false),
            Some(0.5)
        );
        // Empty lane space next to the clip: no fake motion preview.
        assert_eq!(
            hover_preview_frame(&motion, Some(3.0), Some(MotionHoverTrack::Motion), false),
            None
        );
        // Playback always owns the preview.
        assert_eq!(
            hover_preview_frame(&motion, Some(0.5), Some(MotionHoverTrack::Motion), true),
            None
        );
        // A motion time under a text-lane hover stays on the playhead.
        assert_eq!(
            hover_preview_frame(&motion, Some(0.5), Some(MotionHoverTrack::Text), false),
            None
        );
        motion.add_text_at(4.0).expect("text clip");
        assert_eq!(
            hover_preview_frame(&motion, Some(4.5), Some(MotionHoverTrack::Text), false),
            Some(4.5)
        );
    }

    #[test]
    fn card_styling_keeps_the_cached_backdrop_valid() {
        let original = MotionAppearance::default();
        let mut styled = original.clone();
        styled.background_padding = 24.0;
        styled.border_radius = 18.0;
        styled.border_thickness = 2.0;
        styled.shadow_opacity = 0.75;
        styled.shadow_blur = 32.0;
        styled.shadow_position = (14.0, -8.0);

        assert!(same_backdrop_appearance(&original, &styled));

        styled.background_noise = 0.2;
        assert!(!same_backdrop_appearance(&original, &styled));
    }
}
