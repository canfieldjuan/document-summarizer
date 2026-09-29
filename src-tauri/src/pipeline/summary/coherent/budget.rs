//! One admission decision for the coherent response shape and its token allowance.
use super::*;
use crate::pipeline::qwen_tokenizer::TOKENIZER_FRAMING_RESERVE_TOKENS;

const FORMATTING_TOKENS: u32 = 256;
const REPAIR_FEEDBACK_TOKENS: u32 = 512;
const CANONICAL_JSON_BYTES_PER_CHARACTER: usize = 6;

fn invalid_budget() -> PipelineFailure {
    stage_failure(
        PipelineStage::Synthesize,
        "INVALID_SYNTHESIS_BUDGET",
        "The synthesis response shape cannot be budgeted",
        false,
    )
}

/// Qualified byte-level BPE tokenizers cannot emit more tokens than canonical
/// JSON bytes. Include the longest allowed distinct IDs and all punctuation.
/// Arbitrary wire whitespace/escape choices still require a verified stop.
fn response_tokens(schema: &Value, characters: usize) -> Option<u32> {
    let units = &schema["properties"]["units"];
    let count = usize::try_from(units["maxItems"].as_u64()?).ok()?;
    let sources = &units["items"]["properties"]["source_ids"];
    let source_count = usize::try_from(sources["maxItems"].as_u64()?).ok()?;
    if count == 0
        || count > MAX_SUMMARY_CLAIMS
        || source_count == 0
        || source_count > MAX_SOURCES_PER_UNIT
        || characters == 0
        || characters > MAX_UNIT_CHARACTERS
    {
        return None;
    }
    let mut ids = sources["items"]["enum"]
        .as_array()?
        .iter()
        .map(|id| Some((serde_json::to_string(id.as_str()?).ok()?.len(), id.clone())))
        .collect::<Option<Vec<_>>>()?;
    if ids.len() < source_count {
        return None;
    }
    ids.sort_by_key(|(size, _)| std::cmp::Reverse(*size));
    let ids: Vec<_> = ids
        .into_iter()
        .take(source_count)
        .map(|(_, id)| id)
        .collect();
    let empty = json!({"units": vec![json!({"text":"","source_ids":ids}); count]});
    let text_bytes = count
        .checked_mul(characters)?
        .checked_mul(CANONICAL_JSON_BYTES_PER_CHARACTER)?;
    let bytes = serde_json::to_vec(&empty)
        .ok()?
        .len()
        .checked_add(text_bytes)?;
    u32::try_from(bytes)
        .ok()?
        .checked_add(FORMATTING_TOKENS)
        .map(|n| n.max(OUTPUT_TOKENS))
}

/// Search uses the runtime's existing exact preflight, so stage wrappers cannot
/// accidentally substitute a second tokenizer or a different framing policy.
pub(super) fn admit(
    runtime: &dyn ModelRuntime,
    request: &ModelRequest,
    resize: bool,
) -> Result<Option<ModelRequest>, PipelineFailure> {
    let ModelOutputFormat::JsonSchema { schema, .. } = &request.output_format else {
        return Err(invalid_budget());
    };
    let maximum = schema["properties"]["units"]["items"]["properties"]["text"]["maxLength"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0 && *n <= MAX_UNIT_CHARACTERS)
        .ok_or_else(invalid_budget)?;
    let mut low = if resize { 1 } else { maximum };
    let mut high = maximum;
    let mut admitted = None;
    while low <= high {
        let characters = low + (high - low) / 2;
        let tokens = response_tokens(schema, characters).ok_or_else(invalid_budget)?;
        let mut candidate = request.clone();
        let ModelOutputFormat::JsonSchema { schema, .. } = &mut candidate.output_format else {
            unreachable!("the request shape was checked above");
        };
        schema["properties"]["units"]["items"]["properties"]["text"]["maxLength"] =
            json!(characters);
        // A repair consumes the headroom reserved by the initial request.
        candidate.max_output_tokens = tokens
            .checked_add(if resize { REPAIR_FEEDBACK_TOKENS } else { 0 })
            .ok_or_else(invalid_budget)?;
        let within_context = candidate
            .max_output_tokens
            .checked_add(TOKENIZER_FRAMING_RESERVE_TOKENS)
            .is_some_and(|n| n <= runtime.context_tokens(PipelineStage::Synthesize));
        if within_context && !request_exceeds_runtime_context(runtime, &candidate)? {
            candidate.max_output_tokens = tokens;
            admitted = Some(candidate);
            low = characters + 1;
        } else {
            high = characters - 1;
        }
    }
    Ok(admitted)
}

