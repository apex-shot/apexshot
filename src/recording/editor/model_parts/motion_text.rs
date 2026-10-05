#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionTextAnimation {
    None,
    Typewriter,
    SlideFromLeft,
    SlideFromRight,
    SlideTop,
    SlideBottom,
    Fade,
}

impl MotionTextAnimation {
    /// Cases recovered from the `TextEffectPreset` metadata, extended with Fade.
    pub const ALL: [Self; 7] = [
        Self::None,
        Self::Typewriter,
        Self::SlideFromLeft,
        Self::SlideFromRight,
        Self::SlideTop,
        Self::SlideBottom,
        Self::Fade,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Typewriter => "Typewriter",
            Self::SlideFromLeft => "From left",
            Self::SlideFromRight => "From right",
            Self::SlideTop => "From top",
            Self::SlideBottom => "From bottom",
            Self::Fade => "Fade",
        }
    }
}

/// This is stored as the text effect `scope` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionTextScope {
    Character,
    Word,
    Line,
}

impl MotionTextScope {
    /// Cases recovered from the `TextEffectScope` metadata.
    pub const ALL: [Self; 3] = [Self::Character, Self::Word, Self::Line];

    pub fn label(self) -> &'static str {
        match self {
            Self::Character => "Character",
            Self::Word => "Word",
            Self::Line => "Line",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionTextStyle {
    pub alpha: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub reveal: f64,
}

/// Where a Motion text annotation is authored. These are the two coordinate
/// spaces named by `MotionTextSegment.annotationCoordinateSpace`, extended
/// with an independent composition canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionTextCoordinateSpace {
    /// Coordinates are normalized to the moving image card.
    #[default]
    MotionCanvasLocal,
    /// Coordinates are normalized to the untransformed source image.
    CanonicalSource,
    /// Coordinates are normalized to the whole composition, so the title
    /// stands beside the image instead of travelling with it.
    Canvas,
}

impl MotionTextCoordinateSpace {
    /// Both original cases lay a title on the moving card; they keep that
    /// meaning so files written before the Canvas space existed render
    /// exactly where they did.
    pub fn is_canvas(self) -> bool {
        matches!(self, Self::Canvas)
    }

    /// The attachment switch's card-relative case. CanonicalSource also means
    /// "on the card", so Image maps every legacy case to one entry.
    pub fn image() -> Self {
        Self::MotionCanvasLocal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionTextAlignment {
    Left,
    #[default]
    Center,
    Right,
}

impl MotionTextAlignment {
    pub const ALL: [Self; 3] = [Self::Left, Self::Center, Self::Right];

    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Center => "Center",
            Self::Right => "Right",
        }
    }
}

/// Typography for one title. The defaults are the legacy card title: bold
/// white text centred on its anchor with a soft shadow and natural width.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionTextFormat {
    pub font_family: String,
    pub bold: bool,
    pub italic: bool,
    /// RGB with alpha as the title's opacity.
    pub color: [f64; 4],
    pub alignment: MotionTextAlignment,
    /// Paragraph-box width as a fraction of the coordinate reference width;
    /// 0 is the legacy natural width.
    pub wrap_width: f64,
    pub line_spacing: f64,
    /// Letter spacing in em.
    pub letter_spacing: f64,
    pub rotation: f64,
    /// Outline width in em; 0 draws no outline.
    pub outline_width: f64,
    pub shadow: bool,
}

impl Default for MotionTextFormat {
    fn default() -> Self {
        Self {
            font_family: crate::typography::UI_FONT_FAMILY.to_string(),
            bold: true,
            italic: false,
            color: [1.0, 1.0, 1.0, 1.0],
            alignment: MotionTextAlignment::Center,
            wrap_width: 0.0,
            line_spacing: DEFAULT_MOTION_TEXT_LINE_SPACING,
            letter_spacing: DEFAULT_MOTION_TEXT_LETTER_SPACING,
            rotation: 0.0,
            outline_width: 0.0,
            shadow: true,
        }
    }
}

impl MotionTextFormat {
    /// A background headline: left-aligned in a paragraph box, no card shadow.
    pub fn canvas() -> Self {
        Self {
            alignment: MotionTextAlignment::Left,
            wrap_width: DEFAULT_MOTION_TEXT_CANVAS_WIDTH,
            shadow: false,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MotionTextSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub animation: MotionTextAnimation,
    pub scope: MotionTextScope,
    pub typewriter_time: f64,
    pub is_disabled: bool,
    pub annotation_coordinate_space: MotionTextCoordinateSpace,
    pub pos_x: f64,
    pub pos_y: f64,
    pub size: f64,
    pub format: MotionTextFormat,
    /// Entrance length for the slide and fade animations. It runs for its
    /// whole requested length, capped only by the clip that holds it.
    pub transition_duration: f64,
}

impl MotionTextSegment {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    /// Seconds the entrance of this title needs, whichever animation it uses.
    pub fn entrance_seconds(&self) -> f64 {
        match self.animation {
            MotionTextAnimation::Typewriter => self.typewriter_time,
            _ => self.transition_duration,
        }
    }

    pub fn sample(&self, time: f64) -> Option<MotionTextStyle> {
        if self.is_disabled || time < self.start || time > self.end {
            return None;
        }
        let span = self.duration();
        if span <= f64::EPSILON {
            return Some(MotionTextStyle {
                alpha: 1.0,
                offset_x: 0.0,
                offset_y: 0.0,
                reveal: 1.0,
            });
        }
        let local = time - self.start;
        let transition = self
            .transition_duration
            .clamp(
                MIN_MOTION_TEXT_TRANSITION_SECONDS,
                MAX_MOTION_TEXT_TRANSITION_SECONDS,
            )
            .min(span);
        let progress = if transition <= f64::EPSILON {
            1.0
        } else {
            (local / transition).clamp(0.0, 1.0)
        };
        let entrance = 1.0 - (1.0 - progress).powi(3);
        let distance = 36.0 * (1.0 - entrance);
        let (alpha, offset_x, offset_y, reveal) = match self.animation {
            MotionTextAnimation::None => (1.0, 0.0, 0.0, 1.0),
            MotionTextAnimation::Typewriter => (
                1.0,
                0.0,
                0.0,
                (local / self.typewriter_time.max(0.05)).clamp(0.0, 1.0),
            ),
            MotionTextAnimation::SlideFromLeft => (entrance, -distance, 0.0, 1.0),
            MotionTextAnimation::SlideFromRight => (entrance, distance, 0.0, 1.0),
            MotionTextAnimation::SlideTop => (entrance, 0.0, -distance, 1.0),
            MotionTextAnimation::SlideBottom => (entrance, 0.0, distance, 1.0),
            MotionTextAnimation::Fade => (entrance, 0.0, 0.0, 1.0),
        };
        Some(MotionTextStyle { alpha, offset_x, offset_y, reveal })
    }
}
