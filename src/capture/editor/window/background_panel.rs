//! Background assets and the shared Background/Appearance inspector.
//!
//! Asset lookup and decoding here is shared by the static canvas render,
//! export, and the Motion Appearance panel. The static image editor's
//! Background tool *is* the Motion Appearance builder bound to the shared
//! MotionSession runtime; `sync_motion_appearance_to_static` mirrors that
//! runtime into EditorState for static preview/export.

use gtk4::{ApplicationWindow, Box as GtkBox, DrawingArea};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use super::super::state::EditorState;
use super::super::types::{BackgroundStyle, CropAspectRatio, DrawColor};
use crate::recording::editor::model::{
    MotionAppearance, MotionBackgroundFillType, MotionFrame, MotionFramePreset,
};

pub(super) const BACKGROUND_SIDEBAR_WIDTH: i32 = 210;
pub(super) const MAX_BACKGROUND_DIMENSION: u32 = 2560;
/// Long-edge bound for background surfaces drawn on screen. The canvas is
/// screen-resolution; export reloads full resolution for itself.
pub(super) const PREVIEW_BACKGROUND_MAX_EDGE: u32 = 1920;
pub const BACKGROUND_GRADIENT_PREVIEW_FILES: [&str; 20] = [
    "gradient-01.jpg",
    "gradient-02.jpg",
    "gradient-03.jpg",
    "gradient-04.jpg",
    "gradient-05.jpg",
    "gradient-06.jpg",
    "gradient-07.jpg",
    "gradient-08.jpg",
    "gradient-09.jpg",
    "gradient-10.jpg",
    "gradient-11.jpg",
    "gradient-12.jpg",
    "gradient-13.jpg",
    "gradient-14.jpg",
    "gradient-15.jpg",
    "gradient-16.jpg",
    "gradient-17.jpg",
    "gradient-18.jpg",
    "gradient-19.jpg",
    "gradient-20.jpg",
];

/// Built-in Motion wallpaper catalog. The first ten entries preserve the
/// existing ApexShot backgrounds; the remaining entries are the curated image
/// collection.
pub const MOTION_WALLPAPER_FILES: [&str; 70] = [
    "gradient-01.jpg",
    "gradient-02.jpg",
    "gradient-03.jpg",
    "gradient-04.jpg",
    "gradient-05.jpg",
    "gradient-06.jpg",
    "gradient-07.jpg",
    "gradient-08.jpg",
    "gradient-09.jpg",
    "gradient-10.jpg",
    "wallpaper-001.jpg",
    "wallpaper-002.jpg",
    "wallpaper-003.jpg",
    "wallpaper-004.jpg",
    "wallpaper-005.jpg",
    "wallpaper-006.jpg",
    "wallpaper-007.jpg",
    "wallpaper-008.jpg",
    "wallpaper-009.jpg",
    "wallpaper-010.jpg",
    "wallpaper-011.jpg",
    "wallpaper-012.jpg",
    "wallpaper-013.jpg",
    "wallpaper-014.jpg",
    "wallpaper-015.jpg",
    "wallpaper-016.jpg",
    "wallpaper-017.jpg",
    "wallpaper-018.jpg",
    "wallpaper-019.jpg",
    "wallpaper-020.jpg",
    "wallpaper-021.jpg",
    "wallpaper-022.jpg",
    "wallpaper-023.jpg",
    "wallpaper-024.jpg",
    "wallpaper-025.jpg",
    "wallpaper-026.jpg",
    "wallpaper-027.jpg",
    "wallpaper-028.jpg",
    "wallpaper-029.jpg",
    "wallpaper-030.jpg",
    "wallpaper-031.jpg",
    "wallpaper-032.jpg",
    "wallpaper-033.jpg",
    "wallpaper-034.jpg",
    "wallpaper-035.jpg",
    "wallpaper-036.jpg",
    "wallpaper-037.jpg",
    "wallpaper-038.jpg",
    "wallpaper-039.jpg",
    "wallpaper-040.jpg",
    "wallpaper-041.jpg",
    "wallpaper-042.jpg",
    "wallpaper-043.jpg",
    "wallpaper-044.jpg",
    "wallpaper-045.jpg",
    "wallpaper-046.jpg",
    "wallpaper-047.jpg",
    "wallpaper-048.jpg",
    "wallpaper-049.jpg",
    "wallpaper-050.jpg",
    "wallpaper-051.jpg",
    "wallpaper-052.jpg",
    "wallpaper-053.jpg",
    "wallpaper-054.jpg",
    "wallpaper-055.jpg",
    "wallpaper-056.jpg",
    "wallpaper-057.jpg",
    "wallpaper-058.jpg",
    "wallpaper-059.jpg",
    "wallpaper-060.jpg",
];
pub fn background_gradient_asset_path(file_name: &str) -> PathBuf {
    let asset_paths = [
        std::env::current_dir()
            .unwrap_or_default()
            .join("src/capture/editor/background-images")
            .join(file_name),
        std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.parent()
                    .map(|dir| dir.join("background-images").join(file_name))
            })
            .unwrap_or_default(),
        PathBuf::from("/usr/share/apexshot/background-images").join(file_name),
        PathBuf::from("/usr/local/share/apexshot/background-images").join(file_name),
    ];

    asset_paths
        .into_iter()
        .find(|path| !path.as_os_str().is_empty() && path.exists())
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .join("src/capture/editor/background-images")
                .join(file_name)
        })
}

