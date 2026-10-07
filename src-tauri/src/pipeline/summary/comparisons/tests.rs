use super::*;

struct Runtime {
    schema_limit: usize,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            schema_limit: crate::pipeline::contracts::MAX_CLAIM_COMPARISON_SCHEMA_BYTES,
        }
    }
}

impl ModelRuntime for Runtime {
    fn generate(&self, _: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        panic!("planning must not generate")
    }
    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        Ok(())
    }
    fn runtime_id(&self) -> &str {
        "fixture"
    }
    fn model_id(&self) -> &str {
        "fixture"
    }
    fn context_tokens(&self, _: PipelineStage) -> u32 {
        32_768
    }
    fn response_schema_byte_limit(&self, _: PipelineStage, _: &str) -> usize {
        self.schema_limit
    }
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/c9-replay.json")).unwrap()
}

fn input(record: &Value) -> (VerificationPrompt, Vec<CitedClaim>) {
    let wire = &record["user_prompt"];
    let c = &wire["claims"][0];
    let evidence = c["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            let context = wire["clause_contexts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|context| context["context_id"] == e["clause_context_id"])
                .unwrap();
            PromptVerificationEvidence {
                evidence_id: e["evidence_id"].as_str().unwrap().into(),
                exact_quote: e["exact_quote"].as_str().unwrap().into(),
                full_clause: Some(context["full_clause"].as_str().unwrap().into()),
            }
        })
        .collect::<Vec<_>>();
    let claim = CitedClaim {
        claim_id: "durable-claim".into(),
        text: c["text"].as_str().unwrap().into(),
        evidence_ids: evidence.iter().map(|e| e.evidence_id.clone()).collect(),
    };
    (
        VerificationPrompt {
            claims: vec![PromptVerificationClaim {
                claim_id: claim.claim_id.clone(),
                text: claim.text.clone(),
                source_framing: None,
                evidence,
            }],
        },
        vec![claim],
    )
}

#[test]
fn c9_recorded_public_responses_match_production_schema_prompt_and_verdict() {
    let fixture = fixture();
    let mut approved = 0;
    for record in fixture["records"].as_array().unwrap() {
        let (prompt, claims) = input(record);
        let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
        assert_eq!(batches.len(), 1);
        let batch = &batches[0];
        let request = verification_request(batch, 0, 7);
        assert_eq!(request.max_output_tokens, 4096);
        assert_eq!(
            request.system_prompt,
            fixture["system_prompt"].as_str().unwrap()
        );
        assert_eq!(
            serde_json::from_str::<Value>(&request.user_prompt).unwrap(),
            record["user_prompt"]
        );
        let prepared = batch.comparison.as_ref().unwrap();
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&prepared.schema).unwrap())
            ),
            record["schema_sha256"].as_str().unwrap(),
            "{}",
            record["case"]
        );
        let decoded = prepared
            .parse(record["raw_response"].as_str().unwrap(), &claims[0])
            .unwrap();
        assert_eq!(
            serde_json::to_value(&decoded.verdict).unwrap(),
            record["recorded_verdict"],
            "{}",
            record["case"]
        );
        assert_eq!(decoded.claim_id, claims[0].claim_id);
        if let Some(expected) = record["operator_expected_admit"].as_bool() {
            approved += 1;
            assert_eq!(
                decoded.verdict == ClaimVerdict::Supported,
                expected,
                "{}",
                record["case"]
            );
        }
        // Exercise the actual native schema admission, not just the planner.
        assert_eq!(
            crate::pipeline::model::response_format(&request.output_format)
                .unwrap()
                .unwrap()
                .as_value(),
            &prepared.schema
        );
    }
    assert_eq!(approved, 4);
}

