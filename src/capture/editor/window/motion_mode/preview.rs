use gtk4::cairo::Context;
use std::cell::RefCell;
use std::rc::Rc;

use super::MotionRuntime;

pub(super) fn draw_motion_preview(
    context: &Context,
    width: i32,
    height: i32,
    runtime: &Rc<RefCell<MotionRuntime>>,
    prefers_dark: bool,
) {
    let runtime = runtime.borrow();
    let Some(surface) = runtime.card.as_ref() else {
        crate::capture::editor::render::draw_canvas_checkerboard_background(
            context,
            width,
            height,
            None,
            !prefers_dark,
        );
        return;
    };
    super::super::motion_render::draw_motion_frame(
        context,
        width,
        height,
        surface,
        &runtime.motion,
        runtime.background_surface.as_ref(),
        runtime.watermark_surface.as_ref(),
        runtime.motion.playhead,
        true,
        prefers_dark,
        runtime.live_preview || runtime.playing,
    );
}
