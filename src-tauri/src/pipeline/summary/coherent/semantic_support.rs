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

fn numeric_relation(words: &[String], index: usize) -> Option<NumericRelation> {
    let before = &words[..index];
    let after = &words[index + 1..];
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

fn numeric_constraints(text: &str) -> Vec<NumericConstraint> {
    comparison_clauses(text)
        .into_iter()
        .flat_map(|tokens| {
            tokens
                .iter()
                .enumerate()
                .filter_map(|(index, word)| {
                    match (numeric_value(word), numeric_relation(&tokens, index)) {
                        (Some(value), Some(relation)) => Some(NumericConstraint {
                            value,
                            relation,
                            context: numeric_context(&tokens, index),
                        }),
                        _ => None,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn numeric_mention_contexts(text: &str) -> Vec<Vec<String>> {
    comparison_clauses(text)
        .into_iter()
        .flat_map(|tokens| {
            tokens
                .iter()
                .enumerate()
                .filter(|(_, word)| numeric_value(word).is_some())
                .map(|(index, _)| numeric_context(&tokens, index))
                .collect::<Vec<_>>()
        })
        .filter(|context| !context.is_empty())
        .collect()
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
        let matching_relation = source_constraints
            .iter()
            .filter(|source| {
                source.value == constraint.value && source.relation == constraint.relation
            })
            .collect::<Vec<_>>();
        if matching_relation
            .iter()
            .any(|source| source.context == constraint.context)
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
        constraint.context.is_empty()
            || !source_contexts
                .iter()
                .any(|source| source == &constraint.context)
    })
}

fn contains_words(text: &str, phrase: &[&str]) -> bool {
    let tokens = words(text);
    tokens
        .windows(phrase.len())
        .any(|window| window.iter().map(String::as_str).eq(phrase.iter().copied()))
}

fn broader_enumeration_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    for phrase in [
        &["family", "member"][..],
        &["family", "members"][..],
        &["relative"][..],
        &["relatives"][..],
    ] {
        if contains_words(claim, phrase)
            && !evidence
                .iter()
                .any(|item| contains_words(&item.exact_quote, phrase))
        {
            return false;
        }
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
    let condition = claim_words[condition_index..].join(" ");
    let subject_end = claim_words[..condition_index]
        .iter()
        .position(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "subject" | "must" | "shall" | "may" | "can" | "will" | "required" | "requires"
            )
        })
        .unwrap_or(condition_index);
    let subject = &claim_words[..subject_end];
    let subject_acronyms = subject
        .iter()
        .filter_map(|word| acronym(word))
        .collect::<HashSet<_>>();
    let defined_labels = evidence
        .iter()
        .flat_map(|item| defined_actor_labels(&item.exact_quote))
        .collect::<HashMap<_, _>>();
    let actors = defined_labels
        .iter()
        .filter(|(actor, label)| {
            subject_acronyms.contains(*actor) || contains_word_sequence(subject, label)
        })
        .map(|(actor, label)| (actor.as_str(), label.as_slice()))
        .collect::<Vec<_>>();
    if actors.len() < 2 {
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
    QUALIFIER_CONCEPTS.iter().all(|concept| {
        if !contains_any_word(&condition, concept) {
            return true;
        }
        let source_relations = actor_relation_segments(evidence, &actors, concept);
        let supported = actors
            .iter()
            .map(|(actor, label)| {
                source_relations.iter().any(|relation| {
                    tokens_mention_actor(relation, actor, label)
                        && tokens_contain_any(relation, concept)
                })
            })
            .collect::<Vec<_>>();
        !supported.iter().any(|value| *value) || supported.iter().all(|value| *value)
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

fn evaluation_relation(clause: &str, concept: &[&str]) -> Option<EvaluationRelation> {
    let tokens = original_words(clause)
        .into_iter()
        .map(|word| word.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let concept_index = tokens
        .iter()
        .position(|word| concept.contains(&word.as_str()))?;
    let linking_index = tokens[..concept_index].iter().rposition(|word| {
        matches!(
            word.as_str(),
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
    })?;
    let mut subject = tokens[..linking_index].to_vec();
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
        negated: tokens[linking_index..=concept_index].iter().any(|word| {
            matches!(
                word.as_str(),
                "not" | "no" | "never" | "t" | "isn" | "aren" | "wasn" | "weren"
            )
        }),
    })
}

fn evaluated_subject_matches(source: &[String], claim: &[String]) -> bool {
    source == claim || source.windows(claim.len()).any(|window| window == claim)
}

fn evaluative_conclusions_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    const EVALUATIVE_CONCEPTS: &[&[&str]] = &[
        &["essential", "critical", "vital", "necessary"],
        &["ensure", "ensures", "ensuring", "guarantee", "guarantees"],
        &["important", "importance"],
        &["effective", "effectiveness"],
        &["safe", "safety"],
        &["healthy", "health"],
    ];
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
            .filter_map(|clause| evaluation_relation(clause, concept))
            .collect::<Vec<_>>();
        for clause in claim_clauses {
            let Some(claim_relation) = evaluation_relation(clause, concept) else {
                continue;
            };
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