#[test]
fn c9_decoder_wire_preserves_frozen_comparison_order() {
    let f = fixture();
    let (prompt, claims) = input(&f["records"][0]);
    let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
    let request = verification_request(&batches[0], 0, 7);
    let schema = crate::pipeline::model::response_format(&request.output_format)
        .unwrap()
        .unwrap();
    let wire = serde_json::to_string(&schema).unwrap();
    for names in [
        vec!["stage", "conditions", "qualifiers", "scope"],
        vec!["source_spans", "claim_spans", "relation"],
    ] {
        let offsets = names
            .iter()
            .map(|name| wire.find(&format!("\"{name}\":{{")).unwrap())
            .collect::<Vec<_>>();
        assert!(
            offsets.windows(2).all(|pair| pair[0] < pair[1]),
            "decoder property order drifted: {names:?} at {offsets:?}"
        );
    }
}

#[test]
fn c9_parser_rejects_partial_foreign_wrong_side_and_invented_spans() {
    let fixture = fixture();
    let record = &fixture["records"][0];
    let (prompt, claims) = input(record);
    let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
    let batch = &batches[0];
    let prepared = batch.comparison.as_ref().unwrap();
    let raw = record["raw_response"].as_str().unwrap();
    let valid: Value = serde_json::from_str(raw).unwrap();
    assert!(prepared.parse(raw, &claims[0]).is_ok());
    let mut bad = Vec::new();
    for id in ["durable-claim", "k2", "k01", "e1", "clause-context-1", ""] {
        let mut v = valid.clone();
        v["verdicts"][0]["claim_id"] = json!(id);
        bad.push(v);
    }
    for dim in DIMENSIONS {
        let mut v = valid.clone();
        v["verdicts"][0]["comparisons"]
            .as_object_mut()
            .unwrap()
            .remove(dim);
        bad.push(v);
    }
    for value in [
        json!(null),
        json!(false),
        json!(0),
        json!(""),
        json!([]),
        json!({}),
    ] {
        let mut v = valid.clone();
        v["verdicts"][0]["comparisons"]["scope"] = value;
        bad.push(v);
    }
    for field in ["source_spans", "claim_spans"] {
        for spans in [
            json!(["made up"]),
            json!([""]),
            json!(["ayment"]),
            json!(vec!["payment"; 5]),
        ] {
            let mut v = valid.clone();
            v["verdicts"][0]["comparisons"]["stage"][field] = spans;
            bad.push(v);
        }
    }
    let mut wrong_side = valid.clone();
    wrong_side["verdicts"][0]["comparisons"]["stage"]["source_spans"] = json!(["Final payment"]);
    bad.push(wrong_side);
    for verdicts in [
        json!([]),
        json!([valid["verdicts"][0], valid["verdicts"][0]]),
    ] {
        bad.push(json!({"verdicts":verdicts}));
    }
    let mut extra = valid.clone();
    extra["verdicts"][0]["verdict"] = json!("supported");
    bad.push(extra);
    let mut extra = valid.clone();
    extra["verdicts"][0]["comparisons"]["other"] = json!({});
    bad.push(extra);
    for v in bad {
        assert!(prepared.parse(&v.to_string(), &claims[0]).is_err(), "{v}");
    }
    let duplicate = raw.replacen("\"claim_id\":", "\"claim_id\":\"k1\",\"claim_id\":", 1);
    assert!(prepared.parse(&duplicate, &claims[0]).is_err());
    assert!(prepared
        .parse(
            r#"{"verdicts":[{"claim_id":"k1","verdict":"supported"}]}"#,
            &claims[0]
        )
        .is_err());
}

