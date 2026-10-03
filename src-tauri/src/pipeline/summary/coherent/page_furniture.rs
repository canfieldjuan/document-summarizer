//! Page furniture is excluded as original byte ranges, before quote packing.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::pipeline::summary) enum Policy {
    Original,
    RunningMetadata,
}

pub(in crate::pipeline::summary) struct Furniture {
    policy: Policy,
    removed_any: bool,
    retained: HashMap<String, Vec<(usize, usize)>>,
    pub(in crate::pipeline::summary) excluded_pages: HashSet<u32>,
}

struct Candidate {
    block_id: String,
    page: u32,
    start: usize,
    end: usize,
    key: Option<String>,
    minimum_pages: usize,
}

fn normalized(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn repeated_key(text: &str) -> String {
    normalized(text)
        .chars()
        .filter(|c| !c.is_ascii_digit())
        .collect()
}

fn numbered_heading(text: &str) -> bool {
    let text = text.trim().trim_start_matches('§').trim_start();
    let Some((number, rest)) = text.split_once(char::is_whitespace) else {
        return false;
    };
    let number = number.trim_end_matches(['.', ')']);
    !number.is_empty()
        && number
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
        && rest
            .trim_start()
            .chars()
            .next()
            .is_some_and(|c| c.is_uppercase())
}

fn operative(text: &str) -> bool {
    normalized(text)
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| {
            matches!(
                word,
                "shall"
                    | "must"
                    | "may"
                    | "unless"
                    | "except"
                    | "provided"
                    | "if"
                    | "when"
                    | "requires"
                    | "required"
            )
        })
}

fn page_number(text: &str, page: u32) -> bool {
    let text = normalized(text);
    let bare = page.to_string();
    if text == bare || text == format!("page {page}") {
        return true;
    }
    let text = text.strip_prefix("page ").unwrap_or(&text);
    for separator in [" of ", " / ", "/"] {
        if let Some((first, total)) = text.split_once(separator) {
            if first == bare && total.parse::<u32>().is_ok_and(|total| total >= page) {
                return true;
            }
        }
    }
    false
}

fn running_label(text: &str, page: u32) -> bool {
    let text = text.trim();
    if text.len() > 120
        || text.split_whitespace().count() > 12
        || numbered_heading(text)
        || operative(text)
        || text.contains(['.', ';', '!', '?', '$', '€'])
    {
        return false;
    }
    let lower = normalized(text);
    if ["confidential", "internal use only", "draft"].contains(&lower.as_str()) {
        return true;
    }
    if let Some((_, suffix)) = lower.rsplit_once(" | ") {
        if page_number(suffix, page) {
            return true;
        }
    }
    // A repeated document title is furniture; a governing clause title such as
    // SUBSTANTIAL COMPLETION is not. Ambiguous unmarked labels stay in source.
    text.chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_uppercase())
        && ["agreement", "contract", "report", "manual", "guide"]
            .iter()
            .any(|kind| lower.ends_with(kind))
}

fn filing_label(text: &str) -> bool {
    let mut words = text.split_whitespace();
    words.next() == Some("exhibit")
        && words.next().is_some_and(|id| {
            id.chars().any(|c| c.is_ascii_digit())
                && id
                    .split('.')
                    .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric()))
        })
        && words.next().is_none()
}

fn date_label(words: &[&str]) -> bool {
    let words = if words.first().is_some_and(|word| {
        matches!(
            word.trim_end_matches(','),
            "monday" | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday"
        )
    }) {
        &words[1..]
    } else {
        words
    };
    words.len() == 3
        && matches!(
            words[0],
            "january"
                | "february"
                | "march"
                | "april"
                | "may"
                | "june"
                | "july"
                | "august"
                | "september"
                | "october"
                | "november"
                | "december"
        )
        && words[1]
            .trim_end_matches(',')
            .parse::<u32>()
            .is_ok_and(|day| (1..=31).contains(&day))
        && words[2].len() == 4
        && words[2].bytes().all(|c| c.is_ascii_digit())
}

