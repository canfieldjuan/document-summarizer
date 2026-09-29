//! One admission decision for the coherent response shape and its token allowance.
use super::*;
use crate::pipeline::qwen_tokenizer::TOKENIZER_FRAMING_RESERVE_TOKENS;

const FORMATTING_TOKENS: u32 = 256;
const REPAIR_FEEDBACK_TOKENS: u32 = 512;
// Pinned Qwen3.5 retained-response p99 = 0.22922983626440266. Rounded up.
// See the calibration and stress limitations in PR-SYNTHESIS-OUTPUT-BUDGET.md.
// This estimates normal text; verified completion, not this ratio, is safety.
const TEXT_TOKENS_NUMERATOR: usize = 23;
const TEXT_TOKENS_DENOMINATOR: usize = 100;

fn invalid_budget() -> PipelineFailure {
    stage_failure(
        PipelineStage::Synthesize,
        "INVALID_SYNTHESIS_BUDGET",
        "The synthesis response shape cannot be budgeted",
        false,
    )
}

/// Bound structural overhead with canonical bytes, including the longest
/// permitted distinct IDs. Text is calibrated separately, not multiplied by
/// a worst-case escape expansion.
fn response_structure(schema: &Value, characters: usize) -> Option<(usize, usize)> {
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
    Some((count, serde_json::to_vec(&empty).ok()?.len()))
}

fn response_tokens(schema: &Value, characters: usize) -> Option<u32> {
    let (count, overhead) = response_structure(schema, characters)?;
    let text_tokens = count
        .checked_mul(characters)?
        .checked_mul(TEXT_TOKENS_NUMERATOR)?
        .checked_add(TEXT_TOKENS_DENOMINATOR - 1)?
        / TEXT_TOKENS_DENOMINATOR;
    u32::try_from(overhead.checked_add(text_tokens)?)
        .ok()?
        .checked_add(FORMATTING_TOKENS)
        .map(|n| n.max(OUTPUT_TOKENS))
}

/// Diagnostic only: pathological serialization is intentionally not a decoder
/// ceiling selector. Such output can exceed the estimate and must fail closed
/// at the runtime stop boundary.
#[cfg(test)]
fn canonical_response_byte_bound(schema: &Value, characters: usize) -> Option<usize> {
    let (count, overhead) = response_structure(schema, characters)?;
    overhead.checked_add(count.checked_mul(characters)?.checked_mul(6)?)
}

/// Search the actual runtime boundary, including backend framing and wire caps.
/// Initial feedback headroom is not sent as part of the generation allowance.
fn available_output(
    runtime: &dyn ModelRuntime,
    request: &ModelRequest,
    reserve_feedback: bool,
) -> Result<Option<u32>, PipelineFailure> {
    let reserve = if reserve_feedback {
        REPAIR_FEEDBACK_TOKENS
    } else {
        0
    };
    let Some(mut high) = runtime
        .context_tokens(PipelineStage::Synthesize)
        .checked_sub(TOKENIZER_FRAMING_RESERVE_TOKENS)
        .and_then(|n| n.checked_sub(reserve))
    else {
        return Ok(None);
    };
    let mut low = OUTPUT_TOKENS;
    let mut admitted = None;
    let mut probe = request.clone();
    while low <= high {
        let tokens = low + (high - low) / 2;
        probe.max_output_tokens = tokens.checked_add(reserve).ok_or_else(invalid_budget)?;
        if !request_exceeds_runtime_context(runtime, &probe)? {
            admitted = Some(tokens);
            low = tokens + 1;
        } else {
            high = tokens - 1;
        }
    }
    Ok(admitted)
}

