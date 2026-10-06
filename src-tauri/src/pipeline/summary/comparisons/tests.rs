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

fn recorded_segments(prepared: &Prepared, raw: &str) -> Result<Value, PipelineFailure> {
    let mut response: Value = serde_json::from_str(raw).map_err(|_| invalid_response())?;
    for dim in DIMENSIONS {
        for (side, catalog) in [
            ("source_spans", &prepared.source_spans),
            ("claim_spans", &prepared.claim_spans),
        ] {
            let mut selected = Vec::new();
            for span in response["verdicts"][0]["comparisons"][dim][side]
                .as_array()
                .ok_or_else(invalid_response)?
            {
                let reference = span.as_str().ok_or_else(invalid_response)?;
                let piece = catalog
                    .iter()
                    .find(|piece| piece_covers_reference(piece, reference))
                    .ok_or_else(invalid_response)?;
                if !selected.contains(piece) {
                    selected.push(piece.clone());
                }
            }
            response["verdicts"][0]["comparisons"][dim][side] = json!(selected);
        }
    }
    Ok(response)
}

#[test]
fn segment_terminal_groups_stay_with_their_sentence() {
    for text in [
        "Ask now?! Next sentence.",
        "Ask now!?! Next sentence.",
        "He said, \"Stop?!\" Next sentence.",
    ] {
        let pieces: Vec<_> = segment_ranges(text)
            .unwrap()
            .into_iter()
            .map(|(a, b)| &text[a..b])
            .collect();
        let boundary = text.find(" Next").unwrap();
        assert_eq!(pieces, vec![&text[..boundary], "Next sentence."]);
    }
}

#[test]
fn segment_boundaries_preserve_punctuated_lowercase_sentences() {
    let text = "w000 w001. w002 w003.";
    let actual: Vec<&str> = segment_ranges(text)
        .unwrap()
        .into_iter()
        .map(|(a, b)| &text[a..b])
        .collect();
    assert_eq!(actual, vec!["w000 w001.", "w002 w003."]);
}

