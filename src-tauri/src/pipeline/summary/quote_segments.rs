//! Version-14 source units. Historical segmentation stays in the parent module.
use super::{
    analysis_sentence_boundary, analysis_sentence_units_v13, AnalysisQuoteSegmentation,
    MAX_ANALYSIS_QUOTE_CHARACTERS,
};
use unicode_segmentation::UnicodeSegmentation;

// These are source ranges, never rewritten text. An unavailable range is retained
// until packing so nothing on either side can silently absorb or hide its loss.
struct Unit {
    start: usize,
    end: usize,
    complete: bool,
}

pub(super) fn segment(source: &str) -> AnalysisQuoteSegmentation {
    let trimmed = source.trim();
    if trimmed.chars().count() <= MAX_ANALYSIS_QUOTE_CHARACTERS {
        return AnalysisQuoteSegmentation {
            segments: (!trimmed.is_empty())
                .then(|| trimmed.to_string())
                .into_iter()
                .collect(),
            omitted_source_units: 0,
        };
    }

    let mut groups: Vec<Unit> = Vec::new();
    for (start, end) in paragraphs(source) {
        if let Some(previous) = groups.last_mut() {
            let before = &source[previous.start..previous.end];
            let next = &source[start..end];
            let bounded_form = form_group(next)
                && !before.ends_with([':', ';', ',', '-'])
                && next.chars().count() <= MAX_ANALYSIS_QUOTE_CHARACTERS;
            if lead_in(before) && !bounded_form || qualification(next) {
                previous.end = end;
                continue;
            }
        }
        groups.push(Unit {
            start,
            end,
            complete: true,
        });
    }

    let mut units = Vec::new();
    for group in groups {
        let text = &source[group.start..group.end];
        // Multiple physical field rows establish an indivisible form layout.
        // A single field may instead be colon-prefixed prose: preserve it whole
        // when bounded, but let oversized prose use the sentence policy below.
        let fields = form_field_count(text);
        if fields > 1
            || text.chars().count() <= MAX_ANALYSIS_QUOTE_CHARACTERS
                && (fields > 0
                    || analysis_sentence_boundary(source, group.start, group.end, group.end, false))
        {
            units.push(group);
            continue;
        }
        let retained = retained_prose_ranges(text);
        let mut start = group.start;
        let mut group_units: Vec<Unit> = Vec::new();
        for (offset, sentence) in text.split_sentence_bound_indices() {
            let end = group.start + offset + sentence.len();
            let relative_end = text[..offset + sentence.len()].trim_end().len();
            if !analysis_sentence_boundary(source, start, end, group.end, false)
                || retained
                    .iter()
                    .any(|&(begin, finish)| begin < relative_end && relative_end < finish)
            {
                continue;
            }
            let unit = trimmed_unit(source, start, end, true);
            if let Some(previous) = group_units.last_mut() {
                if qualification(&source[unit.start..unit.end]) {
                    previous.end = unit.end;
                    start = end;
                    continue;
                }
            }
            group_units.push(unit);
            start = end;
        }
        if !source[start..group.end].trim().is_empty() {
            let tail = trimmed_unit(source, start, group.end, false);
            if let Some(previous) = group_units.last_mut() {
                if qualification(&source[tail.start..tail.end]) {
                    previous.end = tail.end;
                    previous.complete = false;
                } else {
                    group_units.push(tail);
                }
            } else {
                group_units.push(tail);
            }
        }
        units.extend(group_units);
    }

    let mut result = AnalysisQuoteSegmentation {
        segments: Vec::new(),
        omitted_source_units: 0,
    };
    let mut packed: Option<Unit> = None;
    for unit in units {
        if !unit.complete
            || source[unit.start..unit.end].chars().count() > MAX_ANALYSIS_QUOTE_CHARACTERS
        {
            flush(source, &mut packed, &mut result.segments);
            result.omitted_source_units += 1;
        } else if let Some(previous) = packed.as_mut() {
            if source[previous.start..unit.end].chars().count() <= MAX_ANALYSIS_QUOTE_CHARACTERS {
                previous.end = unit.end;
            } else {
                flush(source, &mut packed, &mut result.segments);
                packed = Some(unit);
            }
        } else {
            packed = Some(unit);
        }
    }
    flush(source, &mut packed, &mut result.segments);
    result
}

fn retained_prose_ranges(source: &str) -> Vec<(usize, usize)> {
    // Keep the frozen policy's bounded units whole. Use its original offsets,
    // not text matching: an omitted clause can contain an earlier identical copy.
    analysis_sentence_units_v13(source)
        .0
        .into_iter()
        .filter(|&(start, end)| source[start..end].chars().count() <= MAX_ANALYSIS_QUOTE_CHARACTERS)
        .collect()
}

fn trimmed_unit(source: &str, start: usize, end: usize, complete: bool) -> Unit {
    let text = &source[start..end];
    Unit {
        start: start + text.len() - text.trim_start().len(),
        end: start + text.trim_end().len(),
        complete,
    }
}