pub(super) fn admit(
    runtime: &dyn ModelRuntime,
    request: &ModelRequest,
    resize: bool,
) -> Result<Option<ModelRequest>, PipelineFailure> {
    let ModelOutputFormat::JsonSchema { schema, .. } = &request.output_format else {
        return Err(invalid_budget());
    };
    let maximum = maximum_characters(request)?;
    let estimate = response_tokens(schema, maximum).ok_or_else(invalid_budget)?;
    let Some(capacity) = available_output(runtime, request, resize)? else {
        return Ok(None);
    };
    let mut candidate = request.clone();
    if estimate > capacity {
        if !resize {
            return Ok(None);
        }
        let mut low = 1;
        let mut high = maximum;
        let mut ceiling = None;
        while low <= high {
            let characters = low + (high - low) / 2;
            if response_tokens(schema, characters).ok_or_else(invalid_budget)? <= capacity {
                ceiling = Some(characters);
                low = characters + 1;
            } else {
                high = characters - 1;
            }
        }
        let Some(characters) = ceiling else {
            return Ok(None);
        };
        let ModelOutputFormat::JsonSchema { schema, .. } = &mut candidate.output_format else {
            unreachable!("the request shape was checked above");
        };
        schema["properties"]["units"]["items"]["properties"]["text"]["maxLength"] =
            json!(characters);
        // A changed schema can change the input token count on schema-bearing
        // transports. Re-admit that final request, including feedback headroom.
        let Some(final_capacity) = available_output(runtime, &candidate, true)? else {
            return Ok(None);
        };
        let ModelOutputFormat::JsonSchema { schema, .. } = &candidate.output_format else {
            unreachable!("the request shape was checked above");
        };
        if response_tokens(schema, characters).ok_or_else(invalid_budget)? > final_capacity {
            return Ok(None);
        }
        candidate.max_output_tokens = final_capacity;
    } else {
        candidate.max_output_tokens = capacity;
    }
    Ok(Some(candidate))
}

/// The admitted schema owns the ceiling for parsing and every recovery path.
pub(super) fn maximum_characters(request: &ModelRequest) -> Result<usize, PipelineFailure> {
    let ModelOutputFormat::JsonSchema { schema, .. } = &request.output_format else {
        return Err(invalid_budget());
    };
    schema["properties"]["units"]["items"]["properties"]["text"]["maxLength"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0 && *n <= MAX_UNIT_CHARACTERS)
        .ok_or_else(invalid_budget)
}

