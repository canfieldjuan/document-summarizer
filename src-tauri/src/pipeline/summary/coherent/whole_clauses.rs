//! Numbered source units retain their opening words before page balancing.
//! Citation quotes stay within one original block; context may span blocks.
use super::*;

struct Fragment {
    text: String,
}

struct Clause {
    number: String,
    parents: Vec<usize>,
    fragments: Vec<Fragment>,
}

pub(super) struct Clauses {
    segments: HashMap<String, Vec<String>>,
    contexts: HashMap<(String, String), String>,
    omitted: HashMap<String, usize>,
}

fn number(line: &str) -> Option<String> {
    let line = line.trim().trim_start_matches('§').trim_start();
    let (marker, rest) = line.split_once(char::is_whitespace)?;
    let marker = marker.trim_end_matches(['.', ')']);
    (!marker.is_empty()
        && marker.split('.').all(|part| {
            !part.is_empty() && part.len() <= 3 && part.bytes().all(|b| b.is_ascii_digit())
        })
        && rest
            .trim_start()
            .chars()
            .next()
            .is_some_and(char::is_uppercase))
    .then(|| marker.to_string())
}

impl Clauses {
    pub(super) fn new(
        document: &NormalizedDocument,
        furniture: &page_furniture::Furniture,
        analysis_version: &str,
    ) -> Self {
        let mut clauses: Vec<Clause> = Vec::new();
        let mut segments: HashMap<String, Vec<String>> = HashMap::new();
        let mut order: Vec<(String, Option<usize>, String)> = Vec::new();
        let mut active = None;
        let mut parents: Vec<usize> = Vec::new();
        // Normalized content order, never lexical block IDs or HashMap order.
        for page in &document.pages {
            for block in &page.content {
                for &(start, end) in furniture.retained_ranges(&block.block_id) {
                    let mut begin = start;
                    let mut offset = start;
                    for line in block.text[start..end].split_inclusive('\n') {
                        if let Some(marker) = number(line) {
                            let prefix = block.text[begin..offset].trim();
                            // A printed governing title immediately before a numbered
                            // clause opens that clause, not the preceding page's clause.
                            let leading_title = !prefix.is_empty()
                                && prefix.chars().any(char::is_alphabetic)
                                && prefix
                                    .chars()
                                    .all(|c| c.is_whitespace() || c.is_uppercase());
                            if !leading_title {
                                append(&mut clauses, &mut order, active, block, begin, offset);
                            }
                            while parents.last().is_some_and(|&index| {
                                !marker.starts_with(&format!("{}.", clauses[index].number))
                            }) {
                                parents.pop();
                            }
                            let index = clauses.len();
                            clauses.push(Clause {
                                number: marker,
                                parents: parents.clone(),
                                fragments: Vec::new(),
                            });
                            parents.push(index);
                            active = Some(index);
                            if !leading_title {
                                begin = offset;
                            }
                        }
                        offset += line.len();
                    }
                    append(&mut clauses, &mut order, active, block, begin, end);
                }
            }
        }
        let mut omitted: HashMap<String, usize> = HashMap::new();
        let mut contexts: HashMap<(String, String), String> = HashMap::new();
        for (block_id, clause_index, text) in order {
            let output = segments.entry(block_id.clone()).or_default();
            if let Some(index) = clause_index {
                let clause = &clauses[index];
                let context = clause
                    .parents
                    .iter()
                    .copied()
                    .chain([index])
                    .flat_map(|i| clauses[i].fragments.iter())
                    .map(|fragment| fragment.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                // Repeated wording in one block must not acquire arbitrary context.
                let key = (block_id, text.clone());
                if let Some(previous) = contexts.get_mut(&key) {
                    if *previous != context {
                        previous.clear();
                    }
                } else {
                    contexts.insert(key, context);
                }
                output.push(text);
            } else {
                let segmentation = analysis_quote_segmentation_for_version(analysis_version, &text);
                output.extend(segmentation.segments);
                *omitted.entry(block_id).or_default() += segmentation.omitted_source_units;
            }
        }
        Self {
            segments,
            contexts,
            omitted,
        }
    }

    pub(super) fn segment(&self, block: &NormalizedBlock) -> AnalysisQuoteSegmentation {
        let mut result = AnalysisQuoteSegmentation {
            segments: self
                .segments
                .get(&block.block_id)
                .cloned()
                .unwrap_or_default(),
            omitted_source_units: self
                .omitted
                .get(&block.block_id)
                .copied()
                .unwrap_or_default(),
        };
        result.segments.retain(|quote| {
            let ambiguous = self
                .contexts
                .get(&(block.block_id.clone(), quote.clone()))
                .is_some_and(String::is_empty);
            result.omitted_source_units += usize::from(ambiguous);
            !ambiguous
        });
        result
    }

    pub(super) fn context(&self, block_id: &str, quote: &str) -> Option<String> {
        self.contexts
            .get(&(block_id.to_string(), quote.to_string()))
            .filter(|text| !text.is_empty())
            .cloned()
    }
}

fn append(
    clauses: &mut [Clause],
    order: &mut Vec<(String, Option<usize>, String)>,
    active: Option<usize>,
    block: &NormalizedBlock,
    start: usize,
    end: usize,
) {
    let text = block.text[start..end].trim();
    if text.is_empty() {
        return;
    }
    if let Some(index) = active {
        clauses[index].fragments.push(Fragment {
            text: text.to_string(),
        });
    }
    order.push((block.block_id.clone(), active, text.to_string()));
}
