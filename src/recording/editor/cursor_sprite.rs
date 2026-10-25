use super::click_effect::{
    circle_opacity, circle_radius_fraction, ripple_opacity, ripple_radius_fraction, CIRCLE_ALPHA,
    CIRCLE_FILL_GRAY_LINEAR, CIRCLE_RIM_ALPHA, CIRCLE_RIM_GRAY_LINEAR, CIRCLE_RIM_WIDTH_01,
    RIPPLE_BAND_01,
};
use super::model::{ClickEffect, CursorSettings, CursorTheme, MAX_CURSOR_SIZE, MIN_CURSOR_SIZE};
use gtk4::cairo::{
    Antialias, Context, FillRule, Filter, Format, ImageSurface, RadialGradient, SurfacePattern,
};
use image::RgbaImage;
use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::TAU;
use std::path::Path;
use std::rc::Rc;
use std::sync::OnceLock;

const ADWAITA_DEFAULT: &[u8] = include_bytes!("../../../assets/cursors/adwaita/default.png");
const ADWAITA_HAND: &[u8] = include_bytes!("../../../assets/cursors/adwaita/hand.png");
const ADWAITA_TEXT: &[u8] = include_bytes!("../../../assets/cursors/adwaita/text.png");
const ADWAITA_CROSSHAIR: &[u8] = include_bytes!("../../../assets/cursors/adwaita/crosshair.png");
const YARU_DEFAULT: &[u8] = include_bytes!("../../../assets/cursors/yaru/default.png");
const YARU_HAND: &[u8] = include_bytes!("../../../assets/cursors/yaru/hand.png");
const YARU_TEXT: &[u8] = include_bytes!("../../../assets/cursors/yaru/text.png");
const YARU_CROSSHAIR: &[u8] = include_bytes!("../../../assets/cursors/yaru/crosshair.png");
const WHITE_DEFAULT: &[u8] = include_bytes!("../../../assets/cursors/white/default.png");
const WHITE_HAND: &[u8] = include_bytes!("../../../assets/cursors/white/hand.png");
const WHITE_TEXT: &[u8] = include_bytes!("../../../assets/cursors/white/text.png");
const WHITE_CROSSHAIR: &[u8] = include_bytes!("../../../assets/cursors/white/crosshair.png");
const BLACK_DEFAULT: &[u8] = include_bytes!("../../../assets/cursors/black/default.png");
const BLACK_HAND: &[u8] = include_bytes!("../../../assets/cursors/black/hand.png");
const BLACK_TEXT: &[u8] = include_bytes!("../../../assets/cursors/black/text.png");
const BLACK_CROSSHAIR: &[u8] = include_bytes!("../../../assets/cursors/black/crosshair.png");
const MACOS_DEFAULT: &[u8] = include_bytes!("../../../assets/cursors/macos/default.png");
const MACOS_HAND: &[u8] = include_bytes!("../../../assets/cursors/macos/hand.png");
const MACOS_TEXT: &[u8] = include_bytes!("../../../assets/cursors/macos/text.png");
const MACOS_CROSSHAIR: &[u8] = include_bytes!("../../../assets/cursors/macos/crosshair.png");
const TAHOE_DEFAULT: &[u8] = include_bytes!("../../../assets/cursors/tahoe/default.png");
const TAHOE_HAND: &[u8] = include_bytes!("../../../assets/cursors/tahoe/hand.png");
const TAHOE_TEXT: &[u8] = include_bytes!("../../../assets/cursors/tahoe/text.png");
const TAHOE_CROSSHAIR: &[u8] = include_bytes!("../../../assets/cursors/tahoe/crosshair.png");
const TAHOE_INVERTED_DEFAULT: &[u8] =
    include_bytes!("../../../assets/cursors/tahoe_inverted/default.png");
const TAHOE_INVERTED_HAND: &[u8] =
    include_bytes!("../../../assets/cursors/tahoe_inverted/hand.png");
const TAHOE_INVERTED_TEXT: &[u8] =
    include_bytes!("../../../assets/cursors/tahoe_inverted/text.png");
const TAHOE_INVERTED_CROSSHAIR: &[u8] =
    include_bytes!("../../../assets/cursors/tahoe_inverted/crosshair.png");
const DOT_DEFAULT: &[u8] = include_bytes!("../../../assets/cursors/dot/default.png");
const DOT_HAND: &[u8] = include_bytes!("../../../assets/cursors/dot/hand.png");
const DOT_TEXT: &[u8] = include_bytes!("../../../assets/cursors/dot/text.png");
const DOT_CROSSHAIR: &[u8] = include_bytes!("../../../assets/cursors/dot/crosshair.png");
const MINIMAL_DEFAULT: &[u8] = include_bytes!("../../../assets/cursors/minimal/default.png");
const MINIMAL_HAND: &[u8] = include_bytes!("../../../assets/cursors/minimal/hand.png");
const MINIMAL_TEXT: &[u8] = include_bytes!("../../../assets/cursors/minimal/text.png");
const MINIMAL_CROSSHAIR: &[u8] = include_bytes!("../../../assets/cursors/minimal/crosshair.png");
const CURSOR_SHADOW_OFFSET_X: f64 = 2.2;
const CURSOR_SHADOW_OFFSET_Y: f64 = 3.2;
const DOT_CURSOR_RADIUS: f64 = 21.0;
const DOT_CURSOR_SHADOW_BLUR: f64 = 4.0;
const MAX_CURSOR_SHADOW: f64 = 1.0;
const PREVIEW_FIT_FRACTION: f64 = 0.9;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpriteKind {
    Default,
    Hand,
    Text,
    Crosshair,
}

pub fn overlay_scale(size: f64, zoom: f64) -> f64 {
    let _ = zoom;
    size.clamp(MIN_CURSOR_SIZE, MAX_CURSOR_SIZE)
}

