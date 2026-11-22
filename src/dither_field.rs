//! The ordered-dither accent field shared by the Studio surfaces.

use gtk4::{glib, prelude::*, DrawingArea};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const DITHER_CELL: f64 = 6.0;
const DITHER_GAP: f64 = 2.0;
const DITHER_BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
const ACCENT_DARK: (u8, u8, u8) = (0xff, 0x67, 0x0d);
const ACCENT_LIGHT: (u8, u8, u8) = (0xd1, 0x5b, 0x12);

/// The scale the field is authored against. GDK already reports a scaled
/// display on Linux, so only Windows needs the conversion.
fn ui_scale() -> f64 {
    #[cfg(target_os = "windows")]
    {
        crate::platform::ui_scale()
    }
    #[cfg(not(target_os = "windows"))]
    {
        1.0
    }
}

/// Logical size to device pixels.
fn px(value: f64) -> i32 {
    (value * ui_scale()).round() as i32
}

/// The dot field. Nothing else is painted, so the surface behind it shows
/// through the gaps and around the dissolving edge.
pub(crate) fn dither_field(dark: bool, height: f64) -> DrawingArea {
    let field = DrawingArea::new();
    field.set_hexpand(true);
    field.set_size_request(-1, px(height));
    field.set_can_target(false);

    let elapsed = Rc::new(Cell::new(0.0));
    field.set_draw_func({
        let elapsed = elapsed.clone();
        move |_area, context, width, height| {
            draw_dither(
                context,
                f64::from(width),
                f64::from(height),
                elapsed.get(),
                dark,
            );
        }
    });

    if animations_enabled() {
        let tick = field.add_tick_callback({
            let elapsed = elapsed.clone();
            let started = Cell::new(None::<i64>);
            let last_draw = Cell::new(f64::NEG_INFINITY);
            move |area, clock| {
                let now = clock.frame_time();
                let start = started.get().unwrap_or(now);
                started.set(Some(start));
                elapsed.set((now - start).max(0) as f64 / 1_000_000.0);
                let seconds = now as f64 / 1_000_000.0;
                // The shimmer reads the same at 30 fps as at the display's
                // refresh rate, so skip the frames in between.
                if seconds - last_draw.get() >= 1.0 / 30.0 {
                    last_draw.set(seconds);
                    area.queue_draw();
                }
                glib::ControlFlow::Continue
            }
        });
        let tick = Rc::new(RefCell::new(Some(tick)));
        field.connect_unrealize(move |_| {
            if let Some(tick) = tick.borrow_mut().take() {
                tick.remove();
            }
        });
    }
    field
}

fn animations_enabled() -> bool {
    gtk4::Settings::default().is_none_or(|settings| settings.is_gtk_enable_animations())
}

fn draw_dither(context: &gtk4::cairo::Context, width: f64, height: f64, t: f64, dark: bool) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let cell = (DITHER_CELL * ui_scale()).max(DITHER_GAP + 1.0);
    let dot = (cell - DITHER_GAP).round().max(1.0);
    let (red, green, blue) = if dark { ACCENT_DARK } else { ACCENT_LIGHT };
    let (red, green, blue) = (
        f64::from(red) / 255.0,
        f64::from(green) / 255.0,
        f64::from(blue) / 255.0,
    );
    let columns = (width / cell).ceil() as usize;
    let rows = (height / cell).ceil() as usize;
    for row in 0..rows {
        for column in 0..columns {
            let x = (column as f64 + 0.5) * cell;
            let y = (row as f64 + 0.5) * cell;
            let v = dither_intensity(x, y, width, height, t);
            if dither_lit(column, row, v) {
                context.set_source_rgba(red, green, blue, 0.35 + 0.65 * v);
                context.rectangle(
                    (column as f64 * cell).round(),
                    (row as f64 * cell).round(),
                    dot,
                    dot,
                );
                context.fill();
            }
        }
    }
}

/// The field's brightness at one cell centre: strongest along the top, with
/// a bloom toward the upper right and a slow diagonal shimmer.
fn dither_intensity(x: f64, y: f64, width: f64, height: f64, t: f64) -> f64 {
    if width <= 0.0 || height <= 0.0 {
        return 0.0;
    }
    let fall = (1.0 - y / height).clamp(0.0, 1.0).powf(2.3);
    let dx = x - 0.72 * width;
    let distance = (dx * dx + y * y).sqrt();
    let glow = 0.55 + 0.45 * (1.0 - distance / (0.9 * width)).clamp(0.0, 1.0);
    let shimmer = 0.16 * (std::f64::consts::TAU * ((x + y) / 260.0 - t * 0.12)).sin();
    (fall * glow + shimmer * fall).clamp(0.0, 1.0)
}

/// Ordered dithering: the cell lights when its intensity clears the 4x4
/// Bayer threshold, so mid-tones arrive as a stable dot pattern.
fn dither_lit(column: usize, row: usize, v: f64) -> bool {
    let bayer = f64::from(DITHER_BAYER[row % 4][column % 4]);
    v > (bayer + 0.5) / 16.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dither_field_is_gone_at_the_bottom() {
        for t in [0.0, 1.7, 9.3] {
            for x in [0.0, 300.0, 599.0] {
                assert_eq!(dither_intensity(x, 190.0, 600.0, 190.0, t), 0.0);
            }
        }
    }

    #[test]
    fn the_dither_field_is_brighter_toward_the_top_right() {
        let top_right = dither_intensity(0.72 * 600.0, 0.0, 600.0, 190.0, 0.0);
        let top_left = dither_intensity(0.0, 0.0, 600.0, 190.0, 0.0);
        let bottom_left = dither_intensity(0.0, 190.0, 600.0, 190.0, 0.0);
        assert!(top_right > top_left);
        assert!(top_right > bottom_left);
        assert!(top_right > 0.5);
    }

    #[test]
    fn extreme_intensities_light_every_cell_or_none() {
        for row in 0..4 {
            for column in 0..4 {
                assert!(dither_lit(column, row, 1.0));
                assert!(!dither_lit(column, row, 0.0));
            }
        }
    }

    #[test]
    fn the_bayer_grid_lights_half_its_cells_at_mid_intensity() {
        let lit = (0..4)
            .flat_map(|row| (0..4).map(move |column| (column, row)))
            .filter(|(column, row)| dither_lit(*column, *row, 0.5))
            .count();
        assert_eq!(lit, 8);
    }
}
