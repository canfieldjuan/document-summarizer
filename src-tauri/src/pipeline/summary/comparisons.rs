//! Bounded, source-owned C9 comparisons for General prose verification.
use super::*;
use crate::pipeline::contracts::{AdmissionLimits, AdmissionWithhold, AdmissionWithholdReason};
use std::collections::BTreeSet;

#[derive(Debug)]
struct CapacityExcess {
    reason: AdmissionWithholdReason,
    observed: usize,
    minimum: usize,
    maximum: usize,
}

#[derive(Debug)]
enum PreparationFailure {
    Invalid(PipelineFailure),
    Capacity(CapacityExcess),
}
impl From<PipelineFailure> for PreparationFailure {
    fn from(value: PipelineFailure) -> Self {
        Self::Invalid(value)
    }
}
impl PreparationFailure {
    #[cfg(test)]
    fn into_pipeline(self) -> PipelineFailure {
        match self {
            Self::Invalid(failure) => failure,
            Self::Capacity(_) => too_large(),
        }
    }
}
fn capacity(
    reason: AdmissionWithholdReason,
    observed: usize,
    maximum: usize,
) -> PreparationFailure {
    PreparationFailure::Capacity(CapacityExcess {
        reason,
        observed,
        minimum: 0,
        maximum,
    })
}

pub(super) const SCHEMA_NAME: &str = crate::pipeline::contracts::CLAIM_COMPARISON_SCHEMA_NAME;
const MAX_INPUT_CHARACTERS: usize = 4096;
const MAX_ENUM_VALUES: usize = 8192;
const MAX_SPAN_CHARACTERS: usize = 240;
const MAX_SPANS: usize = 4;
const DIMENSIONS: [&str; 4] = ["stage", "conditions", "qualifiers", "scope"];
const INSTRUCTION: &str = r#"Compare the claim with its cited quotation interpreted in the complete governing clause.
Return exactly one JSON object with verdicts containing one item with claim_id and comparisons.
The comparisons object must contain stage, conditions, qualifiers, and scope. Do not choose a subset.
For each dimension copy source_spans from the supplied full_clause and claim_spans from the claim text, exactly. Do not paraphrase these spans. Then assign its relation.
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

// The candidate has not passed the fidelity gate. Keep qualification possible
// in unit-test binaries, but provide no feature, setting or runtime opt-in in
// an ordinary library/application build. `applies` remains a format predicate:
// disabling it would silently send pending C9 prose through an older verifier.
pub(super) fn candidate_enabled() -> bool {
    cfg!(test)
}

pub(super) fn require_candidate_enabled() -> Result<(), PipelineFailure> {
    if candidate_enabled() {
        Ok(())
    } else {
        Err(stage_failure(
            PipelineStage::Verify,
            "VERIFICATION_NOT_QUALIFIED",
            "General coherent verification has not passed qualification; retry to use verified source claims",
            true,
        ))
    }
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

#[cfg(test)]
fn span_catalog(texts: &[&str]) -> Result<BTreeSet<String>, PipelineFailure> {
    bounded_span_catalog(texts, AdmissionWithholdReason::SourceEnumValues)
        .map_err(PreparationFailure::into_pipeline)
}

fn bounded_span_catalog(
    texts: &[&str],
    reason: AdmissionWithholdReason,
) -> Result<BTreeSet<String>, PreparationFailure> {
    let mut values = BTreeSet::new();
    for text in texts {
        let mut tokens: Vec<(usize, usize, usize, usize)> = Vec::new();
        let mut previous_word = false;
        for (position, (byte, c)) in text.char_indices().enumerate() {
            let word = word_character(c);
            if word && previous_word {
                let last = tokens.last_mut().expect("preceding word token");
                last.1 = byte + c.len_utf8();
                last.3 = position + 1;
            } else if !whitespace(c) {
                tokens.push((byte, byte + c.len_utf8(), position, position + 1));
            }
            previous_word = word;
        }
        for (index, start) in tokens.iter().enumerate() {
            for end in &tokens[index..] {
                if end.3 - start.2 > MAX_SPAN_CHARACTERS {
                    break;
                }
                values.insert(text[start.0..end.1].to_string());
                if values.len() > MAX_ENUM_VALUES {
                    return Err(capacity(reason, values.len(), MAX_ENUM_VALUES));
                }
            }
        }
    }
    if values.is_empty() {
        return Err(PreparationFailure::Capacity(CapacityExcess {
            reason,
            observed: 0,
            minimum: 1,
            maximum: MAX_ENUM_VALUES,
        }));
    }
    Ok(values)
}

#[derive(Debug, Clone)]
pub(super) struct Prepared {
    claim: CitedClaim,
    pub(super) schema: Value,
    source_spans: BTreeSet<String>,
    claim_spans: BTreeSet<String>,
}

fn strict_object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object", "properties":properties, "required":required, "additionalProperties":false})
}