/// Locate the small, bundled preview for a Motion wallpaper. Both the legacy
/// ApexShot gradients and imported wallpapers have dedicated thumbnails, so
/// expanding the catalog never decodes a full-size background on the UI
/// thread.
pub fn motion_wallpaper_preview_asset_path(file_name: &str) -> PathBuf {
    let preview_file_name = file_name
        .strip_prefix("wallpaper-")
        .map(|suffix| format!("wallpaper-thumb-{suffix}"))
        .or_else(|| {
            file_name
                .strip_prefix("gradient-")
                .map(|suffix| format!("gradient-thumb-{suffix}"))
        })
        .unwrap_or_else(|| file_name.to_owned());
    background_gradient_asset_path(&preview_file_name)
}

/// First bundled Motion wallpaper that exists on disk. Available as an
/// explicit picker choice; it is never auto-applied so Motion looks like
/// Static on entry instead of gaining a background Static never had.
pub fn default_motion_wallpaper() -> Option<String> {
    MOTION_WALLPAPER_FILES.iter().find_map(|file_name| {
        let path = background_gradient_asset_path(file_name);
        path.is_file().then(|| path.to_string_lossy().into_owned())
    })
}

pub fn load_background_image_optimized(path: &Path) -> Option<RgbaImage> {
    let img = match image::io::Reader::open(path) {
        Ok(reader) => match reader.with_guessed_format() {
            Ok(reader) => reader.decode().ok(),
            Err(_) => None,
        },
        Err(_) => None,
    }?;

    let image = img.into_rgba8();
    let (width, height) = image.dimensions();

    if width > MAX_BACKGROUND_DIMENSION || height > MAX_BACKGROUND_DIMENSION {
        let scale = MAX_BACKGROUND_DIMENSION as f64 / (width.max(height) as f64);
        let new_width = (width as f64 * scale) as u32;
        let new_height = (height as f64 * scale) as u32;

        return Some(image::imageops::resize(
            &image,
            new_width,
            new_height,
            image::imageops::FilterType::Triangle,
        ));
    }

    Some(image)
}

/// Decode an image for on-screen use, bounded near `max_edge` on its long
/// edge.
///
/// JPEGs decode through the DCT-scaled path first: an 8000x6000 catalog
/// wallpaper decodes at 1/4 size instead of 48 MP, which is the difference
/// between a click feeling instant and freezing the UI. The caller draws with
/// cover-fit resampling, so a surface within 25% of the target is used as-is —
/// pre-resizing it costs more than the decode itself. Other formats
/// (user-chosen PNG/WebP) fall back to a full decode plus resize.
pub(super) fn load_background_preview_image(path: &Path, max_edge: u32) -> Option<RgbaImage> {
    let image =
        load_scaled_jpeg(path, max_edge).or_else(|| load_background_image_optimized(path))?;
    Some(resize_to_max_edge(image, max_edge + max_edge / 4))
}

/// Decode a JPEG straight to the smallest DCT scale that still covers
/// `max_edge`; the decoder picks 1/8, 1/4, 1/2 or 1. Returns `None` for
/// non-JPEG sources or exotic JPEG pixel formats so the caller can fall back.
fn load_scaled_jpeg(path: &Path, max_edge: u32) -> Option<RgbaImage> {
    let is_jpeg = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg")
        });
    if !is_jpeg {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = jpeg_decoder::Decoder::new(std::io::BufReader::new(file));
    decoder.read_info().ok()?;
    let info = decoder.info()?;
    let requested = jpeg_request_edge(u32::from(info.width).max(u32::from(info.height)), max_edge);
    let (width, height) = decoder.scale(requested, requested).ok()?;
    let pixels = decoder.decode().ok()?;
    let (width, height) = (u32::from(width), u32::from(height));
    let rgba = match decoder.info()?.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => pixels
            .chunks_exact(3)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        jpeg_decoder::PixelFormat::L8 => pixels
            .iter()
            .flat_map(|&luma| [luma, luma, luma, 255])
            .collect(),
        _ => return None,
    };
    RgbaImage::from_raw(width, height, rgba)
}

