//! The new General policy owns dependency identity here, before request planning.
//! Historical clause contexts and Contract extraction do not consume this projection.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(in crate::pipeline::summary) struct ExternalSource {
    pub document: String,
    pub reason: &'static str,
    pub disclosure: &'static str,
}

impl ExternalSource {
    fn foreign(number: &[String]) -> Self {
        Self {
            document: number.join("."),
            reason: "foreign_numbering",
            disclosure: "text not in this document",
        }
    }
    fn annex(kind: &str, label: &str) -> Self {
        Self {
            document: format!("{kind} {label}"),
            reason: "absent_annex",
            disclosure: "attachment not included in this document",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::pipeline::summary) enum ReferenceStatus {
    Resolved(Vec<usize>),
    External(ExternalSource),
    Unresolved(&'static str),
}

#[derive(Clone, Debug)]
pub(in crate::pipeline::summary) struct Reference {
    pub span: SourceFragment,
    #[cfg(test)]
    pub text: String,
    pub status: ReferenceStatus,
    // Captured at the reader, consumed once by the source-identity resolver.
    namespace: Option<QualifiedNamespace>,
}

#[derive(Clone, Debug)]
enum QualifiedNamespace {
    ExplicitDocument(String),
    OpaqueName(String),
}
impl QualifiedNamespace {
    fn name(&self) -> &str {
        match self {
            Self::ExplicitDocument(name) | Self::OpaqueName(name) => name,
        }
    }
}

pub(in crate::pipeline::summary) struct DependencyProjection {
    pub clauses: Vec<SourceClause>,
    pub ids: Vec<String>,
    pub references: Vec<Vec<Reference>>,
    pub closures: Vec<Vec<usize>>,
    #[cfg(test)]
    pub excluded_heading_references: usize,
}

struct Token<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}
fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut result = Vec::new();
    let mut start = None;
    for (offset, ch) in text.char_indices().chain([(text.len(), ' ')]) {
        if ch == '§' {
            if let Some(begin) = start.take() {
                result.push(Token {
                    start: begin,
                    end: offset,
                    text: &text[begin..offset],
                });
            }
            result.push(Token {
                start: offset,
                end: offset + ch.len_utf8(),
                text: &text[offset..offset + ch.len_utf8()],
            });
        } else if ch.is_alphanumeric() || ch == '.' {
            start.get_or_insert(offset);
        } else if let Some(begin) = start.take() {
            result.push(Token {
                start: begin,
                end: offset,
                text: &text[begin..offset],
            });
        }
    }
    result
}
fn explicit_marker(token: &str) -> bool {
    let token = token.trim_end_matches('.');
    token.chars().next().is_some_and(|c| c.is_ascii_digit())
        || token.chars().count() == 1 && token.chars().all(char::is_alphabetic)
        || matches!(token.to_ascii_lowercase().as_str(), "above" | "below")
}
#[derive(Clone)]
enum Kind {
    Section,
    Article,
    Annex(String),
    Unsupported,
}
fn kind(token: &str) -> Option<Kind> {
    match token.to_ascii_lowercase().as_str() {
        "section" | "sections" | "§" => Some(Kind::Section),
        "article" | "articles" => Some(Kind::Article),
        "paragraph" | "paragraphs" | "subsection" | "subsections" | "annex" => {
            Some(Kind::Unsupported)
        }
        _ => annex_kind(token).map(Kind::Annex),
    }
}
#[derive(Clone, PartialEq, Eq)]
enum Target {
    Section(Vec<String>),
    Article(u32),
    Annex(String, String),
    Range(Box<Target>, Box<Target>, bool),
    UnsupportedSuffix(Box<Target>),
}
fn target(token: &str, kind: &Kind) -> Option<Target> {
    match kind {
        Kind::Section => contract_marker(token).map(Target::Section),
        Kind::Article => article_ordinal(token.trim_end_matches('.')).map(Target::Article),
        Kind::Annex(kind) => {
            annex_label(token).map(|label| Target::Annex(kind.clone(), label.into()))
        }
        Kind::Unsupported => None,
    }
}
fn token_end(token: &Token<'_>) -> usize {
    token.end - (token.text.len() - token.text.trim_end_matches('.').len())
}

fn initialism(word: &str) -> bool {
    let letters = word
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect::<Vec<_>>();
    letters.len() > 1
        && letters.iter().all(|c| c.is_ascii_uppercase())
        && word.chars().all(|c| c.is_ascii_uppercase() || c == '.')
}

// Qualified names are read structurally. No document/law name or abbreviation
// vocabulary supplies an identity. A bare capitalized name is never scanned.
fn qualifier_name_part(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_uppercase)
        || word.chars().all(|c| c.is_ascii_digit())
        || matches!(word.to_ascii_lowercase().as_str(), "of" | "the")
}

