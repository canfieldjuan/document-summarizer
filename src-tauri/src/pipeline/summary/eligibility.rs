//! Conservative, versioned whole-page admission. Never modifies source bytes.
use unicode_properties::{
    GeneralCategory as Category, GeneralCategoryGroup as Group, UnicodeGeneralCategory,
};

pub(super) const VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Omission {
    ScanNoise,
    DatePageStamp,
}

fn multi_letter_word(text: &str) -> bool {
    let mut letters = 0;
    for c in text.chars() {
        match c.general_category_group() {
            Group::Letter => {
                letters += 1;
                if letters >= 2 {
                    return true;
                }
            }
            Group::Mark if letters > 0 => {}
            _ => letters = 0,
        }
    }
    false
}

fn digits(text: &str, minimum: usize, maximum: usize) -> bool {
    (minimum..=maximum).contains(&text.len()) && text.bytes().all(|c| c.is_ascii_digit())
}

fn date_stamp(text: &str) -> bool {
    if text.chars().count() > 32 {
        return false;
    }
    let parts = text.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 2 {
        return false;
    }
    let date = parts[0].split('/').collect::<Vec<_>>();
    let marker = parts[1].split('-').collect::<Vec<_>>();
    date.len() == 3
        && digits(date[0], 1, 2)
        && digits(date[1], 1, 2)
        && (digits(date[2], 2, 2) || digits(date[2], 4, 4))
        && marker.len() == 2
        && (1..=2).contains(&marker[0].len())
        && marker[0].bytes().all(|c| c.is_ascii_alphabetic())
        && digits(marker[1], 1, 3)
}

fn numeric_content(text: &str) -> bool {
    // Numeric tables and formatted values are substantive even without words.
    // A lone OCR digit embedded in punctuation is not a recognized value.
    if text.chars().any(|c| c.is_numeric())
        && text.chars().any(|c| {
            c.general_category() == Category::CurrencySymbol || matches!(c, '%' | '‰' | '‱' | '°')
        })
    {
        return true;
    }
    const SINGLE_LETTER_UNITS: &[&str] =
        &["l", "L", "m", "g", "s", "A", "V", "W", "J", "K", "C", "F"];
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    for (index, token) in tokens.iter().enumerate() {
        let first_letter = token
            .char_indices()
            .find(|(_, c)| c.is_alphabetic())
            .map(|(i, _)| i);
        if let Some(split) = first_letter {
            if SINGLE_LETTER_UNITS.contains(&&token[split..])
                && token[..split].parse::<f64>().is_ok()
            {
                return true;
            }
        }
        if token.parse::<f64>().is_ok()
            && tokens
                .get(index + 1)
                .is_some_and(|unit| SINGLE_LETTER_UNITS.contains(unit))
        {
            return true;
        }
    }
    for line in text.lines() {
        let line = line.trim();
        if !line.is_empty() && line.chars().all(char::is_numeric) {
            return true;
        }
        let mut consecutive_digits = 0;
        for c in line.chars() {
            consecutive_digits = if c.is_numeric() {
                consecutive_digits + 1
            } else {
                0
            };
            if consecutive_digits >= 2 {
                return true;
            }
        }
        let tokens = line.split_whitespace().collect::<Vec<_>>();
        let numbers = tokens
            .iter()
            .filter(|token| {
                let token = token.trim_matches(|c: char| matches!(c, '|' | ';' | ','));
                token.parse::<f64>().is_ok()
            })
            .count();
        if numbers >= 2 {
            return true;
        }
    }
    // Retain standalone numeric pages, including non-ASCII numeric characters.
    text.chars().any(|c| c.is_numeric())
        && text.chars().all(|c| {
            c.is_numeric()
                || c.is_whitespace()
                || matches!(c, '+' | '-' | '.' | ',' | '/' | ':' | '(' | ')')
        })
}

pub(super) fn classify(text: &str) -> Option<Omission> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if date_stamp(text) {
        return Some(Omission::DatePageStamp);
    }
    if multi_letter_word(text) || numeric_content(text) {
        return None;
    }
    let mut total = 0u128;
    let mut noise = 0u128;
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        total += 1;
        if matches!(
            c.general_category(),
            Category::Unassigned | Category::PrivateUse | Category::Format
        ) {
            return None; // Unknown or ambiguous characters cannot justify omission.
        }
        if matches!(
            c.general_category_group(),
            Group::Punctuation | Group::Symbol
        ) || c.general_category() == Category::Control
        {
            noise += 1;
        }
    }
    (total > 0 && noise * 5 >= total * 4).then_some(Omission::ScanNoise)
}

