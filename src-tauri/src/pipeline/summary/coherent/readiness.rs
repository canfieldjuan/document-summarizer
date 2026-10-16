//! Offline measurement adapters. Never construct a live runtime here.
use super::*;
use crate::pipeline::contracts::{DocumentNormalizer, DocumentParser};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

struct OfflineRuntime {
    generation_calls: AtomicUsize,
    schema_limit: usize,
}
impl Default for OfflineRuntime {
    fn default() -> Self {
        Self {
            generation_calls: AtomicUsize::new(0),
            schema_limit: crate::pipeline::contracts::MAX_CLAIM_COMPARISON_SCHEMA_BYTES,
        }
    }
}
impl ModelRuntime for OfflineRuntime {
    fn generate(&self, _: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        self.generation_calls.fetch_add(1, Ordering::SeqCst);
        panic!("READINESS_GENERATION_FORBIDDEN")
    }
    fn preflight_request(&self, _: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        panic!("READINESS_RUNTIME_PREFLIGHT_FORBIDDEN")
    }
    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        panic!("READINESS_RUNTIME_STARTUP_FORBIDDEN")
    }
    fn runtime_id(&self) -> &str {
        "offline-saved-bytes"
    }
    fn model_id(&self) -> &str {
        "offline-saved-bytes"
    }
    fn context_tokens(&self, _: PipelineStage) -> u32 {
        32_768
    }
    fn response_schema_byte_limit(&self, _: PipelineStage, _: &str) -> usize {
        self.schema_limit
    }
}

fn invalid_readiness_input() -> PipelineFailure {
    stage_failure(
        PipelineStage::Verify,
        "READINESS_INPUT_MISMATCH",
        "Frozen measurement inputs do not match the planned request",
        false,
    )
}

fn replay_saved(
    draft: &SynthesizedDocument,
    normalized: &NormalizedDocument,
    raw: &str,
    expected_request: Option<&Value>,
    runtime: &OfflineRuntime,
) -> Result<(ClaimVerdict, ClaimVerdict), PipelineFailure> {
    let batches = verification::summary_verification_batches(
        SummaryProfile::General,
        runtime,
        draft,
        normalized,
    )?;
    let [batch] = batches.as_slice() else {
        return Err(invalid_readiness_input());
    };
    let (ordinal, seed) = if let Some(expected) = expected_request {
        (
            expected["ordinal"]
                .as_u64()
                .ok_or_else(invalid_readiness_input)?,
            expected["seed"]
                .as_u64()
                .ok_or_else(invalid_readiness_input)?,
        )
    } else {
        (0, 7)
    };
    let ordinal = u32::try_from(ordinal).map_err(|_| invalid_readiness_input())?;
    let request = verification_request(batch, ordinal, seed);
    if let Some(expected) = expected_request {
        let ModelOutputFormat::JsonSchema { name, schema } = &request.output_format else {
            return Err(invalid_readiness_input());
        };
        let decoder = crate::pipeline::model::response_format(&request.output_format)
            .map_err(|_| invalid_readiness_input())?;
        let decoder = serde_json::to_string(&decoder.ok_or_else(invalid_readiness_input)?)
            .map_err(|_| invalid_readiness_input())?;
        if expected["system_prompt"] != request.system_prompt
            || expected["user_prompt"] != request.user_prompt
            || expected["schema"] != *schema
            || expected["schema_name"] != *name
            || expected["stage"] != json!(request.stage)
            || expected["max_output_tokens"] != request.max_output_tokens
            || expected["decoder_schema_json"] != decoder
        {
            return Err(invalid_readiness_input());
        }
    }
    let parsed = batch
        .comparison
        .as_ref()
        .ok_or_else(invalid_readiness_input)?
        .parse(raw, &batch.claims[0])?;
    let initial = parsed.verdict.clone();
    let mut verdicts = vec![parsed];
    apply_semantic_fidelity_guards(
        &draft.summary_claims,
        &draft.synthesis_evidence,
        &mut verdicts,
    )?;
    Ok((initial, verdicts[0].verdict.clone()))
}

