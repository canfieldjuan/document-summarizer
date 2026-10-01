use super::*;
use crate::pipeline::summary::coherent::tests::{contract_documents, set_furniture_fixture};
use std::sync::Mutex;

struct Case {
    name: &'static str,
    opening: &'static str,
    continuation: &'static str,
    wrong: &'static str,
    faithful: &'static str,
}

// Public synthetic equivalents. These are expectations, never model verdicts.
const CASES: &[Case] = &[
    Case {
        name: "payment-stage",
        opening: "10.2 Substantial Completion\nThe owner approves payment when the work reaches substantial completion.",
        continuation: "Such payment releases retained funds except for incomplete work.",
        wrong: "Final payment releases retained funds except for incomplete work.",
        faithful: "At substantial completion, payment releases retained funds except for incomplete work.",
    },
    Case {
        name: "invoice-period",
        opening: "2.1 Agreement total\nThe monthly agreement total covers routine services.",
        continuation: "Extra visits outside the current invoice period are excluded from the agreement total.",
        wrong: "Extra visits are excluded from the agreement total.",
        faithful: "Extra visits are excluded from the agreement total when they fall outside the current billing period.",
    },
    Case {
        name: "working-day-cure",
        opening: "4.1 Failure to correct\nWritten notice begins the opportunity to cure a service failure.",
        continuation: "If the contractor does not begin correction within seven working days after written notice and continue correction diligently, the owner may perform the correction and charge its cost to the contractor.",
        wrong: "The owner may perform correction and charge the contractor after seven days of written notice.",
        faithful: "If the contractor fails to start correction within seven working days after written notice and continue diligently, the owner may correct the failure and charge the contractor.",
    },
    Case {
        name: "coverage-condition",
        opening: "6.1 Property insurance\nThe owner maintains property insurance for the work.",
        continuation: "The insurer's payment is due to the owner only to the extent the loss is covered by the property insurance policy.",
        wrong: "The insurer's payment is due to the owner for the loss.",
        faithful: "The insurer pays the owner only for the portion of the loss covered by the property insurance policy.",
    },
    Case {
        name: "equipment-scope",
        opening: "8.1 Equipment use\nThe contractor may use the owner's equipment with permission.",
        continuation: "The contractor indemnifies the owner for losses arising from equipment use, whether or not that use was negligent.",
        wrong: "The contractor indemnifies the owner for losses from negligent equipment use.",
        faithful: "The contractor indemnifies the owner for equipment-use losses regardless of whether the use was negligent.",
    },
];

#[derive(Default)]
struct FixtureRuntime {
    requests: Mutex<Vec<ModelRequest>>,
    admissions: Mutex<Vec<ModelRequest>>,
    unsupported: Option<String>,
}

impl ModelRuntime for FixtureRuntime {
    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        Ok(())
    }
    fn runtime_id(&self) -> &str {
        "f3-fixture"
    }
    fn model_id(&self) -> &str {
        "f3-fixture"
    }
    fn context_tokens(&self, _: PipelineStage) -> u32 {
        32_768
    }
    fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        self.admissions.lock().unwrap().push(request.clone());
        Ok(())
    }
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        self.requests.lock().unwrap().push(request.clone());
        let mut text = crate::pipeline::summary::fixture_model_output(request);
        if request.stage == PipelineStage::Verify {
            let input: Value = serde_json::from_str(&request.user_prompt).unwrap();
            if let Some(claims) = input["claims"].as_array() {
                text = json!({"verdicts": claims.iter().map(|c| json!({
                    "claim_id": c["claim_id"],
                    "verdict": if c["text"].as_str() == self.unsupported.as_deref() { "unsupported" } else { "supported" }
                })).collect::<Vec<_>>()}).to_string();
            }
        }
        Ok(ModelResponse {
            text,
            runtime_id: self.runtime_id().into(),
            model_id: self.model_id().into(),
            request_attempts: vec![],
        })
    }
}