impl Prepared {
    #[cfg(test)]
    fn new(input: &PromptVerificationClaim, schema_limit: usize) -> Result<Self, PipelineFailure> {
        Self::prepare(input, schema_limit).map_err(PreparationFailure::into_pipeline)
    }

    fn prepare(
        input: &PromptVerificationClaim,
        schema_limit: usize,
    ) -> Result<Self, PreparationFailure> {
        if input.text.trim().is_empty() || input.evidence.is_empty() {
            return Err(invalid_input().into());
        }
        let mut seen = HashSet::new();
        let mut sources = Vec::new();
        for evidence in &input.evidence {
            let source = evidence.full_clause.as_deref().ok_or_else(invalid_input)?;
            if source.trim().is_empty()
                || evidence.exact_quote.trim().is_empty()
                || !source.contains(&evidence.exact_quote)
            {
                return Err(invalid_input().into());
            }
            if seen.insert(source) {
                sources.push(source);
            }
        }
        for source in &sources {
            let length = source.chars().count();
            if length > MAX_INPUT_CHARACTERS {
                return Err(capacity(
                    AdmissionWithholdReason::GoverningClauseScalars,
                    length,
                    MAX_INPUT_CHARACTERS,
                ));
            }
        }
        let length =
            input.text.chars().count() + sources.iter().map(|s| s.chars().count()).sum::<usize>();
        if length > MAX_INPUT_CHARACTERS {
            return Err(capacity(
                AdmissionWithholdReason::CombinedScalars,
                length,
                MAX_INPUT_CHARACTERS,
            ));
        }
        let source_spans =
            bounded_span_catalog(&sources, AdmissionWithholdReason::SourceEnumValues)?;
        let claim_spans =
            bounded_span_catalog(&[&input.text], AdmissionWithholdReason::ClaimEnumValues)?;
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
        let schema_bytes = serde_json::to_vec(&schema)
            .map_err(|_| invalid_input())?
            .len();
        if schema_bytes > limit {
            return Err(capacity(
                AdmissionWithholdReason::SchemaBytes,
                schema_bytes,
                limit,
            ));
        }
        Ok(Self {
            claim: CitedClaim {
                claim_id: input.claim_id.clone(),
                text: input.text.clone(),
                evidence_ids: input
                    .evidence
                    .iter()
                    .map(|e| e.evidence_id.clone())
                    .collect(),
            },
            schema,
            source_spans,
            claim_spans,
        })
    }