#[test]
fn c9_relations_derive_verdict_and_enforce_both_span_sides() {
    let fixture = fixture();
    let (prompt, claims) = input(&fixture["records"][0]);
    let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
    let batch = &batches[0];
    let prepared = batch.comparison.as_ref().unwrap();
    let mut response: Value =
        serde_json::from_str(&fixture_response(&verification_request(batch, 0, 7), false)).unwrap();
    for (relation, source, target, expected) in [
        ("preserved", true, true, Some(ClaimVerdict::Supported)),
        ("changed", true, true, Some(ClaimVerdict::Unsupported)),
        ("omitted", true, false, Some(ClaimVerdict::Unsupported)),
        ("uncertain", true, false, Some(ClaimVerdict::Ambiguous)),
        ("uncertain", false, true, Some(ClaimVerdict::Ambiguous)),
        (
            "not_applicable",
            false,
            false,
            Some(ClaimVerdict::Supported),
        ),
        ("preserved", true, false, None),
        ("changed", false, true, None),
        ("omitted", false, true, None),
        ("uncertain", false, false, None),
        ("not_applicable", true, false, None),
        ("unknown", true, true, None),
    ] {
        response["verdicts"][0]["comparisons"]["conditions"] = json!({
            "relation":relation,
            "source_spans":if source {vec![prepared.source_spans.first().unwrap().clone()]} else {vec![]},
            "claim_spans":if target {vec![prepared.claim_spans.first().unwrap().clone()]} else {vec![]}
        });
        let actual = prepared.parse(&response.to_string(), &claims[0]);
        assert_eq!(
            actual.ok().map(|v| v.verdict),
            expected,
            "{relation}/{source}/{target}"
        );
    }
}

#[test]
fn c9_no_applicable_comparison_cannot_support_claim() {
    let f = fixture();
    let (prompt, claims) = input(&f["records"][0]);
    let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
    let prepared = batches[0].comparison.as_ref().unwrap();
    let empty = json!({"source_spans":[],"claim_spans":[],"relation":"not_applicable"});
    let mut response = json!({"verdicts":[{"claim_id":"k1","comparisons":{
        "stage":empty,"conditions":empty,"qualifiers":empty,"scope":empty}}]});
    let actual = prepared
        .parse(&response.to_string(), &claims[0])
        .unwrap()
        .verdict;
    println!("all_not_applicable_verdict={actual:?}");
    assert_eq!(
        actual,
        ClaimVerdict::Ambiguous,
        "empty comparisons cannot prove support"
    );
    for dimension in DIMENSIONS {
        for (relation, expected) in [
            ("preserved", ClaimVerdict::Supported),
            ("changed", ClaimVerdict::Unsupported),
            ("omitted", ClaimVerdict::Unsupported),
            ("uncertain", ClaimVerdict::Ambiguous),
        ] {
            response["verdicts"][0]["comparisons"][dimension] = json!({
                "relation":relation, "source_spans":[prepared.source_spans.first().unwrap()],
                "claim_spans":[prepared.claim_spans.first().unwrap()]});
            assert_eq!(
                prepared
                    .parse(&response.to_string(), &claims[0])
                    .unwrap()
                    .verdict,
                expected
            );
        }
        response["verdicts"][0]["comparisons"][dimension] = empty.clone();
    }
}

#[test]
fn c9_catalog_preserves_unicode_punctuation_and_token_boundaries() {
    let catalog = span_catalog(&["A\tB, café e\u{301} 一二 ² _x \u{1c}Z"]).unwrap();
    for yes in [
        "A\tB", "B, café", "café", "e", "\u{301}", "e\u{301}", "一二", "²", "_x", "Z",
    ] {
        assert!(catalog.contains(yes), "{yes}");
    }
    for no in ["A B", "caf", "一", "x", "CAFÉ"] {
        assert!(!catalog.contains(no), "{no}");
    }
    assert!(span_catalog(&[&"x".repeat(MAX_SPAN_CHARACTERS)]).is_ok());
    assert!(span_catalog(&[&"x".repeat(MAX_SPAN_CHARACTERS + 1)]).is_err());
}

#[test]
fn c9_single_token_over_span_limit_uses_admission_fallback() {
    let f = fixture();
    let (mut prompt, claims) = input(&f["records"][0]);
    let source = "x".repeat(MAX_SPAN_CHARACTERS + 1);
    prompt.claims[0].evidence[0].exact_quote = source.clone();
    prompt.claims[0].evidence[0].full_clause = Some(source);
    assert_eq!(
        plan(&Runtime::default(), &prompt, &claims, 8)
            .unwrap_err()
            .code,
        "VERIFICATION_INPUT_TOO_LARGE"
    );
}

