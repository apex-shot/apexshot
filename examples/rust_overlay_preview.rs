fn main() {
    let selector = apexshot::AreaSelector::new();
    match selector.run() {
        Ok(apexshot::OverlaySelection::Area(Some(area))) => {
            eprintln!(
                "Selected area: x={} y={} width={} height={}",
                area.x, area.y, area.width, area.height
            );
        }
        Ok(apexshot::OverlaySelection::Area(None))
        | Ok(apexshot::OverlaySelection::Recording(_)) => {
            eprintln!("Selection cancelled")
        }
        Err(err) => {
            eprintln!("Rust overlay preview failed: {err}");
            std::process::exit(1);
        }
    }
}
