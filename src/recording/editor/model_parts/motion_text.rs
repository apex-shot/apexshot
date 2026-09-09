#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionTextAnimation {
    None,
    Typewriter,
    SlideFromLeft,
    SlideFromRight,
    SlideTop,
    SlideBottom,
}

impl MotionTextAnimation {
    /// Cases recovered from Shotbase `TextEffectPreset` metadata.
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Typewriter,
        Self::SlideFromLeft,
        Self::SlideFromRight,
        Self::SlideTop,
        Self::SlideBottom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Typewriter => "Typewriter",
            Self::SlideFromLeft => "From left",
            Self::SlideFromRight => "From right",
            Self::SlideTop => "From top",
            Self::SlideBottom => "From bottom",
        }
    }
}

/// Shotbase stores this as the text effect `scope` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionTextScope {
    Character,
    Word,
    Line,
}

impl MotionTextScope {
    /// Cases recovered from Shotbase's `TextEffectScope` metadata.
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
/// spaces named by Shotbase's `MotionTextSegment.annotationCoordinateSpace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionTextCoordinateSpace {
    /// Coordinates are normalized to the moving image card.
    #[default]
    MotionCanvasLocal,
    /// Coordinates are normalized to the untransformed source image.
    CanonicalSource,
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
}

impl MotionTextSegment {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
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
        let transition = 0.28_f64.min(span / 3.0).max(0.05);
        let progress = (local / transition).clamp(0.0, 1.0);
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
        };
        Some(MotionTextStyle { alpha, offset_x, offset_y, reveal })
    }
}
