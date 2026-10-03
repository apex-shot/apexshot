//! Rasterises the studied ripple's footage displacement as an ffmpeg `remap`
//! pair (xmap/ymap), so the composite export can sample the footage at
//! `pixel - pull` instead of drawing a ring on the cursor overlay.
//!
//! The maps are 16-bit single-channel, the only depth ffmpeg's `remap`
//! accepts. Coordinates are absolute source pixels, so a map value is the
//! source pixel a destination pixel reads. Values are rounded to the nearest
//! integer pixel; the displacement maths itself is the f64 curve in
//! [`super::click_effect`].
//!
//! The maps are generated at the *source* resolution and applied before the
//! crop and zoom, so the studied read margin is the whole source rather than a
//! padded texture, and the displacement is in video pixels at every zoom.

use super::click_effect::{ripple_pull_px, RIPPLE_BAND_01, RIPPLE_PULL_SIZE_01};
use super::model::{ClickEffect, VideoEditState};
use std::io::Write;
use std::path::Path;

/// True when the export window `[start, end)` contains a frame where the
/// studied ripple is visible.
///
/// A click at `c` is visible over `[c, c + window)`; the export intersects it
/// when `c < end` and `c + window > start`. The window is the ripple's fixed
/// one-second lifetime.
pub fn ripple_warp_active(state: &VideoEditState, start: f64, end: f64) -> bool {
    if state.cursor.click_effect != ClickEffect::Ripple {
        return false;
    }
    let Some(sidecar) = state.sidecar.as_ref() else {
        return false;
    };
    if !sidecar.can_render_cursor_overlay() {
        return false;
    }
    let window = state.cursor.click_window_seconds();
    sidecar
        .clicks
        .iter()
        .any(|click| click.t < end && click.t + window > start)
}

/// Write the per-frame `xmap`/`ymap` pair the composite export feeds to
/// ffmpeg's `remap`.
///
/// `width` and `height` are the source pixel dimensions the maps apply to.
/// Every frame gets a map: pixels outside an active ripple band read their own
/// coordinate, so the warp is an identity outside the frames where a ripple is
/// actually visible.
pub fn write_ripple_maps(
    state: &VideoEditState,
    start: f64,
    end: f64,
    width: u32,
    height: u32,
    x_path: &Path,
    y_path: &Path,
) -> anyhow::Result<()> {
    let Some(sidecar) = state.sidecar.as_ref() else {
        anyhow::bail!("no pointer sidecar for the ripple warp");
    };
    if !sidecar.can_render_cursor_overlay() {
        anyhow::bail!("pointer data was inferred from baked video frames");
    }
    let width = width.max(2);
    let height = height.max(2);
    let frame_rate = state.metadata.export_frame_rate();
    let frames = (((end - start).max(0.0) * frame_rate).ceil() as usize).max(1);
    let window = state.cursor.click_window_seconds();
    let video_width = state.metadata.width as f64;
    let video_height = state.metadata.height as f64;

    let mut x_file = std::fs::File::create(x_path)?;
    let mut y_file = std::fs::File::create(y_path)?;
    let mut x_map = vec![0u16; (width as usize) * (height as usize)];
    let mut y_map = vec![0u16; (width as usize) * (height as usize)];

    for index in 0..frames {
        let source_t = start + index as f64 / frame_rate;
        write_identity(&mut x_map, &mut y_map, width, height);
        let clicks = sidecar.click_ripples_in_video_at(source_t, window, video_width, video_height);
        if !clicks.is_empty() {
            paint_ripples(
                &mut x_map,
                &mut y_map,
                width,
                height,
                &clicks,
                window,
                video_width,
            );
        }
        write_gray16(&mut x_file, &x_map)?;
        write_gray16(&mut y_file, &y_map)?;
    }
    Ok(())
}

fn write_identity(x_map: &mut [u16], y_map: &mut [u16], width: u32, height: u32) {
    for y in 0..height {
        let row = (y as usize) * (width as usize);
        for x in 0..width {
            x_map[row + x as usize] = x as u16;
            y_map[row + x as usize] = y as u16;
        }
    }
}

