//! Deterministic high-precision checks that can only downgrade a coherent-summary
//! model verdict when a source relationship changes mechanically.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumericRelation {
    LessThan,
    AtMost,
    GreaterThan,
    AtLeast,
    Equal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NumericConstraint {
    value: String,
    relation: NumericRelation,
    context: Vec<String>,
    trailing_subject: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NumericReference {
    context: Vec<String>,
    trailing_subject: Vec<String>,
}

fn ends_with_words(words: &[String], suffix: &[&str]) -> bool {
    words.len() >= suffix.len()
        && words[words.len() - suffix.len()..]
            .iter()
            .map(String::as_str)
            .eq(suffix.iter().copied())
}

fn numeric_value(word: &str) -> Option<String> {
    let (negative, word) = if let Some(value) = word.strip_prefix('-') {
        (true, value)
    } else if let Some(value) = word.strip_prefix('+') {
        (false, value)
    } else {
        (false, word)
    };
    let mut parts = word.split('.');
    let integer = parts.next()?;
    let fraction = parts.next();
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.chars().all(|character| character.is_ascii_digit())
        || fraction.is_some_and(|value| {
            value.is_empty() || !value.chars().all(|character| character.is_ascii_digit())
        })
    {
        return None;
    }
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let fraction = fraction.map(|value| value.trim_end_matches('0'));
    let normalized = match fraction {
        Some("") | None => integer.to_string(),
        Some(value) => format!("{integer}.{value}"),
    };
    Some(if negative && normalized != "0" {
        format!("-{normalized}")
    } else {
        normalized
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NumericMention {
    value: String,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardinalKind {
    Small,
    Tens,
    Hundred,
    Scale,
}

fn small_cardinal(word: &str) -> Option<u64> {
    Some(match word {
        "zero" => 0,
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        "thirteen" => 13,
        "fourteen" => 14,
        "fifteen" => 15,
        "sixteen" => 16,
        "seventeen" => 17,
        "eighteen" => 18,
        "nineteen" => 19,
        _ => return None,
    })
}

fn tens_cardinal(word: &str) -> Option<u64> {
    Some(match word {
        "twenty" => 20,
        "thirty" => 30,
        "forty" => 40,
        "fifty" => 50,
        "sixty" => 60,
        "seventy" => 70,
        "eighty" => 80,
        "ninety" => 90,
        _ => return None,
    })
}

fn cardinal_scale(word: &str) -> Option<u64> {
    Some(match word {
        "thousand" => 1_000,
        "million" => 1_000_000,
        "billion" => 1_000_000_000,
        _ => return None,
    })
}

fn spelled_numeric_value(tokens: &[String], start: usize) -> Option<NumericMention> {
    let mut index = start;
    let negative = tokens
        .get(index)
        .is_some_and(|word| matches!(word.as_str(), "minus" | "negative"));
    if negative {
        index += 1;
    }
    let mut total = 0_u64;
    let mut group = 0_u64;
    let mut last_kind = None;
    let mut last_scale = u64::MAX;
    let mut saw_number = false;
    while let Some(word) = tokens.get(index).map(String::as_str) {
        if word == "and"
            && saw_number
            && tokens
                .get(index + 1)
                .is_some_and(|next| small_cardinal(next).is_some() || tens_cardinal(next).is_some())
        {
            index += 1;
            continue;
        }
        if let Some(value) = small_cardinal(word) {
            if matches!(last_kind, Some(CardinalKind::Small))
                || matches!(last_kind, Some(CardinalKind::Tens)) && value > 9
            {
                break;
            }
            group = group.checked_add(value)?;
            last_kind = Some(CardinalKind::Small);
        } else if let Some(value) = tens_cardinal(word) {
            if matches!(last_kind, Some(CardinalKind::Small | CardinalKind::Tens)) {
                break;
            }
            group = group.checked_add(value)?;
            last_kind = Some(CardinalKind::Tens);
        } else if word == "hundred" {
            if !matches!(last_kind, Some(CardinalKind::Small)) || !(1..=9).contains(&group) {
                break;
            }
            group = group.checked_mul(100)?;
            last_kind = Some(CardinalKind::Hundred);
        } else if let Some(scale) = cardinal_scale(word) {
            if !saw_number || group == 0 || scale >= last_scale {
                break;
            }
            total = total.checked_add(group.checked_mul(scale)?)?;
            group = 0;
            last_scale = scale;
            last_kind = Some(CardinalKind::Scale);
        } else {
            break;
        }
        saw_number = true;
        index += 1;
    }
    if !saw_number || index == start + usize::from(negative) {
        return None;
    }
    let value = total.checked_add(group)?;
    Some(NumericMention {
        value: if negative && value != 0 {
            format!("-{value}")
        } else {
            value.to_string()
        },
        start,
        end: index,
    })
}

fn numeric_mentions(tokens: &[String]) -> Vec<NumericMention> {
    let mut mentions = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if let Some(value) = numeric_value(&tokens[index]) {
            mentions.push(NumericMention {
                value,
                start: index,
                end: index + 1,
            });
            index += 1;
        } else if let Some(mention) = spelled_numeric_value(tokens, index) {
            index = mention.end;
            mentions.push(mention);
        } else {
            index += 1;
        }
    }
    mentions
}

fn numeric_relation(words: &[String], start: usize, end: usize) -> Option<NumericRelation> {
    let before = &words[..start];
    let after = &words[end..];
    if before.last().is_some_and(|word| word == "__lt") {
        return Some(NumericRelation::LessThan);
    }
    if before.last().is_some_and(|word| word == "__le") {
        return Some(NumericRelation::AtMost);
    }
    if before.last().is_some_and(|word| word == "__gt") {
        return Some(NumericRelation::GreaterThan);
    }
    if before.last().is_some_and(|word| word == "__ge") {
        return Some(NumericRelation::AtLeast);
    }
    if ends_with_words(before, &["no", "more", "than"])
        || ends_with_words(before, &["not", "more", "than"])
        || ends_with_words(before, &["at", "most"])
        || ends_with_words(before, &["up", "to"])
        || before.last().is_some_and(|word| word == "maximum")
        || ends_with_words(before, &["maximum", "of"])
        || after.starts_with(&["or".to_string(), "fewer".to_string()])
        || after.starts_with(&["or".to_string(), "less".to_string()])
    {
        return Some(NumericRelation::AtMost);
    }
    if ends_with_words(before, &["no", "less", "than"])
        || ends_with_words(before, &["not", "less", "than"])
        || ends_with_words(before, &["at", "least"])
        || before.last().is_some_and(|word| word == "minimum")
        || ends_with_words(before, &["minimum", "of"])
        || after.starts_with(&["or".to_string(), "more".to_string()])
        || after.starts_with(&["or".to_string(), "greater".to_string()])
    {
        return Some(NumericRelation::AtLeast);
    }
    if ends_with_words(before, &["more", "than"]) || ends_with_words(before, &["greater", "than"]) {
        let negated = before[..before.len().saturating_sub(2)]
            .iter()
            .rev()
            .take(3)
            .any(|word| matches!(word.as_str(), "no" | "not" | "never"));
        return Some(if negated {
            NumericRelation::AtMost
        } else {
            NumericRelation::GreaterThan
        });
    }
    if ends_with_words(before, &["less", "than"]) || ends_with_words(before, &["fewer", "than"]) {
        let negated = before[..before.len().saturating_sub(2)]
            .iter()
            .rev()
            .take(3)
            .any(|word| matches!(word.as_str(), "no" | "not" | "never"));
        return Some(if negated {
            NumericRelation::AtLeast
        } else {
            NumericRelation::LessThan
        });
    }
    if before
        .last()
        .is_some_and(|word| matches!(word.as_str(), "under" | "below"))
    {
        return Some(NumericRelation::LessThan);
    }
    if before
        .last()
        .is_some_and(|word| matches!(word.as_str(), "over" | "above"))
    {
        return Some(NumericRelation::GreaterThan);
    }
    if before.last().is_some_and(|word| word == "exactly") {
        return Some(NumericRelation::Equal);
    }
    None
}

fn semantic_clauses(text: &str) -> impl Iterator<Item = &str> {
    text.split(['.', '?', '!', ';', '\n', '\r'])
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
}

fn comparison_clauses(text: &str) -> Vec<Vec<String>> {
    let characters = text.chars().collect::<Vec<_>>();
    let mut clauses = Vec::new();
    let mut clause = Vec::new();
    let mut token = String::new();
    let flush_token = |token: &mut String, clause: &mut Vec<String>| {
        if !token.is_empty() {
            clause.push(std::mem::take(token));
        }
    };
    for (index, character) in characters.iter().copied().enumerate() {
        if character.is_alphanumeric() {
            token.extend(character.to_lowercase());
            continue;
        }
        let next_is_digit = characters
            .get(index + 1)
            .is_some_and(|value| value.is_ascii_digit());
        if matches!(character, '+' | '-' | '−') && token.is_empty() && next_is_digit {
            token.push(if character == '+' { '+' } else { '-' });
            continue;
        }
        let unsigned_token = token.strip_prefix(['+', '-']).unwrap_or(token.as_str());
        let inside_number = unsigned_token.chars().all(|value| value.is_ascii_digit())
            && !unsigned_token.is_empty()
            && characters
                .get(index + 1)
                .is_some_and(|value| value.is_ascii_digit());
        if character == ',' && inside_number {
            continue;
        }
        if character == '.' && inside_number {
            token.push('.');
            continue;
        }
        flush_token(&mut token, &mut clause);
        if matches!(character, '<' | '>' | '≤' | '≥') {
            let followed_by_equals = characters.get(index + 1).is_some_and(|value| *value == '=');
            clause.push(
                match (character, followed_by_equals) {
                    ('<', false) => "__lt",
                    ('<', true) | ('≤', _) => "__le",
                    ('>', false) => "__gt",
                    ('>', true) | ('≥', _) => "__ge",
                    _ => unreachable!("comparison operator is exhaustively matched"),
                }
                .into(),
            );
            continue;
        }
        if matches!(character, '.' | '?' | '!' | ';' | '\n' | '\r') && !clause.is_empty() {
            clauses.push(std::mem::take(&mut clause));
        }
    }
    flush_token(&mut token, &mut clause);
    if !clause.is_empty() {
        clauses.push(clause);
    }
    clauses
}

fn numeric_context(tokens: &[String], index: usize) -> Vec<String> {
    const COMPARISON_WORDS: &[&str] = &[
        "no", "not", "more", "less", "fewer", "greater", "than", "at", "most", "least", "up", "to",
        "maximum", "minimum", "of", "exactly", "under", "below", "over", "above",
    ];
    let mut end = index;
    while end > 0 && COMPARISON_WORDS.contains(&tokens[end - 1].as_str()) {
        end -= 1;
    }
    tokens[end.saturating_sub(8)..end].to_vec()
}

fn numeric_trailing_subject(tokens: &[String], end: usize) -> Vec<String> {
    let trailing = &tokens[end..];
    let Some(by) = trailing.iter().position(|word| word == "by") else {
        return Vec::new();
    };
    trailing[by + 1..]
        .iter()
        .take_while(|word| {
            !matches!(
                word.as_str(),
                "and" | "or" | "when" | "if" | "unless" | "under" | "because" | "while"
            )
        })
        .take(4)
        .cloned()
        .collect()
}

fn numeric_constraints(text: &str) -> Vec<NumericConstraint> {
    comparison_clauses(text)
        .into_iter()
        .flat_map(|tokens| {
            numeric_mentions(&tokens)
                .into_iter()
                .filter_map(|mention| {
                    numeric_relation(&tokens, mention.start, mention.end).map(|relation| {
                        NumericConstraint {
                            value: mention.value,
                            relation,
                            context: numeric_context(&tokens, mention.start),
                            trailing_subject: numeric_trailing_subject(&tokens, mention.end),
                        }
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn numeric_mention_contexts(text: &str) -> Vec<NumericReference> {
    comparison_clauses(text)
        .into_iter()
        .flat_map(|tokens| {
            numeric_mentions(&tokens)
                .into_iter()
                .map(|mention| NumericReference {
                    context: numeric_context(&tokens, mention.start),
                    trailing_subject: numeric_trailing_subject(&tokens, mention.end),
                })
                .collect::<Vec<_>>()
        })
        .filter(|reference| !reference.context.is_empty() || !reference.trailing_subject.is_empty())
        .collect()
}

fn contains_sequence(haystack: &[String], needle: &[String]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn numeric_contexts_match(source: &NumericReference, claim: &NumericReference) -> bool {
    (!source.context.is_empty() && source.context == claim.context)
        || (!source.trailing_subject.is_empty()
            && (source.trailing_subject == claim.trailing_subject
                || contains_sequence(&claim.context, &source.trailing_subject)))
        || (!claim.trailing_subject.is_empty()
            && contains_sequence(&source.context, &claim.trailing_subject))
}

fn constraint_reference(constraint: &NumericConstraint) -> NumericReference {
    NumericReference {
        context: constraint.context.clone(),
        trailing_subject: constraint.trailing_subject.clone(),
    }
}

fn comparison_boundaries_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let source_constraints = evidence
        .iter()
        .flat_map(|item| numeric_constraints(&item.exact_quote))
        .collect::<Vec<_>>();
    let source_contexts = evidence
        .iter()
        .flat_map(|item| numeric_mention_contexts(&item.exact_quote))
        .collect::<Vec<_>>();
    numeric_constraints(claim).into_iter().all(|constraint| {
        let claim_reference = constraint_reference(&constraint);
        let matching_relation = source_constraints
            .iter()
            .filter(|source| {
                source.value == constraint.value && source.relation == constraint.relation
            })
            .collect::<Vec<_>>();
        if matching_relation
            .iter()
            .any(|source| numeric_contexts_match(&constraint_reference(source), &claim_reference))
        {
            return true;
        }
        if matching_relation.is_empty()
            && source_constraints
                .iter()
                .any(|source| source.value == constraint.value)
        {
            return false;
        }
        (constraint.context.is_empty() && constraint.trailing_subject.is_empty())
            || !source_contexts
                .iter()
                .any(|source| numeric_contexts_match(source, &claim_reference))
    })
}

fn contains_words(text: &str, phrase: &[&str]) -> bool {
    let tokens = words(text);
    tokens
        .windows(phrase.len())
        .any(|window| window.iter().map(String::as_str).eq(phrase.iter().copied()))
}

fn broader_enumeration_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    for phrase in [&["family", "member"][..], &["family", "members"][..]] {
        if contains_words(claim, phrase)
            && !evidence
                .iter()
                .any(|item| contains_words(&item.exact_quote, phrase))
        {
            return false;
        }
    }
    let kinship_relative = |text: &str| {
        let tokens = words(text);
        tokens.iter().enumerate().any(|(index, word)| {
            matches!(word.as_str(), "relative" | "relatives")
                && (tokens.get(index + 1).is_some_and(|next| next == "of")
                    || tokens.get(index.wrapping_sub(1)).is_some_and(|previous| {
                        matches!(
                            previous.as_str(),
                            "his" | "her" | "their" | "our" | "your" | "s"
                        )
                    }))
        })
    };
    if kinship_relative(claim)
        && !evidence
            .iter()
            .any(|item| kinship_relative(&item.exact_quote))
    {
        return false;
    }
    true
}

fn endpoint_word(word: &str) -> Option<String> {
    if matches!(
        word,
        "a" | "an" | "the" | "their" | "his" | "her" | "its" | "any"
    ) {
        return None;
    }
    Some(word.to_string())
}

fn directional_relations_in_clause(clause: &str) -> Vec<(Vec<String>, Vec<String>)> {
    let tokens = words(clause);
    let mut relations = Vec::new();
    for (from, token) in tokens.iter().enumerate() {
        if token != "from" {
            continue;
        }
        let Some(to) =
            (from + 1..(from + 13).min(tokens.len())).find(|index| tokens[*index] == "to")
        else {
            continue;
        };
        let origin = tokens[from + 1..to]
            .iter()
            .filter_map(|word| endpoint_word(word))
            .collect::<Vec<_>>();
        let destination = tokens[to + 1..]
            .iter()
            .take_while(|word| {
                !matches!(
                    word.as_str(),
                    "and" | "or" | "by" | "from" | "unless" | "absent" | "if" | "when"
                )
            })
            .take(8)
            .filter_map(|word| endpoint_word(word))
            .collect::<Vec<_>>();
        if !origin.is_empty() && !destination.is_empty() {
            relations.push((origin, destination));
        }
    }
    for (to, token) in tokens.iter().enumerate() {
        if token != "to" {
            continue;
        }
        let Some(from) =
            (to + 1..(to + 13).min(tokens.len())).find(|index| tokens[*index] == "from")
        else {
            continue;
        };
        let destination = tokens[to + 1..from]
            .iter()
            .filter_map(|word| endpoint_word(word))
            .collect::<Vec<_>>();
        let origin = tokens[from + 1..]
            .iter()
            .take_while(|word| {
                !matches!(
                    word.as_str(),
                    "and" | "or" | "by" | "to" | "unless" | "absent" | "if" | "when"
                )
            })
            .take(8)
            .filter_map(|word| endpoint_word(word))
            .collect::<Vec<_>>();
        if !origin.is_empty() && !destination.is_empty() {
            relations.push((origin, destination));
        }
    }
    relations
}

fn normalized_endpoint(words: &[String]) -> Vec<String> {
    let modifier_start = words.iter().position(|word| {
        matches!(
            word.as_str(),
            "each" | "every" | "daily" | "weekly" | "monthly" | "annually"
        )
    });
    let words = &words[..modifier_start.unwrap_or(words.len())];
    let mut normalized = Vec::new();
    let mut index = 0;
    while index < words.len() {
        let remaining = &words[index..];
        if remaining.starts_with(&["living".into(), "quarters".into()]) {
            normalized.push("housing".into());
            index += 2;
        } else if remaining.starts_with(&["work".into(), "site".into()]) {
            normalized.push("worksite".into());
            index += 2;
        } else if remaining.starts_with(&["place".into(), "of".into(), "employment".into()]) {
            normalized.push("worksite".into());
            index += 3;
        } else {
            normalized.push(match words[index].as_str() {
                "workplace" => "worksite".into(),
                other => other.to_string(),
            });
            index += 1;
        }
    }
    normalized
}

fn endpoints_match(left: &[String], right: &[String]) -> bool {
    normalized_endpoint(left) == normalized_endpoint(right)
}

fn relations_match(left: &(Vec<String>, Vec<String>), right: &(Vec<String>, Vec<String>)) -> bool {
    endpoints_match(&left.0, &right.0) && endpoints_match(&left.1, &right.1)
}

fn directional_relations(text: &str) -> Vec<(Vec<String>, Vec<String>)> {
    semantic_clauses(text)
        .flat_map(directional_relations_in_clause)
        .collect()
}

fn directional_endpoints_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let source_relations = evidence
        .iter()
        .flat_map(|item| directional_relations(&item.exact_quote))
        .collect::<Vec<_>>();
    directional_relations(claim).into_iter().all(|relation| {
        if source_relations
            .iter()
            .any(|source| relations_match(source, &relation))
        {
            return true;
        }
        let known_origin = source_relations.iter().any(|source| {
            endpoints_match(&source.0, &relation.0) || endpoints_match(&source.1, &relation.0)
        });
        let known_destination = source_relations.iter().any(|source| {
            endpoints_match(&source.0, &relation.1) || endpoints_match(&source.1, &relation.1)
        });
        !(known_origin && known_destination)
    })
}

fn acronym(word: &str) -> Option<String> {
    let word = word.strip_suffix('s').unwrap_or(word);
    ((2..=10).contains(&word.len()) && word.chars().all(|character| character.is_ascii_uppercase()))
        .then(|| word.to_string())
}

fn defined_actor_labels(text: &str) -> HashMap<String, Vec<String>> {
    let mut labels = HashMap::new();
    for (open, _) in text.match_indices('(') {
        let Some(relative_close) = text[open + 1..].find(')') else {
            continue;
        };
        let close = open + 1 + relative_close;
        let Some(key) = acronym(text[open + 1..close].trim()) else {
            continue;
        };
        let before = &text[..open];
        let punctuation_start = before
            .rfind(['.', ';', ':', ',', '\n', '\r'])
            .map_or(0, |index| index + 1);
        let conjunction_start = before.to_ascii_lowercase()[punctuation_start..]
            .rfind(" and ")
            .map_or(punctuation_start, |index| punctuation_start + index + 5);
        let mut label = words(&before[conjunction_start..]);
        if label.len() > 6 {
            label.drain(..label.len() - 6);
        }
        if !label.is_empty()
            && !label.iter().any(|word| {
                matches!(
                    word.as_str(),
                    "act" | "law" | "program" | "standard" | "regulation"
                )
            })
        {
            labels.insert(key, label);
        }
    }
    labels
}

fn contains_word_sequence(words: &[&str], phrase: &[String]) -> bool {
    words.windows(phrase.len()).any(|window| {
        window
            .iter()
            .map(|word| word.to_ascii_lowercase())
            .eq(phrase.iter().cloned())
    })
}

fn contains_owned_words(text: &str, phrase: &[String]) -> bool {
    let tokens = words(text);
    tokens.windows(phrase.len()).any(|window| window == phrase)
}

fn contains_any_word(text: &str, alternatives: &[&str]) -> bool {
    let tokens = words(text);
    tokens
        .iter()
        .any(|word| alternatives.contains(&word.as_str()))
}

fn tokens_contain_any(tokens: &[String], alternatives: &[&str]) -> bool {
    tokens
        .iter()
        .any(|word| alternatives.contains(&word.as_str()))
}

fn actor_mention_end(tokens: &[String], actor: &str, label: &[String]) -> Option<usize> {
    let actor = actor.to_ascii_lowercase();
    let acronym_end = tokens
        .iter()
        .enumerate()
        .filter(|(_, word)| {
            *word == &actor
                || word
                    .strip_suffix('s')
                    .is_some_and(|word| word == actor.as_str())
        })
        .map(|(index, _)| index + 1)
        .max();
    let label_end = tokens
        .windows(label.len())
        .enumerate()
        .filter(|(_, window)| *window == label)
        .map(|(index, _)| index + label.len())
        .max();
    acronym_end.into_iter().chain(label_end).max()
}

fn tokens_mention_actor(tokens: &[String], actor: &str, label: &[String]) -> bool {
    actor_mention_end(tokens, actor, label).is_some()
}

fn actor_relation_segments(
    evidence: &[&EvidenceItem],
    actors: &[(&str, &[String])],
    concept: &[&str],
) -> Vec<Vec<String>> {
    evidence
        .iter()
        .flat_map(|item| semantic_clauses(&item.exact_quote))
        .flat_map(|clause| {
            let tokens = words(clause);
            let mut segments = Vec::new();
            let mut start = 0;
            for index in 0..tokens.len() {
                let contrast = matches!(
                    tokens[index].as_str(),
                    "while" | "whereas" | "but" | "although"
                );
                let left = &tokens[start..index];
                let right_mentions_actor = actors
                    .iter()
                    .any(|(actor, label)| tokens_mention_actor(&tokens[index + 1..], actor, label));
                let completed_coordination = matches!(tokens[index].as_str(), "and" | "or")
                    && right_mentions_actor
                    && (tokens_contain_any(left, concept)
                        || actors
                            .iter()
                            .filter_map(|(actor, label)| actor_mention_end(left, actor, label))
                            .max()
                            .is_some_and(|actor_end| actor_end < left.len()));
                if (contrast || completed_coordination) && start < index {
                    segments.push(tokens[start..index].to_vec());
                    start = index + 1;
                }
            }
            if start < tokens.len() {
                segments.push(tokens[start..].to_vec());
            }
            segments
        })
        .collect()
}

fn qualifier_polarities(tokens: &[String], concept: &[&str]) -> HashSet<bool> {
    tokens
        .iter()
        .enumerate()
        .filter(|(_, word)| concept.contains(&word.as_str()))
        .map(|(index, _)| {
            tokens[index.saturating_sub(6)..index]
                .iter()
                .enumerate()
                .any(|(relative, word)| {
                    let absolute = index.saturating_sub(6) + relative;
                    let additive_not = word == "not"
                        && tokens.get(absolute + 1).is_some_and(|next| next == "only");
                    !additive_not
                        && matches!(
                            word.as_str(),
                            "not" | "no" | "never" | "without" | "isn" | "aren" | "wasn" | "weren"
                        )
                })
        })
        .collect()
}

fn actor_qualification_clause_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let claim_words = claim
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let Some(condition_index) = claim_words.iter().position(|word| {
        matches!(
            word.to_ascii_lowercase().as_str(),
            "if" | "when" | "unless" | "provided"
        )
    }) else {
        return true;
    };
    let leading_condition = condition_index == 0;
    let (subject_words, condition) = if leading_condition {
        let Some(comma) = claim.find(',') else {
            return true;
        };
        (
            claim[comma + 1..]
                .split(|character: char| !character.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>(),
            claim[..comma].to_string(),
        )
    } else {
        (
            claim_words[..condition_index].to_vec(),
            claim_words[condition_index..].join(" "),
        )
    };
    let subject_end = subject_words
        .iter()
        .position(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "subject"
                    | "is"
                    | "are"
                    | "was"
                    | "were"
                    | "must"
                    | "shall"
                    | "may"
                    | "can"
                    | "will"
                    | "required"
                    | "requires"
            )
        })
        .unwrap_or(subject_words.len());
    let subject = &subject_words[..subject_end];
    let subject_acronyms = subject
        .iter()
        .filter_map(|word| acronym(word))
        .collect::<HashSet<_>>();
    let defined_labels = evidence
        .iter()
        .flat_map(|item| defined_actor_labels(&item.exact_quote))
        .collect::<HashMap<_, _>>();
    let source_actors = defined_labels
        .iter()
        .map(|(actor, label)| (actor.as_str(), label.as_slice()))
        .collect::<Vec<_>>();
    let claimed_actors = defined_labels
        .iter()
        .filter(|(actor, label)| {
            subject_acronyms.contains(*actor) || contains_word_sequence(subject, label)
        })
        .map(|(actor, label)| (actor.as_str(), label.as_slice()))
        .collect::<Vec<_>>();
    if claimed_actors.is_empty() {
        return true;
    }
    const QUALIFIER_CONCEPTS: &[&[&str]] = &[
        &[
            "compensation",
            "compensated",
            "consideration",
            "money",
            "paid",
            "payment",
            "fee",
        ],
        &["only", "solely", "exclusively"],
        &["unless", "except", "excluding", "absent", "without"],
    ];
    let condition_tokens = words(&condition);
    QUALIFIER_CONCEPTS.iter().all(|concept| {
        let claim_polarities = qualifier_polarities(&condition_tokens, concept);
        if claim_polarities.is_empty() {
            return true;
        }
        let source_relations = actor_relation_segments(evidence, &source_actors, concept);
        let supports_actor = |actor: &str, label: &[String]| {
            source_relations.iter().any(|relation| {
                tokens_mention_actor(relation, actor, label)
                    && tokens_contain_any(relation, concept)
                    && !qualifier_polarities(relation, concept).is_disjoint(&claim_polarities)
            })
        };
        let source_has_actor_qualifier = source_relations.iter().any(|relation| {
            tokens_contain_any(relation, concept)
                && source_actors
                    .iter()
                    .any(|(actor, label)| tokens_mention_actor(relation, actor, label))
        });
        !source_has_actor_qualifier
            || claimed_actors
                .iter()
                .all(|(actor, label)| supports_actor(actor, label))
    })
}

fn actor_qualifications_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    semantic_clauses(claim).all(|clause| actor_qualification_clause_supported(clause, evidence))
}

fn original_words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EvaluationRelation {
    subject: Vec<String>,
    negated: bool,
}

const EVALUATIVE_CONCEPTS: &[&[&str]] = &[
    &["essential", "critical", "vital", "necessary"],
    &["ensure", "ensures", "ensuring", "guarantee", "guarantees"],
    &["important", "importance"],
    &["effective", "effectiveness"],
    &["safe", "safety"],
    &["healthy", "health"],
];

fn is_evaluative_word(word: &str) -> bool {
    EVALUATIVE_CONCEPTS
        .iter()
        .any(|concept| concept.contains(&word))
}

fn is_evaluation_link(word: &str) -> bool {
    matches!(
        word,
        "is" | "are"
            | "was"
            | "were"
            | "be"
            | "been"
            | "being"
            | "seems"
            | "seemed"
            | "remains"
            | "remained"
            | "isn"
            | "aren"
            | "wasn"
            | "weren"
    )
}

fn evaluation_relations(clause: &str, concept: &[&str]) -> Vec<EvaluationRelation> {
    let tokens = original_words(clause)
        .into_iter()
        .map(|word| word.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let concept_indices = tokens
        .iter()
        .enumerate()
        .filter(|(_, word)| concept.contains(&word.as_str()))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    concept_indices
        .into_iter()
        .filter_map(|concept_index| {
            let linking_index = tokens[..concept_index]
                .iter()
                .rposition(|word| is_evaluation_link(word))?;
            let previous_evaluation = tokens[..linking_index]
                .iter()
                .rposition(|word| is_evaluative_word(word));
            let subject_start = previous_evaluation
                .and_then(|previous| {
                    tokens[previous + 1..linking_index]
                        .iter()
                        .rposition(|word| {
                            matches!(word.as_str(), "and" | "or" | "but" | "while" | "whereas")
                        })
                        .map(|relative| previous + relative + 2)
                })
                .unwrap_or(0);
            let mut subject = tokens[subject_start..linking_index].to_vec();
            if subject.first().is_some_and(|word| {
                matches!(
                    word.as_str(),
                    "a" | "an" | "the" | "this" | "these" | "that" | "those"
                )
            }) {
                subject.remove(0);
            }
            let subject_is_concrete = !subject.is_empty()
                && !matches!(
                    subject.as_slice(),
                    [word] if matches!(word.as_str(), "it" | "they" | "he" | "she")
                );
            subject_is_concrete.then(|| EvaluationRelation {
                subject,
                negated: tokens[linking_index..=concept_index]
                    .iter()
                    .enumerate()
                    .any(|(relative, word)| {
                        let additive_not = word == "not"
                            && tokens
                                .get(linking_index + relative + 1)
                                .is_some_and(|next| next == "only");
                        !additive_not
                            && matches!(
                                word.as_str(),
                                "not" | "no" | "never" | "t" | "isn" | "aren" | "wasn" | "weren"
                            )
                    }),
            })
        })
        .collect()
}

fn evaluated_subject_matches(source: &[String], claim: &[String]) -> bool {
    source == claim || source.windows(claim.len()).any(|window| window == claim)
}

fn evaluative_conclusions_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let source_clauses = evidence
        .iter()
        .flat_map(|item| semantic_clauses(&item.exact_quote))
        .collect::<Vec<_>>();
    for concept in EVALUATIVE_CONCEPTS {
        let claim_clauses = semantic_clauses(claim)
            .filter(|clause| contains_any_word(clause, concept))
            .collect::<Vec<_>>();
        if claim_clauses.is_empty() {
            continue;
        }
        let evaluated_source_clauses = source_clauses
            .iter()
            .copied()
            .filter(|clause| contains_any_word(clause, concept))
            .collect::<Vec<_>>();
        if evaluated_source_clauses.is_empty() {
            return false;
        }
        let evaluated_relations = evaluated_source_clauses
            .iter()
            .flat_map(|clause| evaluation_relations(clause, concept))
            .collect::<Vec<_>>();
        for clause in claim_clauses {
            for claim_relation in evaluation_relations(clause, concept) {
                let subject_is_cited = evidence
                    .iter()
                    .any(|item| contains_owned_words(&item.exact_quote, &claim_relation.subject));
                if subject_is_cited
                    && !evaluated_relations.is_empty()
                    && !evaluated_relations.iter().any(|source| {
                        evaluated_subject_matches(&source.subject, &claim_relation.subject)
                            && source.negated == claim_relation.negated
                    })
                {
                    return false;
                }
            }
        }
    }
    true
}

fn modal_force_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let weak_source = evidence
        .iter()
        .flat_map(|item| modal_predicates(&item.exact_quote, false))
        .collect::<Vec<_>>();
    let strong_source = evidence
        .iter()
        .flat_map(|item| modal_predicates(&item.exact_quote, true))
        .collect::<Vec<_>>();
    modal_predicates(claim, true).into_iter().all(|predicate| {
        !weak_source
            .iter()
            .any(|source| source.predicate == predicate.predicate)
            || strong_source.iter().any(|source| {
                source.predicate == predicate.predicate && source.negated == predicate.negated
            })
    })
}

fn semantic_fidelity_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    comparison_boundaries_supported(claim, evidence)
        && broader_enumeration_supported(claim, evidence)
        && directional_endpoints_supported(claim, evidence)
        && actor_qualifications_supported(claim, evidence)
        && evaluative_conclusions_supported(claim, evidence)
        && modal_force_supported(claim, evidence)
}

pub(in crate::pipeline::summary) fn apply_semantic_fidelity_guards(
    claims: &[CitedClaim],
    evidence: &[EvidenceItem],
    verifications: &mut [ClaimVerification],
) -> Result<(), PipelineFailure> {
    if claims.len() != verifications.len() {
        return Err(stage_failure(
            PipelineStage::Verify,
            "INVALID_VERIFICATION_RESPONSE",
            "Semantic-fidelity verification coverage must match the coherent claim catalog",
            false,
        ));
    }
    let evidence = evidence
        .iter()
        .map(|item| (item.evidence_id.as_str(), item))
        .collect::<HashMap<_, _>>();
    for (claim, verification) in claims.iter().zip(verifications) {
        if verification.claim_id != claim.claim_id
            || verification.evidence_ids != claim.evidence_ids
        {
            return Err(stage_failure(
                PipelineStage::Verify,
                "INVALID_VERIFICATION_RESPONSE",
                "Semantic-fidelity verification identity must match the coherent claim catalog",
                false,
            ));
        }
        if verification.verdict != ClaimVerdict::Supported {
            continue;
        }
        let cited = claim
            .evidence_ids
            .iter()
            .map(|evidence_id| evidence.get(evidence_id.as_str()).copied())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| {
                stage_failure(
                    PipelineStage::Verify,
                    "INVALID_SYNTHESIZED_DOCUMENT",
                    "Semantic-fidelity verification references unknown synthesis evidence",
                    false,
                )
            })?;
        if !semantic_fidelity_supported(&claim.text, &cited) {
            verification.verdict = ClaimVerdict::Unsupported;
        }
    }
    Ok(())
}
