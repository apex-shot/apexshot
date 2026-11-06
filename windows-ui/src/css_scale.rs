//! Scale stylesheets authored for a 100% display.
//!
//! GDK reports a scale of 1 on this Windows build even when the display runs at
//! 125% or 150%; [`crate::platform::ui_scale`] carries the system scale instead.
//! Sheets that were authored at 100% are passed through here so their type,
//! spacing and control metrics keep their proportions on a scaled display.

/// Scale every `px` length in `css` by `scale`.
///
/// Comments quote pixel sizes in prose, so they are copied untouched. Other
/// units, plain numbers and multi-byte characters are left exactly as written.
pub fn scale_px_lengths(css: &str, scale: f64) -> String {
    if (scale - 1.0).abs() < 1e-3 {
        return css.to_owned();
    }

    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len() + css.len() / 16);
    let mut index = 0;
    while index < bytes.len() {
        if css[index..].starts_with("/*") {
            let end = css[index + 2..]
                .find("*/")
                .map_or(bytes.len(), |offset| index + 2 + offset + 2);
            out.push_str(&css[index..end]);
            index = end;
            continue;
        }

        let byte = bytes[index];
        let number_starts = byte.is_ascii_digit()
            || (byte == b'.'
                && bytes
                    .get(index + 1)
                    .is_some_and(|next| next.is_ascii_digit()));
        if !number_starts {
            // Copy one whole character so multi-byte comments survive.
            let character = css[index..].chars().next().unwrap_or_default();
            out.push(character);
            index += character.len_utf8();
            continue;
        }

        let start = index;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
            index += 1;
        }
        let number = &css[start..index];
        if css[index..].starts_with("px") {
            let value = number.parse::<f64>().unwrap_or(1.0);
            out.push_str(&format_length(value * scale));
            out.push_str("px");
            index += 2;
        } else {
            out.push_str(number);
        }
    }
    out
}

/// Scale only the `font-size` values in `css`, leaving every other length as
/// authored.
///
/// The Settings type sheet is layered over a shell sheet whose metrics already
/// carry the display scale, so scaling the whole sheet would grow its spacing
/// twice. Type is the part that has to follow the display, and it is the part
/// this leaves alone everywhere else.
pub fn scale_font_sizes(css: &str, scale: f64) -> String {
    const PROPERTY: &str = "font-size";
    if (scale - 1.0).abs() < 1e-3 {
        return css.to_owned();
    }

    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len() + 32);
    let mut index = 0;
    while index < bytes.len() {
        if css[index..].starts_with("/*") {
            let end = css[index + 2..]
                .find("*/")
                .map_or(bytes.len(), |offset| index + 2 + offset + 2);
            out.push_str(&css[index..end]);
            index = end;
            continue;
        }

        if !css[index..].starts_with(PROPERTY) {
            // Copy one whole character so multi-byte text survives.
            let character = css[index..].chars().next().unwrap_or_default();
            out.push(character);
            index += character.len_utf8();
            continue;
        }

        // The identifier only counts as a property when its value is a px
        // length, so a selector or comment that mentions the name is copied.
        match px_value_after(bytes, css, index + PROPERTY.len()) {
            Some((start, end)) => {
                out.push_str(&css[index..start]);
                let value = css[start..end].parse::<f64>().unwrap_or(1.0);
                out.push_str(&format_length(value * scale));
                out.push_str("px");
                index = end + 2;
            }
            None => {
                out.push_str(PROPERTY);
                index += PROPERTY.len();
            }
        }
    }
    out
}

/// The byte range of the number in `: <number>px`, read from `cursor`.
fn px_value_after(bytes: &[u8], css: &str, cursor: usize) -> Option<(usize, usize)> {
    let mut at = cursor;
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    if bytes.get(at) != Some(&b':') {
        return None;
    }
    let mut start = at + 1;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    let mut end = start;
    while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
        end += 1;
    }
    (end > start && css[end..].starts_with("px")).then_some((start, end))
}

fn format_length(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 {
        format!("{}", rounded as i64)
    } else {
        let text = format!("{rounded:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{scale_font_sizes, scale_px_lengths};

    #[test]
    fn every_px_length_is_scaled() {
        assert_eq!(
            scale_px_lengths(
                ".a { font-size: 12px; padding: 0 14px 6px 14px; border: 1px solid red; }",
                1.25
            ),
            ".a { font-size: 15px; padding: 0 17.5px 7.5px 17.5px; border: 1.25px solid red; }"
        );
    }

    #[test]
    fn fractions_and_negatives_keep_their_precision() {
        assert_eq!(
            scale_px_lengths("letter-spacing: 0.6px; margin-left: -2px;", 1.5),
            "letter-spacing: 0.9px; margin-left: -3px;"
        );
    }

    #[test]
    fn other_numbers_and_units_are_untouched() {
        let css = "font-weight: 600; color: #ff670d; background: alpha(white, 0.10); \
                   margin: 0 0 8rem 0; border-radius: 0;";
        assert_eq!(scale_px_lengths(css, 2.0), css);
    }

    #[test]
    fn comments_survive_untouched() {
        let css = "/* 12px ── editor */ .a { min-height: 20px; }";
        assert_eq!(
            scale_px_lengths(css, 1.5),
            "/* 12px ── editor */ .a { min-height: 30px; }"
        );
    }

    #[test]
    fn unit_scale_is_a_no_op() {
        let css = ".a { font-size: 12px; }";
        assert_eq!(scale_px_lengths(css, 1.0), css);
        assert_eq!(scale_font_sizes(css, 1.0), css);
    }

    #[test]
    fn font_sizes_scale_without_touching_spacing() {
        assert_eq!(
            scale_font_sizes(
                ".a { font-size: 14px; font-weight: 600; margin-top: 24px; \
                 letter-spacing: -0.3px; } .b { font-size:12px; } ",
                1.25
            ),
            ".a { font-size: 17.5px; font-weight: 600; margin-top: 24px; \
             letter-spacing: -0.3px; } .b { font-size:15px; } "
        );
    }

    #[test]
    fn selectors_and_comments_keep_their_numbers() {
        assert_eq!(
            scale_font_sizes(
                "/* font-size: 12px; */ .font-size-row { font-size: 13px; }",
                1.5
            ),
            "/* font-size: 12px; */ .font-size-row { font-size: 19.5px; }"
        );
    }

    #[test]
    fn other_units_and_plain_numbers_are_left_alone() {
        let css = ".a { font-size: 1.1em; } .b { font-size: large; } \
                   .c { font-size: 0; } .d { font-size: 12; }";
        assert_eq!(scale_font_sizes(css, 2.0), css);
    }
}