pub fn hotspot(theme: CursorTheme, kind: &str) -> (f64, f64) {
    match (theme, sprite_kind(kind)) {
        (CursorTheme::Adwaita, SpriteKind::Default) => (6.0, 2.0),
        (CursorTheme::Adwaita, SpriteKind::Hand) => (14.0, 10.0),
        (CursorTheme::Adwaita, SpriteKind::Text) => (22.0, 24.0),
        (CursorTheme::Adwaita, SpriteKind::Crosshair) => (22.0, 22.0),
        (CursorTheme::Yaru, SpriteKind::Default) => (7.0, 7.0),
        (CursorTheme::Yaru, SpriteKind::Hand) => (15.0, 9.0),
        (CursorTheme::Yaru, SpriteKind::Text) => (21.0, 23.0),
        (CursorTheme::Yaru, SpriteKind::Crosshair) => (22.0, 22.0),
        (CursorTheme::White, SpriteKind::Default) => (14.0, 8.0),
        (CursorTheme::White, SpriteKind::Hand) => (18.0, 10.0),
        (CursorTheme::White, SpriteKind::Text) => (22.0, 22.0),
        (CursorTheme::White, SpriteKind::Crosshair) => (22.0, 22.0),
        (CursorTheme::Black, SpriteKind::Default) => (14.0, 8.0),
        (CursorTheme::Black, SpriteKind::Hand) => (18.0, 10.0),
        (CursorTheme::Black, SpriteKind::Text) => (22.0, 22.0),
        (CursorTheme::Black, SpriteKind::Crosshair) => (22.0, 22.0),
        (CursorTheme::Macos, SpriteKind::Default) => (7.8, 4.1),
        (CursorTheme::Macos, SpriteKind::Hand) => (16.5, 6.2),
        (CursorTheme::Macos, SpriteKind::Text) => (24.2, 24.1),
        (CursorTheme::Macos, SpriteKind::Crosshair) => (24.1, 24.0),
        (CursorTheme::Tahoe, SpriteKind::Default) => (7.9, 4.1),
        (CursorTheme::Tahoe, SpriteKind::Hand) => (21.0, 5.0),
        (CursorTheme::Tahoe, SpriteKind::Text) => (24.0, 24.0),
        (CursorTheme::Tahoe, SpriteKind::Crosshair) => (24.1, 24.0),
        (CursorTheme::TahoeInverted, SpriteKind::Default) => (7.9, 4.1),
        (CursorTheme::TahoeInverted, SpriteKind::Hand) => (21.0, 5.0),
        (CursorTheme::TahoeInverted, SpriteKind::Text) => (24.0, 24.0),
        (CursorTheme::TahoeInverted, SpriteKind::Crosshair) => (24.1, 24.0),
        (CursorTheme::Dot, SpriteKind::Default) => (24.0, 24.0),
        (CursorTheme::Dot, SpriteKind::Hand) => (24.0, 24.0),
        (CursorTheme::Dot, SpriteKind::Text) => (24.0, 24.0),
        (CursorTheme::Dot, SpriteKind::Crosshair) => (24.0, 24.0),
        (CursorTheme::Minimal, SpriteKind::Default) => (7.0, 3.0),
        (CursorTheme::Minimal, SpriteKind::Hand) => (7.0, 3.0),
        (CursorTheme::Minimal, SpriteKind::Text) => (7.0, 3.0),
        (CursorTheme::Minimal, SpriteKind::Crosshair) => (7.0, 3.0),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    cr: &Context,
    x: f64,
    y: f64,
    pulse: f64,
    press_scale: f64,
    kind: &str,
    settings: CursorSettings,
    alpha: f64,
) {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha < 0.02 {
        return;
    }
    let settings = settings.clamped();
    let scale = overlay_scale(settings.size, 1.0) * pulse.max(0.7) * press_scale.clamp(0.5, 1.5);
    if pulse > 1.02 {
        cr.set_source_rgba(
            1.0,
            1.0,
            1.0,
            0.22 * ((pulse - 1.0) / 0.35).clamp(0.0, 1.0) * alpha,
        );
        cr.arc(x, y, 16.0 * scale, 0.0, TAU);
        let _ = cr.fill();
    }
    if settings.theme == CursorTheme::Dot {
        let surface = surface_from_rgba(bitmap(settings.theme, kind));
        paint_dot_sprite(cr, &surface, x, y, scale, settings.shadow, alpha);
        return;
    }
    // A large cursor magnifies the sprite past its own pixels, so the art is
    // supersampled first; `art` is how many source pixels one hotspot unit is.
    let (surface, art) = sprite_art(settings.theme, kind, scale);
    let (hx, hy) = hotspot(settings.theme, kind);
    paint_sprite(
        cr,
        &surface,
        x,
        y,
        hx,
        hy,
        scale,
        settings.shadow,
        alpha,
        0.0,
        art,
    );
}

pub fn draw_click(
    cr: &Context,
    x: f64,
    y: f64,
    progress: f64,
    settings: CursorSettings,
    alpha: f64,
    reference_width: f64,
) {
    let settings = settings.clamped();
    let frame_alpha = alpha.clamp(0.0, 1.0);
    // The studied ripple and circle have fixed opacity; only the local
    // Spotlight/Echo effects use the user opacity knob.
    let legacy_alpha = (frame_alpha * settings.click_opacity).clamp(0.0, 1.0);
    match settings.click_effect {
        ClickEffect::None => {}
        ClickEffect::Spotlight => draw_spotlight(
            cr,
            x,
            y,
            progress,
            settings.size,
            settings.click_intensity,
            legacy_alpha,
            settings.click_color,
            settings.click_scale,
        ),
        ClickEffect::Ripple => draw_ripple(cr, x, y, progress, frame_alpha, reference_width),
        ClickEffect::Circle => draw_circle(cr, x, y, progress, frame_alpha, reference_width),
        ClickEffect::Echo => draw_echo(
            cr,
            x,
            y,
            progress,
            settings.size,
            settings.click_intensity,
            legacy_alpha,
            settings.click_color,
            settings.click_scale,
        ),
    }
}

/// Draw the studied ripple's geometry as a ring overlay.
///
/// The studied ripple is a footage warp; this is a drawn stand-in that carries
/// the studied lifetime, band, and outward travel but does not pull or split
/// the video pixels. Export and the video preview apply the footage warp
/// separately; this remains the overlay-only fallback. The studied effect has a
/// fixed size, opacity, and no colour, so the local size/opacity/intensity and
/// colour knobs do not apply; the ring is drawn in a neutral white.
pub fn draw_ripple(cr: &Context, x: f64, y: f64, progress: f64, alpha: f64, reference_width: f64) {
    let progress = progress.clamp(0.0, 1.0);
    let amount = ripple_opacity(progress) * alpha.clamp(0.0, 1.0);
    if amount < 0.02 || reference_width <= 0.0 {
        return;
    }
    let radius = reference_width * ripple_radius_fraction(progress);
    let band = (reference_width * RIPPLE_BAND_01).max(1.0);
    cr.set_line_width(band);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.82 * amount);
    cr.arc(x, y, radius, 0.0, TAU);
    let _ = cr.stroke();
}

/// Draw the studied circle: a grey disc with a thin dark rim.
///
/// Sizes are fractions of the video width, so the effect scales with the
/// recording resolution. The studied greys are linear-space values; they are
/// used directly on our non-linear overlay surface. The studied effect has a
/// fixed size and opacity, so the local size/opacity knobs do not apply.
pub fn draw_circle(cr: &Context, x: f64, y: f64, progress: f64, alpha: f64, reference_width: f64) {
    let progress = progress.clamp(0.0, 1.0);
    let fade = circle_opacity(progress);
    let alpha = (alpha.clamp(0.0, 1.0) * CIRCLE_ALPHA * fade).clamp(0.0, 1.0);
    if alpha <= 0.0 || reference_width <= 0.0 {
        return;
    }
    let radius = reference_width * circle_radius_fraction(progress);
    if radius <= 0.0 {
        return;
    }
    let rim_width = (reference_width * CIRCLE_RIM_WIDTH_01).max(0.75);
    let fill = CIRCLE_FILL_GRAY_LINEAR;
    let rim = CIRCLE_RIM_GRAY_LINEAR;
    cr.set_source_rgba(fill, fill, fill, alpha);
    cr.arc(x, y, radius, 0.0, TAU);
    let _ = cr.fill();
    cr.set_line_width(rim_width);
    cr.set_source_rgba(rim, rim, rim, alpha * CIRCLE_RIM_ALPHA);
    cr.arc(x, y, radius, 0.0, TAU);
    let _ = cr.stroke();
}

