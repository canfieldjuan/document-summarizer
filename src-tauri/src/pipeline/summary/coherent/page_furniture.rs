//! Page furniture is excluded as original byte ranges, before quote packing.
use super::*;

pub(super) struct Furniture {
    retained: HashMap<String, Vec<(usize, usize)>>,
    pub(super) excluded_pages: HashSet<u32>,
}

struct Candidate {
    block_id: String,
    page: u32,
    start: usize,
    end: usize,
    key: Option<String>,
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

impl Furniture {
    pub(super) fn new(document: &NormalizedDocument) -> Self {
        let mut candidates = Vec::new();
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
                    if edge
                        && (page_number(line, page.page_number)
                            || running_label(line, page.page_number))
                    {
                        candidates.push(Candidate {
                            block_id: block.block_id.clone(),
                            page: page.page_number,
                            start,
                            end,
                            key: (!page_number(line, page.page_number)).then(|| repeated_key(line)),
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
                                key: Some(repeated_key(line)),
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
        for page in &document.pages {
            let mut has_substantive_text = false;
            for block in &page.content {
                let mut excluded = candidates
                    .iter()
                    .filter(|c| {
                        c.block_id == block.block_id
                            && c.key
                                .as_deref()
                                .is_none_or(|key| pages.get(key).is_some_and(|p| p.len() >= 2))
                    })
                    .map(|c| (c.start, c.end))
                    .collect::<Vec<_>>();
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
            retained,
            excluded_pages,
        }
    }

    pub(super) fn has_retained_text(&self, block: &NormalizedBlock) -> bool {
        self.retained[&block.block_id]
            .iter()
            .any(|&(start, end)| !block.text[start..end].trim().is_empty())
    }

    pub(super) fn warning(&self) -> Option<PipelineWarning> {
        if self.excluded_pages.is_empty() {
            return None;
        }
        let mut pages = self.excluded_pages.iter().copied().collect::<Vec<_>>();
        pages.sort_unstable();
        Some(PipelineWarning {
            code: "SUMMARY_FURNITURE_PAGES_EXCLUDED".into(),
            message: format!("Pages {pages:?} contain only recognized page furniture; excluded from coherent excerpt selection and both page-coverage totals"),
            stage: Some(PipelineStage::Synthesize),
        })
    }

    pub(super) fn segment(
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
