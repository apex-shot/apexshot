use unicode_segmentation::UnicodeSegmentation;

/// A title's font size in its reference rectangle: card titles keep the
/// recovered 6 %-of-card-height rule and its pixel cap, measured in source
/// pixels so preview and export round the same clamp.
fn motion_text_card_font_px(source_h: f64, size: f64) -> f64 {
    (source_h * 0.06 * size.clamp(MIN_MOTION_TEXT_SIZE, MAX_MOTION_TEXT_SIZE)).clamp(14.0, 160.0)
}

/// The rectangle a title is measured against. Card titles compose through the
/// card's own matrix; Canvas titles sit on the composition with no image
/// transform at all.
struct MotionTextReference {
    matrix: Matrix,
    card: Option<CardLayout>,
    width: f64,
    height: f64,
    font_size: f64,
}

fn motion_text_reference(
    layout: CardLayout,
    segment: &MotionTextSegment,
) -> Option<MotionTextReference> {
    let stage = layout.stage;
    if segment.annotation_coordinate_space.is_canvas() {
        return Some(MotionTextReference {
            matrix: Matrix::new(
                1.0,
                0.0,
                0.0,
                1.0,
                stage.center_x - stage.bounds_w * 0.5,
                stage.center_y - stage.bounds_h * 0.5,
            ),
            card: None,
            width: stage.bounds_w,
            height: stage.bounds_h,
            font_size: 0.06
                * stage.bounds_h
                * segment
                    .size
                    .clamp(MIN_MOTION_TEXT_SIZE, MAX_MOTION_TEXT_SIZE),
        });
    }
    let font_size = motion_text_card_font_px(layout.img_h(), segment.size);
    Some(MotionTextReference {
        matrix: Matrix::identity(),
        card: Some(layout),
        width: layout.img_w(),
        height: layout.img_h(),
        font_size,
    })
}

/// Compose affine maps by pushing the inner map's basis through the outer one.
/// Reading three points leaves no doubt about multiplication order: the result
/// maps a point through `inner` and then through `outer`.
fn motion_text_compose(outer: &Matrix, inner: &Matrix) -> Matrix {
    let (p0, p1, p2) = (
        inner.transform_point(0.0, 0.0),
        inner.transform_point(1.0, 0.0),
        inner.transform_point(0.0, 1.0),
    );
    let (q0, q1, q2) = (
        outer.transform_point(p0.0, p0.1),
        outer.transform_point(p1.0, p1.1),
        outer.transform_point(p2.0, p2.1),
    );
    Matrix::new(
        q1.0 - q0.0,
        q1.1 - q0.1,
        q2.0 - q0.0,
        q2.1 - q0.1,
        q0.0,
        q0.1,
    )
}

/// Affine that lands a paragraph box's top-left origin on the stage with the
/// box centre at `(center_x, center_y)` and the box rotated about that centre.
fn motion_text_box_matrix(
    center_x: f64,
    center_y: f64,
    box_w: f64,
    box_h: f64,
    rotation: f64,
    scale: f64,
) -> Matrix {
    let (sin, cos) = rotation.to_radians().sin_cos();
    let (sin, cos) = (sin * scale, cos * scale);
    let (half_w, half_h) = (box_w * 0.5, box_h * 0.5);
    Matrix::new(
        cos,
        sin,
        -sin,
        cos,
        center_x - (cos * half_w - sin * half_h),
        center_y - (sin * half_w + cos * half_h),
    )
}

/// One revealed rectangle of the paragraph, in paragraph-local coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MotionTextRevealRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Which part of the paragraph the entrance has revealed. Completed rows stay
/// whole and only the row being typed is cut, so an earlier finished line can
/// never lose its tail to a later short one. The revealed ranges come from the
/// layout's own visual ranges, so right-to-left and mixed runs reveal in their
/// reading order instead of left to right.
#[derive(Debug, Clone, PartialEq)]
enum MotionTextReveal {
    All,
    Nothing,
    Rects(Vec<MotionTextRevealRect>),
}

impl MotionTextReveal {
    fn contains(&self, x: f64, y: f64) -> bool {
        match self {
            Self::All => true,
            Self::Nothing => false,
            Self::Rects(rects) => rects.iter().any(|rect| {
                x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
            }),
        }
    }

