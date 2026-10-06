//! Bounded, source-owned C9 comparisons for General prose verification.
use super::*;
use std::collections::BTreeSet;

pub(super) const SCHEMA_NAME: &str = crate::pipeline::contracts::CLAIM_COMPARISON_SCHEMA_NAME;
const MAX_INPUT_CHARACTERS: usize = 4096;
const MAX_SPAN_CHARACTERS: usize = 240;
const MAX_SPANS: usize = 4;
const DIMENSIONS: [&str; 4] = ["stage", "conditions", "qualifiers", "scope"];
const INSTRUCTION: &str = r#"Compare the claim with its cited quotation interpreted in the complete governing clause.
Return exactly one JSON object with verdicts containing one item with claim_id and comparisons.
The comparisons object must contain stage, conditions, qualifiers, and scope. Do not choose a subset.
For each dimension choose source_spans from source_segments and claim_spans from claim_segments. Each selection must be one complete supplied segment string, exactly. Do not shorten, join or paraphrase segments. Interpret each selected piece in the complete governing clause, including restrictions in neighboring pieces. Then assign its relation.
Stage means the event or lifecycle point to which the claim assigns an action or consequence.
Conditions means prerequisites, exceptions or contingencies governing the proposition the claim asserts.
Qualifiers means negation, modality, time-unit or other restrictions on the asserted relationship.
Scope means the actors, objects or classes included or excluded by that proposition, including an explicitly included class omitted by a narrower summary.
Assess only what governs the proposition asserted by the claim. A summary need not repeat distinct, unrelated facts from the surrounding clause. Equivalent faithful wording is preserved.
Use preserved when the claim retains the applicable meaning; changed when it assigns a different meaning; omitted when it drops an applicable restriction or included scope; uncertain when the relationship cannot be established; not_applicable only when that dimension does not apply to this proposition.
Preserved and changed require spans on both sides. Omitted requires source spans and may have no claim span. Uncertain requires at least one side. Not_applicable requires both span lists empty. An empty list is not a substitute for inspecting that dimension.
The application derives the overall verdict from these comparisons. Do not emit an overall verdict, extra fields, reasoning text or prose."#;

pub(super) fn applies(profile: SummaryProfile, synthesized: &SynthesizedDocument) -> bool {
    profile == SummaryProfile::General
        && synthesized.presentation_mode == SummaryPresentationMode::Coherent
        && coherent_synthesis_uses_clause_verification(&synthesized.synthesis_version)
}

fn too_large() -> PipelineFailure {
    stage_failure(
        PipelineStage::Verify,
        "VERIFICATION_INPUT_TOO_LARGE",
        "A complete claim and its governing source exceed bounded comparison verification",
        false,
    )
}

fn invalid_input() -> PipelineFailure {
    stage_failure(
        PipelineStage::Verify,
        "MODEL_REQUEST_INVALID",
        "Comparison verification requires a claim and its owned source quotations",
        false,
    )
}

fn invalid_response() -> PipelineFailure {
    stage_failure(
        PipelineStage::Verify,
        "MODEL_VERIFICATION_RESPONSE_INVALID",
        "Verification comparisons must contain exact owned passages and complete valid relations",
        true,
    )
}

// Python C9 uses Unicode \w+|[^\w\s]: letters, numbers and underscore form
// words; combining marks and punctuation are individual tokens. Keep source
// byte boundaries as well as Unicode scalar counts; never normalize text.
#[cfg(test)]
fn word_character(c: char) -> bool {
    c == '_'
        || matches!(
            c.general_category(),
            GeneralCategory::UppercaseLetter
                | GeneralCategory::LowercaseLetter
                | GeneralCategory::TitlecaseLetter
                | GeneralCategory::ModifierLetter
                | GeneralCategory::OtherLetter
                | GeneralCategory::DecimalNumber
                | GeneralCategory::LetterNumber
                | GeneralCategory::OtherNumber
        )
}

