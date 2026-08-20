impl VideoEditState {
    /// Output frame size. Aspect-ratio picks (WebCut) set this canvas; the
    /// source is letterboxed inside it instead of shrinking the frame.
    pub fn canvas_dimensions(&self) -> (u32, u32) {
        let (src_w, src_h) = self.effective_source_dimensions();
        match self.dimension_preset {
            DimensionPreset::Original => (src_w, src_h),
            DimensionPreset::P1080 => (1920, 1080),
            DimensionPreset::P720 => (1280, 720),
            DimensionPreset::P480 => (854, 480),
            DimensionPreset::Custom => (
                even_dimension(self.custom_width.max(MIN_DIMENSION)),
                even_dimension(self.custom_height.max(MIN_DIMENSION)),
            ),
        }
    }

    pub fn target_dimensions(&self) -> (u32, u32) {
        let (src_w, src_h) = self.effective_source_dimensions();
        let (box_w, box_h) = self.canvas_dimensions();
        match self.dimension_preset {
            DimensionPreset::Original => (box_w, box_h),
            _ => fit_dimensions(src_w, src_h, box_w, box_h),
        }
    }

    /// True when quality/dimensions/zoom/pad require a re-encode (stream-copy cannot apply them).
    pub fn needs_reencode(&self) -> bool {
        if self.crop.is_some() {
            return true;
        }
        if self.needs_composite() {
            return true;
        }
        let (tw, th) = self.canvas_dimensions();
        let (sw, sh) = (
            even_dimension(self.metadata.width.max(1)),
            even_dimension(self.metadata.height.max(1)),
        );
        if tw != sw || th != sh {
            return true;
        }
        if self.timeline_offset_seconds > 0.001 || self.has_segment_gaps() {
            return true;
        }
        if self
            .segment_speeds
            .iter()
            .any(|speed| (*speed - 1.0).abs() > 1e-6)
        {
            return true;
        }
        // Quality only takes effect when re-encoding.
        self.quality != 70
    }

    pub fn needs_composite(&self) -> bool {
        (!self.zoom_clips.is_empty() && !self.zoom_hidden)
            || !self.background.is_none()
            || self
                .sidecar
                .as_ref()
                .is_some_and(|sidecar| !sidecar.pointer.is_empty())
    }

    pub fn default_zoom_center(&self, at_seconds: f64) -> (f64, f64) {
        if let Some(sidecar) = &self.sidecar {
            if let Some((x, y, _)) = sidecar.interpolated_at(at_seconds) {
                return (x, y);
            }
        }
        (
            self.metadata.width as f64 / 2.0,
            self.metadata.height as f64 / 2.0,
        )
    }

}
