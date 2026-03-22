//! Numbering style and size types for number markers

/// Numbering style for sequential markers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NumberingStyle {
    /// Numeric: 1, 2, 3, 4...
    #[default]
    Numeric,
    /// Uppercase alphabetic: A, B, C, D...
    Uppercase,
    /// Lowercase alphabetic: a, b, c, d...
    Lowercase,
    /// Roman numerals: i, ii, iii, iv...
    Roman,
}

impl NumberingStyle {
    /// All available numbering styles
    pub const ALL: [Self; 4] = [Self::Numeric, Self::Uppercase, Self::Lowercase, Self::Roman];

    /// Format a number according to this style
    pub fn format(&self, number: u32) -> String {
        match self {
            Self::Numeric => number.to_string(),
            Self::Uppercase => Self::to_alpha(number, true),
            Self::Lowercase => Self::to_alpha(number, false),
            Self::Roman => Self::to_roman(number),
        }
    }

    /// Get display label for UI
    pub fn label(&self) -> &'static str {
        match self {
            Self::Numeric => "1, 2, 3, 4...",
            Self::Uppercase => "A, B, C, D...",
            Self::Lowercase => "a, b, c, d...",
            Self::Roman => "i, ii, iii, iv...",
        }
    }

    /// Get index for UI list
    pub fn index(&self) -> usize {
        match self {
            Self::Numeric => 0,
            Self::Uppercase => 1,
            Self::Lowercase => 2,
            Self::Roman => 3,
        }
    }

    /// Create from index
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Numeric,
            1 => Self::Uppercase,
            2 => Self::Lowercase,
            3 => Self::Roman,
            _ => Self::default(),
        }
    }

    /// Convert number to alphabetic representation (Excel-style columns)
    /// 1=A, 2=B, ..., 26=Z, 27=AA, 28=AB...
    fn to_alpha(n: u32, uppercase: bool) -> String {
        if n == 0 {
            return if uppercase {
                "A".to_string()
            } else {
                "a".to_string()
            };
        }

        let mut result = String::new();
        let mut num = n;

        while num > 0 {
            num -= 1;
            let remainder = (num % 26) as u8;
            let c = if uppercase {
                (b'A' + remainder) as char
            } else {
                (b'a' + remainder) as char
            };
            result.push(c);
            num /= 26;
        }

        result.chars().rev().collect()
    }

    /// Convert number to lowercase Roman numerals
    /// 1=i, 2=ii, 3=iii, 4=iv, 5=v...
    fn to_roman(n: u32) -> String {
        if n == 0 {
            return String::new();
        }

        // Support up to 3999 (MMMCMXCIX)
        let numerals = [
            (1000, "m"),
            (900, "cm"),
            (500, "d"),
            (400, "cd"),
            (100, "c"),
            (90, "xc"),
            (50, "l"),
            (40, "xl"),
            (10, "x"),
            (9, "ix"),
            (5, "v"),
            (4, "iv"),
            (1, "i"),
        ];

        let mut result = String::new();
        let mut num = n;

        for (value, symbol) in numerals {
            while num >= value {
                result.push_str(symbol);
                num -= value;
            }
        }

        result
    }
}

/// Size preset for number markers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NumberSize {
    /// Small: radius 12px, font 11pt
    Small,
    /// Medium: radius 15px, font 14pt
    #[default]
    Medium,
    /// Large: radius 20px, font 18pt
    Large,
    /// Extra Large: radius 25px, font 22pt
    ExtraLarge,
}

impl NumberSize {
    /// All available sizes
    pub const ALL: [Self; 4] = [Self::Small, Self::Medium, Self::Large, Self::ExtraLarge];

    /// Get the circle radius in pixels
    pub fn radius(&self) -> f64 {
        match self {
            Self::Small => 12.0,
            Self::Medium => 15.0,
            Self::Large => 20.0,
            Self::ExtraLarge => 25.0,
        }
    }

    /// Get the font size in points
    pub fn font_size(&self) -> f64 {
        match self {
            Self::Small => 11.0,
            Self::Medium => 14.0,
            Self::Large => 18.0,
            Self::ExtraLarge => 22.0,
        }
    }

    /// Get display label for UI
    pub fn label(&self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
            Self::ExtraLarge => "Extra Large",
        }
    }

    /// Get index for UI list
    pub fn index(&self) -> usize {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
            Self::ExtraLarge => 3,
        }
    }

    /// Create from index
    pub fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Small,
            1 => Self::Medium,
            2 => Self::Large,
            3 => Self::ExtraLarge,
            _ => Self::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numeric_style() {
        assert_eq!(NumberingStyle::Numeric.format(1), "1");
        assert_eq!(NumberingStyle::Numeric.format(10), "10");
        assert_eq!(NumberingStyle::Numeric.format(100), "100");
    }

    #[test]
    fn test_uppercase_alpha() {
        assert_eq!(NumberingStyle::Uppercase.format(1), "A");
        assert_eq!(NumberingStyle::Uppercase.format(26), "Z");
        assert_eq!(NumberingStyle::Uppercase.format(27), "AA");
        assert_eq!(NumberingStyle::Uppercase.format(28), "AB");
        assert_eq!(NumberingStyle::Uppercase.format(52), "AZ");
        assert_eq!(NumberingStyle::Uppercase.format(53), "BA");
    }

    #[test]
    fn test_lowercase_alpha() {
        assert_eq!(NumberingStyle::Lowercase.format(1), "a");
        assert_eq!(NumberingStyle::Lowercase.format(26), "z");
        assert_eq!(NumberingStyle::Lowercase.format(27), "aa");
        assert_eq!(NumberingStyle::Lowercase.format(28), "ab");
    }

    #[test]
    fn test_roman_numerals() {
        assert_eq!(NumberingStyle::Roman.format(1), "i");
        assert_eq!(NumberingStyle::Roman.format(2), "ii");
        assert_eq!(NumberingStyle::Roman.format(3), "iii");
        assert_eq!(NumberingStyle::Roman.format(4), "iv");
        assert_eq!(NumberingStyle::Roman.format(5), "v");
        assert_eq!(NumberingStyle::Roman.format(10), "x");
        assert_eq!(NumberingStyle::Roman.format(50), "l");
        assert_eq!(NumberingStyle::Roman.format(100), "c");
        assert_eq!(NumberingStyle::Roman.format(500), "d");
        assert_eq!(NumberingStyle::Roman.format(1000), "m");
        assert_eq!(NumberingStyle::Roman.format(2024), "mmxxiv");
    }

    #[test]
    fn test_number_size() {
        assert_eq!(NumberSize::Small.radius(), 12.0);
        assert_eq!(NumberSize::Medium.radius(), 15.0);
        assert_eq!(NumberSize::Large.radius(), 20.0);
        assert_eq!(NumberSize::ExtraLarge.radius(), 25.0);

        assert_eq!(NumberSize::Small.font_size(), 11.0);
        assert_eq!(NumberSize::Medium.font_size(), 14.0);
        assert_eq!(NumberSize::Large.font_size(), 18.0);
        assert_eq!(NumberSize::ExtraLarge.font_size(), 22.0);
    }
}
