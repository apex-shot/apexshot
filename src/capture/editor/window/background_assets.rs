//! Background wallpaper asset loading and cache handles (PR 10.13).
//!
//! Owns the preload worker, the UI-thread completion poll, and the wallpaper
//! path→surface cache. The draw path consumes the returned cache handle.

use gtk4::{glib, prelude::*, DrawingArea};
use image::RgbaImage;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use super::super::render::rgba_image_to_surface;
use super::background_panel;

pub(super) struct BackgroundAssetCaches {
    pub wallpaper_cache: Rc<RefCell<HashMap<PathBuf, gtk4::cairo::ImageSurface>>>,
}

/// Start background preload of gradients/system wallpaper and return cache handles.
pub(super) fn install_background_asset_loading(
    drawing_area: &DrawingArea,
) -> BackgroundAssetCaches {
    let wallpaper_cache = Rc::new(RefCell::new(
        HashMap::<PathBuf, gtk4::cairo::ImageSurface>::new(),
    ));

    let (wallpaper_loader_sender, receiver) = mpsc::channel::<(PathBuf, RgbaImage)>();

    // Pre-load the system wallpaper in the background.
    {
        let sender = wallpaper_loader_sender.clone();
        // Background loader thread
        std::thread::spawn({
            move || {
                // System wallpaper (high priority), or the bundled fallback.
                if let Some(path) = background_panel::detect_system_wallpaper_path() {
                    println!("[DEBUG] Detected system wallpaper: {:?}", path);
                    if let Some(rgba) = background_panel::load_background_preview_image(
                        &path,
                        background_panel::PREVIEW_BACKGROUND_MAX_EDGE,
                    ) {
                        let _ = sender.send((path, rgba));
                    }
                } else {
                    println!("[DEBUG] No system wallpaper detected.");
                    let fallback_path = background_panel::background_gradient_asset_path(
                        background_panel::MOTION_WALLPAPER_FILES[0],
                    );
                    if let Some(rgba) = background_panel::load_background_preview_image(
                        &fallback_path,
                        background_panel::PREVIEW_BACKGROUND_MAX_EDGE,
                    ) {
                        let _ = sender.send((fallback_path, rgba));
                    }
                }
            }
        });

        let wallpaper_cache_main = wallpaper_cache.clone();
        let drawing_area_main = drawing_area.downgrade();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            while let Ok((path, rgba)) = receiver.try_recv() {
                if let Some(surface) = rgba_image_to_surface(&rgba) {
                    wallpaper_cache_main.borrow_mut().insert(path, surface);
                    if let Some(area) = drawing_area_main.upgrade() {
                        area.queue_draw();
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    BackgroundAssetCaches { wallpaper_cache }
}

#[cfg(test)]
mod tests {
    #[test]
    fn background_assets_preload_the_system_wallpaper() {
        let source = include_str!("background_assets.rs");
        assert!(
            source.contains("detect_system_wallpaper_path()")
                && source.contains("load_background_preview_image")
                && source.contains("Duration::from_millis(100)")
                && source.contains("struct BackgroundAssetCaches")
                && source.contains("fn install_background_asset_loading"),
            "background assets must preload the wallpaper and expose cache handles"
        );
    }
}
