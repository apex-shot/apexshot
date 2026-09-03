use super::model::{ClickEffect, CursorSettings, CursorTheme, MAX_CURSOR_SIZE, MIN_CURSOR_SIZE};
use gtk4::cairo::{Antialias, Context, Filter, Format, ImageSurface, SurfacePattern};
use image::RgbaImage;
use std::f64::consts::TAU;
use std::path::Path;
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
const FIGMA_DEFAULT: &[u8] = include_bytes!("../../../assets/cursors/figma/default.png");
const FIGMA_HAND: &[u8] = include_bytes!("../../../assets/cursors/figma/hand.png");
const FIGMA_TEXT: &[u8] = include_bytes!("../../../assets/cursors/figma/text.png");
const FIGMA_CROSSHAIR: &[u8] = include_bytes!("../../../assets/cursors/figma/crosshair.png");

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
        (CursorTheme::Tahoe, SpriteKind::Hand) => (15.8, 5.1),
        (CursorTheme::Tahoe, SpriteKind::Text) => (24.3, 40.1),
        (CursorTheme::Tahoe, SpriteKind::Crosshair) => (24.1, 24.0),
        (CursorTheme::TahoeInverted, SpriteKind::Default) => (7.9, 4.1),
        (CursorTheme::TahoeInverted, SpriteKind::Hand) => (15.8, 5.1),
        (CursorTheme::TahoeInverted, SpriteKind::Text) => (24.3, 40.1),
        (CursorTheme::TahoeInverted, SpriteKind::Crosshair) => (24.1, 24.0),
        (CursorTheme::Dot, SpriteKind::Default) => (24.0, 24.0),
        (CursorTheme::Dot, SpriteKind::Hand) => (24.0, 24.0),
        (CursorTheme::Dot, SpriteKind::Text) => (24.0, 24.0),
        (CursorTheme::Dot, SpriteKind::Crosshair) => (24.0, 24.0),
        (CursorTheme::Figma, SpriteKind::Default) => (7.0, 3.0),
        (CursorTheme::Figma, SpriteKind::Hand) => (7.0, 3.0),
        (CursorTheme::Figma, SpriteKind::Text) => (7.0, 3.0),
        (CursorTheme::Figma, SpriteKind::Crosshair) => (7.0, 3.0),
    }
}

pub fn draw(
    cr: &Context,
    x: f64,
    y: f64,
    pulse: f64,
    kind: &str,
    settings: CursorSettings,
    alpha: f64,
) {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha < 0.02 {
        return;
    }
    let settings = settings.clamped();
    let scale = overlay_scale(settings.size, 1.0) * pulse.max(0.7);
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
    let bitmap = bitmap(settings.theme, kind);
    let surface = surface_from_rgba(bitmap);
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
    );
}

pub fn draw_tilted(
    cr: &Context,
    x: f64,
    y: f64,
    pulse: f64,
    kind: &str,
    settings: CursorSettings,
    alpha: f64,
    tilt: f64,
) {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha < 0.02 {
        return;
    }
    let settings = settings.clamped();
    let scale = overlay_scale(settings.size, 1.0) * pulse.max(0.7);
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
    let bitmap = bitmap(settings.theme, kind);
    let surface = surface_from_rgba(bitmap);
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
        tilt,
    );
}

