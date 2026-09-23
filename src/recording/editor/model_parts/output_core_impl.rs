impl VideoEditState {
    /// Frame canvas. A Frame/aspect pick fixes this canvas, which is
    /// also the export size; the source is fitted inside it. `Original` keeps
    /// the source size so an unpicked frame exports untouched.
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

    /// True when a Frame pick fixes the canvas instead of keeping the source
    /// size. A fixed frame holds its aspect: padding insets the video inside
    /// the canvas rather than growing the output past the picked ratio.
    pub fn has_fixed_frame(&self) -> bool {
        self.dimension_preset != DimensionPreset::Original
    }

    /// Background surround in canvas pixels. Slider units are defined against
    /// a 400px long edge like the image editor (padding 40 is ~10% per side),
    /// floored while a fill is active so the video never sits edge-to-edge
    /// against it. Zero when no fill is picked.
    pub fn background_padding_px(&self) -> f64 {
        if self.background.is_none() {
            return 0.0;
        }
        let (canvas_w, canvas_h) = self.canvas_dimensions();
        let reference = canvas_w.max(canvas_h) as f64 / 400.0;
        crate::capture::editor::types::effective_background_padding(self.background_padding, true)
            * reference
    }

    /// Corner radius on the video card, in canvas pixels. Uses the same
    /// 400px-long-edge slider units as padding so the panel's number matches
    /// what preview and export draw. Unlike padding this does not depend on a
    /// fill being active — a rounded card over a black scene still reads as
    /// rounded, so it stays live with `background: None`.
    pub fn background_corner_radius_px(&self) -> f64 {
        if !self.background_corner_radius.is_finite() || self.background_corner_radius <= 0.0 {
            return 0.0;
        }
        let (canvas_w, canvas_h) = self.canvas_dimensions();
        let reference = canvas_w.max(canvas_h) as f64 / 400.0;
        self.background_corner_radius * reference
    }

    /// True when the corner radius is large enough to be worth masking. The
    /// export and preview both skip the rounded-corner path below this, so a
    /// fractional slider value near zero doesn't add a filter for nothing.
    pub fn has_corner_radius(&self) -> bool {
        self.background_corner_radius_px() > 0.5
    }

    /// Export size. A fixed Frame is the output exactly (16:9 exports
    /// 1920x1080 whatever the recording is); `Original` has no ratio to hold,
    /// so a fill grows the canvas around the source, matching Auto sizing in
    /// other editors.
    pub fn output_dimensions(&self) -> (u32, u32) {
        let (base_w, base_h) = self.canvas_dimensions();
        if self.background.is_none() || self.has_fixed_frame() {
            return (base_w, base_h);
        }
        let pad = self.background_padding_px().round().max(0.0) as u32;
        (
            even_dimension(base_w + pad * 2),
            even_dimension(base_h + pad * 2),
        )
    }

    /// The video rect inside the output canvas: the source fitted with the
    /// background padding as an inset, centered. This is the layer the preview
    /// and the composite export both draw, so cursor math stays mapped to the
    /// footage instead of the letterbox around it.
    pub fn video_rect_dimensions(&self) -> (u32, u32) {
        let (src_w, src_h) = self.effective_source_dimensions();
        let (out_w, out_h) = self.output_dimensions();
        let pad = self.background_padding_px().round().max(0.0) as u32;
        let box_w = out_w.saturating_sub(pad * 2).max(MIN_DIMENSION);
        let box_h = out_h.saturating_sub(pad * 2).max(MIN_DIMENSION);
        contain_fit(src_w, src_h, box_w, box_h)
    }

    /// True when quality/dimensions/zoom/pad require a re-encode (stream-copy cannot apply them).
    pub fn needs_reencode(&self) -> bool {
        if self.crop.is_some() {
            return true;
        }
        // A held last frame is padded with tpad, which a stream copy cannot
        // express — the tail is longer than the source it came from.
        if self.freeze_tail_seconds() > 0.001 {
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
        if self.segment_muted.iter().any(|muted| *muted) {
            return true;
        }
        // Quality only takes effect when re-encoding. The default tier keeps
        // the untouched-export stream copy; any other tier forces a re-encode
        // so the picked CRF actually applies.
        self.quality != ExportQuality::default()
    }

    pub fn needs_composite(&self) -> bool {
        (!self.zoom_clips.is_empty() && !self.zoom_hidden)
            || !self.background.is_none()
            // A radius needs the composite graph even with no fill, otherwise
            // the mask never reaches the encoder and the corners stay square.
            || self.has_corner_radius()
            || self
                .sidecar
                .as_ref()
                .is_some_and(|sidecar| sidecar.can_render_cursor_overlay())
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
