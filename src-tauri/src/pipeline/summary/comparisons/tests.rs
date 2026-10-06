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

// Only test/replay data uses the historical four-dimension fixture wrapper.
// Production receives one object per request and cannot use this adapter.
fn parse_recorded_comparisons(
    prepared: &Prepared,
    response: &Value,
) -> Result<ClaimVerdict, PipelineFailure> {
    let selected: Vec<_> = DIMENSIONS
        .iter()
        .map(|dim| prepared.parse(&response["verdicts"][0]["comparisons"][dim].to_string()))
        .collect::<Result<_, _>>()?;
    Ok(aggregate(
        &selected.try_into().map_err(|_| invalid_response())?,
    ))
}

fn static_catalog_coverage(records: &[Value]) -> Value {
    let mut controls = Vec::new();
    for record in records
        .iter()
        .filter(|r| r["operator_expected_admit"].is_boolean())
    {
        let (prompt, claims) = input(record);
        let planned = plan(&Runtime::default(), &prompt, &claims, 8);
        let recorded: Value =
            serde_json::from_str(record["raw_response"].as_str().unwrap()).unwrap();
        let prepared = planned
            .as_ref()
            .ok()
            .and_then(|b| b.first())
            .and_then(|b| b.comparison.as_ref());
        let mut references = Vec::new();
        for dimension in DIMENSIONS {
            for side in ["source_spans", "claim_spans"] {
                for passage in recorded["verdicts"][0]["comparisons"][dimension][side]
                    .as_array()
                    .unwrap()
                {
                    let containing: Vec<_> = prepared
                        .into_iter()
                        .flat_map(|p| {
                            if side == "source_spans" {
                                &p.source_spans
                            } else {
                                &p.claim_spans
                            }
                        })
                        .filter(|piece| piece_covers_reference(piece, passage.as_str().unwrap()))
                        .collect();
                    references.push(json!({"dimension":dimension,"side":side,"recorded":passage,
                        "covered":!containing.is_empty(),"containing_pieces":containing}));
                }
            }
        }
        controls.push(json!({"case":record["case"],"admitted":planned.is_ok(),
            "error":planned.err().map(|e| e.code),
            "coverage_complete":references.iter().all(|r| r["covered"]==true),"references":references}));
    }
    json!({"gate_passed":controls.len()==4 && controls.iter().all(|c| c["admitted"]==true && c["coverage_complete"]==true),"controls":controls})
}

