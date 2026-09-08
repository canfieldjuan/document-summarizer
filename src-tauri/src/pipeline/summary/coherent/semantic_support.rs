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

fn ends_with_words(words: &[String], suffix: &[&str]) -> bool {
    words.len() >= suffix.len()
        && words[words.len() - suffix.len()..]
            .iter()
            .map(String::as_str)
            .eq(suffix.iter().copied())
}

fn numeric_value(word: &str) -> Option<String> {
    word.chars()
        .all(|character| character.is_ascii_digit())
        .then(|| word.to_string())
        .filter(|value| !value.is_empty())
}

fn numeric_relation(words: &[String], index: usize) -> Option<NumericRelation> {
    let before = &words[..index];
    let after = &words[index + 1..];
    if ends_with_words(before, &["no", "more", "than"])
        || ends_with_words(before, &["not", "more", "than"])
        || ends_with_words(before, &["at", "most"])
        || ends_with_words(before, &["up", "to"])
        || before.last().is_some_and(|word| word == "maximum")
        || after.starts_with(&["or".to_string(), "fewer".to_string()])
        || after.starts_with(&["or".to_string(), "less".to_string()])
    {
        return Some(NumericRelation::AtMost);
    }
    if ends_with_words(before, &["no", "less", "than"])
        || ends_with_words(before, &["not", "less", "than"])
        || ends_with_words(before, &["at", "least"])
        || before.last().is_some_and(|word| word == "minimum")
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

fn numeric_constraints(text: &str) -> Vec<(String, NumericRelation)> {
    semantic_clauses(text)
        .flat_map(|clause| {
            let tokens = words(clause);
            tokens
                .iter()
                .enumerate()
                .filter_map(|(index, word)| {
                    numeric_value(word).zip(numeric_relation(&tokens, index))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn comparison_boundaries_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let source_constraints = evidence
        .iter()
        .flat_map(|item| numeric_constraints(&item.exact_quote))
        .collect::<Vec<_>>();
    numeric_constraints(claim).into_iter().all(|constraint| {
        source_constraints
            .iter()
            .any(|source| source == &constraint)
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
    directional_relations(claim)
        .into_iter()
        .all(|relation| source_relations.iter().any(|source| source == &relation))
}

fn acronym(word: &str) -> Option<String> {
    let word = word.strip_suffix('s').unwrap_or(word);
    ((2..=10).contains(&word.len()) && word.chars().all(|character| character.is_ascii_uppercase()))
        .then(|| word.to_string())
}

fn acronym_set(text: &str) -> HashSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter_map(acronym)
        .collect()
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
    let evidence_acronyms = evidence
        .iter()
        .map(|item| acronym_set(&item.exact_quote))
        .collect::<Vec<_>>();
    let mut actors = subject
        .iter()
        .filter_map(|word| acronym(word))
        .collect::<HashSet<_>>();
    actors.retain(|actor| {
        !evidence_acronyms
            .iter()
            .all(|source| source.contains(actor))
    });
    let defined_labels = evidence
        .iter()
        .flat_map(|item| defined_actor_labels(&item.exact_quote))
        .collect::<HashMap<_, _>>();
    actors.extend(
        defined_labels
            .iter()
            .filter(|(_, label)| contains_word_sequence(subject, label))
            .map(|(actor, _)| actor.clone()),
    );
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
        !contains_any_word(&condition, concept)
            || actors.iter().all(|actor| {
                evidence
                    .iter()
                    .filter(|item| {
                        acronym_set(&item.exact_quote).contains(actor)
                            || defined_labels
                                .get(actor)
                                .is_some_and(|label| contains_owned_words(&item.exact_quote, label))
                    })
                    .any(|item| contains_any_word(&item.exact_quote, concept))
            })
    })
}

fn actor_qualifications_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    semantic_clauses(claim).all(|clause| actor_qualification_clause_supported(clause, evidence))
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
    EVALUATIVE_CONCEPTS.iter().all(|concept| {
        !contains_any_word(claim, concept)
            || evidence
                .iter()
                .any(|item| contains_any_word(&item.exact_quote, concept))
    })
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
