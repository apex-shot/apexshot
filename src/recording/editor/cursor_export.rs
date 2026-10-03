use super::cursor_sprite;
use super::model::{
    even_crop_rect, press_cursor_scale, source_to_zoomed_point, ClickEffect, VideoEditState,
};
use super::sidecar::CursorMotion;
use gtk4::cairo::{Context, Format, ImageSurface, Operator};
use std::io::Write;

/// Render the cursor overlay as raw RGBA frames, one per output frame.
///
/// Frames go to `sink` rather than to a file: a composite export streams them
/// into ffmpeg through a pipe (see [`super::cursor_track`]), so a track that
/// runs to gigabytes never exists on disk. Callers that only want the bytes can
/// pass a `Vec<u8>`.
pub fn write_rgba_track<W: Write>(
    state: &VideoEditState,
    start: f64,
    end: f64,
    width: u32,
    height: u32,
    skip_ripple_ring: bool,
    sink: &mut W,
) -> anyhow::Result<()> {
    let Some(sidecar) = state.sidecar.as_ref() else {
        anyhow::bail!("no pointer sidecar");
    };
    if !sidecar.can_render_cursor_overlay() {
        anyhow::bail!("pointer data was inferred from baked video frames");
    }
    let width = width.max(2);
    let height = height.max(2);
    let duration = (end - start).max(0.0);
    let frame_rate = state.metadata.export_frame_rate();
    let frames = ((duration * frame_rate).ceil() as usize).max(1);
    let (crop_x, crop_y, eff_w, eff_h) = state.crop_or_full();
    let src_w = eff_w.max(2.0) as u32;
    let src_h = eff_h.max(2.0) as u32;
    let cursor = state.cursor.clamped();
    let motion = CursorMotion {
        smooth: cursor.smooth,
        hide_idle: cursor.hide_idle,
        idle_ms: cursor.idle_ms,
        speed: cursor.speed,
    };
    let mut surface = ImageSurface::create(Format::ARgb32, width as i32, height as i32)?;
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for index in 0..frames {
        let source_t = start + index as f64 / frame_rate;
        let cr = Context::new(&surface)?;
        cr.set_operator(Operator::Clear);
        let _ = cr.paint();
        cr.set_operator(Operator::Over);
        let (scale, center) = state.eval_zoom(source_t);
        let center = (center.0 - crop_x, center.1 - crop_y);
        let (zx, zy, zw, zh) = even_crop_rect(scale, center, src_w, src_h);
        let view = (crop_x + zx as f64, crop_y + zy as f64, zw as f64, zh as f64);
        // Click effects are sized as fractions of the source video width, so
        // map that width into the same zoomed space the click points use.
        let reference_width = state.metadata.width as f64 * width as f64 / zw.max(1) as f64;
        let mut overlay_cursor = cursor;
        overlay_cursor.size = cursor_sprite::overlay_scale(cursor.size, scale);
        if let Some(mut frame) = sidecar.presented_in_video_at(
            source_t,
            motion,
            state.metadata.width as f64,
            state.metadata.height as f64,
        ) {
            frame.alpha *= state.cursor_hide_alpha_for_source(source_t);
            for (x, y, progress) in sidecar.click_ripples_in_video_at(
                source_t,
                overlay_cursor.click_window_seconds(),
                state.metadata.width as f64,
                state.metadata.height as f64,
            ) {
                // When the composite runs the footage warp, the displaced
                // band *is* the ripple; drawing a ring on top would double it.
                if skip_ripple_ring && overlay_cursor.click_effect == ClickEffect::Ripple {
                    continue;
                }
                let (px, py) = source_to_zoomed_point(x, y, view, width as f64, height as f64);
                cursor_sprite::draw_click(
                    &cr,
                    px,
                    py,
                    progress,
                    overlay_cursor,
                    frame.alpha,
                    reference_width,
                );
            }
            let (px, py) =
                source_to_zoomed_point(frame.x, frame.y, view, width as f64, height as f64);
            cursor_sprite::draw(
                &cr,
                px,
                py,
                1.0,
                press_cursor_scale(sidecar, source_t),
                frame.kind.as_str(),
                overlay_cursor,
                frame.alpha,
            );
        }
        drop(cr);
        surface.flush();
        write_rgba_frame(&mut surface, width, height, &mut pixels, sink)?;
    }
    Ok(())
}