pub(super) fn validate_response(
    request: &ModelRequest,
    response: &str,
) -> Result<(), PipelineFailure> {
    let ModelOutputFormat::JsonSchema { schema, .. } = &request.output_format else {
        return Err(invalid_budget());
    };
    let maximum = schema["properties"]["units"]["items"]["properties"]["text"]["maxLength"]
        .as_u64()
        .ok_or_else(invalid_budget)?;
    let raw: RawResponse = serde_json::from_str(response).map_err(|_| invalid_response())?;
    if raw
        .units
        .iter()
        .any(|unit| unit.text.chars().count() as u64 > maximum)
    {
        return Err(invalid_response());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::qwen_tokenizer::QwenPromptTokenizer;

    struct CapacityRuntime {
        context: u32,
        input: u32,
        failure: Option<&'static str>,
    }

    impl ModelRuntime for CapacityRuntime {
        fn generate(&self, _: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            panic!("preflight does not generate")
        }
        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }
        fn runtime_id(&self) -> &str {
            "capacity-test"
        }
        fn model_id(&self) -> &str {
            "capacity-test"
        }
        fn context_tokens(&self, _: PipelineStage) -> u32 {
            self.context
        }
        fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
            if let Some(code) = self.failure {
                return Err(ModelRuntimeFailure {
                    code: code.into(),
                    message: "admission failed".into(),
                    recoverable: false,
                    request_attempts: vec![],
                });
            }
            if crate::pipeline::qwen_tokenizer::request_fits_context(
                self.input,
                request.max_output_tokens,
                self.context,
            ) {
                Ok(())
            } else {
                Err(ModelRuntimeFailure {
                    code: "MODEL_CONTEXT_EXCEEDED".into(),
                    message: "full".into(),
                    recoverable: false,
                    request_attempts: vec![],
                })
            }
        }
    }

    fn request() -> ModelRequest {
        let schema = json!({"properties":{"units":{"maxItems":8,"items":{"properties":{
            "text":{"maxLength":1200}, "source_ids":{"maxItems":8,"items":{"enum":
                (1..=10).map(|n| format!("s{n}{}", "x".repeat(n))).collect::<Vec<_>>()}}
        }}}}});
        summary_request(SummaryProfile::General, "sources", &schema, 0, 1)
    }

    fn schema(request: &ModelRequest) -> &Value {
        match &request.output_format {
            ModelOutputFormat::JsonSchema { schema, .. } => schema,
            _ => panic!("schema required"),
        }
    }

    #[test]
    fn exact_capacity_and_serialized_unicode_bounds_agree() {
        let runtime = CapacityRuntime {
            context: 32_768,
            input: 20_395,
            failure: None,
        };
        let admitted = admit(&runtime, &request(), true).unwrap().unwrap();
        let fields = &schema(&admitted)["properties"]["units"]["items"]["properties"];
        let length = fields["text"]["maxLength"].as_u64().unwrap() as usize;
        assert!(length < MAX_UNIT_CHARACTERS);
        assert!(admitted.max_output_tokens > OUTPUT_TOKENS);
        let bound = response_tokens(schema(&admitted), length).unwrap();
        assert_eq!(bound, admitted.max_output_tokens);
        let next = response_tokens(schema(&admitted), length + 1).unwrap();
        assert!(
            runtime.input + next + REPAIR_FEEDBACK_TOKENS + TOKENIZER_FRAMING_RESERVE_TOKENS
                > runtime.context
        );
        let tokenizer = QwenPromptTokenizer::conservative_byte_counter().unwrap();
        let ids: Vec<_> = fields["source_ids"]["items"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .take(8)
            .cloned()
            .collect();
        for character in ['x', '\u{0001}', '\n', '"', '\\', '\u{10ffff}', '界'] {
            let text = character.to_string().repeat(length);
            let payload =
                json!({"units":vec![json!({"text":text,"source_ids":ids});8]}).to_string();
            assert!(tokenizer.count(&payload).unwrap() + FORMATTING_TOKENS <= bound);
            validate_response(&admitted, &payload).unwrap();
        }
        let too_long =
            json!({"units":[{"text":"x".repeat(length+1),"source_ids":["s1x"]}]}).to_string();
        assert!(validate_response(&admitted, &too_long).is_err());
    }

    #[test]
    fn exhausted_context_and_non_context_failures_do_not_generate() {
        for context in [
            0,
            1,
            OUTPUT_TOKENS,
            OUTPUT_TOKENS + REPAIR_FEEDBACK_TOKENS + TOKENIZER_FRAMING_RESERVE_TOKENS - 1,
        ] {
            let runtime = CapacityRuntime {
                context,
                input: 0,
                failure: None,
            };
            assert!(admit(&runtime, &request(), true).unwrap().is_none());
        }
        let runtime = CapacityRuntime {
            context: 32_768,
            input: 0,
            failure: Some("MODEL_PROFILE_STALE"),
        };
        assert_eq!(
            admit(&runtime, &request(), true).unwrap_err().code,
            "MODEL_PROFILE_STALE"
        );
        let mut malformed = request();
        if let ModelOutputFormat::JsonSchema { schema, .. } = &mut malformed.output_format {
            schema["properties"]["units"]["maxItems"] = json!(0);
        }
        assert_eq!(
            admit(&runtime, &malformed, true).unwrap_err().code,
            "INVALID_SYNTHESIS_BUDGET"
        );
    }

    #[test]
    fn repair_keeps_shape_and_readmits_instead_of_shrinking() {
        let runtime = CapacityRuntime {
            context: 32_768,
            input: 20_395,
            failure: None,
        };
        let first = admit(&runtime, &request(), true).unwrap().unwrap();
        let repair = admit(&runtime, &first, false).unwrap().unwrap();
        assert_eq!(schema(&repair), schema(&first));
        let full = CapacityRuntime {
            context: 32_768,
            input: 21_000,
            failure: None,
        };
        assert!(admit(&full, &first, false).unwrap().is_none());
        assert!(admit(&full, &first, true).unwrap().is_some());
    }
}