fn write_gray16(file: &mut std::fs::File, map: &[u16]) -> anyhow::Result<()> {
    let mut bytes = Vec::with_capacity(map.len() * 2);
    for value in map {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    file.write_all(&bytes)?;
    Ok(())
}

/// Subtract each active click's pull from the map, rounding to the nearest
/// source pixel and clamping to the frame so the warp never samples the black
/// `remap` fill at the edge.
///
/// The studied shader sums the offsets of all active clicks and offsets the
/// sample once; computing every click at the same destination pixel and adding
/// the vectors here is the same sum.
fn paint_ripples(
    x_map: &mut [u16],
    y_map: &mut [u16],
    width: u32,
    height: u32,
    clicks: &[(f64, f64, f64)],
    window: f64,
    video_width: f64,
) {
    let band = video_width * RIPPLE_BAND_01;
    let band_reach = band * 3.0;
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for (click_x, click_y, progress) in clicks {
        let age_ms = progress * window * 1000.0;
        let radius =
            super::click_effect::ripple_radius_px(age_ms, RIPPLE_PULL_SIZE_01, video_width);
        let reach = radius + band_reach + 1.0;
        let x0 = ((click_x - reach).floor().max(0.0)) as u32;
        let x1 = ((click_x + reach).ceil().min(width as f64 - 1.0)).max(0.0) as u32;
        let y0 = ((click_y - reach).floor().max(0.0)) as u32;
        let y1 = ((click_y + reach).ceil().min(height as f64 - 1.0)).max(0.0) as u32;
        bounds = Some(match bounds {
            Some((bx0, by0, bx1, by1)) => (bx0.min(x0), by0.min(y0), bx1.max(x1), by1.max(y1)),
            None => (x0, y0, x1, y1),
        });
    }
    let Some((x0, y0, x1, y1)) = bounds else {
        return;
    };
    for py in y0..=y1 {
        for px in x0..=x1 {
            let mut pull_x = 0.0;
            let mut pull_y = 0.0;
            for (click_x, click_y, progress) in clicks {
                let age_ms = progress * window * 1000.0;
                let (dx, dy) = ripple_pull_px(
                    px as f64,
                    py as f64,
                    *click_x,
                    *click_y,
                    age_ms,
                    RIPPLE_PULL_SIZE_01,
                    video_width,
                );
                pull_x += dx;
                pull_y += dy;
            }
            if pull_x == 0.0 && pull_y == 0.0 {
                continue;
            }
            let source_x = (px as f64 - pull_x).round().clamp(0.0, width as f64 - 1.0);
            let source_y = (py as f64 - pull_y).round().clamp(0.0, height as f64 - 1.0);
            let index = (py as usize) * (width as usize) + px as usize;
            x_map[index] = source_x as u16;
            y_map[index] = source_y as u16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::editor::model::VideoMetadata;
    use crate::recording::editor::sidecar::{
        CaptureRegion, ClickSample, CursorKind, PointerSample, PointerSidecar,
    };
    use std::path::PathBuf;

    fn warp_state(width: u32, height: u32, clicks: Vec<ClickSample>) -> VideoEditState {
        let mut state = VideoEditState::new(VideoMetadata {
            path: PathBuf::from("/tmp/ripple-warp.mp4"),
            duration_seconds: 2.0,
            width,
            height,
            file_size_bytes: 8,
            has_audio: false,
            frame_rate: 30.0,
        });
        state.cursor.click_effect = ClickEffect::Ripple;
        let mut sidecar =
            PointerSidecar::new(0, CaptureRegion::from_capture(None, None, None, None));
        sidecar.pointer.push(PointerSample {
            t: 0.0,
            x: 0.0,
            y: 0.0,
            kind: CursorKind::Default,
        });
        sidecar.clicks = clicks;
        state.sidecar = Some(sidecar);
        state
    }

    fn ffmpeg_available() -> bool {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("apexshot-ripple-warp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn warp_is_active_only_for_a_visible_ripple() {
        let click = ClickSample {
            t: 1.0,
            x: 10.0,
            y: 10.0,
            button: 1,
        };
        let state = warp_state(64, 48, vec![click]);
        assert!(ripple_warp_active(&state, 0.5, 1.5));
        // Ends before the click.
        assert!(!ripple_warp_active(&state, 0.0, 0.5));
        // Starts after the one-second lifetime.
        assert!(!ripple_warp_active(&state, 2.1, 2.5));

        let mut circle = warp_state(64, 48, vec![click]);
        circle.cursor.click_effect = ClickEffect::Circle;
        assert!(!ripple_warp_active(&circle, 0.5, 1.5));
    }

    #[test]
    fn identity_maps_when_no_ripple_is_visible() {
        // A click outside the window still produces a frame, and that frame
        // must read its own pixel so the common path is untouched.
        let state = warp_state(
            8,
            4,
            vec![ClickSample {
                t: 0.0,
                x: 4.0,
                y: 2.0,
                button: 1,
            }],
        );
        let dir = temp_dir("identity");
        let x_path = dir.join("x.raw");
        let y_path = dir.join("y.raw");
        // Frame 30 is one second in, past the ripple's lifetime.
        write_ripple_maps(&state, 1.0, 1.1, 8, 4, &x_path, &y_path).unwrap();
        let x = std::fs::read(&x_path).unwrap();
        let y = std::fs::read(&y_path).unwrap();
        for py in 0..4u16 {
            for px in 0..8u16 {
                let i = (py as usize * 8 + px as usize) * 2;
                assert_eq!(u16::from_le_bytes([x[i], x[i + 1]]), px);
                assert_eq!(u16::from_le_bytes([y[i], y[i + 1]]), py);
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_three_most_recent_clicks_displace_the_footage() {
        // The studied renderer keeps the last three clicks inside the visible
        // window; the warp consumes exactly that list.
        let mut state = warp_state(
            64,
            48,
            (0..4)
                .map(|i| ClickSample {
                    t: i as f64 * 0.01,
                    x: (i + 1) as f64,
                    y: 10.0,
                    button: 1,
                })
                .collect(),
        );
        let sidecar = state.sidecar.take().unwrap();
        let ripples = sidecar.click_ripples_at(0.1, 1.0);
        assert_eq!(ripples.len(), 3);
        assert!(
            !ripples.iter().any(|(x, _, _)| *x == 1.0),
            "the oldest click must be dropped: {ripples:?}"
        );
    }

    #[test]
    fn the_rendered_map_moves_a_known_fixture_pixel_by_the_expected_vector() {
        if !ffmpeg_available() {
            eprintln!("skipping: ffmpeg is not available");
            return;
        }
        // 640x480 source, click in the middle, sample a frame 120 ms in so the
        // ring has grown and the pull is well clear of zero.
        let width = 640u32;
        let height = 480u32;
        let click = ClickSample {
            t: 0.0,
            x: 320.0,
            y: 240.0,
            button: 1,
        };
        let state = warp_state(width, height, vec![click]);
        let dir = temp_dir("render");
        let x_path = dir.join("x.raw");
        let y_path = dir.join("y.raw");
        // 0.12 s into the effect is frame 3 at 30 fps (0.1 s); the map for the
        // first frame is enough for the fixture.
        write_ripple_maps(&state, 0.1, 0.2, width, height, &x_path, &y_path).unwrap();
        let x_bytes = std::fs::read(&x_path).unwrap();
        let y_bytes = std::fs::read(&y_path).unwrap();

        // Pick the destination pixel the map moves furthest, and place the
        // white fixture at the source that pixel reads. The remap must then
        // land the white pixel on that destination.
        let mut chosen: Option<(u32, u32, u32, u32, f64)> = None;
        for py in 0..height {
            for px in 0..width {
                let i = (py as usize * width as usize + px as usize) * 2;
                let mx = u16::from_le_bytes([x_bytes[i], x_bytes[i + 1]]) as u32;
                let my = u16::from_le_bytes([y_bytes[i], y_bytes[i + 1]]) as u32;
                let dx = px as f64 - mx as f64;
                let dy = py as f64 - my as f64;
                let magnitude = dx * dx + dy * dy;
                if chosen.map(|c| magnitude > c.4).unwrap_or(true) {
                    chosen = Some((px, py, mx, my, magnitude));
                }
            }
        }
        let (destination_x, destination_y, source_x, source_y, magnitude) =
            chosen.expect("the map must contain at least one pixel");
        assert!(
            magnitude >= 1.0,
            "the chosen displacement must be at least one pixel: {magnitude}"
        );

        // Build a yuv420p source with a white luma pixel at the fixture.
        let mut source = vec![16u8; (width * height) as usize];
        source[(source_y * width + source_x) as usize] = 235;
        source.resize((width * height * 3 / 2) as usize, 128);
        let source_path = dir.join("source.yuv");
        std::fs::write(&source_path, &source).unwrap();

        let output_path = dir.join("output.yuv");
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "yuv420p",
                "-s",
                &format!("{width}x{height}"),
                "-i",
                source_path.to_str().unwrap(),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "gray16le",
                "-s",
                &format!("{width}x{height}"),
                "-i",
                x_path.to_str().unwrap(),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "gray16le",
                "-s",
                &format!("{width}x{height}"),
                "-i",
                y_path.to_str().unwrap(),
                "-filter_complex",
                "[0:v][1:v][2:v]remap,format=yuv420p",
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "yuv420p",
                output_path.to_str().unwrap(),
            ])
            .status()
            .expect("ffmpeg must run");
        assert!(status.success(), "ffmpeg remap must succeed");

        let rendered = std::fs::read(&output_path).unwrap();
        let mut bright = std::collections::HashSet::new();
        for py in 0..height {
            for px in 0..width {
                let value = rendered[(py * width + px) as usize];
                if value > 200 {
                    bright.insert((px, py));
                }
            }
        }
        assert!(
            bright.contains(&(destination_x, destination_y)),
            "the fixture pixel must land at {destination_x},{destination_y}: {bright:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
