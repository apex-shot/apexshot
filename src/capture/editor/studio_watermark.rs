//! The Studio preview mark: a small "Made with ApexShot" signature painted on
//! the canvas while an unlicensed document uses a Studio style. It is never
//! part of an export; the export gate blocks unlicensed styled output.

use gtk4::cairo::{Context, Format, ImageSurface, LineJoin};
use gtk4::pango;

use super::render::rounded_rect_path;

/// Always the product name, never the build's display name (dev builds add a
/// suffix there).
pub(crate) const BRAND: &str = "ApexShot";
const FAMILY: &str = "DM Sans";

// Proportions of the mark height `H`.
const LOGO: f64 = 0.92;
const GAP: f64 = 0.28;
const SMALL_SIZE: f64 = 0.27;
const SMALL_BASELINE: f64 = 0.38;
const BRAND_SIZE: f64 = 0.44;
const BRAND_BASELINE: f64 = 0.90;
const DARK_INK_ABOVE: f64 = 0.62;

/// Where the mark sits, in the coordinates of whatever context paints it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MarkRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The mark overlaps the screenshot rather than sitting on the wallpaper.
    pub on_card: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ink {
    pub rgb: (f64, f64, f64),
    pub alpha: f64,
    pub shadow_rgb: (f64, f64, f64),
    pub shadow: f64,
}

impl Ink {
    /// White ink with a dark shadow, for dark and mid backgrounds.
    pub(crate) fn light() -> Self {
        Self {
            rgb: (1.0, 1.0, 1.0),
            alpha: 0.95,
            shadow_rgb: (0.0, 0.0, 0.0),
            shadow: 0.70,
        }
    }

    /// Near-black ink with a light shadow, for pale wallpapers.
    pub(crate) fn dark() -> Self {
        Self {
            rgb: (0.114, 0.129, 0.161),
            alpha: 0.94,
            shadow_rgb: (1.0, 1.0, 1.0),
            shadow: 0.70,
        }
    }
}

/// A rectangle in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ScreenRect {
    fn right(self) -> f64 {
        self.x + self.width
    }

    fn bottom(self) -> f64 {
        self.y + self.height
    }

    pub(crate) fn intersect(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then_some(Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }
}

/// What placement needs, in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Stage {
    /// The whole composition (wallpaper included).
    pub canvas: ScreenRect,
    /// The screenshot card inside it.
    pub image: ScreenRect,
    /// The canvas widget's content area; anything outside is not visible.
    pub visible: ScreenRect,
}

const SCREEN_HEIGHT_OF_CANVAS: f64 = 0.080;
const SCREEN_MIN_HEIGHT: f64 = 32.0;
const SCREEN_MAX_HEIGHT: f64 = 84.0;
const MIN_REGION: f64 = 24.0;

/// Place the mark in the centre of the card, kept inside what is visible of
/// the canvas. `None` when too little of the canvas is visible.
pub(crate) fn place(stage: Stage, text_ratio: f64) -> Option<MarkRect> {
    let region = stage.canvas.intersect(stage.visible)?;
    if region.width < MIN_REGION || region.height < MIN_REGION {
        return None;
    }
    let mut height =
        (SCREEN_HEIGHT_OF_CANVAS * stage.canvas.height).clamp(SCREEN_MIN_HEIGHT, SCREEN_MAX_HEIGHT);
    let per_height = LOGO + GAP + text_ratio;
    let room = region.width * 0.9;
    if height * per_height > room {
        height *= (room / (height * per_height)).clamp(0.6, 1.0);
    }
    let width = height * per_height;
    let image = stage.image;
    let x = (image.x + (image.width - width) / 2.0)
        .min(region.right() - width)
        .max(region.x);
    let y = (image.y + (image.height - height) / 2.0)
        .min(region.bottom() - height)
        .max(region.y);
    Some(MarkRect {
        x,
        y,
        width,
        height,
        on_card: true,
    })
}

/// The wider text line's width divided by the mark height, measured once at
/// a reference height. Text width scales linearly with the font size.
pub(crate) fn text_ratio(context: &Context) -> f64 {
    const REFERENCE: f64 = 100.0;
    let small = text_layout(
        context,
        "Made with",
        REFERENCE * SMALL_SIZE,
        pango::Weight::Semibold,
    );
    let brand = text_layout(context, BRAND, REFERENCE * BRAND_SIZE, pango::Weight::Bold);
    let width = small
        .pixel_extents()
        .1
        .width()
        .max(brand.pixel_extents().1.width());
    (f64::from(width) / REFERENCE).max(0.5)
}

