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
                .unwrap(),
            prepared.schema
        );
    }
    assert_eq!(approved, 4);
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

#[test]
#[ignore = "explicit frozen 9B candidate, private output directory, exclusive GPU and driver inference lock required"]
fn c9_live_approved_controls_use_production_runtime() {
    use crate::pipeline::llama_cpp::{self, GgufRuntimeConfig, LlamaCppRuntime};
    use crate::pipeline::model_settings::{
        JACK_LLAMA_CPP_LIBRARIES, QUALIFIED_LLAMA_SERVER_DIGEST,
    };
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    struct Shutdown;
    impl Drop for Shutdown {
        fn drop(&mut self) {
            llama_cpp::shutdown_managed_runtimes();
        }
    }
    struct Recording {
        inner: Arc<LlamaCppRuntime>,
        responses: Mutex<Vec<Value>>,
    }
    impl ModelRuntime for Recording {
        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            self.generate_with_control(request, &UNCONTROLLED_EXECUTION)
        }
        fn generate_with_control(
            &self,
            request: &ModelRequest,
            control: &dyn ExecutionControl,
        ) -> Result<ModelResponse, ModelRuntimeFailure> {
            let result = self.inner.generate_with_control(request, control);
            self.responses.lock().unwrap().push(match &result {
                Ok(response) => json!({"text":response.text,"runtime_id":response.runtime_id,
                    "model_id":response.model_id,"request_attempts":response.request_attempts}),
                Err(error) => json!({"error":error.code,"message":error.message,"request_attempts":error.request_attempts}),
            });
            result
        }
        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            self.inner.health()
        }
        fn runtime_id(&self) -> &str {
            self.inner.runtime_id()
        }
        fn model_id(&self) -> &str {
            self.inner.model_id()
        }
        fn context_tokens(&self, stage: PipelineStage) -> u32 {
            self.inner.context_tokens(stage)
        }
        fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
            self.inner.preflight_request(request)
        }
    }
    let output = PathBuf::from(std::env::var("DOC_SUM_C9_LIVE_OUTPUT").unwrap());
    assert!(output.is_dir() && !output.join("results.json").exists());
    let save = |name: &str, value: &Value| {
        std::fs::write(output.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
    };
    let model = PathBuf::from(std::env::var("DOC_SUM_QUALIFICATION_GGUF").unwrap());
    let (model_path, size, digest, identity) = llama_cpp::inspect_regular_file(&model).unwrap();
    assert_eq!(
        digest,
        "cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13"
    );
    save(
        "identities.json",
        &json!({"model":digest,"file_identity":identity,
        "server":QUALIFIED_LLAMA_SERVER_DIGEST,"context_tokens":32768,"thinking":false,
        "libraries":JACK_LLAMA_CPP_LIBRARIES.iter().map(|f| json!({"name":f.file_name,"digest":f.digest})).collect::<Vec<_>>()}),
    );
    // A short private scratch root avoids Unix-domain-socket path length limits.
    // All review evidence is written to the durable output directory above.
    let scratch = tempfile::Builder::new()
        .prefix("docsum-c9-")
        .tempdir()
        .unwrap();
    let _shutdown = Shutdown;
    let runtime = Recording {
        inner: LlamaCppRuntime::shared(GgufRuntimeConfig {
            model_path,
            runtime_parent: scratch.path().into(),
            model_digest: digest,
            expected_size_bytes: size,
            expected_file_identity: identity,
            expected_server_digest: QUALIFIED_LLAMA_SERVER_DIGEST.into(),
            expected_runtime_libraries: JACK_LLAMA_CPP_LIBRARIES,
            context_tokens: 32768,
        })
        .unwrap(),
        responses: Mutex::new(vec![]),
    };
    let f = fixture();
    let mut results = Vec::new();
    for record in f["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["operator_expected_admit"].is_boolean())
    {
        let (prompt, claims) = input(record);
        let batches = plan(&runtime, &prompt, &claims, 8).unwrap();
        let mut ordinal = results.len() as u32;
        let request = verification_request(&batches[0], ordinal, 7);
        let ModelOutputFormat::JsonSchema { name, schema } = &request.output_format else {
            unreachable!();
        };
        save(
            &format!("request-{ordinal}.json"),
            &json!({"system_prompt":request.system_prompt,
            "user_prompt":request.user_prompt,"seed":request.seed,"max_output_tokens":request.max_output_tokens,
            "schema_name":name,"schema":schema}),
        );
        runtime.preflight_request(&request).unwrap();
        let started = std::time::Instant::now();
        let actual = classify_verification_batches(
            &runtime,
            batches,
            7,
            &mut ordinal,
            &UNCONTROLLED_EXECUTION,
        );
        let raw = runtime.responses.lock().unwrap().last().unwrap().clone();
        save(&format!("response-{}.json", results.len()), &raw);
        let expected = record["operator_expected_admit"].as_bool().unwrap();
        let verdict = actual.as_ref().ok().map(|v| v[0].verdict.clone());
        let passed = actual.is_ok() && (verdict == Some(ClaimVerdict::Supported)) == expected;
        let row = json!({"case":record["case"],"expected_admit":expected,"verdict":verdict,"passed":passed,
            "error":actual.err().map(|e| e.code),"milliseconds":started.elapsed().as_millis()});
        println!("C9_PRODUCTION_CONTROL {row}");
        results.push(row);
        save("results.json", &json!(results));
    }
    assert_eq!(results.len(), 4);
    assert!(results.iter().all(|row| row["passed"] == true));
}
