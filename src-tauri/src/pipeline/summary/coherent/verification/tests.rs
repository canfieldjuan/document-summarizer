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
    schema_limit: Option<usize>,
    unsupported_protocol: bool,
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
    fn supports_response_schema(&self, name: &str) -> bool {
        !self.unsupported_protocol || name != comparisons::SCHEMA_NAME
    }
    fn response_schema_byte_limit(&self, _: PipelineStage, name: &str) -> usize {
        self.schema_limit
            .unwrap_or_else(|| crate::pipeline::contracts::response_schema_byte_limit(name))
    }
    fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        self.admissions.lock().unwrap().push(request.clone());
        Ok(())
    }
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        if matches!(&request.output_format,ModelOutputFormat::JsonSchema{name,..} if name=="document_trimmed_source_contribution_v1")
        {
            panic!("HELD_TRIM_GENERATION_FORBIDDEN");
        }
        self.requests.lock().unwrap().push(request.clone());
        let mut text = crate::pipeline::summary::fixture_model_output(request);
        if request.stage == PipelineStage::Verify {
            let input: Value = serde_json::from_str(&request.user_prompt).unwrap();
            if let Some(claims) = input["claims"].as_array() {
                if matches!(&request.output_format, ModelOutputFormat::JsonSchema { name, .. } if name == comparisons::SCHEMA_NAME)
                {
                    let unsupported = claims[0]["text"].as_str() == self.unsupported.as_deref();
                    text = comparisons::fixture_response(request, unsupported);
                } else {
                    text = json!({"verdicts": claims.iter().map(|c| json!({
                    "claim_id": c["claim_id"],
                    "verdict": if c["text"].as_str() == self.unsupported.as_deref() { "unsupported" } else { "supported" }
                })).collect::<Vec<_>>()}).to_string();
                }
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
    fixture_for_version(case, text, VERSION)
}

fn fixture_for_version(
    case: &Case,
    text: &str,
    version: &str,
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
        version,
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
    let (mut summary_claims, synthesis_evidence) = parse_response(
        SummaryProfile::General,
        &json!({"units":[{"text":text,"source_ids":[source.request_id]}]}).to_string(),
        &normalized.document_id,
        &catalog,
    )
    .unwrap();
    if version != VERSION {
        summary_claims = materialize_cited_claims(
            &normalized.document_id,
            version,
            summary_claims
                .into_iter()
                .map(|claim| ValidatedClaim {
                    text: claim.text,
                    evidence_ids: claim.evidence_ids,
                })
                .collect(),
        )
        .unwrap();
    }
    let mut synthesized = SynthesizedDocument {
        whole_document_progress: None,
        contract_extraction: None,
        document_id: normalized.document_id.clone(),
        synthesis_version: version.into(),
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
    if version == WHOLE_DOCUMENT_VERSION {
        let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
        let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
        assert_eq!(plan.drafts.len(), 1);
        let mut progress = input.new_progress(&plan, &runtime, 7).unwrap();
        input
            .dispatch_section(&mut progress, 0, &runtime, 7)
            .unwrap();
        input
            .record_section(
                &mut progress,
                0,
                &ModelResponse {
                    text: json!({"units":[{"text":text,"source_ids":[source.request_id]}]})
                        .to_string(),
                    runtime_id: runtime.runtime_id().into(),
                    model_id: runtime.model_id().into(),
                    request_attempts: vec![],
                },
            )
            .unwrap();
        synthesized.whole_document_progress = Some(progress);
        input.finalize_progress(&mut synthesized).unwrap();
    }
    (normalized, chunked, analyzed, synthesized)
}

#[test]
fn c9_production_request_requires_comparisons_instead_of_model_verdict() {
    let (normalized, _, analyzed, synthesized) = fixture(&CASES[0], CASES[0].wrong);
    let runtime = FixtureRuntime::default();
    assert!(!coherent_verification_exceeds_runtime_context(
        SummaryProfile::General,
        &runtime,
        &synthesized,
        &analyzed,
        &normalized,
        7,
        None
    )
    .unwrap());
    let requests = runtime.admissions.lock().unwrap();
    let request = requests.last().unwrap();
    let ModelOutputFormat::JsonSchema { schema, .. } = &request.output_format else {
        panic!("verification must require structured output");
    };
    let item = &schema["properties"]["verdicts"]["items"];
    assert_eq!(item["required"], json!(["claim_id", "comparisons"]));
    assert!(item["properties"].get("verdict").is_none());
    let comparisons = &item["properties"]["comparisons"];
    assert_eq!(
        comparisons["required"],
        json!(["stage", "conditions", "qualifiers", "scope"])
    );
    for dimension in ["stage", "conditions", "qualifiers", "scope"] {
        let shape = &comparisons["properties"][dimension];
        assert_eq!(
            shape["required"],
            json!(["source_spans", "claim_spans", "relation"])
        );
        assert!(shape["properties"].get("verdict").is_none());
    }
    let comparison_requests: Vec<_> = requests
        .iter()
        .filter(|r| {
            matches!(&r.output_format,
        ModelOutputFormat::JsonSchema { name, .. } if name == comparisons::SCHEMA_NAME)
        })
        .collect();
    assert_eq!(comparison_requests.len(), 1);
    let input: Value = serde_json::from_str(&comparison_requests[0].user_prompt).unwrap();
    assert!(input.get("dimension").is_none());
    assert!(input.get("source_segments").is_none());
    assert_eq!(input["claims"].as_array().unwrap().len(), 1);
}

#[test]
fn c9_current_and_historical_saved_results_remain_readable() {
    for version in [SYNTHESIS_VERSION, PRE_RUNNING_FURNITURE_SYNTHESIS_VERSION] {
        let (normalized, chunked, analyzed, synthesized) =
            fixture_for_version(&CASES[0], CASES[0].faithful, version);
        let verified = verify(
            SummaryProfile::General,
            &FixtureRuntime::default(),
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
        assert_eq!(verified.verification_version, VERIFICATION_VERSION);
        for saved_version in [
            VERIFICATION_VERSION,
            PRE_SEGMENT_ENUM_VERIFICATION_VERSION,
            PRE_COMPARISON_VERIFICATION_VERSION,
            PRE_CLAUSE_VERIFICATION_VERSION,
        ] {
            let mut saved = verified.clone();
            saved.verification_version = saved_version.into();
            let reopened: VerifiedDocument =
                serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
            validate_verified_document(&reopened, &synthesized, &analyzed, &chunked, &normalized)
                .unwrap();
            assert_eq!(reopened, saved);
        }
        for profile in [SummaryProfile::Story, SummaryProfile::Contract] {
            assert!(!comparisons::applies(profile, &synthesized));
        }
        let mut fallback = synthesized.clone();
        fallback.presentation_mode = SummaryPresentationMode::ClaimLedgerFallback;
        assert!(!comparisons::applies(SummaryProfile::General, &fallback));
    }
}

#[test]
fn c9_unsupported_protocol_falls_back_before_drafting_and_survives_validation() {
    let (normalized, chunked, analyzed, synthesized) = fixture(&CASES[0], CASES[0].faithful);
    let runtime = FixtureRuntime {
        unsupported_protocol: true,
        ..Default::default()
    };
    let error = summary_verification_batches(
        SummaryProfile::General,
        &runtime,
        &synthesized,
        &normalized,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, "VERIFICATION_PROTOCOL_UNSUPPORTED");
    let fallback = synthesize(
        SummaryProfile::General,
        &runtime,
        &analyzed,
        &chunked,
        &normalized,
        7,
        &UNCONTROLLED_EXECUTION,
    )
    .unwrap();
    assert_eq!(
        fallback.presentation_mode,
        SummaryPresentationMode::ClaimLedgerFallback
    );
    assert!(has_fallback_warning(
        &fallback,
        FallbackReason::VerificationUnavailable
    ));
    assert!(runtime.requests.lock().unwrap().is_empty());
    validate_for_runtime(
        SummaryProfile::General,
        &fallback,
        &analyzed,
        &chunked,
        &normalized,
        &runtime,
    )
    .unwrap();
    validate_content(&fallback, &analyzed, &chunked, &normalized).unwrap();
}

#[test]
fn c9_schema_overflow_uses_existing_admission_fallback_without_generation() {
    let (normalized, _, analyzed, synthesized) = fixture(&CASES[0], CASES[0].faithful);
    let runtime = FixtureRuntime {
        schema_limit: Some(0),
        ..Default::default()
    };
    assert!(coherent_verification_exceeds_runtime_context(
        SummaryProfile::General,
        &runtime,
        &synthesized,
        &analyzed,
        &normalized,
        7,
        None
    )
    .unwrap());
    assert!(runtime.requests.lock().unwrap().is_empty());
    assert!(runtime.admissions.lock().unwrap().is_empty());
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
                7,
                None
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
        summary_verification_prompt(SummaryProfile::General, &synthesized, &normalized, None)
            .unwrap();
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
        let old = summary_verification_prompt(profile, &synthesized, &normalized, None).unwrap();
        assert!(old
            .claims
            .iter()
            .flat_map(|c| &c.evidence)
            .all(|e| e.full_clause.is_none()));
    }
    let mut old = synthesized;
    old.synthesis_version = PRE_CLAUSE_SYNTHESIS_VERSION.into();
    let prompt =
        summary_verification_prompt(SummaryProfile::General, &old, &normalized, None).unwrap();
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
        summary_verification_prompt(SummaryProfile::General, &synthesized, &normalized, None)
            .unwrap();
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
                let prompt = summary_verification_prompt(
                    SummaryProfile::General,
                    &source,
                    &normalized,
                    None,
                )
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
                summary_verification_prompt(SummaryProfile::General, &saved, &normalized, None)
                    .unwrap();
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
            let prompt = summary_verification_prompt(
                SummaryProfile::General,
                &synthesized,
                &normalized,
                None,
            )
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

#[test]
fn clause_verification_version_refresh_preserves_completed_pairs() {
    for synthesis in [
        SYNTHESIS_VERSION,
        PRE_DRAFT_GUIDANCE_SYNTHESIS_VERSION,
        PRE_RUNNING_FURNITURE_SYNTHESIS_VERSION,
    ] {
        for verification in [VERIFICATION_VERSION, PRE_CLAUSE_VERIFICATION_VERSION] {
            assert!(
                coherent_verification_versions_match(synthesis, verification),
                "completed synthesis {synthesis} / verification {verification} must remain readable"
            );
        }
    }
    for synthesis in [
        PRE_CLAUSE_SYNTHESIS_VERSION,
        PRE_FURNITURE_SYNTHESIS_VERSION,
        PRE_BALANCED_SYNTHESIS_VERSION,
        PRE_CONTRACT_SOURCE_SYNTHESIS_VERSION,
    ] {
        assert!(coherent_verification_versions_match(
            synthesis,
            PRE_CLAUSE_VERIFICATION_VERSION
        ));
        assert!(!coherent_verification_versions_match(
            synthesis,
            VERIFICATION_VERSION
        ));
    }
    for synthesis in ["", "0", "15.0.0"] {
        assert!(!coherent_verification_versions_match(
            synthesis,
            VERIFICATION_VERSION
        ));
    }
}

#[test]
fn clause_verification_version_refresh_preserves_previous_context() {
    let (normalized, chunked, analyzed, synthesized) = fixture_for_version(
        &CASES[0],
        CASES[0].faithful,
        PRE_RUNNING_FURNITURE_SYNTHESIS_VERSION,
    );
    let prompt =
        summary_verification_prompt(SummaryProfile::General, &synthesized, &normalized, None)
            .unwrap();
    assert!(
        prompt.claims[0].evidence[0]
            .full_clause
            .as_ref()
            .is_some_and(|context| context.contains(CASES[0].opening)),
        "previous F3 synthesis must retain its governing clause"
    );
    let verified = verify(
        SummaryProfile::General,
        &FixtureRuntime::default(),
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
    assert_eq!(verified.verification_version, VERIFICATION_VERSION);
    validate_verified_document(&verified, &synthesized, &analyzed, &chunked, &normalized).unwrap();
    let mut completed_before_f3 = verified;
    completed_before_f3.verification_version = PRE_CLAUSE_VERIFICATION_VERSION.into();
    validate_verified_document(
        &completed_before_f3,
        &synthesized,
        &analyzed,
        &chunked,
        &normalized,
    )
    .unwrap();
}

#[test]
fn clause_verification_version_refresh_reuses_historical_source_policy() {
    let (mut normalized, _, _, mut synthesized) = fixture(&CASES[0], CASES[0].faithful);
    // Keep the cited continuation intact; add running furniture to its governing
    // page and the other pages so the original and current policies differ.
    for page in normalized
        .pages
        .iter_mut()
        .filter(|page| page.page_number != 2)
    {
        page.content[0].text.push_str(&format!(
            "\nPublic Service Agreement Page {} of 6",
            page.page_number
        ));
    }
    for version in [PRE_RUNNING_FURNITURE_SYNTHESIS_VERSION, SYNTHESIS_VERSION] {
        synthesized.synthesis_version = version.into();
        let prompt =
            summary_verification_prompt(SummaryProfile::General, &synthesized, &normalized, None)
                .unwrap();
        let context = prompt.claims[0].evidence[0].full_clause.as_ref().unwrap();
        assert!(context.contains(CASES[0].opening));
        assert_eq!(
            context.contains("Public Service Agreement"),
            version == PRE_RUNNING_FURNITURE_SYNTHESIS_VERSION
        );
    }
}

#[test]
fn contextual_anchor_verification_requires_complete_owned_governing_context() {
    let (normalized, chunked, _, synthesized) =
        fixture_for_version(&CASES[0], CASES[0].faithful, WHOLE_DOCUMENT_VERSION);
    let prompt = summary_verification_prompt(
        SummaryProfile::General,
        &synthesized,
        &normalized,
        Some(&chunked),
    )
    .unwrap();
    let context = prompt.claims[0].evidence[0].full_clause.as_deref().expect(
        "contextual anchors require complete owned governing context, never a quote-only fallback",
    );
    assert!(context.contains(CASES[0].opening));
    assert!(context.contains(CASES[0].continuation));
}

#[test]
fn contextual_anchor_verification_rejects_missing_and_altered_bindings() {
    let (normalized, chunked, _, synthesized) =
        fixture_for_version(&CASES[0], CASES[0].faithful, WHOLE_DOCUMENT_VERSION);
    assert!(
        summary_verification_prompt(SummaryProfile::General, &synthesized, &normalized, None)
            .is_err()
    );
    for change in 0..6 {
        let mut altered = synthesized.clone();
        match change {
            0 => altered.synthesis_evidence[0].exact_quote.push('x'),
            1 => altered.synthesis_evidence[0].evidence_id.push('x'),
            2 => altered.synthesis_evidence[0].chunk_id.push('x'),
            3 => altered.synthesis_evidence[0].block_id.push('x'),
            4 => altered.synthesis_evidence[0].source_span.page_start += 1,
            _ => altered
                .synthesis_evidence
                .push(altered.synthesis_evidence[0].clone()),
        }
        assert!(
            summary_verification_prompt(
                SummaryProfile::General,
                &altered,
                &normalized,
                Some(&chunked)
            )
            .is_err(),
            "tampered binding {change} was accepted"
        );
    }
    assert!(summary_verification_prompt(
        SummaryProfile::Story,
        &synthesized,
        &normalized,
        Some(&chunked)
    )
    .is_err());
    let reloaded: SynthesizedDocument =
        serde_json::from_str(&serde_json::to_string(&synthesized).unwrap()).unwrap();
    let expected = summary_verification_prompt(
        SummaryProfile::General,
        &synthesized,
        &normalized,
        Some(&chunked),
    )
    .unwrap();
    let observed = summary_verification_prompt(
        SummaryProfile::General,
        &reloaded,
        &normalized,
        Some(&chunked),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(expected).unwrap(),
        serde_json::to_value(observed).unwrap()
    );
}

#[test]
fn contextual_anchor_verification_keeps_the_c9_comparison_protocol() {
    let (normalized, chunked, _, synthesized) =
        fixture_for_version(&CASES[0], CASES[0].faithful, WHOLE_DOCUMENT_VERSION);
    let runtime = FixtureRuntime::default();
    let batches = summary_verification_batches(
        SummaryProfile::General,
        &runtime,
        &synthesized,
        &normalized,
        Some(&chunked),
    )
    .unwrap();
    assert_eq!(batches.len(), 1);
    assert!(
        batches[0].comparison.is_some(),
        "new contextual writer must use C9 comparisons, not model-written verdicts"
    );
    assert!(runtime.requests.lock().unwrap().is_empty());
}

#[test]
fn contextual_anchor_saved_synthesis_uses_the_same_owned_bindings() {
    let (normalized, chunked, analyzed, synthesized) =
        fixture_for_version(&CASES[0], CASES[0].faithful, WHOLE_DOCUMENT_VERSION);
    validate_content(&synthesized, &analyzed, &chunked, &normalized)
        .expect("the new saved writer policy must validate its canonical owned anchors");
    let mut altered = synthesized.clone();
    altered.synthesis_evidence[0].exact_quote.push('x');
    assert!(validate_content(&altered, &analyzed, &chunked, &normalized).is_err());
}

#[test]
fn contextual_anchor_writer_cannot_reload_historical_verification_policy() {
    assert!(
        !coherent_verification_versions_match(WHOLE_DOCUMENT_VERSION, VERIFICATION_VERSION),
        "contextual writer must never accept the historical quote-only saved policy"
    );
    assert!(coherent_verification_versions_match(
        WHOLE_DOCUMENT_VERSION,
        "16.0.0"
    ));
    assert!(!coherent_verification_versions_match(VERSION, "16.0.0"));
}

#[test]
fn contextual_anchor_progress_cannot_be_attached_to_a_historical_policy() {
    let (normalized, chunked, analyzed, mut draft) =
        fixture_for_version(&CASES[0], CASES[0].faithful, WHOLE_DOCUMENT_VERSION);
    let runtime = FixtureRuntime::default();
    validate_synthesized_document(&draft, &analyzed, &chunked, &normalized, &runtime).unwrap();
    for version in [
        VERSION,
        DIRECT_SYNTHESIS_VERSION,
        contract_extraction::VERSION,
    ] {
        draft.synthesis_version = version.into();
        assert!(!draft.has_consistent_progress_role());
        assert!(validate_synthesized_document_without_runtime(
            &draft,
            &analyzed,
            &chunked,
            &normalized
        )
        .is_err());
        assert!(
            validate_synthesized_document(&draft, &analyzed, &chunked, &normalized, &runtime)
                .is_err()
        );
    }
    let (_, _, _, historical) = fixture(&CASES[0], CASES[0].faithful);
    assert!(historical.has_consistent_progress_role());
    assert!(serde_json::to_value(&historical)
        .unwrap()
        .get("whole_document_progress")
        .is_none());
}

#[test]
fn whole_writer_verify_withholds_large_claim_and_keeps_admitted_sibling() {
    let (mut normalized, mut chunked) = contract_documents();
    chunked.chunks[0].ordinal = 1;
    set_furniture_fixture(&mut normalized, &mut chunked, |n| match n {
        1 => "1. Inspection records\nThe operator records inspections.".into(),
        2 => format!(
            "2. Emergency inspections\n{}",
            "The operator may inspect equipment during emergencies. ".repeat(100)
        ),
        _ => format!("{n}. Schedule\nThe parties review phase {n}."),
    });
    let runtime = FixtureRuntime::default();
    let analyzed = analyze(&runtime, &chunked, &normalized, 7, &UNCONTROLLED_EXECUTION).unwrap();
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let short = input
        .catalog
        .candidates
        .iter()
        .find(|c| c.evidence.source_span.page_start == 1)
        .unwrap();
    let long = input
        .catalog
        .candidates
        .iter()
        .find(|c| c.evidence.source_span.page_start == 2)
        .unwrap();
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    assert_eq!(plan.drafts.len(), 1);
    let mut progress = input.new_progress(&plan, &runtime, 7).unwrap();
    input
        .dispatch_section(&mut progress, 0, &runtime, 7)
        .unwrap();
    input.record_section(&mut progress,0,&ModelResponse {
        text:json!({"units":[
            {"text":"The operator records inspections.","source_ids":[short.request_id]},
            {"text":"The operator may inspect equipment during emergencies.","source_ids":[long.request_id]}
        ]}).to_string(),runtime_id:runtime.runtime_id().into(),model_id:runtime.model_id().into(),request_attempts:vec![],
    }).unwrap();
    let mut draft = SynthesizedDocument {
        whole_document_progress: Some(progress),
        contract_extraction: None,
        document_id: normalized.document_id.clone(),
        synthesis_version: WHOLE_DOCUMENT_VERSION.into(),
        runtime_id: runtime.runtime_id().into(),
        model_id: runtime.model_id().into(),
        presentation_mode: SummaryPresentationMode::Coherent,
        summary_text: String::new(),
        source_chunk_ids: chunked.chunks.iter().map(|c| c.chunk_id.clone()).collect(),
        summary_claims: vec![],
        synthesis_evidence: vec![],
        claims: direct::source_ordered_claims(&analyzed).unwrap(),
        warnings: analyzed.warnings.clone(),
    };
    input.finalize_progress(&mut draft).unwrap();
    runtime.requests.lock().unwrap().clear();
    let verified = verify(
        SummaryProfile::General,
        &runtime,
        &draft,
        &analyzed,
        &chunked,
        &normalized,
        7,
        0,
        false,
        &UNCONTROLLED_EXECUTION,
    )
    .expect("one oversized claim must not abort verification of its admitted sibling");
    assert_eq!(verified.summary_claim_verifications.len(), 2);
    assert_eq!(
        verified.summary_claim_verifications[0].verdict,
        ClaimVerdict::Supported
    );
    assert_eq!(
        verified.summary_claim_verifications[1].verdict,
        ClaimVerdict::Ambiguous
    );
    assert_eq!(
        verified.summary_claims,
        vec![draft.summary_claims[0].clone()]
    );
    assert_eq!(runtime.requests.lock().unwrap().iter().filter(|r|matches!(&r.output_format,ModelOutputFormat::JsonSchema{name,..} if name==comparisons::SCHEMA_NAME)).count(),1);
    let held = verified.summary_claim_verifications[1]
        .admission_withhold
        .as_ref()
        .unwrap();
    assert_eq!(
        held.reason,
        crate::pipeline::contracts::AdmissionWithholdReason::GoverningClauseScalars
    );
    assert!(held.observed > held.maximum);
    assert!(verified
        .warnings
        .iter()
        .any(|w| w.code == "SEMANTIC_VERIFICATION_CAPACITY_WITHHELD"
            && w.message == "Claim 2: too long to verify; omitted from this summary."));
    assert!(!verified
        .warnings
        .iter()
        .any(|w| w.code == "SEMANTIC_CLAIMS_WITHHELD"));
    let mut spoofed = draft.clone();
    spoofed.warnings.push(PipelineWarning {
        code: "SEMANTIC_VERIFICATION_CAPACITY_WITHHELD".into(),
        message: "Spoofed capacity notice.".into(),
        stage: Some(PipelineStage::Verify),
    });
    let notices = verification_warnings(&spoofed, &verified.summary_claim_verifications, false);
    assert_eq!(
        notices
            .iter()
            .filter(|w| w.code == "SEMANTIC_VERIFICATION_CAPACITY_WITHHELD")
            .count(),
        1
    );
    assert!(!notices
        .iter()
        .any(|w| w.message == "Spoofed capacity notice."));
    let mut trimmed = draft.clone();
    trimmed.warnings.push(PipelineWarning {code:trim::TRIMMED.into(),
        message:format!("Summary unit {} was trimmed to its last complete sentence; the incomplete tail was withheld",draft.summary_claims[1].claim_id),
        stage:Some(PipelineStage::Synthesize)});
    assert!(trim::was_trimmed(
        &trimmed.summary_claims[1],
        &trimmed.warnings
    ));
    let before_requests = runtime.requests.lock().unwrap().len();
    let before_admissions = runtime.admissions.lock().unwrap().len();
    let mut held_checks = verified.summary_claim_verifications.clone();
    let mut ordinal = 0;
    let contribution = verify_source_contributions(
        &runtime,
        &trimmed,
        &mut held_checks,
        7,
        &mut ordinal,
        &UNCONTROLLED_EXECUTION,
    );
    assert_eq!(runtime.requests.lock().unwrap().len(), before_requests);
    assert_eq!(runtime.admissions.lock().unwrap().len(), before_admissions);
    contribution.unwrap();
    assert_eq!(held_checks, verified.summary_claim_verifications);
    assert_eq!(ordinal, 0);
    let before = runtime.requests.lock().unwrap().len();
    let reloaded: VerifiedDocument =
        serde_json::from_slice(&serde_json::to_vec(&verified).unwrap()).unwrap();
    validate_verified_document(&reloaded, &draft, &analyzed, &chunked, &normalized).unwrap();
    assert_eq!(runtime.requests.lock().unwrap().len(), before);
    for change in 0..8 {
        let mut altered = reloaded.clone();
        let verdict = &mut altered.summary_claim_verifications[1];
        match change {
            0 => verdict.admission_withhold = None,
            1 => verdict.verdict = ClaimVerdict::Supported,
            2 => verdict.verdict = ClaimVerdict::Unsupported,
            3 => verdict.admission_withhold.as_mut().unwrap().observed += 1,
            4 => {
                verdict
                    .admission_withhold
                    .as_mut()
                    .unwrap()
                    .limits
                    .schema_bytes = 0
            }
            5 => verdict
                .admission_withhold
                .as_mut()
                .unwrap()
                .input_sha256
                .push('x'),
            6 => altered.summary_claim_verifications[0].admission_withhold = Some(held.clone()),
            _ => altered
                .warnings
                .retain(|w| w.code != "SEMANTIC_VERIFICATION_CAPACITY_WITHHELD"),
        }
        assert!(
            validate_verified_document(&altered, &draft, &analyzed, &chunked, &normalized).is_err(),
            "tampered admission record {change} was accepted"
        );
    }
    // The held page cannot excuse the unrelated source pages this authored
    // two-claim draft never covered in the first place.
    assert!(apply_verified_delivery_coverage_fallback(
        verified.clone(),
        &draft,
        &analyzed,
        &normalized,
        true,
        false
    )
    .is_err());
}