#[test]
fn segment_catalog_admits_sentence_inputs_rejected_by_excerpt_expansion() {
    let f = fixture();
    for sentence_count in [5, 12, 30] {
        let (mut prompt, mut claims) = input(&f["records"][0]);
        let source = (0..sentence_count)
            .map(|sentence| {
                format!(
                    "{}.",
                    (sentence * 8..sentence * 8 + 8)
                        .map(|n| format!("w{n:03}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        prompt.claims[0].text = "Recorded items are listed.".into();
        claims[0].text = prompt.claims[0].text.clone();
        prompt.claims[0].evidence[0].exact_quote = "w000".into();
        prompt.claims[0].evidence[0].full_clause = Some(source.clone());
        let planned = plan(
            &Runtime {
                schema_limit: crate::pipeline::gateway_client::MAX_SCHEMA_BYTES,
            },
            &prompt,
            &claims,
            8,
        );
        assert!(
            planned.is_ok(),
            "{sentence_count} bounded sentences should fit: {:?}",
            planned.err()
        );
        let request = verification_request(&planned.unwrap()[0], 0, 7);
        let wire: Value = serde_json::from_str(&request.user_prompt).unwrap();
        assert_eq!(wire["clause_contexts"][0]["full_clause"], source);
    }
}

#[test]
fn c9_recorded_controls_fit_segments_and_keep_relation_aggregation() {
    let fixture = fixture();
    let mut approved = 0;
    for record in fixture["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["operator_expected_admit"].is_boolean())
    {
        let (prompt, claims) = input(record);
        let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
        assert_eq!(batches.len(), 1);
        let batch = &batches[0];
        let request = verification_request(batch, 0, 7);
        assert_eq!(request.max_output_tokens, 4096);
        let prepared = batch.comparison.as_ref().unwrap();
        let mut wire: Value = serde_json::from_str(&request.user_prompt).unwrap();
        assert_eq!(wire["source_segments"], json!(prepared.source_spans));
        assert_eq!(wire["claim_segments"], json!(prepared.claim_spans));
        wire.as_object_mut().unwrap().remove("source_segments");
        wire.as_object_mut().unwrap().remove("claim_segments");
        assert_eq!(wire, record["user_prompt"]);
        let response =
            recorded_segments(prepared, record["raw_response"].as_str().unwrap()).unwrap();
        let accuracy = passage_accuracy(
            prepared,
            &response.to_string(),
            record["raw_response"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(accuracy["coverage_complete"], true);
        println!(
            "C9_CATALOG_ORACLE {}",
            json!({"case":record["case"],"source_pieces":prepared.source_spans.len(),"claim_pieces":prepared.claim_spans.len(),"schema_bytes":serde_json::to_vec(&prepared.schema).unwrap().len(),"accuracy":accuracy})
        );
        let decoded = prepared.parse(&response.to_string(), &claims[0]).unwrap();
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
    let raw = fixture_response(&verification_request(batch, 0, 7), false);
    let valid: Value = serde_json::from_str(&raw).unwrap();
    assert!(prepared.parse(&raw, &claims[0]).is_ok());
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
fn segment_catalog_preserves_exact_unicode_and_complete_pieces() {
    let text = "A\tB, café e\u{301} 一二 ² _x \u{1c}Z";
    let catalog = segment_catalog(&[text]).unwrap();
    assert_eq!(catalog, BTreeSet::from([text.to_string()]));
    for partial in ["A B", "café", "caf", "一", "x", "CAFÉ"] {
        assert!(!catalog.contains(partial));
    }
    for count in [239, 240, 241] {
        assert_eq!(
            segment_catalog(&[&"字".repeat(count)]).is_ok(),
            count <= 240
        );
    }
}

#[test]
fn segments_keep_all_source_and_reject_unsplittable_restrictions() {
    let cases = [
        "Dr. Smith pays $27.50. Work starts after approval.",
        "A\tB stays exact.\nWrapped text stays\non one sentence.",
        "Rule applies.\n\nExcept for temporary equipment.",
        "項目を守る。 次も守る。",
        "First sentence. A bounded unfinished tail",
    ];
    for text in cases {
        let ranges = segment_ranges(text).unwrap();
        let kept: String = ranges.iter().map(|&(a, b)| &text[a..b]).collect();
        assert_eq!(
            kept.chars().filter(|c| !whitespace(*c)).collect::<String>(),
            text.chars().filter(|c| !whitespace(*c)).collect::<String>()
        );
        assert!(ranges.windows(2).all(|r| r[0].1 <= r[1].0));
        assert!(ranges
            .iter()
            .all(|&(a, b)| text[a..b].chars().count() <= 240));
    }
    for separator in ["; ", ": ", "\n\n"] {
        let text = format!("{}{}{}", "a ".repeat(80), separator, "b ".repeat(80));
        let ranges = segment_ranges(&text).unwrap();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges, segment_ranges(&text).unwrap());
    }
    let long = format!(
        "{}unless separately authorized",
        "Work continues ".repeat(30)
    );
    assert_eq!(
        segment_ranges(&long).unwrap_err().message,
        SEGMENTATION_FAILURE_MESSAGE
    );
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

fn owned_passages(prepared: &Prepared, raw: &str) -> Result<Value, PipelineFailure> {
    let response: Value = serde_json::from_str(raw).map_err(|_| invalid_response())?;
    let verdicts = response["verdicts"]
        .as_array()
        .ok_or_else(invalid_response)?;
    if verdicts.len() != 1 {
        return Err(invalid_response());
    }
    for dim in DIMENSIONS {
        for (side, catalog) in [
            ("source_spans", &prepared.source_spans),
            ("claim_spans", &prepared.claim_spans),
        ] {
            let spans = verdicts[0]["comparisons"][dim][side]
                .as_array()
                .ok_or_else(invalid_response)?;
            if spans.len() > MAX_SPANS
                || spans
                    .iter()
                    .any(|s| s.as_str().is_none_or(|s| !catalog.contains(s)))
            {
                return Err(invalid_response());
            }
        }
    }
    Ok(response)
}

fn passage_accuracy(
    prepared: &Prepared,
    response: &str,
    recorded: &str,
) -> Result<Value, PipelineFailure> {
    let actual = owned_passages(prepared, response)?;
    let expected: Value = serde_json::from_str(recorded).map_err(|_| invalid_response())?;
    let mut sides = Vec::new();
    let mut total_matches = 0;
    let mut total_expected = 0;
    let mut total_actual = 0;
    let mut total_covered = 0;
    for dim in DIMENSIONS {
        for side in ["source_spans", "claim_spans"] {
            let wanted = expected["verdicts"][0]["comparisons"][dim][side]
                .as_array()
                .ok_or_else(invalid_response)?;
            let got = actual["verdicts"][0]["comparisons"][dim][side]
                .as_array()
                .ok_or_else(invalid_response)?;
            let mut remaining = wanted.clone();
            let mut matches = 0;
            for span in got {
                if let Some(position) = remaining.iter().position(|s| s == span) {
                    remaining.remove(position);
                    matches += 1;
                }
            }
            let covered = wanted
                .iter()
                .filter(|reference| {
                    got.iter().any(|piece| {
                        piece_covers_reference(piece.as_str().unwrap(), reference.as_str().unwrap())
                    })
                })
                .count();
            total_covered += covered;
            total_matches += matches;
            total_expected += wanted.len();
            total_actual += got.len();
            sides.push(json!({"dimension":dim,"side":side,"recorded":wanted,"reconstructed":got,
                "covered_recorded_spans":covered,"coverage_complete":covered==wanted.len(),"exact_list_match":wanted==got,"exact_span_matches":matches,"missing":wanted.len()-matches,"extra":got.len()-matches}));
        }
    }
    Ok(
        json!({"exact_side_matches":sides.iter().filter(|s| s["exact_list_match"]==true).count(),
        "side_count":sides.len(),"exact_span_matches":total_matches,"recorded_span_count":total_expected,
        "generated_span_count":total_actual,"covered_recorded_span_count":total_covered,"coverage_complete":total_covered==total_expected,"sides":sides}),
    )
}

fn live_control_observation(
    prepared: &Prepared,
    record: &Value,
    raw: &Value,
    actual: Result<Vec<ClaimVerification>, PipelineFailure>,
    milliseconds: u128,
) -> Value {
    let expected = record["operator_expected_admit"].as_bool().unwrap();
    let verdict = actual.as_ref().ok().map(|v| v[0].verdict.clone());
    let verdict_passed = actual.is_ok() && (verdict == Some(ClaimVerdict::Supported)) == expected;
    let accuracy = raw["text"].as_str().and_then(|text| {
        passage_accuracy(prepared, text, record["raw_response"].as_str().unwrap()).ok()
    });
    let passage_review_required = accuracy
        .as_ref()
        .is_none_or(|a| a["exact_side_matches"] != a["side_count"]);
    let coverage_passed = accuracy
        .as_ref()
        .is_some_and(|a| a["coverage_complete"] == true);
    json!({"coverage_passed":coverage_passed,"gate_passed":verdict_passed && coverage_passed,"case":record["case"],"expected_admit":expected,"verdict":verdict,"verdict_parity_passed":verdict_passed,
        "response_valid":actual.is_ok(),"passage_membership_valid":accuracy.is_some(),
        "passage_review_required":passage_review_required,
        "passage_accuracy":accuracy,"recorded_c9_response":record["raw_response"],
        "error":actual.err().map(|e| e.code),"milliseconds":milliseconds})
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
        inner: Arc<dyn ModelRuntime>,
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
        fn response_schema_byte_limit(&self, stage: PipelineStage, name: &str) -> usize {
            self.inner.response_schema_byte_limit(stage, name)
        }
        fn supports_response_schema(&self, name: &str) -> bool {
            self.inner.supports_response_schema(name)
        }
    }
    let output = PathBuf::from(std::env::var("DOC_SUM_C9_LIVE_OUTPUT").unwrap());
    assert!(output.is_dir() && !output.join("results.json").exists());
    let save = |name: &str, value: &Value| {
        std::fs::write(output.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
    };
    let route = std::env::var("DOC_SUM_C9_RUNTIME").unwrap_or_else(|_| "native".into());
    let scratch = tempfile::Builder::new()
        .prefix("docsum-c9-")
        .tempdir()
        .unwrap();
    let _shutdown = Shutdown;
    let inner: Arc<dyn ModelRuntime> = if route == "gateway" {
        use crate::pipeline::{
            db, gateway_client::GatewayClientConfig, gateway_runtime::GatewayRuntime,
            service::admit_pdf_for_background,
        };
        let settings: Value = serde_json::from_slice(
            &std::fs::read(std::env::var("DOC_SUM_C9_GATEWAY_SETTINGS").unwrap()).unwrap(),
        )
        .unwrap();
        let gateway = &settings["gateway"];
        let db_path = output.join("gateway-ledger.sqlite");
        let mut runtime = GatewayRuntime::new(
            db_path.clone(),
            GatewayClientConfig {
                base_url: gateway["baseUrl"].as_str().unwrap().into(),
                token_file: gateway["tokenFile"].as_str().unwrap().into(),
                ca_file: gateway["caFile"].as_str().unwrap().into(),
                timeout: std::time::Duration::from_secs(900),
                request_lifetime: std::time::Duration::from_secs(900),
            },
        )
        .unwrap();
        assert_eq!(runtime.context_tokens(PipelineStage::Verify), 32768);
        assert!(runtime.supports_response_schema(SCHEMA_NAME));
        let snapshot = runtime.profile_snapshot().unwrap();
        save(
            "identities.json",
            &json!({"route":route,"snapshot":snapshot}),
        );
        let mut conn = db::init_db(&db_path).unwrap();
        let pdf =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let (_, run) = admit_pdf_for_background(
            &mut conn,
            pdf.to_str().unwrap(),
            Some(&snapshot),
            SummaryProfile::General,
            None,
        )
        .unwrap();
        runtime.bind_request_owner(&run.run_id);
        Arc::new(runtime)
    } else {
        assert_eq!(route, "native");
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

        LlamaCppRuntime::shared(GgufRuntimeConfig {
            model_path,
            runtime_parent: scratch.path().into(),
            model_digest: digest,
            expected_size_bytes: size,
            expected_file_identity: identity,
            expected_server_digest: QUALIFIED_LLAMA_SERVER_DIGEST.into(),
            expected_runtime_libraries: JACK_LLAMA_CPP_LIBRARIES,
            context_tokens: 32768,
        })
        .unwrap()
    };
    let runtime = Recording {
        inner,
        responses: Mutex::new(vec![]),
    };
    let boundaries = measure_admission_boundaries(&runtime);
    save("admission-boundaries.json", &boundaries);
    println!("C9_ADMISSION_BOUNDARIES {boundaries}");
    if let Ok(path) = std::env::var("DOC_SUM_C9_REPLAY_INPUT") {
        let inputs: Vec<PromptVerificationClaim> =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let mut rows = Vec::new();
        for (index, input) in inputs.into_iter().enumerate() {
            let claim = CitedClaim {
                claim_id: input.claim_id.clone(),
                text: input.text.clone(),
                evidence_ids: input
                    .evidence
                    .iter()
                    .map(|e| e.evidence_id.clone())
                    .collect(),
            };
            let planned = plan(
                &runtime,
                &VerificationPrompt {
                    claims: vec![input],
                },
                &[claim],
                8,
            );
            let batches = match planned {
                Ok(batches) => batches,
                Err(error) => {
                    rows.push(json!({"unit":index+1,"admitted":false,"error":error.code,"message":error.message}));
                    continue;
                }
            };
            let request = verification_request(&batches[0], 0, 7);
            runtime.preflight_request(&request).unwrap();
            let ModelOutputFormat::JsonSchema { name, schema } = &request.output_format else {
                unreachable!();
            };
            save(
                &format!("public-B-request-{}.json", index + 1),
                &json!({"system_prompt":request.system_prompt,"user_prompt":request.user_prompt,"schema_name":name,"schema":schema}),
            );
            rows.push(json!({"unit":index+1,"admitted":true,"prompt_characters":request.system_prompt.chars().count()+request.user_prompt.chars().count(),"schema_bytes":serde_json::to_vec(schema).unwrap().len()}));
        }
        save("public-B-admission.json", &json!(rows));
        println!("C9_PUBLIC_B_ADMISSION {}", json!(rows));
    }
    if std::env::var("DOC_SUM_C9_BOUNDARIES_ONLY").as_deref() == Ok("1") {
        save(
            "results.json",
            &json!({"boundaries_only":true,"inference_calls":0}),
        );
        return;
    }
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
            "schema_name":name,"schema":schema,
            "decoder_schema_json":serde_json::to_string(&crate::pipeline::model::response_format(&request.output_format).unwrap().unwrap()).unwrap()}),
        );
        runtime.preflight_request(&request).unwrap();
        let prepared = Prepared::new(
            &prompt.claims[0],
            runtime.response_schema_byte_limit(PipelineStage::Verify, SCHEMA_NAME),
        )
        .unwrap();
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
        let row = live_control_observation(
            &prepared,
            record,
            &raw,
            actual,
            started.elapsed().as_millis(),
        );
        println!("C9_PRODUCTION_CONTROL {row}");
        results.push(row);
        save("results.json", &json!(results));
    }
    assert_eq!(results.len(), 4);
    save(
        "control-gate.json",
        &json!({
            "verdict_parity_passed":results.iter().all(|row| row["verdict_parity_passed"]==true),
            "coverage_passed":results.iter().all(|row| row["coverage_passed"]==true),
            "gate_passed":results.iter().all(|row| row["gate_passed"]==true),
            "passage_review_required":results.iter().any(|row| row["passage_review_required"]==true),
            "fidelity_qualified":false
        }),
    );
    assert!(results.iter().all(|row| row["gate_passed"] == true));
}

#[test]
fn passage_accuracy_separates_containment_exactness_and_relation_validity() {
    let f = fixture();
    let record = f["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["case"] == "equipment-restored-restriction")
        .unwrap();
    let (prompt, claims) = input(record);
    let prepared = Prepared::new(&prompt.claims[0], usize::MAX).unwrap();
    let response = recorded_segments(&prepared, record["raw_response"].as_str().unwrap()).unwrap();
    let observe = |response: &Value| {
        let text = response.to_string();
        let actual = prepared.parse(&text, &claims[0]).map(|v| vec![v]);
        live_control_observation(&prepared, record, &json!({"text":text}), actual, 0)
    };
    let good = observe(&response);
    assert_eq!(good["gate_passed"], true);
    assert_eq!(good["passage_membership_valid"], true);
    assert_eq!(good["coverage_passed"], true);
    assert_ne!(
        good["passage_accuracy"]["exact_span_matches"],
        good["passage_accuracy"]["recorded_span_count"]
    );
    for relation in ["preserved", "not_a_relation"] {
        let mut bad_relation = response.clone();
        bad_relation["verdicts"][0]["comparisons"]["stage"]["relation"] = json!(relation);
        if relation == "preserved" {
            bad_relation["verdicts"][0]["comparisons"]["stage"]["source_spans"] = json!([]);
        }
        let row = observe(&bad_relation);
        assert_eq!(row["passage_membership_valid"], true);
        assert_eq!(row["response_valid"], false);
        assert_eq!(row["gate_passed"], false);
    }
    let mut near_copy = response.clone();
    near_copy["verdicts"][0]["comparisons"]["stage"]["source_spans"][0] =
        json!("The contractor indemnifies the owner for losses from borrowed equipment.");
    let row = observe(&near_copy);
    assert_eq!(row["passage_membership_valid"], false);
    assert_eq!(row["response_valid"], false);
    assert_eq!(row["gate_passed"], false);
    let pieces: Vec<_> = prepared.source_spans.iter().cloned().collect();
    assert_eq!(pieces.len(), 2);
    let mut joined_reference: Value =
        serde_json::from_str(record["raw_response"].as_str().unwrap()).unwrap();
    joined_reference["verdicts"][0]["comparisons"]["stage"]["source_spans"] =
        json!([pieces.join(" ")]);
    let mut separately_selected = response;
    separately_selected["verdicts"][0]["comparisons"]["stage"]["source_spans"] = json!(pieces);
    let accuracy = passage_accuracy(
        &prepared,
        &separately_selected.to_string(),
        &joined_reference.to_string(),
    )
    .unwrap();
    assert_eq!(accuracy["coverage_complete"], false);
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

fn sizing_input(family: &str, total_characters: usize) -> (VerificationPrompt, Vec<CitedClaim>) {
    let text = "Work needs approval.";
    let n = total_characters - text.chars().count();
    assert!(n >= 2);
    let source = match family {
        "sparse" => format!("A{}B", " ".repeat(n - 2)),
        "dense" => format!("A{}", ",".repeat(n - 1)),
        "prose" => format!(
            "A {}",
            "work starts only after written approval. ".repeat(n)
        )
        .chars()
        .take(n)
        .collect(),
        _ => panic!("unknown size family"),
    };
    let claim = CitedClaim {
        claim_id: "size-claim".into(),
        text: text.into(),
        evidence_ids: vec!["size-evidence".into()],
    };
    let prompt = VerificationPrompt {
        claims: vec![PromptVerificationClaim {
            claim_id: claim.claim_id.clone(),
            text: claim.text.clone(),
            source_framing: None,
            evidence: vec![PromptVerificationEvidence {
                evidence_id: "size-evidence".into(),
                exact_quote: "A".into(),
                full_clause: Some(source),
            }],
        }],
    };
    (prompt, vec![claim])
}

fn sizing_observation(runtime: &dyn ModelRuntime, family: &str, size: usize) -> Value {
    let (prompt, claims) = sizing_input(family, size);
    let planned = plan(runtime, &prompt, &claims, 8);
    let mut row =
        json!({"family":family,"claim_and_unique_source_characters":size,"admitted":false});
    match planned {
        Ok(batches) => {
            let request = verification_request(&batches[0], 0, 7);
            row["projected_prompt_characters"] =
                json!(request.system_prompt.chars().count() + request.user_prompt.chars().count());
            row["schema_bytes"] = json!(serde_json::to_vec(
                &batches[0].comparison.as_ref().unwrap().schema
            )
            .unwrap()
            .len());
            match runtime.preflight_request(&request) {
                Ok(()) => row["admitted"] = json!(true),
                Err(e) => {
                    assert_eq!(
                        e.code, "MODEL_CONTEXT_EXCEEDED",
                        "unexpected preflight error: {e:?}"
                    );
                    row["error"] = json!(e.code);
                    row["message"] = json!(e.message);
                }
            }
        }
        Err(e) => {
            assert_eq!(
                e.code, "VERIFICATION_INPUT_TOO_LARGE",
                "unexpected planner error: {e:?}"
            );
            row["error"] = json!(e.code);
            row["message"] = json!(e.message);
            if let Ok(prepared) = Prepared::new(
                &prompt.claims[0],
                runtime.response_schema_byte_limit(PipelineStage::Verify, SCHEMA_NAME),
            ) {
                let (user, _) = prepared.prompt(&prompt.claims[0]).unwrap();
                row["projected_prompt_characters"]=json!(format!("{VERIFICATION_ENTAILMENT_INSTRUCTION}{CLAUSE_VERIFICATION_INSTRUCTION}\n{INSTRUCTION}").chars().count()+user.chars().count());
                row["schema_bytes"] = json!(serde_json::to_vec(&prepared.schema).unwrap().len());
            }
        }
    }
    row
}

fn measure_admission_boundaries(runtime: &dyn ModelRuntime) -> Value {
    let mut rows = Vec::new();
    for family in ["sparse", "prose", "dense"] {
        // These deterministic families grow by one source character at a time.
        // Search the fixed input domain; verify the actual neighboring requests.
        let mut low = 22;
        let mut high = MAX_INPUT_CHARACTERS + 1;
        assert_eq!(sizing_observation(runtime, family, low)["admitted"], true);
        assert_eq!(sizing_observation(runtime, family, high)["admitted"], false);
        while high - low > 1 {
            let mid = (high + low) / 2;
            if sizing_observation(runtime, family, mid)["admitted"] == true {
                low = mid;
            } else {
                high = mid;
            }
        }
        rows.push(json!({"family":family,"maximum_admitted":sizing_observation(runtime,family,low),"first_rejected":sizing_observation(runtime,family,high)}));
    }
    json!({"runtime_id":runtime.runtime_id(),"model_id":runtime.model_id(),"context_tokens":runtime.context_tokens(PipelineStage::Verify),"max_output_tokens":VERIFICATION_OUTPUT_TOKENS,"schema_byte_limit":runtime.response_schema_byte_limit(PipelineStage::Verify,SCHEMA_NAME),"input_character_limit":MAX_INPUT_CHARACTERS,"projected_prompt_character_limit":MAX_VERIFICATION_REQUEST_CHARACTERS,"families":rows})
}

#[test]
fn segment_projection_size_boundaries() {
    for (name, limit) in [
        ("gateway", crate::pipeline::gateway_client::MAX_SCHEMA_BYTES),
        (
            "native",
            crate::pipeline::contracts::MAX_CLAIM_COMPARISON_SCHEMA_BYTES,
        ),
    ] {
        let result = measure_admission_boundaries(&Runtime {
            schema_limit: limit,
        });
        println!("SEGMENT_PLANNER_BOUNDARY {name} {result}");
        for (row, expected) in result["families"]
            .as_array()
            .unwrap()
            .iter()
            .zip([260, 4096, 260])
        {
            assert_eq!(
                row["maximum_admitted"]["claim_and_unique_source_characters"],
                expected
            );
            assert_eq!(row["maximum_admitted"]["admitted"], true);
            assert_eq!(
                row["first_rejected"]["claim_and_unique_source_characters"],
                expected + 1
            );
            assert_eq!(row["first_rejected"]["admitted"], false);
        }
    }
}

#[test]
#[ignore = "explicit copied saved-artifact input required; no inference"]
fn segment_saved_views_reopen_unchanged() {
    let cases: Vec<Value> = serde_json::from_slice(
        &std::fs::read(std::env::var("DOC_SUM_C9_REOPEN_INPUT").unwrap()).unwrap(),
    )
    .unwrap();
    for case in cases {
        let conn = rusqlite::Connection::open_with_flags(
            case["database"].as_str().unwrap(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let view = crate::pipeline::workspace::get_persisted_summary(
            &conn,
            case["run_id"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(serde_json::to_value(view).unwrap(), case["view"]);
        println!("SEGMENT_SAVED_VIEW_UNCHANGED {}", case["alias"]);
    }
}