pub(super) fn validate_response(
    request: &ModelRequest,
    response: &str,
) -> Result<(), PipelineFailure> {
    let maximum = maximum_characters(request)?;
    let raw: RawResponse = serde_json::from_str(response).map_err(|_| invalid_response())?;
    if raw
        .units
        .iter()
        .any(|unit| unit.text.chars().count() > maximum)
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
    fn b_and_small_32k_prompts_preserve_1200_character_sections() {
        for input in [20_395, 4_000, 1_500] {
            let runtime = CapacityRuntime {
                context: 32_768,
                input,
                failure: None,
            };
            let admitted = admit(&runtime, &request(), true).unwrap().unwrap();
            assert_eq!(
                schema(&admitted)["properties"]["units"]["items"]["properties"]["text"]
                    ["maxLength"],
                json!(MAX_UNIT_CHARACTERS),
                "fitting 32k response must preserve the existing paragraph ceiling; input={input}"
            );
        }
    }

    #[test]
    fn fitting_8k_shapes_keep_the_existing_ceiling_and_all_available_output() {
        let mut request = request();
        if let ModelOutputFormat::JsonSchema { schema, .. } = &mut request.output_format {
            // Production IDs are s + ordinal, including real B's 146 sources.
            schema["properties"]["units"]["items"]["properties"]["source_ids"]["items"]["enum"] =
                json!((1..=146).map(|n| format!("s{n}")).collect::<Vec<_>>());
        }
        for (context, input) in [(32_768, 20_395), (8_192, 4_000), (8_192, 2_000)] {
            let runtime = CapacityRuntime {
                context,
                input,
                failure: None,
            };
            let admitted = admit(&runtime, &request, true).unwrap().unwrap();
            assert_eq!(schema(&admitted), schema(&request));
            assert_eq!(
                admitted.max_output_tokens,
                context - input - TOKENIZER_FRAMING_RESERVE_TOKENS - REPAIR_FEEDBACK_TOKENS
            );
            let mut probe = admitted.clone();
            probe.max_output_tokens += REPAIR_FEEDBACK_TOKENS;
            runtime.preflight_request(&probe).unwrap();
            probe.max_output_tokens += 1;
            assert!(runtime.preflight_request(&probe).is_err());
        }
    }

    #[test]
    fn measured_shortfall_alone_reduces_shape_and_locally_enforces_it() {
        let request = request();
        let required = response_tokens(schema(&request), MAX_UNIT_CHARACTERS).unwrap();
        for deficit in [0, 1, 500] {
            let runtime = CapacityRuntime {
                context: 8192,
                input: 8192 - TOKENIZER_FRAMING_RESERVE_TOKENS - REPAIR_FEEDBACK_TOKENS - required
                    + deficit,
                failure: None,
            };
            let admitted = admit(&runtime, &request, true).unwrap().unwrap();
            let fields = &schema(&admitted)["properties"]["units"]["items"]["properties"];
            let length = fields["text"]["maxLength"].as_u64().unwrap() as usize;
            assert_eq!(length == MAX_UNIT_CHARACTERS, deficit == 0);
            assert_eq!(admitted.max_output_tokens, required - deficit);
            assert!(
                response_tokens(schema(&admitted), length).unwrap() <= admitted.max_output_tokens
            );
            if deficit > 0 {
                assert!(
                    response_tokens(schema(&admitted), length + 1).unwrap()
                        > admitted.max_output_tokens
                );
            }
            let payload =
                json!({"units":[{"text":"x".repeat(length),"source_ids":["s1x"]}]}).to_string();
            validate_response(&admitted, &payload).unwrap();
            let too_long =
                json!({"units":[{"text":"x".repeat(length+1),"source_ids":["s1x"]}]}).to_string();
            assert!(validate_response(&admitted, &too_long).is_err());
        }
    }

    #[test]
    fn pathological_byte_bound_is_diagnostic_not_an_admission_rule() {
        let request = request();
        let diagnostic =
            canonical_response_byte_bound(schema(&request), MAX_UNIT_CHARACTERS).unwrap();
        assert!(diagnostic > 32_768);
        let runtime = CapacityRuntime {
            context: 32_768,
            input: 20_395,
            failure: None,
        };
        let admitted = admit(&runtime, &request, true).unwrap().unwrap();
        assert_eq!(schema(&admitted), schema(&request));
        let text = "\u{0001}\\\"\n界\u{10ffff}".repeat(200);
        let payload =
            json!({"units":vec![json!({"text":text,"source_ids":["s1x"]});8]}).to_string();
        let tokenizer = QwenPromptTokenizer::conservative_byte_counter().unwrap();
        assert!(tokenizer.count(&payload).unwrap() > admitted.max_output_tokens);
        assert!(payload.len() <= diagnostic);
        // A schema-valid answer is not guaranteed to fit. Adapter regressions
        // independently require a qualified completion before returning text.
        validate_response(&admitted, &payload).unwrap();
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
    fn output_cap_is_capacity_not_an_unrelated_runtime_failure() {
        struct CappedRuntime(CapacityRuntime);
        impl ModelRuntime for CappedRuntime {
            fn generate(&self, _: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
                panic!("no inference")
            }
            fn health(&self) -> Result<(), ModelRuntimeFailure> {
                Ok(())
            }
            fn runtime_id(&self) -> &str {
                self.0.runtime_id()
            }
            fn model_id(&self) -> &str {
                self.0.model_id()
            }
            fn context_tokens(&self, stage: PipelineStage) -> u32 {
                self.0.context_tokens(stage)
            }
            fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
                if request.max_output_tokens > crate::pipeline::gateway_client::MAX_OUTPUT_TOKENS {
                    Err(ModelRuntimeFailure {
                        code: "MODEL_OUTPUT_BUDGET_EXCEEDED".into(),
                        message: "cap".into(),
                        recoverable: false,
                        request_attempts: vec![],
                    })
                } else {
                    self.0.preflight_request(request)
                }
            }
        }
        let runtime = CappedRuntime(CapacityRuntime {
            context: 8192,
            input: 1000,
            failure: None,
        });
        let admitted = admit(&runtime, &request(), true).unwrap().unwrap();
        assert!(admitted.max_output_tokens > OUTPUT_TOKENS);
        assert!(
            admitted.max_output_tokens + REPAIR_FEEDBACK_TOKENS
                <= crate::pipeline::gateway_client::MAX_OUTPUT_TOKENS
        );
        runtime.preflight_request(&admitted).unwrap();
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
        let required = response_tokens(schema(&first), MAX_UNIT_CHARACTERS).unwrap();
        let full = CapacityRuntime {
            context: 32_768,
            input: 32_768 - TOKENIZER_FRAMING_RESERVE_TOKENS - required + 1,
            failure: None,
        };
        assert!(admit(&full, &first, false).unwrap().is_none());
        assert!(admit(&full, &first, true).unwrap().is_some());
        assert_eq!(schema(&first), schema(&request()));
    }
}