fn inventory(normalized: &NormalizedDocument) -> Value {
    let furniture = page_furniture::Furniture::for_synthesis_version(normalized, VERSION);
    let clauses = whole_clauses::Clauses::new(normalized, &furniture, ANALYSIS_VERSION);
    let mut sources = BTreeSet::new();
    let mut expanded = BTreeSet::new();
    let mut quote_only = BTreeSet::new();
    let mut offered_fragments = 0;
    let mut omitted_source_units = 0;
    for block in normalized.pages.iter().flat_map(|p| &p.content) {
        let segmentation = clauses.segment(block);
        omitted_source_units += segmentation.omitted_source_units;
        for quote in segmentation.segments {
            offered_fragments += 1;
            let source = if let Some(context) = clauses.context(&block.block_id, &quote) {
                expanded.insert(context.clone());
                context
            } else {
                quote_only.insert(quote.clone());
                quote
            };
            sources.insert(source);
        }
    }
    let mut sizes = sources
        .iter()
        .map(|s| s.chars().count())
        .collect::<Vec<_>>();
    sizes.sort_unstable();
    let percentile = |p: usize| {
        sizes
            .get((sizes.len() * p).div_ceil(100).saturating_sub(1))
            .copied()
    };
    json!({
        "source_context_scalar_sizes": sizes,
        "unique_effective_source_contexts": sources.len(),
        "expanded_context_values": expanded.len(),
        "quote_only_values": quote_only.len(),
        "offered_fragments": offered_fragments,
        "omitted_source_units": omitted_source_units,
        "over_4096": sizes.iter().filter(|&&n| n > 4096).count(),
        "distribution": {"min":sizes.first(),"p50":percentile(50),"p90":percentile(90),"max":sizes.last()},
        "buckets": {"1_to_240":sizes.iter().filter(|&&n|n<=240).count(),
            "241_to_4095":sizes.iter().filter(|&&n|(241..4096).contains(&n)).count(),
            "4096":sizes.iter().filter(|&&n|n==4096).count(),
            "over_4096":sizes.iter().filter(|&&n|n>4096).count()},
        "visual_pages":normalized.pages.iter().filter(|p|p.requires_visual_processing).count(),
        "measurement_complete":normalized.pages.iter().all(|p|!p.requires_visual_processing),
        "scope":"unique effective source strings offered by General Clauses; context() or unchanged quote fallback; not claim admission or all logical clauses"
    })
}

fn public_document(texts: &[String]) -> NormalizedDocument {
    use crate::pipeline::contracts::{NormalizedBlockKind, NormalizedPage, SourceSpan, SourceType};
    NormalizedDocument {
        document_id: "public-readiness-control".into(),
        normalization_version: "1.0.0".into(),
        pages: texts
            .iter()
            .enumerate()
            .map(|(i, text)| NormalizedPage {
                page_number: i as u32 + 1,
                content: vec![NormalizedBlock {
                    block_id: format!("block-{i}"),
                    kind: NormalizedBlockKind::Text,
                    text: text.clone(),
                    source: SourceSpan {
                        page_start: i as u32 + 1,
                        page_end: i as u32 + 1,
                        section_id: None,
                        source_type: SourceType::NativeText,
                    },
                }],
                warnings: vec![],
                requires_visual_processing: false,
            })
            .collect(),
        warnings: vec![],
    }
}

fn public_draft(claim: &str) -> (NormalizedDocument, SynthesizedDocument) {
    let source = "The operator should inspect the pump.";
    let normalized = public_document(&[source.into()]);
    let fixture: Value = serde_json::from_str(include_str!(
        "../comparisons/fixtures/c9-sentence-public.json"
    ))
    .unwrap();
    let mut draft: SynthesizedDocument =
        serde_json::from_value(fixture["documents"][0]["synthesized"].clone()).unwrap();
    let evidence = EvidenceItem {
        evidence_id: "public-evidence".into(),
        chunk_id: "public-chunk".into(),
        block_id: normalized.pages[0].content[0].block_id.clone(),
        claim_text: source.into(),
        exact_quote: source.into(),
        source_span: normalized.pages[0].content[0].source.clone(),
    };
    draft.document_id = normalized.document_id.clone();
    draft.summary_claims = vec![CitedClaim {
        claim_id: "public-claim".into(),
        text: claim.into(),
        evidence_ids: vec![evidence.evidence_id.clone()],
    }];
    draft.synthesis_evidence = vec![evidence];
    (normalized, draft)
}