// Counter canonicalization only touches a field admitted by this grammar.
// Filing IDs, dates, and numbers in ordinary text remain distinct.
fn metadata_key(text: &str) -> Option<(String, bool)> {
    let lower = normalized(text);
    let words = lower.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() || text.len() > 180 || words.len() > 20 {
        return None;
    }
    if filing_label(&lower) {
        return Some((lower, false));
    }
    if let Some((counter, date)) = words.split_first() {
        if counter.parse::<u32>().is_ok_and(|n| n > 0) && date_label(date) {
            return Some((format!("# {}", date.join(" ")), true));
        }
    }
    if date_label(&words) {
        return Some((lower, true));
    }
    if operative(text) || numbered_heading(text) || text.contains([';', '!', '?', '$', '€']) {
        return None;
    }
    if let Some(index) = words.iter().position(|word| *word == "page") {
        let number = words.get(index + 1)?.parse::<u32>().ok()?;
        if number == 0 {
            return None;
        }
        let mut end = index + 2;
        if matches!(words.get(end), Some(&"of") | Some(&"/")) {
            let total = words.get(end + 1)?.parse::<u32>().ok()?;
            if total < number {
                return None;
            }
            end += 2;
        }
        if end != words.len() && !date_label(&words[end..]) {
            return None;
        }
        let mut key = words.clone();
        key[index + 1] = "#";
        return Some((key.join(" "), index > 0 || end != words.len()));
    }
    if text.contains('.') {
        return None;
    }
    if matches!(
        lower.as_str(),
        "confidential" | "internal use only" | "draft"
    ) {
        return Some((lower, false));
    }
    let title = |value: &str| {
        ["agreement", "contract", "report", "manual", "guide"]
            .iter()
            .any(|kind| value.ends_with(kind))
    };
    if title(&lower) {
        return Some((lower, false));
    }
    if let Some((prefix, counter)) = lower.rsplit_once(' ') {
        if counter.parse::<u32>().is_ok_and(|n| n > 0) && title(prefix.trim_end_matches(" |")) {
            return Some((format!("{prefix} #"), true));
        }
    }
    None
}

fn edgar_header(text: &str) -> bool {
    let words = text.split_whitespace().collect::<Vec<_>>();
    words.len() >= 3
        && words.len() <= 30
        && !operative(text)
        && words[0].strip_prefix("EX-").is_some_and(|value| {
            value
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()))
        })
        && words[1].bytes().all(|c| c.is_ascii_digit())
        && [".htm", ".html", ".txt"]
            .iter()
            .any(|suffix| words[2].to_ascii_lowercase().ends_with(suffix))
        && words[2]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn metadata_candidates(document: &NormalizedDocument) -> Vec<Candidate> {
    let minimum_pages = (native_text_pages(document).len() / 2 + 1).max(2);
    let mut result = Vec::new();
    for (page_index, page) in document.pages.iter().enumerate() {
        let mut lines = Vec::new();
        for block in &page.content {
            let mut offset = 0;
            for line in block.text.split_inclusive('\n') {
                let start = offset;
                offset += line.len();
                if !line.trim().is_empty() {
                    lines.push((block, start, offset, line.trim()));
                }
            }
        }
        let keys = lines
            .iter()
            .map(|(_, _, _, text)| metadata_key(text))
            .collect::<Vec<_>>();
        for (index, &(block, start, end, text)) in lines.iter().enumerate() {
            let bottom = lines.len() - index - 1;
            let push = |result: &mut Vec<Candidate>, key| {
                result.push(Candidate {
                    block_id: block.block_id.clone(),
                    page: page.page_number,
                    start,
                    end,
                    key,
                    minimum_pages,
                })
            };
            if (page_index == 0 && index == 0 && edgar_header(text))
                || ((index == 0 || bottom == 0) && page_number(text, page.page_number))
            {
                push(&mut result, None);
                continue;
            }
            for (side, position) in [("top", index), ("bottom", bottom)] {
                if position >= 4 {
                    continue;
                }
                let key = keys[index]
                    .as_ref()
                    .map(|(key, _)| key.clone())
                    .or_else(|| {
                        // A repeated initials/party row belongs to the footer only
                        // after a marked document/date footer and its initials row.
                        if side != "bottom"
                            || operative(text)
                            || numbered_heading(text)
                            || text.len() > 100
                            || text.contains(['.', ';', '!', '?', '$'])
                        {
                            return None;
                        }
                        let first = lines.len().saturating_sub(4);
                        let anchor = (first..index)
                            .rfind(|&i| keys[i].as_ref().is_some_and(|(_, strong)| *strong))?;
                        let initials = (anchor + 1..=index)
                            .find(|&i| normalized(lines[i].3).contains("initials"))?;
                        (initials == index
                            || (initials + 1 == index
                                && text
                                    .chars()
                                    .filter(|c| c.is_alphabetic())
                                    .all(char::is_uppercase)))
                        .then(|| normalized(text))
                    });
                if let Some(key) = key {
                    push(&mut result, Some(format!("{side}:{position}:{key}")));
                }
            }
        }
    }
    result
}

