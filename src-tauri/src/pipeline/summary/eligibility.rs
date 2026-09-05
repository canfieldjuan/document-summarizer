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

fn marker_token(text: &str) -> &str {
    text.trim_matches(|c: char| {
        matches!(
            c,
            ',' | ';'
                | ':'
                | '!'
                | '?'
                | '.'
                | '"'
                | '\''
                | '“'
                | '”'
                | '‘'
                | '’'
                | '['
                | ']'
                | '{'
                | '}'
                | '('
                | ')'
        )
    })
}

fn email_marker(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = marker_token(token);
        let mut parts = token.split('@');
        let Some(local) = parts.next() else {
            return false;
        };
        let Some(domain) = parts.next() else {
            return false;
        };
        if parts.next().is_some()
            || local.is_empty()
            || !local
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'%' | b'+' | b'-'))
        {
            return false;
        }
        let labels = domain.split('.').collect::<Vec<_>>();
        labels.len() >= 2
            && labels.iter().all(|label| {
                !label.is_empty()
                    && label
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
            && labels
                .last()
                .is_some_and(|label| label.bytes().all(|c| c.is_ascii_alphanumeric()))
    })
}

fn phone_marker(text: &str) -> bool {
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    for start in 0..tokens.len() {
        let mut digits = 0usize;
        let mut separator = false;
        for token in tokens.iter().skip(start).take(3) {
            let token = token.trim_matches(|c: char| {
                matches!(
                    c,
                    ',' | ';' | ':' | '!' | '"' | '\'' | '“' | '”' | '‘' | '’'
                )
            });
            if token.is_empty()
                || !token
                    .bytes()
                    .all(|c| c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'(' | b')' | b'.'))
            {
                break;
            }
            digits += token.bytes().filter(u8::is_ascii_digit).count();
            separator |= token
                .bytes()
                .any(|c| matches!(c, b'+' | b'-' | b'(' | b')'));
            if (7..=15).contains(&digits) && separator {
                return true;
            }
            if digits > 15 {
                break;
            }
        }
    }
    false
}

fn numeric_date_marker(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = marker_token(token).trim_matches('.');
        ['/', '-'].into_iter().any(|separator| {
            let fields = token.split(separator).collect::<Vec<_>>();
            fields.len() == 3
                && fields.iter().all(|field| {
                    (1..=4).contains(&field.len()) && field.bytes().all(|c| c.is_ascii_digit())
                })
        })
    })
}

fn named_date_marker(text: &str) -> bool {
    const MONTHS: &[&str] = &[
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    tokens.windows(3).any(|window| {
        let month = marker_token(window[0]).to_ascii_lowercase();
        let day = marker_token(window[1]).trim_matches('.');
        let year = marker_token(window[2]).trim_matches('.');
        MONTHS.contains(&month.as_str())
            && (1..=2).contains(&day.len())
            && day.bytes().all(|c| c.is_ascii_digit())
            && matches!(year.len(), 2 | 4)
            && year.bytes().all(|c| c.is_ascii_digit())
    })
}

fn reference_marker(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = marker_token(token);
        (5..=64).contains(&token.len())
            && token
                .bytes()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            && token.bytes().filter(u8::is_ascii_digit).count() >= 2
            && token.bytes().any(|c| matches!(c, b'/' | b'-'))
            && token
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'/' | b'-' | b'?' | b'.'))
    })
}

/// Conservative veto for model-side omission. A match retains content; it does
/// not certify that the marker is well-formed or semantically important.
pub(super) fn material_marker(text: &str) -> bool {
    email_marker(text)
        || phone_marker(text)
        || numeric_date_marker(text)
        || named_date_marker(text)
        || reference_marker(text)
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
pub(super) const NARA_AMBIGUOUS_PAGE: &str = "....... �� \n\n--� \n\n·.,( \n...• ... \n\n\n\n \n\n  \n 5l  • \n\n- .. \n\n ·\"' -·• \n·-J.. \n.. , \n.,. \n-4\n\n.; \n\n. .:-J ...... \n\n,.: ... \n\n.:1 \n..... .... \n!~";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_nara_page_is_retained_under_numeric_unit_veto() {
        assert_eq!(classify(NARA_AMBIGUOUS_PAGE), None);
        assert!(!heading_admitted(NARA_AMBIGUOUS_PAGE));
        assert!(!material_marker(NARA_AMBIGUOUS_PAGE));
    }

    #[test]
    fn material_markers_veto_only_recognized_identifier_shapes() {
        for text in [
            "Contact Luz.Ortiz@WHS.MIL. for records.",
            "Call (703) 696-4959 for assistance.",
            "Approved 5/19/08.",
            "The memo is dated May 28, 2008.",
            "NARA job NC1-330-78-7? It applies.",
        ] {
            assert!(material_marker(text), "expected marker in {text:?}");
        }
        for text in [
            "not-an-email@",
            "Call (70) 69-49.",
            "Approved 5/19.",
            "The memo is dated May 2008.",
            "A-3",
            "02/25/20Q8",
            "ABC-1",
            "plain short page furniture",
        ] {
            assert!(!material_marker(text), "unexpected marker in {text:?}");
        }
        assert!(material_marker(
            "Noise before it; contact user@example.com; noise after it."
        ));
    }

    #[test]
    fn material_marker_numeric_boundaries_are_both_sided() {
        assert!(phone_marker("123-4567"));
        assert!(phone_marker("+123456789012345"));
        assert!(!phone_marker("12-3456"));
        assert!(!phone_marker("+1234567890123456"));

        assert!(reference_marker("A-123"));
        assert!(!reference_marker("A-12"));
        let maximum = format!("A-{}", "1".repeat(62));
        let over_maximum = format!("A-{}", "1".repeat(63));
        assert_eq!(maximum.len(), 64);
        assert_eq!(over_maximum.len(), 65);
        assert!(reference_marker(&maximum));
        assert!(!reference_marker(&over_maximum));

        assert!(numeric_date_marker("1-2-03"));
        assert!(!numeric_date_marker("1/2-03"));
        assert!(named_date_marker("May 1, 03"));
        assert!(!named_date_marker("May 2003"));
    }

    #[test]
    fn unit_free_noise_and_captured_stamp_are_omitted() {
        assert_eq!(
            classify("....... ��\n--�\n·.,(\n...• ..."),
            Some(Omission::ScanNoise)
        );
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
            classify(&format!("{NARA_AMBIGUOUS_PAGE}\nRecords must be retained.")),
            None
        );
    }

    #[test]
    fn heading_admission_does_not_admit_obligations_or_body_text() {
        assert!(heading_admitted("Labor Standards in Agriculture"));
        for text in [
            NARA_AMBIGUOUS_PAGE,
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