/// Ink for a mark over the screenshot: sample the image under it.
/// `image_rect` is where `image` is shown on screen.
pub(crate) fn ink_for_image(
    image: &image::RgbaImage,
    mark: MarkRect,
    image_rect: ScreenRect,
) -> Ink {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || image_rect.width <= 0.0 || image_rect.height <= 0.0 {
        return Ink::light();
    }
    let sx = f64::from(width) / image_rect.width;
    let sy = f64::from(height) / image_rect.height;
    let mut total = 0.0;
    let mut count = 0.0;
    for row in 0..5 {
        for column in 0..5 {
            let x = mark.x + mark.width * (f64::from(column) + 0.5) / 5.0;
            let y = mark.y + mark.height * (f64::from(row) + 0.5) / 5.0;
            let ix = ((x - image_rect.x) * sx).floor();
            let iy = ((y - image_rect.y) * sy).floor();
            if ix < 0.0 || iy < 0.0 || ix >= f64::from(width) || iy >= f64::from(height) {
                continue;
            }
            let p = image.get_pixel(ix as u32, iy as u32);
            let a = f64::from(p[3]) / 255.0;
            total += a
                * (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]))
                / 255.0;
            count += a;
        }
    }
    if count > 0.0 && total / count > DARK_INK_ABOVE {
        Ink::dark()
    } else {
        Ink::light()
    }
}

/// Paint the mark at `mark`: a soft drop shadow, the app icon, then the text.
/// The caller clips to the visible canvas.
pub(crate) fn paint(context: &Context, mark: MarkRect, ink: Ink) {
    let h = mark.height;
    if h <= 0.0 {
        return;
    }
    let _ = context.save();
    for (offset, radius, alpha) in [
        (0.06 * h, 0.30 * h, ink.shadow),
        (0.02 * h, 0.06 * h, ink.shadow * 0.55),
    ] {
        if let Some((surface, x, y)) = shadow(context, mark, radius, ink.shadow_rgb, alpha) {
            if context.set_source_surface(&surface, x, y + offset).is_ok() {
                let _ = context.paint();
            }
        }
    }
    icon(context, mark);
    let (r, g, b) = ink.rgb;
    context.set_source_rgba(r, g, b, ink.alpha);
    for (layout, x, y) in text(context, mark) {
        context.move_to(x, y);
        pangocairo::functions::show_layout(context, &layout);
    }
    let _ = context.restore();
}

/// The two text lines and where their layouts start.
fn text(context: &Context, mark: MarkRect) -> [(pango::Layout, f64, f64); 2] {
    let h = mark.height;
    let small = text_layout(
        context,
        "Made with",
        SMALL_SIZE * h,
        pango::Weight::Semibold,
    );
    let brand = text_layout(context, BRAND, BRAND_SIZE * h, pango::Weight::Bold);
    let x = mark.x + (LOGO + GAP) * h;
    let small_top = mark.y + SMALL_BASELINE * h - baseline(&small);
    let brand_top = mark.y + BRAND_BASELINE * h - baseline(&brand);
    [(small, x, small_top), (brand, x, brand_top)]
}

/// The icon's square, in the coordinates `paint` uses.
fn icon_box(mark: MarkRect) -> (f64, f64, f64) {
    let size = LOGO * mark.height;
    (mark.x, mark.y + (mark.height - size) / 2.0, size)
}

/// The ApexShot app icon: an orange rounded square with the white arch, the
/// same geometry as `packaging/apexshot.svg` (its 8..120 box on a 128 grid).
fn icon(context: &Context, mark: MarkRect) {
    let (x, y, size) = icon_box(mark);
    let _ = context.save();
    rounded_rect_path(context, x, y, size, size, size * 24.0 / 112.0);
    context.set_source_rgb(233.0 / 255.0, 84.0 / 255.0, 32.0 / 255.0);
    let _ = context.fill();
    let unit = size / 112.0;
    let point = |px: f64, py: f64| (x + (px - 8.0) * unit, y + (py - 8.0) * unit);
    let (sx, sy) = point(24.0, 96.0);
    context.move_to(sx, sy);
    let (a, b, c) = (point(40.0, 96.0), point(48.0, 20.0), point(64.0, 20.0));
    context.curve_to(a.0, a.1, b.0, b.1, c.0, c.1);
    let (a, b, c) = (point(80.0, 20.0), point(88.0, 96.0), point(104.0, 96.0));
    context.curve_to(a.0, a.1, b.0, b.1, c.0, c.1);
    context.set_source_rgb(1.0, 1.0, 1.0);
    context.set_line_width(14.0 * unit);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(LineJoin::Round);
    let _ = context.stroke();
    let _ = context.restore();
}