fn whitespace(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

const SEGMENTATION_FAILURE_MESSAGE: &str =
    "A complete source or claim cannot be partitioned into bounded comparison segments";

fn trimmed_range(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let piece = &text[start..end];
    let left = start + piece.len() - piece.trim_start_matches(whitespace).len();
    let right = start + piece.trim_end_matches(whitespace).len();
    (left < right).then_some((left, right))
}

fn segment_ranges(text: &str) -> Result<Vec<(usize, usize)>, PipelineFailure> {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (offset, c) in text.char_indices() {
        if offset < start {
            continue;
        }
        let class = analysis_sentence_break(c);
        if !matches!(class, SentenceBreak::ATerm | SentenceBreak::STerm) {
            continue;
        }
        let mut end = offset + c.len_utf8();
        while let Some(closer) = text[end..].chars().next().filter(|c| {
            is_analysis_sentence_closer(*c)
                || matches!(
                    analysis_sentence_break(*c),
                    SentenceBreak::ATerm | SentenceBreak::STerm
                )
        }) {
            end += closer.len_utf8();
        }
        if class == SentenceBreak::ATerm
            && text[end..].chars().next().is_some_and(|c| !whitespace(c))
        {
            continue;
        }
        if analysis_sentence_boundary(text, start, end, text.len(), false) {
            sentences.push((start, end));
            start = end;
        }
    }
    if start < text.len() {
        sentences.push((start, text.len()));
    }
    let mut pieces = Vec::new();
    for (start, end) in sentences {
        let Some((mut start, end)) = trimmed_range(text, start, end) else {
            continue;
        };
        if text[start..end].chars().count() <= MAX_SPAN_CHARACTERS {
            pieces.push((start, end));
            continue;
        }
        let mut boundaries = Vec::new();
        for (offset, c) in text[start..end].char_indices() {
            let next = start + offset + c.len_utf8();
            let after = &text[next..end];
            let punctuation = matches!(c, ';' | '\u{61b}' | ':' | '\u{ff1a}')
                && after.chars().next().is_some_and(whitespace);
            let paragraph = c == '\n'
                && after
                    .trim_start_matches([' ', '\t', '\r'])
                    .starts_with('\n');
            if punctuation || paragraph {
                boundaries.push(next);
            }
        }
        boundaries.push(end);
        while start < end {
            let mut selected = None;
            for &cut in boundaries.iter().filter(|&&cut| cut > start) {
                if let Some(range) = trimmed_range(text, start, cut) {
                    if text[range.0..range.1].chars().count() > MAX_SPAN_CHARACTERS {
                        break;
                    }
                    selected = Some((cut, range));
                }
            }
            let (cut, range) = selected.ok_or_else(|| {
                stage_failure(
                    PipelineStage::Verify,
                    "VERIFICATION_INPUT_TOO_LARGE",
                    SEGMENTATION_FAILURE_MESSAGE,
                    false,
                )
            })?;
            pieces.push(range);
            start = trimmed_range(text, cut, end).map_or(end, |(left, _)| left);
        }
    }
    Ok(pieces)
}

fn segment_catalog(texts: &[&str]) -> Result<BTreeSet<String>, PipelineFailure> {
    let mut values = BTreeSet::new();
    for text in texts {
        for (start, end) in segment_ranges(text)? {
            values.insert(text[start..end].to_string());
        }
    }
    if values.is_empty() {
        return Err(too_large());
    }
    Ok(values)
}

#[derive(Debug)]
pub(super) struct Prepared {
    pub(super) schema: Value,
    source_spans: BTreeSet<String>,
    claim_spans: BTreeSet<String>,
}

fn strict_object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object", "properties":properties, "required":required, "additionalProperties":false})
}