    pub(super) fn parse(
        &self,
        response: &str,
        claim: &CitedClaim,
    ) -> Result<ClaimVerification, PipelineFailure> {
        if claim != &self.claim {
            return Err(invalid_response());
        }
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
            if !comparison.owned_by(self) || !comparison.relation_valid() {
                return Err(invalid_response());
            }
            changed |= matches!(comparison.relation, Relation::Changed | Relation::Omitted);
            uncertain |= comparison.relation == Relation::Uncertain;
            preserved |= comparison.relation == Relation::Preserved;
        }
        Ok(ClaimVerification {
            admission_withhold: None,
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
impl Comparison {
    fn owned_by(&self, prepared: &Prepared) -> bool {
        self.source_spans.len() <= MAX_SPANS
            && self.claim_spans.len() <= MAX_SPANS
            && self
                .source_spans
                .iter()
                .all(|s| prepared.source_spans.contains(s))
            && self
                .claim_spans
                .iter()
                .all(|s| prepared.claim_spans.contains(s))
    }
    fn relation_valid(&self) -> bool {
        let source = !self.source_spans.is_empty();
        let target = !self.claim_spans.is_empty();
        match self.relation {
            Relation::Preserved | Relation::Changed => source && target,
            Relation::Omitted => source,
            Relation::Uncertain => source || target,
            Relation::NotApplicable => !source && !target,
        }
    }
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

pub(super) fn classify(
    runtime: &dyn ModelRuntime,
    batches: Vec<VerificationBatch>,
    seed: u64,
    next_ordinal: &mut u32,
    control: &dyn ExecutionControl,
) -> Result<Vec<ClaimVerification>, PipelineFailure> {
    require_candidate_enabled()?;
    ensure_verification_batch_count(batches.len())?;
    let mut seen = HashSet::new();
    let mut admission_ordinal = *next_ordinal;
    for batch in &batches {
        let [claim] = batch.claims.as_slice() else {
            return Err(invalid_input());
        };
        let prepared = batch.comparison.as_ref().ok_or_else(invalid_input)?;
        if claim != &prepared.claim || !seen.insert(&claim.claim_id) {
            return Err(invalid_input());
        }
        cancellation_checkpoint(control, PipelineStage::Verify)?;
        let ordinal = reserve_model_request_ordinal(&mut admission_ordinal, PipelineStage::Verify)?;
        runtime
            .preflight_request(&verification_request(batch, ordinal, seed))
            .map_err(|e| {
                runtime_pipeline_failure(PipelineStage::Verify, "MODEL_VERIFICATION_ADMISSION", e)
            })?;
    }
    let mut verdicts = Vec::new();
    for batch in &batches {
        cancellation_checkpoint(control, PipelineStage::Verify)?;
        let ordinal = reserve_model_request_ordinal(next_ordinal, PipelineStage::Verify)?;
        let response =
            runtime.generate_with_control(&verification_request(batch, ordinal, seed), control);
        cancellation_checkpoint(control, PipelineStage::Verify)?;
        let response = response.map_err(|e| {
            runtime_pipeline_failure(PipelineStage::Verify, "MODEL_VERIFICATION", e)
        })?;
        validate_runtime_response(runtime, &response, PipelineStage::Verify)?;
        verdicts.push(
            batch
                .comparison
                .as_ref()
                .ok_or_else(invalid_input)?
                .parse(&response.text, &batch.claims[0])?,
        );
    }
    Ok(verdicts)
}

pub(super) fn limits_for_runtime(runtime: &dyn ModelRuntime) -> AdmissionLimits {
    AdmissionLimits {
        context_tokens: runtime.context_tokens(PipelineStage::Verify),
        schema_bytes: runtime
            .response_schema_byte_limit(PipelineStage::Verify, SCHEMA_NAME)
            .min(crate::pipeline::contracts::MAX_CLAIM_COMPARISON_SCHEMA_BYTES),
    }
}

#[derive(Debug)]
pub(super) enum AdmissionEntry {
    Admitted(Box<VerificationBatch>),
    Withheld(ClaimVerification),
}
#[derive(Debug)]
pub(super) struct AdmissionPlan {
    pub entries: Vec<AdmissionEntry>,
}
impl AdmissionPlan {
    fn require_all_admitted(self) -> Result<Vec<VerificationBatch>, PipelineFailure> {
        self.entries
            .into_iter()
            .map(|entry| match entry {
                AdmissionEntry::Admitted(batch) => Ok(*batch),
                AdmissionEntry::Withheld(_) => Err(too_large()),
            })
            .collect()
    }

    pub(super) fn preflight(
        &self,
        runtime: &dyn ModelRuntime,
        seed: u64,
        first_ordinal: u32,
        control: &dyn ExecutionControl,
    ) -> Result<(), PipelineFailure> {
        let mut ordinal = first_ordinal;
        for entry in &self.entries {
            if let AdmissionEntry::Admitted(batch) = entry {
                cancellation_checkpoint(control, PipelineStage::Verify)?;
                let next = reserve_model_request_ordinal(&mut ordinal, PipelineStage::Verify)?;
                runtime
                    .preflight_request(&verification_request(batch, next, seed))
                    .map_err(|failure| {
                        runtime_pipeline_failure(
                            PipelineStage::Verify,
                            "MODEL_VERIFICATION_ADMISSION",
                            failure,
                        )
                    })?;
            }
        }
        Ok(())
    }

    pub(super) fn classify(
        self,
        runtime: &dyn ModelRuntime,
        seed: u64,
        next_ordinal: &mut u32,
        control: &dyn ExecutionControl,
    ) -> Result<Vec<ClaimVerification>, PipelineFailure> {
        let mut slots = Vec::new();
        let mut batches = Vec::new();
        for entry in self.entries {
            match entry {
                AdmissionEntry::Admitted(batch) => {
                    slots.push(None);
                    batches.push(*batch);
                }
                AdmissionEntry::Withheld(held) => slots.push(Some(held)),
            }
        }
        let mut generated = if batches.is_empty() {
            vec![]
        } else {
            classify_verification_batches(runtime, batches, seed, next_ordinal, control)?
        }
        .into_iter();
        let results = slots
            .into_iter()
            .map(|slot| slot.map_or_else(|| generated.next().ok_or_else(invalid_response), Ok))
            .collect::<Result<Vec<_>, _>>()?;
        if generated.next().is_some() {
            return Err(invalid_response());
        }
        Ok(results)
    }
}

pub(super) fn plan(
    runtime: &dyn ModelRuntime,
    prompt: &VerificationPrompt,
    claims: &[CitedClaim],
    claim_budget: usize,
) -> Result<Vec<VerificationBatch>, PipelineFailure> {
    require_candidate_enabled()?;
    plan_for_limits(
        prompt,
        claims,
        claim_budget,
        &limits_for_runtime(runtime),
        true,
        runtime.supports_response_schema(SCHEMA_NAME),
    )?
    .require_all_admitted()
}

pub(super) fn plan_admissions(
    runtime: &dyn ModelRuntime,
    prompt: &VerificationPrompt,
    claims: &[CitedClaim],
    claim_budget: usize,
) -> Result<AdmissionPlan, PipelineFailure> {
    require_candidate_enabled()?;
    plan_for_limits(
        prompt,
        claims,
        claim_budget,
        &limits_for_runtime(runtime),
        false,
        runtime.supports_response_schema(SCHEMA_NAME),
    )
}

pub(super) fn plan_for_limits(
    prompt: &VerificationPrompt,
    claims: &[CitedClaim],
    claim_budget: usize,
    limits: &AdmissionLimits,
    allow_quote_fallback: bool,
    protocol_supported: bool,
) -> Result<AdmissionPlan, PipelineFailure> {
    validate_verification_claim_catalog(prompt, claims, claim_budget)?;
    if !protocol_supported {
        return Err(stage_failure(
            PipelineStage::Verify,
            "VERIFICATION_PROTOCOL_UNSUPPORTED",
            "The selected runtime cannot represent comparison verification",
            false,
        ));
    }
    ensure_verification_batch_count(claims.len())?;
    let request_limit =
        verification_request_character_limit(limits.context_tokens, VERIFICATION_OUTPUT_TOKENS)?;
    if limits.schema_bytes > crate::pipeline::contracts::MAX_CLAIM_COMPARISON_SCHEMA_BYTES {
        return Err(invalid_input());
    }
    let mut entries = Vec::new();
    for (input, claim) in prompt.claims.iter().zip(claims) {
        let mut input = input.clone();
        for evidence in &mut input.evidence {
            if evidence.full_clause.is_none() {
                if !allow_quote_fallback {
                    return Err(invalid_input());
                }
                evidence.full_clause = Some(evidence.exact_quote.clone());
            }
        }
        let owned = input
            .evidence
            .iter()
            .map(|e| (&e.evidence_id, &e.exact_quote, &e.full_clause))
            .collect::<Vec<_>>();
        let input_sha256 = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(&input.claim_id, &input.text, &input.source_framing, owned))
                    .map_err(|_| invalid_input())?
            )
        );
        let held = |excess: CapacityExcess| {
            AdmissionEntry::Withheld(ClaimVerification {
                claim_id: claim.claim_id.clone(),
                evidence_ids: claim.evidence_ids.clone(),
                verdict: ClaimVerdict::Ambiguous,
                admission_withhold: Some(AdmissionWithhold {
                    verification_version: WHOLE_DOCUMENT_VERIFICATION_VERSION.into(),
                    input_sha256: input_sha256.clone(),
                    limits: limits.clone(),
                    reason: excess.reason,
                    observed: excess.observed,
                    minimum: excess.minimum,
                    maximum: excess.maximum,
                }),
            })
        };
        let prepared = match Prepared::prepare(&input, limits.schema_bytes) {
            Ok(prepared) => prepared,
            Err(PreparationFailure::Capacity(excess)) => {
                if allow_quote_fallback {
                    return Err(too_large());
                }
                entries.push(held(excess));
                continue;
            }
            Err(PreparationFailure::Invalid(failure)) => return Err(failure),
        };
        let mut system_prompt = format!(
            "{VERIFICATION_ENTAILMENT_INSTRUCTION}{CLAUSE_VERIFICATION_INSTRUCTION}\n{INSTRUCTION}"
        );
        if input.source_framing.is_some() {
            system_prompt.push_str(SOURCE_FRAMING_VERIFICATION_INSTRUCTION);
        }
        let (user_prompt, identifiers) =
            identifiers::verification_prompt(&[input]).map_err(|_| invalid_input())?;
        let model_facing_characters = system_prompt.chars().count() + user_prompt.chars().count();
        if !verification_request_within_bounds(1, model_facing_characters, request_limit) {
            if allow_quote_fallback {
                return Err(too_large());
            }
            entries.push(held(CapacityExcess {
                reason: AdmissionWithholdReason::RequestCharacters,
                observed: model_facing_characters,
                minimum: 0,
                maximum: request_limit,
            }));
            continue;
        }
        entries.push(AdmissionEntry::Admitted(Box::new(VerificationBatch {
            system_prompt,
            user_prompt,
            claims: vec![claim.clone()],
            identifiers,
            comparison: Some(prepared),
            #[cfg(test)]
            model_facing_characters,
        })));
    }
    Ok(AdmissionPlan { entries })
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
pub(super) fn readiness_catalog_size(texts: &[&str]) -> Result<usize, PipelineFailure> {
    span_catalog(texts).map(|catalog| catalog.len())
}

#[cfg(test)]
pub(super) fn readiness_prepared_admits(
    input: &PromptVerificationClaim,
    schema_limit: usize,
) -> bool {
    Prepared::new(input, schema_limit).is_ok()
}

#[cfg(test)]
mod tests;