impl Furniture {
    pub(in crate::pipeline::summary) fn retained_ranges(
        &self,
        block_id: &str,
    ) -> &[(usize, usize)] {
        &self.retained[block_id]
    }

    pub(in crate::pipeline::summary) fn new(document: &NormalizedDocument) -> Self {
        Self::for_policy(document, Policy::RunningMetadata)
    }

    pub(in crate::pipeline::summary) fn for_synthesis_version(
        document: &NormalizedDocument,
        version: &str,
    ) -> Self {
        Self::for_policy(
            document,
            if version == SYNTHESIS_VERSION {
                Policy::RunningMetadata
            } else {
                Policy::Original
            },
        )
    }

    pub(in crate::pipeline::summary) fn for_policy(
        document: &NormalizedDocument,
        policy: Policy,
    ) -> Self {
        let mut candidates = if policy == Policy::RunningMetadata {
            metadata_candidates(document)
        } else {
            Vec::new()
        };
        for page in &document.pages {
            for (block_index, block) in page.content.iter().enumerate() {
                let mut offset = 0;
                let lines = block
                    .text
                    .split_inclusive('\n')
                    .map(|line| {
                        let start = offset;
                        offset += line.len();
                        (start, offset, line)
                    })
                    .collect::<Vec<_>>();
                let nonempty = lines
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, _, line))| !line.trim().is_empty())
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                for (index, &(start, end, line)) in lines.iter().enumerate() {
                    let edge = (block_index == 0 && nonempty.first() == Some(&index))
                        || (block_index + 1 == page.content.len()
                            && nonempty.last() == Some(&index));
                    if policy == Policy::Original
                        && edge
                        && (page_number(line, page.page_number)
                            || running_label(line, page.page_number))
                    {
                        candidates.push(Candidate {
                            block_id: block.block_id.clone(),
                            page: page.page_number,
                            start,
                            end,
                            key: (!page_number(line, page.page_number)).then(|| repeated_key(line)),
                            minimum_pages: 2,
                        });
                    }
                    if (index < 16 || index + 16 >= lines.len())
                        && normalized(line).contains("copyright")
                        && !numbered_heading(line)
                    {
                        let notice = lines[index..]
                            .iter()
                            .take_while(|(_, _, line)| {
                                !line.trim().is_empty() && !numbered_heading(line)
                            })
                            .collect::<Vec<_>>();
                        let mut notice_end = notice.last().map_or(end, |(_, end, _)| *end);
                        let text = &block.text[start..notice_end];
                        let lower = normalized(text);
                        if lower.contains("rights reserved")
                            && (lower.contains("trademark")
                                || lower.contains("licensed reproduction")
                                || lower.contains("electronically produced"))
                            && !operative(text)
                        {
                            // The parser may put the publisher footer and its page
                            // counter before the body. Remove only attached counters.
                            for &(_, next_end, next_line) in &lines[index + notice.len()..] {
                                if next_line.trim().is_empty()
                                    || page_number(next_line, page.page_number)
                                {
                                    notice_end = next_end;
                                } else {
                                    break;
                                }
                            }
                            candidates.push(Candidate {
                                block_id: block.block_id.clone(),
                                page: page.page_number,
                                start,
                                end: notice_end,
                                key: Some(if policy == Policy::Original {
                                    repeated_key(line)
                                } else {
                                    normalized(line)
                                }),
                                minimum_pages: 2,
                            });
                        }
                    }
                }
            }
        }
        let mut pages: HashMap<&str, HashSet<u32>> = HashMap::new();
        for candidate in &candidates {
            if let Some(key) = &candidate.key {
                pages.entry(key).or_default().insert(candidate.page);
            }
        }
        let text_pages = native_text_pages(document);
        let mut retained = HashMap::new();
        let mut excluded_pages = HashSet::new();
        let mut removed_any = false;
        for page in &document.pages {
            let mut has_substantive_text = false;
            for block in &page.content {
                let mut excluded = candidates
                    .iter()
                    .filter(|c| {
                        c.block_id == block.block_id
                            && c.key.as_deref().is_none_or(|key| {
                                pages.get(key).is_some_and(|p| p.len() >= c.minimum_pages)
                            })
                    })
                    .map(|c| (c.start, c.end))
                    .collect::<Vec<_>>();
                removed_any |= !excluded.is_empty();
                excluded.sort_unstable();
                let mut ranges = Vec::new();
                let mut cursor = 0;
                for (start, end) in excluded {
                    if cursor < start {
                        ranges.push((cursor, start));
                    }
                    cursor = cursor.max(end);
                }
                if cursor < block.text.len() {
                    ranges.push((cursor, block.text.len()));
                }
                has_substantive_text |= ranges
                    .iter()
                    .any(|&(start, end)| !block.text[start..end].trim().is_empty());
                retained.insert(block.block_id.clone(), ranges);
            }
            if text_pages.contains(&page.page_number) && !has_substantive_text {
                excluded_pages.insert(page.page_number);
            }
        }
        Self {
            policy,
            removed_any,
            retained,
            excluded_pages,
        }
    }

    pub(in crate::pipeline::summary) fn has_retained_text(&self, block: &NormalizedBlock) -> bool {
        self.retained[&block.block_id]
            .iter()
            .any(|&(start, end)| !block.text[start..end].trim().is_empty())
    }

    pub(in crate::pipeline::summary) fn warning(&self) -> Option<PipelineWarning> {
        if self.excluded_pages.is_empty() && (self.policy == Policy::Original || !self.removed_any)
        {
            return None;
        }
        let mut pages = self.excluded_pages.iter().copied().collect::<Vec<_>>();
        pages.sort_unstable();
        Some(PipelineWarning {
            code: "SUMMARY_FURNITURE_PAGES_EXCLUDED".into(),
            message: if self.policy == Policy::Original {
                format!("Pages {pages:?} contain only recognized page furniture; excluded from coherent excerpt selection and both page-coverage totals")
            } else {
                format!("Recognized page furniture excluded under furniture policy 2; fully excluded pages {pages:?} are omitted from both page-coverage totals")
            },
            stage: Some(PipelineStage::Synthesize),
        })
    }

    pub(in crate::pipeline::summary) fn segment(
        &self,
        block: &NormalizedBlock,
        analysis_version: &str,
    ) -> AnalysisQuoteSegmentation {
        let mut result = AnalysisQuoteSegmentation {
            segments: Vec::new(),
            omitted_source_units: 0,
        };
        for &(start, end) in &self.retained[&block.block_id] {
            if block.text[start..end].trim().is_empty() {
                continue;
            }
            let part =
                analysis_quote_segmentation_for_version(analysis_version, &block.text[start..end]);
            result.segments.extend(part.segments);
            result.omitted_source_units += part.omitted_source_units;
        }
        result
    }
}

