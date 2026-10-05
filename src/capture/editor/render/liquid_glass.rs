//! Liquid Glass frame rendering.
//!
//! A CPU port of the glass pipeline from <https://github.com/ybouane/liquidglass>
//! applied to the frame band around a captured card. The band is treated as a
//! curved slab of glass sitting on the composited backdrop:
//!
//! * A rounded-rect signed distance field gives the band its shape and the
//!   surface normal of the bevel.
//! * Refraction bends the backdrop along that bevel, so whatever sits behind
//!   the frame is pulled through the glass instead of just tinted.
//! * Chromatic aberration fringes red and blue along the same normal.
//! * Fresnel reflection, four Blinn-Phong rim lights and an inner stroke
//!   highlight give the glass its highlights, which is what carries the look
//!   when the backdrop is flat, dark, or transparent.

use image::RgbaImage;

use crate::capture::editor::types::{FrameStyle, Rect};

/// The card the glass wraps, in backdrop pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassRing {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub radius: f64,
    /// Glass thickness: the visible band plus its specular rim.
    pub gap: f64,
}

/// Look of the glass, mirroring the reference shader's uniforms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlassLook {
    /// Use a continuous clear body with a single outer reflection.
    pub clear_body: bool,
    /// How far samples bend, at most this fraction of the band.
    pub refraction: f64,
    /// Colour fringing, as a fraction of the band.
    pub chroma: f64,
    /// Backdrop blur, as a fraction of the band (0 keeps the backdrop sharp).
    pub blur: f64,
    /// Blinn-Phong rim highlight intensity.
    pub specular: f64,
    /// Fresnel reflection intensity.
    pub fresnel: f64,
    /// Broad reflection across the band, brightest along the lit top lip.
    pub reflection: f64,
    /// Inner stroke and rim glow intensity.
    pub edge_highlight: f64,
    /// Cool blue glass tint.
    pub tint: f64,
    pub brightness: f64,
    pub saturation: f64,
    /// Body shade for the tinted glass siblings: >0 deepens the refracted
    /// body toward black (Glass Dark), <0 lifts it toward white (Glass
    /// Light). The speculars and rim are untouched, so shaded glass keeps
    /// its bright edge light like dark-mode system glass.
    pub body_shade: f64,
}

impl Default for GlassLook {
    fn default() -> Self {
        Self {
            clear_body: true,
            refraction: 0.65,
            chroma: 0.025,
            blur: 0.04,
            specular: 0.20,
            fresnel: 1.0,
            reflection: 0.12,
            edge_highlight: 0.06,
            tint: 0.0,
            brightness: 0.16,
            saturation: 0.02,
            body_shade: 0.0,
        }
    }
}

impl GlassLook {
    /// Shared lens optics with the selected frame's light or smoked tint.
    pub fn for_style(style: FrameStyle) -> Self {
        Self {
            body_shade: match style {
                FrameStyle::GlassLight => -0.55,
                FrameStyle::GlassDark => 0.55,
                _ => 0.0,
            },
            ..Self::default()
        }
    }
}

/// Refracted and lit glass band, cropped to the pixels it actually covers.
pub struct GlassLayer {
    pub image: RgbaImage,
    pub x: i64,
    pub y: i64,
}

/// Signed distance and outward normal of the card's shared squircle outline.
fn rounded_rect_sdf(px: f64, py: f64, half_w: f64, half_h: f64, radius: f64) -> (f64, f64, f64) {
    let qx = px.abs() - half_w + radius;
    let qy = py.abs() - half_h + radius;
    let sign_x = if px < 0.0 { -1.0 } else { 1.0 };
    let sign_y = if py < 0.0 { -1.0 } else { 1.0 };
    if qx <= 0.0 || qy <= 0.0 {
        let mx = qx.max(0.0);
        let my = qy.max(0.0);
        let (normal_x, normal_y) = if mx > my {
            (sign_x, 0.0)
        } else if my > mx {
            (0.0, sign_y)
        } else {
            (0.0, 0.0)
        };
        return (qx.max(qy) - radius, normal_x, normal_y);
    }
    let (distance, normal_x, normal_y) = squircle_corner_sdf(qx, qy, radius);
    (distance, normal_x * sign_x, normal_y * sign_y)
}

