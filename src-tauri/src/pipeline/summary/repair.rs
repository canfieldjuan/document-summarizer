use super::*;

// Reserve the complete feedback envelope plus the maximum eight bounded IDs
// during partitioning, before any inference. Actual payloads are checked too.
pub(super) const FEEDBACK_RESERVE: usize = 1_536;
const MISSING_REFERENCES: &str = "SYNTHESIS_MISSING_REFERENCES";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MissingReferences {
    missing_reference_ids: Vec<String>,
}

pub(super) fn missing_references(mut ids: Vec<String>) -> PipelineFailure {
    ids.sort();
    stage_failure(
        PipelineStage::Synthesize,
        MISSING_REFERENCES,
        json!({"missing_reference_ids": ids}).to_string(),
        true,
    )
}

fn with_feedback(user: &str, ids: &[String]) -> Result<String, PipelineFailure> {
    let mut value: Value = serde_json::from_str(user).map_err(|_| invalid_request())?;
    value.as_object_mut().ok_or_else(invalid_request)?.insert("repair".to_string(), json!({
        "missing_reference_ids": ids,
        "instruction": "Regenerate the complete response within the same bounds. Your previous response omitted these mandatory references. Cover ALL supplied references with genuinely supported claims; do not append IDs to unrelated claims."
    }));
    serde_json::to_string(&value).map_err(|_| invalid_request())
}

fn invalid_request() -> PipelineFailure {
    stage_failure(
        PipelineStage::Synthesize,
        "MODEL_REQUEST_INVALID",
        "Cannot serialize bounded synthesis repair",
        false,
    )
}

pub(super) fn generate(
    runtime: &dyn ModelRuntime,
    mut request: ModelRequest,
    required: &[String],
    control: &dyn ExecutionControl,
    budget: &mut SynthesisRequestBudget,
    parse: impl Fn(&str) -> Result<Vec<ValidatedClaim>, PipelineFailure>,
) -> Result<Vec<ValidatedClaim>, PipelineFailure> {
    let original_user = request.user_prompt.clone();
    let worst_user = with_feedback(&original_user, required)?;
    let limit =
        generation_input_character_limit(SYNTHESIS_OUTPUT_TOKENS).ok_or_else(invalid_request)?;
    if worst_user.chars().count() > original_user.chars().count() + FEEDBACK_RESERVE
        || worst_user.chars().count() + request.system_prompt.chars().count() > limit
        || budget
            .used
            .checked_add(2)
            .is_none_or(|used| used > MAX_SYNTHESIS_MODEL_REQUESTS)
    {
        return Err(stage_failure(
            PipelineStage::Synthesize,
            "SYNTHESIS_PLAN_TOO_LARGE",
            "Synthesis and its bounded repair must fit before generation",
            false,
        ));
    }
    let seed = request.seed;
    for attempt in 0..=1 {
        cancellation_checkpoint(control, PipelineStage::Synthesize)?;
        request.ordinal = budget.reserve()?;
        if attempt == 1 {
            request.seed = generation_seed_for_attempt(seed, request.ordinal.saturating_add(1));
        }
        let response = runtime.generate(&request).map_err(|f| {
            runtime_pipeline_failure(PipelineStage::Synthesize, "MODEL_SYNTHESIS", f)
        })?;
        validate_runtime_response(runtime, &response, PipelineStage::Synthesize)?;
        cancellation_checkpoint(control, PipelineStage::Synthesize)?;
        match parse(&response.text) {
            Ok(claims) => return Ok(claims),
            Err(failure) if attempt == 0 && failure.code == MISSING_REFERENCES => {
                let missing: MissingReferences =
                    serde_json::from_str(&failure.message).map_err(|_| invalid_request())?;
                if missing.missing_reference_ids.is_empty()
                    || missing
                        .missing_reference_ids
                        .iter()
                        .any(|id| !required.contains(id))
                {
                    return Err(invalid_request());
                }
                request.user_prompt =
                    with_feedback(&original_user, &missing.missing_reference_ids)?;
            }
            Err(failure) => return Err(failure),
        }
    }
    unreachable!("bounded synthesis loop always returns after its second response")
}