fn fixture(
    case: &Case,
    text: &str,
) -> (
    NormalizedDocument,
    ChunkedDocument,
    AnalyzedDocument,
    SynthesizedDocument,
) {
    let (mut normalized, mut chunked) = contract_documents();
    chunked.chunks[0].ordinal = 1;
    set_furniture_fixture(&mut normalized, &mut chunked, |n| match n {
        1 => case.opening.into(),
        2 => case.continuation.into(),
        _ => format!("20. Other services\nThe parties review the schedule for phase {n}."),
    });
    let runtime = FixtureRuntime::default();
    let analyzed = analyze(&runtime, &chunked, &normalized, 7, &UNCONTROLLED_EXECUTION).unwrap();
    let catalog = source_catalog_for_profile(
        SummaryProfile::General,
        VERSION,
        &chunked,
        &normalized,
        Some(&analyzed),
    )
    .unwrap();
    let source = catalog
        .candidates
        .iter()
        .find(|c| c.evidence.source_span.page_start == 2)
        .unwrap();
    let (summary_claims, synthesis_evidence) = parse_response(
        SummaryProfile::General,
        &json!({"units":[{"text":text,"source_ids":[source.request_id]}]}).to_string(),
        &normalized.document_id,
        &catalog,
    )
    .unwrap();
    let synthesized = SynthesizedDocument {
        document_id: normalized.document_id.clone(),
        synthesis_version: VERSION.into(),
        runtime_id: runtime.runtime_id().into(),
        model_id: runtime.model_id().into(),
        presentation_mode: SummaryPresentationMode::Coherent,
        summary_text: render_cited_summary_with_evidence(&summary_claims, &synthesis_evidence)
            .unwrap(),
        source_chunk_ids: chunked.chunks.iter().map(|c| c.chunk_id.clone()).collect(),
        summary_claims,
        synthesis_evidence,
        claims: direct::source_ordered_claims(&analyzed).unwrap(),
        warnings: vec![],
    };
    (normalized, chunked, analyzed, synthesized)
}

#[test]
fn clause_verification_execution_matches_admission_and_enforces_all_five_verdicts() {
    for case in CASES {
        for (text, should_pass) in [(case.wrong, false), (case.faithful, true)] {
            let (normalized, chunked, analyzed, synthesized) = fixture(case, text);
            let runtime = FixtureRuntime {
                unsupported: (!should_pass).then(|| text.into()),
                ..Default::default()
            };
            assert!(!coherent_verification_exceeds_runtime_context(
                SummaryProfile::General,
                &runtime,
                &synthesized,
                &analyzed,
                &normalized,
                7
            )
            .unwrap());
            let result = verify(
                SummaryProfile::General,
                &runtime,
                &synthesized,
                &analyzed,
                &chunked,
                &normalized,
                7,
                0,
                false,
                &UNCONTROLLED_EXECUTION,
            )
            .unwrap();
            assert_eq!(
                result.summary_claims.len(),
                usize::from(should_pass),
                "{}",
                case.name
            );
            assert_eq!(result.verification_version, VERIFICATION_VERSION);
            let requests = runtime.requests.lock().unwrap();
            let actual = requests.last().unwrap();
            assert_eq!(runtime.admissions.lock().unwrap().last().unwrap(), actual);
            let value: Value = serde_json::from_str(&actual.user_prompt).unwrap();
            assert!(value["clause_contexts"][0]["full_clause"]
                .as_str()
                .unwrap()
                .contains(case.opening));
            assert_eq!(
                value["claims"][0]["evidence"][0]["exact_quote"],
                case.continuation
            );
            if !should_pass {
                assert!(!result.summary_text.contains(text));
                assert!(result
                    .warnings
                    .iter()
                    .any(|w| w.code == "SEMANTIC_CLAIMS_WITHHELD"));
            }
        }
    }
}