/// Segments per rounded corner in [`crate::capture::editor::render::rounded_rect_path`].
const CORNER_SEGMENTS: usize = 16;

/// Signed distance to the card's corner polygon in the corner's own frame,
/// where `qx`/`qy` are the offsets from the corner centre along its two
/// outward axes (both positive here): the nearest point on the
/// quarter-superellipse segment chain, signed by the chain's outward normal.
fn squircle_corner_sdf(qx: f64, qy: f64, radius: f64) -> (f64, f64, f64) {
    if radius <= 0.0 {
        let length = qx.hypot(qy);
        return (length, qx / length, qy / length);
    }
    let mut best = f64::INFINITY;
    let mut closest = (0.0, 0.0);
    let mut edge_normal = (0.0, 0.0);
    let mut previous = (radius, 0.0);
    for step in 1..=CORNER_SEGMENTS {
        let angle = step as f64 / CORNER_SEGMENTS as f64 * std::f64::consts::FRAC_PI_2;
        let (sine, cosine) = angle.sin_cos();
        let point = (radius * cosine.abs().sqrt(), radius * sine.abs().sqrt());
        let edge = (point.0 - previous.0, point.1 - previous.1);
        let length_squared = edge.0 * edge.0 + edge.1 * edge.1;
        let (projected, normal) = if length_squared <= 1e-12 {
            (previous, (0.0, 0.0))
        } else {
            let along = (((qx - previous.0) * edge.0 + (qy - previous.1) * edge.1)
                / length_squared)
                .clamp(0.0, 1.0);
            let length = length_squared.sqrt();
            (
                (previous.0 + edge.0 * along, previous.1 + edge.1 * along),
                (edge.1 / length, -edge.0 / length),
            )
        };
        let offset = (qx - projected.0, qy - projected.1);
        let distance = (offset.0 * offset.0 + offset.1 * offset.1).sqrt();
        if distance < best {
            best = distance;
            closest = projected;
            edge_normal = normal;
        }
        previous = point;
    }
    let offset = (qx - closest.0, qy - closest.1);
    let sign = if offset.0 * edge_normal.0 + offset.1 * edge_normal.1 >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let normal = if best > 1e-9 {
        (sign * offset.0 / best, sign * offset.1 / best)
    } else {
        edge_normal
    };
    (sign * best, normal.0, normal.1)
}

/// Surface slope and inward displacement through a convex squircle bevel,
/// refracting a perpendicular ray from air into glass with index 1.5.
pub fn clear_lens(u: f64, gap: f64, strength: f64) -> (f64, f64) {
    if u <= 0.0 {
        return (0.0, 0.0);
    }
    if u >= 1.0 {
        return (1.0e4, 0.0);
    }
    let quartic = u * u * u * u;
    let under_dome = 1.0 - quartic;
    let height = under_dome.powf(0.25);
    let derivative = u * u * u / under_dome.powf(0.75).max(1e-6);
    let slope = 4.0 * derivative;
    let theta_i = slope.atan();
    let theta_t = (theta_i.sin() / 1.5).asin();
    let displacement = height * (4.0 * gap) * (theta_i - theta_t).tan() * strength;
    (slope, displacement)
}