#[test]
fn c9_runtime_schema_limit_and_total_input_limits_fail_closed() {
    let fixture = fixture();
    let (prompt, claims) = input(&fixture["records"][0]);
    let batch = plan(&Runtime::default(), &prompt, &claims, 8)
        .unwrap()
        .remove(0);
    let size = serde_json::to_vec(&batch.comparison.unwrap().schema)
        .unwrap()
        .len();
    for limit in [0, size - 1] {
        assert_eq!(
            plan(
                &Runtime {
                    schema_limit: limit
                },
                &prompt,
                &claims,
                8
            )
            .unwrap_err()
            .code,
            "VERIFICATION_INPUT_TOO_LARGE"
        );
    }
    for limit in [size, size + 1] {
        assert!(plan(
            &Runtime {
                schema_limit: limit
            },
            &prompt,
            &claims,
            8
        )
        .is_ok());
    }
    let mut input = prompt.claims[0].clone();
    input.text = "B".into();
    input.evidence[0].exact_quote = "A".into();
    for count in [
        MAX_INPUT_CHARACTERS - 2,
        MAX_INPUT_CHARACTERS - 1,
        MAX_INPUT_CHARACTERS,
    ] {
        input.evidence[0].full_clause = Some(format!("A{}", " ".repeat(count - 1)));
        assert_eq!(
            Prepared::new(&input, usize::MAX).is_ok(),
            count < MAX_INPUT_CHARACTERS
        );
    }
    input.evidence[0].full_clause = Some(
        (0..250)
            .map(|n| format!("w{n}"))
            .collect::<Vec<_>>()
            .join(" "),
    );
    input.evidence[0].exact_quote = "w0".into();
    assert_eq!(
        Prepared::new(&input, usize::MAX).unwrap_err().code,
        "VERIFICATION_INPUT_TOO_LARGE"
    );
    for text in ["", " \t\n"] {
        input.text = text.into();
        assert!(Prepared::new(&input, usize::MAX).is_err());
    }
}

#[test]
fn c9_multiple_claims_keep_passages_and_local_ids_isolated() {
    let fixture = fixture();
    let records = fixture["records"].as_array().unwrap();
    let (mut prompt, mut claims) = input(&records[0]);
    let (mut second, mut next) = input(records.last().unwrap());
    second.claims[0].claim_id = "other-durable".into();
    next[0].claim_id = "other-durable".into();
    prompt.claims.extend(second.claims);
    claims.extend(next);
    let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
    assert_eq!(batches.len(), 2);
    for batch in &batches {
        assert_eq!(batch.identifiers.vocabulary(), &["k1"]);
    }
    assert!(!batches[0].user_prompt.contains("temporary equipment"));
    assert!(!batches[1].user_prompt.contains("Substantial Completion"));
    let raw = records[0]["raw_response"].as_str().unwrap();
    assert!(batches[1]
        .comparison
        .as_ref()
        .unwrap()
        .parse(raw, &claims[1])
        .is_err());
}

#[test]
fn c9_cancellation_stops_at_request_boundaries() {
    use crate::pipeline::control::CancellationToken;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Cancelling {
        token: CancellationToken,
        calls: AtomicUsize,
    }
    impl ModelRuntime for Cancelling {
        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.token.request();
            Ok(ModelResponse {
                text: fixture_response(request, false),
                runtime_id: "fixture".into(),
                model_id: "fixture".into(),
                request_attempts: vec![],
            })
        }
        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }
        fn runtime_id(&self) -> &str {
            "fixture"
        }
        fn model_id(&self) -> &str {
            "fixture"
        }
    }
    let f = fixture();
    let (mut prompt, mut claims) = input(&f["records"][0]);
    let mut second = prompt.claims[0].clone();
    second.claim_id = "second".into();
    let mut next = claims[0].clone();
    next.claim_id = "second".into();
    prompt.claims.push(second);
    claims.push(next);
    let runtime = Cancelling {
        token: CancellationToken::new(),
        calls: AtomicUsize::new(0),
    };
    let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
    let mut ordinal = 0;
    let error = classify_verification_batches(&runtime, batches, 7, &mut ordinal, &runtime.token)
        .unwrap_err();
    assert!(cancellation_observed(&error));
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
    let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
    assert!(cancellation_observed(
        &classify_verification_batches(&runtime, batches, 7, &mut ordinal, &runtime.token)
            .unwrap_err()
    ));
    assert_eq!(runtime.calls.load(Ordering::SeqCst), 1);
}

