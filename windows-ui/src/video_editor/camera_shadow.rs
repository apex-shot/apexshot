//! The camera's studied drop shadow.
//!
//! The studied build lights the camera from a point far up-left of the stage
//! and marches a soft shadow toward it, so the shadow falls down-right of the
//! camera, darkest against its edge and gone 150 stage units out. A cut-out
//! person casts the outline of their matte instead of the camera's rect, and a
//! partly cut-out camera blends the two. Lengths are in stage units, 1080 to
//! the stage's height, and the shadow darkens in linear light, which this
//! file approximates in squared light the way the motion blur trail does.
//!
//! The mask is computed at one texel per stage unit (never finer than a
//! pixel) and kept per camera size and light offset, so a still camera
//! reuses it every frame.

use super::webcam::Rect;
use gtk4::cairo::{Context, Format, ImageSurface};
use std::cell::RefCell;
use std::rc::Rc;

/// The studied default shadow: intensity, distance (stage units), angle
/// (degrees, clockwise from +x with y down) and blur.
const INTENSITY: f64 = 0.3;
const DISTANCE: f64 = 25.0;
const ANGLE_DEGREES: f64 = 45.0;
const BLUR: f64 = 20.0;
/// The studied resolve multiplies the blur by this before it reaches the light.
const RESOLVED_BLUR_SCALE: f64 = 8.0;
/// The studied stage is this many units tall.
const STAGE_HEIGHT_UNITS: f64 = 1080.0;
/// The light sits this many half-diagonals of the stage from its centre.
const LIGHT_DISTANCE_SCALE: f64 = 1.5;
/// The light's radius per unit of resolved blur, and its smallest radius.
const LIGHT_RADIUS_SCALE: f64 = 2.5;
const MIN_LIGHT_RADIUS: f64 = 4.0;
/// How far the shadow reaches per unit of offset length, and at least.
const FALLOFF_PER_OFFSET: f64 = 6.0;
const MIN_FALLOFF: f64 = 40.0;
/// The rect shadow's march toward the light: steps, first step, least step.
const MARCH_STEPS: usize = 12;
const MARCH_START: f64 = 2.0;
const MARCH_MIN_STEP: f64 = 3.0;
/// The silhouette shadow's samples toward the light, and the blur that
/// fills the gap between two samples.
const CONTENT_STEPS: usize = 6;
const CONTENT_GAP_BLUR: f64 = 1.5;

/// The studied offset length in stage units.
fn offset_length() -> f64 {
    DISTANCE
}

fn falloff_units() -> f64 {
    (offset_length() * FALLOFF_PER_OFFSET).max(MIN_FALLOFF)
}

fn light_radius_units() -> f64 {
    (BLUR * RESOLVED_BLUR_SCALE * LIGHT_RADIUS_SCALE).max(MIN_LIGHT_RADIUS)
}

/// One camera's shadow, on a grid of texels around the camera rect.
#[derive(Debug)]
pub(super) struct ShadowMask {
    /// Where the grid's top-left corner sits relative to the camera rect's
    /// top-left, in output pixels (both negative: the grid has a margin).
    pub offset_x: f64,
    pub offset_y: f64,
    /// Output pixels per texel.
    pub texel: f64,
    pub width: usize,
    pub height: usize,
    /// The shadow's opacity per texel, intensity included, before the camera
    /// covers it.
    pub cast: Vec<f32>,
    /// How much of each texel the camera itself covers, 0..=1.
    pub coverage: Vec<f32>,
}

impl ShadowMask {
    /// The cast opacity at (`x`, `y`) output pixels from the camera rect's
    /// top-left, bilinear between texel centres, 0 off the grid.
    pub fn cast_at(&self, x: f64, y: f64) -> f32 {
        let gx = (x - self.offset_x) / self.texel - 0.5;
        let gy = (y - self.offset_y) / self.texel - 0.5;
        sample_grid(&self.cast, self.width, self.height, gx, gy) as f32
    }
}