/// The supported JPEG DCT scales are 1/1, 1/2, 1/4 and 1/8, and the decoder
/// returns the smallest one that still covers the request. Asking for exactly
/// `max_edge` therefore decodes up to twice the edge we need — four times the
/// pixels, on every click. Pick the request so the decode lands on the scale
/// nearest the target (accepting a small upscale) instead of the scale above
/// it.
fn jpeg_request_edge(source_edge: u32, max_edge: u32) -> u16 {
    if source_edge == 0 {
        return max_edge.min(u32::from(u16::MAX)) as u16;
    }
    let candidates = [
        source_edge.div_ceil(8),
        source_edge.div_ceil(4),
        source_edge.div_ceil(2),
        source_edge,
    ];
    let near_target = candidates
        .iter()
        .copied()
        .filter(|edge| *edge <= max_edge)
        .max()
        .filter(|edge| *edge * 4 >= max_edge * 3);
    let chosen = near_target.unwrap_or_else(|| {
        candidates
            .iter()
            .copied()
            .find(|edge| *edge > max_edge)
            .unwrap_or(source_edge)
    });
    chosen.min(u32::from(u16::MAX)) as u16
}

fn resize_to_max_edge(image: RgbaImage, max_edge: u32) -> RgbaImage {
    let longest = image.width().max(image.height());
    if longest <= max_edge {
        return image;
    }
    let scale = f64::from(max_edge) / f64::from(longest);
    image::imageops::resize(
        &image,
        (f64::from(image.width()) * scale).round().max(1.0) as u32,
        (f64::from(image.height()) * scale).round().max(1.0) as u32,
        image::imageops::FilterType::Triangle,
    )
}

