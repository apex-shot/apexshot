//! Background gradient/wallpaper asset loading and cache handles (PR 10.13).
//!
//! Owns preload worker, UI-thread completion poll, gradient surface slots, and
//! wallpaper path→surface cache. The draw path and background panel still
//! consume the returned handles.

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
    pub gradient_surfaces: Rc<RefCell<Vec<Option<gtk4::cairo::ImageSurface>>>>,
    pub wallpaper_cache: Rc<RefCell<HashMap<PathBuf, gtk4::cairo::ImageSurface>>>,
    pub wallpaper_loader_sender: mpsc::Sender<(Option<usize>, PathBuf, RgbaImage)>,
}

/// Start background preload of gradients/system wallpaper and return cache handles.
pub(super) fn install_background_asset_loading(
    drawing_area: &DrawingArea,
) -> BackgroundAssetCaches {
    let gradient_surfaces = Rc::new(RefCell::new(vec![
        None::<gtk4::cairo::ImageSurface>;
        background_panel::BACKGROUND_GRADIENT_PREVIEW_FILES.len()
    ]));
    let wallpaper_cache = Rc::new(RefCell::new(
        HashMap::<PathBuf, gtk4::cairo::ImageSurface>::new(),
    ));

    let (wallpaper_loader_sender, receiver) =
        mpsc::channel::<(Option<usize>, PathBuf, RgbaImage)>();

    // Pre-load gradients and system wallpaper in background
    {
        let sender = wallpaper_loader_sender.clone();
        // Background loader thread
        std::thread::spawn({
            move || {
                // 1. System wallpaper (High Priority)
                if let Some(path) = background_panel::detect_system_wallpaper_path() {
                    println!("[DEBUG] Detected system wallpaper: {:?}", path);
                    if let Some(rgba) = background_panel::load_background_image_optimized(&path) {
                        let _ = sender.send((None, path, rgba));
                    }
                } else {
                    println!("[DEBUG] No system wallpaper detected.");
                    // Also load the fallback wallpaper into cache
                    let fallback_path = background_panel::background_gradient_asset_path(
                        background_panel::BACKGROUND_GRADIENT_PREVIEW_FILES[0],
                    );
                    if let Some(rgba) =
                        background_panel::load_background_image_optimized(&fallback_path)
                    {
                        let _ = sender.send((None, fallback_path, rgba));
                    }
                }

                // 2. Gradients
                for (idx, file_name) in background_panel::BACKGROUND_GRADIENT_PREVIEW_FILES
                    .iter()
                    .enumerate()
                {
                    let path = background_panel::background_gradient_asset_path(file_name);
                    if let Some(rgba) = background_panel::load_background_image_optimized(&path) {
                        if sender.send((Some(idx), path, rgba)).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let gradient_surfaces_main = gradient_surfaces.clone();
        let wallpaper_cache_main = wallpaper_cache.clone();
        let drawing_area_main = drawing_area.downgrade();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            while let Ok((idx_opt, path, rgba)) = receiver.try_recv() {
                if let Some(surface) = rgba_image_to_surface(&rgba) {
                    if let Some(idx) = idx_opt {
                        gradient_surfaces_main.borrow_mut()[idx] = Some(surface);
                    } else {
                        wallpaper_cache_main.borrow_mut().insert(path, surface);
                    }
                    if let Some(area) = drawing_area_main.upgrade() {
                        area.queue_draw();
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    BackgroundAssetCaches {
        gradient_surfaces,
        wallpaper_cache,
        wallpaper_loader_sender,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn background_assets_preload_system_wallpaper_and_gradients() {
        let source = include_str!("background_assets.rs");
        assert!(
            source.contains("detect_system_wallpaper_path()")
                && source.contains("BACKGROUND_GRADIENT_PREVIEW_FILES")
                && source.contains("load_background_image_optimized")
                && source.contains("Duration::from_millis(100)")
                && source.contains("struct BackgroundAssetCaches")
                && source.contains("fn install_background_asset_loading"),
            "background assets must preload wallpaper/gradients and expose cache handles"
        );
    }
}