/// A blurred silhouette of the whole mark (icon square plus text), tinted
/// `rgb` at `alpha`, and where to paint it.
fn shadow(
    context: &Context,
    mark: MarkRect,
    radius: f64,
    rgb: (f64, f64, f64),
    alpha: f64,
) -> Option<(ImageSurface, f64, f64)> {
    let pad = (radius * 2.0 + 2.0).ceil();
    let width = (mark.width + 2.0 * pad).ceil() as i32;
    let height = (mark.height + 2.0 * pad).ceil() as i32;
    let origin_x = mark.x.floor() - pad;
    let origin_y = mark.y.floor() - pad;
    let mut silhouette = ImageSurface::create(Format::ARgb32, width, height).ok()?;
    {
        let shape = Context::new(&silhouette).ok()?;
        shape.set_source_rgb(0.0, 0.0, 0.0);
        let _ = shape.paint();
        shape.translate(-origin_x, -origin_y);
        shape.set_source_rgb(1.0, 1.0, 1.0);
        let (x, y, size) = icon_box(mark);
        rounded_rect_path(&shape, x, y, size, size, size * 24.0 / 112.0);
        let _ = shape.fill();
        for (layout, x, y) in text(context, mark) {
            shape.move_to(x, y);
            pangocairo::functions::layout_path(&shape, &layout);
        }
        let _ = shape.fill();
    }
    silhouette.flush();
    let stride = usize::try_from(silhouette.stride()).ok()?;
    let mut image = {
        let data = silhouette.data().ok()?;
        super::render::cairo_argb_to_rgba_image(width as u32, height as u32, stride, &data)
    };
    super::render::apply_blur_rect(
        &mut image,
        super::types::Rect {
            x: 0,
            y: 0,
            width,
            height,
        },
        radius.max(0.5),
        false,
    );
    let to_byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    for pixel in image.pixels_mut() {
        // The silhouette is white on black: red carries the coverage.
        let a = f64::from(pixel[0]) / 255.0 * alpha;
        *pixel = image::Rgba([to_byte(rgb.0), to_byte(rgb.1), to_byte(rgb.2), to_byte(a)]);
    }
    let surface = super::render::rgba_image_to_surface(&image)?;
    Some((surface, origin_x, origin_y))
}

fn text_layout(context: &Context, text: &str, size: f64, weight: pango::Weight) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(context);
    let mut font = pango::FontDescription::new();
    font.set_family(FAMILY);
    font.set_weight(weight);
    font.set_absolute_size(size.max(1.0) * f64::from(pango::SCALE));
    layout.set_font_description(Some(&font));
    layout.set_text(text);
    layout
}