/// Glass cross-section across the band, ported from the reference
/// `bevelHeight(d) = sqrt(d * (2*zR - d))` half-circle profile. The band is a
/// tiny glass tube: height is zero at both lips and peaks in the middle, so
/// `d` is the distance to the nearest lip and `zR` is half the band. Returns
/// dh/distance along the outward normal (+ on the inner half, - on the outer
/// half), clamped like the reference's 2px finite difference so the lips stay
/// bright without sampling halfway across the canvas.
fn lens_slope(u: f64, gap: f64) -> f64 {
    let gap = gap.max(1.0);
    let z_r = gap * 0.5;
    let dist = (u.clamp(0.0, 1.0) * gap).min(gap - u.clamp(0.0, 1.0) * gap);
    let d = dist.clamp(0.0, z_r);
    let denom = (d * (2.0 * z_r - d)).max(1e-6).sqrt();
    let mut slope = (z_r - d) / denom;
    if u < 0.5 {
        slope = slope.abs();
    } else {
        slope = -slope.abs();
    }
    slope.clamp(-4.0, 4.0)
}

fn smoothstep(edge0: f64, edge1: f64, value: f64) -> f64 {
    if (edge1 - edge0).abs() <= f64::EPSILON {
        return if value < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bilinear sample of the backdrop, clamped at the edges. Returns straight
/// (non-premultiplied) 0..1 RGBA.
fn sample(backdrop: &RgbaImage, x: f64, y: f64) -> [f64; 4] {
    let (width, height) = backdrop.dimensions();
    if width == 0 || height == 0 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let max_x = width as f64 - 1.0;
    let max_y = height as f64 - 1.0;
    let x = x.clamp(0.0, max_x);
    let y = y.clamp(0.0, max_y);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let fx = x - f64::from(x0);
    let fy = y - f64::from(y0);
    let p00 = backdrop.get_pixel(x0, y0);
    let p10 = backdrop.get_pixel(x1, y0);
    let p01 = backdrop.get_pixel(x0, y1);
    let p11 = backdrop.get_pixel(x1, y1);
    let mut out = [0.0; 4];
    for channel in 0..4 {
        let top = f64::from(p00[channel]) * (1.0 - fx) + f64::from(p10[channel]) * fx;
        let bottom = f64::from(p01[channel]) * (1.0 - fx) + f64::from(p11[channel]) * fx;
        out[channel] = (top * (1.0 - fy) + bottom * fy) / 255.0;
    }
    out
}

/// Render the glass band on top of `backdrop`.
///
/// The returned layer is transparent outside the band, so callers can overlay
/// it directly at [`GlassLayer::x`]/[`GlassLayer::y`].
pub fn glass_layer(backdrop: &RgbaImage, ring: &GlassRing, look: &GlassLook) -> Option<GlassLayer> {
    let (image_w, image_h) = backdrop.dimensions();
    if image_w == 0 || image_h == 0 {
        return None;
    }
    let gap = ring.gap;
    let half_w = ring.width * 0.5;
    let half_h = ring.height * 0.5;
    if gap <= 1.0 || half_w <= 0.5 || half_h <= 0.5 {
        return None;
    }
    let radius = ring.radius.clamp(0.0, half_w.min(half_h));
    let center_x = ring.x + half_w;
    let center_y = ring.y + half_h;

    // The band, plus the reach of the refraction so bent samples stay inside
    // the crop. Everything outside this box is untouched canvas.
    let reach = gap * (1.0 + look.refraction) + 4.0;
    let x0 = (center_x - half_w - reach).floor().max(0.0);
    let y0 = (center_y - half_h - reach).floor().max(0.0);
    let x1 = (center_x + half_w + reach).ceil().min(f64::from(image_w));
    let y1 = (center_y + half_h + reach).ceil().min(f64::from(image_h));
    if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
        return None;
    }
    let layer_w = (x1 - x0) as u32;
    let layer_h = (y1 - y0) as u32;
    let mut layer = RgbaImage::from_pixel(layer_w, layer_h, image::Rgba([0, 0, 0, 0]));

    // Frosted glass reads better over busy backdrops, and the reference mixes
    // a blurred copy in everywhere except right at the lips, where the
    // refracted edge has to stay crisp. Blur only the crop we sample from.
    let blurred = if look.blur > 0.0 {
        let mut crop = RgbaImage::from_pixel(layer_w, layer_h, image::Rgba([0, 0, 0, 0]));
        image::imageops::replace(&mut crop, backdrop, -x0 as i64, -y0 as i64);
        let radius = (look.blur * gap).max(0.5);
        crate::capture::editor::render::apply_blur_rect(
            &mut crop,
            crate::capture::editor::types::Rect {
                x: 0,
                y: 0,
                width: layer_w as i32,
                height: layer_h as i32,
            },
            radius,
            false,
        );
        Some(crop)
    } else {
        None
    };

    // Reference scale: refrPx = hGrad * (1-1/1.5) * refract * 30px. For a thin
    // frame band that would throw samples across the card, so normalize the
    // slope (lips = +/-1) and bend by a fraction of the band instead.
    let max_bend = gap * look.refraction * 0.45;
    // Light rig from the reference shader (y negated: GL is y-up, images are
    // y-down): key upper-right, fill lower-left, broad sheen, tight sky light.
    let lights: [(f64, f64, f64, f64, f64); 4] = [
        (0.4, -0.7, 1.0, 90.0, 1.0),
        (-0.3, 0.5, 1.0, 50.0, 0.3),
        (0.1, -0.3, 1.0, 6.0, 0.1),
        (0.0, -0.9, 0.4, 120.0, 0.6),
    ];

    for ly in 0..layer_h {
        let pixel_y = y0 + f64::from(ly) + 0.5;
        let py = pixel_y - center_y;
        for lx in 0..layer_w {
            let pixel_x = x0 + f64::from(lx) + 0.5;
            let px = pixel_x - center_x;
            let (distance, normal_x, normal_y) = rounded_rect_sdf(px, py, half_w, half_h, radius);
            if distance > gap + 1.5 || distance < -1.5 {
                continue;
            }
            let distance_from_card = distance.clamp(0.0, gap);

            let u = distance_from_card / gap;
            let (slope, bend_amount) = if look.clear_body {
                clear_lens(u, gap, look.refraction)
            } else if distance > gap {
                // Outside the band there is no glass: keep the backdrop sharp so
                // no gray fringe leaks past the outer lip.
                (lens_slope(u, gap), 0.0)
            } else {
                let frost_slope = lens_slope(u, gap);
                (frost_slope, (frost_slope / 4.0) * max_bend)
            };
            let bend_sign = if look.clear_body { -1.0 } else { 1.0 };
            let sample_x = pixel_x + bend_sign * normal_x * bend_amount;
            let sample_y = pixel_y + bend_sign * normal_y * bend_amount;

            // Colour fringing follows the reference: stronger at the rim where
            // the surface tilts, calmer through the middle.
            let edge_weight =
                (1.0 - (2.0 * (distance_from_card / gap - 0.5)).abs()).clamp(0.0, 1.0);
            let edge = 1.0 - edge_weight;
            let fringing = if look.clear_body {
                bend_amount * look.chroma
            } else {
                look.chroma * 18.0 * (edge * 0.7 + 0.3) * 0.12 * gap * 0.25
            };
            let red = sample(
                backdrop,
                sample_x + normal_x * fringing,
                sample_y + normal_y * fringing,
            );
            let green = sample(backdrop, sample_x, sample_y);
            let blue = sample(
                backdrop,
                sample_x - normal_x * fringing,
                sample_y - normal_y * fringing,
            );
            // Edge-weighted blur mix: frosted through the body, sharp enough
            // at the lips that the refracted edge stays readable.
            let blur_mix = 1.0 - 0.15 * edge_weight;
            let (red, green, blue) = match blurred.as_ref() {
                Some(frosted) => {
                    let frost = |offset_x: f64, offset_y: f64| {
                        sample(frosted, sample_x + offset_x - x0, sample_y + offset_y - y0)
                    };
                    let frosted_red = frost(normal_x * fringing, normal_y * fringing);
                    let frosted_green = frost(0.0, 0.0);
                    let frosted_blue = frost(-normal_x * fringing, -normal_y * fringing);
                    let mix =
                        |sharp: f64, blurred: f64| sharp * (1.0 - blur_mix) + blurred * blur_mix;
                    (
                        [
                            mix(red[0], frosted_red[0]),
                            mix(red[1], frosted_red[1]),
                            mix(red[2], frosted_red[2]),
                            mix(red[3], frosted_red[3]),
                        ],
                        [
                            mix(green[0], frosted_green[0]),
                            mix(green[1], frosted_green[1]),
                            mix(green[2], frosted_green[2]),
                            mix(green[3], frosted_green[3]),
                        ],
                        [
                            mix(blue[0], frosted_blue[0]),
                            mix(blue[1], frosted_blue[1]),
                            mix(blue[2], frosted_blue[2]),
                            mix(blue[3], frosted_blue[3]),
                        ],
                    )
                }
                None => (red, green, blue),
            };
            // Work premultiplied so a transparent or partly transparent
            // backdrop keeps its alpha and the highlights can light it up.
            let backdrop_alpha = green[3];
            let mut color = if backdrop_alpha > 0.001 {
                [
                    red[0] * red[3] / backdrop_alpha,
                    green[1] * green[3] / backdrop_alpha,
                    blue[2] * blue[3] / backdrop_alpha,
                ]
            } else {
                [0.0, 0.0, 0.0]
            };

            let depth = if gap > 0.0 {
                smoothstep(0.0, gap, distance_from_card)
            } else {
                0.0
            };
            let source_luminance = 0.299 * color[0] + 0.587 * color[1] + 0.114 * color[2];
            let gain = if look.clear_body {
                1.0 + look.brightness * (1.0 - smoothstep(0.85, 0.98, source_luminance))
            } else {
                1.0 + look.brightness
            };
            for channel in color.iter_mut() {
                *channel *= gain;
            }
            let luminance = source_luminance * gain;
            for channel in color.iter_mut() {
                *channel = luminance + (*channel - luminance) * (1.0 + look.saturation);
            }
            // Cool glass tint from the reference, plus its depth lift.
            color = [
                color[0] * (1.0 - 0.08 * look.tint),
                color[1] * (1.0 - 0.05 * look.tint),
                color[2] * (1.0 + 0.05 * look.tint),
            ];
            for channel in color.iter_mut() {
                *channel *= 1.0 + 0.06 * depth * look.tint.max(0.2);
            }
            // Tinted siblings (Glass Light/Dark): lift or deepen the body
            // here, before the lighting terms are added, so the speculars
            // and rim keep full strength on the shaded body.
            if look.body_shade > 0.0 {
                let keep = (1.0 - look.body_shade.clamp(0.0, 0.9)).max(0.0);
                for channel in color.iter_mut() {
                    *channel *= keep;
                }
            } else if look.body_shade < 0.0 {
                let lift = (-look.body_shade).clamp(0.0, 0.9);
                for channel in color.iter_mut() {
                    *channel = *channel * (1.0 - lift * 0.5) + lift * 0.5;
                }
            }

            // Surface normal of the beveled glass: the clear dome rises along
            // the outward normal, the frost band keeps the reference tube's
            // negated gradient.
            let normal = if look.clear_body {
                normalize3(normal_x * slope, normal_y * slope, 1.0)
            } else {
                normalize3(-normal_x * slope, -normal_y * slope, 1.0)
            };

            let mut specular = 0.0;
            if look.clear_body {
                for (light_x, light_y, weight) in [(-0.6, -0.6, 1.0), (0.6, 0.6, 0.5)] {
                    let (hx, hy, hz) = normalize3(light_x, light_y, 2.0);
                    let dot = (normal.0 * hx + normal.1 * hy + normal.2 * hz).max(0.0);
                    specular += dot.powf(12.0) * weight;
                }
            } else {
                for (light_x, light_y, light_z, shininess, weight) in lights {
                    let (hx, hy, hz) = normalize3(light_x, light_y, light_z + 1.0);
                    let dot = (normal.0 * hx + normal.1 * hy + normal.2 * hz).max(0.0);
                    specular += dot.powf(shininess) * weight;
                }
            }
            specular *= look.specular;

            let fresnel = if look.clear_body {
                let f0 = ((1.5_f64 - 1.0) / (1.5 + 1.0)).powi(2);
                f0 + (1.0 - f0) * (1.0 - normal.2.abs()).powf(5.0) * look.fresnel
            } else {
                (1.0 - normal.2.abs()).powf(4.0) * look.fresnel
            };

            let alpha = if look.clear_body {
                let inverse_root_two = std::f64::consts::FRAC_1_SQRT_2;
                let environment =
                    (0.70 + 0.30 * (normal.0 + normal.1) * -inverse_root_two).clamp(0.08, 0.93);
                let white_level = color.iter().copied().fold(f64::INFINITY, f64::min);
                let environment = environment
                    + ((1.0 - environment) - environment) * smoothstep(0.90, 0.98, white_level);
                let reflect = ((fresnel + specular) * (1.0 + look.reflection)).clamp(0.0, 0.92);
                for channel in color.iter_mut() {
                    *channel = *channel * backdrop_alpha * (1.0 - reflect) + environment * reflect;
                }
                backdrop_alpha * (1.0 - reflect) + reflect
            } else {
                let top_bias = (0.5 - 0.5 * py / half_h.max(1.0)).clamp(0.0, 1.0);
                let tilt = (1.0 - normal.2.abs()).powf(1.5);
                let reflection = tilt * (0.25 + 0.75 * top_bias) * look.reflection;
                let sheen = edge_weight * top_bias.powf(2.0) * 0.20;
                let outer_sdf = distance - gap;
                let outer_stroke = smoothstep(-2.5, -1.5, outer_sdf)
                    * (1.0 - smoothstep(-1.0, 0.0, outer_sdf))
                    * (0.3 + 0.7 * top_bias);
                let inner_stroke = smoothstep(-1.5, -0.5, distance)
                    * (1.0 - smoothstep(0.5, 1.5, distance))
                    * (0.3 + 0.7 * top_bias);
                let rim = edge * look.edge_highlight * 0.20;
                let inner_glow = smoothstep(
                    5.0,
                    0.0,
                    (gap - distance_from_card).min(distance_from_card + 1.0),
                ) * look.edge_highlight
                    * 0.15;
                let environment = (0.5 - normal.1 * 0.5) * fresnel * 0.04;

                let body = specular + reflection + sheen + environment;
                let lips = rim
                    + inner_glow
                    + (outer_stroke + inner_stroke * 0.8) * look.edge_highlight * 0.5;
                let envelope = (0.35 + 0.65 * edge.max(tilt)).clamp(0.0, 1.0);
                let addition = body * envelope + lips;
                for channel in color.iter_mut() {
                    *channel = *channel * backdrop_alpha + addition;
                }
                (backdrop_alpha + addition).clamp(0.0, 1.0)
            };
            let fresnel_alpha = if look.clear_body { 0.0 } else { fresnel * 0.2 };
            for channel in color.iter_mut() {
                *channel = *channel * (1.0 - fresnel_alpha) + alpha * fresnel_alpha;
            }

            // Anti-aliased mask: opaque through the band, feathered on the
            // outer lip and where the glass meets the card.
            let inner_alpha = smoothstep(-0.5, 0.5, distance);
            let outer_alpha = 1.0 - smoothstep(gap - 0.5, gap + 0.5, distance);
            let coverage = (inner_alpha * outer_alpha).clamp(0.0, 1.0);
            let masked_alpha = (alpha * coverage).clamp(0.0, 1.0);
            if coverage <= 0.002 || masked_alpha <= 0.002 {
                continue;
            }
            let to_byte = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            let straight = |value: f64| (value / alpha).clamp(0.0, 1.0);
            layer.put_pixel(
                lx,
                ly,
                image::Rgba([
                    to_byte(straight(color[0])),
                    to_byte(straight(color[1])),
                    to_byte(straight(color[2])),
                    to_byte(masked_alpha),
                ]),
            );
        }
    }

    Some(GlassLayer {
        image: layer,
        x: x0 as i64,
        y: y0 as i64,
    })
}

/// One-frame memo of the static preview's glass layer, keyed by the captured
/// backdrop crop, the ring, and the look.
pub struct LiquidPreview {
    cached: Option<LiquidPreviewCache>,
}

struct LiquidPreviewCache {
    backdrop: RgbaImage,
    ring: GlassRing,
    look: GlassLook,
    layer: GlassLayer,
}

impl LiquidPreview {
    /// Create an empty preview cache.
    pub fn new() -> Self {
        Self { cached: None }
    }

    /// The glass layer for `backdrop` in backdrop pixels, reusing the last
    /// result while the backdrop, ring and look are unchanged.
    pub fn layer(
        &mut self,
        backdrop: &RgbaImage,
        ring: &GlassRing,
        look: &GlassLook,
    ) -> Option<&GlassLayer> {
        let reusable = self.cached.as_ref().is_some_and(|cached| {
            &cached.backdrop == backdrop && cached.ring == *ring && cached.look == *look
        });
        if !reusable {
            let layer = glass_layer(backdrop, ring, look)?;
            self.cached = Some(LiquidPreviewCache {
                backdrop: backdrop.clone(),
                ring: *ring,
                look: *look,
                layer,
            });
        }
        self.cached.as_ref().map(|cached| &cached.layer)
    }
}

impl Default for LiquidPreview {
    fn default() -> Self {
        Self::new()
    }
}

/// Rasterize a recorded scene group into straight-alpha pixels at `crop`.
pub fn liquid_preview_backdrop(group: &gtk4::cairo::Pattern, crop: Rect) -> Option<RgbaImage> {
    if crop.width <= 0 || crop.height <= 0 {
        return None;
    }
    let mut surface =
        gtk4::cairo::ImageSurface::create(gtk4::cairo::Format::ARgb32, crop.width, crop.height)
            .ok()?;
    {
        let context = gtk4::cairo::Context::new(&surface).ok()?;
        context.set_operator(gtk4::cairo::Operator::Source);
        context.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        context.paint().ok()?;
        context.set_operator(gtk4::cairo::Operator::Over);
        context.translate(-f64::from(crop.x), -f64::from(crop.y));
        context.set_source(group).ok()?;
        context.paint().ok()?;
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().ok()?;
    Some(super::cairo_argb_to_rgba_image(
        crop.width as u32,
        crop.height as u32,
        stride,
        data.as_ref(),
    ))
}

/// The refracted glass band for one preview frame, in absolute device pixels:
/// `crop` is the bounded rectangle rasterized from `group`, and `ring` sits
/// inside it.
pub fn liquid_preview_layer(
    preview: &mut LiquidPreview,
    group: &gtk4::cairo::Pattern,
    crop: Rect,
    ring: &GlassRing,
    look: &GlassLook,
) -> Option<GlassLayer> {
    let backdrop = liquid_preview_backdrop(group, crop)?;
    let layer = preview.layer(&backdrop, ring, look)?;
    Some(GlassLayer {
        image: layer.image.clone(),
        x: i64::from(crop.x) + layer.x,
        y: i64::from(crop.y) + layer.y,
    })
}

fn normalize3(x: f64, y: f64, z: f64) -> (f64, f64, f64) {
    let length = (x * x + y * y + z * z).sqrt();
    if length <= 1e-9 {
        return (0.0, 0.0, 1.0);
    }
    (x / length, y / length, z / length)
}
