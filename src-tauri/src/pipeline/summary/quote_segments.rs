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
    let mut fields = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if let Some(value) = label_value(line) {
            fields.push((offset, offset + line.trim_end().len() - value.len()));
        } else if fields.is_empty() && !line.trim().is_empty() && !line.trim_end().ends_with(':') {
            return 0;
        }
        offset += line.len();
    }

    // Bounded layouts already fit without detaching any values. Only decide
    // prose versus indivisible form when the group needs a splitting decision.
    if text.chars().count() <= MAX_ANALYSIS_QUOTE_CHARACTERS {
        return fields.len();
    }

    let mut all_sentences = !fields.is_empty();
    for (index, &(_, value_start)) in fields.iter().enumerate() {
        let value_end = fields
            .get(index + 1)
            .map_or(text.len(), |&(start, _)| start);
        let value = &text[value_start..value_end];
        // Labels also introduce operative clauses. Reuse the historical
        // sentence owner on each full value, preserving wrapped text and its
        // abbreviation/decimal defenses. No clause-name exceptions are needed.
        let (sentences, tail) = analysis_sentence_units_v13(value);
        if sentences
            .first()
            .is_some_and(|&(_, end)| !value[end..].trim().is_empty())
        {
            return 0;
        }
        all_sentences &= !sentences.is_empty() && value[tail..].trim().is_empty();
    }
    if all_sentences {
        0
    } else {
        fields.len()
    }
}

fn label_value(line: &str) -> Option<&str> {
    let (label, value) = line.trim().split_once(':')?;
    let words = label.split_whitespace().count();
    ((1..=8).contains(&words)
        // Digits can belong to a field name, but number-led clause headings
        // must still reach prose segmentation, even inside parentheses.
        && label
            .chars()
            .find(|c| c.is_alphanumeric())
            .is_some_and(char::is_alphabetic)
        && label.chars().all(|c| {
            c.is_alphanumeric()
                || c.is_whitespace()
                || matches!(c, '-' | '/' | '(' | ')' | '\'' | '’' | '&' | '#')
        })
        && !value.trim().is_empty())
    .then_some(value)
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
    fn prose_values_keep_complete_units_and_omit_unterminated_continuations() {
        let complete = format!(
            "Scope: The supplier {} records.",
            "keeps supporting ".repeat(20)
        );
        let source = format!(
            "{complete} {}\nDetails: {}",
            "Unresolved material ".repeat(10),
            "further documentation ".repeat(10)
        );
        let old = super::super::analysis_quote_segments_v13(&source);
        assert_eq!(old.segments, vec![complete.clone()]);
        let current = segment(&source);
        assert_eq!(current.segments, old.segments);
        assert_eq!(current.omitted_source_units, 1);
    }

    #[test]
    fn abbreviations_in_single_sentence_values_do_not_make_a_form_prose() {
        for token in ["Dept.", "Qzx.", "Dr. A.", "3.14"] {
            let first = format!(
                "Description: The {token} office {} recorded.",
                "documented details ".repeat(20)
            );
            let second = format!(
                "Conditions: The supplier {} completed.",
                "required procedures ".repeat(20)
            );
            let source =
                format!("Name: Example\n{first}\n{second}\n\nRetain the final complete sentence.");
            let current = segment(&source);
            assert_eq!(
                current.segments,
                vec!["Retain the final complete sentence."],
                "{token}"
            );
            assert_eq!(current.omitted_source_units, 1);
        }
    }

    #[test]
    fn bounded_question_answer_forms_keep_the_printed_answer() {
        let complete = format!("{}.", "a".repeat(599));
        for newline in ["\n", "\r\n"] {
            let form = format!("Total cost: $800{newline}Routine services on site? YES");
            let source = format!("{complete}{newline}{newline}{form}");
            let current = segment(&source);
            assert_eq!(current.segments, vec![complete.clone(), form]);
            assert_eq!(current.omitted_source_units, 0);
        }
    }

    #[test]
    fn mixed_form_values_remain_together_at_the_character_limit() {
        let complete = format!("{}.", "a".repeat(599));
        for newline in ["\n", "\r\n"] {
            for notes_first in [false, true] {
                for size in [599, 600, 601] {
                    let scalar = format!("Name: Example{newline}Phone: 555-0100{newline}Details: ");
                    let note = "Notes: Call before arrival.";
                    let padding =
                        "x".repeat(size - scalar.chars().count() - note.len() - newline.len());
                    let form = if notes_first {
                        format!("{note}{newline}{scalar}{padding}")
                    } else {
                        format!("{scalar}{padding}{newline}{note}")
                    };
                    let source = format!("{complete}{newline}{newline}{form}");
                    let result = segment(&source);
                    assert_eq!(result.segments.contains(&form), size <= 600);
                    assert_eq!(result.omitted_source_units, usize::from(size > 600));
                    assert!(result
                        .segments
                        .iter()
                        .all(|quote| source.contains(quote) && quote.chars().count() <= 600));
                }
            }
        }
    }

    #[test]
    fn wrapped_values_are_evaluated_as_whole_fields() {
        let first = format!(
            "Description: The supplier {} completed.",
            "keeps documentation ".repeat(18)
        );
        let second = format!("Instructions: {}", "Unfinished directions ".repeat(18));
        let third = format!(
            "Notes: The reviewer {} confirmed.",
            "verified details ".repeat(18)
        );
        // A wrapped complete sentence is prose even though its first line is
        // not complete. A wrapped fragment must not qualify on its first line.
        let prose = format!(
            "Description: The supplier\n{}\n{third}",
            "keeps documentation ".repeat(18) + "completed."
        );
        let old = super::super::analysis_quote_segments_v13(&prose);
        assert_eq!(old.omitted_source_units, 0);
        assert_eq!(segment(&prose).segments, old.segments);
        let form = format!("{first}\n  unfinished continuation\n{second}\n{third}");
        // A sentence followed by an unfinished continuation is still prose;
        // preserve its complete sentence, disclose the unusable tail.
        let old = super::super::analysis_quote_segments_v13(&form);
        let current = segment(&form);
        assert_eq!(current.segments, old.segments);
        assert_eq!(current.omitted_source_units, old.omitted_source_units);
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

        // A mixed form stays indivisible even when some values are sentences.
        // Scalar fields retain their connection to the sentence-valued fields.
        let first = format!(
            "Description: {} recorded.",
            "documented details ".repeat(20)
        );
        let second = format!(
            "Conditions: {} completed.",
            "required procedures ".repeat(20)
        );
        for separator in ["\n", "\r\n"] {
            let table = format!("Name: Example{separator}{first}{separator}{second}");
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