/// Admission only, never a determination that the page is non-substantive.
/// Restrict residual model discretion to short title-shaped text without values,
/// sentence punctuation, or common assertion/obligation/exception markers.
pub(super) fn heading_admitted(text: &str) -> bool {
    let text = text.trim();
    let words = text.split_whitespace().collect::<Vec<_>>();
    if text.chars().count() > 120
        || !(2..=12).contains(&words.len())
        || !text.chars().all(|c| c.is_alphabetic() || c.is_whitespace())
    {
        return false;
    }
    let prohibited = [
        "must",
        "shall",
        "may",
        "should",
        "not",
        "no",
        "except",
        "unless",
        "only",
        "required",
        "prohibited",
        "mandatory",
        "is",
        "are",
        "was",
        "were",
        "be",
        "has",
        "have",
        "will",
        "can",
        "cannot",
        "pay",
        "file",
        "submit",
        "retain",
        "keep",
        "wear",
        "apply",
        "applies",
        "do",
        "does",
        "never",
        "always",
        "due",
        "exempt",
        "excludes",
        "includes",
    ];
    words.iter().all(|word| {
        let lower = word.to_lowercase();
        !prohibited.contains(&lower.as_str())
            && (word.chars().next().is_some_and(char::is_uppercase)
                || ["of", "in", "and", "the", "for", "to", "on", "a", "an"]
                    .contains(&lower.as_str()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const NARA_NOISE: &str = "....... �� \n\n--� \n\n·.,( \n...• ... \n\n\n\n \n\n  \n 5l  • \n\n- .. \n\n ·\"' -·• \n·-J.. \n.. , \n.,. \n-4\n\n.; \n\n. .:-J ...... \n\n,.: ... \n\n.:1 \n..... .... \n!~";

    #[test]
    fn captured_nara_furniture_is_mechanically_recognized() {
        assert_eq!(classify(NARA_NOISE), Some(Omission::ScanNoise));
        assert_eq!(classify("03/10/03  A-4"), Some(Omission::DatePageStamp));
    }

    #[test]
    fn both_ratio_and_word_predicates_are_required() {
        assert_eq!(classify("...a"), None); // 75 percent
        assert_eq!(classify("....a"), Some(Omission::ScanNoise)); // 80 percent
        assert_eq!(classify(".....a"), Some(Omission::ScanNoise));
        for word in ["ab", "éé", "义务", "a\u{301}b"] {
            assert_eq!(classify(&format!("{} {word}", ".".repeat(100))), None);
        }
        assert_eq!(classify("！—�\u{0001}a"), Some(Omission::ScanNoise));
        assert_eq!(classify("....\u{e000}"), None);
    }

    #[test]
    fn stamp_is_anchored_and_length_bounded() {
        for length in [31, 32, 33] {
            let source = format!("03/10/03{}A-4", " ".repeat(length - 11));
            assert_eq!(source.chars().count(), length);
            assert_eq!(
                classify(&source),
                (length <= 32).then_some(Omission::DatePageStamp)
            );
        }
        for source in [
            "03/10/03",
            "03/10/03 A-",
            "03/10/03 ABC-4",
            "3/1/003 A-4",
            "03/10/03 A-1234",
            "03/10/03 A-4 File now",
            "Due 03/10/03 A-4",
        ] {
            assert_eq!(classify(source), None, "{source}");
        }
    }

    #[test]
    fn substantive_negative_controls_retain_without_model_help() {
        for source in [
            "",
            " ",
            "Pay now",
            "Except minors",
            "Due Friday",
            "03/10/03",
            "$4",
            "5%",
            "10 kg",
            "1 2\n3 4",
            "١٢",
            "µg",
            "No",
            "€1",
            "5°C",
        ] {
            assert_eq!(classify(source), None, "{source}");
            if !source.trim().is_empty() {
                // Punctuation-heavy pages must retain substantive tail content.
                assert_eq!(
                    classify(&format!("{}\n{source}", ".".repeat(200))),
                    None,
                    "tail {source}"
                );
            }
        }
        assert_eq!(
            classify(&format!("{NARA_NOISE}\nRecords must be retained.")),
            None
        );
    }

    #[test]
    fn heading_admission_does_not_admit_obligations_or_body_text() {
        assert!(heading_admitted("Labor Standards in Agriculture"));
        for text in [
            NARA_NOISE,
            "03/10/03 A-4",
            "Pay Now",
            "Records Must Be Retained",
            "Exceptions Apply",
            "No Exceptions",
            "Required Records",
            "Results Are Final",
            "Minimum Wage $7.25",
            "This is body text.",
        ] {
            assert!(!heading_admitted(text), "{text}");
        }
    }

    #[test]
    fn short_numeric_units_veto_noise_even_at_the_page_tail() {
        for value in ["5 L", "5l", "5 m", "5m", "4 g", "4g"] {
            let page = format!("{}\n{value}", ".".repeat(200));
            assert_eq!(classify(&page), None, "substantive unit {value}");
        }
    }
}