fn piece_covers_reference(piece: &str, reference: &str) -> bool {
    if reference.is_empty() || reference.starts_with(whitespace) || reference.ends_with(whitespace)
    {
        return false;
    }
    piece.char_indices().any(|(start, _)| {
        let end = start + reference.len();
        piece[start..].starts_with(reference)
            && !(piece[..start]
                .chars()
                .next_back()
                .is_some_and(word_character)
                && reference.chars().next().is_some_and(word_character))
            && !(reference.chars().next_back().is_some_and(word_character)
                && piece[end..].chars().next().is_some_and(word_character))
    })
}

#[test]
fn passage_coverage_requires_exact_bytes_and_whole_token_boundaries() {
    for (piece, reference, expected) in [
        ("Pay $270 now.", "$27", false),
        ("Pay $27 now.", "$27", true),
        ("Pay $270, then $27.", "$27", true),
        ("Payments are excluded.", "Payment", false),
        ("A\tB applies.", "A B", false),
        ("A\tB applies.", "A\tB", true),
        ("café applies.", "cafe\u{301}", false),
        ("X appears.", "", false),
    ] {
        assert_eq!(
            piece_covers_reference(piece, reference),
            expected,
            "{piece:?}/{reference:?}"
        );
    }
}

fn repeated_claims(count: usize) -> (VerificationPrompt, Vec<CitedClaim>) {
    let f = fixture();
    let (prompt, claims) = input(&f["records"][0]);
    let mut repeated = VerificationPrompt { claims: vec![] };
    let mut owned = vec![];
    for n in 0..count {
        let mut input = prompt.claims[0].clone();
        let mut claim = claims[0].clone();
        input.claim_id = format!("durable-{n}");
        claim.claim_id.clone_from(&input.claim_id);
        repeated.claims.push(input);
        owned.push(claim);
    }
    (repeated, owned)
}