impl Prepared {
    fn new(input: &PromptVerificationClaim, schema_limit: usize) -> Result<Self, PipelineFailure> {
        if input.text.trim().is_empty() || input.evidence.is_empty() {
            return Err(invalid_input());
        }
        let mut seen = HashSet::new();
        let mut sources = Vec::new();
        for evidence in &input.evidence {
            let source = evidence.full_clause.as_deref().ok_or_else(invalid_input)?;
            if source.trim().is_empty()
                || evidence.exact_quote.trim().is_empty()
                || !source.contains(&evidence.exact_quote)
            {
                return Err(invalid_input());
            }
            if seen.insert(source) {
                sources.push(source);
            }
        }
        if input.text.chars().count() + sources.iter().map(|s| s.chars().count()).sum::<usize>()
            > MAX_INPUT_CHARACTERS
        {
            return Err(too_large());
        }
        let source_spans = segment_catalog(&sources)?;
        let claim_spans = segment_catalog(&[&input.text])?;
        let comparison = strict_object(
            json!({
                "source_spans":{"type":"array", "items":{"$ref":"#/$defs/source_span"}, "minItems":0,"maxItems":MAX_SPANS},
                "claim_spans":{"type":"array", "items":{"$ref":"#/$defs/claim_span"}, "minItems":0,"maxItems":MAX_SPANS},
                "relation":{"type":"string","enum":["preserved","changed","omitted","uncertain","not_applicable"]}
            }),
            &["source_spans", "claim_spans", "relation"],
        );
        let dimensions: serde_json::Map<String, Value> = DIMENSIONS
            .iter()
            .map(|name| (name.to_string(), comparison.clone()))
            .collect();
        let mut schema = strict_object(
            json!({"verdicts":{
                "type":"array","minItems":1,"maxItems":1,
                "items":strict_object(json!({
                    "claim_id":{"type":"string","enum":["k1"]},
                    "comparisons":strict_object(Value::Object(dimensions), &DIMENSIONS)
                }), &["claim_id","comparisons"])
            }}),
            &["verdicts"],
        );
        schema["$defs"] = json!({
            "source_span":{"type":"string","enum":source_spans},
            "claim_span":{"type":"string","enum":claim_spans}
        });
        let limit = schema_limit.min(crate::pipeline::contracts::MAX_CLAIM_COMPARISON_SCHEMA_BYTES);
        if serde_json::to_vec(&schema)
            .map_err(|_| invalid_input())?
            .len()
            > limit
        {
            return Err(too_large());
        }
        Ok(Self {
            schema,
            source_spans,
            claim_spans,
        })
    }

    fn prompt(
        &self,
        input: &PromptVerificationClaim,
    ) -> Result<(String, identifiers::RequestIds), PipelineFailure> {
        let (wire, ids) = identifiers::verification_prompt(std::slice::from_ref(input))
            .map_err(|_| invalid_input())?;
        let mut value: Value = serde_json::from_str(&wire).map_err(|_| invalid_input())?;
        value["source_segments"] = json!(self.source_spans);
        value["claim_segments"] = json!(self.claim_spans);
        Ok((
            serde_json::to_string(&value).map_err(|_| invalid_input())?,
            ids,
        ))
    }