#[test]
fn clause_verification_context_is_bounded_and_not_a_verdict_id() {
    let (normalized, _, _, synthesized) = fixture(&CASES[0], CASES[0].faithful);
    let prompt =
        summary_verification_prompt(SummaryProfile::General, &synthesized, &normalized).unwrap();
    let batches = verification_batches_for_runtime(
        &FixtureRuntime::default(),
        &prompt,
        &synthesized.summary_claims,
        MAX_SUMMARY_CLAIMS,
    )
    .unwrap();
    let batch = &batches[0];
    let size = batch.system_prompt.chars().count() + batch.user_prompt.chars().count();
    assert!(plan_verification_batches(
        &prompt,
        &synthesized.summary_claims,
        MAX_SUMMARY_CLAIMS,
        size
    )
    .is_ok());
    assert_eq!(
        plan_verification_batches(
            &prompt,
            &synthesized.summary_claims,
            MAX_SUMMARY_CLAIMS,
            size - 1
        )
        .unwrap_err()
        .code,
        "VERIFICATION_INPUT_TOO_LARGE"
    );
    let value: Value = serde_json::from_str(&batch.user_prompt).unwrap();
    let bad = json!({"verdicts":[{"claim_id":value["clause_contexts"][0]["context_id"],"verdict":"supported"}]}).to_string();
    assert!(batch.identifiers.verdict_response(&bad).is_err());
    let good = json!({"verdicts":[{"claim_id":"k1","verdict":"supported"}]}).to_string();
    assert!(batch.identifiers.verdict_response(&good).is_ok());
    for profile in [SummaryProfile::Story, SummaryProfile::Contract] {
        let old = summary_verification_prompt(profile, &synthesized, &normalized).unwrap();
        assert!(old
            .claims
            .iter()
            .flat_map(|c| &c.evidence)
            .all(|e| e.full_clause.is_none()));
    }
    let mut old = synthesized;
    old.synthesis_version = PRE_CLAUSE_SYNTHESIS_VERSION.into();
    let prompt = summary_verification_prompt(SummaryProfile::General, &old, &normalized).unwrap();
    assert!(prompt
        .claims
        .iter()
        .flat_map(|c| &c.evidence)
        .all(|e| e.full_clause.is_none()));
}

#[test]
fn clause_verification_tables_are_distinct_deduplicated_and_request_local() {
    let (normalized, _, _, synthesized) = fixture(&CASES[0], CASES[0].faithful);
    let mut prompt =
        summary_verification_prompt(SummaryProfile::General, &synthesized, &normalized).unwrap();
    let original = prompt.claims[0].evidence[0].clone();
    let mut repeated = original.clone();
    repeated.evidence_id = "another-fragment".into();
    let mut different = original.clone();
    different.evidence_id = "another-clause".into();
    different.full_clause = Some("11. Final payment requires final acceptance.".into());
    prompt.claims[0]
        .evidence
        .extend([repeated, different.clone()]);
    let (wire, _) = identifiers::verification_prompt(&prompt.claims).unwrap();
    let wire: Value = serde_json::from_str(&wire).unwrap();
    let contexts = wire["clause_contexts"].as_array().unwrap();
    assert_eq!(contexts.len(), 2);
    let evidence = wire["claims"][0]["evidence"].as_array().unwrap();
    assert_eq!(
        evidence[0]["clause_context_id"],
        evidence[1]["clause_context_id"]
    );
    assert_ne!(
        evidence[0]["clause_context_id"],
        evidence[2]["clause_context_id"]
    );
    prompt.claims[0].evidence = vec![different];
    let (wire, _) = identifiers::verification_prompt(&prompt.claims).unwrap();
    assert!(!wire.contains("Substantial Completion"));
    prompt.claims[0].evidence[0].full_clause = None;
    let (wire, _) = identifiers::verification_prompt(&prompt.claims).unwrap();
    assert!(!wire.contains("clause_context"));
}

