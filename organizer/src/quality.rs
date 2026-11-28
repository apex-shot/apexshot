//! Keeps unreadable OCR out of file names.
//!
//! Text recognised from a small or dark capture often comes back as noise
//! ("TOM Bil QCc y Crarannchatc", "ee es Se history"). A name built from that
//! is worse than the default one, so a name made from on-screen text has to
//! read as words before a capture is renamed to it. Stray one- and two-letter
//! fragments are dropped first; what is left must be mostly real-looking
//! words, or the capture keeps the name it was saved with.

/// Short words a real name may contain.
const SHORT_WORDS: &[&str] = &[
    "a", "i", "an", "as", "at", "be", "by", "do", "go", "if", "in", "is", "it", "me", "my", "no",
    "of", "on", "or", "so", "to", "up", "us", "we",
];

/// The name with stray fragments removed, or None when what is left does not
/// read as words.
pub fn readable_name(stem: &str) -> Option<String> {
    let mut kept: Vec<&str> = Vec::new();
    let mut junk = 0usize;
    for token in stem.split_whitespace() {
        match classify(token) {
            Token::Word | Token::Solid => kept.push(token),
            Token::Joiner => kept.push(token),
            Token::Fragment => {}
            Token::Junk => {
                junk += 1;
                kept.push(token);
            }
        }
    }
    // Joiners ("-", "&", "·") only make sense between words.
    while kept.first().is_some_and(|t| classify(t) == Token::Joiner) {
        kept.remove(0);
    }
    while kept.last().is_some_and(|t| classify(t) == Token::Joiner) {
        kept.pop();
    }

    let words = kept
        .iter()
        .filter(|t| matches!(classify(t), Token::Word | Token::Solid | Token::Junk))
        .count();
    let solid = kept.iter().filter(|t| classify(t) == Token::Solid).count();
    // Two words at least, one of them clearly a word, and at most one noisy
    // token in five.
    if words < 2 || solid == 0 || junk * 5 > words {
        return None;
    }
    Some(kept.join(" "))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    /// A word that is clearly real: four or more letters that read like one.
    Solid,
    /// A short word, number, acronym, or model name ("PC", "2B", "i7-1370P").
    Word,
    /// Punctuation standing between words.
    Joiner,
    /// A one- or two-letter scrap that is not a word; dropped.
    Fragment,
    /// A longer token that does not read as a word.
    Junk,
}

fn classify(token: &str) -> Token {
    let core = token.trim_matches(|c: char| !c.is_alphanumeric());
    if core.is_empty() {
        return Token::Joiner;
    }
    // Other scripts are not judged by Latin spelling rules.
    if core.chars().any(|c| c.is_alphabetic() && !c.is_ascii()) {
        return Token::Solid;
    }
    if core.chars().any(|c| c.is_ascii_digit()) {
        return Token::Word;
    }
    let lower = core.to_ascii_lowercase();
    let len = core.chars().count();
    if len <= 2 {
        return if SHORT_WORDS.contains(&lower.as_str()) || is_acronym(core) {
            Token::Word
        } else {
            Token::Fragment
        };
    }
    if is_acronym(core) || core.strip_suffix('s').is_some_and(is_acronym) {
        return Token::Word;
    }
    if odd_capitals(core) {
        return Token::Junk;
    }
    // Spelling is judged on the letters alone ("What's" reads as "whats").
    let letters: String = lower.chars().filter(char::is_ascii_alphabetic).collect();
    let vowels = letters.chars().filter(|c| "aeiouy".contains(*c)).count();
    if vowels == 0 {
        return Token::Junk;
    }
    let letter_count = letters.len();
    if letter_count >= 4 && vowels * 10 >= letter_count * 2 && longest_consonant_run(&letters) <= 4
    {
        Token::Solid
    } else if len == 3 {
        Token::Word
    } else {
        Token::Junk
    }
}

/// All capitals, two to five letters: "PC", "UI", "HTML".
fn is_acronym(core: &str) -> bool {
    let len = core.chars().count();
    (2..=5).contains(&len) && core.chars().all(|c| c.is_ascii_uppercase())
}

/// Capitals in places a word does not have them: "QCc", "PCOMMMala".
/// One inner capital ("iPhone", "GitHub") and plural acronyms ("PCs") are fine.
fn odd_capitals(core: &str) -> bool {
    if core.strip_suffix('s').is_some_and(is_acronym) {
        return false;
    }
    let chars: Vec<char> = core.chars().collect();
    if !chars.iter().any(|c| c.is_ascii_lowercase()) {
        return false;
    }
    let capital_pair = chars
        .windows(2)
        .any(|w| w[0].is_ascii_uppercase() && w[1].is_ascii_uppercase());
    let inner_capitals = chars
        .windows(2)
        .filter(|w| w[0].is_ascii_lowercase() && w[1].is_ascii_uppercase())
        .count();
    capital_pair || inner_capitals > 1
}

fn longest_consonant_run(word: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for c in word.chars() {
        if c.is_ascii_alphabetic() && !"aeiouy".contains(c) {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::readable_name;

    #[test]
    fn real_names_pass_unchanged() {
        for name in [
            "Invoice reminder from Northwind Traders",
            "Rust organizer module with naming tests",
            "What I measured on your i7-1370P",
            "Settings app",
            "Standard - Qwen3.5 2B model on this PC",
            "Sam Lee post announcing a new keyboard",
            "Pull request 283 apex-shot apexshot",
            "iPhone setup guide",
            "Compare GPUs and PCs",
            "Настройки приложения",
        ] {
            assert_eq!(readable_name(name).as_deref(), Some(name), "{name}");
        }
    }

    #[test]
    fn stray_fragments_are_dropped() {
        assert_eq!(
            readable_name("& Video Edits screenshot").as_deref(),
            Some("Video Edits screenshot")
        );
        assert_eq!(
            readable_name("fe Pro What's included").as_deref(),
            Some("Pro What's included")
        );
        assert_eq!(
            readable_name("Standard - Qwen3.5 2B model on this PC g Remove").as_deref(),
            Some("Standard - Qwen3.5 2B model on this PC Remove")
        );
    }

    #[test]
    fn noise_is_refused() {
        for name in [
            "TOM Bil QCc y Crarannchatc",
            "ee es Se history",
            "ee es ~ Se",
            "TOM Bil",
            "Rall PCOMMMala",
            "a i",
            "x y? Cc",
        ] {
            assert_eq!(readable_name(name), None, "{name}");
        }
    }
}
