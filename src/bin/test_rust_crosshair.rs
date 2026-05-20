use apexshot::backend::{CaptureData, PixelFormat};
use apexshot::overlay::select_crosshair_from_capture_with_gtk;
use image::RgbaImage;

fn main() {
    let width = 1920;
    let height = 1080;

    let mut img = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let r = ((x as f32 / width as f32) * 255.0) as u8;
            let g = ((y as f32 / height as f32) * 255.0) as u8;
            let b = 128u8;
            img.put_pixel(x, y, image::Rgba([r, g, b, 255]));
        }
    }

    let raw: Vec<u8> = img.into_raw();
    let capture = CaptureData::new(raw, width, height, PixelFormat::RGBA32);

    eprintln!("Opening Rust GTK overlay in crosshair mode...");
    eprintln!("Click and drag to select an area, press ESC to cancel.");

    match select_crosshair_from_capture_with_gtk(&capture) {
        Ok(Some(area)) => {
            println!(
                "Selected area: x={}, y={}, width={}, height={}",
                area.x, area.y, area.width, area.height
            );
        }
        Ok(None) => {
            println!("Selection cancelled");
        }
        Err(e) => {
            eprintln!("Selection error: {e}");
            std::process::exit(1);
        }
    }
}