#[cfg(test)]
mod origin_tests {
    use super::*;
    use crate::pipeline::contracts::{NormalizedBlockKind, NormalizedPage, SourceType};

    const BODY: &str = "1. Payment\nClient shall pay after acceptance.";

    fn document(source: impl Fn(u32) -> String) -> NormalizedDocument {
        NormalizedDocument {
            document_id: "public-furniture-layouts".into(),
            normalization_version: "fixture".into(),
            pages: (1..=3)
                .map(|n| NormalizedPage {
                    page_number: n,
                    content: vec![NormalizedBlock {
                        block_id: format!("block-{n}"),
                        kind: NormalizedBlockKind::Text,
                        text: source(n),
                        source: SourceSpan {
                            page_start: n,
                            page_end: n,
                            section_id: None,
                            source_type: SourceType::NativeText,
                        },
                    }],
                    warnings: vec![],
                    requires_visual_processing: false,
                })
                .collect(),
            warnings: vec![],
        }
    }

    fn assert_only_body(document: &NormalizedDocument) {
        let furniture = Furniture::new(document);
        for page in &document.pages {
            let block = &page.content[0];
            let retained = furniture
                .retained_ranges(&block.block_id)
                .iter()
                .map(|&(start, end)| &block.text[start..end])
                .collect::<String>();
            assert_eq!(
                retained.trim(),
                BODY,
                "metadata remained on page {}",
                page.page_number
            );
        }
    }