fn write_rgba_frame<W: Write>(
    surface: &mut ImageSurface,
    width: u32,
    height: u32,
    rgba: &mut [u8],
    sink: &mut W,
) -> anyhow::Result<()> {
    let stride = surface.stride() as usize;
    let data = surface.data()?;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let i = y * stride + x * 4;
            let b = data[i] as u16;
            let g = data[i + 1] as u16;
            let r = data[i + 2] as u16;
            let a = data[i + 3] as u16;
            let o = (y * width as usize + x) * 4;
            if a == 0 {
                rgba[o] = 0;
                rgba[o + 1] = 0;
                rgba[o + 2] = 0;
                rgba[o + 3] = 0;
            } else {
                rgba[o] = ((r * 255) / a).min(255) as u8;
                rgba[o + 1] = ((g * 255) / a).min(255) as u8;
                rgba[o + 2] = ((b * 255) / a).min(255) as u8;
                rgba[o + 3] = a as u8;
            }
        }
    }
    drop(data);
    sink.write_all(rgba)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::VideoMetadata;
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, CursorKind, PointerSample, PointerSidecar,
    };
    use std::path::PathBuf;

    #[test]
    fn writes_rgba_bytes_for_each_frame() {
        let mut state = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/cursor-export.mp4"),
            duration_seconds: 0.2,
            width: 80,
            height: 60,
            file_size_bytes: 8,
            has_audio: false,
            frame_rate: 30.0,
        });
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 10.0,
            kind: CursorKind::Default,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.2,
            x: 40.0,
            y: 20.0,
            kind: CursorKind::Hand,
        });
        state.sidecar = Some(sidecar);
        let mut bytes = Vec::new();
        write_rgba_track(&state, 0.0, 0.2, 80, 60, false, &mut bytes).unwrap();
        let frames = ((0.2 * state.metadata.export_frame_rate()).ceil() as usize).max(1);
        assert_eq!(bytes.len(), frames * 80 * 60 * 4);
        assert!(bytes.iter().any(|b| *b != 0));
    }

    #[test]
    fn the_drawn_ripple_ring_is_the_fallback_when_the_warp_is_skipped() {
        let mut state = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/cursor-export-ring.mp4"),
            duration_seconds: 0.4,
            width: 80,
            height: 60,
            file_size_bytes: 8,
            has_audio: false,
            frame_rate: 30.0,
        });
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 2.0,
            y: 2.0,
            kind: CursorKind::Default,
        });
        sidecar.clicks.push(ClickSample {
            t: 0.0,
            x: 40.0,
            y: 30.0,
            button: 1,
        });
        state.sidecar = Some(sidecar);
        state.cursor.click_effect = ClickEffect::Ripple;

        // `false` is the fallback: the warp is not running, so the ring draws.
        let mut ring = Vec::new();
        write_rgba_track(&state, 0.1, 0.2, 80, 60, false, &mut ring).unwrap();
        // `true` is the composite: the displaced band is the ripple, so the
        // overlay must not double it with a ring.
        let mut warp = Vec::new();
        write_rgba_track(&state, 0.1, 0.2, 80, 60, true, &mut warp).unwrap();
        let lit = |track: &[u8]| track.chunks_exact(4).filter(|pixel| pixel[3] != 0).count();
        assert!(
            lit(&ring) > lit(&warp),
            "the fallback must draw the ripple ring"
        );
        assert!(lit(&warp) > 0, "the cursor must still render with the warp");
    }

    #[test]
    fn cursor_track_follows_the_source_frame_rate() {
        let mut state = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/cursor-export-hi-fps.mp4"),
            duration_seconds: 0.2,
            width: 80,
            height: 60,
            file_size_bytes: 8,
            has_audio: false,
            frame_rate: 60.0,
        });
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 10.0,
            kind: CursorKind::Default,
        });
        sidecar.pointer.push(PointerSample {
            t: 0.2,
            x: 40.0,
            y: 20.0,
            kind: CursorKind::Default,
        });
        state.sidecar = Some(sidecar);
        let mut bytes = Vec::new();
        write_rgba_track(&state, 0.0, 0.2, 80, 60, false, &mut bytes).unwrap();
        // 0.2 s at the source's 60 fps is 12 frames — twice the old fixed
        // 30 fps grid, so the cursor no longer steps in high-fps exports.
        assert_eq!(bytes.len(), 12 * 80 * 60 * 4);
    }

    #[test]
    fn rejects_inferred_pointer_tracks_with_a_baked_cursor() {
        let mut state = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/imported-video.mp4"),
            duration_seconds: 0.2,
            width: 80,
            height: 60,
            file_size_bytes: 8,
            has_audio: false,
            frame_rate: 30.0,
        });
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 10.0,
            y: 10.0,
            kind: CursorKind::Default,
        });
        sidecar.mark_inferred_from_video();
        state.sidecar = Some(sidecar);

        let mut sink = Vec::new();
        let error = write_rgba_track(&state, 0.0, 0.2, 80, 60, false, &mut sink).unwrap_err();
        assert!(error.to_string().contains("inferred"));
        assert!(sink.is_empty(), "a rejected track must write nothing");
    }
}