    fn clip(&self, context: &Context) {
        let Self::Rects(rects) = self else {
            return;
        };
        for rect in rects {
            context.rectangle(rect.x, rect.y, rect.width, rect.height);
        }
        context.clip();
    }
}

/// A shaped paragraph plus where it lands. Drawing and hit-testing both use
/// this, so rotation, paragraph box, attachment and entrance offset can never
/// disagree between what is painted and what the pointer can grab.
pub(in crate::capture::editor::window) struct MotionTextPlacement {
    layout: pango::Layout,
    style: MotionTextStyle,
    format: MotionTextFormat,
    font_size: f64,
    to_stage: Matrix,
    reveal: MotionTextReveal,
    reference: MotionTextReference,
    position_range: [(f64, f64); 2],
    position_band: (f64, f64),
    anchor: (f64, f64),
}

impl MotionTextPlacement {
    fn effective_alpha(&self) -> f64 {
        self.style.alpha * self.format.color[3]
    }

    fn to_local(&self, stage_x: f64, stage_y: f64) -> Option<(f64, f64)> {
        let inverse = self.to_stage.try_invert().ok()?;
        Some(inverse.transform_point(stage_x, stage_y))
    }

    fn ink_local(&self) -> (f64, f64, f64, f64) {
        let (ink, _) = self.layout.pixel_extents();
        (
            f64::from(ink.x()),
            f64::from(ink.y()),
            f64::from(ink.width()),
            f64::from(ink.height()),
        )
    }

    /// Stage-space hit test: inside the revealed part of the paragraph and on
    /// the visible glyphs, with a small slop so a thin line stays grabbable.
    pub(in crate::capture::editor::window) fn contains_stage_point(
        &self,
        stage_x: f64,
        stage_y: f64,
        slop: f64,
    ) -> bool {
        if self.effective_alpha() < 0.02 {
            return false;
        }
        let Some((x, y)) = self.to_local(stage_x, stage_y) else {
            return false;
        };
        if !self.reveal.contains(x, y) {
            return false;
        }
        if let Some(card) = self.reference.card {
            let points = [
                card.project(0.0, 0.0),
                card.project(card.img_w(), 0.0),
                card.project(card.img_w(), card.img_h()),
                card.project(0.0, card.img_h()),
            ];
            let mut crosses = (0..4).map(|index| {
                let a = points[index];
                let b = points[(index + 1) % 4];
                (b.0 - a.0) * (stage_y - a.1) - (b.1 - a.1) * (stage_x - a.0)
            });
            if !crosses.clone().all(|cross| cross >= -1e-6) && !crosses.all(|cross| cross <= 1e-6) {
                return false;
            }
        }
        let (ink_x, ink_y, ink_w, ink_h) = self.ink_local();
        x >= ink_x - slop
            && x <= ink_x + ink_w + slop
            && y >= ink_y - slop
            && y <= ink_y + ink_h + slop
    }

    /// Slop in paragraph-local units for a pointer tolerance given in stage
    /// pixels, capped so it can never reach past the text itself.
    pub(in crate::capture::editor::window) fn local_slop(&self, stage_px: f64) -> f64 {
        let (x, y) = self.to_stage.transform_distance(1.0, 0.0);
        (stage_px / x.hypot(y).max(1e-6)).min(self.font_size * 0.35)
    }

    pub(in crate::capture::editor::window) fn anchor_view_point(&self) -> (f64, f64) {
        if let Some(card) = self.reference.card {
            card.project(self.anchor.0, self.anchor.1)
        } else {
            self.reference
                .matrix
                .transform_point(self.anchor.0, self.anchor.1)
        }
    }

    pub(in crate::capture::editor::window) fn position_at(
        &self,
        view_x: f64,
        view_y: f64,
    ) -> Option<(f64, f64)> {
        let point = if let Some(card) = self.reference.card {
            let (x, y) = card.unproject(view_x, view_y);
            (x * self.reference.width, y * self.reference.height)
        } else {
            self.reference
                .matrix
                .try_invert()
                .ok()?
                .transform_point(view_x, view_y)
        };
        let (min, max) = self.position_band;
        let position = |value: f64, extent: f64, (low, high): (f64, f64)| {
            if high - low < 1e-6 {
                return (min + max) * 0.5;
            }
            if value <= low + 1e-6 {
                return min;
            }
            if value >= high - 1e-6 {
                return max;
            }
            (value / extent).clamp(min, max)
        };
        Some((
            position(point.0, self.reference.width, self.position_range[0]),
            position(point.1, self.reference.height, self.position_range[1]),
        ))
    }
}