    #[test]
    fn furniture_origin_reproduction_filing_labels() {
        assert_only_body(&document(|n| format!("Exhibit 10.25\n{BODY}\nPage {n}")));
    }

    #[test]
    fn furniture_origin_reproduction_running_header() {
        assert_only_body(&document(|n| {
            format!("Public Service Agreement Page {n} of 3\n{BODY}")
        }));
    }

    #[test]
    fn furniture_origin_reproduction_date_footer() {
        assert_only_body(&document(|n| format!("{BODY}\n{n} December 13, 2016")));
    }

    #[test]
    fn furniture_origin_reproduction_multiline_footer() {
        assert_only_body(&document(|n| {
            format!(
                "{BODY}\nPublic Service Agreement Page {n} of 3\nInitials and Date\nPUBLIC COMPANY"
            )
        }));
    }

    #[test]
    fn furniture_origin_reproduction_edgar_header() {
        assert_only_body(&document(|n| {
            if n == 1 {
                format!("EX-10.25 28 filing.htm EXHIBIT 10.25\n{BODY}")
            } else {
                BODY.into()
            }
        }));
    }
    fn assert_unchanged(document: &NormalizedDocument) {
        let furniture = Furniture::new(document);
        assert!(furniture.warning().is_none());
        for page in &document.pages {
            for block in &page.content {
                assert_eq!(
                    furniture.retained_ranges(&block.block_id),
                    [(0, block.text.len())]
                );
            }
        }
    }

    #[test]
    fn furniture_origin_preserves_repeated_operative_and_governing_lines() {
        for line in [
            "The Client shall follow the Public Service Agreement",
            "Unless approved, see Page 5",
            "4. Public Service Agreement",
            "SUBSTANTIAL COMPLETION",
            "PAYMENT TERMS",
            "Exhibit 10.25 contains the agreed rates.",
            "Copyright retained. All rights reserved. Licensed reproduction only. The licensee must obtain permission.",
        ] {
            assert_unchanged(&document(|_| format!("{line}\n{BODY}\n{line}")));
        }
    }

