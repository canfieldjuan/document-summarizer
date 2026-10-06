//! Bounded, source-owned C9 comparisons for General prose verification.
use super::*;
use std::collections::BTreeSet;

pub(super) const SCHEMA_NAME: &str = crate::pipeline::contracts::CLAIM_COMPARISON_SCHEMA_NAME;
const MAX_INPUT_CHARACTERS: usize = 4096;
const MAX_SPAN_CHARACTERS: usize = 240;
const MAX_SPANS: usize = 4;
const DIMENSIONS: [&str; 4] = ["stage", "conditions", "qualifiers", "scope"];
const DIMENSION_DEFINITIONS: [&str; 4] = [
    "Stage means the event or lifecycle point to which the claim assigns an action or consequence.",
    "Conditions means prerequisites, exceptions or contingencies governing the proposition the claim asserts.",
    "Qualifiers means negation, modality, time-unit or other restrictions on the asserted relationship.",
    "Scope means the actors, objects or classes included or excluded by that proposition, including an explicitly included class omitted by a narrower summary.",
];
const INSTRUCTION: &str = r#"Compare the claim with its cited quotation interpreted in the complete governing clause.
Return exactly one JSON comparison object containing source_spans, claim_spans, and relation for the requested dimension only.
Choose source_spans from source_segments and claim_spans from claim_segments. Each selection must be one complete supplied segment string, exactly. Do not shorten, join or paraphrase segments. Interpret each selected piece in the complete governing clause, including restrictions in neighboring pieces. Then assign its relation.
Assess only what governs the proposition asserted by the claim. A summary need not repeat distinct, unrelated facts from the surrounding clause. Equivalent faithful wording is preserved.
Use preserved when the claim retains the applicable meaning; changed when it assigns a different meaning; omitted when it drops an applicable restriction or included scope; uncertain when the relationship cannot be established; not_applicable only when that dimension does not apply to this proposition.
Preserved and changed require spans on both sides. Omitted requires source spans and may have no claim span. Uncertain requires at least one side. Not_applicable requires both span lists empty. An empty list is not a substitute for inspecting that dimension.
The application derives the overall verdict from all four comparisons. Do not emit an overall verdict, extra fields, reasoning text or prose."#;

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
        let sentence = (start, end);
        let first_piece = pieces.len();
        let mut primary = Vec::new();
        let mut fallback = Vec::new();
        for (offset, c) in text[start..end].char_indices() {
            let position = start + offset;
            let next = position + c.len_utf8();
            let after = &text[next..end];
            let followed_by_space = after.chars().next().is_some_and(whitespace);
            if (matches!(c, ';' | '\u{61b}' | ':' | '\u{ff1a}') && followed_by_space)
                || (c == '\n'
                    && after
                        .trim_start_matches([' ', '\t', '\r'])
                        .starts_with('\n'))
            {
                primary.push(next);
            }
            if c == ',' && followed_by_space {
                fallback.push(next);
            }
            if position > start && text[..position].chars().next_back().is_some_and(whitespace) {
                let tail = &text[position..end];
                let word_end = tail
                    .find(|c: char| !c.is_alphabetic())
                    .unwrap_or(tail.len());
                let word = &tail[..word_end];
                if ["and", "or", "but", "nor"]
                    .iter()
                    .any(|w| word.eq_ignore_ascii_case(w))
                    && tail[word_end..]
                        .chars()
                        .next()
                        .is_none_or(|c| whitespace(c) || (c.is_ascii_punctuation() && c != '_'))
                {
                    fallback.push(position);
                }
            }
        }
        while start < end {
            if text[start..end].chars().count() <= MAX_SPAN_CHARACTERS {
                pieces.push((start, end));
                break;
            }
            let choose = |cuts: &[usize]| {
                cuts.iter()
                    .copied()
                    .filter(|&cut| cut > start)
                    .filter_map(|cut| trimmed_range(text, start, cut).map(|range| (cut, range)))
                    .take_while(|(_, range)| {
                        text[range.0..range.1].chars().count() <= MAX_SPAN_CHARACTERS
                    })
                    .last()
            };
            if let Some((cut, range)) = choose(&primary).or_else(|| choose(&fallback)) {
                pieces.push(range);
                start = trimmed_range(text, cut, end).map_or(end, |(left, _)| left);
            } else {
                pieces.truncate(first_piece);
                pieces.push(sentence);
                break;
            }
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

#[derive(Debug, Clone)]
pub(super) struct Prepared {
    dimension: usize,
    claim_id: String,
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
        let choices: Vec<_> = Relation::ALL
            .iter()
            .flat_map(|relation| {
                relation.shapes().iter().map(move |&(source, claim)| strict_object(json!({
                "source_spans":{"type":"array", "items":{"$ref":"#/$defs/source_span"},
                    "minItems":usize::from(source), "maxItems":if source {MAX_SPANS} else {0}},
                "claim_spans":{"type":"array", "items":{"$ref":"#/$defs/claim_span"},
                    "minItems":usize::from(claim), "maxItems":if claim {MAX_SPANS} else {0}},
                "relation":{"type":"string", "enum":[relation]}
            }), &["source_spans", "claim_spans", "relation"]))
            })
            .collect();
        let schema = json!({"anyOf":choices, "$defs":{
            "source_span":{"type":"string","enum":source_spans},
            "claim_span":{"type":"string","enum":claim_spans}
        }});
        let limit = schema_limit.min(crate::pipeline::contracts::MAX_CLAIM_COMPARISON_SCHEMA_BYTES);
        if serde_json::to_vec(&schema)
            .map_err(|_| invalid_input())?
            .len()
            > limit
        {
            return Err(too_large());
        }
        Ok(Self {
            dimension: 0,
            claim_id: input.claim_id.clone(),
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
        value["dimension"] = json!(DIMENSIONS[self.dimension]);
        value["source_segments"] = json!(self.source_spans);
        value["claim_segments"] = json!(self.claim_spans);
        Ok((
            serde_json::to_string(&value).map_err(|_| invalid_input())?,
            ids,
        ))
    }

    fn parse(&self, response: &str) -> Result<Comparison, PipelineFailure> {
        let comparison: Comparison =
            serde_json::from_str(response).map_err(|_| invalid_response())?;
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
            || !comparison.relation.shapes().contains(&(
                !comparison.source_spans.is_empty(),
                !comparison.claim_spans.is_empty(),
            ))
        {
            return Err(invalid_response());
        }
        Ok(comparison)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Comparison {
    source_spans: Vec<String>,
    claim_spans: Vec<String>,
    relation: Relation,
}
#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Relation {
    Preserved,
    Changed,
    Omitted,
    Uncertain,
    NotApplicable,
}
impl Relation {
    const ALL: [Self; 5] = [
        Self::Preserved,
        Self::Changed,
        Self::Omitted,
        Self::Uncertain,
        Self::NotApplicable,
    ];
    fn shapes(&self) -> &'static [(bool, bool)] {
        match self {
            Self::Preserved | Self::Changed => &[(true, true)],
            Self::Omitted => &[(true, false), (true, true)],
            Self::Uncertain => &[(false, true), (true, false), (true, true)],
            Self::NotApplicable => &[(false, false)],
        }
    }
}

fn aggregate(comparisons: &[Comparison; 4]) -> ClaimVerdict {
    if comparisons
        .iter()
        .any(|c| matches!(c.relation, Relation::Changed | Relation::Omitted))
    {
        ClaimVerdict::Unsupported
    } else if comparisons
        .iter()
        .any(|c| c.relation == Relation::Uncertain)
        || !comparisons
            .iter()
            .any(|c| c.relation == Relation::Preserved)
    {
        ClaimVerdict::Ambiguous
    } else {
        ClaimVerdict::Supported
    }
}

fn validate_plan(batches: &[VerificationBatch]) -> Result<(), PipelineFailure> {
    ensure_verification_batch_count(batches.len())?;
    if !batches.len().is_multiple_of(DIMENSIONS.len()) {
        return Err(invalid_input());
    }
    let mut seen = HashSet::new();
    for group in batches.as_chunks::<{ DIMENSIONS.len() }>().0 {
        let [claim] = group[0].claims.as_slice() else {
            return Err(invalid_input());
        };
        if !seen.insert(&claim.claim_id) {
            return Err(invalid_input());
        }
        for (dimension, batch) in group.iter().enumerate() {
            let [owned] = batch.claims.as_slice() else {
                return Err(invalid_input());
            };
            let prepared = batch.comparison.as_ref().ok_or_else(invalid_input)?;
            if prepared.dimension != dimension
                || prepared.claim_id != claim.claim_id
                || owned.claim_id != claim.claim_id
                || owned.text != claim.text
                || owned.evidence_ids != claim.evidence_ids
            {
                return Err(invalid_input());
            }
        }
    }
    Ok(())
}

pub(super) fn classify(
    runtime: &dyn ModelRuntime,
    batches: Vec<VerificationBatch>,
    seed: u64,
    next_ordinal: &mut u32,
    control: &dyn ExecutionControl,
) -> Result<Vec<ClaimVerification>, PipelineFailure> {
    validate_plan(&batches)?;
    let mut admission_ordinal = *next_ordinal;
    for batch in &batches {
        cancellation_checkpoint(control, PipelineStage::Verify)?;
        let ordinal = reserve_model_request_ordinal(&mut admission_ordinal, PipelineStage::Verify)?;
        runtime
            .preflight_request(&verification_request(batch, ordinal, seed))
            .map_err(|e| {
                runtime_pipeline_failure(PipelineStage::Verify, "MODEL_VERIFICATION_ADMISSION", e)
            })?;
    }
    let mut verdicts = Vec::new();
    for group in batches.as_chunks::<{ DIMENSIONS.len() }>().0 {
        let mut selected = Vec::new();
        for batch in group {
            cancellation_checkpoint(control, PipelineStage::Verify)?;
            let ordinal = reserve_model_request_ordinal(next_ordinal, PipelineStage::Verify)?;
            let response =
                runtime.generate_with_control(&verification_request(batch, ordinal, seed), control);
            cancellation_checkpoint(control, PipelineStage::Verify)?;
            let response = response.map_err(|e| {
                runtime_pipeline_failure(PipelineStage::Verify, "MODEL_VERIFICATION", e)
            })?;
            validate_runtime_response(runtime, &response, PipelineStage::Verify)?;
            selected.push(
                batch
                    .comparison
                    .as_ref()
                    .ok_or_else(invalid_input)?
                    .parse(&response.text)?,
            );
        }
        let selected: [Comparison; 4] = selected.try_into().map_err(|_| invalid_response())?;
        verdicts.push(ClaimVerification {
            claim_id: group[0].claims[0].claim_id.clone(),
            evidence_ids: group[0].claims[0].evidence_ids.clone(),
            verdict: aggregate(&selected),
        });
    }
    Ok(verdicts)
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
    ensure_verification_batch_count(
        claims
            .len()
            .checked_mul(DIMENSIONS.len())
            .ok_or_else(too_large)?,
    )?;
    let request_limit = verification_request_character_limit(
        runtime.context_tokens(PipelineStage::Verify),
        VERIFICATION_OUTPUT_TOKENS,
    )?;
    let groups = prompt
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
            (0..DIMENSIONS.len()).map(|dimension| {
                let mut prepared = prepared.clone();
                prepared.dimension = dimension;
                let mut system_prompt = format!("{VERIFICATION_ENTAILMENT_INSTRUCTION}{CLAUSE_VERIFICATION_INSTRUCTION}\n{INSTRUCTION}\nRequested dimension: {}. {}", DIMENSIONS[dimension], DIMENSION_DEFINITIONS[dimension]);
                if input.source_framing.is_some() { system_prompt.push_str(SOURCE_FRAMING_VERIFICATION_INSTRUCTION); }
                let (user_prompt, identifiers) = prepared.prompt(&input)?;
                let model_facing_characters = system_prompt.chars().count() + user_prompt.chars().count();
                if !verification_request_within_bounds(1, model_facing_characters, request_limit) { return Err(too_large()); }
                Ok(VerificationBatch { system_prompt, user_prompt, claims:vec![claim.clone()], identifiers,
                    comparison:Some(prepared), #[cfg(test)] model_facing_characters })
            }).collect::<Result<Vec<_>, PipelineFailure>>()
        }).collect::<Result<Vec<_>, PipelineFailure>>()?;
    Ok(groups.into_iter().flatten().collect())
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
    json!({"source_spans":[source], "claim_spans":[claim], "relation":match verdict {
        ClaimVerdict::Supported=>"preserved", ClaimVerdict::Unsupported=>"changed", ClaimVerdict::Ambiguous=>"uncertain"
    }}).to_string()
}

#[cfg(test)]
mod tests;