fn preposed_qualifier(
    text: &str,
    ts: &[Token<'_>],
    section: usize,
) -> Option<(usize, Option<QualifiedNamespace>)> {
    let end = section.checked_sub(1)?;
    if text[ts[end].end..ts[section].start].contains(['\n', ';', ',', '(', ')'])
        || !qualifier_name_part(ts[end].text)
        || ts[end].text.ends_with('.') && !initialism(ts[end].text)
    {
        return None;
    }
    let mut start = end;
    while start > 0 {
        let prior = &ts[start - 1];
        if prior.text.eq_ignore_ascii_case("this") {
            return None;
        }
        if text[prior.end..ts[start].start].contains(['\n', ';', ',', '(', ')'])
            || prior.text.ends_with('.') && !initialism(prior.text)
            || !qualifier_name_part(prior.text)
        {
            break;
        }
        start -= 1;
    }
    while start < end && matches!(ts[start].text.to_ascii_lowercase().as_str(), "of" | "the") {
        start += 1;
    }
    let numeric_namespace = initialism(ts[end].text)
        && end > 0
        && ts[end - 1].text.chars().all(|c| c.is_ascii_digit())
        && text[ts[end - 1].end..ts[end].start].trim().is_empty();
    if numeric_namespace {
        start = end - 1;
    }
    let words = &ts[start..=end];
    let named = words
        .iter()
        .filter(|t| !matches!(t.text.to_ascii_lowercase().as_str(), "of" | "the"))
        .count()
        >= 2
        && words.iter().any(|t| t.text.chars().any(char::is_uppercase));
    let recognized = numeric_namespace
        || matches!(
            ts[end]
                .text
                .trim_end_matches('.')
                .to_ascii_lowercase()
                .as_str(),
            "document" | "agreement" | "contract"
        );
    named.then(|| {
        (
            start,
            recognized.then(|| {
                QualifiedNamespace::ExplicitDocument(
                    text[ts[start].start..if initialism(ts[end].text) {
                        ts[end].end
                    } else {
                        token_end(&ts[end])
                    }]
                        .to_owned(),
                )
            }),
        )
    })
}

// Consume the document qualifier once, preserving its bytes even for this document.
fn qualifier(
    text: &str,
    ts: &[Token<'_>],
    end: usize,
) -> (usize, Option<QualifiedNamespace>, bool) {
    if !ts
        .get(end)
        .is_some_and(|t| t.text.eq_ignore_ascii_case("of"))
    {
        return (end, None, false);
    }
    let mut cursor = end + 1;
    if ts
        .get(cursor)
        .is_some_and(|t| t.text.eq_ignore_ascii_case("this"))
    {
        cursor += 1;
        let current = ts.get(cursor).is_some_and(|t| {
            t.text
                .trim_end_matches('.')
                .chars()
                .all(char::is_alphabetic)
        });
        return ((cursor + 1).min(ts.len()), None, !current);
    }
    if ts
        .get(cursor)
        .is_some_and(|t| t.text.eq_ignore_ascii_case("the"))
    {
        cursor += 1;
    }
    let begin = cursor;
    // Explicit numeric namespace: the spelling of the initialism is never looked up.
    if let (Some(number), Some(namespace)) = (ts.get(begin), ts.get(begin + 1)) {
        if number.text.chars().all(|c| c.is_ascii_digit())
            && initialism(namespace.text)
            && text[number.end..namespace.start].trim().is_empty()
        {
            return (
                begin + 2,
                Some(QualifiedNamespace::ExplicitDocument(
                    text[number.start..namespace.end].to_owned(),
                )),
                false,
            );
        }
    }
    while let Some(t) = ts.get(cursor) {
        if cursor > begin
            && (text[ts[cursor - 1].end..t.start].contains([';', ',', '(', ')'])
                || ts[cursor - 1].text.ends_with('.') && !initialism(ts[cursor - 1].text))
        {
            break;
        }
        let word = t.text.trim_end_matches('.').to_ascii_lowercase();
        let document = word == "document";
        let named = matches!(word.as_str(), "agreement" | "contract")
            && cursor > begin
            && ts[begin..cursor]
                .iter()
                .all(|w| w.text.chars().next().is_some_and(char::is_uppercase));
        if document || named {
            let mut limit = cursor + 1;
            if document {
                while ts.get(limit).is_some_and(|next| {
                    next.text.chars().any(|c| c.is_ascii_digit())
                        && !text[ts[limit - 1].end..next.start].contains([';', ',', '(', ')'])
                }) {
                    limit += 1;
                }
            }
            let name = &text[ts[begin].start..token_end(&ts[limit - 1])];
            return (
                limit,
                Some(QualifiedNamespace::ExplicitDocument(name.to_owned())),
                false,
            );
        }
        if !t.text.chars().next().is_some_and(char::is_uppercase)
            && !t.text.chars().all(|c| c.is_ascii_digit())
            && !matches!(word.as_str(), "of" | "the")
        {
            break;
        }
        cursor += 1;
    }
    let name = (cursor > begin
        && ts[begin]
            .text
            .chars()
            .next()
            .is_some_and(char::is_uppercase))
    .then(|| text[ts[begin].start..token_end(&ts[cursor - 1])].to_owned());
    (
        cursor.max(end + 1),
        name.map(QualifiedNamespace::OpaqueName),
        true,
    )
}

fn scan(text: &str, span: &SourceFragment) -> Vec<(Reference, Vec<Target>)> {
    let ts = tokens(text);
    let mut result = Vec::new();
    let mut i = 0;
    while i < ts.len() {
        let mut start = i;
        let plural = matches!(
            ts[i].text.to_ascii_lowercase().as_str(),
            "sections" | "articles"
        );
        let Some(kind) = kind(ts[i].text) else {
            i += 1;
            continue;
        };
        if !ts
            .get(i + 1)
            .is_some_and(|t| explicit_marker(t.text) || target(t.text, &kind).is_some())
        {
            i += 1;
            continue;
        }
        let mut unresolved_preposed = false;
        let preposed = preposed_qualifier(text, &ts, i).and_then(|(begin, document)| {
            start = begin;
            unresolved_preposed = document.is_none();
            document
        });
        i += 1;
        let mut invalid = matches!(kind, Kind::Unsupported);
        let mut targets = Vec::new();
        if let Some(t) = target(ts[i].text, &kind) {
            targets.push(t);
        } else {
            invalid = true;
        }
        let mut end_byte = token_end(&ts[i]);
        if text[ts[i - 1].end..ts[i].start].trim() == "(" && text[end_byte..].starts_with(')') {
            end_byte += 1;
        }
        i += 1;
        while let Some(next) = ts.get(i) {
            let gap = &text[end_byte..next.start];
            // Read from the consumed source boundary, including any caption.
            if gap.contains(['.', ';', ':', ')']) {
                break;
            }
            if gap.trim_start().starts_with('(') {
                let close = text[next.start..].find(')').map(|n| next.start + n);
                let caption = gap.starts_with(char::is_whitespace)
                    && next.text.chars().count() > 1
                    && next.text.chars().all(char::is_alphabetic);
                if let Some(close) =
                    close.filter(|&close| !text[next.start..close].contains([';', '\n']))
                {
                    invalid |= !caption;
                    end_byte = close + 1;
                    while ts.get(i).is_some_and(|t| t.start <= close) {
                        i += 1;
                    }
                    continue;
                } else {
                    invalid = true;
                    break;
                }
            }
            let dash = matches!(gap.trim(), "-" | "–");
            let range = dash || next.text.eq_ignore_ascii_case("through");
            let connector = range
                || next.text.eq_ignore_ascii_case("and")
                || next.text.eq_ignore_ascii_case("or");
            let n = if dash {
                i
            } else if connector {
                i + 1
            } else if gap.trim() == "," {
                i
            } else {
                break;
            };
            let Some(t) = ts.get(n).and_then(|t| target(t.text, &kind)) else {
                if range || connector && ts.get(n).is_some_and(|t| explicit_marker(t.text)) {
                    invalid = true;
                }
                break;
            };
            if range {
                if let Some(first) = targets.pop() {
                    targets.push(Target::Range(Box::new(first), Box::new(t), dash && !plural));
                } else {
                    invalid = true;
                }
            } else {
                targets.push(t);
            }
            end_byte = token_end(&ts[n]);
            i = n + 1;
        }
        let (end, namespace, unclear) = qualifier(text, &ts, i);
        let conflict = preposed
            .as_ref()
            .zip(namespace.as_ref())
            .is_some_and(|(a, b)| a.name() != b.name())
            || preposed.is_some() && end > i && namespace.is_none()
            || unresolved_preposed && namespace.is_some();
        let namespace = if conflict {
            None
        } else {
            namespace.or(preposed)
        };
        if end > i {
            end_byte = token_end(&ts[end - 1]);
            i = end;
        }
        // An unqualified hyphen form can only be a local range when its
        // endpoints are owned. Resolve that identity before diagnosing a suffix.
        if invalid && targets.iter().any(|t| matches!(t, Target::Range(..))) {
            targets = targets
                .into_iter()
                .map(|t| Target::UnsupportedSuffix(Box::new(t)))
                .collect();
            invalid = false;
        }
        #[cfg(test)]
        let raw = &text[ts[start].start..end_byte];
        let status = if matches!(namespace, Some(QualifiedNamespace::ExplicitDocument(_))) {
            ReferenceStatus::Unresolved("ambiguous_document_qualifier")
        } else if invalid {
            ReferenceStatus::Unresolved("unsupported_form")
        } else if namespace.is_some() || unclear || unresolved_preposed || conflict {
            ReferenceStatus::Unresolved("ambiguous_document_qualifier")
        } else {
            ReferenceStatus::Resolved(vec![])
        };
        result.push((
            Reference {
                span: SourceFragment {
                    block_id: span.block_id.clone(),
                    start: span.start + ts[start].start,
                    end: span.start + end_byte,
                },
                #[cfg(test)]
                text: raw.into(),
                status,
                namespace,
            },
            targets,
        ));
    }
    result
}

fn annex_scope(clauses: &[SourceClause], mut index: usize) -> Option<usize> {
    loop {
        if clauses[index].annex.is_some() {
            return Some(index);
        }
        index = clauses[index].parent?;
    }
}

fn exact_targets(clauses: &[SourceClause], target: &Target, origin: usize) -> Vec<usize> {
    clauses
        .iter()
        .enumerate()
        .filter(|(i, c)| match target {
            Target::Section(n) => {
                c.number.as_ref() == Some(n)
                    && annex_scope(clauses, *i) == annex_scope(clauses, origin)
            }
            Target::Article(n) => {
                c.article == Some(*n) && annex_scope(clauses, *i) == annex_scope(clauses, origin)
            }
            Target::Annex(kind, label) => c
                .annex
                .as_ref()
                .is_some_and(|(k, l)| k == kind && l == label),
            Target::Range(..) | Target::UnsupportedSuffix(_) => false,
        })
        .map(|(i, _)| i)
        .collect()
}
fn single_target(
    clauses: &[SourceClause],
    target: &Target,
    origin: usize,
) -> Result<usize, &'static str> {
    let matches = exact_targets(clauses, target, origin);
    match matches.as_slice() {
        [i] => Ok(*i),
        [] => Err("missing_target"),
        _ => Err("duplicate_target"),
    }
}
fn numeric(number: &[String]) -> Vec<u32> {
    number
        .iter()
        .flat_map(|s| s.split('-'))
        .map(|s| s.parse().expect("owned numeric heading"))
        .collect()
}
fn numbering_family(number: &[String]) -> String {
    let Some(last_dash) = number.iter().rposition(|part| part.contains('-')) else {
        return "decimal".into();
    };
    number[..=last_dash]
        .iter()
        .map(|part| part.split('-').map(|_| "N").collect::<Vec<_>>().join("-"))
        .collect::<Vec<_>>()
        .join(".")
}
fn numbering_compatible(clauses: &[SourceClause], number: &[String]) -> bool {
    let families = clauses
        .iter()
        .filter_map(|c| c.number.as_ref())
        .map(|n| numbering_family(n))
        .collect::<HashSet<_>>();
    families.is_empty() || families.contains(&numbering_family(number))
}
fn range_status(
    clauses: &[SourceClause],
    first: &Target,
    last: &Target,
    origin: usize,
) -> Result<ReferenceStatus, &'static str> {
    let low = single_target(clauses, first, origin)?;
    let high = single_target(clauses, last, origin)?;
    if low > high {
        return Err("reversed_range");
    }
    let valid = match (first, last) {
        (Target::Section(a), Target::Section(b)) => {
            numbering_family(a) == numbering_family(b) && numeric(a) <= numeric(b)
        }
        (Target::Article(a), Target::Article(b)) => a <= b,
        _ => false,
    };
    if !valid {
        return Err("unsupported_form");
    }
    let mut result = Vec::new();
    for i in low..=high {
        let selected = match (first, last) {
            (Target::Section(a), Target::Section(b)) => {
                clauses[i].number.as_ref().is_some_and(|n| {
                    numbering_family(n) == numbering_family(a)
                        && numeric(a) <= numeric(n)
                        && numeric(n) <= numeric(b)
                })
            }
            (Target::Article(a), Target::Article(b)) => {
                clauses[i].article.is_some_and(|n| *a <= n && n <= *b)
            }
            _ => false,
        };
        if selected && annex_scope(clauses, i) == annex_scope(clauses, origin) {
            result.push(i);
        }
    }
    Ok(ReferenceStatus::Resolved(result))
}
fn resolve(
    clauses: &[SourceClause],
    target: &Target,
    origin: usize,
) -> Result<ReferenceStatus, &'static str> {
    if let Target::UnsupportedSuffix(base) = target {
        let status = resolve(clauses, base, origin)?;
        return if matches!(status, ReferenceStatus::External(_)) {
            Ok(status)
        } else {
            Err("unsupported_form")
        };
    }
    if let Target::Range(first, last, singular_dash) = target {
        if let (Target::Section(a), Target::Section(b)) = (first.as_ref(), last.as_ref()) {
            let combined = whole_document_marker(&format!("{}-{}", a.join("."), b.join(".")))
                .ok_or("unsupported_form")?;
            let identity = Target::Section(combined.clone());
            let exact = exact_targets(clauses, &identity, origin);
            let first_owned = !exact_targets(clauses, first, origin).is_empty();
            let last_owned = !exact_targets(clauses, last, origin).is_empty();
            if !singular_dash && first_owned && last_owned {
                return range_status(clauses, first, last, origin);
            }
            if exact.len() == 1 && *singular_dash && first_owned && last_owned {
                return Err("ambiguous_numbering");
            }
            if !exact.is_empty() {
                return single_target(clauses, &identity, origin)
                    .map(|i| ReferenceStatus::Resolved(section_members(clauses, i)));
            }
            if *singular_dash {
                if first_owned || last_owned {
                    return range_status(clauses, first, last, origin);
                }
                return if numbering_compatible(clauses, &combined) {
                    Err("missing_target")
                } else {
                    Ok(ReferenceStatus::External(ExternalSource::foreign(
                        &combined,
                    )))
                };
            }
        }
        return range_status(clauses, first, last, origin);
    }
    if let Target::Annex(kind, label) = target {
        if exact_targets(clauses, target, origin).is_empty() {
            return Ok(ReferenceStatus::External(ExternalSource::annex(
                kind, label,
            )));
        }
    }
    match single_target(clauses, target, origin) {
        Ok(i) => Ok(ReferenceStatus::Resolved(section_members(clauses, i))),
        Err("missing_target") => match target {
            Target::Section(number) if !numbering_compatible(clauses, number) => {
                Ok(ReferenceStatus::External(ExternalSource::foreign(number)))
            }
            _ => Err("missing_target"),
        },
        Err(reason) => Err(reason),
    }
}