fn motion_text_layout(
    context: &Context,
    format: &MotionTextFormat,
    text: &str,
    font_size: f64,
    wrap_width: f64,
) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(context);
    let mut description = pango::FontDescription::new();
    description.set_family(&format.font_family);
    description.set_absolute_size(font_size * f64::from(pango::SCALE));
    description.set_weight(if format.bold {
        pango::Weight::Bold
    } else {
        pango::Weight::Normal
    });
    description.set_style(if format.italic {
        pango::Style::Italic
    } else {
        pango::Style::Normal
    });
    layout.set_font_description(Some(&description));
    layout.set_text(text);
    layout.set_line_spacing(
        format
            .line_spacing
            .clamp(MIN_MOTION_TEXT_LINE_SPACING, MAX_MOTION_TEXT_LINE_SPACING) as f32,
    );
    if format.letter_spacing.abs() > 1e-6 {
        let attributes = pango::AttrList::new();
        attributes.insert(pango::AttrInt::new_letter_spacing(
            (format.letter_spacing * font_size * f64::from(pango::SCALE)).round() as i32,
        ));
        layout.set_attributes(Some(&attributes));
    }
    layout.set_alignment(match format.alignment {
        MotionTextAlignment::Left => pango::Alignment::Left,
        MotionTextAlignment::Center => pango::Alignment::Center,
        MotionTextAlignment::Right => pango::Alignment::Right,
    });
    if wrap_width > 0.0 {
        layout.set_width((wrap_width * f64::from(pango::SCALE)).round() as i32);
        layout.set_wrap(pango::WrapMode::WordChar);
    } else if layout.line_count() > 1 {
        let (_, logical) = layout.pixel_extents();
        layout.set_width(logical.width() * pango::SCALE);
    }
    layout
}

/// The text the title actually draws. An untitled clip still shows its
/// placeholder, exactly as the single-line painter did.
fn motion_text_content(segment: &MotionTextSegment) -> &str {
    if segment.text.trim().is_empty() {
        "Title"
    } else {
        segment.text.as_str()
    }
}

/// Byte offset the entrance has revealed, or `None` when nothing shows yet.
fn motion_text_revealed_bytes(text: &str, scope: MotionTextScope, reveal: f64) -> Option<usize> {
    if reveal >= 0.999 {
        return Some(text.len());
    }
    if reveal <= 0.0 {
        return None;
    }
    match scope {
        MotionTextScope::Character => {
            let boundaries = text
                .grapheme_indices(true)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let count = (boundaries.len() as f64 * reveal).ceil() as usize;
            Some(boundaries.get(count).copied().unwrap_or(text.len()))
        }
        MotionTextScope::Word => {
            let words = text.unicode_word_indices().collect::<Vec<_>>();
            if words.is_empty() {
                return Some(text.len());
            }
            let count = (words.len() as f64 * reveal).ceil() as usize;
            Some(
                words
                    .get(count.saturating_sub(1))
                    .map(|(index, word)| index + word.len())
                    .unwrap_or(text.len()),
            )
        }
        MotionTextScope::Line => {
            let lines = text.split('\n').count();
            let count = (lines as f64 * reveal).ceil() as usize;
            if count >= lines {
                return Some(text.len());
            }
            let mut offset = 0usize;
            for (index, line) in text.split('\n').enumerate() {
                if index == count {
                    return Some(offset);
                }
                offset += line.len() + 1;
            }
            Some(text.len())
        }
    }
}

