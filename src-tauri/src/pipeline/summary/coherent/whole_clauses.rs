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

// Contract policy v2 exposes source ranges without changing General's versioned
// segmentation above. All ranges follow normalized document order and F1 filtering.
#[derive(Clone, Debug)]
pub(in crate::pipeline::summary) struct SourceFragment {
    pub block_id: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug)]
pub(in crate::pipeline::summary) struct SourceClause {
    pub headings: Vec<String>,
    pub fragments: Vec<SourceFragment>,
    pub parent: Option<usize>,
    pub number: Option<Vec<String>>,
    pub article: Option<u32>,
    pub opening: bool,
    pub heading_only: bool,
}

impl SourceClause {
    pub(in crate::pipeline::summary) fn text(
        &self,
        blocks: &HashMap<&str, &NormalizedBlock>,
    ) -> String {
        self.fragments
            .iter()
            .map(|f| &blocks[f.block_id.as_str()].text[f.start..f.end])
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

fn contract_number(line: &str) -> Option<(Vec<String>, &str)> {
    let line = line
        .trim()
        .strip_prefix('§')
        .unwrap_or(line.trim())
        .trim_start();
    let (marker, rest) = line.split_once(char::is_whitespace)?;
    let marker = marker.strip_suffix(['.', ')']).unwrap_or(marker);
    let parts = marker.split('.').collect::<Vec<_>>();
    if rest.trim().is_empty()
        || parts
            .iter()
            .any(|p| p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some((parts.into_iter().map(str::to_string).collect(), rest.trim()))
}

fn contract_title(text: &str) -> (&str, bool) {
    for (offset, ch) in text.char_indices() {
        if matches!(ch, '.' | ':') {
            let after = &text[offset + ch.len_utf8()..];
            if after.is_empty() || after.starts_with(char::is_whitespace) {
                return (text[..offset].trim(), !after.trim().is_empty());
            }
        }
    }
    (text.trim(), false)
}

fn leading_title(text: &str) -> bool {
    let text = text.trim().strip_suffix(['.', ':']).unwrap_or(text.trim());
    if text.is_empty()
        || !text
            .chars()
            .all(|c| c.is_alphabetic() || c.is_whitespace() || matches!(c, '&' | '/' | '-' | '\''))
    {
        return false;
    }
    let words = text
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>();
    let connectors = [
        "and", "or", "of", "the", "to", "for", "in", "with", "a", "an",
    ];
    words.iter().any(|w| w.starts_with(char::is_uppercase))
        && words.iter().all(|word| {
            connectors.contains(word) || word.chars().all(char::is_uppercase) || {
                let mut chars = word.chars();
                chars.next().is_some_and(char::is_uppercase) && chars.all(char::is_lowercase)
            }
        })
}

fn article_title(text: &str) -> Option<(u32, &str)> {
    let (kind, rest) = text.trim().split_once(char::is_whitespace)?;
    if !kind.eq_ignore_ascii_case("article") {
        return None;
    }
    let rest = rest.trim();
    let end = rest.find(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—' | ':'))?;
    let ordinal = &rest[..end];
    let rest = rest[end..].trim_start();
    let separator = rest.chars().next()?;
    if !matches!(separator, '-' | '–' | '—' | ':') {
        return None;
    }
    let title = rest[separator.len_utf8()..].trim();
    if title.is_empty() {
        return None;
    }
    let number = if ordinal.len() <= 3 && ordinal.bytes().all(|b| b.is_ascii_digit()) {
        ordinal.parse::<u32>().ok().filter(|v| *v > 0)?
    } else {
        // Accept only canonical Roman spelling, not arbitrary Roman-letter words.
        let values = [
            (1000, "M"),
            (900, "CM"),
            (500, "D"),
            (400, "CD"),
            (100, "C"),
            (90, "XC"),
            (50, "L"),
            (40, "XL"),
            (10, "X"),
            (9, "IX"),
            (5, "V"),
            (4, "IV"),
            (1, "I"),
        ];
        let mut tail = ordinal;
        let mut n = 0;
        for (value, token) in values {
            while let Some(after) = tail.strip_prefix(token) {
                n += value;
                tail = after;
                if n > 3999 {
                    return None;
                }
            }
        }
        if !tail.is_empty() || n == 0 {
            return None;
        }
        let mut remaining = n;
        let mut canonical = String::new();
        for (value, token) in values {
            while remaining >= value {
                canonical.push_str(token);
                remaining -= value;
            }
        }
        if canonical != ordinal {
            return None;
        }
        n
    };
    Some((
        number,
        title.strip_suffix(['.', ':']).unwrap_or(title).trim_end(),
    ))
}

fn append_source(
    clause: &mut SourceClause,
    fragment: SourceFragment,
    blocks: &HashMap<&str, &NormalizedBlock>,
) {
    if let Some(last) = clause.fragments.last_mut() {
        if last.block_id == fragment.block_id
            && last.end <= fragment.start
            && blocks[last.block_id.as_str()].text[last.end..fragment.start]
                .trim()
                .is_empty()
        {
            last.end = fragment.end;
            return;
        }
    }
    clause.fragments.push(fragment);
}

fn strict_child(child: &[String], parent: &[String]) -> bool {
    child.len() > parent.len() && child.starts_with(parent)
}

pub(in crate::pipeline::summary) fn contract_sources(
    document: &NormalizedDocument,
) -> Vec<SourceClause> {
    let furniture = page_furniture::Furniture::new(document);
    let blocks = document
        .pages
        .iter()
        .flat_map(|p| &p.content)
        .map(|b| (b.block_id.as_str(), b))
        .collect::<HashMap<_, _>>();
    let mut lines = Vec::new();
    for page in &document.pages {
        for block in &page.content {
            for &(start, end) in furniture.retained_ranges(&block.block_id) {
                let mut offset = start;
                for line in block.text[start..end].split_inclusive('\n') {
                    let value = line.trim();
                    if !value.is_empty() {
                        let begin = offset + line.len() - line.trim_start().len();
                        lines.push(SourceFragment {
                            block_id: block.block_id.clone(),
                            start: begin,
                            end: begin + value.len(),
                        });
                    }
                    offset += line.len();
                }
            }
        }
    }
    let line_text = |f: &SourceFragment| &blocks[f.block_id.as_str()].text[f.start..f.end];
    let mut clauses: Vec<SourceClause> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let fragment = lines[i].clone();
        let text = line_text(&fragment);
        if let Some((article, title)) = article_title(text) {
            stack.clear();
            let index = clauses.len();
            clauses.push(SourceClause {
                headings: vec![title.to_string()],
                fragments: vec![fragment],
                parent: None,
                number: None,
                article: Some(article),
                opening: false,
                heading_only: true,
            });
            stack.push(index);
            i += 1;
            continue;
        }
        let mut leading = None;
        let mut numbered = contract_number(text);
        if numbered.is_none() && leading_title(text) && i + 1 < lines.len() {
            if let Some(next) = contract_number(line_text(&lines[i + 1])) {
                leading = Some((
                    fragment.clone(),
                    text.trim_end_matches(['.', ':']).to_string(),
                ));
                numbered = Some(next);
                i += 1;
            }
        }
        if let Some((number, rest)) = numbered {
            // Pop ancestors by numeric components, keeping the enclosing article.
            while let Some(&last) = stack.last() {
                if clauses[last].article.is_some()
                    || clauses[last]
                        .number
                        .as_ref()
                        .is_some_and(|p| strict_child(&number, p))
                {
                    break;
                }
                stack.pop();
            }
            if number.len() > 1 && leading.is_some() {
                let parent_number = &number[..number.len() - 1];
                if !stack
                    .iter()
                    .any(|&n| clauses[n].number.as_deref() == Some(parent_number))
                {
                    let (fragment, heading) = leading.take().expect("checked leading title");
                    let index = clauses.len();
                    clauses.push(SourceClause {
                        headings: vec![heading],
                        fragments: vec![fragment],
                        parent: stack.last().copied(),
                        number: Some(parent_number.to_vec()),
                        article: None,
                        opening: false,
                        heading_only: true,
                    });
                    stack.push(index);
                }
            }
            let (title, has_body) = contract_title(rest);
            let mut clause = SourceClause {
                headings: vec![],
                fragments: vec![],
                parent: stack.last().copied(),
                number: Some(number),
                article: None,
                opening: false,
                heading_only: !has_body && leading.is_none(),
            };
            if let Some((fragment, title)) = leading {
                clause.headings.push(title);
                append_source(&mut clause, fragment, &blocks);
            }
            if !title.is_empty() {
                clause.headings.push(title.to_string());
            }
            append_source(&mut clause, lines[i].clone(), &blocks);
            stack.push(clauses.len());
            clauses.push(clause);
        } else if let Some(clause) = clauses.last_mut() {
            clause.heading_only = false;
            append_source(clause, fragment, &blocks);
        } else {
            clauses.push(SourceClause {
                headings: vec![],
                fragments: vec![fragment],
                parent: None,
                number: None,
                article: None,
                opening: true,
                heading_only: false,
            });
        }
        i += 1;
    }
    clauses
}

pub(in crate::pipeline::summary) fn section_members(
    clauses: &[SourceClause],
    root: usize,
) -> Vec<usize> {
    let mut result = vec![root];
    for (index, clause) in clauses.iter().enumerate().skip(root + 1) {
        let mut parent = clause.parent;
        while let Some(n) = parent {
            if n == root {
                break;
            }
            parent = clauses[n].parent;
        }
        if parent != Some(root) {
            break;
        }
        result.push(index);
    }
    result
}
