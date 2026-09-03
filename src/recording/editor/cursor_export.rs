use super::cursor_sprite;
use super::model::{even_crop_rect, source_to_zoomed_point, VideoEditState};
use super::sidecar::CursorMotion;
use gtk4::cairo::{Context, Format, ImageSurface, Operator};
use std::io::Write;
use std::path::Path;

const CURSOR_FPS: f64 = 30.0;

pub fn write_rgba_track(
    state: &VideoEditState,
    start: f64,
    end: f64,
    width: u32,
    height: u32,
    path: &Path,
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
    let frames = ((duration * CURSOR_FPS).ceil() as usize).max(1);
    let (crop_x, crop_y, eff_w, eff_h) = state.crop_or_full();
    let src_w = eff_w.max(2.0) as u32;
    let src_h = eff_h.max(2.0) as u32;
    let cursor = state.cursor.clamped();
    let motion = CursorMotion {
        smooth: cursor.smooth,
        hide_idle: cursor.hide_idle,
        idle_ms: cursor.idle_ms,
        trail: cursor.trail,
        tilt: cursor.tilt,
        sway: cursor.sway,
        speed: cursor.speed,
    };
    let mut surface = ImageSurface::create(Format::ARgb32, width as i32, height as i32)?;
    let mut file = std::fs::File::create(path)?;
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for index in 0..frames {
        let source_t = start + index as f64 / CURSOR_FPS;
        let cr = Context::new(&surface)?;
        cr.set_operator(Operator::Clear);
        let _ = cr.paint();
        cr.set_operator(Operator::Over);
        let (scale, center) = state.eval_zoom(source_t);
        let center = (center.0 - crop_x, center.1 - crop_y);
        let (zx, zy, zw, zh) = even_crop_rect(scale, center, src_w, src_h);
        let view = (crop_x + zx as f64, crop_y + zy as f64, zw as f64, zh as f64);
        let mut overlay_cursor = cursor;
        overlay_cursor.size = cursor_sprite::overlay_scale(cursor.size, scale);
        if let Some(mut frame) = sidecar.presented_at(source_t, motion) {
            frame.alpha *= state.cursor_hide_alpha_for_source(source_t);
            for (x, y, progress) in
                sidecar.click_ripples_at(source_t, overlay_cursor.click_window_seconds())
            {
                let (px, py) = source_to_zoomed_point(x, y, view, width as f64, height as f64);
                cursor_sprite::draw_click(&cr, px, py, progress, overlay_cursor, frame.alpha);
            }
            for &(x, y, ghost) in &frame.trail {
                let (px, py) = source_to_zoomed_point(x, y, view, width as f64, height as f64);
                cursor_sprite::draw_tilted(
                    &cr,
                    px,
                    py,
                    1.0,
                    frame.kind.as_str(),
                    overlay_cursor,
                    frame.alpha * ghost,
                    frame.tilt,
                );
            }
            let (px, py) =
                source_to_zoomed_point(frame.x, frame.y, view, width as f64, height as f64);
            cursor_sprite::draw_tilted(
                &cr,
                px,
                py,
                1.0,
                frame.kind.as_str(),
                overlay_cursor,
                frame.alpha,
                frame.tilt,
            );
        }
        drop(cr);
        surface.flush();
        write_rgba_frame(&mut surface, width, height, &mut pixels, &mut file)?;
    }
    Ok(())
}

fn write_rgba_frame(
    surface: &mut ImageSurface,
    width: u32,
    height: u32,
    rgba: &mut [u8],
    file: &mut std::fs::File,
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
    file.write_all(rgba)?;
    Ok(())
}

pub fn fps() -> f64 {
    CURSOR_FPS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::VideoMetadata;
    use crate::recording::editor::sidecar::{
        CaptureRegion, CursorKind, PointerSample, PointerSidecar,
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
        state.cursor.trail = 0.6;
        state.cursor.tilt = 0.8;
        let dir = std::env::temp_dir().join(format!("apexshot-cursor-rgba-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("cursor.rgba");
        write_rgba_track(&state, 0.0, 0.2, 80, 60, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let frames = ((0.2 * CURSOR_FPS).ceil() as usize).max(1);
        assert_eq!(bytes.len(), frames * 80 * 60 * 4);
        assert!(bytes.iter().any(|b| *b != 0));
        let _ = std::fs::remove_dir_all(&dir);
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

        let path = std::env::temp_dir().join("apexshot-inferred-cursor.rgba");
        let error = write_rgba_track(&state, 0.0, 0.2, 80, 60, &path).unwrap_err();
        assert!(error.to_string().contains("inferred"));
        assert!(!path.exists());
    }
}