#[test]
fn joint_executor_preflights_all_and_returns_only_complete_owned_results() {
    use crate::pipeline::control::CancellationToken;
    use std::sync::Mutex;
    struct Sequenced {
        preflights: Mutex<Vec<u32>>,
        calls: Mutex<Vec<u32>>,
        reject: Option<u32>,
        invalid: Option<u32>,
        cancel: Option<u32>,
        token: CancellationToken,
    }
    impl ModelRuntime for Sequenced {
        fn preflight_request(&self, r: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
            self.preflights.lock().unwrap().push(r.ordinal);
            if self.reject == Some(r.ordinal) {
                return Err(ModelRuntimeFailure {
                    code: "MODEL_CONTEXT_EXCEEDED".into(),
                    message: "fixture".into(),
                    recoverable: false,
                    request_attempts: vec![],
                });
            }
            Ok(())
        }
        fn generate(&self, r: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            assert_eq!(
                *self.preflights.lock().unwrap(),
                (10..12).collect::<Vec<_>>()
            );
            self.calls.lock().unwrap().push(r.ordinal);
            if self.cancel == Some(r.ordinal) {
                self.token.request();
            }
            Ok(ModelResponse {
                text: if self.invalid == Some(r.ordinal) {
                    "{}".into()
                } else {
                    fixture_response(r, r.ordinal == 11)
                },
                runtime_id: "fixture".into(),
                model_id: "fixture".into(),
                request_attempts: vec![],
            })
        }
        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }
        fn runtime_id(&self) -> &str {
            "fixture"
        }
        fn model_id(&self) -> &str {
            "fixture"
        }
    }
    for (reject, invalid, cancel) in [
        (None, None, None),
        (Some(11), None, None),
        (None, Some(11), None),
        (None, None, Some(10)),
        (None, None, Some(11)),
    ] {
        {
            let runtime = Sequenced {
                preflights: Mutex::new(vec![]),
                calls: Mutex::new(vec![]),
                reject,
                invalid,
                cancel,
                token: CancellationToken::new(),
            };
            let (prompt, claims) = repeated_claims(2);
            let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
            let mut ordinal = 10;
            let result =
                classify_verification_batches(&runtime, batches, 7, &mut ordinal, &runtime.token);
            let expected_calls = if reject.is_some() {
                0
            } else if invalid.is_some() {
                2
            } else if let Some(cancel) = cancel {
                cancel - 10 + 1
            } else {
                2
            };
            assert_eq!(ordinal, 10 + expected_calls);
            assert_eq!(
                *runtime.calls.lock().unwrap(),
                (10..10 + expected_calls).collect::<Vec<_>>()
            );
            if reject.is_some() || invalid.is_some() || cancel.is_some() {
                assert!(result.is_err());
            } else {
                let verdicts = result.unwrap();
                assert_eq!(verdicts.len(), claims.len());
                for (index, (verdict, claim)) in verdicts.iter().zip(&claims).enumerate() {
                    assert_eq!(verdict.claim_id, claim.claim_id);
                    assert_eq!(verdict.evidence_ids, claim.evidence_ids);
                    assert_eq!(
                        verdict.verdict,
                        if index + 1 == claims.len() {
                            ClaimVerdict::Unsupported
                        } else {
                            ClaimVerdict::Supported
                        }
                    );
                }
            }
        }
    }
}

#[test]
fn excerpt_enum_boundaries_admit_exact_max_and_reject_next_value() {
    let symbols: String = (0..127)
        .map(|i| char::from_u32(0x2600 + i).unwrap())
        .collect();
    assert!(symbols
        .chars()
        .all(|c| !word_character(c) && !whitespace(c)));
    assert_eq!(span_catalog(&[&symbols]).unwrap().len(), 8128);
    let singletons: Vec<String> = (0..65)
        .map(|i| char::from_u32(0x2800 + i).unwrap().to_string())
        .collect();
    for (extra, expected) in [(63, Some(8191)), (64, Some(8192)), (65, None)] {
        let inputs: Vec<&str> = std::iter::once(symbols.as_str())
            .chain(singletons[..extra].iter().map(String::as_str))
            .collect();
        assert_eq!(span_catalog(&inputs).ok().map(|v| v.len()), expected);
    }
    assert!(span_catalog(&[]).is_err());
    assert!(span_catalog(&["", " "]).is_err());
    assert_eq!(span_catalog(&["a"]).unwrap().len(), 1);
}

#[test]
fn joint_executor_rejects_empty_mixed_duplicate_and_rebound_claim_plans() {
    let (prompt, claims) = repeated_claims(2);
    for change in 0..5 {
        let mut batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
        match change {
            0 => batches.clear(),
            1 => batches[1].comparison = None,
            2 => batches[1].claims[0].text = "foreign".into(),
            3 => batches[1].claims[0].evidence_ids.clear(),
            4 => {
                let first = batches[0].claims[0].clone();
                batches[1].claims[0] = first.clone();
                batches[1].comparison.as_mut().unwrap().claim = first;
            }
            _ => unreachable!(),
        }
        let mut ordinal = 0;
        let result = classify(
            &Runtime::default(),
            batches,
            7,
            &mut ordinal,
            &UNCONTROLLED_EXECUTION,
        );
        // Planning-only Runtime panics if an invalid plan reaches generation.
        assert!(result.is_err());
        assert_eq!(ordinal, 0);
    }
}

#[path = "parity.rs"]
mod parity;
#[path = "qualification.rs"]
mod qualification;