#[allow(clippy::too_many_arguments)]
pub fn draw_spotlight(
    cr: &Context,
    x: f64,
    y: f64,
    progress: f64,
    size: f64,
    intensity: f64,
    alpha: f64,
    color: (u8, u8, u8),
    scale: f64,
) {
    let progress = progress.clamp(0.0, 1.0);
    let fade = (1.0 - progress).powi(2);
    let amount = fade * intensity.clamp(0.0, 1.0) * alpha.clamp(0.0, 1.0);
    if amount < 0.02 {
        return;
    }
    let (r, g, b) = click_rgb(color);
    let scale = overlay_scale(size, 1.0) * scale.max(0.01);
    let radius = (18.0 + progress * 12.0) * scale;
    let glow = gtk4::cairo::RadialGradient::new(x, y, 0.0, x, y, radius);
    glow.add_color_stop_rgba(0.0, r, g, b, 0.52 * amount);
    glow.add_color_stop_rgba(0.48, r, g, b, 0.24 * amount);
    glow.add_color_stop_rgba(1.0, r, g, b, 0.0);
    let _ = cr.set_source(&glow);
    cr.arc(x, y, radius, 0.0, TAU);
    let _ = cr.fill();
}

#[allow(clippy::too_many_arguments)]
pub fn draw_echo(
    cr: &Context,
    x: f64,
    y: f64,
    progress: f64,
    size: f64,
    intensity: f64,
    alpha: f64,
    color: (u8, u8, u8),
    scale: f64,
) {
    let progress = progress.clamp(0.0, 1.0);
    let fade = (1.0 - progress).powi(2);
    let amount = fade * intensity.clamp(0.0, 1.0) * alpha.clamp(0.0, 1.0);
    if amount < 0.02 {
        return;
    }
    let (r, g, b) = click_rgb(color);
    let scale = overlay_scale(size, 1.0) * scale.max(0.01);
    let base = 12.0 * scale;
    let outer = base + progress * 32.0 * scale;
    let inner = (base * 0.58).max(outer * 0.62);
    cr.set_line_width(2.0);
    cr.set_source_rgba(r, g, b, 0.72 * amount);
    cr.arc(x, y, outer, 0.0, TAU);
    let _ = cr.stroke();
    cr.set_line_width(1.5);
    cr.set_source_rgba(r, g, b, 0.4 * amount);
    cr.arc(x, y, inner, 0.0, TAU);
    let _ = cr.stroke();
    cr.set_source_rgba(r, g, b, 0.22 * amount);
    cr.arc(x, y, 3.0_f64.max(base * 0.18), 0.0, TAU);
    let _ = cr.fill();
}

fn click_rgb(color: (u8, u8, u8)) -> (f64, f64, f64) {
    (
        color.0 as f64 / 255.0,
        color.1 as f64 / 255.0,
        color.2 as f64 / 255.0,
    )
}

/// Draw a centered theme-card cursor using the current size, shadow, and alpha.
pub fn draw_centered(cr: &Context, width: f64, height: f64, settings: CursorSettings, alpha: f64) {
    let settings = settings.clamped();
    let Some((scale, x, y)) = centered_preview_layout(width, height, settings) else {
        return;
    };
    let _ = cr.save();
    cr.translate(width / 2.0 + x * scale, height / 2.0 + y * scale);
    cr.scale(scale, scale);
    draw(cr, 0.0, 0.0, 1.0, 1.0, "default", settings, alpha);
    let _ = cr.restore();
}

/// Return the theme-card cursor's hotspot in preview coordinates.
pub fn centered_preview_hotspot(width: f64, height: f64, settings: CursorSettings) -> (f64, f64) {
    let Some((scale, x, y)) = centered_preview_layout(width, height, settings.clamped()) else {
        return (width / 2.0, height / 2.0);
    };
    (width / 2.0 + x * scale, height / 2.0 + y * scale)
}

fn centered_preview_layout(
    width: f64,
    height: f64,
    settings: CursorSettings,
) -> Option<(f64, f64, f64)> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let bitmap = bitmap(settings.theme, "default");
    let (bx, by, bw, bh) = opaque_bounds(bitmap);
    if bw <= 0.0 || bh <= 0.0 {
        return None;
    }
    let (hx, hy) = hotspot(settings.theme, "default");
    let (max_width, max_height) = max_preview_footprint(settings.theme);
    let scale = (width / (max_width * MAX_CURSOR_SIZE))
        .min(height / (max_height * MAX_CURSOR_SIZE))
        .min(1.0)
        * PREVIEW_FIT_FRACTION;
    Some((
        scale,
        (hx - bx - bw / 2.0) * settings.size,
        (hy - by - bh / 2.0) * settings.size,
    ))
}

fn max_preview_footprint(theme: CursorTheme) -> (f64, f64) {
    if theme == CursorTheme::Dot {
        let extent = DOT_CURSOR_RADIUS
            + DOT_CURSOR_SHADOW_BLUR * MAX_CURSOR_SHADOW
            + (CURSOR_SHADOW_OFFSET_X * MAX_CURSOR_SHADOW)
                .hypot(CURSOR_SHADOW_OFFSET_Y * MAX_CURSOR_SHADOW);
        (extent * 2.0, extent * 2.0)
    } else {
        (
            48.0 + CURSOR_SHADOW_OFFSET_X * MAX_CURSOR_SHADOW,
            48.0 + CURSOR_SHADOW_OFFSET_Y * MAX_CURSOR_SHADOW,
        )
    }
}

pub fn write_png(path: &Path, settings: CursorSettings, kind: &str) -> anyhow::Result<(f64, f64)> {
    let settings = settings.clamped();
    let scale = settings.size;
    let (hx, hy) = hotspot(settings.theme, kind);
    let pad = 8.0;
    let shadow_x = 2.4 * scale * (0.25 + settings.shadow);
    let shadow_y = 3.4 * scale * (0.25 + settings.shadow);
    let width = ((48.0 * scale) + shadow_x + pad * 2.0).ceil().max(16.0) as i32;
    let height = ((48.0 * scale) + shadow_y + pad * 2.0).ceil().max(16.0) as i32;
    let mut surface = ImageSurface::create(Format::ARgb32, width, height)?;
    let out_hot = (pad + hx * scale, pad + hy * scale);
    {
        let cr = Context::new(&surface)?;
        cr.set_antialias(Antialias::Best);
        draw(&cr, out_hot.0, out_hot.1, 1.0, 1.0, kind, settings, 1.0);
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let pixels = surface.data()?;
    let mut img = image::RgbaImage::new(width as u32, height as u32);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let i = y * stride + x * 4;
            let a = pixels[i + 3];
            img.put_pixel(
                x as u32,
                y as u32,
                image::Rgba([pixels[i + 2], pixels[i + 1], pixels[i], a]),
            );
        }
    }
    drop(pixels);
    img.save(path)?;
    Ok(out_hot)
}

// Supersampled sprite surfaces for the current thread. `ImageSurface` is
// neither `Send` nor `Sync`, and drawing happens on the one thread that owns
// the cairo context, so the cache is thread-local.
thread_local! {
    static SUPERSAMPLED: RefCell<HashMap<(u8, u8), Rc<ImageSurface>>> =
        RefCell::new(HashMap::new());
}

/// The sprite art is 48 pixels across. Anything drawn larger than its own
/// pixels shows them, so past this factor the draw path samples a supersampled
/// copy: the edges read as continuous instead of as magnified blocks.
const SUPERSAMPLE_ABOVE: f64 = 1.2;
const SUPERSAMPLE_FACTOR: u32 = 4;

