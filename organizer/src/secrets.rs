//! Finds secrets in recognised text so they never reach a filename or the
//! search index, and so copies can be redacted where they appear.

use crate::OcrPage;

/// What kind of secret a span holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretKind {
    Email,
    CardNumber,
    ApiKey,
    Jwt,
    Password,
}

/// A secret inside a string, as byte offsets (`text[start..end]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretSpan {
    pub kind: SecretKind,
    pub start: usize,
    pub end: usize,
}

/// Every secret in `text`, sorted by `start`, never overlapping.
pub fn find_secrets(text: &str) -> Vec<SecretSpan> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    find_emails(bytes, &mut spans);
    find_cards(bytes, &mut spans);
    find_jwts(bytes, &mut spans);
    find_api_keys(bytes, &mut spans);
    find_passwords(text, &mut spans);
    resolve_overlaps(spans)
}

/// `text` with every secret removed and the surrounding whitespace collapsed.
pub fn scrub(text: &str) -> String {
    let mut rest = String::with_capacity(text.len());
    let mut cursor = 0;
    for span in find_secrets(text) {
        rest.push_str(&text[cursor..span.start]);
        cursor = span.end;
    }
    rest.push_str(&text[cursor..]);
    tidy_lines(&rest)
}

/// Indices into `page.lines` of the lines holding a secret, with its kind.
pub fn secret_lines(page: &OcrPage) -> Vec<(usize, SecretKind)> {
    page.lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            find_secrets(&line.text)
                .first()
                .map(|span| (index, span.kind))
        })
        .collect()
}

/// Bytes that may continue a keyword-like token.
fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

/// Bytes that may appear in a domain label.
fn is_label_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}

/// Bytes that may appear in the part of an address before the `@`.
fn is_local_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn find_emails(bytes: &[u8], out: &mut Vec<SecretSpan>) {
    for at in 0..bytes.len() {
        if bytes[at] != b'@' {
            continue;
        }
        let Some(start) = local_part_start(bytes, at) else {
            continue;
        };
        // A complete address, or one cut off on screen ("name@gmai…"): the
        // part before the `@` already identifies the person.
        let Some(end) = domain_end(bytes, at + 1).or_else(|| truncated_domain_end(bytes, at + 1))
        else {
            continue;
        };
        out.push(SecretSpan {
            kind: SecretKind::Email,
            start,
            end,
        });
    }
}

fn local_part_start(bytes: &[u8], at: usize) -> Option<usize> {
    let mut start = at;
    while start > 0 && is_local_byte(bytes[start - 1]) {
        start -= 1;
    }
    (start < at).then_some(start)
}

/// End of a domain that was cut off before its TLD, including the ellipsis
/// or dots that cut it off. None when nothing host-like follows the `@`.
fn truncated_domain_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut pos = start;
    while pos < bytes.len() && (is_label_byte(bytes[pos]) || bytes[pos] == b'.') {
        pos += 1;
    }
    if pos == start || !bytes[start].is_ascii_alphanumeric() {
        return None;
    }
    // A "@2x" density marker on a file name, not a cut-off address.
    let run = &bytes[start..pos];
    if let Some((last, digits)) = run.split_last() {
        if matches!(last, b'x' | b'X')
            && !digits.is_empty()
            && digits.iter().all(u8::is_ascii_digit)
        {
            return None;
        }
    }
    // "…" is three bytes in UTF-8.
    if bytes[pos..].starts_with("\u{2026}".as_bytes()) {
        pos += "\u{2026}".len();
    }
    Some(pos)
}

/// End of the first host-like run that ends in a plausible TLD.
fn domain_end(bytes: &[u8], start: usize) -> Option<usize> {
    let len = bytes.len();
    let mut labels: Vec<(usize, usize)> = Vec::new();
    let mut pos = start;
    while pos < len {
        let label_start = pos;
        while pos < len && is_label_byte(bytes[pos]) {
            pos += 1;
        }
        if pos == label_start || bytes[label_start] == b'-' || bytes[pos - 1] == b'-' {
            break;
        }
        labels.push((label_start, pos));
        if pos < len && bytes[pos] == b'.' {
            pos += 1;
        } else {
            break;
        }
    }
    // The last label decides: "co.uk." ends at "uk", not at the trailing dot.
    while labels.len() >= 2 {
        let (label_start, label_end) = labels[labels.len() - 1];
        let tld = &bytes[label_start..label_end];
        if tld.len() >= 2 && tld.iter().all(u8::is_ascii_alphabetic) {
            return Some(label_end);
        }
        labels.pop();
    }
    None
}