fn motion_text_reveal(
    layout: &pango::Layout,
    text: &str,
    scope: MotionTextScope,
    reveal: f64,
    box_w: f64,
    bleed: f64,
) -> MotionTextReveal {
    let Some(bytes) = motion_text_revealed_bytes(text, scope, reveal) else {
        return MotionTextReveal::Nothing;
    };
    if bytes >= text.len() {
        return MotionTextReveal::All;
    }
    let unit = f64::from(pango::SCALE);
    let mut rects = Vec::new();
    for line_index in 0..layout.line_count() {
        let Some(line) = layout.line(line_index) else {
            continue;
        };
        let start = line.start_index();
        if start < 0 || start as usize >= bytes {
            break;
        }
        let row = layout.index_to_pos(start);
        let top = f64::from(row.y()) / unit;
        let height = f64::from(row.height()) / unit;
        if (start as usize) + (line.length().max(0) as usize) <= bytes {
            rects.push(MotionTextRevealRect {
                x: -bleed,
                y: top,
                width: box_w + bleed * 2.0,
                height,
            });
            continue;
        }
        for pair in line.x_ranges(start, bytes as i32).chunks_exact(2) {
            let (from, to) = (f64::from(pair[0]) / unit, f64::from(pair[1]) / unit);
            rects.push(MotionTextRevealRect {
                x: from.min(to),
                y: top,
                width: (to - from).abs(),
                height,
            });
        }
    }
    if rects.is_empty() {
        return MotionTextReveal::Nothing;
    }
    MotionTextReveal::Rects(rects)
}