fn identity_words(text: &str) -> Vec<String> {
    tokens(text)
        .iter()
        .map(|t| t.text.trim_end_matches('.').to_lowercase())
        .collect()
}

fn namespace_is_owned(name: &str, clauses: &[SourceClause], identities: &[SourceIdentity]) -> bool {
    let identity = identity_words(name);
    if identity.is_empty() {
        return true;
    }
    clauses.iter().any(|clause| {
        clause.headings.iter().any(|heading| {
            identity_words(heading)
                .windows(identity.len())
                .any(|words| words == identity)
        }) || clause
            .annex
            .as_ref()
            .is_some_and(|(kind, label)| identity_words(&format!("{kind} {label}")) == identity)
    }) || identities.iter().any(|entry| match entry {
        SourceIdentity::Heading(text) | SourceIdentity::Declared(text) => identity_words(text)
            .windows(identity.len())
            .any(|words| words == identity),
        SourceIdentity::Introduction(text) => {
            let words = identity_words(text);
            words.first().is_some_and(|w| w == "this") && words[1..].starts_with(&identity)
        }
    })
}

fn numbered_target_is_owned(target: &Target, clauses: &[SourceClause]) -> bool {
    match target {
        Target::Section(number) => clauses.iter().any(|c| c.number.as_ref() == Some(number)),
        Target::Article(number) => clauses.iter().any(|c| c.article == Some(*number)),
        // Only the explicitly accepted singular numbered form can gain this status.
        _ => true,
    }
}

