use apexshot::backend::{DisplayBackend, WaylandBackend, X11Backend};
use apexshot::overlay::{select_crosshair_from_capture_with_gtk, OverlaySelection};

fn main() {
    let capture = if WaylandBackend::is_supported() {
        eprintln!("Using Wayland backend to capture screen...");
        let backend = WaylandBackend::new().expect("Failed to initialize Wayland backend");
        backend
            .capture_screen_for_selection_impl()
            .or_else(|_| backend.capture_screen())
            .expect("Failed to capture screen under Wayland")
    } else if X11Backend::is_supported() {
        eprintln!("Using X11 backend to capture screen...");
        let backend = X11Backend::new().expect("Failed to initialize X11 backend");
        backend
            .capture_screen()
            .expect("Failed to capture screen under X11")
    } else {
        eprintln!("No supported display backend found!");
        std::process::exit(1);
    };

    eprintln!("Opening Rust GTK overlay in crosshair mode...");
    eprintln!("Click and drag to select an area, press ESC to cancel.");

    match select_crosshair_from_capture_with_gtk(&capture) {
        Ok(OverlaySelection::Area(Some(area))) => {
            println!(
                "Selected area: x={}, y={}, width={}, height={}",
                area.x, area.y, area.width, area.height
            );
        }
        Ok(OverlaySelection::Area(None)) | Ok(OverlaySelection::Recording(_)) => {
            println!("Selection cancelled");
        }
        Err(e) => {
            eprintln!("Selection error: {e}");
            std::process::exit(1);
        }
    }
}