pub(in crate::capture::editor::window) fn motion_text_placement(
    context: &Context,
    card_layout: CardLayout,
    segment: &MotionTextSegment,
    time: f64,
) -> Option<MotionTextPlacement> {
    let style = segment.sample(time)?;
    let mut reference = motion_text_reference(card_layout, segment)?;
    let format = &segment.format;
    let text = motion_text_content(segment);
    let wrap_width = if format.wrap_width > 0.0 {
        format.wrap_width.clamp(MIN_MOTION_TEXT_WIDTH, 1.0) * reference.width
    } else {
        0.0
    };
    let layout = motion_text_layout(context, format, text, reference.font_size, wrap_width);
    let (ink, logical) = layout.pixel_extents();
    let box_w = if wrap_width > 0.0 {
        wrap_width
    } else {
        f64::from(logical.width())
    };
    let box_h = f64::from(logical.height());
    let outline = format.outline_width * reference.font_size * 0.5;
    let shadow = if format.shadow {
        reference.font_size * 0.06
    } else {
        0.0
    };
    let left = f64::from(ink.x()) - outline;
    let top = f64::from(ink.y()) - outline;
    let right = f64::from(ink.x() + ink.width()) + outline;
    let bottom = f64::from(ink.y() + ink.height()) + outline + shadow;
    let rotation = motion_text_box_matrix(0.0, 0.0, box_w, box_h, format.rotation, 1.0);
    let corners = [(left, top), (right, top), (right, bottom), (left, bottom)]
        .map(|(x, y)| rotation.transform_point(x, y));
    let min_x = corners
        .iter()
        .map(|point| point.0)
        .fold(f64::INFINITY, f64::min);
    let max_x = corners
        .iter()
        .map(|point| point.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = corners
        .iter()
        .map(|point| point.1)
        .fold(f64::INFINITY, f64::min);
    let max_y = corners
        .iter()
        .map(|point| point.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let inset = reference.font_size * 0.03;
    let scale = ((reference.width - inset * 2.0).max(1.0) / (max_x - min_x).max(1.0))
        .min((reference.height - inset * 2.0).max(1.0) / (max_y - min_y).max(1.0))
        .min(1.0);
    let half_w = (max_x - min_x) * scale * 0.5;
    let half_h = (max_y - min_y) * scale * 0.5;
    let position_range = [
        (half_w + inset, reference.width - half_w - inset),
        (half_h + inset, reference.height - half_h - inset),
    ];
    let position_band = if segment.annotation_coordinate_space.is_canvas() {
        (0.0, 1.0)
    } else {
        (MIN_MOTION_TEXT_POS, MAX_MOTION_TEXT_POS)
    };
    let position = |value: f64, extent: f64, (low, high): (f64, f64)| {
        (value.clamp(position_band.0, position_band.1) * extent).clamp(low, high.max(low))
    };
    let anchor = (
        position(segment.pos_x, reference.width, position_range[0]),
        position(segment.pos_y, reference.height, position_range[1]),
    );
    if let Some(card) = reference.card {
        reference.matrix = card.local_matrix(anchor.0, anchor.1)?;
    }
    let center_x =
        anchor.0 - (min_x + max_x) * scale * 0.5 + style.offset_x * reference.height / 1080.0;
    let center_y =
        anchor.1 - (min_y + max_y) * scale * 0.5 + style.offset_y * reference.height / 1080.0;
    let to_stage = motion_text_compose(
        &reference.matrix,
        &motion_text_box_matrix(center_x, center_y, box_w, box_h, format.rotation, scale),
    );
    let reveal = motion_text_reveal(
        &layout,
        text,
        segment.scope,
        style.reveal,
        box_w,
        reference.font_size,
    );
    Some(MotionTextPlacement {
        layout,
        style,
        format: format.clone(),
        font_size: reference.font_size,
        to_stage,
        reveal,
        reference,
        position_range,
        position_band,
        anchor,
    })
}

impl MotionTextPlacement {
    fn paint(&self, context: &Context) {
        let format_color = self.format.color;
        let alpha = self.effective_alpha();
        if alpha < 0.02 || self.reveal == MotionTextReveal::Nothing {
            return;
        }
        let _ = context.save();
        if let Some(card) = self.reference.card {
            let points = [
                card.project(0.0, 0.0),
                card.project(card.img_w(), 0.0),
                card.project(card.img_w(), card.img_h()),
                card.project(0.0, card.img_h()),
            ];
            path_through_points(context, &points);
            context.clip();
        }
        context.transform(self.to_stage);
        self.reveal.clip(context);
        context.set_line_join(gtk4::cairo::LineJoin::Round);
        context.set_line_cap(gtk4::cairo::LineCap::Round);
        let outline = self.format.outline_width * self.font_size;
        context.set_line_width(outline.max(0.0));
        if self.format.shadow {
            let _ = context.save();
            context.translate(0.0, self.font_size * 0.06);
            context.set_source_rgba(0.0, 0.0, 0.0, alpha * 0.42);
            pangocairo::functions::layout_path(context, &self.layout);
            if outline > 0.0 {
                let _ = context.stroke_preserve();
            }
            let _ = context.fill();
            let _ = context.restore();
        }
        context.set_source_rgba(format_color[0], format_color[1], format_color[2], alpha);
        pangocairo::functions::layout_path(context, &self.layout);
        if outline > 0.0 {
            let _ = context.stroke_preserve();
        }
        let _ = context.fill();
        let _ = context.restore();
    }
}

/// Draw every title. Card-attached titles follow the camera; Canvas titles are
/// a layer of their own and never inherit the image transform.
fn paint_motion_text(context: &Context, card_layout: CardLayout, motion: &MotionState, time: f64) {
    for segment in &motion.text_segments {
        let Some(placement) = motion_text_placement(context, card_layout, segment, time) else {
            continue;
        };
        placement.paint(context);
    }
}

/// Where a title's anchor lands in preview view coordinates, without its
/// entrance offset: the point a placement drag steers.
#[cfg(test)]
pub fn motion_text_anchor_view_point(
    card_layout: CardLayout,
    segment: &MotionTextSegment,
) -> Option<(f64, f64)> {
    let surface = ImageSurface::create(Format::ARgb32, 1, 1).ok()?;
    let context = Context::new(&surface).ok()?;
    let placement = motion_text_placement(&context, card_layout, segment, segment.start)?;
    Some(placement.anchor_view_point())
}

/// Whether a preview pointer is on the rendered text itself.
#[cfg(test)]
pub fn motion_text_contains_view_point(
    card_layout: CardLayout,
    segment: &MotionTextSegment,
    time: f64,
    view_x: f64,
    view_y: f64,
) -> bool {
    let Ok(surface) = ImageSurface::create(Format::ARgb32, 1, 1) else {
        return false;
    };
    let Ok(context) = Context::new(&surface) else {
        return false;
    };
    let Some(placement) = motion_text_placement(&context, card_layout, segment, time) else {
        return false;
    };
    placement.contains_stage_point(view_x, view_y, placement.local_slop(6.0))
}