impl DependencyProjection {
    pub(in crate::pipeline::summary) fn new(document: &NormalizedDocument) -> Self {
        let (clauses, identities) = contract_source_inventory(
            document,
            page_furniture::Policy::RunningMetadata,
            HeadingGrammar::WholeDocument,
        );
        let blocks = document
            .pages
            .iter()
            .flat_map(|p| &p.content)
            .map(|b| (b.block_id.as_str(), b))
            .collect::<HashMap<_, _>>();
        let ids = clauses
            .iter()
            .map(|clause| {
                let spans = clause
                    .fragments
                    .iter()
                    .map(|f| {
                        (
                            &f.block_id,
                            f.start,
                            f.end,
                            &blocks[f.block_id.as_str()].text[f.start..f.end],
                        )
                    })
                    .collect::<Vec<_>>();
                format!(
                    "clause-{:x}",
                    Sha256::digest(
                        serde_json::to_vec(&(&document.document_id, spans))
                            .expect("source identity serializes")
                    )
                )
            })
            .collect();
        #[cfg(test)]
        let mut excluded_heading_references = 0;
        let mut references = Vec::new();
        for (origin, clause) in clauses.iter().enumerate() {
            let mut owned = Vec::new();
            for fragment in &clause.fragments {
                let text = &blocks[fragment.block_id.as_str()].text[fragment.start..fragment.end];
                for (mut reference, targets) in scan(text, fragment) {
                    if clause.heading_spans.iter().any(|h| {
                        h.block_id == reference.span.block_id
                            && h.start <= reference.span.start
                            && reference.span.start < h.end
                    }) {
                        #[cfg(test)]
                        {
                            excluded_heading_references += 1;
                        }
                        continue;
                    }
                    if matches!(
                        reference.status,
                        ReferenceStatus::Unresolved("ambiguous_document_qualifier")
                    ) {
                        if let Some(namespace) = &reference.namespace {
                            let name = namespace.name();
                            let admissible = match namespace {
                                QualifiedNamespace::ExplicitDocument(_) => true,
                                QualifiedNamespace::OpaqueName(_) => {
                                    targets.len() == 1
                                        && targets.first().is_some_and(|target| {
                                            !numbered_target_is_owned(target, &clauses)
                                        })
                                }
                            };
                            if admissible && !namespace_is_owned(name, &clauses, &identities) {
                                reference.status = ReferenceStatus::External(ExternalSource {
                                    document: name.to_owned(),
                                    reason: match namespace {
                                        QualifiedNamespace::ExplicitDocument(_) => {
                                            "absent_document"
                                        }
                                        QualifiedNamespace::OpaqueName(_) => {
                                            "unowned_named_document"
                                        }
                                    },
                                    disclosure: "text not in this document",
                                });
                            }
                        }
                    }
                    if matches!(reference.status, ReferenceStatus::Resolved(_)) {
                        // Each list member owns a status. A foreign member must not
                        // erase an internal dependency, or hide an unresolved sibling.
                        for target in targets {
                            let mut item = reference.clone();
                            item.status = match resolve(&clauses, &target, origin) {
                                Ok(status) => status,
                                Err(reason) => ReferenceStatus::Unresolved(reason),
                            };
                            owned.push(item);
                        }
                    } else {
                        owned.push(reference);
                    }
                }
            }
            references.push(owned);
        }
        let closures = (0..clauses.len())
            .map(|root| {
                let mut visited = HashSet::new();
                let mut pending = vec![root];
                while let Some(index) = pending.pop() {
                    if !visited.insert(index) {
                        continue;
                    }
                    if let Some(parent) = clauses[index].parent {
                        pending.push(parent);
                    }
                    for reference in &references[index] {
                        if let ReferenceStatus::Resolved(targets) = &reference.status {
                            pending.extend(targets);
                        }
                    }
                }
                let mut result = visited.into_iter().collect::<Vec<_>>();
                result.sort_unstable();
                result
            })
            .collect();
        Self {
            clauses,
            ids,
            references,
            closures,
            #[cfg(test)]
            excluded_heading_references,
        }
    }

    pub(in crate::pipeline::summary) fn blocking_reasons(&self, index: usize) -> Vec<&'static str> {
        let mut result = Vec::new();
        for &i in &self.closures[index] {
            if self.clauses[i].boundary_uncertain {
                result.push("boundary_uncertain");
            }
            for reference in &self.references[i] {
                if let ReferenceStatus::Unresolved(reason) = reference.status {
                    result.push(reason);
                }
            }
        }
        result.sort_unstable();
        result.dedup();
        result
    }

    #[cfg(test)]
    pub(in crate::pipeline::summary) fn context(
        &self,
        index: usize,
        document: &NormalizedDocument,
    ) -> String {
        let blocks = document
            .pages
            .iter()
            .flat_map(|p| &p.content)
            .map(|b| (b.block_id.as_str(), b))
            .collect::<HashMap<_, _>>();
        self.closures[index]
            .iter()
            .map(|&i| self.clauses[i].text(&blocks))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}