fn flush(source: &str, packed: &mut Option<Unit>, segments: &mut Vec<String>) {
    if let Some(unit) = packed.take() {
        segments.push(source[unit.start..unit.end].to_string());
    }
}

fn paragraphs(source: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        if line.trim().is_empty() {
            let unit = trimmed_unit(source, start, offset, true);
            if unit.start < unit.end {
                ranges.push((unit.start, unit.end));
            }
            start = offset + line.len();
        }
        offset += line.len();
    }
    let unit = trimmed_unit(source, start, source.len(), true);
    if unit.start < unit.end {
        ranges.push((unit.start, unit.end));
    }
    ranges
}

fn form_group(text: &str) -> bool {
    form_field_count(text) > 0
}

fn form_field_count(text: &str) -> usize {
    // A field group starts with a label/value row, optionally under headings.
    // Wrapped value lines do not establish additional fields. A later label
    // cannot reclassify ordinary preceding prose as a form.
    let mut fields = 0;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if label_value(line) {
            fields += 1;
        } else if fields == 0 && !line.trim_end().ends_with(':') {
            return 0;
        }
    }
    fields
}

fn label_value(line: &str) -> bool {
    let Some((label, value)) = line.trim().split_once(':') else {
        return false;
    };
    let words = label.split_whitespace().count();
    (1..=8).contains(&words)
        && label.chars().any(char::is_alphabetic)
        && label
            .chars()
            .all(|c| c.is_alphabetic() || c.is_whitespace() || matches!(c, '-' | '/' | '(' | ')'))
        && !value.trim().is_empty()
}

fn lead_in(text: &str) -> bool {
    if text.ends_with([':', ';', ',', '-']) {
        return true;
    }
    !form_group(text) && !analysis_sentence_boundary(text, 0, text.len(), text.len(), false)
}