    pub(super) fn parse(
        &self,
        response: &str,
        claim: &CitedClaim,
    ) -> Result<ClaimVerification, PipelineFailure> {
        let response: Response = serde_json::from_str(response).map_err(|_| invalid_response())?;
        let [verdict] = response.verdicts.as_slice() else {
            return Err(invalid_response());
        };
        if verdict.claim_id != "k1" {
            return Err(invalid_response());
        }
        let mut changed = false;
        let mut uncertain = false;
        let mut preserved = false;
        for comparison in verdict.comparisons.all() {
            if comparison.source_spans.len() > MAX_SPANS
                || comparison.claim_spans.len() > MAX_SPANS
                || comparison
                    .source_spans
                    .iter()
                    .any(|s| !self.source_spans.contains(s))
                || comparison
                    .claim_spans
                    .iter()
                    .any(|s| !self.claim_spans.contains(s))
            {
                return Err(invalid_response());
            }
            let source = !comparison.source_spans.is_empty();
            let target = !comparison.claim_spans.is_empty();
            let valid = match comparison.relation {
                Relation::Preserved | Relation::Changed => source && target,
                Relation::Omitted => source,
                Relation::Uncertain => source || target,
                Relation::NotApplicable => !source && !target,
            };
            if !valid {
                return Err(invalid_response());
            }
            changed |= matches!(comparison.relation, Relation::Changed | Relation::Omitted);
            uncertain |= comparison.relation == Relation::Uncertain;
            preserved |= comparison.relation == Relation::Preserved;
        }
        Ok(ClaimVerification {
            claim_id: claim.claim_id.clone(),
            evidence_ids: claim.evidence_ids.clone(),
            verdict: if changed {
                ClaimVerdict::Unsupported
            } else if uncertain || !preserved {
                ClaimVerdict::Ambiguous
            } else {
                ClaimVerdict::Supported
            },
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    verdicts: Vec<ComparisonVerdict>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComparisonVerdict {
    claim_id: String,
    comparisons: Comparisons,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Comparisons {
    stage: Comparison,
    conditions: Comparison,
    qualifiers: Comparison,
    scope: Comparison,
}
impl Comparisons {
    fn all(&self) -> [&Comparison; 4] {
        [&self.stage, &self.conditions, &self.qualifiers, &self.scope]
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Comparison {
    source_spans: Vec<String>,
    claim_spans: Vec<String>,
    relation: Relation,
}
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Relation {
    Preserved,
    Changed,
    Omitted,
    Uncertain,
    NotApplicable,
}

pub(super) fn plan(
    runtime: &dyn ModelRuntime,
    prompt: &VerificationPrompt,
    claims: &[CitedClaim],
    claim_budget: usize,
) -> Result<Vec<VerificationBatch>, PipelineFailure> {
    validate_verification_claim_catalog(prompt, claims, claim_budget)?;
    if !runtime.supports_response_schema(SCHEMA_NAME) {
        return Err(stage_failure(
            PipelineStage::Verify,
            "VERIFICATION_PROTOCOL_UNSUPPORTED",
            "The selected runtime cannot represent comparison verification",
            false,
        ));
    }
    ensure_verification_batch_count(claims.len())?;
    let request_limit = verification_request_character_limit(
        runtime.context_tokens(PipelineStage::Verify),
        VERIFICATION_OUTPUT_TOKENS,
    )?;
    prompt
        .claims
        .iter()
        .zip(claims)
        .map(|(input, claim)| {
            let mut input = input.clone();
            // The source owner may have no larger clause. Keep the exact quotation;
            // never borrow another claim's context or silently shorten a clause.
            for evidence in &mut input.evidence {
                if evidence.full_clause.is_none() {
                    evidence.full_clause = Some(evidence.exact_quote.clone());
                }
            }
            let prepared = Prepared::new(
                &input,
                runtime.response_schema_byte_limit(PipelineStage::Verify, SCHEMA_NAME),
            )?;
            let mut system_prompt = format!(
                "{VERIFICATION_ENTAILMENT_INSTRUCTION}{CLAUSE_VERIFICATION_INSTRUCTION}\n{INSTRUCTION}"
            );
            if input.source_framing.is_some() {
                system_prompt.push_str(SOURCE_FRAMING_VERIFICATION_INSTRUCTION);
            }
            let (user_prompt, identifiers) = prepared.prompt(&input)?;
            let model_facing_characters =
                system_prompt.chars().count() + user_prompt.chars().count();
            if !verification_request_within_bounds(1, model_facing_characters, request_limit) {
                return Err(too_large());
            }
            Ok(VerificationBatch {
                system_prompt,
                user_prompt,
                claims: vec![claim.clone()],
                identifiers,
                comparison: Some(prepared),
                #[cfg(test)]
                model_facing_characters,
            })
        })
        .collect()
}

#[cfg(test)]
pub(super) fn fixture_response(request: &ModelRequest, unsupported: bool) -> String {
    fixture_verdict_response(
        request,
        if unsupported {
            ClaimVerdict::Unsupported
        } else {
            ClaimVerdict::Supported
        },
    )
}

#[cfg(test)]
pub(super) fn fixture_verdict_response(request: &ModelRequest, verdict: ClaimVerdict) -> String {
    let ModelOutputFormat::JsonSchema { schema, .. } = &request.output_format else {
        panic!("schema required");
    };
    let source = schema["$defs"]["source_span"]["enum"][0].clone();
    let claim = schema["$defs"]["claim_span"]["enum"][0].clone();
    let comparisons: serde_json::Map<String, Value> = DIMENSIONS.iter().map(|name| (name.to_string(), json!({
        "source_spans":[source], "claim_spans":[claim], "relation":match verdict { ClaimVerdict::Supported => "preserved", ClaimVerdict::Unsupported => "changed", ClaimVerdict::Ambiguous => "uncertain" }
    }))).collect();
    json!({"verdicts":[{"claim_id":"k1","comparisons":comparisons}]}).to_string()
}

#[cfg(test)]
mod tests;