#[test]
#[ignore = "owner-only saved A/B artifact replay; no inference"]
fn clause_verification_saved_source_replay() {
    let path = std::env::var("DOC_SUM_F3_REPLAY_INPUT").unwrap();
    let cases: Vec<Value> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut receipts = vec![];
    for (index, case) in cases.iter().enumerate() {
        let normalized: NormalizedDocument =
            serde_json::from_value(case["normalized"].clone()).unwrap();
        let chunked: ChunkedDocument = serde_json::from_value(case["chunked"].clone()).unwrap();
        let analyzed: AnalyzedDocument = serde_json::from_value(case["analyzed"].clone()).unwrap();
        let historical: SynthesizedDocument =
            serde_json::from_value(case["synthesized"].clone()).unwrap();
        let verified: VerifiedDocument = serde_json::from_value(case["verified"].clone()).unwrap();
        let summary: SummaryArtifact = serde_json::from_value(case["summary"].clone()).unwrap();
        let citations: CitationArtifact =
            serde_json::from_value(case["citations"].clone()).unwrap();
        validate_citation_artifact(
            &citations,
            &summary,
            &verified,
            &historical,
            &analyzed,
            &chunked,
            &normalized,
        )
        .unwrap();
        let catalog = source_catalog_for_profile(
            SummaryProfile::General,
            VERSION,
            &chunked,
            &normalized,
            Some(&analyzed),
        )
        .unwrap();
        for context_tokens in [8_192, 32_768] {
            let request_limit =
                verification_request_character_limit(context_tokens, VERIFICATION_OUTPUT_TOKENS)
                    .unwrap();
            let mut requests = 0;
            let mut with_context = 0;
            let mut max_characters = 0;
            let mut overflow = 0;
            // Replay each currently offered source through the shared verifier builder.
            // Exact source text is a transport control, not new generated prose.
            for candidate in &catalog.candidates {
                let mut source = historical.clone();
                source.synthesis_version = VERSION.into();
                source.presentation_mode = SummaryPresentationMode::Coherent;
                source.synthesis_evidence = vec![candidate.evidence.clone()];
                source.summary_claims = vec![CitedClaim {
                    claim_id: "replay".into(),
                    text: candidate.evidence.exact_quote.clone(),
                    evidence_ids: vec![candidate.evidence.evidence_id.clone()],
                }];
                let prompt =
                    summary_verification_prompt(SummaryProfile::General, &source, &normalized)
                        .unwrap();
                assert!(
                    prompt.claims[0].evidence[0].full_clause == candidate.full_clause,
                    "context ownership differs; private text omitted"
                );
                with_context += usize::from(candidate.full_clause.is_some());
                match plan_verification_batches(
                    &prompt,
                    &source.summary_claims,
                    MAX_SUMMARY_CLAIMS,
                    request_limit,
                ) {
                    Ok(batches) => {
                        for batch in batches {
                            requests += 1;
                            max_characters = max_characters.max(
                                batch.system_prompt.chars().count()
                                    + batch.user_prompt.chars().count(),
                            );
                        }
                    }
                    Err(error) if error.code == "VERIFICATION_INPUT_TOO_LARGE" => overflow += 1,
                    Err(error) => panic!("unexpected replay failure: {}", error.code),
                }
            }
            let mut saved = historical.clone();
            saved.synthesis_version = VERSION.into();
            saved.presentation_mode = SummaryPresentationMode::Coherent;
            let prompt =
                summary_verification_prompt(SummaryProfile::General, &saved, &normalized).unwrap();
            let saved_fit = match plan_verification_batches(
                &prompt,
                &saved.summary_claims,
                MAX_SUMMARY_CLAIMS,
                request_limit,
            ) {
                Ok(batches) => {
                    json!({"fallback":false,"batches":batches.len(),"max_request_characters":batches.iter().map(|b| b.system_prompt.chars().count()+b.user_prompt.chars().count()).max()})
                }
                Err(error) if error.code == "VERIFICATION_INPUT_TOO_LARGE" => {
                    json!({"fallback":true,"reason":error.code})
                }
                Err(error) => panic!("unexpected saved-claim fit failure: {}", error.code),
            };
            receipts.push(json!({"case":index,"context_tokens":context_tokens,"planner_character_limit":request_limit,"historical_artifacts_valid":true,"sources":catalog.candidates.len(),"with_context":with_context,"requests":requests,"max_request_characters":max_characters,"oversized_sources":overflow,"saved_claim_fit":saved_fit}));
        }
    }
    std::fs::write(
        std::env::var("DOC_SUM_F3_REPLAY_OUTPUT").unwrap(),
        serde_json::to_vec_pretty(&receipts).unwrap(),
    )
    .unwrap();
    println!(
        "F3_SOURCE_REPLAY {}",
        serde_json::to_string(&receipts).unwrap()
    );
}