#[test]
fn static_coverage_gate_reports_every_reference_and_rejects_cross_piece_spans() {
    let f = fixture();
    let mut records = f["records"].as_array().unwrap().clone();
    let gate = static_catalog_coverage(&records);
    println!("C9_STATIC_COVERAGE {gate}");
    assert_eq!(gate["gate_passed"], true);
    assert_eq!(gate["controls"].as_array().unwrap().len(), 4);
    let record = records
        .iter_mut()
        .find(|r| r["operator_expected_admit"].is_boolean())
        .unwrap();
    let (prompt, _) = input(record);
    let source = prompt.claims[0].evidence[0].full_clause.as_ref().unwrap();
    assert!(segment_ranges(source).unwrap().len() > 1);
    let mut raw: Value = serde_json::from_str(record["raw_response"].as_str().unwrap()).unwrap();
    raw["verdicts"][0]["comparisons"]["stage"]["source_spans"] = json!([source]);
    record["raw_response"] = json!(raw.to_string());
    let failed = static_catalog_coverage(&records);
    assert_eq!(failed["gate_passed"], false);
    assert_eq!(failed["controls"][0]["coverage_complete"], false);
    assert_eq!(failed["controls"].as_array().unwrap().len(), 4);
    assert_eq!(
        failed["controls"][0]["references"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
}

#[test]
fn c9_recorded_controls_fit_segments_and_keep_relation_aggregation() {
    let f = fixture();
    let mut approved = 0;
    for record in f["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["operator_expected_admit"].is_boolean())
    {
        let (prompt, claims) = input(record);
        let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
        assert_eq!(batches.len(), 4);
        for (dimension, batch) in batches.iter().enumerate() {
            let request = verification_request(batch, dimension as u32, 7);
            let projected = fixture_verification_prompt(&request.user_prompt).unwrap();
            assert_eq!(projected.claims.len(), 1);
            assert_eq!(request.max_output_tokens, 4096);
            let prepared = batch.comparison.as_ref().unwrap();
            let mut wire: Value = serde_json::from_str(&request.user_prompt).unwrap();
            assert_eq!(wire["dimension"], DIMENSIONS[dimension]);
            assert_eq!(wire["source_segments"], json!(prepared.source_spans));
            assert_eq!(wire["claim_segments"], json!(prepared.claim_spans));
            for key in [
                "source_segments",
                "claim_segments",
                "dimension",
                "parent_claim_context",
            ] {
                wire.as_object_mut().unwrap().remove(key);
            }
            assert_eq!(wire, record["user_prompt"]);
            assert_eq!(
                crate::pipeline::model::response_format(&request.output_format)
                    .unwrap()
                    .unwrap()
                    .as_value(),
                &prepared.schema
            );
        }
        let prepared = batches[0].comparison.as_ref().unwrap();
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
        let verdict = parse_recorded_comparisons(prepared, &response).unwrap();
        assert_eq!(
            serde_json::to_value(&verdict).unwrap(),
            record["recorded_verdict"]
        );
        assert_eq!(
            verdict == ClaimVerdict::Supported,
            record["operator_expected_admit"].as_bool().unwrap()
        );
        approved += 1;
    }
    assert_eq!(approved, 4);
}

#[test]
fn c9_decoder_wire_preserves_comparison_field_order() {
    let f = fixture();
    let (prompt, claims) = input(&f["records"][0]);
    for batch in plan(&Runtime::default(), &prompt, &claims, 8).unwrap() {
        let request = verification_request(&batch, 0, 7);
        let wire = serde_json::to_string(
            &crate::pipeline::model::response_format(&request.output_format)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        let offsets = ["source_spans", "claim_spans", "relation"]
            .map(|name| wire.find(&format!("\"{name}\":{{")).unwrap());
        assert!(offsets.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(!wire.contains("\"comparisons\""));
        assert!(!wire.contains("\"verdicts\""));
    }
}

#[test]
fn c9_parser_rejects_partial_foreign_wrong_side_and_invented_spans() {
    let f = fixture();
    let (prompt, claims) = input(&f["records"][0]);
    let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
    let prepared = batches[0].comparison.as_ref().unwrap();
    let raw = fixture_response(&verification_request(&batches[0], 0, 7), false);
    let valid: Value = serde_json::from_str(&raw).unwrap();
    assert!(prepared.parse(&raw).is_ok());
    let mut bad = Vec::new();
    for key in ["source_spans", "claim_spans", "relation"] {
        let mut v = valid.clone();
        v.as_object_mut().unwrap().remove(key);
        bad.push(v);
    }
    for key in ["claim_id", "verdict", "comparisons", "other"] {
        let mut v = valid.clone();
        v[key] = json!("extra");
        bad.push(v);
    }
    for value in [
        json!(null),
        json!(false),
        json!(0),
        json!(""),
        json!([]),
        json!({}),
        json!("unknown"),
    ] {
        let mut v = valid.clone();
        v["relation"] = value;
        bad.push(v);
    }
    for field in ["source_spans", "claim_spans"] {
        for spans in [
            json!(["made up"]),
            json!([""]),
            json!(["ayment"]),
            json!(vec!["payment"; 5]),
            json!(false),
            json!(null),
        ] {
            let mut v = valid.clone();
            v[field] = spans;
            bad.push(v);
        }
    }
    let mut wrong_side = valid.clone();
    wrong_side["source_spans"] = valid["claim_spans"].clone();
    bad.push(wrong_side);
    for v in bad {
        assert!(prepared.parse(&v.to_string()).is_err(), "{v}");
    }
    let duplicate = raw.replacen(
        "\"relation\":",
        "\"relation\":\"preserved\",\"relation\":",
        1,
    );
    assert!(prepared.parse(&duplicate).is_err());
    assert!(prepared
        .parse(f["records"][0]["raw_response"].as_str().unwrap())
        .is_err());
}

#[test]
fn c9_relations_schema_and_parser_agree_on_independent_shape_matrix() {
    let f = fixture();
    let (prompt, _) = input(&f["records"][0]);
    let prepared = Prepared::new(&prompt.claims[0], usize::MAX).unwrap();
    let mut shape_cases = 0;
    for (relation, allowed) in [
        ("preserved", vec![(true, true)]),
        ("changed", vec![(true, true)]),
        ("omitted", vec![(true, false), (true, true)]),
        (
            "uncertain",
            vec![(false, true), (true, false), (true, true)],
        ),
        ("not_applicable", vec![(false, false)]),
    ] {
        for source in [0, 1, 4, 5] {
            for claim in [0, 1, 4, 5] {
                let response = json!({"source_spans":vec![prepared.source_spans.first().unwrap();source],
                "claim_spans":vec![prepared.claim_spans.first().unwrap();claim],"relation":relation});
                let expected =
                    source <= 4 && claim <= 4 && allowed.contains(&(source > 0, claim > 0));
                assert_eq!(
                    prepared.parse(&response.to_string()).is_ok(),
                    expected,
                    "Rust {relation}/{source}/{claim}"
                );
                assert_eq!(
                    schema_accepts(&prepared.schema, &prepared.schema, &response),
                    expected,
                    "schema {relation}/{source}/{claim}"
                );
                if source <= 1 && claim <= 1 {
                    shape_cases += 1;
                }
            }
        }
    }
    assert_eq!(shape_cases, 20);
}

#[test]
fn c9_no_applicable_comparison_cannot_support_claim() {
    let empty = || Comparison {
        source_spans: vec![],
        claim_spans: vec![],
        relation: Relation::NotApplicable,
    };
    let mut comparisons = std::array::from_fn(|_| empty());
    assert_eq!(aggregate(&comparisons), ClaimVerdict::Ambiguous);
    for dimension in 0..DIMENSIONS.len() {
        for (relation, expected) in [
            (Relation::Preserved, ClaimVerdict::Supported),
            (Relation::Changed, ClaimVerdict::Unsupported),
            (Relation::Omitted, ClaimVerdict::Unsupported),
            (Relation::Uncertain, ClaimVerdict::Ambiguous),
        ] {
            comparisons[dimension] = Comparison {
                source_spans: vec!["source".into()],
                claim_spans: vec!["claim".into()],
                relation,
            };
            assert_eq!(aggregate(&comparisons), expected);
        }
        comparisons[dimension] = empty();
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
        let text = "字".repeat(count);
        assert_eq!(segment_catalog(&[&text]).unwrap(), BTreeSet::from([text]));
    }
}

#[test]
fn segments_keep_all_source_and_preserve_unsplittable_restrictions() {
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
    assert_eq!(segment_ranges(&long).unwrap(), vec![(0, long.len())]);
}

#[test]
fn c9_single_token_over_target_uses_exact_sentence_fallback() {
    let f = fixture();
    let (mut prompt, claims) = input(&f["records"][0]);
    let source = "x".repeat(MAX_SPAN_CHARACTERS + 1);
    prompt.claims[0].evidence[0].exact_quote = source.clone();
    prompt.claims[0].evidence[0].full_clause = Some(source);
    assert!(plan(&Runtime::default(), &prompt, &claims, 8).is_ok());
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
    assert_eq!(batches.len(), 8);
    for batch in &batches {
        assert_eq!(batch.identifiers.vocabulary(), &["k1"]);
    }
    assert!(!batches[0].user_prompt.contains("temporary equipment"));
    assert!(!batches[4].user_prompt.contains("Substantial Completion"));
    let raw = records[0]["raw_response"].as_str().unwrap();
    assert!(batches[4].comparison.as_ref().unwrap().parse(raw).is_err());
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

// Reporting only: preserve each received dimension object verbatim in the
// historical scorer's container. Missing/duplicate/foreign dimensions fail.
fn scoring_wrapper(responses: &[Value]) -> Option<Value> {
    if responses.len() != DIMENSIONS.len() {
        return None;
    }
    let mut comparisons = serde_json::Map::new();
    for (expected, response) in DIMENSIONS.iter().zip(responses) {
        if response["dimension"].as_str() != Some(expected) {
            return None;
        }
        let decoded: Value = serde_json::from_str(response["text"].as_str()?).ok()?;
        comparisons.insert((*expected).into(), decoded);
    }
    Some(json!({"text":json!({"verdicts":[{"comparisons":comparisons}]}).to_string()}))
}

#[test]
fn scoring_wrapper_cannot_fill_missing_dimensions_or_repair_shapes() {
    let responses:Vec<_> = DIMENSIONS.iter().map(|d| json!({"dimension":d,"text":r#"{"source_spans":["owned"],"claim_spans":[],"relation":"not_applicable"}"#})).collect();
    let wrapped = scoring_wrapper(&responses).unwrap();
    let v: Value = serde_json::from_str(wrapped["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        v["verdicts"][0]["comparisons"]["stage"]["source_spans"],
        json!(["owned"])
    );
    assert_eq!(
        v["verdicts"][0]["comparisons"]["stage"]["relation"],
        "not_applicable"
    );
    assert!(scoring_wrapper(&responses[..3]).is_none());
    let mut wrong = responses.clone();
    wrong[1]["dimension"] = json!("stage");
    assert!(scoring_wrapper(&wrong).is_none());
    wrong[1]["dimension"] = json!("conditions");
    wrong[1]["text"] = json!("{");
    assert!(scoring_wrapper(&wrong).is_none());
}

fn live_control_observation(
    prepared: &Prepared,
    record: &Value,
    raw: &Value,
    actual: Result<Vec<ClaimVerification>, PipelineFailure>,
    milliseconds: u128,
) -> Value {
    let expected = record["operator_expected_admit"].as_bool().unwrap();
    let verdict = actual
        .as_ref()
        .ok()
        .filter(|v| v.len() == 1)
        .map(|v| v[0].verdict.clone());
    let verdict_passed = verdict.as_ref().is_some_and(|v| {
        serde_json::to_value(v).unwrap() == record["recorded_verdict"]
            && (*v == ClaimVerdict::Supported) == expected
    });
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
            let started = std::time::Instant::now();
            let result = self.inner.generate_with_control(request, control);
            let mut row = match &result {
                Ok(response) => json!({"text":response.text,"runtime_id":response.runtime_id,
                    "model_id":response.model_id,"request_attempts":response.request_attempts}),
                Err(error) => {
                    json!({"error":error.code,"message":error.message,"request_attempts":error.request_attempts})
                }
            };
            row["ordinal"] = json!(request.ordinal);
            row["milliseconds"] = json!(started.elapsed().as_millis());
            row["dimension"] = serde_json::from_str::<Value>(&request.user_prompt)
                .ok()
                .and_then(|v| v.get("dimension").cloned())
                .unwrap_or(Value::Null);
            self.responses.lock().unwrap().push(row);
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
    let f = fixture();
    let coverage = static_catalog_coverage(f["records"].as_array().unwrap());
    save("static-coverage.json", &coverage);
    assert_eq!(
        coverage["gate_passed"], true,
        "static coverage must pass before runtime or inference"
    );
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
            let mut dimensions = Vec::new();
            for (dimension, batch) in batches.iter().enumerate() {
                let request = verification_request(batch, dimension as u32, 7);
                let admission = runtime.preflight_request(&request);
                let ModelOutputFormat::JsonSchema { name, schema } = &request.output_format else {
                    unreachable!();
                };
                save(
                    &format!("public-B-request-{}-{dimension}.json", index + 1),
                    &json!({"system_prompt":request.system_prompt,"user_prompt":request.user_prompt,"schema_name":name,"schema":schema}),
                );
                dimensions.push(json!({"dimension":DIMENSIONS[dimension],"admitted":admission.is_ok(),
                    "error":admission.err().map(|e| json!({"code":e.code,"message":e.message})),
                    "prompt_characters":request.system_prompt.chars().count()+request.user_prompt.chars().count(),
                    "schema_bytes":serde_json::to_vec(schema).unwrap().len()}));
            }
            rows.push(json!({"unit":index+1,"admitted":dimensions.iter().all(|d| d["admitted"]==true),"dimensions":dimensions}));
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
    let mut ordinal = 0;
    for record in f["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["operator_expected_admit"].is_boolean())
    {
        let (prompt, claims) = input(record);
        let batches = plan(&runtime, &prompt, &claims, 8).unwrap();
        let start_response = runtime.responses.lock().unwrap().len();
        for (dimension, batch) in batches.iter().enumerate() {
            let request = verification_request(batch, ordinal + dimension as u32, 7);
            let ModelOutputFormat::JsonSchema { name, schema } = &request.output_format else {
                unreachable!();
            };
            save(
                &format!("request-{}.json", request.ordinal),
                &json!({
                "case":record["case"],"dimension":DIMENSIONS[dimension],"ordinal":request.ordinal,
                "system_prompt":request.system_prompt,"user_prompt":request.user_prompt,"seed":request.seed,
                "max_output_tokens":request.max_output_tokens,"schema_name":name,"schema":schema,
                "decoder_schema_json":serde_json::to_string(&crate::pipeline::model::response_format(&request.output_format).unwrap().unwrap()).unwrap()}),
            );
        }
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
        let raw = runtime.responses.lock().unwrap()[start_response..].to_vec();
        save(&format!("responses-{}.json", results.len()), &json!(raw));
        let scored = scoring_wrapper(&raw).unwrap_or(Value::Null);
        let mut row = live_control_observation(
            &prepared,
            record,
            &scored,
            actual,
            started.elapsed().as_millis(),
        );
        row["planned_generation_calls"] = json!(DIMENSIONS.len());
        row["actual_generation_calls"] = json!(raw.len());
        row["dimensions"] = json!(DIMENSIONS
            .iter()
            .enumerate()
            .map(|(n, dim)| json!({
            "dimension":dim,"executed":n<raw.len(),"raw":raw.get(n)}))
            .collect::<Vec<_>>());
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
        let actual = parse_recorded_comparisons(&prepared, response).map(|verdict| {
            vec![ClaimVerification {
                claim_id: claims[0].claim_id.clone(),
                evidence_ids: claims[0].evidence_ids.clone(),
                verdict,
            }]
        });
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
            let dimensions: Vec<_> = batches.iter().enumerate().map(|(dimension, batch)| {
                let request = verification_request(batch, dimension as u32, 7);
                let admission = runtime.preflight_request(&request);
                json!({"dimension":DIMENSIONS[dimension],
                    "projected_prompt_characters":request.system_prompt.chars().count()+request.user_prompt.chars().count(),
                    "schema_bytes":serde_json::to_vec(&batch.comparison.as_ref().unwrap().schema).unwrap().len(),
                    "admitted":admission.is_ok(),"error":admission.err().map(|e| json!({"code":e.code,"message":e.message}))})
            }).collect();
            row["admitted"] = json!(dimensions.iter().all(|d| d["admitted"] == true));
            row["dimensions"] = json!(dimensions);
        }
        Err(e) => {
            assert_eq!(
                e.code, "VERIFICATION_INPUT_TOO_LARGE",
                "unexpected planner error: {e:?}"
            );
            row["error"] = json!(e.code);
            row["message"] = json!(e.message);
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
            .zip([4096, 4096, 4096])
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

// Independent, deliberately limited JSON Schema evaluator for the generated
// comparison schema. Unsupported keywords fail the test rather than pass open.
fn schema_accepts(root: &Value, node: &Value, value: &Value) -> bool {
    for key in node.as_object().unwrap().keys() {
        assert!(
            [
                "$defs",
                "$ref",
                "anyOf",
                "type",
                "enum",
                "properties",
                "required",
                "additionalProperties",
                "items",
                "minItems",
                "maxItems"
            ]
            .contains(&key.as_str()),
            "unsupported test schema keyword: {key}"
        );
    }
    if let Some(reference) = node["$ref"].as_str() {
        return schema_accepts(
            root,
            root.pointer(reference.strip_prefix('#').unwrap()).unwrap(),
            value,
        );
    }
    if let Some(choices) = node["anyOf"].as_array() {
        return choices
            .iter()
            .any(|choice| schema_accepts(root, choice, value));
    }
    if let Some(choices) = node["enum"].as_array() {
        if !choices.contains(value) {
            return false;
        }
    }
    match node["type"].as_str().unwrap() {
        "string" => value.is_string(),
        "array" => value.as_array().is_some_and(|items| {
            items.len() >= node["minItems"].as_u64().unwrap_or(0) as usize
                && items.len() <= node["maxItems"].as_u64().unwrap_or(u64::MAX) as usize
                && items
                    .iter()
                    .all(|item| schema_accepts(root, &node["items"], item))
        }),
        "object" => value.as_object().is_some_and(|object| {
            let properties = node["properties"].as_object().unwrap();
            node["required"]
                .as_array()
                .unwrap()
                .iter()
                .all(|key| object.contains_key(key.as_str().unwrap()))
                && object.iter().all(|(key, child)| match properties.get(key) {
                    Some(property) => schema_accepts(root, property, child),
                    None => node["additionalProperties"] != false,
                })
        }),
        other => panic!("unsupported test schema type: {other}"),
    }
}

#[test]
fn revision_schema_rejects_contradictory_relation_shape() {
    let f = fixture();
    let (prompt, _) = input(&f["records"][0]);
    let prepared = Prepared::new(&prompt.claims[0], usize::MAX).unwrap();
    let invalid = json!({"source_spans":[prepared.source_spans.first().unwrap()],
        "claim_spans":[],"relation":"not_applicable"});
    assert!(
        !schema_accepts(&prepared.schema, &prepared.schema, &invalid),
        "decoder admits a nonempty not_applicable source list"
    );
}

#[test]
fn revision_comma_fallback_preserves_source() {
    let text = format!(
        "{}, {}.",
        "alpha ".repeat(25).trim(),
        "beta ".repeat(25).trim()
    );
    let ranges = segment_ranges(&text).expect("comma-only long sentence should be admitted");
    assert_eq!(ranges.len(), 2);
    assert_eq!(
        &text[ranges[0].0..ranges[0].1],
        text.split_once(", ").unwrap().0.to_string() + ","
    );
    assert_eq!(
        &text[ranges[1].0..ranges[1].1],
        text.split_once(", ").unwrap().1
    );
}

#[test]
fn revision_unsplittable_sentence_is_one_exact_piece() {
    let text = format!("{}.", "unbroken ".repeat(40).trim());
    assert_eq!(
        segment_ranges(&text).expect("whole sentence fallback should be admitted"),
        vec![(0, text.len())]
    );
}

#[test]
fn revision_plans_each_dimension() {
    let f = fixture();
    let (prompt, claims) = input(&f["records"][0]);
    let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
    assert_eq!(
        batches.len(),
        DIMENSIONS.len(),
        "one prepared request per dimension"
    );
}

#[test]
fn segment_conjunction_vocabulary_and_fallback_rollback() {
    let left = "alpha ".repeat(25);
    let right = "beta ".repeat(25);
    for word in ["and", "OR", "but", "Nor"] {
        let text = format!("{left}{word} {right}");
        let ranges = segment_ranges(&text).unwrap();
        assert_eq!(ranges.len(), 2, "{word}");
        assert!(text[ranges[1].0..ranges[1].1].starts_with(word));
        let joined: String = ranges
            .iter()
            .flat_map(|&(a, b)| text[a..b].chars())
            .filter(|c| !whitespace(*c))
            .collect();
        assert_eq!(
            joined,
            text.chars().filter(|c| !whitespace(*c)).collect::<String>()
        );
    }
    for word in ["for", "yet", "so", "android", "and2", "and_name", "oranges"] {
        let text = format!("{left}{word} {right}");
        assert_eq!(
            segment_ranges(&text).unwrap(),
            vec![(0, text.trim_end().len())],
            "{word}"
        );
    }
    let text = format!("Already done. {}and {}", left, "unbroken ".repeat(40));
    let ranges = segment_ranges(&text).unwrap();
    assert_eq!(ranges.len(), 2);
    assert_eq!(&text[ranges[0].0..ranges[0].1], "Already done.");
    assert_eq!(&text[ranges[1].0..ranges[1].1], text[14..].trim());
    let text = format!("{left}1,000 {right}");
    assert_eq!(segment_ranges(&text).unwrap().len(), 1);
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
fn dimension_plan_count_and_association_boundaries() {
    for count in [0, 1, 16, 17, 31, 32, 33] {
        let (prompt, claims) = repeated_claims(count);
        let result = plan(&Runtime::default(), &prompt, &claims, 32);
        if (1..=32).contains(&count) {
            let batches = result.unwrap();
            assert_eq!(batches.len(), count * 4);
            validate_plan(&batches).unwrap();
        } else {
            assert!(result.is_err());
        }
    }
    let (prompt, claims) = repeated_claims(2);
    for defect in 0..6 {
        let mut batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
        match defect {
            0 => {
                batches.pop();
            }
            1 => batches.swap(0, 1),
            2 => batches.swap(0, 4),
            3 => batches[2].claims[0].text.push('!'),
            4 => batches[2].comparison = None,
            5 => batches[2].claims[0].evidence_ids.clear(),
            _ => unreachable!(),
        }
        assert!(validate_plan(&batches).is_err(), "defect {defect}");
    }
}

#[test]
fn dimension_executor_preflights_all_and_returns_only_complete_owned_results() {
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
                (10..18).collect::<Vec<_>>()
            );
            self.calls.lock().unwrap().push(r.ordinal);
            if self.cancel == Some(r.ordinal) {
                self.token.request();
            }
            Ok(ModelResponse {
                text: if self.invalid == Some(r.ordinal) {
                    "{}".into()
                } else {
                    fixture_response(r, r.ordinal == 16)
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
        (Some(17), None, None),
        (None, Some(16), None),
        (None, None, Some(12)),
        (None, None, Some(13)),
        (None, None, Some(14)),
    ] {
        for sentence_counts in [&[1, 1][..], &[2][..]] {
            let runtime = Sequenced {
                preflights: Mutex::new(vec![]),
                calls: Mutex::new(vec![]),
                reject,
                invalid,
                cancel,
                token: CancellationToken::new(),
            };
            let (prompt, claims) = sentence_document(sentence_counts);
            let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
            let mut ordinal = 10;
            let result =
                classify_verification_batches(&runtime, batches, 7, &mut ordinal, &runtime.token);
            let expected_calls = if reject.is_some() {
                0
            } else if invalid.is_some() {
                7
            } else if let Some(cancel) = cancel {
                cancel - 10 + 1
            } else {
                8
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
fn export_actual_decoder_schema_and_independent_shapes() {
    let f = fixture();
    let record = f["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["operator_expected_admit"].is_boolean())
        .unwrap();
    let (prompt, claims) = input(record);
    let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
    let request = verification_request(&batches[0], 0, 7);
    let prepared = batches[0].comparison.as_ref().unwrap();
    let source = prepared.source_spans.iter().next().unwrap();
    let claim = prepared.claim_spans.iter().next().unwrap();
    let mut cases = Vec::new();
    for relation in [
        "preserved",
        "changed",
        "omitted",
        "uncertain",
        "not_applicable",
    ] {
        for n in [0, 1, 4, 5] {
            for m in [0, 1, 4, 5] {
                let expected = n <= 4
                    && m <= 4
                    && match relation {
                        "preserved" | "changed" => n > 0 && m > 0,
                        "omitted" => n > 0,
                        "uncertain" => n > 0 || m > 0,
                        "not_applicable" => n == 0 && m == 0,
                        _ => unreachable!(),
                    };
                let text = format!(
                    "{{\"source_spans\":{},\"claim_spans\":{},\"relation\":{}}}",
                    json!(vec![source; n]),
                    json!(vec![claim; m]),
                    json!(relation)
                );
                assert_eq!(prepared.parse(&text).is_ok(), expected);
                cases.push(json!({"relation":relation,"source_count":n,"claim_count":m,"expected":expected,"text":text}));
            }
        }
    }
    let schema = crate::pipeline::model::response_format(&request.output_format)
        .unwrap()
        .unwrap();
    if let Ok(path) = std::env::var("DOC_SUM_C9_GRAMMAR_PACKET") {
        std::fs::write(path, serde_json::to_vec_pretty(&json!({"schema":schema.as_value(),"decoder_schema_json":serde_json::to_string(&schema).unwrap(),"cases":cases})).unwrap()).unwrap();
    }
}

#[test]
fn verdict_parity_rejects_ambiguous_in_place_of_recorded_unsupported() {
    let f = fixture();
    let record = f["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["case"] == "invoice-explicit-difference")
        .unwrap();
    let (prompt, claims) = input(record);
    let prepared = Prepared::new(&prompt.claims[0], usize::MAX).unwrap();
    let covered = recorded_segments(&prepared, record["raw_response"].as_str().unwrap()).unwrap();
    let row = live_control_observation(
        &prepared,
        record,
        &json!({"text":covered.to_string()}),
        Ok(vec![ClaimVerification {
            claim_id: claims[0].claim_id.clone(),
            evidence_ids: claims[0].evidence_ids.clone(),
            verdict: ClaimVerdict::Ambiguous,
        }]),
        0,
    );
    assert_eq!(row["verdict_parity_passed"], false);
    assert_eq!(row["gate_passed"], false);
}

#[test]
fn sentence_parent_cannot_hide_a_wrong_first_middle_or_last_sentence() {
    struct SentenceRuntime;
    impl ModelRuntime for SentenceRuntime {
        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            let wire: Value = serde_json::from_str(&request.user_prompt).unwrap();
            // Public scripted reproduction: the coarse judgment misses the wrong
            // relationship, while the isolated sentence exposes it.
            let wrong = wire["claims"][0]["text"] == "Payment precedes approval.";
            Ok(ModelResponse {
                text: fixture_response(request, wrong),
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
    for wrong_index in 0..3 {
        let (mut prompt, mut claims) = repeated_claims(1);
        let mut sentences = [
            "Approval is required.",
            "Approval is required.",
            "Approval is required.",
        ];
        sentences[wrong_index] = "Payment precedes approval.";
        claims[0].text = sentences.join(" ");
        prompt.claims[0].text = claims[0].text.clone();
        let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
        let mut ordinal = 0;
        let verdicts = classify(
            &SentenceRuntime,
            batches,
            7,
            &mut ordinal,
            &UNCONTROLLED_EXECUTION,
        )
        .unwrap();
        assert_eq!(verdicts.len(), 1);
        assert_eq!(verdicts[0].claim_id, claims[0].claim_id);
        assert_eq!(verdicts[0].evidence_ids, claims[0].evidence_ids);
        assert_eq!(
            verdicts[0].verdict,
            ClaimVerdict::Unsupported,
            "wrong sentence index {wrong_index}"
        );
        assert_eq!(ordinal, 12);
    }
}

fn sentence_document(counts: &[usize]) -> (VerificationPrompt, Vec<CitedClaim>) {
    let (mut prompt, mut claims) = repeated_claims(counts.len());
    for ((input, claim), count) in prompt.claims.iter_mut().zip(&mut claims).zip(counts) {
        claim.text = std::iter::repeat_n("Approval is required.", *count)
            .collect::<Vec<_>>()
            .join(" ");
        input.text.clone_from(&claim.text);
    }
    (prompt, claims)
}

#[test]
fn sentence_document_admission_and_original_parent_bounds() {
    for counts in [vec![1], vec![12, 12], vec![1, 30], vec![1, 31], vec![1, 32]] {
        let (prompt, claims) = sentence_document(&counts);
        let calls = counts.iter().sum::<usize>() * 4;
        let planned = plan(&Runtime::default(), &prompt, &claims, 8);
        if calls <= 128 {
            let batches = planned.unwrap();
            assert_eq!(batches.len(), calls);
            assert_eq!(validate_plan(&batches).unwrap().len(), counts.len());
        } else {
            assert_eq!(planned.unwrap_err().code, "VERIFICATION_PLAN_TOO_LARGE");
        }
    }
    for (count, valid) in [
        (0, false),
        (1, true),
        (127, true),
        (128, true),
        (129, false),
    ] {
        assert_eq!(ensure_comparison_call_count(count).is_ok(), valid);
    }
    // The shared legacy policy and coherent parent count are not widened.
    assert!(ensure_verification_batch_count(64).is_ok());
    assert!(ensure_verification_batch_count(65).is_err());
    let (prompt, claims) = sentence_document(&[1; 9]);
    assert!(plan(&Runtime::default(), &prompt, &claims, 8).is_err());
    let (mut prompt, mut claims) = sentence_document(&[2]);
    prompt.claims[0].evidence.truncate(1);
    claims[0].evidence_ids.truncate(1);
    let parent_len = claims[0].text.chars().count();
    prompt.claims[0].evidence[0].exact_quote = "X".into();
    for total in [4095, 4096, 4097] {
        prompt.claims[0].evidence[0].full_clause = Some("X".repeat(total - parent_len));
        let planned = plan(&Runtime::default(), &prompt, &claims, 8);
        assert_eq!(
            planned.is_ok(),
            total <= 4096,
            "full-parent plus source {total}"
        );
    }
}

#[test]
fn sentence_plan_rejects_missing_duplicate_foreign_and_partial_ownership() {
    let (prompt, claims) = sentence_document(&[3, 1]);
    for defect in 0..8 {
        let mut batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
        match defect {
            0 => {
                batches.drain(4..8);
            } // Complete middle sentence missing.
            1 => {
                batches.drain(12..);
            } // Complete final parent missing.
            2 => {
                batches.truncate(4);
            } // Supported prefix is not a full plan.
            3 => {
                for dimension in 0..4 {
                    batches[4 + dimension].claims = batches[dimension].claims.clone();
                    batches[4 + dimension].comparison = batches[dimension].comparison.clone();
                }
            }
            4 => {
                batches[4] = plan(&Runtime::default(), &prompt, &claims, 8)
                    .unwrap()
                    .remove(4);
            }
            5 => {
                batches[4]
                    .comparison
                    .as_mut()
                    .unwrap()
                    .sentence
                    .as_mut()
                    .unwrap()
                    .ordinal = 0;
            }
            6 => {
                batches[4].claims[0].claim_id = "foreign".into();
            }
            7 => {
                batches.pop();
            }
            _ => unreachable!(),
        }
        // Runtime::generate panics; malformed ownership must fail before it.
        let mut ordinal = 0;
        assert_eq!(
            classify(
                &Runtime::default(),
                batches,
                7,
                &mut ordinal,
                &UNCONTROLLED_EXECUTION
            )
            .unwrap_err()
            .code,
            "MODEL_REQUEST_INVALID",
            "defect {defect}"
        );
        assert_eq!(ordinal, 0);
    }
}

#[test]
fn sentences_preserve_context_ranges_identity_and_long_piece_catalogs() {
    for text in ["", " \t\n"] {
        assert!(sentence_ranges(text).is_empty());
    }
    let text = "  Dr. Smith pays $27.50 today.\nApproval is required.  Approval is required.\t";
    let (mut prompt, mut claims) = sentence_document(&[1]);
    claims[0].text = text.into();
    prompt.claims[0].text = text.into();
    let original_catalog = Prepared::new(&prompt.claims[0], usize::MAX)
        .unwrap()
        .source_spans;
    let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
    let document = validate_plan(&batches).unwrap();
    let parent = &document[0];
    assert_eq!(parent.ranges.len(), 3);
    let mut cursor = 0;
    let mut reconstructed = String::new();
    let mut ids = HashSet::new();
    for (ordinal, &(start, end)) in parent.ranges.iter().enumerate() {
        assert!(text[cursor..start].chars().all(whitespace));
        reconstructed.push_str(&text[cursor..start]);
        reconstructed.push_str(&text[start..end]);
        cursor = end;
        let batch = &batches[ordinal * 4];
        let prepared = batch.comparison.as_ref().unwrap();
        assert!(ids.insert(batch.claims[0].claim_id.clone()));
        assert_eq!(prepared.source_spans, original_catalog);
        assert_eq!(batch.claims[0].text, text[start..end]);
        let wire: Value = serde_json::from_str(&batch.user_prompt).unwrap();
        assert_eq!(wire["parent_claim_context"], text);
        assert_eq!(wire["claim_segments"], json!([&text[start..end]]));
        assert_eq!(
            batch.model_facing_characters,
            batch.system_prompt.chars().count() + batch.user_prompt.chars().count()
        );
    }
    assert!(text[cursor..].chars().all(whitespace));
    reconstructed.push_str(&text[cursor..]);
    assert_eq!(reconstructed, text);
    let long = format!("{}; {}.", "alpha ".repeat(30), "beta ".repeat(30));
    assert_eq!(sentence_ranges(&long), vec![(0, long.len())]);
    assert_eq!(segment_ranges(&long).unwrap().len(), 2);
}

#[test]
fn sentence_uncertainty_and_not_applicable_cannot_be_rescued_by_siblings() {
    for middle in [
        ClaimVerdict::Supported,
        ClaimVerdict::Unsupported,
        ClaimVerdict::Ambiguous,
    ] {
        let result = aggregate_parent(&[
            ClaimVerdict::Supported,
            middle.clone(),
            ClaimVerdict::Supported,
        ])
        .unwrap();
        assert_eq!(result, middle);
    }
    let absent = std::array::from_fn(|_| Comparison {
        source_spans: vec![],
        claim_spans: vec![],
        relation: Relation::NotApplicable,
    });
    assert_eq!(
        aggregate_parent(&[ClaimVerdict::Supported, aggregate(&absent)]).unwrap(),
        ClaimVerdict::Ambiguous
    );
    assert!(aggregate_parent(&[]).is_err());
}

#[path = "sentence_run.rs"]
mod sentence_run;