fn sprite_art(theme: CursorTheme, kind: &str, scale: f64) -> (Rc<ImageSurface>, f64) {
    if scale <= SUPERSAMPLE_ABOVE {
        return (Rc::new(surface_from_rgba(bitmap(theme, kind))), 1.0);
    }
    let key = (theme as u8, sprite_kind(kind) as u8);
    let surface = SUPERSAMPLED.with(|cache| {
        if let Some(surface) = cache.borrow().get(&key) {
            return Rc::clone(surface);
        }
        let surface = Rc::new(surface_from_rgba(&supersampled_bitmap(theme, kind)));
        cache.borrow_mut().insert(key, Rc::clone(&surface));
        surface
    });
    (surface, f64::from(SUPERSAMPLE_FACTOR))
}

/// Enlarge the sprite with a smooth resampler, in premultiplied alpha so the
/// transparent surround does not bleed dark edges into the outline.
fn supersampled_bitmap(theme: CursorTheme, kind: &str) -> RgbaImage {
    let base = bitmap(theme, kind);
    let mut premultiplied = RgbaImage::new(base.width(), base.height());
    for (x, y, pixel) in base.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let alpha = u16::from(a);
        let premultiply = |channel: u8| ((u16::from(channel) * alpha) / 255) as u8;
        premultiplied.put_pixel(
            x,
            y,
            image::Rgba([premultiply(r), premultiply(g), premultiply(b), a]),
        );
    }
    let width = base.width() * SUPERSAMPLE_FACTOR;
    let height = base.height() * SUPERSAMPLE_FACTOR;
    let resized = image::imageops::resize(
        &premultiplied,
        width,
        height,
        image::imageops::FilterType::Lanczos3,
    );
    let mut out = RgbaImage::new(width, height);
    for (x, y, pixel) in resized.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let alpha = u16::from(a);
        let unpremultiply = |channel: u8| {
            if alpha == 0 {
                0
            } else {
                ((u16::from(channel) * 255) / alpha).min(255) as u8
            }
        };
        out.put_pixel(
            x,
            y,
            image::Rgba([unpremultiply(r), unpremultiply(g), unpremultiply(b), a]),
        );
    }
    out
}

fn paint_sprite(
    cr: &Context,
    surface: &ImageSurface,
    x: f64,
    y: f64,
    hx: f64,
    hy: f64,
    scale: f64,
    shadow: f64,
    alpha: f64,
    tilt: f64,
    art: f64,
) {
    let art = art.max(1.0);
    let sprite_scale = scale / art;
    let _ = cr.save();
    cr.translate(x, y);
    if tilt.abs() > 0.001 {
        cr.rotate(tilt);
    }
    if shadow > 0.01 {
        let _ = cr.save();
        cr.translate(
            CURSOR_SHADOW_OFFSET_X * scale * shadow,
            CURSOR_SHADOW_OFFSET_Y * scale * shadow,
        );
        cr.scale(sprite_scale, sprite_scale);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.55 * shadow * alpha);
        let _ = cr.mask_surface(surface, -hx * art, -hy * art);
        let _ = cr.restore();
    }
    cr.scale(sprite_scale, sprite_scale);
    cr.translate(-hx * art, -hy * art);
    let pattern = SurfacePattern::create(surface);
    pattern.set_filter(Filter::Best);
    let _ = cr.set_source(&pattern);
    cr.set_antialias(Antialias::Best);
    let _ = cr.paint_with_alpha(alpha);
    let _ = cr.restore();
}

fn paint_dot_sprite(
    cr: &Context,
    surface: &ImageSurface,
    x: f64,
    y: f64,
    scale: f64,
    shadow: f64,
    alpha: f64,
) {
    if shadow > 0.01 {
        let radius = DOT_CURSOR_RADIUS;
        let offset_x = CURSOR_SHADOW_OFFSET_X * shadow;
        let offset_y = CURSOR_SHADOW_OFFSET_Y * shadow;
        let blur = DOT_CURSOR_SHADOW_BLUR * shadow;
        let extent = radius + blur + offset_x.hypot(offset_y);
        let shadow_alpha = 0.16 * shadow * alpha;
        let _ = cr.save();
        cr.translate(x, y);
        cr.scale(scale, scale);
        cr.set_antialias(Antialias::Best);
        cr.new_path();
        cr.arc(0.0, 0.0, extent, 0.0, TAU);
        cr.arc(0.0, 0.0, radius, 0.0, TAU);
        cr.set_fill_rule(FillRule::EvenOdd);
        cr.clip();
        cr.arc(offset_x, offset_y, radius + blur, 0.0, TAU);
        let gradient = RadialGradient::new(
            offset_x,
            offset_y,
            radius,
            offset_x,
            offset_y,
            radius + blur,
        );
        gradient.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, shadow_alpha);
        gradient.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
        let _ = cr.set_source(&gradient);
        let _ = cr.fill();
        let _ = cr.restore();
    }

    let _ = cr.save();
    cr.translate(x, y);
    cr.scale(scale, scale);
    cr.translate(-24.0, -24.0);
    let pattern = SurfacePattern::create(surface);
    pattern.set_filter(Filter::Best);
    let _ = cr.set_source(&pattern);
    cr.set_antialias(Antialias::Best);
    let _ = cr.paint_with_alpha(alpha);
    let _ = cr.restore();
}

fn sprite_kind(kind: &str) -> SpriteKind {
    match kind {
        "hand" => SpriteKind::Hand,
        "text" => SpriteKind::Text,
        "crosshair" => SpriteKind::Crosshair,
        _ => SpriteKind::Default,
    }
}

fn bitmap(theme: CursorTheme, kind: &str) -> &'static RgbaImage {
    fn decode(bytes: &[u8]) -> RgbaImage {
        image::load_from_memory(bytes)
            .expect("cursor png")
            .to_rgba8()
    }
    let kind = sprite_kind(kind);
    match (theme, kind) {
        (CursorTheme::Adwaita, SpriteKind::Default) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(ADWAITA_DEFAULT))
        }
        (CursorTheme::Adwaita, SpriteKind::Hand) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(ADWAITA_HAND))
        }
        (CursorTheme::Adwaita, SpriteKind::Text) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(ADWAITA_TEXT))
        }
        (CursorTheme::Adwaita, SpriteKind::Crosshair) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(ADWAITA_CROSSHAIR))
        }
        (CursorTheme::Yaru, SpriteKind::Default) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(YARU_DEFAULT))
        }
        (CursorTheme::Yaru, SpriteKind::Hand) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(YARU_HAND))
        }
        (CursorTheme::Yaru, SpriteKind::Text) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(YARU_TEXT))
        }
        (CursorTheme::Yaru, SpriteKind::Crosshair) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(YARU_CROSSHAIR))
        }
        (CursorTheme::White, SpriteKind::Default) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(WHITE_DEFAULT))
        }
        (CursorTheme::White, SpriteKind::Hand) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(WHITE_HAND))
        }
        (CursorTheme::White, SpriteKind::Text) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(WHITE_TEXT))
        }
        (CursorTheme::White, SpriteKind::Crosshair) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(WHITE_CROSSHAIR))
        }
        (CursorTheme::Black, SpriteKind::Default) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(BLACK_DEFAULT))
        }
        (CursorTheme::Black, SpriteKind::Hand) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(BLACK_HAND))
        }
        (CursorTheme::Black, SpriteKind::Text) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(BLACK_TEXT))
        }
        (CursorTheme::Black, SpriteKind::Crosshair) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(BLACK_CROSSHAIR))
        }
        (CursorTheme::Macos, SpriteKind::Default) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(MACOS_DEFAULT))
        }
        (CursorTheme::Macos, SpriteKind::Hand) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(MACOS_HAND))
        }
        (CursorTheme::Macos, SpriteKind::Text) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(MACOS_TEXT))
        }
        (CursorTheme::Macos, SpriteKind::Crosshair) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(MACOS_CROSSHAIR))
        }
        (CursorTheme::Tahoe, SpriteKind::Default) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(TAHOE_DEFAULT))
        }
        (CursorTheme::Tahoe, SpriteKind::Hand) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(TAHOE_HAND))
        }
        (CursorTheme::Tahoe, SpriteKind::Text) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(TAHOE_TEXT))
        }
        (CursorTheme::Tahoe, SpriteKind::Crosshair) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(TAHOE_CROSSHAIR))
        }
        (CursorTheme::TahoeInverted, SpriteKind::Default) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(TAHOE_INVERTED_DEFAULT))
        }
        (CursorTheme::TahoeInverted, SpriteKind::Hand) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(TAHOE_INVERTED_HAND))
        }
        (CursorTheme::TahoeInverted, SpriteKind::Text) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(TAHOE_INVERTED_TEXT))
        }
        (CursorTheme::TahoeInverted, SpriteKind::Crosshair) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(TAHOE_INVERTED_CROSSHAIR))
        }
        (CursorTheme::Dot, SpriteKind::Default) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(DOT_DEFAULT))
        }
        (CursorTheme::Dot, SpriteKind::Hand) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(DOT_HAND))
        }
        (CursorTheme::Dot, SpriteKind::Text) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(DOT_TEXT))
        }
        (CursorTheme::Dot, SpriteKind::Crosshair) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(DOT_CROSSHAIR))
        }
        (CursorTheme::Minimal, SpriteKind::Default) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(MINIMAL_DEFAULT))
        }
        (CursorTheme::Minimal, SpriteKind::Hand) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(MINIMAL_HAND))
        }
        (CursorTheme::Minimal, SpriteKind::Text) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(MINIMAL_TEXT))
        }
        (CursorTheme::Minimal, SpriteKind::Crosshair) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(MINIMAL_CROSSHAIR))
        }
    }
}