fn parse_wallpaper_setting(raw_value: &str) -> Option<PathBuf> {
    let trimmed = raw_value.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }

    if let Ok(uri) = url::Url::parse(trimmed) {
        if uri.scheme() == "file" {
            if let Ok(path) = uri.to_file_path() {
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }

    let path = PathBuf::from(trimmed);
    if path.is_file() {
        return Some(path);
    }

    None
}

pub(super) fn detect_system_wallpaper_path() -> Option<PathBuf> {
    let setting_queries = [
        ("org.gnome.desktop.background", "picture-uri-dark"),
        ("org.gnome.desktop.background", "picture-uri"),
        ("org.cinnamon.desktop.background", "picture-uri"),
        ("org.mate.background", "picture-filename"),
    ];

    for (schema, key) in setting_queries {
        let output = match Command::new("gsettings")
            .arg("get")
            .arg(schema)
            .arg(key)
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(path) = parse_wallpaper_setting(&stdout) {
            return Some(path);
        }
    }

    None
}

/// Static Background now uses the same Appearance builder as Motion (same
/// side-panel tools). Both inspectors edit the shared MotionSession runtime,
/// so static + motion truly share one background tool. Static preview/export
/// still read EditorState, so this sync copies Motion -> static before draw.
/// ponytail: one conversion fn, not two panels; render reuse follows.
/// Map a Motion frame to the closest static crop ratio. Standard keeps the
/// original canvas; every fixed ratio maps exactly now that the static
/// crop list covers the picker grid. Legacy X (1.91:1 link-card) has no
/// static twin, so it falls back to 16:9. Custom matches by nearest aspect
/// so a manual W/H still previews close to its Motion output.
fn frame_preset_to_crop_ratio(
    frame: &crate::recording::editor::model::MotionFrame,
) -> CropAspectRatio {
    use crate::recording::editor::model::MotionFramePreset as Preset;
    match frame.preset {
        Preset::Standard => CropAspectRatio::Original,
        Preset::Instagram | Preset::OneOne => CropAspectRatio::Square,
        Preset::YouTube | Preset::SixteenNine => CropAspectRatio::SixteenNine,
        Preset::ThreeTwo => CropAspectRatio::ThreeTwo,
        Preset::FourThree => CropAspectRatio::FourThree,
        Preset::FiveFour => CropAspectRatio::FiveFour,
        Preset::FourFive => CropAspectRatio::FourFive,
        Preset::ThreeFour => CropAspectRatio::ThreeFour,
        Preset::TwoThree => CropAspectRatio::TwoThree,
        Preset::NineSixteen | Preset::InstagramStory => CropAspectRatio::NineSixteen,
        Preset::TenTwentyOne | Preset::PinterestLong => CropAspectRatio::TenTwentyOne,
        Preset::YouTubeBanner | Preset::YouTubeThumbnail | Preset::YouTubeVideo => {
            CropAspectRatio::SixteenNine
        }
        Preset::TwitterTweet => CropAspectRatio::SixteenNine,
        Preset::TwitterCover => CropAspectRatio::ThreeOne,
        Preset::InstagramPost | Preset::PinterestSquare => CropAspectRatio::Square,
        Preset::InstagramPortrait => CropAspectRatio::FourFive,
        Preset::PinterestOptimal => CropAspectRatio::TwoThree,
        Preset::X => CropAspectRatio::SixteenNine,
        Preset::Custom => {
            let aspect = frame.effective_aspect().unwrap_or(0.0);
            if aspect <= 0.0 {
                return CropAspectRatio::Original;
            }
            let candidates = [
                (CropAspectRatio::Square, 1.0),
                (CropAspectRatio::FourThree, 4.0 / 3.0),
                (CropAspectRatio::SixteenNine, 16.0 / 9.0),
                (CropAspectRatio::ThreeTwo, 3.0 / 2.0),
                (CropAspectRatio::NineSixteen, 9.0 / 16.0),
                (CropAspectRatio::FiveFour, 5.0 / 4.0),
                (CropAspectRatio::FourFive, 4.0 / 5.0),
                (CropAspectRatio::ThreeFour, 3.0 / 4.0),
                (CropAspectRatio::TwoThree, 2.0 / 3.0),
                (CropAspectRatio::TenTwentyOne, 10.0 / 21.0),
                (CropAspectRatio::ThreeOne, 3.0),
            ];
            // X link-card (1.91:1) still maps to 16:9 for static, so treat
            // it as a candidate to keep Custom 1.91:1 close to the card.
            let mut best = CropAspectRatio::Original;
            let mut best_dist = f64::INFINITY;
            for (ratio, target) in candidates {
                let dist = (aspect - target).abs();
                if dist < best_dist {
                    best_dist = dist;
                    best = ratio;
                }
            }
            // Only snap when reasonably close; exotic customs keep Original.
            if best_dist < 0.08 {
                best
            } else {
                CropAspectRatio::Original
            }
        }
    }
}

/// Reverse of `sync_motion_appearance_to_static`: seed the shared Motion
/// runtime from a restored static `EditorState` so opening an image (or
/// entering Motion) never discards the user's existing background.
///
/// Single-tool rule: static and Motion share one background. The Motion card
/// snapshot is background-free (see `to_motion_card_image`), so the fill must
/// live in exactly one place — the shared `MotionAppearance`. When static
/// already has a background, copy it across instead of layering Motion's
/// default wallpaper behind a card that already contains it.
///
/// `None` clears the Motion fill so Motion looks like Static (checkerboard in
/// preview, black in export) instead of keeping a stale wallpaper from a
/// previous Motion session after the user removed the Static background.
///
/// The static crop aspect also seeds the Motion frame so a cropped still does
/// not reopen as Standard (original canvas) in Motion. Freeform/Original map
/// to Standard; fixed ratios map to their Motion preset twin.
pub(super) fn sync_static_appearance_to_motion(
    state: &EditorState,
    motion: &mut MotionAppearance,
    frame: &mut MotionFrame,
) {
    frame.preset = match state.background_aspect_ratio {
        CropAspectRatio::Freeform | CropAspectRatio::Original => MotionFramePreset::Standard,
        CropAspectRatio::Square => MotionFramePreset::OneOne,
        CropAspectRatio::FourThree => MotionFramePreset::FourThree,
        CropAspectRatio::SixteenNine => MotionFramePreset::SixteenNine,
        CropAspectRatio::TwentyOneNine | CropAspectRatio::TenTwentyOne => {
            MotionFramePreset::TenTwentyOne
        }
        CropAspectRatio::ThreeTwo => MotionFramePreset::ThreeTwo,
        CropAspectRatio::NineSixteen => MotionFramePreset::NineSixteen,
        CropAspectRatio::FiveFour => MotionFramePreset::FiveFour,
        CropAspectRatio::FourFive => MotionFramePreset::FourFive,
        CropAspectRatio::ThreeFour => MotionFramePreset::ThreeFour,
        CropAspectRatio::TwoThree => MotionFramePreset::TwoThree,
        CropAspectRatio::ThreeOne => MotionFramePreset::TwitterCover,
    };
    match &state.background_style {
        BackgroundStyle::None => {
            motion.background_fill_type = MotionBackgroundFillType::None;
            motion.wallpaper_image_name = None;
            motion.custom_background_image = None;
        }
        BackgroundStyle::PlainColor(color) => {
            motion.background_fill_type = MotionBackgroundFillType::Color;
            motion.background_color = [color.r, color.g, color.b, color.a];
        }
        BackgroundStyle::Gradient(idx) => {
            let file =
                BACKGROUND_GRADIENT_PREVIEW_FILES[*idx % BACKGROUND_GRADIENT_PREVIEW_FILES.len()];
            let path = background_gradient_asset_path(file);
            motion.background_fill_type = MotionBackgroundFillType::Wallpaper;
            motion.wallpaper_image_name = Some(path.to_string_lossy().into_owned());
            motion.custom_background_image = None;
        }
        BackgroundStyle::Wallpaper(path) => {
            let full = path.to_string_lossy().into_owned();
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if MOTION_WALLPAPER_FILES.contains(&file_name) {
                motion.background_fill_type = MotionBackgroundFillType::Wallpaper;
                motion.wallpaper_image_name = Some(full);
            } else {
                motion.background_fill_type = MotionBackgroundFillType::Image;
                motion.custom_background_image = Some(full);
            }
        }
        // No Motion equivalent (screenshot-blurred surround); keep the
        // current Motion fill rather than inventing a wrong background.
        BackgroundStyle::Blurred(_) => {}
    }
    motion.background_padding = state.background_padding;
    motion.background_blur = state.background_blur;
    motion.background_noise = state.background_noise;
    motion.border_radius = state.background_corner_radius;
    motion.frame_style = state.frame_style;
    motion.border_thickness = state.border_thickness;
    motion.border_fill_color = [
        state.border_color.r,
        state.border_color.g,
        state.border_color.b,
        state.border_color.a,
    ];
    motion.shadow_opacity = state.shadow_opacity;
    motion.shadow_blur = state.shadow_blur;
    motion.shadow_position = (state.shadow_offset_x, state.shadow_offset_y);
}

pub(super) fn sync_motion_appearance_to_static(
    motion: &MotionAppearance,
    frame: &crate::recording::editor::model::MotionFrame,
    scene_shadow: &crate::recording::editor::model::MotionSceneShadow,
    state: &mut EditorState,
) {
    state.background_style = match &motion.background_fill_type {
        MotionBackgroundFillType::None => BackgroundStyle::None,
        MotionBackgroundFillType::Color => {
            let [r, g, b, a] = motion.background_color;
            BackgroundStyle::PlainColor(DrawColor::new(r, g, b, a))
        }
        MotionBackgroundFillType::Gradient => {
            let idx = motion.selected_gradient_preset_index.unwrap_or(0)
                % BACKGROUND_GRADIENT_PREVIEW_FILES.len();
            BackgroundStyle::Gradient(idx)
        }
        MotionBackgroundFillType::Wallpaper => match &motion.wallpaper_image_name {
            Some(name) => BackgroundStyle::Wallpaper(PathBuf::from(name)),
            None => BackgroundStyle::None,
        },
        MotionBackgroundFillType::Image => match &motion.custom_background_image {
            Some(path) => BackgroundStyle::Wallpaper(PathBuf::from(path)),
            None => BackgroundStyle::None,
        },
    };
    state.background_padding = motion.background_padding;
    state.background_blur = motion.background_blur;
    state.background_noise = motion.background_noise;
    state.background_corner_radius = motion.border_radius;
    state.frame_style = motion.frame_style;
    state.border_thickness = motion.border_thickness;
    {
        let [r, g, b, a] = motion.border_fill_color;
        state.border_color = DrawColor::new(r, g, b, a);
    }
    state.shadow_opacity = motion.shadow_opacity;
    state.shadow_blur = motion.shadow_blur;
    state.shadow_offset_x = motion.shadow_position.0;
    state.shadow_offset_y = motion.shadow_position.1;
    // Keep legacy single-value shadow driving current static render.
    state.background_shadow = (motion.shadow_opacity * 30.0).clamp(0.0, 60.0);
    // Scene Shadows are an independent layer of their own; without this the
    // shared Scene Shadows controls edit the Motion runtime only and the
    // static preview/export show nothing.
    state.scene_shadow = scene_shadow.clone();
    state.background_aspect_ratio = frame_preset_to_crop_ratio(frame);
}

/// Build the static Background inspector with the Motion Appearance builder,
/// bound to the shared MotionSession so both modes edit one runtime.
/// `preview` is the static canvas (edits redraw static); motion preview
/// redraws via the motion inspector's own panel (same runtime, live on switch).
/// `on_interact` arms the Background tool on any Appearance interaction so a
/// stale Pen/Arrow/etc. never draws when the user meant to tweak background.
pub(super) fn build_shared_background_panel(
    window: &ApplicationWindow,
    session: &super::motion_mode::MotionSession,
    preview: &DrawingArea,
    on_interact: Option<Rc<dyn Fn()>>,
) -> GtkBox {
    super::motion_mode::build_motion_appearance_panel(window, session, preview, on_interact)
}

#[cfg(test)]
mod tests {
    use super::{motion_wallpaper_preview_asset_path, EditorState, MOTION_WALLPAPER_FILES};

    /// The Scene Shadows controls live in the shared Appearance panel, so the
    /// mirror into EditorState has to carry the layer or the static preview and
    /// the still export silently show nothing while Motion does.
    #[test]
    fn scene_shadow_mirrors_into_the_static_still() {
        use crate::recording::editor::model::{
            MotionAppearance, MotionBackgroundFillType, MotionFrame, MotionSceneShadow,
            MotionSceneShadowPlacement, MotionSceneShadowPreset,
        };
        let mut appearance = MotionAppearance::default();
        appearance.background_fill_type = MotionBackgroundFillType::Color;
        appearance.background_color = [1.0, 1.0, 1.0, 1.0];
        // 200px source makes the composition's scale factor 0.5, so this is a
        // 25px fill band: a real fill corner and a real card center to sample.
        appearance.background_padding = 50.0;

        let mut shadow = MotionSceneShadow::default();
        shadow.preset = MotionSceneShadowPreset::Vignette;
        shadow.opacity = 1.0;
        shadow.placement = MotionSceneShadowPlacement::Underlay;

        let mut state = EditorState::new(image::RgbaImage::from_pixel(
            200,
            200,
            image::Rgba([255, 255, 255, 255]),
        ));
        super::sync_motion_appearance_to_static(
            &appearance,
            &MotionFrame::default(),
            &shadow,
            &mut state,
        );
        assert_eq!(
            state.scene_shadow, shadow,
            "the mirror must carry the layer"
        );

        let still = state.to_final_image().expect("final renders");
        let corner = still.get_pixel(0, 0).0;
        let center = still.get_pixel(still.width() / 2, still.height() / 2).0;
        assert!(
            corner[0] < 255,
            "the static still must shade its fill like the Motion preview"
        );
        assert_eq!(
            center,
            [255, 255, 255, 255],
            "an underlay stays under the card"
        );
    }

    #[test]
    fn background_gradient_assets_support_installed_runtime_paths() {
        let source = include_str!("background_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);

        assert!(
            production_source.contains("/usr/share/apexshot/background-images")
                && production_source.contains("/usr/local/share/apexshot/background-images"),
            "background gradient lookup should support installed shared asset directories",
        );
    }

    #[test]
    fn motion_wallpaper_catalog_includes_full_size_assets_only() {
        assert_eq!(MOTION_WALLPAPER_FILES.len(), 70);
        assert!(MOTION_WALLPAPER_FILES.contains(&"wallpaper-001.jpg"));
        assert!(MOTION_WALLPAPER_FILES.contains(&"wallpaper-060.jpg"));
        assert_eq!(
            motion_wallpaper_preview_asset_path("wallpaper-001.jpg")
                .file_name()
                .and_then(|name| name.to_str()),
            Some("wallpaper-thumb-001.jpg"),
        );
        assert_eq!(
            motion_wallpaper_preview_asset_path("wallpaper-060.jpg")
                .file_name()
                .and_then(|name| name.to_str()),
            Some("wallpaper-thumb-060.jpg"),
        );
        assert_eq!(
            motion_wallpaper_preview_asset_path("gradient-01.jpg")
                .file_name()
                .and_then(|name| name.to_str()),
            Some("gradient-thumb-01.jpg"),
        );
    }

    #[test]
    fn jpeg_request_picks_the_scale_nearest_the_target() {
        // 6000px source at a 1600 target: decode 1/4 (1500), not 1/2 (3000),
        // which would cost four times the pixels on every wallpaper click.
        assert_eq!(super::jpeg_request_edge(6000, 1600), 1500);
        assert_eq!(super::jpeg_request_edge(3328, 1920), 1664);
        // Scales that would land below 3/4 of the target fall back upward.
        assert_eq!(super::jpeg_request_edge(1920, 1600), 1920);
        assert_eq!(super::jpeg_request_edge(8000, 1920), 2000);
        assert_eq!(super::jpeg_request_edge(5120, 1920), 2560);
        assert_eq!(super::jpeg_request_edge(0, 1920), 1920);
    }

    #[test]
    fn preview_decode_is_bounded_to_the_requested_edge() {
        let wallpaper = super::background_gradient_asset_path("wallpaper-001.jpg");
        let thumb =
            super::load_background_preview_image(&wallpaper, 256).expect("wallpaper decodes");
        assert!(thumb.width().max(thumb.height()) <= 256);
        assert_eq!(
            thumb.width() * 9,
            thumb.height() * 16,
            "aspect ratio stays 16:9"
        );
        assert!(
            thumb.pixels().any(|pixel| pixel.0[0] > 0),
            "preview decode must return real pixels"
        );

        let gradient = super::background_gradient_asset_path("gradient-10.jpg");
        let preview =
            super::load_background_preview_image(&gradient, 1600).expect("8000px gradient decodes");
        // Within 25% of the target: the draw path resamples anyway, so the
        // decode is not followed by a redundant full-surface resize.
        assert!(preview.width().max(preview.height()) <= 1600 + 1600 / 4);
    }

    #[test]
    fn static_background_shares_motion_appearance_builder() {
        let source = include_str!("background_panel.rs");
        let production_source = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(
            production_source.contains("build_shared_background_panel")
                && production_source.contains("build_motion_appearance_panel")
                && production_source.contains("sync_motion_appearance_to_static"),
            "static Background must reuse Motion Appearance (same side-panel tools)",
        );
    }

    #[test]
    fn frame_ratios_map_to_matching_static_crop() {
        use super::super::super::types::CropAspectRatio;
        use crate::recording::editor::model::{MotionFrame, MotionFramePreset};
        let ratio_for = |preset: MotionFramePreset| {
            super::frame_preset_to_crop_ratio(&MotionFrame {
                preset,
                custom_width: 1920,
                custom_height: 1440,
            })
        };
        assert_eq!(
            ratio_for(MotionFramePreset::Standard),
            CropAspectRatio::Original
        );
        assert_eq!(
            ratio_for(MotionFramePreset::OneOne),
            CropAspectRatio::Square
        );
        assert_eq!(
            ratio_for(MotionFramePreset::SixteenNine),
            CropAspectRatio::SixteenNine
        );
        assert_eq!(
            ratio_for(MotionFramePreset::FourThree),
            CropAspectRatio::FourThree
        );
        assert_eq!(
            ratio_for(MotionFramePreset::ThreeTwo),
            CropAspectRatio::ThreeTwo
        );
        assert_eq!(
            ratio_for(MotionFramePreset::NineSixteen),
            CropAspectRatio::NineSixteen
        );
        assert_eq!(
            ratio_for(MotionFramePreset::FiveFour),
            CropAspectRatio::FiveFour
        );
        assert_eq!(
            ratio_for(MotionFramePreset::FourFive),
            CropAspectRatio::FourFive
        );
        assert_eq!(
            ratio_for(MotionFramePreset::ThreeFour),
            CropAspectRatio::ThreeFour
        );
        assert_eq!(
            ratio_for(MotionFramePreset::TwoThree),
            CropAspectRatio::TwoThree
        );
        assert_eq!(
            ratio_for(MotionFramePreset::TenTwentyOne),
            CropAspectRatio::TenTwentyOne
        );
        // Legacy aliases resolve to the same static ratio as their canonical twin.
        assert_eq!(
            ratio_for(MotionFramePreset::Instagram),
            CropAspectRatio::Square
        );
        assert_eq!(
            ratio_for(MotionFramePreset::YouTube),
            CropAspectRatio::SixteenNine
        );
        // Social fixed sizes resolve to their ratio twin; W/H shows exact dims.
        assert_eq!(
            ratio_for(MotionFramePreset::YouTubeBanner),
            CropAspectRatio::SixteenNine
        );
        assert_eq!(
            ratio_for(MotionFramePreset::TwitterCover),
            CropAspectRatio::ThreeOne
        );
        assert_eq!(
            ratio_for(MotionFramePreset::InstagramPortrait),
            CropAspectRatio::FourFive
        );
        assert_eq!(
            ratio_for(MotionFramePreset::PinterestLong),
            CropAspectRatio::TenTwentyOne
        );
        assert_eq!(
            MotionFrame {
                preset: MotionFramePreset::YouTubeBanner,
                custom_width: 1920,
                custom_height: 1440,
            }
            .output_size(),
            (2560, 1440)
        );
        assert_eq!(
            MotionFrame {
                preset: MotionFramePreset::TwitterCover,
                custom_width: 1920,
                custom_height: 1440,
            }
            .output_size(),
            (1500, 500)
        );
        assert_eq!(
            MotionFrame {
                preset: MotionFramePreset::InstagramPost,
                custom_width: 1920,
                custom_height: 1440,
            }
            .output_size(),
            (1080, 1080)
        );
        // Custom 4:3 manual dims snap to the 4:3 static crop.
        assert_eq!(
            super::frame_preset_to_crop_ratio(&MotionFrame {
                preset: MotionFramePreset::Custom,
                custom_width: 1920,
                custom_height: 1440,
            }),
            CropAspectRatio::FourThree
        );
    }

    #[test]
    fn static_background_imports_into_motion_without_doubling() {
        use crate::capture::editor::state::EditorState;
        use crate::capture::editor::types::{BackgroundStyle, DrawColor};
        use crate::recording::editor::model::{
            MotionAppearance, MotionBackgroundFillType, MotionFrame,
        };
        use image::RgbaImage;

        let image = RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255]));
        let mut state = EditorState::new(image);
        state.background_style = BackgroundStyle::PlainColor(DrawColor::new(0.2, 0.4, 0.8, 1.0));
        state.background_padding = 40.0;
        state.background_corner_radius = 22.0;

        let mut motion = MotionAppearance::default();
        let mut frame = MotionFrame::default();
        super::sync_static_appearance_to_motion(&state, &mut motion, &mut frame);
        assert_eq!(motion.background_fill_type, MotionBackgroundFillType::Color);
        assert!((motion.background_padding - 40.0).abs() < f64::EPSILON);
        assert!((motion.border_radius - 22.0).abs() < f64::EPSILON);

        // A removed Static background clears Motion instead of keeping a
        // stale wallpaper from a previous Motion session.
        let fresh = EditorState::new(RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255])));
        let mut keep = MotionAppearance {
            background_fill_type: MotionBackgroundFillType::Wallpaper,
            wallpaper_image_name: Some(String::from("/tmp/keep.jpg")),
            ..MotionAppearance::default()
        };
        let mut keep_frame = MotionFrame::default();
        super::sync_static_appearance_to_motion(&fresh, &mut keep, &mut keep_frame);
        assert_eq!(keep.background_fill_type, MotionBackgroundFillType::None);
        assert_eq!(keep.wallpaper_image_name, None);
    }

    #[test]
    fn custom_frame_output_respects_manual_dims_and_even_budget() {
        use crate::recording::editor::model::{MotionFrame, MotionFramePreset};
        let custom = MotionFrame {
            preset: MotionFramePreset::Custom,
            custom_width: 1920,
            custom_height: 1440,
        };
        assert!((custom.effective_aspect().unwrap() - 4.0 / 3.0).abs() < 1e-9);
        assert_eq!(custom.output_size(), (1920, 1440));
        // Odd manual dims round to even for yuv420p; oversized longs scale to 1920.
        let odd = MotionFrame {
            preset: MotionFramePreset::Custom,
            custom_width: 1921,
            custom_height: 1081,
        };
        let (w, h) = odd.output_size();
        assert_eq!((w % 2, h % 2), (0, 0));
        let wide = MotionFrame {
            preset: MotionFramePreset::Custom,
            custom_width: 3840,
            custom_height: 2160,
        };
        assert_eq!(wide.output_size(), (1920, 1080));
        // Fixed ratios keep the established long-edge budget.
        let square = MotionFrame {
            preset: MotionFramePreset::OneOne,
            custom_width: 1920,
            custom_height: 1440,
        };
        assert_eq!(square.output_size(), (1920, 1920));
    }

    #[test]
    fn portrait_frame_output_sizes_by_width() {
        use crate::recording::editor::model::{MotionFrame, MotionFramePreset};
        let size = |preset| {
            MotionFrame {
                preset,
                custom_width: 1920,
                custom_height: 1440,
            }
            .output_size()
        };
        // Vertical frames hold a 1080 short edge, so 9:16 exports at the
        // 1080x1920 story standard instead of a 608px wide letterbox.
        assert_eq!(size(MotionFramePreset::NineSixteen), (1080, 1920));
        assert_eq!(size(MotionFramePreset::ThreeFour), (1080, 1440));
        assert_eq!(size(MotionFramePreset::TwoThree), (1080, 1620));
        assert_eq!(size(MotionFramePreset::FourFive), (1080, 1350));
        // Taller than 9:16 the long edge caps at the same 1920 budget.
        assert_eq!(size(MotionFramePreset::TenTwentyOne), (914, 1920));
    }
}