fn baseline(layout: &pango::Layout) -> f64 {
    f64::from(layout.baseline()) / f64::from(pango::SCALE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> ScreenRect {
        ScreenRect {
            x,
            y,
            width,
            height,
        }
    }

    /// A 1000x700 canvas on screen with an 880x580 card and a snug 60px wallpaper
    /// margin all round, fully visible.
    fn stage() -> Stage {
        Stage {
            canvas: rect(100.0, 50.0, 1000.0, 700.0),
            image: rect(160.0, 110.0, 880.0, 580.0),
            visible: rect(0.0, 0.0, 1400.0, 900.0),
        }
    }

    #[test]
    fn the_mark_sits_in_the_centre_of_the_card() {
        let stage = stage();
        let mark = place(stage, 2.0).unwrap();
        let centre_x = mark.x + mark.width / 2.0;
        let centre_y = mark.y + mark.height / 2.0;
        assert!((centre_x - (stage.image.x + stage.image.width / 2.0)).abs() < 1e-9);
        assert!((centre_y - (stage.image.y + stage.image.height / 2.0)).abs() < 1e-9);
        assert!(mark.on_card);
    }

    #[test]
    fn the_mark_never_leaves_the_visible_canvas() {
        // Panned so the canvas runs off the left and bottom of the view.
        let mut stage = stage();
        stage.canvas = rect(-400.0, 300.0, 1000.0, 700.0);
        stage.image = rect(-300.0, 400.0, 800.0, 500.0);
        stage.visible = rect(0.0, 0.0, 800.0, 600.0);
        let region = stage.canvas.intersect(stage.visible).unwrap();
        let mark = place(stage, 2.0).unwrap();
        assert!(mark.x >= region.x && mark.x + mark.width <= region.right());
        assert!(mark.y >= region.y && mark.y + mark.height <= region.bottom());
        stage.visible = rect(0.0, 0.0, 10.0, 10.0);
        assert!(place(stage, 2.0).is_none());
    }

    #[test]
    fn painting_lights_only_the_mark() {
        let stage = stage();
        let mark = place(stage, 2.0).unwrap();
        let mut target = ImageSurface::create(Format::ARgb32, 1400, 900).unwrap();
        {
            let context = Context::new(&target).unwrap();
            context.set_source_rgb(0.0, 0.0, 0.0);
            context.paint().unwrap();
            let ratio = text_ratio(&context);
            assert!(ratio > 1.0 && ratio < 5.0, "{ratio}");
            paint(&context, mark, Ink::light());
        }
        target.flush();
        let stride = usize::try_from(target.stride()).unwrap();
        let data = target.data().unwrap();
        let bright = |x: usize, y: usize| data[y * stride + x * 4 + 1] > 128;
        let inside = (mark.y as usize..(mark.y + mark.height) as usize)
            .flat_map(|y| (mark.x as usize..(mark.x + mark.width) as usize).map(move |x| (x, y)))
            .any(|(x, y)| bright(x, y));
        let elsewhere = (0..40)
            .flat_map(|y| (0..1400).map(move |x| (x, y)))
            .any(|(x, y)| bright(x, y));
        assert!(inside, "the mark is painted");
        assert!(!elsewhere, "nothing else is painted");
    }

    #[test]
    fn a_light_screenshot_gets_dark_ink() {
        let image = image::RgbaImage::from_pixel(100, 100, image::Rgba([250, 250, 250, 255]));
        let rect = rect(0.0, 0.0, 200.0, 200.0);
        let mark = MarkRect {
            x: 50.0,
            y: 80.0,
            width: 90.0,
            height: 30.0,
            on_card: true,
        };
        assert_eq!(ink_for_image(&image, mark, rect), Ink::dark());
        let image = image::RgbaImage::from_pixel(100, 100, image::Rgba([20, 20, 30, 255]));
        assert_eq!(ink_for_image(&image, mark, rect), Ink::light());
    }

    #[test]
    fn the_icon_is_orange() {
        let mark = MarkRect {
            x: 10.0,
            y: 10.0,
            width: 200.0,
            height: 60.0,
            on_card: false,
        };
        let mut target = ImageSurface::create(Format::ARgb32, 260, 100).unwrap();
        {
            let context = Context::new(&target).unwrap();
            paint(&context, mark, Ink::light());
        }
        target.flush();
        let stride = usize::try_from(target.stride()).unwrap();
        let data = target.data().unwrap();
        // A point inside the icon square, clear of the white arch: near its
        // lower-left corner region above the arch's left foot.
        let (x, y) = (10 + 8, 10 + 12);
        let i = y * stride + x * 4;
        let (b, g, r) = (data[i], data[i + 1], data[i + 2]);
        assert!(r > 200 && g < 120 && b < 80, "rgb {r} {g} {b}");
    }

    #[test]
    fn the_shadow_is_soft_and_shaped_not_a_box() {
        let mark = MarkRect {
            x: 40.0,
            y: 40.0,
            width: 200.0,
            height: 50.0,
            on_card: false,
        };
        let mut target = ImageSurface::create(Format::ARgb32, 320, 140).unwrap();
        {
            let context = Context::new(&target).unwrap();
            let (surface, x, y) =
                shadow(&context, mark, 11.0, (0.0, 0.0, 0.0), 0.5).expect("shadow renders");
            context.set_source_surface(&surface, x, y).unwrap();
            context.paint().unwrap();
        }
        target.flush();
        let stride = usize::try_from(target.stride()).unwrap();
        let data = target.data().unwrap();
        let alpha = |x: usize, y: usize| data[y * stride + x * 4 + 3];
        // Under the icon square: dense shadow.
        assert!(alpha(60, 65) > 60, "{}", alpha(60, 65));
        // Far right of the mark rect, past the text: almost nothing.
        assert!(alpha(236, 44) < 25, "{}", alpha(236, 44));
        // Outside the rect but within the blur: a soft falloff, not zero and not a box edge.
        let edge = alpha(30, 65);
        assert!(edge > 0 && edge < alpha(60, 65), "{edge}");
    }
}