fn public_response(
    draft: &SynthesizedDocument,
    normalized: &NormalizedDocument,
    verdict: ClaimVerdict,
) -> String {
    let batches = verification::summary_verification_batches(
        SummaryProfile::General,
        &OfflineRuntime::default(),
        draft,
        normalized,
    )
    .unwrap();
    comparisons::fixture_verdict_response(&verification_request(&batches[0], 0, 7), verdict)
}

#[test]
fn readiness_required_saved_bytes_preserve_verdict_and_apply_guard() {
    let runtime = OfflineRuntime::default();
    for (claim, wanted) in [
        (
            "The operator should inspect the pump.",
            ClaimVerdict::Supported,
        ),
        (
            "The operator must inspect the pump.",
            ClaimVerdict::Unsupported,
        ),
    ] {
        let (normalized, draft) = public_draft(claim);
        let raw = public_response(&draft, &normalized, ClaimVerdict::Supported);
        let (parsed, final_verdict) =
            replay_saved(&draft, &normalized, &raw, None, &runtime).unwrap();
        assert_eq!(parsed, ClaimVerdict::Supported);
        assert_eq!(final_verdict, wanted);
        let raw = public_response(&draft, &normalized, ClaimVerdict::Unsupported);
        assert_eq!(
            replay_saved(&draft, &normalized, &raw, None, &runtime)
                .unwrap()
                .0,
            ClaimVerdict::Unsupported
        );
    }
    assert_eq!(runtime.generation_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn readiness_required_clause_inventory_counts_unicode_boundaries() {
    let texts = [4095, 4096, 4097]
        .into_iter()
        .enumerate()
        .map(|(i, size)| {
            let prefix = format!("{}. Rule\n", i + 1);
            format!("{prefix}{}", "é".repeat(size - prefix.chars().count()))
        })
        .collect::<Vec<_>>();
    let report = inventory(&public_document(&texts));
    assert_eq!(
        report["source_context_scalar_sizes"],
        json!([4095, 4096, 4097]),
        "General owner must expose exact Unicode scalar counts"
    );
    assert_eq!(report["over_4096"], 1);
}

#[test]
fn readiness_saved_bytes_reject_invalid_and_wrong_owned_strings() {
    let runtime = OfflineRuntime::default();
    let (normalized, draft) = public_draft("The operator should inspect the pump.");
    let raw = public_response(&draft, &normalized, ClaimVerdict::Supported);
    let mut unknown: Value = serde_json::from_str(&raw).unwrap();
    unknown["verdicts"][0]["comparisons"]["stage"]["source_spans"] =
        json!(["invented private source"]);
    let mut missing: Value = serde_json::from_str(&raw).unwrap();
    missing["verdicts"][0]["comparisons"]
        .as_object_mut()
        .unwrap()
        .remove("scope");
    let mut wrong_id: Value = serde_json::from_str(&raw).unwrap();
    wrong_id["verdicts"][0]["claim_id"] = json!("foreign-claim");
    for invalid in [
        "{}".into(),
        "not json".into(),
        unknown.to_string(),
        missing.to_string(),
        wrong_id.to_string(),
    ] {
        assert_eq!(
            replay_saved(&draft, &normalized, &invalid, None, &runtime)
                .unwrap_err()
                .code,
            "MODEL_VERIFICATION_RESPONSE_INVALID"
        );
    }
    let mut empty = draft.clone();
    empty.summary_claims.clear();
    assert!(replay_saved(&empty, &normalized, &raw, None, &runtime).is_err());
    assert_eq!(runtime.generation_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn readiness_saved_bytes_bind_exact_planned_request() {
    let runtime = OfflineRuntime::default();
    let (normalized, draft) = public_draft("The operator should inspect the pump.");
    let batches = verification::summary_verification_batches(
        SummaryProfile::General,
        &runtime,
        &draft,
        &normalized,
    )
    .unwrap();
    let request = verification_request(&batches[0], 3, 19);
    let ModelOutputFormat::JsonSchema { name, schema } = &request.output_format else {
        unreachable!()
    };
    let expected = json!({"ordinal":3,"seed":19,"system_prompt":request.system_prompt,
        "user_prompt":request.user_prompt,"schema_name":name,"schema":schema,
        "stage":request.stage,"max_output_tokens":request.max_output_tokens,
        "decoder_schema_json":serde_json::to_string(&crate::pipeline::model::response_format(&request.output_format).unwrap().unwrap()).unwrap()});
    let raw = public_response(&draft, &normalized, ClaimVerdict::Supported);
    assert!(replay_saved(&draft, &normalized, &raw, Some(&expected), &runtime).is_ok());
    for field in [
        "ordinal",
        "seed",
        "system_prompt",
        "user_prompt",
        "schema_name",
        "schema",
        "stage",
        "max_output_tokens",
        "decoder_schema_json",
    ] {
        let mut changed = expected.clone();
        changed[field] = Value::Null;
        assert_eq!(
            replay_saved(&draft, &normalized, &raw, Some(&changed), &runtime)
                .unwrap_err()
                .code,
            "READINESS_INPUT_MISMATCH",
            "{field}"
        );
    }
    assert_eq!(runtime.generation_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn readiness_clause_inventory_keeps_parent_and_cross_block_context() {
    let parent = "1. Scope\nThe operator should inspect the pump.";
    let child = "1.1 Repairs\nA technician may repair the pump.";
    let continuation = "The technician must record repairs.";
    let normalized = public_document(&[parent.into(), child.into(), continuation.into()]);
    let report = inventory(&normalized);
    let mut expected = vec![
        parent.chars().count(),
        format!("{parent}\n\n{child}\n\n{continuation}")
            .chars()
            .count(),
    ];
    expected.sort_unstable();
    assert_eq!(report["source_context_scalar_sizes"], json!(expected));
    assert_eq!(report["expanded_context_values"], 1);
    assert_eq!(report["over_4096"], 0);
    assert_eq!(
        inventory(&public_document(&[]))["source_context_scalar_sizes"],
        json!([])
    );
}

#[test]
fn readiness_runtime_tripwire_rejects_generation_and_native_preflight() {
    let runtime = OfflineRuntime::default();
    let (normalized, draft) = public_draft("The operator should inspect the pump.");
    let batches = verification::summary_verification_batches(
        SummaryProfile::General,
        &runtime,
        &draft,
        &normalized,
    )
    .unwrap();
    let request = verification_request(&batches[0], 0, 7);
    assert!(std::panic::catch_unwind(|| runtime.generate(&request)).is_err());
    assert_eq!(runtime.generation_calls.load(Ordering::SeqCst), 1);
    assert!(std::panic::catch_unwind(|| runtime.preflight_request(&request)).is_err());
    assert!(std::panic::catch_unwind(|| runtime.health()).is_err());
}

fn frozen_value(spec: &Value) -> Value {
    let bytes = std::fs::read(spec["path"].as_str().expect("frozen path"))
        .expect("frozen artifact unreadable");
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        spec["sha256"].as_str().expect("frozen hash"),
        "frozen input changed"
    );
    serde_json::from_slice(&bytes).expect("frozen artifact JSON")
}

#[test]
#[ignore = "accepted offline A2/A3 only; explicit frozen input/output packet required"]
fn readiness_frozen_offline_measurements() {
    let packet: Value = serde_json::from_slice(
        &std::fs::read(std::env::var_os("DOC_SUM_READINESS_INPUT").expect("explicit input packet"))
            .unwrap(),
    )
    .unwrap();
    let runtime = OfflineRuntime::default();
    let draft: SynthesizedDocument =
        serde_json::from_value(frozen_value(&packet["a1"]["draft"])).unwrap();
    let normalized: NormalizedDocument =
        serde_json::from_value(frozen_value(&packet["a1"]["normalized"])).unwrap();
    let raw = frozen_value(&packet["a1"]["raw"]);
    let request = frozen_value(&packet["a1"]["request"]);
    assert_eq!(raw["truncated"], false);
    assert_eq!(raw["stop_type"], "eos");
    let before = serde_json::to_value(&draft).unwrap();
    let (parsed, final_verdict) = replay_saved(
        &draft,
        &normalized,
        raw["content"].as_str().unwrap(),
        Some(&request),
        &runtime,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&draft).unwrap(),
        before,
        "persisted draft was mutated"
    );
    let reasons = draft
        .summary_claims
        .iter()
        .map(|claim| {
            let evidence = draft
                .synthesis_evidence
                .iter()
                .filter(|e| claim.evidence_ids.contains(&e.evidence_id))
                .collect::<Vec<_>>();
            semantic_support::relation_guard_rejection_reasons(&claim.text, &evidence)
        })
        .collect::<Vec<_>>();
    let mut rows = vec![];
    let fixture: Value = serde_json::from_str(include_str!(
        "../comparisons/fixtures/c9-sentence-public.json"
    ))
    .unwrap();
    for doc in fixture["documents"].as_array().unwrap() {
        let normalized: NormalizedDocument =
            serde_json::from_value(doc["normalized"].clone()).unwrap();
        let draft: SynthesizedDocument =
            serde_json::from_value(doc["synthesized"].clone()).unwrap();
        let mut row = inventory(&normalized);
        row["alias"] = doc["alias"].clone();
        row["input_kind"] = json!("frozen public normalized artifact; not fresh PDF parse");
        row["claims"] = json!(draft.summary_claims.len());
        let prompt =
            verification::summary_verification_prompt(SummaryProfile::General, &draft, &normalized)
                .unwrap();
        let mut claims = vec![];
        for (input, claim) in prompt.claims.iter().zip(&draft.summary_claims) {
            let sources = input
                .evidence
                .iter()
                .map(|e| e.full_clause.as_deref().unwrap_or(&e.exact_quote))
                .collect::<BTreeSet<_>>();
            let scalars = input.text.chars().count()
                + sources.iter().map(|s| s.chars().count()).sum::<usize>();
            let source_texts = sources.iter().copied().collect::<Vec<_>>();
            let individual_source_catalogs_admit = source_texts
                .iter()
                .all(|s| comparisons::readiness_catalog_size(&[s]).is_ok());
            let source_catalog = comparisons::readiness_catalog_size(&source_texts);
            let claim_catalog = comparisons::readiness_catalog_size(&[&input.text]);
            // Match the planner's complete-source input to Prepared::new. The
            // General owner uses the unchanged quote when it has no larger context.
            let mut prepared_input = input.clone();
            for evidence in &mut prepared_input.evidence {
                if evidence.full_clause.is_none() {
                    evidence.full_clause = Some(evidence.exact_quote.clone());
                }
            }
            let catalog_diagnostic = json!({
                "source_catalog_values":source_catalog.as_ref().ok(),
                "claim_catalog_values":claim_catalog.as_ref().ok(),
                "source_union_over_8192":source_catalog.is_err() && individual_source_catalogs_admit,
                "native_prepared_admits":comparisons::readiness_prepared_admits(&prepared_input,crate::pipeline::contracts::MAX_CLAIM_COMPARISON_SCHEMA_BYTES),
                "gateway_prepared_admits":comparisons::readiness_prepared_admits(&prepared_input,250_000),
                "individual_source_catalogs_admit":individual_source_catalogs_admit,
                "claim_catalog_admits":claim_catalog.is_ok()
            });
            let mut attempts = vec![];
            for (route, schema_limit) in [
                (
                    "native",
                    crate::pipeline::contracts::MAX_CLAIM_COMPARISON_SCHEMA_BYTES,
                ),
                ("gateway", 250_000),
            ] {
                let rt = OfflineRuntime {
                    schema_limit,
                    ..Default::default()
                };
                let p = VerificationPrompt {
                    claims: vec![input.clone()],
                };
                let result =
                    comparisons::plan(&rt, &p, std::slice::from_ref(claim), MAX_SUMMARY_CLAIMS);
                attempts.push(json!({"route":route,"admitted":result.is_ok(),"failure":result.err().map(|e|e.code)}));
                assert_eq!(rt.generation_calls.load(Ordering::SeqCst), 0);
            }
            claims.push(json!({"claim_id":claim.claim_id,"claim_plus_unique_source_scalars":scalars,"over_4096":scalars>4096,"catalogs":catalog_diagnostic,"planning":attempts}));
        }
        row["claim_plans"] = json!(claims);
        rows.push(row);
    }
    let manifest = frozen_value(&packet["dv_manifest"]);
    assert_eq!(
        packet["dv_manifest"]["sha256"],
        "d618b0708173dc13f765c319e58789d60cbdccd536ee54f03c97827202b7bcba"
    );
    let specs = packet["dv_pdfs"].as_array().unwrap();
    let wanted = (7..=18).map(|n| format!("DV{n:02}")).collect::<Vec<_>>();
    assert_eq!(
        specs
            .iter()
            .map(|s| s["alias"].as_str().unwrap().to_string())
            .collect::<Vec<_>>(),
        wanted
    );
    for spec in specs {
        let alias = spec["alias"].as_str().unwrap();
        let manifest_row = manifest["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["alias"] == alias)
            .unwrap();
        assert_eq!(manifest_row["sha256"], spec["sha256"]);
        assert_eq!(manifest_row["kind"], "text");
        let (ingested, _) = crate::pipeline::ingest::prepare_pdf_ingestion(
            spec["path"].as_str().unwrap(),
            Some(alias),
        )
        .expect("frozen PDF ingestion failed");
        assert_eq!(ingested.content_hash, spec["sha256"].as_str().unwrap());
        let parsed = crate::pipeline::parser::PdfExtractParser::new().parse(&ingested);
        let mut row = match parsed
            .and_then(|p| crate::pipeline::normalize::CanonicalNormalizer::new().normalize(&p))
        {
            Ok(normalized) => inventory(&normalized),
            Err(error) => json!({"unmeasured":true,"failure":error.code}),
        };
        row["alias"] = json!(alias);
        row["input_sha256"] = spec["sha256"].clone();
        row["claims"] = Value::Null;
        row["claim_admission"] = json!("unmeasured: no saved claim/source set");
        rows.push(row);
    }
    assert_eq!(runtime.generation_calls.load(Ordering::SeqCst), 0);
    let report = json!({"a2":{"parsed":parsed,"final":final_verdict,"guard_rejection_reasons":reasons,"exact_logical_request_match":true,"draft_unchanged":true},"a3":rows,"generation_calls":0,"native_preflights":0,"runtime_startups":0,"labels_read":false,"note":"offline replay and source-size lower bounds only; not qualification"});
    let output = std::env::var_os("DOC_SUM_READINESS_OUTPUT").expect("explicit output");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(Path::new(&output))
        .unwrap();
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(&report).unwrap())
        .unwrap();
    println!("READINESS_OFFLINE_RESULT {}", report);
}

#[test]
fn readiness_clause_inventory_marks_visual_pages_incomplete() {
    let mut normalized = public_document(&[
        "1. Inspection\nThe operator inspects the pump.".into(),
        "".into(),
    ]);
    normalized.pages[1].content.clear();
    normalized.pages[1].requires_visual_processing = true;
    let report = inventory(&normalized);
    assert_eq!(report["measurement_complete"], false);
    assert_eq!(report["visual_pages"], 1);
    assert_eq!(report["unique_effective_source_contexts"], 1);
}

#[test]
fn readiness_catalog_diagnostic_uses_existing_admission() {
    assert!(comparisons::readiness_catalog_size(&["The operator should inspect."]).is_ok());
    assert!(comparisons::readiness_catalog_size(&[&"x".repeat(241)]).is_err());
    assert!(comparisons::readiness_catalog_size(&[]).is_err());
}