fn opaque_bounds(img: &RgbaImage) -> (f64, f64, f64, f64) {
    let mut min_x = img.width();
    let mut min_y = img.height();
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    for (x, y, pixel) in img.enumerate_pixels() {
        if pixel.0[3] < 16 {
            continue;
        }
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    if max_x < min_x {
        return (0.0, 0.0, img.width() as f64, img.height() as f64);
    }
    (
        min_x as f64,
        min_y as f64,
        (max_x - min_x + 1) as f64,
        (max_y - min_y + 1) as f64,
    )
}

fn surface_from_rgba(img: &RgbaImage) -> ImageSurface {
    let width = img.width() as i32;
    let height = img.height() as i32;
    let mut surface = ImageSurface::create(Format::ARgb32, width, height).expect("cursor surface");
    {
        let stride = surface.stride() as usize;
        let mut data = surface.data().expect("cursor pixels");
        for y in 0..height as usize {
            for x in 0..width as usize {
                let p = img.get_pixel(x as u32, y as u32).0;
                let a = p[3] as u16;
                let i = y * stride + x * 4;
                data[i] = ((p[2] as u16 * a) / 255) as u8;
                data[i + 1] = ((p[1] as u16 * a) / 255) as u8;
                data[i + 2] = ((p[0] as u16 * a) / 255) as u8;
                data[i + 3] = p[3];
            }
        }
    }
    surface.mark_dirty();
    surface
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SURFACE_SIZE: i32 = 192;

    fn render_click(settings: CursorSettings) -> Vec<u8> {
        render_click_at(
            settings,
            TEST_SURFACE_SIZE as f64 / 2.0,
            TEST_SURFACE_SIZE as f64 / 2.0,
        )
    }

    fn render_click_at(settings: CursorSettings, x: f64, y: f64) -> Vec<u8> {
        render_click_with_width(settings, x, y, TEST_SURFACE_SIZE as f64)
    }

    fn render_click_with_width(
        settings: CursorSettings,
        x: f64,
        y: f64,
        reference_width: f64,
    ) -> Vec<u8> {
        let mut surface =
            ImageSurface::create(Format::ARgb32, TEST_SURFACE_SIZE, TEST_SURFACE_SIZE).unwrap();
        {
            let cr = Context::new(&surface).unwrap();
            draw_click(&cr, x, y, 0.25, settings, 1.0, reference_width);
        }
        surface.flush();
        let pixels = surface.data().unwrap().to_vec();
        pixels
    }

    fn render_cursor(settings: CursorSettings) -> Vec<u8> {
        render_cursor_with_kind(settings, "default")
    }

    fn render_cursor_with_scale(settings: CursorSettings, press_scale: f64) -> Vec<u8> {
        render_cursor_with_kind_and_press_scale(settings, "default", press_scale)
    }

    fn render_cursor_with_kind(settings: CursorSettings, kind: &str) -> Vec<u8> {
        render_cursor_with_kind_and_press_scale(settings, kind, 1.0)
    }

    fn render_cursor_with_kind_and_press_scale(
        settings: CursorSettings,
        kind: &str,
        press_scale: f64,
    ) -> Vec<u8> {
        let mut surface =
            ImageSurface::create(Format::ARgb32, TEST_SURFACE_SIZE, TEST_SURFACE_SIZE).unwrap();
        {
            let cr = Context::new(&surface).unwrap();
            draw(
                &cr,
                TEST_SURFACE_SIZE as f64 / 2.0,
                TEST_SURFACE_SIZE as f64 / 2.0,
                1.0,
                press_scale,
                kind,
                settings,
                1.0,
            );
        }
        surface.flush();
        let pixels = surface.data().unwrap().to_vec();
        pixels
    }

    fn render_cursor_on_canvas(
        settings: CursorSettings,
        kind: &str,
        width: i32,
        height: i32,
    ) -> Vec<u8> {
        let mut surface = ImageSurface::create(Format::ARgb32, width, height).unwrap();
        {
            let cr = Context::new(&surface).unwrap();
            draw(
                &cr,
                width as f64 / 2.0,
                height as f64 / 2.0,
                1.0,
                1.0,
                kind,
                settings,
                1.0,
            );
        }
        surface.flush();
        let pixels = surface.data().unwrap().to_vec();
        pixels
    }

    fn render_centered_preview(settings: CursorSettings, width: i32, height: i32) -> Vec<u8> {
        let mut surface = ImageSurface::create(Format::ARgb32, width, height).unwrap();
        {
            let cr = Context::new(&surface).unwrap();
            draw_centered(&cr, width as f64, height as f64, settings, 1.0);
        }
        surface.flush();
        let pixels = surface.data().unwrap().to_vec();
        pixels
    }

    fn alpha_bounds(pixels: &[u8]) -> Option<(usize, usize, usize, usize)> {
        alpha_bounds_with_size(
            pixels,
            TEST_SURFACE_SIZE as usize,
            TEST_SURFACE_SIZE as usize,
        )
    }

    fn alpha_bounds_with_size(
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
    fn png_writes_nonempty_file() {
        let dir = std::env::temp_dir().join(format!("apexshot-cursor-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("cursor.png");
        let hot = write_png(&path, CursorSettings::default(), "default").unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() > 80);
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert!(hot.0 > 0.0 && hot.1 > 0.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adwaita_is_default_theme() {
        assert_eq!(CursorTheme::default(), CursorTheme::Adwaita);
        assert_eq!(CursorTheme::parse("yaru"), CursorTheme::Yaru);
        assert_eq!(CursorTheme::parse("dark"), CursorTheme::Black);
        assert_eq!(CursorTheme::parse("crosshair"), CursorTheme::Adwaita);
    }

    #[test]
    fn settings_clamp_size_and_shadow() {
        let settings = CursorSettings {
            theme: CursorTheme::Yaru,
            size: 9.0,
            shadow: 4.0,
            ..CursorSettings::default()
        }
        .clamped();
        assert!((settings.size - 3.0).abs() < 1e-9);
        assert!((settings.shadow - 1.0).abs() < 1e-9);
    }

    #[test]
    fn overlay_scale_ignores_zoom() {
        let size = 1.4;
        assert!((overlay_scale(size, 1.0) - size).abs() < 1e-12);
        assert!((overlay_scale(size, 2.0) - overlay_scale(size, 1.0)).abs() < 1e-12);
        assert!((overlay_scale(size, 2.0) - size * 2.0).abs() > 0.5);
    }

    #[test]
    fn a_held_button_draws_the_cursor_smaller() {
        let plain = render_cursor(CursorSettings::default());
        let pressed = render_cursor_with_scale(CursorSettings::default(), 0.9);
        let (plain_min, plain_max) = {
            let b = alpha_bounds(&plain).unwrap();
            (b.2 - b.0, b.3 - b.1)
        };
        let (pressed_min, pressed_max) = {
            let b = alpha_bounds(&pressed).unwrap();
            (b.2 - b.0, b.3 - b.1)
        };
        assert!(
            pressed_min < plain_min && pressed_max < plain_max,
            "pressed {pressed_min}x{pressed_max} should be smaller than {plain_min}x{plain_max}"
        );
    }

    #[test]
    fn every_theme_has_renderable_cursor_kinds() {
        for theme in CursorTheme::ALL {
            for kind in ["default", "hand", "text", "crosshair"] {
                let image = bitmap(theme, kind);
                assert!(image.width() > 0 && image.height() > 0, "{theme:?} {kind}");
                assert!(
                    image.pixels().any(|pixel| pixel.0[3] > 0),
                    "{theme:?} {kind}"
                );
                let (hot_x, hot_y) = hotspot(theme, kind);
                assert!(
                    hot_x >= 0.0 && hot_x < image.width() as f64,
                    "{theme:?} {kind}"
                );
                assert!(
                    hot_y >= 0.0 && hot_y < image.height() as f64,
                    "{theme:?} {kind}"
                );
            }
        }
    }

    #[test]
    fn size_and_shadow_change_cursor_rendering() {
        let small = render_cursor(CursorSettings {
            size: MIN_CURSOR_SIZE,
            shadow: 0.0,
            ..CursorSettings::default()
        });
        let large = render_cursor(CursorSettings {
            size: MAX_CURSOR_SIZE,
            shadow: 0.0,
            ..CursorSettings::default()
        });
        let shadowed = render_cursor(CursorSettings {
            size: MIN_CURSOR_SIZE,
            shadow: 1.0,
            ..CursorSettings::default()
        });
        let (small_min_x, _, small_max_x, _) = alpha_bounds(&small).unwrap();
        let (large_min_x, _, large_max_x, _) = alpha_bounds(&large).unwrap();
        assert!(large_max_x - large_min_x > small_max_x - small_min_x);
        assert_ne!(small, shadowed);
    }

    #[test]
    fn size_and_shadow_change_cursor_rendering_for_every_theme_and_kind() {
        for theme in CursorTheme::ALL {
            for kind in ["default", "hand", "text", "crosshair"] {
                let small = render_cursor_with_kind(
                    CursorSettings {
                        theme,
                        size: MIN_CURSOR_SIZE,
                        shadow: 0.0,
                        ..CursorSettings::default()
                    },
                    kind,
                );
                let large = render_cursor_with_kind(
                    CursorSettings {
                        theme,
                        size: MAX_CURSOR_SIZE,
                        shadow: 0.0,
                        ..CursorSettings::default()
                    },
                    kind,
                );
                let shadowed = render_cursor_with_kind(
                    CursorSettings {
                        theme,
                        size: MIN_CURSOR_SIZE,
                        shadow: 1.0,
                        ..CursorSettings::default()
                    },
                    kind,
                );
                let (small_min_x, _, small_max_x, _) = alpha_bounds(&small).unwrap();
                let (large_min_x, _, large_max_x, _) = alpha_bounds(&large).unwrap();
                assert!(
                    large_max_x - large_min_x > small_max_x - small_min_x,
                    "{theme:?} {kind}: size must change the cursor bounds"
                );
                assert_ne!(small, shadowed, "{theme:?} {kind}: shadow must render");
            }
        }
    }

    #[test]
    fn drawn_cursor_hotspots_and_shadows_stay_aligned_for_every_theme_and_kind() {
        const CANVAS_SIZE: i32 = 256;
        let center = CANVAS_SIZE as f64 / 2.0;

        for theme in CursorTheme::ALL {
            for kind in ["default", "hand", "text", "crosshair"] {
                let settings = CursorSettings {
                    theme,
                    size: 1.0,
                    shadow: 0.0,
                    ..CursorSettings::default()
                };
                let foreground = render_cursor_on_canvas(settings, kind, CANVAS_SIZE, CANVAS_SIZE);
                let (min_x, min_y, max_x, max_y) =
                    alpha_bounds_with_size(&foreground, CANVAS_SIZE as usize, CANVAS_SIZE as usize)
                        .unwrap_or_else(|| panic!("{theme:?} {kind}: cursor must render"));
                let sprite = bitmap(theme, kind);
                let (bitmap_min_x, bitmap_min_y, bitmap_max_x, bitmap_max_y) =
                    alpha_bounds_with_size(
                        sprite.as_raw(),
                        sprite.width() as usize,
                        sprite.height() as usize,
                    )
                    .unwrap_or_else(|| panic!("{theme:?} {kind}: cursor bitmap must render"));
                let (hotspot_x, hotspot_y) = hotspot(theme, kind);
                let expected_min_x = center + bitmap_min_x as f64 - hotspot_x;
                let expected_min_y = center + bitmap_min_y as f64 - hotspot_y;
                let expected_max_x = center + bitmap_max_x as f64 - hotspot_x;
                let expected_max_y = center + bitmap_max_y as f64 - hotspot_y;

                assert!(
                    (min_x as f64 - expected_min_x).abs() <= 2.0
                        && (max_x as f64 - expected_max_x).abs() <= 2.0
                        && (min_y as f64 - expected_min_y).abs() <= 2.0
                        && (max_y as f64 - expected_max_y).abs() <= 2.0,
                    "{theme:?} {kind}: rendered bounds ({min_x}, {min_y})..({max_x}, {max_y}) should follow hotspot ({hotspot_x}, {hotspot_y})"
                );

                let shadowed = render_cursor_on_canvas(
                    CursorSettings {
                        shadow: 1.0,
                        ..settings
                    },
                    kind,
                    CANVAS_SIZE,
                    CANVAS_SIZE,
                );
                assert_ne!(foreground, shadowed, "{theme:?} {kind}: shadow must render");
            }
        }
    }

    #[test]
    fn sprite_shadow_stays_anchored_to_the_hotspot() {
        const CANVAS_SIZE: i32 = 128;
        const SPRITE_SIZE: u32 = 24;
        let center = CANVAS_SIZE as f64 / 2.0;
        let sprite = RgbaImage::from_pixel(SPRITE_SIZE, SPRITE_SIZE, image::Rgba([255; 4]));
        let sprite_surface = surface_from_rgba(&sprite);
        let render = |shadow| {
            let mut surface =
                ImageSurface::create(Format::ARgb32, CANVAS_SIZE, CANVAS_SIZE).unwrap();
            {
                let cr = Context::new(&surface).unwrap();
                paint_sprite(
                    &cr,
                    &sprite_surface,
                    center,
                    center,
                    SPRITE_SIZE as f64 / 2.0,
                    SPRITE_SIZE as f64 / 2.0,
                    1.0,
                    shadow,
                    1.0,
                    0.0,
                    1.0,
                );
            }
            surface.flush();
            let pixels = surface.data().unwrap().to_vec();
            pixels
        };
        let foreground = render(0.0);
        let shadowed = render(1.0);
        let foreground_bounds =
            alpha_bounds_with_size(&foreground, CANVAS_SIZE as usize, CANVAS_SIZE as usize)
                .unwrap();
        let shadowed_bounds =
            alpha_bounds_with_size(&shadowed, CANVAS_SIZE as usize, CANVAS_SIZE as usize).unwrap();

        assert_eq!(shadowed_bounds.0, foreground_bounds.0);
        assert_eq!(shadowed_bounds.1, foreground_bounds.1);
        assert!(
            (1..=5).contains(&(shadowed_bounds.2 as isize - foreground_bounds.2 as isize))
                && (1..=5)
                    .contains(&(shadowed_bounds.3 as isize - foreground_bounds.3 as isize)),
            "shadow should extend the sprite down and right: foreground {foreground_bounds:?}, shadowed {shadowed_bounds:?}"
        );
    }

    #[test]
    fn centered_theme_previews_follow_size_and_shadow_for_every_theme() {
        for theme in CursorTheme::ALL {
            let small_settings = CursorSettings {
                theme,
                size: MIN_CURSOR_SIZE,
                shadow: 0.0,
                ..CursorSettings::default()
            };
            let large_settings = CursorSettings {
                theme,
                size: MAX_CURSOR_SIZE,
                shadow: 0.0,
                ..CursorSettings::default()
            };
            let small = render_centered_preview(small_settings, 72, 56);
            let large = render_centered_preview(large_settings, 72, 56);
            let shadowed = render_centered_preview(
                CursorSettings {
                    theme,
                    size: MIN_CURSOR_SIZE,
                    shadow: 1.0,
                    ..CursorSettings::default()
                },
                72,
                56,
            );
            assert_ne!(small, large, "{theme:?}: size must change its preview");
            assert_ne!(small, shadowed, "{theme:?}: shadow must change its preview");
            for (size, pixels) in [(MIN_CURSOR_SIZE, &small), (MAX_CURSOR_SIZE, &large)] {
                let (min_x, min_y, max_x, max_y) = alpha_bounds_with_size(pixels, 72, 56).unwrap();
                assert!(
                    (((min_x + max_x) as f64 / 2.0) - 35.5).abs() <= 2.0,
                    "{theme:?} size {size}: preview artwork must stay centered horizontally"
                );
                assert!(
                    (((min_y + max_y) as f64 / 2.0) - 27.5).abs() <= 2.0,
                    "{theme:?} size {size}: preview artwork must stay centered vertically"
                );
            }

            for (width, height) in [(72, 56), (240, 88)] {
                let max_size_shadow = render_centered_preview(
                    CursorSettings {
                        theme,
                        size: MAX_CURSOR_SIZE,
                        shadow: 1.0,
                        ..CursorSettings::default()
                    },
                    width,
                    height,
                );
                let max_size = render_centered_preview(
                    CursorSettings {
                        theme,
                        size: MAX_CURSOR_SIZE,
                        shadow: 0.0,
                        ..CursorSettings::default()
                    },
                    width,
                    height,
                );
                assert_ne!(
                    max_size_shadow, max_size,
                    "{theme:?}: maximum shadow must remain visible in {width}x{height}"
                );
                let (min_x, min_y, max_x, max_y) =
                    alpha_bounds_with_size(&max_size_shadow, width as usize, height as usize)
                        .unwrap();
                assert!(
                    min_x > 0
                        && min_y > 0
                        && max_x + 1 < width as usize
                        && max_y + 1 < height as usize,
                    "{theme:?}: maximum size and shadow must fit in {width}x{height}"
                );
            }
        }
    }

    #[test]
    fn click_effects_are_distinct_and_off_is_transparent() {
        let render = |click_effect| {
            render_click(CursorSettings {
                click_effect,
                ..CursorSettings::default()
            })
        };
        let off = render(ClickEffect::None);
        let spotlight = render(ClickEffect::Spotlight);
        let ripple = render(ClickEffect::Ripple);
        let circle = render(ClickEffect::Circle);
        let echo = render(ClickEffect::Echo);

        assert!(off.iter().all(|byte| *byte == 0));
        assert!(spotlight.iter().any(|byte| *byte != 0));
        assert!(ripple.iter().any(|byte| *byte != 0));
        assert!(circle.iter().any(|byte| *byte != 0));
        assert!(echo.iter().any(|byte| *byte != 0));
        assert_ne!(spotlight, ripple);
        assert_ne!(spotlight, echo);
        assert_ne!(ripple, echo);
        assert_ne!(ripple, circle);
    }

    #[test]
    fn a_click_on_a_frame_edge_still_paints() {
        for (x, y) in [
            (0.0, 0.0),
            (TEST_SURFACE_SIZE as f64, 0.0),
            (0.0, TEST_SURFACE_SIZE as f64),
            (TEST_SURFACE_SIZE as f64, TEST_SURFACE_SIZE as f64),
        ] {
            for effect in [
                ClickEffect::Ripple,
                ClickEffect::Circle,
                ClickEffect::Spotlight,
                ClickEffect::Echo,
            ] {
                let pixels = render_click_at(
                    CursorSettings {
                        click_effect: effect,
                        ..CursorSettings::default()
                    },
                    x,
                    y,
                );
                assert!(
                    pixels.iter().any(|byte| *byte != 0),
                    "{effect:?} at ({x}, {y}) should still paint"
                );
            }
        }
    }

    #[test]
    fn circle_size_scales_with_the_video_width() {
        let render = |reference_width| {
            render_click_with_width(
                CursorSettings {
                    click_effect: ClickEffect::Circle,
                    click_scale: 1.0,
                    click_opacity: 1.0,
                    ..CursorSettings::default()
                },
                TEST_SURFACE_SIZE as f64 / 2.0,
                TEST_SURFACE_SIZE as f64 / 2.0,
                reference_width,
            )
        };
        let small = render(200.0);
        let large = render(400.0);
        let (small_min, _, small_max, _) = alpha_bounds(&small).unwrap();
        let (large_min, _, large_max, _) = alpha_bounds(&large).unwrap();
        assert!(
            large_max - large_min > small_max - small_min,
            "a wider reference should draw a larger circle"
        );
    }

    #[test]
    fn studied_effects_ignore_the_local_size_opacity_and_intensity_knobs() {
        for effect in [ClickEffect::Ripple, ClickEffect::Circle] {
            let render = |scale, opacity, intensity| {
                render_click(CursorSettings {
                    click_effect: effect,
                    click_scale: scale,
                    click_opacity: opacity,
                    click_intensity: intensity,
                    ..CursorSettings::default()
                })
            };
            assert_eq!(
                render(0.5, 0.0, 0.0),
                render(2.0, 1.0, 1.0),
                "{effect:?} must use the studied fixed size and opacity"
            );
        }
    }

    #[test]
    fn legacy_effects_use_the_local_size_opacity_and_intensity_knobs() {
        for effect in [ClickEffect::Spotlight, ClickEffect::Echo] {
            let render = |scale, opacity, intensity| {
                render_click(CursorSettings {
                    click_effect: effect,
                    click_scale: scale,
                    click_opacity: opacity,
                    click_intensity: intensity,
                    ..CursorSettings::default()
                })
            };
            assert_ne!(
                render(0.5, 1.0, 1.0),
                render(2.0, 1.0, 1.0),
                "{effect:?} size"
            );
            assert_ne!(
                render(1.0, 0.0, 1.0),
                render(1.0, 1.0, 1.0),
                "{effect:?} opacity"
            );
            assert_ne!(
                render(1.0, 1.0, 0.0),
                render(1.0, 1.0, 1.0),
                "{effect:?} intensity"
            );
        }
    }

    #[test]
    fn click_colour_affects_only_the_legacy_effects() {
        let render = |effect, color| {
            render_click(CursorSettings {
                click_effect: effect,
                click_color: color,
                ..CursorSettings::default()
            })
        };
        for effect in [ClickEffect::Spotlight, ClickEffect::Echo] {
            assert_ne!(
                render(effect, (255, 255, 255)),
                render(effect, (255, 0, 0)),
                "colour should affect {effect:?}"
            );
        }
        for effect in [ClickEffect::Ripple, ClickEffect::Circle] {
            assert_eq!(
                render(effect, (255, 255, 255)),
                render(effect, (255, 0, 0)),
                "the studied {effect:?} has no colour knob"
            );
        }
    }

    #[test]
    fn tahoe_hotspots_land_on_visible_artwork() {
        for theme in [CursorTheme::Tahoe, CursorTheme::TahoeInverted] {
            for (kind, expected) in [
                ("default", (7.9, 4.1)),
                ("hand", (21.0, 5.0)),
                ("text", (24.0, 24.0)),
                ("crosshair", (24.1, 24.0)),
            ] {
                let image = bitmap(theme, kind);
                let actual = hotspot(theme, kind);
                assert_eq!(actual, expected, "{theme:?} {kind}");
                assert_eq!((image.width(), image.height()), (48, 48));
                assert!(
                    image.get_pixel(actual.0 as u32, actual.1 as u32).0[3] > 0,
                    "{theme:?} {kind} hotspot must touch the visible artwork"
                );
            }
        }
    }

    #[test]
    fn tahoe_inverted_hand_and_text_keep_the_same_shape() {
        for kind in ["hand", "text"] {
            let regular = bitmap(CursorTheme::Tahoe, kind);
            let inverted = bitmap(CursorTheme::TahoeInverted, kind);
            assert_eq!(regular.dimensions(), (48, 48));
            assert_eq!(inverted.dimensions(), regular.dimensions());
            let (_, _, width, height) = opaque_bounds(regular);
            if kind == "text" {
                assert!((17.0..=22.0).contains(&width));
                assert!((40.0..=43.0).contains(&height));
            } else {
                assert!((31.0..=36.0).contains(&width));
                assert!((38.0..=40.0).contains(&height));
            }
            for (light, dark) in regular.pixels().zip(inverted.pixels()) {
                assert_eq!(light.0[3], dark.0[3], "{kind} alpha mask");
            }
            let sample = if kind == "text" { (24, 24) } else { (20, 20) };
            let light = regular.get_pixel(sample.0, sample.1);
            let dark = inverted.get_pixel(sample.0, sample.1);
            assert_eq!(light.0[3], 255, "{kind} light fill");
            assert_eq!(dark.0[3], 255, "{kind} dark fill");
            assert!(light.0[0] < 16 && dark.0[0] > 239, "{kind} inverted fill");
        }
    }

    #[test]
    fn dot_art_is_a_translucent_circle_with_a_subtle_rim() {
        let image = bitmap(CursorTheme::Dot, "default");
        assert_eq!(image.dimensions(), (48, 48));
        let (_, _, width, height) = opaque_bounds(image);
        assert!((40.0..=44.0).contains(&width));
        assert!((40.0..=44.0).contains(&height));

        let center = image.get_pixel(24, 24);
        assert!((100..=130).contains(&center.0[3]));
        assert!((190..=220).contains(&center.0[0]));
        assert_eq!(image.get_pixel(0, 24).0[3], 0);

        let rim = image.get_pixel(4, 24);
        assert!(rim.0[0] > 160 && rim.0[0] < center.0[0]);
    }

    #[test]
    fn dot_kinds_share_one_centered_sprite() {
        let default = bitmap(CursorTheme::Dot, "default");
        assert_eq!(hotspot(CursorTheme::Dot, "default"), (24.0, 24.0));
        for kind in ["hand", "text", "crosshair"] {
            assert_eq!(bitmap(CursorTheme::Dot, kind).as_raw(), default.as_raw());
            assert_eq!(hotspot(CursorTheme::Dot, kind), (24.0, 24.0));
        }
    }

    #[test]
    fn dot_shadow_changes_the_exterior_without_darkening_the_fill() {
        let render = |shadow| {
            let mut surface =
                ImageSurface::create(Format::ARgb32, TEST_SURFACE_SIZE, TEST_SURFACE_SIZE).unwrap();
            {
                let cr = Context::new(&surface).unwrap();
                cr.set_source_rgb(1.0, 1.0, 1.0);
                let _ = cr.paint();
                draw(
                    &cr,
                    TEST_SURFACE_SIZE as f64 / 2.0,
                    TEST_SURFACE_SIZE as f64 / 2.0,
                    1.0,
                    1.0,
                    "default",
                    CursorSettings {
                        theme: CursorTheme::Dot,
                        shadow,
                        ..CursorSettings::default()
                    },
                    1.0,
                );
            }
            surface.flush();
            let stride = surface.stride() as usize;
            let pixels = surface.data().unwrap().to_vec();
            (pixels, stride)
        };
        let center = (TEST_SURFACE_SIZE / 2) as usize;
        let read_pixel = |pixels: &[u8], stride, x, y| {
            let index = y * stride + x * 4;
            [
                pixels[index + 2],
                pixels[index + 1],
                pixels[index],
                pixels[index + 3],
            ]
        };
        let (no_shadow, stride) = render(0.0);
        let (default_shadow, _) = render(0.4);
        let (full_shadow, _) = render(1.0);
        assert_eq!(
            read_pixel(&no_shadow, stride, center, center),
            read_pixel(&default_shadow, stride, center, center)
        );
        assert_eq!(
            read_pixel(&no_shadow, stride, center, center),
            read_pixel(&full_shadow, stride, center, center)
        );
        let outside = (center, center + 22);
        let no_shadow_edge = read_pixel(&no_shadow, stride, outside.0, outside.1);
        let default_shadow_edge = read_pixel(&default_shadow, stride, outside.0, outside.1);
        let full_shadow_edge = read_pixel(&full_shadow, stride, outside.0, outside.1);
        assert_ne!(no_shadow_edge, default_shadow_edge);
        assert_ne!(default_shadow_edge, full_shadow_edge);
        assert!(full_shadow_edge[0] < default_shadow_edge[0]);
    }
}
