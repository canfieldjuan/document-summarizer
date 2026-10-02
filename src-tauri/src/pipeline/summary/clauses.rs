//! Source-owned clauses. Text is always a slice of a normalized block.
use super::*;

pub(super) const MAX_CLAUSE_CHARACTERS: usize = 8_192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Fragment {
    pub block_id: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Clause {
    pub heading: Option<String>,
    pub fragments: Vec<Fragment>,
}

fn numbered_heading(line: &str) -> bool {
    let line = line.trim().trim_start_matches('§').trim_start();
    let Some((number, rest)) = line.split_once(char::is_whitespace) else {
        return false;
    };
    let number = number.trim_end_matches(['.', ')']);
    !number.is_empty()
        && number.len() <= 24
        && number
            .split('.')
            .all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()))
        && rest
            .trim_start()
            .chars()
            .next()
            .is_some_and(|c| c.is_uppercase())
}

fn furniture_key(line: &str) -> String {
    line.chars()
        .filter(|c| !c.is_ascii_digit())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn footer_range(block: &NormalizedBlock) -> Option<(usize, usize)> {
    let lines = block.text.split_inclusive('\n').collect::<Vec<_>>();
    let mut offset = 0;
    for (index, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        // PDF extraction may place a visual footer before the page body.
        // Match a complete repeated publisher notice, never the whole page tail.
        if (index < 16 || index + 16 >= lines.len())
            && !numbered_heading(line)
            && lower.contains("copyright")
        {
            let count = lines[index..]
                .iter()
                .take_while(|l| !l.trim().is_empty() && !numbered_heading(l))
                .count();
            let notice = lines[index..index + count].concat().to_lowercase();
            if notice.contains("rights reserved")
                && (notice.contains("trademark")
                    || notice.contains("licensed reproduction")
                    || notice.contains("electronically produced"))
                && !notice.contains(" shall ")
                && !notice.contains(" must ")
            {
                let mut end = offset
                    + lines[index..index + count]
                        .iter()
                        .map(|l| l.len())
                        .sum::<usize>();
                for next in &lines[index + count..] {
                    let t = next.trim();
                    if t.is_empty() || t == block.source.page_start.to_string() {
                        end += next.len();
                    } else {
                        break;
                    }
                }
                return Some((offset, end));
            }
        }
        offset += line.len();
    }
    None
}

pub(super) fn build(blocks: &HashMap<&str, &NormalizedBlock>) -> Vec<Clause> {
    let mut ordered = blocks.values().copied().collect::<Vec<_>>();
    ordered.sort_by_key(|b| (b.source.page_start, b.block_id.as_str()));
    build_ordered(&ordered)
}

pub(super) fn build_ordered(ordered: &[&NormalizedBlock]) -> Vec<Clause> {
    let mut footer_pages: HashMap<String, HashSet<u32>> = HashMap::new();
    for b in ordered {
        if let Some((start, _)) = footer_range(b) {
            let first = b.text[start..].lines().next().unwrap_or_default();
            footer_pages
                .entry(furniture_key(first))
                .or_default()
                .insert(b.source.page_start);
        }
    }
    let mut clauses: Vec<Clause> = Vec::new();
    let mut numbered = false;
    for block in ordered {
        let excluded = footer_range(block).filter(|&(start, _)| {
            let key = furniture_key(block.text[start..].lines().next().unwrap_or_default());
            footer_pages.get(&key).is_some_and(|pages| pages.len() >= 2)
        });
        let ranges = if let Some((start, end)) = excluded {
            vec![(0, start), (end, block.text.len())]
        } else {
            vec![(0, block.text.len())]
        };
        for (range_start, range_end) in ranges {
            let text = &block.text[range_start..range_end];
            let mut start = range_start;
            let mut offset = range_start;
            let mut heading = None;
            for line in text.split_inclusive('\n') {
                let is_numbered = numbered_heading(line);
                let boundary = is_numbered || (!numbered && line.trim().is_empty());
                if boundary {
                    append(&mut clauses, block, start, offset, heading.take());
                    start = offset;
                    if is_numbered {
                        // A new printed clause always starts a new unit. Continuations
                        // without another printed boundary stay with the previous one.
                        heading = Some(line.trim().to_string());
                        numbered = true;
                    } else {
                        start += line.len();
                    }
                }
                offset += line.len();
            }
            append(&mut clauses, block, start, range_end, heading);
        }
    }
    clauses
}

fn append(
    clauses: &mut Vec<Clause>,
    block: &NormalizedBlock,
    start: usize,
    end: usize,
    heading: Option<String>,
) {
    let raw = &block.text[start..end];
    let text = raw.trim();
    if text.is_empty() {
        return;
    }
    let begin = start + raw.len() - raw.trim_start().len();
    let fragment = Fragment {
        block_id: block.block_id.clone(),
        start: begin,
        end: begin + text.len(),
    };
    let continuation = heading.is_none()
        && clauses.last().is_some_and(|c| {
            c.heading.is_some()
                && c.fragments
                    .last()
                    .is_some_and(|p| p.block_id != block.block_id)
        });
    if continuation {
        clauses
            .last_mut()
            .expect("existing clause")
            .fragments
            .push(fragment);
    } else {
        clauses.push(Clause {
            heading,
            fragments: vec![fragment],
        });
    }
}

pub(super) fn text(clause: &Clause, blocks: &HashMap<&str, &NormalizedBlock>) -> String {
    clause
        .fragments
        .iter()
        .map(|f| &blocks[f.block_id.as_str()].text[f.start..f.end])
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn segmentation(
    block_id: &str,
    blocks: &HashMap<&str, &NormalizedBlock>,
) -> AnalysisQuoteSegmentation {
    let mut result = AnalysisQuoteSegmentation {
        segments: Vec::new(),
        omitted_source_units: 0,
    };
    // Ordinary prose/forms keep their proven quote policy. Clause context is
    // supplied separately; a numbered operative unit must not be sentence-packed.
    let block = blocks[block_id];
    if !block.text.lines().any(numbered_heading) {
        let units = build(blocks);
        let joined = units
            .iter()
            .flat_map(|c| &c.fragments)
            .filter(|f| f.block_id == block_id)
            .map(|f| &block.text[f.start..f.end])
            .collect::<Vec<_>>();
        let inherited = units
            .iter()
            .any(|c| c.heading.is_some() && c.fragments.iter().any(|f| f.block_id == block_id));
        if !inherited {
            if joined.is_empty() {
                return result;
            }
            let retained = units
                .iter()
                .flat_map(|c| &c.fragments)
                .filter(|f| f.block_id == block_id)
                .collect::<Vec<_>>();
            let mut spans: Vec<(usize, usize)> = Vec::new();
            for f in retained {
                if let Some(last) = spans.last_mut() {
                    if block.text[last.1..f.start].trim().is_empty() {
                        last.1 = f.end;
                        continue;
                    }
                }
                spans.push((f.start, f.end));
            }
            for (start, end) in spans {
                let part = quote_segments::segment(&block.text[start..end]);
                result.segments.extend(part.segments);
                result.omitted_source_units += part.omitted_source_units;
            }
            retain_unambiguous(block_id, blocks, &mut result);
            return result;
        }
    }

    for clause in build(blocks) {
        let parts = clause
            .fragments
            .iter()
            .filter(|f| f.block_id == block_id)
            .collect::<Vec<_>>();
        if parts.is_empty() {
            continue;
        }
        if text(&clause, blocks).chars().count() > MAX_CLAUSE_CHARACTERS {
            result.omitted_source_units += 1;
        } else {
            result.segments.extend(
                parts
                    .into_iter()
                    .map(|f| blocks[block_id].text[f.start..f.end].to_string()),
            );
        }
    }
    retain_unambiguous(block_id, blocks, &mut result);
    result
}

fn retain_unambiguous(
    block_id: &str,
    blocks: &HashMap<&str, &NormalizedBlock>,
    result: &mut AnalysisQuoteSegmentation,
) {
    result.segments.retain(|quote| {
        let valid = context(block_id, quote, blocks)
            .is_some_and(|c| c.chars().count() <= MAX_CLAUSE_CHARACTERS);
        if !valid {
            result.omitted_source_units += 1;
        }
        valid
    });
}

pub(super) fn context(
    block_id: &str,
    quote: &str,
    blocks: &HashMap<&str, &NormalizedBlock>,
) -> Option<String> {
    let block = blocks.get(block_id)?;
    let clauses = build(blocks);
    let contexts = block
        .text
        .match_indices(quote)
        .filter_map(|(start, _)| {
            let end = start + quote.len();
            let matching = clauses
                .iter()
                .filter(|clause| {
                    clause
                        .fragments
                        .iter()
                        .any(|f| f.block_id == block_id && f.start < end && start < f.end)
                })
                .map(|clause| text(clause, blocks))
                .collect::<Vec<_>>();
            (!matching.is_empty()).then(|| matching.join("\n\n"))
        })
        .collect::<HashSet<_>>();
    (contexts.len() == 1)
        .then(|| contexts.into_iter().next())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contracts::{NormalizedBlockKind, NormalizedPage, SourceType};

    fn document(texts: &[&str]) -> NormalizedDocument {
        NormalizedDocument {
            document_id: "public-source".into(),
            normalization_version: "1.0.0".into(),
            warnings: Vec::new(),
            pages: texts
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    let page = i as u32 + 1;
                    NormalizedPage {
                        page_number: page,
                        warnings: Vec::new(),
                        requires_visual_processing: false,
                        content: vec![NormalizedBlock {
                            block_id: format!("block-{page}"),
                            kind: NormalizedBlockKind::Text,
                            text: text.to_string(),
                            source: SourceSpan {
                                page_start: page,
                                page_end: page,
                                section_id: None,
                                source_type: SourceType::NativeText,
                            },
                        }],
                    }
                })
                .collect(),
        }
    }

    #[test]
    fn footer_without_blank_separator_preserves_following_clause() {
        let doc = document(&[
            "Copyright 2020. All rights reserved. Licensed reproduction only.\n4. Payment\nPayment is due after acceptance.",
            "Copyright 2020. All rights reserved. Licensed reproduction only.\n5. Term\nThe term is one year.",
        ]);
        let blocks = doc
            .pages
            .iter()
            .flat_map(|p| &p.content)
            .map(|b| (b.block_id.as_str(), b))
            .collect::<HashMap<_, _>>();
        let units = build(&blocks);
        assert_eq!(units.len(), 2, "footer filter removed operative clauses");
        assert_eq!(
            text(&units[0], &blocks),
            "4. Payment\nPayment is due after acceptance."
        );
        assert_eq!(text(&units[1], &blocks), "5. Term\nThe term is one year.");
    }

    #[test]
    fn operative_copyright_and_unique_footer_are_not_silently_removed() {
        let doc = document(&["8. Copyright\nThe publisher retains copyright. All rights reserved.\nThe licensee must obtain permission.", "A unique notice. Copyright 2020. All rights reserved."]);
        let blocks = doc
            .pages
            .iter()
            .flat_map(|p| &p.content)
            .map(|b| (b.block_id.as_str(), b))
            .collect::<HashMap<_, _>>();
        let all = build(&blocks)
            .iter()
            .map(|c| text(c, &blocks))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("licensee must obtain permission"));
        assert!(all.contains("A unique notice"));
    }

    #[test]
    fn oversized_clause_is_omitted_whole_from_model_excerpts() {
        for size in [
            MAX_CLAUSE_CHARACTERS - 1,
            MAX_CLAUSE_CHARACTERS,
            MAX_CLAUSE_CHARACTERS + 1,
        ] {
            let text = format!("1. Duties\n{}.", "x".repeat(size - "1. Duties\n.".len()));
            let doc = document(&[&text]);
            let blocks = doc
                .pages
                .iter()
                .flat_map(|p| &p.content)
                .map(|b| (b.block_id.as_str(), b))
                .collect::<HashMap<_, _>>();
            let result = segmentation("block-1", &blocks);
            assert_eq!(
                result.omitted_source_units,
                usize::from(size > MAX_CLAUSE_CHARACTERS)
            );
            assert_eq!(
                result.segments.len(),
                usize::from(size <= MAX_CLAUSE_CHARACTERS)
            );
            if let Some(quote) = result.segments.first() {
                assert_eq!(quote, &text);
            }
        }
    }
}