    #[test]
    fn furniture_origin_preserves_unique_minority_and_position_changes() {
        assert_unchanged(&document(|n| format!("Exhibit 10.{n}\n{BODY}")));
        assert_unchanged(&document(|n| format!("December {n}, 2016\n{BODY}")));
        assert_unchanged(&document(|n| format!("Unique {n} Agreement\n{BODY}")));
        assert_unchanged(&document(|n| {
            if n == 1 {
                format!("Exhibit 10.25\n{BODY}")
            } else {
                BODY.into()
            }
        }));
        assert_unchanged(&document(|n| {
            format!(
                "{}Exhibit 10.25\n{BODY}{}",
                "Body text.\n".repeat(n as usize),
                "\nBody text.".repeat(n as usize)
            )
        }));
        let mut single = document(|_| format!("Public Service Agreement Page 1\n{BODY}"));
        single.pages.truncate(1);
        assert_unchanged(&single);
        let mut empty = document(|_| String::new());
        empty.pages.clear();
        assert!(Furniture::new(&empty).warning().is_none());
    }

    #[test]
    fn furniture_origin_band_majority_and_counter_boundaries() {
        for repeats in [1, 2, 3] {
            let fixture = document(|n| {
                if n <= repeats {
                    format!("Exhibit 10.25\n{BODY}")
                } else {
                    BODY.into()
                }
            });
            let furniture = Furniture::new(&fixture);
            assert_eq!(furniture.warning().is_some(), repeats >= 2);
        }
        for depth in [3, 4, 5] {
            let fixture = document(|_| {
                format!(
                    "{}Exhibit 10.25\n{BODY}{}",
                    "Ordinary body.\n".repeat(depth),
                    "\nOrdinary body.".repeat(5)
                )
            });
            let furniture = Furniture::new(&fixture);
            assert_eq!(furniture.warning().is_some(), depth < 4);
        }
        for counter in ["0", "4294967296", "", "false", "4 of 3"] {
            assert_unchanged(&document(|_| {
                format!("Public Service Agreement Page {counter}\n{BODY}")
            }));
        }
        // Non-counter numbers must not collapse to one repeated key.
        assert_unchanged(&document(|n| {
            format!("Public Service Agreement {n} Page {n}\n{BODY}")
        }));
    }

    #[test]
    fn furniture_origin_positions_span_blocks_and_preserve_utf8_bytes() {
        let mut fixture = document(|n| {
            format!("Exhibit 10.25\r\n{BODY}\r\nPublic Service Agreement Page {n} of 3\r\nInitials and Date\r\nPUBLIC COMPANY")
        });
        for page in &mut fixture.pages {
            let text = page.content[0].text.clone();
            let template = page.content[0].clone();
            page.content = text
                .split_inclusive('\n')
                .enumerate()
                .map(|(i, line)| {
                    let mut block = template.clone();
                    block.block_id = format!("{}-{i}", template.block_id);
                    block.text = line
                        .replace("acceptance", "café acceptance")
                        .replace("\r\n", "\n")
                        .replace('\n', "\r\n");
                    block
                })
                .collect();
        }
        let furniture = Furniture::new(&fixture);
        assert!(furniture.warning().is_some());
        assert!(furniture.excluded_pages.is_empty());
        for page in &fixture.pages {
            let retained = page
                .content
                .iter()
                .flat_map(|b| {
                    furniture
                        .retained_ranges(&b.block_id)
                        .iter()
                        .map(|&(start, end)| &b.text[start..end])
                })
                .collect::<String>();
            assert_eq!(
                retained.trim(),
                "1. Payment\r\nClient shall pay after café acceptance."
            );
        }
    }

    #[test]
    fn furniture_origin_edgar_exception_requires_start_and_typed_record() {
        for header in [
            "EX-10.25 filing.htm EXHIBIT 10.25",
            "EX-10.25 28 payment terms",
            "EX-10.25 28 filing.htm Client shall pay.",
            "EXHIBIT 10.25",
        ] {
            let mut fixture = document(|_| format!("{header}\n{BODY}"));
            fixture.pages.truncate(1);
            assert_unchanged(&fixture);
        }
        assert_unchanged(&document(|_| {
            format!("{BODY}\nEX-10.25 28 filing.htm EXHIBIT 10.25")
        }));
    }
}