fn find_cards(bytes: &[u8], out: &mut Vec<SecretSpan>) {
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        let start = i;
        if !bytes[start].is_ascii_digit() || (start > 0 && bytes[start - 1].is_ascii_alphanumeric())
        {
            i += 1;
            continue;
        }
        let mut digits: Vec<u8> = Vec::with_capacity(20);
        let mut pos = start;
        let mut end = start;
        loop {
            let group_start = pos;
            while pos < len && bytes[pos].is_ascii_digit() {
                if digits.len() < 20 {
                    digits.push(bytes[pos] - b'0');
                }
                pos += 1;
            }
            if pos == group_start {
                break;
            }
            end = pos;
            if pos + 1 < len
                && (bytes[pos] == b' ' || bytes[pos] == b'-')
                && bytes[pos + 1].is_ascii_digit()
            {
                pos += 1;
            } else {
                break;
            }
        }
        i = pos.max(start + 1);
        if digits.len() < 13 || digits.len() > 19 {
            continue;
        }
        if pos < len && bytes[pos].is_ascii_alphanumeric() {
            continue;
        }
        if luhn(&digits) {
            out.push(SecretSpan {
                kind: SecretKind::CardNumber,
                start,
                end,
            });
        }
    }
}

fn luhn(digits: &[u8]) -> bool {
    let mut sum = 0u32;
    let mut double = false;
    for &digit in digits.iter().rev() {
        let mut value = u32::from(digit);
        if double {
            value *= 2;
            if value > 9 {
                value -= 9;
            }
        }
        sum += value;
        double = !double;
    }
    sum.is_multiple_of(10)
}

fn find_jwts(bytes: &[u8], out: &mut Vec<SecretSpan>) {
    let len = bytes.len();
    let mut i = 0;
    while i + 3 <= len {
        if !bytes[i..].starts_with(b"eyJ") {
            i += 1;
            continue;
        }
        // A token that starts mid-run is a longer token, not a JWT header.
        if i > 0 && is_token_byte(bytes[i - 1]) {
            i += 1;
            continue;
        }
        let Some(end) = jwt_end(bytes, i) else {
            i += 1;
            continue;
        };
        out.push(SecretSpan {
            kind: SecretKind::Jwt,
            start: i,
            end,
        });
        i = end;
    }
}

fn jwt_end(bytes: &[u8], start: usize) -> Option<usize> {
    let len = bytes.len();
    let mut pos = start;
    for segment in 0..3 {
        let segment_start = pos;
        while pos < len && is_token_byte(bytes[pos]) {
            pos += 1;
        }
        if pos - segment_start < 10 {
            return None;
        }
        if segment < 2 {
            if pos >= len || bytes[pos] != b'.' {
                return None;
            }
            pos += 1;
        }
    }
    Some(pos)
}

/// Well-known key prefixes, each needing at least 16 more characters.
const API_KEY_PREFIXES: [&str; 18] = [
    "sk-",
    "sk_live_",
    "sk_test_",
    "pk_live_",
    "rk_live_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xoxs-",
    "hf_",
    "npm_",
];