#[test]
#[ignore = "requires explicit 9B model and exclusive inference lock held by the driver"]
fn clause_verification_live_public_fidelity() {
    use crate::pipeline::model_settings::{register_gguf, runtime_from_settings, settings_path};
    use std::path::Path;
    let dir = tempfile::Builder::new()
        .prefix("docsum-f3-")
        .tempdir()
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let settings = settings_path(dir.path());
    let model = std::env::var("DOC_SUM_QUALIFICATION_GGUF").unwrap();
    register_gguf(&settings, Path::new(&model)).unwrap();
    let db = dir.path().join("probe.db");
    crate::pipeline::db::init_db(&db).unwrap();
    let runtime = runtime_from_settings(&settings, &db).unwrap();
    assert_eq!(runtime.context_tokens(PipelineStage::Verify), 32_768);
    runtime.health().unwrap();
    let mut results = vec![];
    for case in CASES {
        for (text, expected_supported) in [(case.wrong, false), (case.faithful, true)] {
            let (normalized, _, _, synthesized) = fixture(case, text);
            let prompt =
                summary_verification_prompt(SummaryProfile::General, &synthesized, &normalized)
                    .unwrap();
            let batches = verification_batches_for_runtime(
                runtime.as_ref(),
                &prompt,
                &synthesized.summary_claims,
                MAX_SUMMARY_CLAIMS,
            )
            .unwrap();
            assert_eq!(batches.len(), 1);
            let batch = &batches[0];
            let request = verification_request(batch, results.len() as u32, 7);
            runtime.preflight_request(&request).unwrap();
            let response = runtime.generate(&request);
            let result = match response {
                Ok(response) => {
                    let parsed = validate_runtime_response(
                        runtime.as_ref(),
                        &response,
                        PipelineStage::Verify,
                    )
                    .and_then(|()| batch.identifiers.verdict_response(&response.text))
                    .and_then(|text| parse_verification_response(&text, &batch.claims));
                    let raw_supported = parsed
                        .as_ref()
                        .ok()
                        .map(|v| v[0].verdict == ClaimVerdict::Supported);
                    let final_supported = parsed.and_then(|mut v| {
                        apply_semantic_fidelity_guards(
                            &synthesized.summary_claims,
                            &synthesized.synthesis_evidence,
                            &mut v,
                        )?;
                        Ok(v[0].verdict == ClaimVerdict::Supported)
                    });
                    json!({"case":case.name,"expected_supported":expected_supported,"raw_supported":raw_supported,
                        "final_supported":final_supported.as_ref().ok(),"passed":final_supported == Ok(expected_supported),
                        "response":response.text,"attempts":response.request_attempts,
                        "failure":final_supported.err().map(|e|e.code)})
                }
                Err(error) => {
                    json!({"case":case.name,"expected_supported":expected_supported,"passed":false,"failure":error.code,"attempts":error.request_attempts})
                }
            };
            let mut result = result;
            result["request"] = json!({"system_prompt":request.system_prompt,"user_prompt":request.user_prompt,"seed":request.seed,"max_output_tokens":request.max_output_tokens,"output_format":match &request.output_format { ModelOutputFormat::JsonSchema { name, schema } => json!({"name":name,"schema":schema}), ModelOutputFormat::Text => Value::Null }});
            println!(
                "F3_FIDELITY {} {} {}",
                case.name, expected_supported, result["passed"]
            );
            results.push(result);
            std::fs::write(
                std::env::var("DOC_SUM_F3_LIVE_OUTPUT").unwrap(),
                serde_json::to_vec_pretty(
                    &json!({"profile":runtime.profile_snapshot(),"results":results}),
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
    assert!(
        results.iter().all(|r| r["passed"] == true),
        "public fidelity probe has failed cases; retained all attempts"
    );
}
