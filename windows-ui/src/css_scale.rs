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
    use super::scale_px_lengths;

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
    }
}