pub fn draw_click(
    cr: &Context,
    x: f64,
    y: f64,
    progress: f64,
    settings: CursorSettings,
    alpha: f64,
) {
    let settings = settings.clamped();
    let alpha = (alpha * settings.click_opacity).clamp(0.0, 1.0);
    match settings.click_effect {
        ClickEffect::None => {}
        ClickEffect::Spotlight => draw_spotlight(
            cr,
            x,
            y,
            progress,
            settings.size,
            settings.click_intensity,
            alpha,
            settings.click_color,
            settings.click_scale,
        ),
        ClickEffect::Ripple => draw_ripple(
            cr,
            x,
            y,
            progress,
            settings.size,
            settings.click_intensity,
            alpha,
            settings.click_color,
            settings.click_scale,
        ),
        ClickEffect::Echo => draw_echo(
            cr,
            x,
            y,
            progress,
            settings.size,
            settings.click_intensity,
            alpha,
            settings.click_color,
            settings.click_scale,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_ripple(
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
    let radius = (10.0 + 34.0 * progress) * overlay_scale(size, 1.0) * scale.max(0.01);
    cr.set_line_width((2.4 * (1.0 - progress * 0.45)).clamp(1.1, 2.4));
    cr.set_source_rgba(r, g, b, 0.82 * amount);
    cr.arc(x, y, radius, 0.0, TAU);
    let _ = cr.stroke();
    cr.set_source_rgba(r, g, b, 0.16 * amount);
    cr.arc(x, y, radius * 0.55, 0.0, TAU);
    let _ = cr.fill();
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

pub fn draw_centered(cr: &Context, width: f64, height: f64, theme: CursorTheme) {
    let bitmap = bitmap(theme, "default");
    let (bx, by, bw, bh) = opaque_bounds(bitmap);
    if bw <= 0.0 || bh <= 0.0 {
        return;
    }
    let scale = ((width / bw).min(height / bh) * 0.78).min(1.35);
    let x = (width - bw * scale) * 0.5 - bx * scale;
    let y = (height - bh * scale) * 0.5 - by * scale;
    let surface = surface_from_rgba(bitmap);
    let _ = cr.save();
    cr.translate(x, y);
    cr.scale(scale, scale);
    let pattern = SurfacePattern::create(&surface);
    pattern.set_filter(Filter::Best);
    let _ = cr.set_source(&pattern);
    cr.set_antialias(Antialias::Best);
    let _ = cr.paint();
    let _ = cr.restore();
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
        draw(&cr, out_hot.0, out_hot.1, 1.0, kind, settings, 1.0);
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
) {
    let _ = cr.save();
    cr.translate(x, y);
    if tilt.abs() > 0.001 {
        cr.rotate(tilt);
    }
    if shadow > 0.01 {
        let _ = cr.save();
        cr.translate(2.2 * scale * shadow, 3.2 * scale * shadow);
        cr.scale(scale, scale);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.55 * shadow * alpha);
        let _ = cr.mask_surface(surface, -hx, -hy);
        let _ = cr.restore();
    }
    cr.scale(scale, scale);
    let pattern = SurfacePattern::create(surface);
    pattern.set_filter(Filter::Best);
    let _ = cr.set_source(&pattern);
    cr.set_antialias(Antialias::Best);
    cr.translate(-hx, -hy);
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
        (CursorTheme::Figma, SpriteKind::Default) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(FIGMA_DEFAULT))
        }
        (CursorTheme::Figma, SpriteKind::Hand) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(FIGMA_HAND))
        }
        (CursorTheme::Figma, SpriteKind::Text) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(FIGMA_TEXT))
        }
        (CursorTheme::Figma, SpriteKind::Crosshair) => {
            static IMG: OnceLock<RgbaImage> = OnceLock::new();
            IMG.get_or_init(|| decode(FIGMA_CROSSHAIR))
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
    use crate::recording::editor::model::{MAX_CLICK_SCALE, MIN_CLICK_SCALE};

    const TEST_SURFACE_SIZE: i32 = 192;

    fn render_click(settings: CursorSettings) -> Vec<u8> {
        let mut surface =
            ImageSurface::create(Format::ARgb32, TEST_SURFACE_SIZE, TEST_SURFACE_SIZE).unwrap();
        {
            let cr = Context::new(&surface).unwrap();
            draw_click(
                &cr,
                TEST_SURFACE_SIZE as f64 / 2.0,
                TEST_SURFACE_SIZE as f64 / 2.0,
                0.25,
                settings,
                1.0,
            );
        }
        surface.flush();
        let pixels = surface.data().unwrap().to_vec();
        pixels
    }

    fn render_cursor(settings: CursorSettings) -> Vec<u8> {
        let mut surface =
            ImageSurface::create(Format::ARgb32, TEST_SURFACE_SIZE, TEST_SURFACE_SIZE).unwrap();
        {
            let cr = Context::new(&surface).unwrap();
            draw(
                &cr,
                TEST_SURFACE_SIZE as f64 / 2.0,
                TEST_SURFACE_SIZE as f64 / 2.0,
                1.0,
                "default",
                settings,
                1.0,
            );
        }
        surface.flush();
        let pixels = surface.data().unwrap().to_vec();
        pixels
    }

    fn alpha_bounds(pixels: &[u8]) -> Option<(usize, usize, usize, usize)> {
        let size = TEST_SURFACE_SIZE as usize;
        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for y in 0..size {
            for x in 0..size {
                if pixels[(y * size + x) * 4 + 3] == 0 {
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
        let echo = render(ClickEffect::Echo);

        assert!(off.iter().all(|byte| *byte == 0));
        assert!(spotlight.iter().any(|byte| *byte != 0));
        assert!(ripple.iter().any(|byte| *byte != 0));
        assert!(echo.iter().any(|byte| *byte != 0));
        assert_ne!(spotlight, ripple);
        assert_ne!(spotlight, echo);
        assert_ne!(ripple, echo);
    }

    #[test]
    fn click_color_scale_opacity_and_intensity_affect_rendering() {
        let red = render_click(CursorSettings {
            click_color: (255, 0, 0),
            ..CursorSettings::default()
        });
        let blue = render_click(CursorSettings {
            click_color: (0, 0, 255),
            ..CursorSettings::default()
        });
        let red_total: usize = red.chunks_exact(4).map(|pixel| pixel[2] as usize).sum();
        let red_blue_total: usize = red.chunks_exact(4).map(|pixel| pixel[0] as usize).sum();
        let blue_total: usize = blue.chunks_exact(4).map(|pixel| pixel[0] as usize).sum();
        let blue_red_total: usize = blue.chunks_exact(4).map(|pixel| pixel[2] as usize).sum();
        assert!(red_total > red_blue_total);
        assert!(blue_total > blue_red_total);

        let small = render_click(CursorSettings {
            click_scale: MIN_CLICK_SCALE,
            ..CursorSettings::default()
        });
        let large = render_click(CursorSettings {
            click_scale: MAX_CLICK_SCALE,
            ..CursorSettings::default()
        });
        let (small_min_x, _, small_max_x, _) = alpha_bounds(&small).unwrap();
        let (large_min_x, _, large_max_x, _) = alpha_bounds(&large).unwrap();
        assert!(large_max_x - large_min_x > small_max_x - small_min_x);

        let transparent = render_click(CursorSettings {
            click_opacity: 0.0,
            ..CursorSettings::default()
        });
        let no_intensity = render_click(CursorSettings {
            click_intensity: 0.0,
            ..CursorSettings::default()
        });
        assert!(transparent.iter().all(|byte| *byte == 0));
        assert!(no_intensity.iter().all(|byte| *byte == 0));
    }
}
