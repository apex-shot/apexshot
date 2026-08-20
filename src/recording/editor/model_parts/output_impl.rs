impl VideoEditState {
    pub fn apply_aspect_ratio(&mut self, width: u32, height: u32) {
        self.dimension_preset = DimensionPreset::Custom;
        self.custom_width = width.max(MIN_DIMENSION);
        self.custom_height = height.max(MIN_DIMENSION);
    }

    /// Set the source crop. A selection covering (nearly) the whole frame
    /// reverts to the original, so dragging the border back out is enough.
    pub fn set_crop(&mut self, x: f64, y: f64, width: f64, height: f64) {
        let frame_w = self.metadata.width as f64;
        let frame_h = self.metadata.height as f64;
        let x = x.clamp(0.0, frame_w - 2.0);
        let y = y.clamp(0.0, frame_h - 2.0);
        let width = width.clamp(2.0, frame_w - x);
        let height = height.clamp(2.0, frame_h - y);
        if x <= 1.0 && y <= 1.0 && width >= frame_w - 2.0 && height >= frame_h - 2.0 {
            self.crop = None;
            return;
        }
        let even_floor = |v: u32| v - (v % 2);
        self.crop = Some(CropSelection {
            x: even_floor(x.round() as u32),
            y: even_floor(y.round() as u32),
            width: even_dimension(width.round() as u32),
            height: even_dimension(height.round() as u32),
        });
    }

    pub fn reset_crop(&mut self) {
        self.crop = None;
    }

    /// (x, y, w, h) of the source region that survives cropping.
    pub fn crop_or_full(&self) -> (f64, f64, f64, f64) {
        match self.crop {
            Some(c) => (c.x as f64, c.y as f64, c.width as f64, c.height as f64),
            None => (
                0.0,
                0.0,
                self.metadata.width.max(1) as f64,
                self.metadata.height.max(1) as f64,
            ),
        }
    }

    /// Frame size after the static crop is applied.
    pub fn effective_source_dimensions(&self) -> (u32, u32) {
        match self.crop {
            Some(c) => (c.width, c.height),
            None => (
                even_dimension(self.metadata.width.max(1)),
                even_dimension(self.metadata.height.max(1)),
            ),
        }
    }

    pub fn reset_aspect_ratio(&mut self) {
        self.dimension_preset = DimensionPreset::Original;
        self.custom_width = self.metadata.width;
        self.custom_height = self.metadata.height;
    }

    pub fn canvas_label(&self) -> &'static str {
        if self.dimension_preset == DimensionPreset::Original {
            "Original"
        } else {
            let (width, height) = self.padded_output_dimensions();
            closest_aspect_ratio(width, height)
        }
    }

    pub fn padded_output_dimensions(&self) -> (u32, u32) {
        let (base_w, base_h) = self.canvas_dimensions();
        if self.background.is_none() {
            return (base_w, base_h);
        }
        let layout = crate::capture::editor::composition::BackgroundComposition::new(
            base_w as f64,
            base_h as f64,
        )
        .with_style(match self.background {
            VideoBackground::None => crate::capture::editor::types::BackgroundStyle::None,
            VideoBackground::Plain { r, g, b } => {
                crate::capture::editor::types::BackgroundStyle::PlainColor(
                    crate::capture::editor::types::DrawColor::new(
                        r as f64 / 255.0,
                        g as f64 / 255.0,
                        b as f64 / 255.0,
                        1.0,
                    ),
                )
            }
            VideoBackground::Gradient(index) => {
                crate::capture::editor::types::BackgroundStyle::Gradient(index)
            }
        })
        .with_padding(self.background_padding)
        .with_shadow(self.background_shadow)
        .with_corner_radius(self.background_corner_radius)
        .compute();
        (
            even_dimension(layout.canvas_width.round().max(2.0) as u32),
            even_dimension(layout.canvas_height.round().max(2.0) as u32),
        )
    }

    pub fn estimated_size_bytes(&self, trim_only: bool) -> u64 {
        estimate_size_bytes(self, trim_only)
    }
}