fn find_api_keys(bytes: &[u8], out: &mut Vec<SecretSpan>) {
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if !is_token_byte(bytes[i]) || (i > 0 && is_token_byte(bytes[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < len && is_token_byte(bytes[i]) {
            i += 1;
        }
        let token = &bytes[start..i];
        if is_known_api_key(token) || is_generic_api_key(token) {
            out.push(SecretSpan {
                kind: SecretKind::ApiKey,
                start,
                end: i,
            });
        }
    }
}

fn is_known_api_key(token: &[u8]) -> bool {
    if token.len() == 20 && token.starts_with(b"AKIA") {
        return token[4..]
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit());
    }
    if token.len() == 39 && token.starts_with(b"AIza") {
        return token[4..].iter().all(|byte| is_token_byte(*byte));
    }
    API_KEY_PREFIXES.iter().any(|prefix| {
        let prefix = prefix.as_bytes();
        token.len() >= prefix.len() + 16 && token.starts_with(prefix)
    })
}

fn is_generic_api_key(token: &[u8]) -> bool {
    if token.len() < 32 {
        return false;
    }
    let has_lower = token.iter().any(|byte| byte.is_ascii_lowercase());
    let has_upper = token.iter().any(|byte| byte.is_ascii_uppercase());
    let has_digit = token.iter().any(|byte| byte.is_ascii_digit());
    if !(has_lower && has_upper && has_digit) {
        return false;
    }
    // Commit hashes are hex runs and are not secrets.
    !token.iter().all(|byte| byte.is_ascii_hexdigit())
}

/// Labels that introduce a password value, longest matching first.
const PASSWORD_LABELS: [&str; 6] = ["password", "passphrase", "passwd", "secret", "token", "pwd"];

fn find_passwords(text: &str, out: &mut Vec<SecretSpan>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let Some(label_len) = password_label_at(text, i) else {
            i += 1;
            continue;
        };
        let after_label = i + label_len;
        let mut pos = after_label;
        while pos < bytes.len() && bytes[pos] == b' ' {
            pos += 1;
        }
        if pos >= bytes.len() || (bytes[pos] != b':' && bytes[pos] != b'=') {
            i = after_label;
            continue;
        }
        pos += 1;
        while pos < bytes.len() && bytes[pos] == b' ' {
            pos += 1;
        }
        let start = pos;
        let mut end = pos;
        let mut chars = 0;
        for (offset, ch) in text[pos..].char_indices() {
            if ch.is_whitespace() {
                break;
            }
            chars += 1;
            end = pos + offset + ch.len_utf8();
        }
        if chars >= 4 {
            out.push(SecretSpan {
                kind: SecretKind::Password,
                start,
                end,
            });
        }
        i = end.max(after_label);
    }
}

/// Length of a password label starting at byte `at`, if it is a whole word.
fn password_label_at(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut len = 0;
    for label in PASSWORD_LABELS {
        let end = at + label.len();
        if end <= bytes.len() && bytes[at..end].eq_ignore_ascii_case(label.as_bytes()) {
            len = len.max(label.len());
        }
    }
    if len == 0 {
        return None;
    }
    // "mypassword" is one word, not a label followed by a value.
    if at > 0
        && text[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_alphabetic)
    {
        return None;
    }
    Some(len)
}

fn overlaps(a: SecretSpan, b: SecretSpan) -> bool {
    a.start < b.end && b.start < a.end
}

fn resolve_overlaps(mut spans: Vec<SecretSpan>) -> Vec<SecretSpan> {
    spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut kept: Vec<SecretSpan> = Vec::with_capacity(spans.len());
    for span in spans {
        let overlap = kept.iter().position(|other| overlaps(*other, span));
        match overlap {
            Some(index) => {
                let span_len = span.end - span.start;
                let kept_len = kept[index].end - kept[index].start;
                // A wider match wins; equal widths keep the first one seen.
                // It still has to leave every other kept span untouched, so
                // the result stays non-overlapping for `scrub`.
                let straddles = kept
                    .iter()
                    .enumerate()
                    .any(|(other, other_span)| other != index && overlaps(*other_span, span));
                if span_len > kept_len && !straddles {
                    kept[index] = span;
                }
            }
            None => kept.push(span),
        }
    }
    kept.sort_by_key(|span| span.start);
    kept
}

fn tidy_lines(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let collapsed = collapse_spaces(line);
        let trimmed = collapsed.trim();
        if !trimmed.is_empty() {
            lines.push(trimmed.to_string());
        }
    }
    lines.join("\n")
}