/// Signed distance in pixels from `p` (relative to the shape's centre) to a
/// camera shape of half size `half` and corner `radius`, drawn the way
/// `webcam::add_shape_path` draws it: a full radius is an ellipse.
fn shape_distance(p: (f64, f64), half: (f64, f64), radius: f64) -> f64 {
    let shorter = half.0.min(half.1);
    let radius = radius.clamp(0.0, shorter);
    if shorter <= 0.0 {
        return f64::INFINITY;
    }
    if (radius - shorter).abs() < 0.5 {
        let k = ((p.0 / half.0).powi(2) + (p.1 / half.1).powi(2)).sqrt();
        return (k - 1.0) * shorter;
    }
    let qx = p.0.abs() - half.0 + radius;
    let qy = p.1.abs() - half.1 + radius;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

/// The studied smoothstep, which also runs with `edge0 > edge1`.
fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The studied directional shadow of the camera's own shape at `p`
/// (pixels from the camera's centre), 0..=1, without intensity.
fn rect_shadow(
    p: (f64, f64),
    half: (f64, f64),
    radius: f64,
    light: (f64, f64),
    light_radius: f64,
    falloff: f64,
    unit: f64,
) -> f64 {
    let d = shape_distance(p, half, radius);
    let proximity = 1.0 - (d / falloff.max(1.0)).clamp(0.0, 1.0);
    if proximity <= 0.001 {
        return 0.0;
    }
    let to = (light.0 - p.0, light.1 - p.1);
    let dist = (to.0 * to.0 + to.1 * to.1).sqrt().max(1.0);
    let dir = (to.0 / dist, to.1 / dist);
    let hardness = dist / light_radius.max(1.0);
    let mut visibility: f64 = 1.0;
    let mut t = MARCH_START * unit;
    for _ in 0..MARCH_STEPS {
        if t >= dist || visibility < -0.5 {
            break;
        }
        let clearance = shape_distance((p.0 + dir.0 * t, p.1 + dir.1 * t), half, radius);
        visibility = visibility.min(hardness * clearance / t);
        t += clearance.max(MARCH_MIN_STEP * unit);
    }
    let occlusion = 1.0 - visibility.clamp(0.0, 1.0);
    occlusion * proximity * proximity
}

/// `index` clamped to `0..len` as a texel index.
fn clamp_index(index: isize, len: usize) -> usize {
    index.clamp(0, len as isize - 1) as usize
}

/// Blur `grid` (`width` x `height`) by two passes of a box of `radius`
/// texels each way, which is a tent of about twice the radius.
fn tent_blur(grid: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    if radius == 0 {
        return grid.to_vec();
    }
    let inv = 1.0 / (2 * radius + 1) as f32;
    let mut current = grid.to_vec();
    for _ in 0..2 {
        let mut horizontal = vec![0.0f32; current.len()];
        for j in 0..height {
            let row = &current[j * width..(j + 1) * width];
            let mut sum = 0.0f32;
            for k in 0..=2 * radius {
                sum += row[clamp_index(k as isize - radius as isize, width)];
            }
            for i in 0..width {
                horizontal[j * width + i] = sum * inv;
                sum -= row[clamp_index(i as isize - radius as isize, width)];
                sum += row[clamp_index(i as isize + radius as isize + 1, width)];
            }
        }
        let mut vertical = vec![0.0f32; current.len()];
        for i in 0..width {
            let mut sum = 0.0f32;
            for k in 0..=2 * radius {
                sum += horizontal[clamp_index(k as isize - radius as isize, height) * width + i];
            }
            for j in 0..height {
                vertical[j * width + i] = sum * inv;
                sum -= horizontal[clamp_index(j as isize - radius as isize, height) * width + i];
                sum +=
                    horizontal[clamp_index(j as isize + radius as isize + 1, height) * width + i];
            }
        }
        current = vertical;
    }
    current
}

/// Bilinear `grid` sample at grid coordinates (`gx`, `gy`), 0 off the grid.
fn sample_grid(grid: &[f32], width: usize, height: usize, gx: f64, gy: f64) -> f64 {
    if gx < -0.5 || gy < -0.5 || gx > width as f64 - 0.5 || gy > height as f64 - 0.5 {
        return 0.0;
    }
    let gx = gx.clamp(0.0, (width - 1) as f64);
    let gy = gy.clamp(0.0, (height - 1) as f64);
    let x0 = gx.floor() as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y0 = gy.floor() as usize;
    let y1 = (y0 + 1).min(height - 1);
    let fx = gx - x0 as f64;
    let fy = gy - y0 as f64;
    let top = grid[y0 * width + x0] as f64 * (1.0 - fx) + grid[y0 * width + x1] as f64 * fx;
    let bottom = grid[y1 * width + x0] as f64 * (1.0 - fx) + grid[y1 * width + x1] as f64 * fx;
    top * (1.0 - fy) + bottom * fy
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShadowKey {
    width: i64,
    height: i64,
    radius: i64,
    texel: i64,
    light_x: i64,
    light_y: i64,
    /// The camera frame a cut-out silhouette was drawn from, 0 for the rect.
    frame: u64,
    /// The cutout blend in thousandths, so the same frame at another blend
    /// gets its own mask.
    cutout: i64,
    /// Whether that frame was mirrored when its coverage was drawn.
    mirror: bool,
}

/// How many masks to keep: the preview's motion blur trail draws up to five
/// samples, and the still frame needs one more.
const MASK_CACHE_ENTRIES: usize = 6;

thread_local! {
    static MASK_CACHE: RefCell<Vec<(ShadowKey, Rc<ShadowMask>)>> = const { RefCell::new(Vec::new()) };
}

/// The camera's shadow on a stage, or None when there is none to draw.
///
/// `stage` and `camera` are in output pixels in the same space; `radius` is
/// the camera's corner radius in pixels. `cutout` (0..=1) blends the camera's
/// own shape into the silhouette that `draw` paints: `draw(context, rect,
/// radius)` must paint the camera, with its alpha, into `rect` (pixels of the
/// context) cut to `radius`. It is called only when `cutout > 0.001`.
pub(super) fn camera_shadow(
    stage: Rect,
    camera: Rect,
    radius: f64,
    cutout: f64,
    draw: &dyn Fn(&Context, Rect, f64),
) -> Option<Rc<ShadowMask>> {
    camera_shadow_keyed(stage, camera, radius, cutout, None, false, draw)
}

/// [`camera_shadow`] with the camera frame in its cache key. The export passes
/// no frame and rebuilds its silhouette every frame; the preview passes the
/// decoded frame's stamp and mirror so a still camera reuses its mask.
pub(super) fn camera_shadow_keyed(
    stage: Rect,
    camera: Rect,
    radius: f64,
    cutout: f64,
    frame: Option<u64>,
    mirror: bool,
    draw: &dyn Fn(&Context, Rect, f64),
) -> Option<Rc<ShadowMask>> {
    if !stage.width.is_finite()
        || !stage.height.is_finite()
        || !camera.width.is_finite()
        || !camera.height.is_finite()
        || stage.width < 1.0
        || stage.height < 1.0
        || camera.width < 1.0
        || camera.height < 1.0
    {
        return None;
    }
    if camera.x <= stage.x + 0.5
        && camera.y <= stage.y + 0.5
        && camera.x + camera.width >= stage.x + stage.width - 0.5
        && camera.y + camera.height >= stage.y + stage.height - 0.5
    {
        return None;
    }
    let cutout = if cutout.is_finite() {
        cutout.clamp(0.0, 1.0)
    } else {
        0.0
    };

    let unit = stage.height / STAGE_HEIGHT_UNITS;
    let texel = unit.max(1.0);
    let falloff = falloff_units() * unit;
    let light_radius = light_radius_units() * unit;

    let angle = ANGLE_DEGREES.to_radians();
    let reach = (stage.width / 2.0).hypot(stage.height / 2.0) * LIGHT_DISTANCE_SCALE;
    let light_x = stage.x + stage.width / 2.0 - angle.cos() * reach;
    let light_y = stage.y + stage.height / 2.0 - angle.sin() * reach;
    let center = (
        camera.x + camera.width / 2.0,
        camera.y + camera.height / 2.0,
    );
    // Quantised to 8 stage units so a moving camera still reuses its mask; the
    // light is over a thousand units away, so the shadow cannot tell.
    let step = 8.0 * unit;
    let light = (
        ((light_x - center.0) / step).round() * step,
        ((light_y - center.1) / step).round() * step,
    );

    let identity = match frame {
        Some(stamp) if cutout > 0.001 => Some((stamp, (cutout * 1000.0).round() as i64)),
        _ if cutout <= 0.001 => Some((0, 0)),
        _ => None,
    };
    let use_cache = identity.is_some();
    let key = ShadowKey {
        width: (camera.width * 8.0).round() as i64,
        height: (camera.height * 8.0).round() as i64,
        radius: (radius * 8.0).round() as i64,
        texel: (texel * 8.0).round() as i64,
        light_x: (light.0 * 8.0).round() as i64,
        light_y: (light.1 * 8.0).round() as i64,
        frame: identity.map(|(stamp, _)| stamp).unwrap_or(0),
        cutout: identity.map(|(_, cutout)| cutout).unwrap_or(0),
        mirror: identity.is_some() && mirror,
    };
    if use_cache {
        let cached = MASK_CACHE.with(|cache| {
            cache
                .borrow()
                .iter()
                .find(|(cached_key, _)| *cached_key == key)
                .map(|(_, mask)| Rc::clone(mask))
        });
        if let Some(mask) = cached {
            return Some(mask);
        }
    }

    let margin = (falloff / texel).ceil() as usize + 2;
    let width = (camera.width / texel).ceil() as usize + 2 * margin;
    let height = (camera.height / texel).ceil() as usize + 2 * margin;
    let offset_x = -(margin as f64) * texel;
    let offset_y = offset_x;
    let half = (camera.width / 2.0, camera.height / 2.0);
    let texel_center = |i: usize, j: usize| {
        (
            offset_x + (i as f64 + 0.5) * texel - camera.width / 2.0,
            offset_y + (j as f64 + 0.5) * texel - camera.height / 2.0,
        )
    };

    let mut coverage = vec![0.0f32; width * height];
    if cutout <= 0.001 {
        for j in 0..height {
            for i in 0..width {
                let d = shape_distance(texel_center(i, j), half, radius) / texel;
                coverage[j * width + i] = (0.5 - d).clamp(0.0, 1.0) as f32;
            }
        }
    } else {
        let mut surface = ImageSurface::create(Format::ARgb32, width as i32, height as i32).ok()?;
        {
            let context = Context::new(&surface).ok()?;
            context.scale(1.0 / texel, 1.0 / texel);
            draw(
                &context,
                Rect {
                    x: -offset_x,
                    y: -offset_y,
                    width: camera.width,
                    height: camera.height,
                },
                radius,
            );
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().ok()?;
        for j in 0..height {
            for i in 0..width {
                coverage[j * width + i] = data[j * stride + i * 4 + 3] as f32 / 255.0;
            }
        }
    }

    let mut rect = vec![0.0f32; width * height];
    if cutout < 0.999 {
        for j in 0..height {
            for i in 0..width {
                rect[j * width + i] = rect_shadow(
                    texel_center(i, j),
                    half,
                    radius,
                    light,
                    light_radius,
                    falloff,
                    unit,
                ) as f32;
            }
        }
    }

    let mut content = vec![0.0f32; width * height];
    if cutout > 0.001 {
        let dist_to_light = (light.0 * light.0 + light.1 * light.1).sqrt().max(1.0);
        let spread = light_radius / dist_to_light;
        let mut previous = 0.0;
        for step in 0..CONTENT_STEPS {
            let along = 0.1 + 0.75 * step as f64 / (CONTENT_STEPS as f64 - 1.0);
            let distance = falloff * along * along;
            let blur = (distance * spread)
                .max((distance - previous) * CONTENT_GAP_BLUR)
                .max(0.5 * texel);
            previous = distance;
            let blurred = tent_blur(
                &coverage,
                width,
                height,
                ((blur / texel) / 2.0).round() as usize,
            );
            let proximity = 1.0 - along * along;
            for j in 0..height {
                for i in 0..width {
                    let p = texel_center(i, j);
                    if shape_distance(p, half, radius) > falloff {
                        continue;
                    }
                    let to = (light.0 - p.0, light.1 - p.1);
                    let len = (to.0 * to.0 + to.1 * to.1).sqrt().max(1.0);
                    let q = (p.0 + to.0 / len * distance, p.1 + to.1 / len * distance);
                    let clip = smoothstep(blur, -blur, shape_distance(q, half, radius));
                    if clip <= 0.0 {
                        continue;
                    }
                    let gx = (q.0 + camera.width / 2.0 - offset_x) / texel - 0.5;
                    let gy = (q.1 + camera.height / 2.0 - offset_y) / texel - 0.5;
                    let sample = sample_grid(&blurred, width, height, gx, gy);
                    let value = sample * clip * proximity * proximity;
                    let index = j * width + i;
                    content[index] = content[index].max(value as f32);
                }
            }
        }
    }

    let mut cast = vec![0.0f32; width * height];
    for (index, cast) in cast.iter_mut().enumerate() {
        let value = rect[index] as f64 * (1.0 - cutout) + content[index] as f64 * cutout;
        *cast = (INTENSITY * value) as f32;
    }
    let mask = Rc::new(ShadowMask {
        offset_x,
        offset_y,
        texel,
        width,
        height,
        cast,
        coverage,
    });
    if use_cache {
        MASK_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if cache.len() >= MASK_CACHE_ENTRIES {
                cache.remove(0);
            }
            cache.push((key, Rc::clone(&mask)));
        });
    }
    Some(mask)
}

/// Darken an RGBA `pixels` frame of `size` under `mask`, whose camera sits at
/// `camera` (pixels). `alpha` is the camera layer's opacity and
/// `covered(x, y)` how much of output pixel (x, y) the camera itself paints,
/// 0..=1, so the shadow only shows where the camera is not. The shadow is
/// black, darkened in squared light like the motion blur trail.
pub(super) fn darken_rgba(
    pixels: &mut [u8],
    size: (u32, u32),
    camera: Rect,
    mask: &ShadowMask,
    alpha: f64,
    covered: &dyn Fn(u32, u32) -> f64,
) {
    if !alpha.is_finite() || alpha <= 0.0 {
        return;
    }
    if pixels.len() < size.0 as usize * size.1 as usize * 4 {
        return;
    }
    let left = (camera.x + mask.offset_x).floor().max(0.0) as u32;
    let top = (camera.y + mask.offset_y).floor().max(0.0) as u32;
    let right = (camera.x + mask.offset_x + mask.width as f64 * mask.texel)
        .ceil()
        .clamp(0.0, size.0 as f64) as u32;
    let bottom = (camera.y + mask.offset_y + mask.height as f64 * mask.texel)
        .ceil()
        .clamp(0.0, size.1 as f64) as u32;
    let width = size.0 as usize;
    for y in top..bottom {
        for x in left..right {
            let m = mask.cast_at(x as f64 + 0.5 - camera.x, y as f64 + 0.5 - camera.y) as f64;
            if m <= 0.0 {
                continue;
            }
            let a =
                (alpha.clamp(0.0, 1.0) * m * (1.0 - covered(x, y).clamp(0.0, 1.0))).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let factor = (1.0 - a).sqrt();
            let index = (y as usize * width + x as usize) * 4;
            for channel in 0..3 {
                pixels[index + channel] = (pixels[index + channel] as f64 * factor).round() as u8;
            }
        }
    }
}

/// The mask as a premultiplied black ARGB32 surface for cairo, one pixel per
/// texel, with the camera layer's `alpha` baked in. Cairo blends in sRGB, so
/// each alpha is taken from squared light the way `darken_rgba` darkens:
/// `1 - sqrt(1 - alpha * opacity)`. Painting the surface at full opacity then
/// leaves `sqrt(1 - alpha * opacity)`, the export's curve.
pub(super) fn preview_surface(mask: &ShadowMask, alpha: f64) -> Option<ImageSurface> {
    let alpha = if alpha.is_finite() {
        alpha.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut surface =
        ImageSurface::create(Format::ARgb32, mask.width as i32, mask.height as i32).ok()?;
    let stride = surface.stride() as usize;
    {
        let mut data = surface.data().ok()?;
        for j in 0..mask.height {
            for i in 0..mask.width {
                let index = j * mask.width + i;
                let o = (mask.cast[index] as f64 * (1.0 - mask.coverage[index] as f64)).max(0.0);
                let a = 1.0 - (1.0 - alpha * o).max(0.0).sqrt();
                let offset = j * stride + i * 4;
                data[offset] = 0;
                data[offset + 1] = 0;
                data[offset + 2] = 0;
                data[offset + 3] = (a * 255.0).round() as u8;
            }
        }
    }
    surface.mark_dirty();
    Some(surface)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_1_SQRT_2;

    fn stage(width: f64, height: f64) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    }

    fn no_draw(_: &Context, _: Rect, _: f64) {}

    fn camera() -> Rect {
        Rect {
            x: 810.0,
            y: 390.0,
            width: 300.0,
            height: 300.0,
        }
    }

    fn down_right(d: f64) -> (f64, f64) {
        let along = 150.0 + (150.0 + d) * FRAC_1_SQRT_2;
        (along, along)
    }

    fn up_left(d: f64) -> (f64, f64) {
        let along = 150.0 - (150.0 + d) * FRAC_1_SQRT_2;
        (along, along)
    }

    fn test_mask() -> Rc<ShadowMask> {
        camera_shadow(stage(1920.0, 1080.0), camera(), 150.0, 0.0, &no_draw).unwrap()
    }

    #[test]
    fn the_shadow_falls_down_right_for_the_studied_angle() {
        let mask = test_mask();
        let lit = mask.cast_at(down_right(20.0).0, down_right(20.0).1);
        let dark = mask.cast_at(up_left(20.0).0, up_left(20.0).1);
        assert!(lit > 0.12, "down-right cast {lit}");
        assert!(dark < 0.01, "up-left cast {dark}");
    }

    #[test]
    fn the_shadow_fades_out_by_the_falloff() {
        let mask = test_mask();
        let near = mask.cast_at(down_right(10.0).0, down_right(10.0).1);
        let middle = mask.cast_at(down_right(60.0).0, down_right(60.0).1);
        let far = mask.cast_at(down_right(120.0).0, down_right(120.0).1);
        let beyond = mask.cast_at(down_right(160.0).0, down_right(160.0).1);
        assert!(near > middle, "near {near} middle {middle}");
        assert!(middle > far, "middle {middle} far {far}");
        assert_eq!(beyond, 0.0, "beyond {beyond}");
    }

    #[test]
    fn the_shadow_scales_with_the_stage() {
        let small = test_mask();
        let big_camera = Rect {
            x: 1620.0,
            y: 780.0,
            width: 600.0,
            height: 600.0,
        };
        let big = camera_shadow(stage(3840.0, 2160.0), big_camera, 300.0, 0.0, &no_draw).unwrap();
        assert_eq!(big.texel, 2.0);
        let point = down_right(20.0);
        let small_value = small.cast_at(point.0, point.1);
        let big_value = big.cast_at(point.0 * 2.0, point.1 * 2.0);
        assert!(
            (big_value - small_value).abs() <= 0.03,
            "small {small_value} big {big_value}"
        );
    }

    #[test]
    fn a_still_camera_reuses_its_mask() {
        let first = test_mask();
        let second = test_mask();
        assert!(Rc::ptr_eq(&first, &second));
        let moved = Rect {
            x: 811.0,
            ..camera()
        };
        let third = camera_shadow(stage(1920.0, 1080.0), moved, 150.0, 0.0, &no_draw).unwrap();
        assert!(Rc::ptr_eq(&first, &third));
        let resized = Rect {
            width: 310.0,
            ..camera()
        };
        let fourth = camera_shadow(stage(1920.0, 1080.0), resized, 150.0, 0.0, &no_draw).unwrap();
        assert!(!Rc::ptr_eq(&first, &fourth));
    }

    #[test]
    fn the_cutout_casts_the_silhouette_not_the_rect() {
        let draw = |context: &Context, rect: Rect, _radius: f64| {
            context.arc(
                rect.x + rect.width * 0.25,
                rect.y + rect.height * 0.75,
                rect.width * 0.2,
                0.0,
                std::f64::consts::TAU,
            );
            context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            let _ = context.fill();
        };
        let rect_mask = camera_shadow(stage(1920.0, 1080.0), camera(), 0.0, 0.0, &no_draw).unwrap();
        let corner = rect_mask.cast_at(300.0 + 20.0, 20.0);
        assert!(corner > 0.1, "rect corner cast {corner}");
        let cut_mask = camera_shadow(stage(1920.0, 1080.0), camera(), 0.0, 1.0, &draw).unwrap();
        let empty = cut_mask.cast_at(300.0 + 20.0, 20.0);
        assert!(empty < 0.01, "cut corner cast {empty}");
        let silhouette = (
            75.0 + 60.0 * FRAC_1_SQRT_2 + 15.0,
            225.0 + 60.0 * FRAC_1_SQRT_2 + 15.0,
        );
        let cast = cut_mask.cast_at(silhouette.0, silhouette.1);
        assert!(cast > 0.05, "silhouette cast {cast}");
    }

    #[test]
    fn darkening_leaves_the_lit_side_and_the_camera_alone() {
        let mask = test_mask();
        let mut pixels = vec![200u8; 1920 * 1080 * 4];
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        let covered = |x: u32, y: u32| {
            let dx = x as f64 + 0.5 - 960.0;
            let dy = y as f64 + 0.5 - 540.0;
            if (dx * dx + dy * dy).sqrt() <= 150.0 {
                1.0
            } else {
                0.0
            }
        };
        darken_rgba(&mut pixels, (1920, 1080), camera(), &mask, 1.0, &covered);
        let lit_point = down_right(20.0);
        let lit_x = (camera().x + lit_point.0) as u32;
        let lit_y = (camera().y + lit_point.1) as u32;
        let lit_index = (lit_y as usize * 1920 + lit_x as usize) * 4;
        assert!(pixels[lit_index] < 190, "lit pixel {}", pixels[lit_index]);
        assert_eq!(pixels[lit_index + 3], 255);
        let dark_point = up_left(20.0);
        let dark_x = (camera().x + dark_point.0) as u32;
        let dark_y = (camera().y + dark_point.1) as u32;
        let dark_index = (dark_y as usize * 1920 + dark_x as usize) * 4;
        assert_eq!(pixels[dark_index], 200);
        let centre_index = (540 * 1920 + 960) * 4;
        assert_eq!(pixels[centre_index], 200);
        assert_eq!(pixels[centre_index + 3], 255);

        let mut half = vec![200u8; 1920 * 1080 * 4];
        for pixel in half.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        darken_rgba(&mut half, (1920, 1080), camera(), &mask, 0.5, &covered);
        assert!(half[lit_index] > pixels[lit_index]);
    }

    #[test]
    fn the_preview_surface_matches_the_mask_size() {
        let mask = test_mask();
        let surface = preview_surface(&mask, 1.0).unwrap();
        assert_eq!(surface.width(), mask.width as i32);
        assert_eq!(surface.height(), mask.height as i32);
    }

    #[test]
    fn the_preview_surface_bakes_the_camera_alpha_into_the_shadow() {
        let mask = test_mask();
        let mut full = preview_surface(&mask, 1.0).unwrap();
        let mut half = preview_surface(&mask, 0.5).unwrap();
        let index = mask
            .cast
            .iter()
            .enumerate()
            .position(|(index, cast)| *cast > 0.1 && mask.coverage[index] < 0.1)
            .expect("a cast texel outside the camera");
        let offset = (index / mask.width) * full.stride() as usize + (index % mask.width) * 4 + 3;
        let read = |surface: &mut ImageSurface| {
            surface.flush();
            let data = surface.data().unwrap();
            data[offset] as f64 / 255.0
        };
        let o = f64::from(mask.cast[index]) * (1.0 - f64::from(mask.coverage[index]));
        let expected = 1.0 - (1.0 - 0.5 * o).sqrt();
        let half_alpha = read(&mut half);
        assert!(
            (half_alpha - expected).abs() < 0.01,
            "half {half_alpha} expected {expected}"
        );
        assert!(half_alpha < read(&mut full));
    }

    #[test]
    fn a_cut_out_mask_is_reused_for_the_same_frame_and_blend() {
        let draw = |context: &Context, rect: Rect, _radius: f64| {
            context.arc(
                rect.x + rect.width * 0.25,
                rect.y + rect.height * 0.75,
                rect.width * 0.2,
                0.0,
                std::f64::consts::TAU,
            );
            context.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            let _ = context.fill();
        };
        let first = camera_shadow_keyed(
            stage(1920.0, 1080.0),
            camera(),
            0.0,
            1.0,
            Some(11),
            false,
            &draw,
        )
        .unwrap();
        let again = camera_shadow_keyed(
            stage(1920.0, 1080.0),
            camera(),
            0.0,
            1.0,
            Some(11),
            false,
            &draw,
        )
        .unwrap();
        assert!(Rc::ptr_eq(&first, &again));
        let next_frame = camera_shadow_keyed(
            stage(1920.0, 1080.0),
            camera(),
            0.0,
            1.0,
            Some(12),
            false,
            &draw,
        )
        .unwrap();
        assert!(!Rc::ptr_eq(&first, &next_frame));
        let other_blend = camera_shadow_keyed(
            stage(1920.0, 1080.0),
            camera(),
            0.0,
            0.5,
            Some(11),
            false,
            &draw,
        )
        .unwrap();
        assert!(!Rc::ptr_eq(&first, &other_blend));
    }
}