fn qualification(text: &str) -> bool {
    let text = text
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '(' | '['))
        .to_lowercase();
    [
        "unless ",
        "except ",
        "provided ",
        "subject to ",
        "however,",
        "only if ",
        "on condition ",
        "notwithstanding ",
    ]
    .iter()
    .any(|prefix| text.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_whole_blocks_and_unusable_units_keep_the_existing_limits() {
        assert!(segment(" \r\n ").segments.is_empty());
        for length in [599, 600, 601] {
            for terminal in ["", "."] {
                let text = format!("{}{terminal}", "x".repeat(length - terminal.len()));
                let result = segment(&text);
                assert_eq!(result.segments.is_empty(), length > 600);
                assert_eq!(result.omitted_source_units, usize::from(length > 600));
            }
        }
    }

    #[test]
    fn wrapped_abbreviations_initials_and_decimals_do_not_become_sentence_cuts() {
        let first = format!("{}.", "a".repeat(549));
        for continuation in [
            "Staff contacted the Dept. Records office for the documented disposition schedule.",
            "Staff contacted the dept. Records office for the documented disposition schedule.",
            "Staff contacted the (Dept). Records office for the documented disposition schedule.",
            "Dr. A. Smith inspected the U.S. records with 3.14 units and archived the outcome.",
            "The N.A.S.A. Records office archived the documented disposition schedule and outcome.",
        ] {
            let result = segment(&format!("{first} {continuation}"));
            assert_eq!(result.segments, vec![first.clone(), continuation.into()]);
            assert_eq!(result.omitted_source_units, 0);
        }
    }

    #[test]
    fn oversized_qualified_units_are_withheld_together_and_processing_resumes() {
        let rule = format!(
            "The supplier {} work.",
            "must retain records for the scheduled ".repeat(12)
        );
        let exception = format!("Unless {} applies.", "the documented exception ".repeat(8));
        for separator in [" ", "\n\n", "\r\n \r\n"] {
            let source =
                format!("{rule}{separator}{exception}\n\nRetain the final complete sentence.");
            let result = segment(&source);
            assert_eq!(result.segments, vec!["Retain the final complete sentence."]);
            assert_eq!(result.omitted_source_units, 1);
        }
    }

    #[test]
    fn headings_and_wrapped_table_values_remain_attached() {
        let first = format!("{}.", "a".repeat(599));
        let table = "Coverage schedule:\r\n\r\nGeneral liability:\r\n  Each event: $500,000\r\n  Aggregate: $900,000";
        let source = format!("{first}\r\n\r\n{table}");
        let result = segment(&source);
        assert_eq!(result.segments, vec![first, table.into()]);
        assert_eq!(result.omitted_source_units, 0);
    }

    #[test]
    fn long_tables_and_dangling_prose_are_not_cut_into_ordinary_quotes() {
        let table = format!(
            "Coverage schedule:\nGeneral liability: {}",
            "covered equipment ".repeat(40)
        );
        let source =
            format!("{table}\n\nRetain the complete sentence. A dangling exception unless");
        let result = segment(&source);
        assert_eq!(result.segments, vec!["Retain the complete sentence."]);
        assert_eq!(result.omitted_source_units, 2);
    }

    #[test]
    fn a_bounded_form_survives_unrelated_unterminated_prose_but_not_a_governing_lead_in() {
        let prose = "unresolved preceding text ".repeat(26);
        let form = "Contract value not to exceed: $800\nOn site: No";
        let source = format!("{prose}\n\n{form}");
        let result = segment(&source);
        assert_eq!(result.segments, vec![form]);
        assert_eq!(result.omitted_source_units, 1);

        let governed = format!("{prose}subject to:\n\n{form}");
        let result = segment(&governed);
        assert!(result.segments.is_empty());
        assert_eq!(result.omitted_source_units, 1);
    }

    #[test]
    fn form_admission_uses_its_own_limit_and_keeps_governing_prefixes_attached() {
        let complete = format!("{}.", "a".repeat(599));
        for repeats in [1, 30] {
            let prefix = "unfinished text ".repeat(repeats);
            for size in [599, 600, 601] {
                let form = format!("Details: {}", "x".repeat(size - "Details: ".len()));
                let source = format!("{complete}\n\n{prefix}\n\n{form}");
                let result = segment(&source);
                assert_eq!(result.segments.contains(&form), size <= 600);
                assert!(result
                    .segments
                    .iter()
                    .all(|quote| source.contains(quote) && quote.chars().count() <= 600));
            }
            for punctuation in [':', ';', ',', '-'] {
                let form = "Details: documented procedures";
                let source = format!("{complete}\n\n{prefix}{punctuation}\n\n{form}");
                let result = segment(&source);
                assert!(!result.segments.iter().any(|quote| quote == form));
                for quote in result.segments.iter().filter(|quote| quote.contains(form)) {
                    assert!(quote.contains(prefix.trim()));
                }
            }
        }
    }

    #[test]
    fn retained_prose_ranges_use_positions_when_source_text_repeats() {
        let first = format!("The parties {} days.", "recorded details ".repeat(20));
        let repeated = format!(
            "Department staff contacted the Bldg. Records officers {} reviewed.",
            "checked details ".repeat(22)
        );
        let source = format!("{first} {repeated} {repeated}");
        let old = super::super::analysis_quote_segments_v13(&source);
        assert_eq!(old.segments, vec![repeated.clone()]);
        assert_eq!(old.omitted_source_units, 1);
        let result = segment(&source);
        assert_eq!(result.segments.last(), Some(&repeated));
        assert_eq!(result.omitted_source_units, 0);
    }

    #[test]
    fn multi_field_forms_remain_indivisible_at_the_limit() {
        let complete = format!("{}.", "a".repeat(599));
        let labels = "Amount: $1\nDescription: ";
        for size in [599, 600, 601] {
            let form = format!("{labels}{}", "x".repeat(size - labels.len()));
            let result = segment(&format!("{complete}\n\n{form}"));
            assert_eq!(result.segments.contains(&form), size <= 600);
            assert_eq!(result.omitted_source_units, usize::from(size > 600));
        }

        // Sentence punctuation in actual field values must not turn a table
        // into independent assertions stripped of the other field's context.
        let first = format!(
            "Description: {} recorded.",
            "documented details ".repeat(20)
        );
        let second = format!(
            "Conditions: {} completed.",
            "required procedures ".repeat(20)
        );
        for separator in ["\n", "\r\n"] {
            let table = format!("{first}{separator}{second}");
            let source = format!("{table}\n\nRetain the final complete sentence.");
            let result = segment(&source);
            assert_eq!(result.segments, vec!["Retain the final complete sentence."]);
            assert_eq!(result.omitted_source_units, 1);
        }
    }

    #[test]
    fn colon_prefixed_prose_keeps_its_exception_attached() {
        let rule = format!(
            "Summary: The supplier {} work.",
            "must retain scheduled records ".repeat(12)
        );
        let exception = format!("Unless {} applies.", "the documented exception ".repeat(8));
        for separator in [" ", "\n", "\n\n"] {
            let source =
                format!("{rule}{separator}{exception}\n\nRetain the final complete sentence.");
            let result = segment(&source);
            assert_eq!(result.segments, vec!["Retain the final complete sentence."]);
            assert_eq!(result.omitted_source_units, 1);
        }
    }

    #[test]
    fn a_later_form_label_does_not_hide_complete_prose() {
        let prose = format!("The parties {} work.", "recorded details ".repeat(20));
        let form = format!("Details: {}", "documented procedures ".repeat(20));
        let source = format!("{prose}\n{form}");
        let result = segment(&source);
        assert_eq!(result.segments, vec![prose]);
        assert_eq!(result.omitted_source_units, 1);
    }

    #[test]
    fn source_bytes_and_unicode_ranges_survive_layout_packing() {
        let first = format!("{}。", "記録".repeat(240));
        let second = "Service: Installation\nLocation: Upper floor\n  beside the eastern stairway";
        let third = format!("{}。", "確認".repeat(100));
        let source = format!(" \n{first}\n\n{second}\n\n{third}  ");
        let result = segment(&source);
        assert_eq!(result.omitted_source_units, 0);
        for piece in &result.segments {
            assert!(source.contains(piece));
            assert!(piece.chars().count() <= MAX_ANALYSIS_QUOTE_CHARACTERS);
        }
        assert!(result.segments.iter().any(|piece| piece.contains(second)));
    }
}