fn collapse_spaces(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_space = false;
    for ch in line.chars() {
        if ch == ' ' || ch == '\t' {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(ch);
            in_space = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextLine;

    fn found(text: &str) -> Vec<(SecretKind, String)> {
        find_secrets(text)
            .into_iter()
            .map(|span| (span.kind, text[span.start..span.end].to_string()))
            .collect()
    }

    #[test]
    fn finds_an_email_cut_off_before_its_domain_ends() {
        for text in [
            "codegoddy@gmai... Pro",
            "codegoddy@gmai\u{2026} Pro",
            "codegoddy@gmai",
        ] {
            let scrubbed = scrub(text);
            assert!(!scrubbed.contains("codegoddy"), "{text:?} -> {scrubbed:?}");
        }
        // A handle with nothing before the `@` is not an address.
        assert!(scrub("Sam Lee @samlee").contains("samlee"));
        // Nor is a "@2x" density marker.
        assert!(scrub("Settings screenshot@2x").contains("screenshot@2x"));
    }

    #[test]
    fn finds_an_email_and_stops_before_the_trailing_dot() {
        assert_eq!(
            found("write to jane.doe+work@mail.example.co.uk."),
            vec![(
                SecretKind::Email,
                "jane.doe+work@mail.example.co.uk".to_string(),
            )]
        );
    }

    #[test]
    fn finds_card_numbers_only_when_luhn_passes() {
        assert_eq!(
            found("card 4111 1111 1111 1111 exp"),
            vec![(SecretKind::CardNumber, "4111 1111 1111 1111".to_string(),)]
        );
        assert_eq!(
            found("4111-1111-1111-1111"),
            vec![(SecretKind::CardNumber, "4111-1111-1111-1111".to_string(),)]
        );
        assert!(found("4111 1111 1111 1112").is_empty());
        assert!(found("order 1234567890123").is_empty());
    }

    #[test]
    fn finds_prefixed_api_keys() {
        let token = format!("ghp_{}", "a".repeat(36));
        let text = format!("key {token}");
        assert_eq!(found(&text), vec![(SecretKind::ApiKey, token)]);
        assert_eq!(
            found("AKIAIOSFODNN7EXAMPLE"),
            vec![(SecretKind::ApiKey, "AKIAIOSFODNN7EXAMPLE".to_string(),)]
        );
    }

    #[test]
    fn ignores_long_hex_strings() {
        assert!(found("0123456789abcdef0123456789abcdef01234567").is_empty());
    }

    #[test]
    fn finds_a_generic_api_key() {
        let token = "Xk9".repeat(11);
        assert_eq!(found(&token), vec![(SecretKind::ApiKey, token)]);
    }

    #[test]
    fn finds_a_whole_jwt() {
        let token = concat!(
            "eyJhbGciOiJIUzI1NiJ9",
            ".eyJzdWIiOiIxMjM0NSJ9",
            ".dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk",
        );
        assert_eq!(found(token), vec![(SecretKind::Jwt, token.to_string())]);
    }

    #[test]
    fn finds_password_values_only() {
        assert_eq!(
            found("Password: hunter22"),
            vec![(SecretKind::Password, "hunter22".to_string())]
        );
        assert_eq!(
            found("PWD=s3cr3t!"),
            vec![(SecretKind::Password, "s3cr3t!".to_string())]
        );
        assert!(found("password: ab").is_empty());
        assert!(found("mypassword: x1234").is_empty());
    }

    #[test]
    fn handles_non_ascii_around_a_password() {
        assert_eq!(
            found("café • Password: ••••••••"),
            vec![(SecretKind::Password, "••••••••".to_string())]
        );
    }

    #[test]
    fn scrubs_secrets_and_tidies_the_lines() {
        assert_eq!(scrub("Email me at a@b.com today"), "Email me at today");
        assert_eq!(
            scrub("line one\nPassword: hunter22\nline three"),
            "line one\nPassword:\nline three"
        );
    }

    #[test]
    fn keeps_one_span_when_an_email_sits_inside_a_password() {
        assert_eq!(find_secrets("password=me@example.com").len(), 1);
    }

    #[test]
    fn reports_lines_that_hold_a_secret() {
        let page = OcrPage {
            image_width: 100,
            image_height: 100,
            lines: vec![
                TextLine::new("Hello", 0, 0, 10, 10),
                TextLine::new(format!("key ghp_{}", "a".repeat(36)), 0, 20, 10, 10),
            ],
        };
        assert_eq!(secret_lines(&page), vec![(1, SecretKind::ApiKey)]);
    }
}
