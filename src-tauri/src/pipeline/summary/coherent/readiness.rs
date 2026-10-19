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
        None,
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
        None,
    )
    .unwrap();
    comparisons::fixture_verdict_response(&verification_request(&batches[0], 0, 7), verdict)
}

fn dependency_context(normalized: &NormalizedDocument) -> String {
    let projection = whole_clauses::DependencyProjection::new(normalized);
    assert!(
        projection.blocking_reasons(0).is_empty(),
        "public control must admit: {:?}",
        projection.blocking_reasons(0)
    );
    projection.context(0, normalized)
}

#[test]
fn whole_document_cross_section_dependency_preserves_governing_exception() {
    let normalized = public_document(&[
        "1. Permission\nThe operator may use the equipment subject to Section 3.".into(),
        "2. Storage\nThe supplier stores spare equipment indoors.".into(),
        "3. Exceptions\nThis permission excludes exterior work.".into(),
    ]);
    let governing_context = dependency_context(&normalized);
    println!("PUBLIC_DEPENDENCY_CONTEXT={governing_context}");
    assert!(
        governing_context.contains("This permission excludes exterior work."),
        "the section reader omitted the explicitly referenced governing exception"
    );
}

#[test]
fn whole_document_dependencies_multi_hop_parent_cycle() {
    let normalized = public_document(&[
        "1. Permission\nThe operator may use equipment subject to Section 3.1.".into(),
        "2. Unrelated\nThe warehouse closes at noon.".into(),
        "3. Exceptions\nExterior work is excluded.".into(),
        "3.1 Conditions\nSubject to Section 4, equipment stays indoors.".into(),
        "4. Conditions\nPermission follows Section 1.".into(),
    ]);
    let text = dependency_context(&normalized);
    assert!(
        text.contains("Exterior work is excluded."),
        "dependency parent missing"
    );
    assert!(
        text.contains("Permission follows Section 1."),
        "transitive edge missing"
    );
    assert!(!text.contains("warehouse"), "unrelated clause attached");
    assert_eq!(
        text.matches("The operator may use").count(),
        1,
        "cycle repeated its root"
    );
}

#[test]
fn whole_document_dependencies_article_list_range_and_symbol() {
    for reference in [
        "Article III",
        "Sections 3 and 4",
        "Sections 3, 4 and 5",
        "Sections 3 through 5",
        "§ 3",
    ] {
        let normalized = public_document(&[
            format!("1. Permission\nThe operator may use equipment subject to {reference}."),
            "ARTICLE III - EXCEPTIONS\nExterior work is excluded.".into(),
            "3. Scope\nOutdoor work is excluded.".into(),
            "4. Conditions\nWet work is excluded.".into(),
            "5. Conditions\nRoof work is excluded.".into(),
        ]);
        let text = dependency_context(&normalized);
        let required = if reference.starts_with("Article") {
            "Exterior work is excluded."
        } else {
            "Outdoor work is excluded."
        };
        assert!(
            text.contains(required),
            "{reference}: governing target missing"
        );
        if reference.contains('4') || reference.contains("through") {
            assert!(
                text.contains("Wet work is excluded."),
                "{reference}: list/range member missing"
            );
        }
        if reference.contains('5') {
            assert!(
                text.contains("Roof work is excluded."),
                "{reference}: final member missing"
            );
        }
    }
}

#[test]
fn whole_document_dependencies_status_boundaries_and_external() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    // Accepted annex/range amendment: absent annexes disclose; absent range
    // endpoints still block, using the actual owned-heading lookup reason.
    for (reference, expected) in [
        ("Section 3", None),
        ("Section 99", Some("missing_target")),
        ("paragraph 3", Some("unsupported_form")),
        ("subsection (b)", Some("unsupported_form")),
        ("Exhibit A", None),
        ("Schedule 2", None),
        ("Section 3(b)", Some("unsupported_form")),
        ("Sections 3-5", Some("missing_target")),
        ("Sections 3 through 2", Some("missing_target")),
        ("Sections 3 and (b)", Some("unsupported_form")),
    ] {
        let normalized = public_document(&[
            format!("§ 1 Permission\nThe operator may use equipment subject to {reference}."),
            "§ 3 Exceptions\nExterior work is excluded.".into(),
        ]);
        let p = DependencyProjection::new(&normalized);
        let reasons = p.blocking_reasons(0);
        assert_eq!(
            reasons.first().copied(),
            expected,
            "{reference}: {reasons:?}"
        );
        assert_eq!(
            p.references[0].len(),
            1,
            "heading counted as a reference: {reference}"
        );
        assert!(p.excluded_heading_references >= 2);
        for r in p.references.iter().flatten() {
            let block = normalized
                .pages
                .iter()
                .flat_map(|p| &p.content)
                .find(|b| b.block_id == r.span.block_id)
                .unwrap();
            assert_eq!(r.text, block.text[r.span.start..r.span.end]);
        }
        assert_eq!(p.ids, DependencyProjection::new(&normalized).ids);
    }
    let normalized = public_document(&[
        "1. Permission\nThe operator follows Section 3 of Other Document X201-2020 and the Prime Contract.".into(),
        "3. Local\nExterior work is excluded.".into(),
    ]);
    let p = DependencyProjection::new(&normalized);
    // Only the explicit numbered qualifier is a dependency. The bare name
    // remains source text; it cannot establish an absent document.
    assert_eq!(p.references[0].len(), 1);
    assert!(p.references[0]
        .iter()
        .all(|r| matches!(r.status, ReferenceStatus::External(_))));
    assert!(p.blocking_reasons(0).is_empty());
    assert!(
        !p.context(0, &normalized)
            .contains("Exterior work is excluded."),
        "same-number local target attached to external reference"
    );
    assert!(p
        .context(0, &normalized)
        .contains("Other Document X201-2020"));
    // Ownership twin: the reference is present, the external document's alleged
    // exclusion has no owned source. This is not a semantic model verdict.
    assert!(!p
        .context(0, &normalized)
        .contains("Other Document forbids roof work"));
    let duplicate = public_document(&[
        "1. Permission\nPermission follows Section 3.".into(),
        "3. First\nThe first rule applies.".into(),
        "3. Second\nThe second rule applies.".into(),
    ]);
    assert!(DependencyProjection::new(&duplicate)
        .blocking_reasons(0)
        .contains(&"duplicate_target"));
    let mixed = public_document(&[
        "1. Permission\nPermission follows Section 3 and Section 99 and Section 5 of Other Document A200.".into(),
        "3. Scope\nExterior work is excluded.".into(),
    ]);
    let p = DependencyProjection::new(&mixed);
    assert!(p.blocking_reasons(0).contains(&"missing_target"));
    assert_eq!(
        p.references[0]
            .iter()
            .filter(|r| matches!(r.status, ReferenceStatus::Resolved(_)))
            .count(),
        1
    );
    assert_eq!(
        p.references[0]
            .iter()
            .filter(|r| matches!(r.status, ReferenceStatus::External(_)))
            .count(),
        1
    );
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
        None,
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
        None,
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
        let prompt = verification::summary_verification_prompt(
            SummaryProfile::General,
            &draft,
            &normalized,
            None,
        )
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

#[test]
#[ignore = "accepted source-only dependency measurement; explicit frozen text packet and private output required"]
fn whole_document_dependency_frozen_source_measurement() {
    use crate::pipeline::contracts::{ParsedDocument, ParsedPage, SourceType};
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    let packet: Value = serde_json::from_slice(
        &std::fs::read(
            std::env::var_os("DOC_SUM_DEPENDENCIES_INPUT").expect("explicit source packet"),
        )
        .unwrap(),
    )
    .unwrap();
    let specs = packet["inputs"].as_array().unwrap();
    assert_eq!(specs.len(), 2);
    let mut rows = Vec::new();
    for spec in specs {
        let bytes = std::fs::read(spec["path"].as_str().unwrap()).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            spec["sha256"].as_str().unwrap(),
            "frozen source changed"
        );
        let text = String::from_utf8(bytes).unwrap();
        // The baseline inputs are plain text, not PDFs. Feed that complete
        // text to the real normalizer, with no invented page breaks or sampling.
        let parsed = ParsedDocument {
            document_id: spec["alias"].as_str().unwrap().into(),
            parser_id: "frozen-preproduction-text".into(),
            parser_version: "1.0.0".into(),
            source_type: SourceType::NativeText,
            pages: vec![ParsedPage {
                page_number: 1,
                text,
                warnings: vec![],
                requires_visual_processing: false,
            }],
            warnings: vec![],
        };
        let normalized = crate::pipeline::normalize::CanonicalNormalizer::new()
            .normalize(&parsed)
            .unwrap();
        let projection = DependencyProjection::new(&normalized);
        let mut counts = json!({"resolved":0,"external":0,"unresolved":0});
        let mut units = Vec::new();
        let mut reason_counts = std::collections::BTreeMap::<String, usize>::new();
        let mut blocked_units = 0;
        for (index, clause) in projection.clauses.iter().enumerate() {
            let reasons = projection.blocking_reasons(index);
            blocked_units += usize::from(!reasons.is_empty());
            for reason in &reasons {
                *reason_counts.entry((*reason).into()).or_default() += 1;
            }
            let refs = projection.references[index].iter().map(|r| {
                let (status, detail) = match &r.status {
                    ReferenceStatus::Resolved(indices) => ("resolved", json!({"target_ids":indices.iter().map(|&i| &projection.ids[i]).collect::<Vec<_>>()})),
                    ReferenceStatus::External(document) => ("external", json!({"document":document})),
                    ReferenceStatus::Unresolved(reason) => ("unresolved", json!({"reason":reason})),
                };
                counts[status] = json!(counts[status].as_u64().unwrap()+1);
                json!({"text":r.text,"span":{"block_id":r.span.block_id,"start":r.span.start,"end":r.span.end},"status":status,"detail":detail})
            }).collect::<Vec<_>>();
            units.push(json!({"id":projection.ids[index],"number":clause.number,"article":clause.article,"annex":clause.annex,"boundary_uncertain":clause.boundary_uncertain,
                "source_fragments":clause.fragments.iter().map(|f| json!({"block_id":f.block_id,"start":f.start,"end":f.end})).collect::<Vec<_>>(),
                "references":refs,"closure_ids":projection.closures[index].iter().map(|&i| &projection.ids[i]).collect::<Vec<_>>(),"blocking_reasons":reasons}));
        }
        rows.push(json!({"alias":spec["alias"],"input_sha256":spec["sha256"],"counts":counts,"units":units,"blocked_units":blocked_units,"reason_counts":reason_counts,
            "excluded_heading_references":projection.excluded_heading_references,
            "representation":"complete frozen plain text through CanonicalNormalizer; one source page, no reconstructed PDF pagination"}));
    }
    let report = json!({"inputs":rows,"generation_calls":0,"runtime_startups":0,"benchmark_labels_read":false,"scope":"source dependency plan only; not writer/token admission or fidelity qualification"});
    let output = std::env::var_os("DOC_SUM_DEPENDENCIES_OUTPUT").expect("explicit private output");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options
        .open(output)
        .unwrap()
        .write_all(&serde_json::to_vec_pretty(&report).unwrap())
        .unwrap();
    println!("DEPENDENCY_SOURCE_MEASUREMENT_SAVED; source-only, zero generation; inspect private report for gate");
}

#[test]
fn dependency_repair_parenthetical_caption_and_adjacent_suffix_twins() {
    for (reference, expected) in [
        ("Section 7 (Confidentiality)", None),
        ("Section 7; (4) keep a log", None),
        ("Section 7(b)", Some("unsupported_form")),
        ("Section 7 (b)", Some("unsupported_form")),
    ] {
        let normalized = public_document(&[
            format!("1. Permission\nPermission follows {reference}."),
            "7. Conditions\nExterior work is excluded.".into(),
        ]);
        let p = whole_clauses::DependencyProjection::new(&normalized);
        assert_eq!(
            p.blocking_reasons(0).first().copied(),
            expected,
            "{reference}"
        );
        if expected.is_none() {
            assert!(p
                .context(0, &normalized)
                .contains("Exterior work is excluded."));
        }
    }
}

#[test]
fn dependency_repair_current_document_qualifier_keeps_exact_span() {
    for qualifier in ["of this Agreement", "of this Contract", "of this Document"] {
        let reference = format!("Section 7 {qualifier}");
        let normalized = public_document(&[
            format!("1. Permission\nPermission follows {reference}."),
            "7. Conditions\nExterior work is excluded.".into(),
        ]);
        let p = whole_clauses::DependencyProjection::new(&normalized);
        assert_eq!(p.references[0][0].text.trim_end_matches('.'), reference);
        assert!(p.blocking_reasons(0).is_empty());
        assert!(matches!(
            p.references[0][0].status,
            whole_clauses::ReferenceStatus::Resolved(_)
        ));
    }
}

#[test]
fn dependency_repair_qualified_statute_is_external_not_same_number_local() {
    for reference in [
        "Section 105 of Title 17 of the United States Code",
        "17 U.S.C. § 105",
        "Section 105 of the Other Services Agreement",
    ] {
        let normalized = public_document(&[
            format!("1. Permission\nPermission follows {reference}."),
            "105. Local Conditions\nExterior work is excluded.".into(),
        ]);
        let p = whole_clauses::DependencyProjection::new(&normalized);
        if reference.contains("United States Code") {
            assert_eq!(p.blocking_reasons(0), vec!["ambiguous_document_qualifier"]);
            assert!(matches!(
                p.references[0][0].status,
                whole_clauses::ReferenceStatus::Unresolved("ambiguous_document_qualifier")
            ));
        } else {
            assert!(p.blocking_reasons(0).is_empty());
            assert!(matches!(
                p.references[0][0].status,
                whole_clauses::ReferenceStatus::External(_)
            ));
        }
        assert!(!p
            .context(0, &normalized)
            .contains("Exterior work is excluded."));
    }
    let normalized = public_document(&["1. Permission\nPermission follows Section 105.".into()]);
    assert!(whole_clauses::DependencyProjection::new(&normalized)
        .blocking_reasons(0)
        .contains(&"missing_target"));
}

#[test]
fn dependency_repair_ranges_select_owned_heading_identities() {
    for reference in [
        "Sections 3-3.2",
        "Sections 3–3.2",
        "Sections 3 through 3.2",
        "Sections 3.1-3.2",
    ] {
        let normalized = public_document(&[
            format!("1. Permission\nPermission follows {reference}."),
            "3. Conditions\nExterior work is excluded.".into(),
            "3.1 Conditions\nWet work is excluded.".into(),
            "3.2 Conditions\nRoof work is excluded.".into(),
            "4. Other\nThe warehouse closes at noon.".into(),
        ]);
        let p = whole_clauses::DependencyProjection::new(&normalized);
        assert!(
            p.blocking_reasons(0).is_empty(),
            "{reference}: {:?}",
            p.blocking_reasons(0)
        );
        let context = p.context(0, &normalized);
        assert!(context.contains("Wet work is excluded."));
        assert!(context.contains("Roof work is excluded."));
        assert!(!context.contains("warehouse"));
    }
    for reference in ["Sections 3–9", "Sections 3.2–3.1"] {
        let normalized = public_document(&[
            format!("1. Permission\nPermission follows {reference}."),
            "3. Conditions\nExterior work is excluded.".into(),
            "3.1 Conditions\nWet work is excluded.".into(),
            "3.2 Conditions\nRoof work is excluded.".into(),
        ]);
        assert!(
            !whole_clauses::DependencyProjection::new(&normalized)
                .blocking_reasons(0)
                .is_empty(),
            "{reference}"
        );
    }
}

#[test]
fn dependency_repair_annex_unique_absent_duplicate_uncertain_twins() {
    for kind in ["Attachment", "Appendix", "Exhibit", "Schedule"] {
        let root = format!("1. Permission\nPermission follows {kind} A.");
        let heading = format!("{kind} A - Equipment Scope\nExterior work is excluded.");
        let normalized = public_document(&[root.clone(), heading.clone()]);
        let p = whole_clauses::DependencyProjection::new(&normalized);
        assert!(
            p.blocking_reasons(0).is_empty(),
            "included {kind}: {:?}",
            p.blocking_reasons(0)
        );
        assert!(matches!(
            p.references[0][0].status,
            whole_clauses::ReferenceStatus::Resolved(_)
        ));
        assert!(p
            .context(0, &normalized)
            .contains("Exterior work is excluded."));
        let normalized = public_document(std::slice::from_ref(&root));
        let p = whole_clauses::DependencyProjection::new(&normalized);
        assert!(p.blocking_reasons(0).is_empty(), "absent {kind}");
        assert!(matches!(
            p.references[0][0].status,
            whole_clauses::ReferenceStatus::External(_)
        ));
        assert!(p
            .context(0, &normalized)
            .contains(&format!("Permission follows {kind} A")));
        assert!(
            !p.context(0, &normalized)
                .contains("Exterior work is excluded."),
            "absent content acquired evidence"
        );
        let normalized = public_document(&[root.clone(), heading.clone(), heading]);
        assert!(whole_clauses::DependencyProjection::new(&normalized)
            .blocking_reasons(0)
            .contains(&"duplicate_target"));
        let normalized = public_document(&[root, format!("{kind} A - Equipment Scope\nExterior work is excluded.\n{kind} (B OR C)\nIndoor work remains allowed.")]);
        assert!(whole_clauses::DependencyProjection::new(&normalized)
            .blocking_reasons(0)
            .contains(&"boundary_uncertain"));
    }
}

#[test]
fn dependency_repair_heading_origin_preserves_real_uncertainty() {
    let normalized = public_document(&[
        "§ 1 Permission\nPermission follows Section 3.1.\n[PAGE 2]".into(),
        "§ 3 Services Provided by the Supplier\n§ 3.1 Conditions\nExterior work is excluded."
            .into(),
    ]);
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert!(
        p.blocking_reasons(0).is_empty(),
        "recognized numeric headings/format annotation: {:?}",
        p.blocking_reasons(0)
    );
    assert!(p
        .context(0, &normalized)
        .contains("Exterior work is excluded."));
    let normalized = public_document(&["1. Permission\nPermission follows Section 3.".into(),
        "3. Conditions\nExterior work is excluded.\nUNRECOGNIZED DIVIDER\nIndoor work remains allowed.".into()]);
    assert!(whole_clauses::DependencyProjection::new(&normalized)
        .blocking_reasons(0)
        .contains(&"boundary_uncertain"));
    let normalized = public_document(&["1. Permission\nPermission follows Section 3.".into(),
        "3. Conditions\nThe supplier works subject to\nSection 4 Conditions\nIndoor work remains allowed.".into()]);
    assert!(whole_clauses::DependencyProjection::new(&normalized)
        .blocking_reasons(0)
        .contains(&"boundary_uncertain"));
}

#[test]
fn dependency_repair_caption_does_not_end_list_and_suffix_span_is_exact() {
    let normalized = public_document(&[
        "1. Permission\nPermission follows Sections 7 (Conditions) and 8.".into(),
        "7. Conditions\nExterior work is excluded.".into(),
        "8. Conditions\nRoof work is excluded.".into(),
    ]);
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert!(p.blocking_reasons(0).is_empty());
    assert!(
        p.context(0, &normalized).contains("Roof work is excluded."),
        "caption prematurely ended reference list"
    );
    let normalized = public_document(&["1. Permission\nPermission follows subsection (b).".into()]);
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert_eq!(p.references[0][0].text, "subsection (b)");
    assert!(p.blocking_reasons(0).contains(&"unsupported_form"));
}

#[test]
fn dependency_repair_compact_section_symbol_is_not_skipped() {
    for (reference, missing) in [("§7", false), ("§99", true)] {
        let normalized = public_document(&[
            format!("1. Permission\nPermission follows {reference}."),
            "7. Conditions\nExterior work is excluded.".into(),
        ]);
        let p = whole_clauses::DependencyProjection::new(&normalized);
        assert_eq!(
            p.references[0].len(),
            1,
            "compact section reference skipped"
        );
        assert_eq!(!p.blocking_reasons(0).is_empty(), missing);
        if !missing {
            assert!(p
                .context(0, &normalized)
                .contains("Exterior work is excluded."));
        }
    }
}

#[test]
fn dependency_namespace_repair_abbreviation_and_real_sentence_end_twins() {
    for (reference, reason) in [
        (
            "Section 501 of the U.S. Code",
            "ambiguous_document_qualifier",
        ),
        (
            "Section 501(c)(3) of the U.S. Internal Revenue Code",
            "unsupported_form",
        ),
    ] {
        let normalized = public_document(&[
            format!("1. Permission\nPermission follows {reference}."),
            "501. Local Rule\nExterior work is excluded.".into(),
        ]);
        let p = whole_clauses::DependencyProjection::new(&normalized);
        assert_eq!(p.blocking_reasons(0), vec![reason]);
        assert!(matches!(
            p.references[0][0].status,
            whole_clauses::ReferenceStatus::Unresolved(actual) if actual == reason
        ));
        assert_eq!(p.references[0][0].text, reference);
        assert!(!p
            .context(0, &normalized)
            .contains("Exterior work is excluded."));
    }
    let normalized = public_document(&[
        "1. Permission\nThe supplier works in the U.S. Section 3 applies.".into(),
        "3. Rule\nExterior work is excluded.".into(),
    ]);
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert!(matches!(
        p.references[0][0].status,
        whole_clauses::ReferenceStatus::Resolved(_)
    ));
    assert!(p
        .context(0, &normalized)
        .contains("Exterior work is excluded."));
}

#[test]
fn dependency_namespace_repair_preposed_code_and_owned_range_twins() {
    for reference in [
        "Example State Code Section 21-301.1(a)",
        "42 XYZ Section 21-301.1(a)",
        "42 X.Y.Z. Section 21-301.1(a)",
    ] {
        let normalized = public_document(&[
            format!("1. Permission\nPermission follows {reference}."),
            "21. First\nExterior work is excluded.".into(),
            "301.1 Second\nRoof work is excluded.".into(),
        ]);
        let p = whole_clauses::DependencyProjection::new(&normalized);
        if reference.starts_with("Example State Code") {
            assert_eq!(p.blocking_reasons(0), vec!["ambiguous_document_qualifier"]);
            assert!(matches!(
                p.references[0][0].status,
                whole_clauses::ReferenceStatus::Unresolved("ambiguous_document_qualifier")
            ));
        } else {
            assert!(p.blocking_reasons(0).is_empty());
            assert!(matches!(
                p.references[0][0].status,
                whole_clauses::ReferenceStatus::External(_)
            ));
        }
        assert!(!p.context(0, &normalized).contains("Roof work is excluded."));
    }
    let normalized = public_document(&[
        "1. Permission\nPermission follows Sections 2-4.".into(),
        "2. First\nExterior work is excluded.".into(),
        "3. Second\nRoof work is excluded.".into(),
        "4. Third\nWet work is excluded.".into(),
    ]);
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert!(p.blocking_reasons(0).is_empty());
    assert!(p.context(0, &normalized).contains("Roof work is excluded."));
    let normalized =
        public_document(&["1. Permission\nPermission follows Section 21-301.1(a).".into()]);
    // The later accepted foreign-numbering rule supersedes the former
    // unconditional missing-target result for this foreign dashed grammar.
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert!(p.blocking_reasons(0).is_empty());
    assert!(matches!(
        p.references[0][0].status,
        whole_clauses::ReferenceStatus::External(_)
    ));
}

#[test]
fn dependency_namespace_repair_annex_scope_and_conflicting_endings_twins() {
    let normalized = public_document(&[
        "1. Permission\nPermission follows Attachment A and Section 2.".into(),
        "2. Main Rule\nExterior work is excluded.".into(),
        "ATTACHMENT A\nSUPPLEMENTAL TERMS\nThe unit stays indoors.\n1. First Item\nThe supplier inspects the pump.\nARTICLE I - CONDITIONS\n2. Last Item\nRoof work is excluded.".into(),
        "ATTACHMENT B\nDifferent equipment is supplied.".into(),
    ]);
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert!(
        p.blocking_reasons(0).is_empty(),
        "{:?}",
        p.blocking_reasons(0)
    );
    let context = p.context(0, &normalized);
    assert!(context.contains("Exterior work is excluded."));
    assert!(
        context.contains("Roof work is excluded."),
        "annex lost its inner Article/numbered units"
    );
    assert!(!context.contains("Different equipment is supplied."));
    let normalized = public_document(&["1. Permission\nPermission follows Attachment A.".into(),
        "ATTACHMENT A\nExterior work is excluded.\nATTACHMENT (B OR C)\nIndoor work remains allowed.".into()]);
    assert!(whole_clauses::DependencyProjection::new(&normalized)
        .blocking_reasons(0)
        .contains(&"boundary_uncertain"));
}

#[test]
fn dependency_namespace_repair_named_document_sentence_end_is_not_preposed_scope() {
    let normalized = public_document(&[
        "1. Permission\nThe supplier follows Other Services Agreement. Section 3 applies.".into(),
        "3. Rule\nExterior work is excluded.".into(),
    ]);
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert!(p.blocking_reasons(0).is_empty());
    assert!(
        matches!(
            p.references[0][0].status,
            whole_clauses::ReferenceStatus::Resolved(_)
        ),
        "a named document before a sentence end became a preposed qualifier"
    );
    assert!(p
        .context(0, &normalized)
        .contains("Exterior work is excluded."));
}

#[test]
fn foreign_numbering_external_only_for_foreign_shape() {
    let normalized = public_document(&[
        "1. Permission\nPermission follows Section 21-301.1(a).".into(),
        "1.1 Local Rule\nExterior work is excluded.".into(),
    ]);
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert!(
        p.blocking_reasons(0).is_empty(),
        "foreign numbering: {:?}",
        p.blocking_reasons(0)
    );
    let whole_clauses::ReferenceStatus::External(external) = &p.references[0][0].status else {
        panic!("foreign number was not external");
    };
    assert_eq!(external.reason, "foreign_numbering");
    assert_eq!(external.disclosure, "text not in this document");
    assert!(p
        .context(0, &normalized)
        .contains("Permission follows Section 21-301.1(a)."));
    assert!(!p
        .context(0, &normalized)
        .contains("The foreign rule permits exterior work"));
}

#[test]
fn foreign_numbering_compatible_missing_target_must_block() {
    for reference in [
        "Section 9.4",
        "Section 9.4.2",
        "Sections 2-4",
        "Section 2-4",
    ] {
        let normalized = public_document(&[
            format!("1. Permission\nPermission follows {reference}."),
            "1.1 Scope\nEquipment remains indoors.".into(),
            "2. Scope\nExterior work is excluded.".into(),
        ]);
        let p = whole_clauses::DependencyProjection::new(&normalized);
        assert!(
            !p.blocking_reasons(0).is_empty(),
            "missing internal target became external: {reference}"
        );
        assert!(matches!(
            p.references[0][0].status,
            whole_clauses::ReferenceStatus::Unresolved(_)
        ));
    }
}

#[test]
fn foreign_numbering_owned_dashed_identity_present_missing_twins() {
    for (reference, missing) in [("Section 21-301.1", false), ("Section 21-302.1", true)] {
        let normalized = public_document(&[
            format!("1. Permission\nPermission follows {reference}."),
            "§ 21-301.1 Scope\nExterior work is excluded.".into(),
        ]);
        let p = whole_clauses::DependencyProjection::new(&normalized);
        assert_eq!(
            !p.blocking_reasons(0).is_empty(),
            missing,
            "{reference}: {:?}",
            p.blocking_reasons(0)
        );
        assert!(
            !matches!(
                p.references[0][0].status,
                whole_clauses::ReferenceStatus::External(_)
            ),
            "owned grammar lost"
        );
        if !missing {
            assert!(p
                .context(0, &normalized)
                .contains("Exterior work is excluded."));
        }
    }
}

#[test]
fn foreign_numbering_ranges_do_not_cross_numbering_families() {
    let normalized = public_document(&[
        "1. Permission\nPermission follows Sections 2-4.".into(),
        "2. First Rule\nExterior work is excluded.".into(),
        "3-999. Different Grammar\nThe unrelated engine is supplied.".into(),
        "4. Last Rule\nRoof work is excluded.".into(),
    ]);
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert!(p.blocking_reasons(0).is_empty());
    assert!(
        !p.context(0, &normalized)
            .contains("The unrelated engine is supplied."),
        "range crossed numbering families"
    );
    let normalized = public_document(&[
        "1. Permission\nPermission follows Section 2 and 21-301.1.".into(),
        "2. Rule\nExterior work is excluded.".into(),
    ]);
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert!(p.blocking_reasons(0).is_empty());
    assert_eq!(
        p.references[0]
            .iter()
            .filter(|r| matches!(r.status, whole_clauses::ReferenceStatus::Resolved(_)))
            .count(),
        1
    );
    assert_eq!(
        p.references[0]
            .iter()
            .filter(|r| matches!(r.status, whole_clauses::ReferenceStatus::External(_)))
            .count(),
        1
    );
    assert!(p
        .context(0, &normalized)
        .contains("Exterior work is excluded."));
}

#[test]
fn foreign_numbering_compound_and_range_both_owned_is_ambiguous() {
    let normalized = public_document(&[
        "1. Permission\nPermission follows Section 2-4.".into(),
        "2. First Rule\nExterior work is excluded.".into(),
        "2-4. Dashed Rule\nWet work is excluded.".into(),
        "4. Last Rule\nRoof work is excluded.".into(),
    ]);
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert!(
        p.blocking_reasons(0).contains(&"ambiguous_numbering"),
        "a reference with two owned interpretations was selected arbitrarily"
    );
    let normalized = public_document(&[
        "1. Permission\nPermission follows Sections 2-4.".into(),
        "2. First Rule\nExterior work is excluded.".into(),
        "2-4. Dashed Rule\nWet work is excluded.".into(),
        "4. Last Rule\nRoof work is excluded.".into(),
    ]);
    let p = whole_clauses::DependencyProjection::new(&normalized);
    assert!(p.blocking_reasons(0).is_empty());
    let context = p.context(0, &normalized);
    assert!(context.contains("Exterior work is excluded."));
    assert!(context.contains("Roof work is excluded."));
    assert!(!context.contains("Wet work is excluded."));
}

fn exact_string_occurrences(value: &Value, text: &str) -> usize {
    match value {
        Value::String(s) => usize::from(s == text),
        Value::Array(values) => values
            .iter()
            .map(|v| exact_string_occurrences(v, text))
            .sum(),
        Value::Object(values) => values
            .values()
            .map(|v| exact_string_occurrences(v, text))
            .sum(),
        _ => 0,
    }
}

#[test]
fn whole_writer_input_contains_complete_source_once_beyond_old_character_cap() {
    use crate::pipeline::contracts::{DocumentChunker, StructureInterpreter};
    let text = (1..=480)
        .map(|n| format!("The operator records inspection number {n}. "))
        .collect::<String>();
    assert!(text.chars().count() > 16_000);
    let normalized = public_document(std::slice::from_ref(&text));
    let parsed_layout = crate::pipeline::structure::DeterministicStructureInterpreter::new()
        .interpret(&normalized)
        .unwrap();
    let chunked = crate::pipeline::chunk::DeterministicDocumentChunker::new()
        .chunk(&normalized, &parsed_layout)
        .unwrap();
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let (prompt, schema) = input.full_projection().unwrap();
    let value: Value = serde_json::from_str(&prompt).unwrap();
    assert_eq!(
        exact_string_occurrences(&value, &text),
        1,
        "writer input does not contain the complete normalized source exactly once"
    );
    assert_eq!(
        schema["properties"]["units"]["maxItems"],
        MAX_VERIFICATION_BATCHES
    );
}

fn whole_writer_chunks(normalized: &NormalizedDocument) -> ChunkedDocument {
    use crate::pipeline::contracts::{DocumentChunker, StructureInterpreter};
    let parsed_layout = crate::pipeline::structure::DeterministicStructureInterpreter::new()
        .interpret(normalized)
        .unwrap();
    crate::pipeline::chunk::DeterministicDocumentChunker::new()
        .chunk(normalized, &parsed_layout)
        .unwrap()
}

#[test]
fn whole_writer_registry_binds_every_block_and_preserves_unicode_offsets() {
    let texts = (1..=19)
        .map(|n| format!("Inspecteur {n} records café №{n}. Safety remains required."))
        .collect::<Vec<_>>();
    let normalized = public_document(&texts);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let (wire, _) = input.full_projection().unwrap();
    let payload: Value = serde_json::from_str(&wire).unwrap();
    assert_eq!(input.catalog.omitted_source_units, 0);
    let blocks = normalized
        .pages
        .iter()
        .flat_map(|p| &p.content)
        .collect::<Vec<_>>();
    let bound = input
        .anchors
        .iter()
        .map(|a| a.span.block)
        .collect::<BTreeSet<_>>();
    assert_eq!(bound, (0..blocks.len()).collect());
    for text in &texts {
        assert_eq!(exact_string_occurrences(&payload, text), 1);
    }
    for (anchor, candidate) in input.anchors.iter().zip(&input.catalog.candidates) {
        assert_eq!(anchor.source_id, candidate.request_id);
        assert_eq!(
            &blocks[anchor.span.block].text[anchor.span.start..anchor.span.end],
            candidate.evidence.exact_quote
        );
    }
}

#[test]
fn whole_writer_keeps_uncitable_text_and_reports_existing_quote_omission() {
    let long_sentence = format!(
        "The operator {} remains responsible.",
        "explicitly ".repeat(100)
    );
    let text =
        format!("The first fact remains citable. {long_sentence} The final fact remains citable.");
    let normalized = public_document(std::slice::from_ref(&text));
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let existing = analysis_quote_segmentation_for_version(ANALYSIS_VERSION, &text);
    assert!(existing.omitted_source_units > 0);
    assert_eq!(
        input.catalog.omitted_source_units,
        existing.omitted_source_units
    );
    assert!(input.catalog.candidates.len() > existing.segments.len());
    for (byte, _) in text.char_indices().filter(|(_, c)| !c.is_whitespace()) {
        assert_eq!(
            input
                .anchors
                .iter()
                .filter(|a| a.span.start <= byte && byte < a.span.end)
                .count(),
            1
        );
    }
    let (wire, _) = input.full_projection().unwrap();
    assert_eq!(
        exact_string_occurrences(&serde_json::from_str(&wire).unwrap(), &text),
        1
    );
}

#[test]
fn whole_writer_registry_keeps_identical_quotes_at_distinct_owned_spans() {
    let repeated = "The operator must inspect the equipment.";
    let text = format!("Section 1 Inspections\n{repeated}\nSection 2 Renewals\n{repeated}");
    let normalized = public_document(&[text]);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    assert_eq!(input.catalog.candidates.len(), input.anchors.len());
    assert_eq!(
        input
            .catalog
            .candidates
            .iter()
            .map(|c| &c.evidence.evidence_id)
            .collect::<BTreeSet<_>>()
            .len(),
        input.anchors.len()
    );
    for pair in input.anchors.windows(2) {
        assert!(pair[0].span.end <= pair[1].span.start);
    }
}

#[test]
fn whole_writer_request_must_bind_new_protocol_and_system_prompt() {
    let normalized = public_document(&["The operator must inspect the equipment.".into()]);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let (prompt, schema) = input.full_projection().unwrap();
    let request = summary_request_for_version(
        SummaryProfile::General,
        WHOLE_DOCUMENT_VERSION,
        &prompt,
        &schema,
        0,
        7,
    );
    let ModelOutputFormat::JsonSchema { name, .. } = request.output_format else {
        panic!("the whole-document writer requires a schema");
    };
    assert_eq!(name, WHOLE_DOCUMENT_SCHEMA_NAME);
    assert_eq!(request.system_prompt, whole_document::SYSTEM_PROMPT);
}

// Controlled admission for planner routing tests only. This deliberately is not
// a tokenizer or evidence of native capacity; the shared budget owner calls it.
struct WriterProjectionRuntime {
    maximum_prompt_bytes: usize,
    context_tokens: u32,
    failure: Option<&'static str>,
    preflights: AtomicUsize,
}
impl WriterProjectionRuntime {
    fn new(maximum_prompt_bytes: usize) -> Self {
        Self {
            maximum_prompt_bytes,
            context_tokens: 32_768,
            failure: None,
            preflights: AtomicUsize::new(0),
        }
    }
}
impl ModelRuntime for WriterProjectionRuntime {
    fn generate(&self, _: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        panic!("planning must never generate");
    }
    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        panic!("planning must not start a runtime");
    }
    fn runtime_id(&self) -> &str {
        "controlled-writer-admission"
    }
    fn model_id(&self) -> &str {
        "controlled-writer-admission"
    }
    fn context_tokens(&self, _: PipelineStage) -> u32 {
        self.context_tokens
    }
    fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        self.preflights.fetch_add(1, Ordering::SeqCst);
        let code = self.failure.or_else(|| {
            (request.user_prompt.len() + request.system_prompt.len() > self.maximum_prompt_bytes)
                .then_some("MODEL_CONTEXT_EXCEEDED")
        });
        if let Some(code) = code {
            Err(ModelRuntimeFailure {
                code: code.into(),
                message: "controlled admission boundary".into(),
                recoverable: false,
                request_attempts: vec![],
            })
        } else {
            Ok(())
        }
    }
}

#[test]
fn whole_writer_all_fit_plan_uses_actual_payload_and_budget_owner() {
    let normalized = public_document(&[(1..=480)
        .map(|n| format!("The operator records inspection number {n}. "))
        .collect()]);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let runtime = WriterProjectionRuntime::new(usize::MAX);
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    assert!(!plan.requires_consolidation);
    assert_eq!(plan.drafts.len(), 1);
    assert_eq!(
        plan.drafts[0].primary,
        (0..input.units.len()).collect::<Vec<_>>()
    );
    assert!(plan.drafts[0].request.user_prompt.len() > 16_000);
    assert!(runtime.preflights.load(Ordering::SeqCst) > 1);
    let request = &plan.drafts[0].request;
    assert!(budget::maximum_characters(request).unwrap() <= MAX_UNIT_CHARACTERS);
    assert!(request.max_output_tokens >= OUTPUT_TOKENS);
}

#[test]
fn whole_writer_section_plan_assigns_all_primary_spans_and_keeps_dependencies() {
    let texts = (1..=12)
        .map(|n| {
            format!(
                "Section {n} Duties\n{} {}",
                format!("The operator records inspection number {n}. ").repeat(22),
                if n == 1 {
                    "Permission follows Section 12."
                } else {
                    "Exterior work is excluded."
                }
            )
        })
        .collect::<Vec<_>>();
    let normalized = public_document(&texts);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let runtime = WriterProjectionRuntime::new(7_500);
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    assert!(plan.requires_consolidation);
    assert!(plan.drafts.len() > 1);
    assert_eq!(
        plan.drafts
            .iter()
            .flat_map(|d| d.primary.iter().copied())
            .collect::<Vec<_>>(),
        (0..input.units.len()).collect::<Vec<_>>()
    );
    for draft in &plan.drafts {
        runtime.preflight_request(&draft.request).unwrap();
        let payload: Value = serde_json::from_str(&draft.request.user_prompt).unwrap();
        assert_eq!(
            draft.catalog.candidates.len(),
            payload["sources"].as_array().unwrap().len()
        );
        let units = payload["units"].as_array().unwrap();
        for &primary in &draft.primary {
            assert!(units
                .iter()
                .any(|u| u["index"] == primary && u["primary"] == true));
            for &governing in &input.units[primary].context {
                assert!(units.iter().any(|u| u["index"] == governing));
            }
        }
        if draft.primary.contains(&0) {
            assert!(payload["document"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row[5].as_str().unwrap().contains("inspection number 12")));
        }
    }
}

#[test]
fn whole_writer_planner_does_not_split_indivisible_units_or_swallow_infrastructure_errors() {
    let normalized = public_document(&["The operator must inspect all equipment. ".repeat(400)]);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let runtime = WriterProjectionRuntime::new(3_000);
    assert_eq!(
        input
            .plan(&runtime, 7, &UNCONTROLLED_EXECUTION)
            .unwrap_err()
            .code,
        "WHOLE_DOCUMENT_UNIT_TOO_LARGE"
    );
    let mut runtime = WriterProjectionRuntime::new(usize::MAX);
    runtime.failure = Some("MODEL_RUNTIME_UNAVAILABLE");
    assert_eq!(
        input
            .plan(&runtime, 7, &UNCONTROLLED_EXECUTION)
            .unwrap_err()
            .code,
        "MODEL_RUNTIME_UNAVAILABLE"
    );
}

#[test]
fn whole_writer_materialization_must_bind_the_writer_revision() {
    let normalized = public_document(&["The operator must inspect the equipment.".into()]);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let text = "The operator must inspect the equipment.";
    let response = json!({"units":[{"text":text,"source_ids":["s1"]}]}).to_string();
    let (claims, _) = parse_response_for_version(
        SummaryProfile::General,
        WHOLE_DOCUMENT_VERSION,
        &response,
        &normalized.document_id,
        &input.catalog,
        MAX_VERIFICATION_BATCHES,
        MAX_UNIT_CHARACTERS,
    )
    .unwrap();
    let expected = materialize_cited_claims(
        &normalized.document_id,
        WHOLE_DOCUMENT_VERSION,
        vec![ValidatedClaim {
            text: text.into(),
            evidence_ids: vec![input.catalog.candidates[0].evidence.evidence_id.clone()],
        }],
    )
    .unwrap();
    assert_eq!(
        claims, expected,
        "whole-document claim identities must bind the new writer policy"
    );
}

#[test]
fn standalone_quote_offset_must_not_bind_an_earlier_uncitable_copy() {
    let quote = "The operator must inspect the equipment.";
    let omitted = format!("The supplier records {}{quote}", "archived ".repeat(100));
    let text = format!("{omitted} {quote}");
    let owned = quote_segments::segment_spans(&text);
    assert_eq!(owned.omitted_source_units, 1);
    let &(start, end) = owned
        .spans
        .iter()
        .find(|&&(start, end)| &text[start..end] == quote)
        .unwrap();
    assert_eq!(&text[start..end], quote);
    assert_eq!(
        start,
        omitted.len() + 1,
        "citation offset must come from the quote owner, not an earlier identical string"
    );
}

#[test]
fn whole_writer_parser_keeps_claim_count_boundaries_and_legacy_limit() {
    let normalized = public_document(
        &(1..=65)
            .map(|n| format!("Operator {n} must inspect the equipment."))
            .collect::<Vec<_>>(),
    );
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    assert_eq!(input.catalog.candidates.len(), 65);
    for count in [0, 1, 8, 9, 63, 64, 65] {
        let response=json!({"units":(1..=count).map(|n|json!({"text":format!("Operator {n} must inspect the equipment."),"source_ids":[format!("s{n}")]})).collect::<Vec<_>>()}).to_string();
        let parsed = parse_response_for_version(
            SummaryProfile::General,
            WHOLE_DOCUMENT_VERSION,
            &response,
            &normalized.document_id,
            &input.catalog,
            MAX_VERIFICATION_BATCHES,
            MAX_UNIT_CHARACTERS,
        );
        if count == 0 || count > MAX_VERIFICATION_BATCHES {
            assert!(parsed.is_err(), "count={count}");
        } else {
            assert_eq!(parsed.unwrap().0.len(), count);
        }
        if count == 9 {
            assert!(parse_response(
                SummaryProfile::General,
                &response,
                &normalized.document_id,
                &input.catalog
            )
            .is_err());
        }
    }
}

#[test]
fn whole_writer_parser_rejects_mixed_unknown_duplicate_and_excess_source_ids() {
    let normalized = public_document(
        &(1..=9)
            .map(|n| format!("Operator {n} must inspect the equipment."))
            .collect::<Vec<_>>(),
    );
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let parse = |units: Vec<Value>| {
        parse_response_for_version(
            SummaryProfile::General,
            WHOLE_DOCUMENT_VERSION,
            &json!({"units":units}).to_string(),
            &normalized.document_id,
            &input.catalog,
            MAX_VERIFICATION_BATCHES,
            MAX_UNIT_CHARACTERS,
        )
    };
    let text = "Operators must inspect the equipment.";
    let unit = |ids: Vec<String>| json!({"text":text,"source_ids":ids});
    assert!(parse(vec![unit((1..=8).map(|n| format!("s{n}")).collect())]).is_ok());
    for ids in [
        vec![],
        vec!["s1".into(), "s1".into()],
        vec!["s9999".into()],
        (1..=9).map(|n| format!("s{n}")).collect(),
    ] {
        assert!(parse(vec![
            json!({"text":"Operator 1 must inspect the equipment.","source_ids":["s1"]}),
            unit(ids)
        ])
        .is_err());
    }
}

#[test]
fn whole_writer_parser_uses_admitted_unicode_scalar_bound_without_clipping() {
    let normalized = public_document(&["The operator must inspect the equipment.".into()]);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    for maximum in [1, 146, MAX_UNIT_CHARACTERS] {
        for count in [maximum, maximum + 1] {
            let text = if count == 1 {
                ".".into()
            } else {
                format!("{}.", "é".repeat(count - 1))
            };
            let wire = json!({"units":[{"text":text,"source_ids":["s1"]}]}).to_string();
            let parsed = parse_response_for_version(
                SummaryProfile::General,
                WHOLE_DOCUMENT_VERSION,
                &wire,
                &normalized.document_id,
                &input.catalog,
                MAX_VERIFICATION_BATCHES,
                maximum,
            );
            // A one-character ceiling cannot carry substantive prose. It is
            // not permission to clip a claim to punctuation.
            if count > maximum || count == 1 {
                assert!(parsed.is_err());
            } else {
                assert_eq!(parsed.unwrap().0[0].text, text);
            }
        }
    }
}

#[test]
#[ignore = "offline capacity only; requires frozen source inputs and exact native-count parity inventory"]
fn whole_writer_offline_native_capacity_measurement() {
    use crate::pipeline::contracts::{ParsedDocument, ParsedPage, SourceType};
    use crate::pipeline::llama_cpp::framing_tests::OfflineNativeCountRuntime;
    let inventory =
        std::path::PathBuf::from(std::env::var_os("DOC_SUM_OFFLINE_COUNT_INVENTORY").unwrap());
    let inventory_hash = std::env::var("DOC_SUM_OFFLINE_COUNT_INVENTORY_SHA256").unwrap();
    // Construction proves every recorded count before any capacity observation.
    let runtime = OfflineNativeCountRuntime::from_frozen_inventory(&inventory, &inventory_hash);
    let packet: Value = serde_json::from_slice(
        &std::fs::read(std::env::var_os("DOC_SUM_DEPENDENCIES_INPUT").unwrap()).unwrap(),
    )
    .unwrap();
    let mut inputs = Vec::new();
    for spec in packet["inputs"].as_array().unwrap() {
        let bytes = std::fs::read(spec["path"].as_str().unwrap()).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            spec["sha256"].as_str().unwrap()
        );
        let parsed = ParsedDocument {
            document_id: spec["alias"].as_str().unwrap().into(),
            parser_id: "frozen-preproduction-text".into(),
            parser_version: "1.0.0".into(),
            source_type: SourceType::NativeText,
            pages: vec![ParsedPage {
                page_number: 1,
                text: String::from_utf8(bytes).unwrap(),
                warnings: vec![],
                requires_visual_processing: false,
            }],
            warnings: vec![],
        };
        let normalized = crate::pipeline::normalize::CanonicalNormalizer::new()
            .normalize(&parsed)
            .unwrap();
        inputs.push((
            spec["alias"].as_str().unwrap().to_owned(),
            normalized,
            false,
        ));
    }
    let public = (1..=40)
        .map(|n| {
            format!(
                "Section {n} Inspections\n{}",
                format!("The operator records inspection number {n}. ").repeat(100)
            )
        })
        .collect::<Vec<_>>();
    inputs.push((
        "public-over-context-control".into(),
        public_document(&public),
        true,
    ));
    let output =
        std::path::PathBuf::from(std::env::var_os("DOC_SUM_WRITER_CAPACITY_OUTPUT").unwrap());
    let diagnose_only =
        std::env::var("DOC_SUM_WRITER_RESERVATION_DIAGNOSTIC").as_deref() == Ok("1");
    let mut rows = Vec::new();
    for (alias, normalized, require_sections) in inputs {
        let chunked = whole_writer_chunks(&normalized);
        let input = match whole_document::SourceInput::new(&normalized, &chunked) {
            Ok(input) => input,
            Err(error) if diagnose_only => {
                rows.push(
                    json!({"alias":alias,"status":"source_blocked","failure":error,
                    "reservation_measurement":"unavailable; source gate retained"}),
                );
                continue;
            }
            Err(error) => panic!("source input failed: {error:?}"),
        };
        if diagnose_only {
            let units = (0..input.units.len())
                .map(|index| {
                    let request = input.diagnostic_unit_request(index).unwrap();
                    let mut row = budget::diagnose_reservation(&runtime, &request).unwrap();
                    row["input_tokens"] = json!(runtime.input_tokens(&request));
                    row["unit_id"] = json!(input.units[index].id);
                    row
                })
                .collect::<Vec<_>>();
            rows.push(
                json!({"alias":alias,"status":"read_only_reservation_diagnostics","units":units}),
            );
            continue;
        }
        let before = runtime.preflight_count();
        let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION);
        let row = match plan {
            Ok(plan) => {
                assert!(
                    !require_sections || plan.requires_consolidation,
                    "the public control must genuinely exceed whole-request capacity"
                );
                assert_eq!(
                    plan.drafts
                        .iter()
                        .flat_map(|d| d.primary.iter().copied())
                        .collect::<Vec<_>>(),
                    (0..input.units.len()).collect::<Vec<_>>()
                );
                let drafts=plan.drafts.iter().map(|draft| {
                    runtime.preflight_request(&draft.request).unwrap();
                    assert_eq!(budget::maximum_characters(&draft.request).unwrap(), MAX_UNIT_CHARACTERS,
                        "admitted writer sections must retain the full claim allowance");
                    let ModelOutputFormat::JsonSchema {name,schema}=&draft.request.output_format else {panic!("writer schema required");};
                    json!({"primary":draft.primary,"request":{"stage":draft.request.stage,"ordinal":draft.request.ordinal,"seed":draft.request.seed,"system_prompt":draft.request.system_prompt,"user_prompt":draft.request.user_prompt,"output_format":{"name":name,"schema":schema},"max_output_tokens":draft.request.max_output_tokens},
                        "input_tokens":runtime.input_tokens(&draft.request),"output_allowance_tokens":draft.request.max_output_tokens,"maximum_units":schema["properties"]["units"]["maxItems"],"citation_count":draft.catalog.candidates.len(),"schema_bytes":serde_json::to_vec(schema).unwrap().len(),"maximum_unit_characters":budget::maximum_characters(&draft.request).unwrap()})
                }).collect::<Vec<_>>();
                json!({"alias":alias,"status":"drafts_admitted","requires_consolidation":plan.requires_consolidation,"drafts":drafts})
            }
            Err(error) => json!({"alias":alias,"status":"STOP","failure":error}),
        };
        let stopped = row["status"] == "STOP";
        rows.push(json!({"result":row,"normalized":normalized,"source_units":input.units.iter().map(|u|json!({"id":u.id,"spans":u.spans,"context":u.context})).collect::<Vec<_>>(),
            "omitted_standalone_quote_segments":input.catalog.omitted_source_units,
            "anchors":input.anchors.iter().map(|a|json!({"source_id":a.source_id,"span":a.span,"unit":a.unit,"partial_excerpt":a.partial_excerpt})).collect::<Vec<_>>(),
            "governing_units_with_zero_anchors":(0..input.units.len()).filter(|i|!input.anchors.iter().any(|a|a.unit==*i)).count(),
            "contextual_anchor_coverage_gaps":normalized.pages.iter().flat_map(|p|&p.content).enumerate().map(|(i,b)|b.text.char_indices().filter(|(offset,c)|!c.is_whitespace() && !input.anchors.iter().any(|a|a.span.block==i && a.span.start<=*offset && *offset<a.span.end)).count()).sum::<usize>(),"preflights":runtime.preflight_count()-before}));
        if stopped {
            save_capacity_report(&output, &inventory_hash, &rows, "STOP");
            panic!("offline whole-writer capacity STOP; see private report; no generation");
        }
    }
    if diagnose_only {
        save_capacity_report(
            &output,
            &inventory_hash,
            &rows,
            "diagnostic_only_not_admission",
        );
        println!("RESERVATION_DIAGNOSTICS_SAVED; unchanged source gate; generation_calls=0 runtime_startups=0");
        return;
    }
    save_capacity_report(&output, &inventory_hash, &rows, "draft_planning_only");
    println!("OFFLINE_WRITER_CAPACITY_SAVED; draft admission only, consolidation/persistence/fidelity not proven; generation_calls=0 runtime_startups=0");
}

fn save_capacity_report(
    path: &std::path::Path,
    inventory_hash: &str,
    rows: &[Value],
    status: &str,
) {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let report = json!({"status":status,"count_inventory_sha256":inventory_hash,"inputs":rows,"evidence_kind":"offline capacity only, not native qualification","generation_calls":0,"runtime_startups":0,"benchmark_labels_read":false,"consolidation_proven":false});
    options
        .open(path)
        .unwrap()
        .write_all(&serde_json::to_vec_pretty(&report).unwrap())
        .unwrap();
}

#[test]
fn whole_writer_contextual_anchors_cover_previously_omitted_layouts() {
    let long = format!(
        "The operator {} remains responsible.",
        "explicitly ".repeat(100)
    );
    let layouts = [
        long.clone(),
        format!(
            "{} Unless {} applies.",
            "The operator keeps records. ".repeat(30),
            "the exception ".repeat(50)
        ),
        format!(
            "Coverage:\nEach event: {}\nAggregate: $900,000",
            "covered equipment ".repeat(80)
        ),
        format!(
            "{} A dangling exception unless",
            "Retain the complete sentence. ".repeat(30)
        ),
        format!(
            "The supplier {}Retain the records. Retain the records.",
            "archived ".repeat(100)
        ),
        format!("The first fact remains citable. {long} The final fact remains citable."),
    ];
    for (layout, text) in layouts.iter().enumerate() {
        let text = format!("A retained opening sentence. {text} A retained closing sentence.");
        let normalized = public_document(std::slice::from_ref(&text));
        let chunked = whole_writer_chunks(&normalized);
        let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
        for (byte, character) in text.char_indices().filter(|(_, c)| !c.is_whitespace()) {
            let count = input
                .anchors
                .iter()
                .filter(|a| a.span.block == 0 && a.span.start <= byte && byte < a.span.end)
                .count();
            assert_eq!(
                count, 1,
                "layout {layout}: uncovered or multiply covered scalar at byte {byte}: {character}"
            );
        }
    }
}

#[test]
fn contextual_anchor_metadata_retains_dependencies_and_rejects_wrong_owner() {
    let normalized = public_document(&[
        format!(
            "1. Permission\nThe operator may use equipment {}subject to Section 3.",
            "carefully ".repeat(100)
        ),
        "2. Storage\nThe supplier stores the equipment indoors.".into(),
        "3. Exceptions\nRoutine replacement and cosmetic work are excluded.".into(),
    ]);
    let chunked = whole_writer_chunks(&normalized);
    let mut input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let evidence = input
        .catalog
        .candidates
        .iter()
        .map(|c| c.evidence.clone())
        .collect::<Vec<_>>();
    let contexts = input.evidence_contexts(&evidence).unwrap();
    for (anchor, candidate) in input.anchors.iter().zip(&input.catalog.candidates) {
        let owned = &contexts[&candidate.evidence.evidence_id];
        assert_eq!(owned.partial_excerpt, anchor.partial_excerpt);
        if anchor.unit == 0 {
            assert!(owned.partial_excerpt);
            assert!(owned
                .full_clause
                .contains("Routine replacement and cosmetic work are excluded."));
            assert!(owned.full_clause.contains("subject to Section 3."));
        } else {
            assert!(!owned.partial_excerpt);
        }
    }
    let (_, mut draft) = public_draft("The operator may use equipment.");
    draft.document_id = normalized.document_id.clone();
    draft.synthesis_version = WHOLE_DOCUMENT_VERSION.into();
    draft.synthesis_evidence = evidence.clone();
    let partial = partial_excerpt_evidence_ids(&draft, &chunked, &normalized).unwrap();
    for (anchor, candidate) in input.anchors.iter().zip(&input.catalog.candidates) {
        assert_eq!(
            partial.contains(&candidate.evidence.evidence_id),
            anchor.partial_excerpt
        );
    }
    let unit = input.anchors[0].unit;
    input.anchors[0].unit = 1;
    assert!(input.evidence_contexts(&evidence).is_err());
    input.anchors[0].unit = unit;
    input.units[unit].context.clear();
    assert!(input.evidence_contexts(&evidence).is_err());
}

#[test]
fn contextual_anchor_context_and_partial_metadata_survive_actual_database_reopen() {
    use crate::pipeline::{db, ingest::prepare_pdf_ingestion};
    let normalized = public_document(&[
        format!(
            "1. Permission\nThe operator may use equipment {}subject to Section 2.",
            "carefully ".repeat(100)
        ),
        "2. Exceptions\nRoutine replacement and cosmetic work are excluded.".into(),
    ]);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let runtime = WriterProjectionRuntime::new(usize::MAX);
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    assert_eq!(plan.drafts.len(), 1);
    let mut draft =
        partial_progress_envelope(input.new_progress(&plan, &runtime, 7).unwrap(), &chunked);
    let progress = draft.whole_document_progress.as_mut().unwrap();
    input.dispatch_section(progress, 0, &runtime, 7).unwrap();
    input.record_section(progress, 0, &ModelResponse {
        text: json!({"units":[{"text":"The operator may use equipment subject to the exclusions.","source_ids":[input.catalog.candidates[0].request_id]}]}).to_string(),
        runtime_id: runtime.runtime_id().into(), model_id: runtime.model_id().into(), request_attempts: vec![],
    }).unwrap();
    input.finalize_progress(&mut draft).unwrap();
    let before = verification::summary_verification_prompt(
        SummaryProfile::General,
        &draft,
        &normalized,
        Some(&chunked),
    )
    .unwrap();
    let partial_before = partial_excerpt_evidence_ids(&draft, &chunked, &normalized).unwrap();
    assert!(!partial_before.is_empty());
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source-bindings.sqlite");
    let mut conn = db::init_db(&path).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
    let (mut document, mut run) = prepare_pdf_ingestion(fixture.to_str().unwrap(), None).unwrap();
    document.document_id = normalized.document_id.clone();
    run.document_id = normalized.document_id.clone();
    let ingested = db::persist_ingestion(&mut conn, &document, &run).unwrap();
    // Fixture preconditions select storage boundaries only; this is not a
    // claim that parsing, model generation or complete-pipeline delivery ran.
    conn.execute(
        "UPDATE pipeline_runs SET state=?1 WHERE run_id=?2",
        rusqlite::params![
            serde_json::to_string(&crate::pipeline::contracts::PipelineState::Normalizing).unwrap(),
            run.run_id
        ],
    )
    .unwrap();
    let stored = db::complete_normalization(
        &mut conn,
        &run.run_id,
        ingested.state_version,
        &normalized,
        vec![],
    )
    .unwrap();
    conn.execute(
        "UPDATE pipeline_runs SET state=?1 WHERE run_id=?2",
        rusqlite::params![
            serde_json::to_string(&crate::pipeline::contracts::PipelineState::Chunking).unwrap(),
            run.run_id
        ],
    )
    .unwrap();
    let stored = db::complete_chunking(
        &mut conn,
        &run.run_id,
        stored.state_version,
        &chunked,
        vec![],
    )
    .unwrap();
    conn.execute(
        "UPDATE pipeline_runs SET state=?1 WHERE run_id=?2",
        rusqlite::params![
            serde_json::to_string(&crate::pipeline::contracts::PipelineState::Synthesizing)
                .unwrap(),
            run.run_id
        ],
    )
    .unwrap();
    db::complete_synthesis(&mut conn, &run.run_id, stored.state_version, &draft, vec![]).unwrap();
    drop(conn);
    let conn = db::init_db(&path).unwrap();
    let reloaded = db::get_synthesis_attempt(&conn, &run.run_id, 0)
        .unwrap()
        .unwrap();
    assert_eq!(
        reloaded,
        db::get_synthesized_document(&conn, &run.run_id)
            .unwrap()
            .unwrap()
    );
    let source = db::get_normalized_document(&conn, &run.run_id)
        .unwrap()
        .unwrap();
    let chunks = db::get_chunked_document(&conn, &run.run_id)
        .unwrap()
        .unwrap();
    let after = verification::summary_verification_prompt(
        SummaryProfile::General,
        &reloaded,
        &source,
        Some(&chunks),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
    assert_eq!(
        partial_before,
        partial_excerpt_evidence_ids(&reloaded, &chunks, &source).unwrap()
    );
    let mut tampered = reloaded;
    tampered.synthesis_evidence[0].exact_quote.push('x');
    assert!(partial_excerpt_evidence_ids(&tampered, &chunks, &source).is_err());
    assert!(verification::summary_verification_prompt(
        SummaryProfile::General,
        &tampered,
        &source,
        Some(&chunks)
    )
    .is_err());
}

// Authored public size controls, not model outputs or fidelity labels.
fn inspection_control_responses(
    input: &whole_document::SourceInput<'_>,
    plan: &whole_document::DraftPlan,
) -> Vec<String> {
    plan.drafts.iter().map(|draft| {
        let units=draft.primary.iter().map(|&unit| {
            let (anchor,candidate)=input.anchors.iter().zip(&input.catalog.candidates).find(|(a,_)|a.unit==unit).unwrap();
            assert!(draft.catalog.candidates.iter().any(|c|c.request_id==candidate.request_id));
            json!({"text":format!("Operator records inspection {}.",anchor.unit+1),"source_ids":[candidate.request_id]})
        }).collect::<Vec<_>>();
        json!({"units":units}).to_string()
    }).collect()
}

#[test]
fn whole_writer_assembly_keeps_exact_lineage_and_rejects_invalid_sections() {
    let texts = (1..=4)
        .map(|n| {
            format!(
                "Section {n} Inspections\n{}{}",
                format!("The operator records inspection number {n}. ").repeat(50),
                if n == 1 {
                    "Inspections follow Section 4."
                } else {
                    ""
                }
            )
        })
        .collect::<Vec<_>>();
    let normalized = public_document(&texts);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let mut plan = input
        .plan(
            &WriterProjectionRuntime::new(7_500),
            7,
            &UNCONTROLLED_EXECUTION,
        )
        .unwrap();
    assert!(plan.requires_consolidation);
    let mut responses = inspection_control_responses(&input, &plan);
    // The last unit is also governing context in the first section. An exact
    // duplicate keeps both origins while producing one canonical candidate.
    let source = input
        .anchors
        .iter()
        .zip(&input.catalog.candidates)
        .find(|(a, _)| a.unit == 3)
        .unwrap()
        .1;
    let mut first: Value = serde_json::from_str(&responses[0]).unwrap();
    first["units"]
        .as_array_mut()
        .unwrap()
        .push(json!({"text":"Operator records inspection 4.","source_ids":[source.request_id]}));
    responses[0] = first.to_string();
    let assembly = input.assemble(&plan, &responses, 7).unwrap();
    assert_eq!(assembly.claims.len(), 4);
    assert_eq!(assembly.lineage.len(), 5);
    assert_eq!(
        assembly
            .claims
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>(),
        (1..=4)
            .map(|n| format!("Operator records inspection {n}."))
            .collect::<Vec<_>>()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    assert_eq!(assembly.lineage[1].2, 3);
    assert_eq!(assembly.lineage.last().unwrap().2, 3);
    assert!(input
        .assemble(&plan, &responses[..responses.len() - 1], 7)
        .is_err());
    let mut invalid = responses.clone();
    invalid[0] = "{\"units\":[]}".into();
    assert!(input.assemble(&plan, &invalid, 7).is_err());
    let saved = plan.drafts[0].request.user_prompt.clone();
    plan.drafts[0].request.user_prompt.push(' ');
    assert!(input.assemble(&plan, &responses, 7).is_err());
    plan.drafts[0].request.user_prompt = saved;
    let saved = plan.drafts[0].request.seed;
    plan.drafts[0].request.seed += 1;
    assert!(input.assemble(&plan, &responses, 7).is_err());
    plan.drafts[0].request.seed = saved;
    let saved = plan.drafts[0].primary.clone();
    plan.drafts[0].primary.clear();
    assert!(input.assemble(&plan, &responses, 7).is_err());
    plan.drafts[0].primary = saved;
    let mut variants = responses.clone();
    let mut first: Value = serde_json::from_str(&variants[0]).unwrap();
    let units = first["units"].as_array_mut().unwrap();
    units.push(json!({"text":"Operator logs inspection 4.","source_ids":[source.request_id]}));
    variants[0] = first.to_string();
    assert_eq!(
        input.assemble(&plan, &variants, 7).unwrap().claims.len(),
        5,
        "wording variants must not be semantically deduplicated"
    );
}

#[test]
fn whole_writer_assembly_uses_the_original_claim_boundaries() {
    let normalized =
        public_document(&["The operator records inspection numbers 1 through 65.".into()]);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let plan = input
        .plan(
            &WriterProjectionRuntime::new(usize::MAX),
            7,
            &UNCONTROLLED_EXECUTION,
        )
        .unwrap();
    for count in [0, 1, 8, 9, 63, 64, 65] {
        let units=(1..=count).map(|n|json!({"text":format!("Operator records inspection {n}."),"source_ids":[input.catalog.candidates[0].request_id]})).collect::<Vec<_>>();
        let response = json!({"units":units}).to_string();
        let result = input.assemble(&plan, &[response], 7);
        assert_eq!(
            result.is_ok(),
            (1..=64).contains(&count),
            "assembly count {count}"
        );
        if let Ok(assembly) = result {
            assert_eq!(assembly.claims.len(), count);
            assert_eq!(assembly.lineage.len(), count);
        }
    }
}

#[test]
#[ignore = "zero-generation replay of frozen authored public section responses"]
fn whole_writer_assembly_replays_the_frozen_public_candidate_packet() {
    let path =
        std::path::PathBuf::from(std::env::var_os("DOC_SUM_ASSEMBLY_PUBLIC_FREEZE").unwrap());
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        std::env::var("DOC_SUM_ASSEMBLY_PUBLIC_FREEZE_SHA256").unwrap()
    );
    let packet: Value = serde_json::from_slice(&bytes).unwrap();
    let normalized: NormalizedDocument =
        serde_json::from_value(packet["normalized"].clone()).unwrap();
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let mut drafts = Vec::new();
    let mut responses = Vec::new();
    for row in packet["drafts"].as_array().unwrap() {
        let request = writer_request_from_value(&row["request"]);
        let body: Value = serde_json::from_str(&request.user_prompt).unwrap();
        let ids = body["sources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r[0].as_str().unwrap())
            .collect::<HashSet<_>>();
        let catalog = SourceCatalog {
            candidates: input
                .catalog
                .candidates
                .iter()
                .filter(|c| ids.contains(c.request_id.as_str()))
                .cloned()
                .collect(),
            omitted_source_units: input.catalog.omitted_source_units,
        };
        drafts.push(whole_document::PlannedDraft {
            request,
            primary: serde_json::from_value(row["primary"].clone()).unwrap(),
            catalog,
        });
        let response = row["response"].as_str().unwrap().to_owned();
        assert_eq!(
            format!("{:x}", Sha256::digest(response.as_bytes())),
            row["response_sha256"].as_str().unwrap()
        );
        responses.push(response);
    }
    let plan = whole_document::DraftPlan {
        drafts,
        requires_consolidation: true,
    };
    // Keep the original artifact as evidence of the rejected 39/282-character
    // request shape. Reuse its exact authored text/IDs under a newly admitted
    // plan; this is request repair, never new generation or rewritten claims.
    assert_eq!(
        input.assemble(&plan, &responses, 7).unwrap_err().code,
        "WHOLE_DOCUMENT_SOURCE_INVALID"
    );
    let authored = responses
        .iter()
        .flat_map(|response| {
            serde_json::from_str::<Value>(response).unwrap()["units"]
                .as_array()
                .unwrap()
                .clone()
        })
        .collect::<Vec<_>>();
    assert_eq!(authored.len(), 40);
    let runtime =
        crate::pipeline::llama_cpp::framing_tests::OfflineNativeCountRuntime::from_frozen_inventory(
            &std::path::PathBuf::from(std::env::var_os("DOC_SUM_OFFLINE_COUNT_INVENTORY").unwrap()),
            &std::env::var("DOC_SUM_OFFLINE_COUNT_INVENTORY_SHA256").unwrap(),
        );
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    let mut progress = input.new_progress(&plan, &runtime, 7).unwrap();
    let mut section = 0;
    while section < progress.sections.len() {
        let request = input
            .dispatch_section(&mut progress, section, &runtime, 7)
            .unwrap();
        let primary = progress.sections[section].primary.clone();
        assert_eq!(
            budget::maximum_characters(&request).unwrap(),
            MAX_UNIT_CHARACTERS
        );
        let units = authored
            .iter()
            .filter(|unit| {
                assert_eq!(
                    unit["source_ids"].as_array().unwrap().len(),
                    1,
                    "the pinned authored size fixture uses one source per claim"
                );
                let source = unit["source_ids"][0].as_str().unwrap();
                let (anchor, _) = input
                    .anchors
                    .iter()
                    .zip(&input.catalog.candidates)
                    .find(|(_, candidate)| candidate.request_id == source)
                    .unwrap();
                primary.contains(&anchor.unit)
            })
            .cloned()
            .collect::<Vec<_>>();
        assert!(!units.is_empty());
        let ModelOutputFormat::JsonSchema { schema, .. } = &request.output_format else {
            panic!("schema")
        };
        let limit = schema["properties"]["units"]["maxItems"].as_u64().unwrap() as usize;
        // Authored size-control simulation only, never a new model result:
        // a full response drives the real split owner. No parent prefix is
        // delivered; all original authored claims must survive in leaf output.
        let text = json!({"units":units.into_iter().take(limit).collect::<Vec<_>>()}).to_string();
        input
            .record_section(
                &mut progress,
                section,
                &ModelResponse {
                    text,
                    runtime_id: runtime.runtime_id().into(),
                    model_id: runtime.model_id().into(),
                    request_attempts: vec![],
                },
            )
            .unwrap();
        if matches!(
            progress.sections[section].state,
            crate::pipeline::contracts::WholeDocumentSectionState::Saturated { .. }
        ) {
            input
                .split_saturated(&mut progress, section, &runtime, 7)
                .unwrap();
        }
        section += 1;
    }
    let plan = input.restore_progress(&progress).unwrap();
    let assembly = input.finish_progress(&mut progress).unwrap();
    for (claim, original) in assembly.claims.iter().zip(&authored) {
        assert_eq!(claim.text, original["text"].as_str().unwrap());
        let expected = original["source_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| {
                input
                    .catalog
                    .candidates
                    .iter()
                    .find(|candidate| candidate.request_id == id.as_str().unwrap())
                    .unwrap()
                    .evidence
                    .evidence_id
                    .clone()
            })
            .collect::<Vec<_>>();
        assert_eq!(claim.evidence_ids, expected);
    }
    assert_eq!(assembly.claims.len(), 40);
    assert_eq!(assembly.lineage.len(), 40);
    for ((section, candidate, target), claim) in assembly.lineage.iter().zip(&assembly.claims) {
        let crate::pipeline::contracts::WholeDocumentSectionState::Completed { response } =
            &progress.sections[*section].state
        else {
            panic!("lineage cannot borrow discarded saturation output")
        };
        let raw: Value = serde_json::from_str(&response.text).unwrap();
        assert_eq!(
            claim.text,
            raw["units"][*candidate]["text"].as_str().unwrap()
        );
        assert_eq!(claim, &assembly.claims[*target]);
    }
    assert_eq!(
        input.evidence_contexts(&assembly.evidence).unwrap().len(),
        assembly.evidence.len()
    );
    use std::io::Write;
    let output =
        std::path::PathBuf::from(std::env::var_os("DOC_SUM_ASSEMBLY_REPLAY_OUTPUT").unwrap());
    let result = json!({"kind":"frozen authored public packet replay with authored saturation controls, not model output or fidelity qualification","public_freeze_sha256":format!("{:x}",Sha256::digest(&bytes)),"assembly":assembly,"draft_count":plan.drafts.len(),"retained_section_attempts":progress.sections.len(),"original_request_shape_rejected":true,"exact_authored_text_and_citations_preserved":true,"live_model_calls":0,"runtime_startups":0});
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(output)
        .unwrap()
        .write_all(&serde_json::to_vec_pretty(&result).unwrap())
        .unwrap();
    println!("ASSEMBLY_FROZEN_PUBLIC_REPLAY drafts={} candidates={} lineage={} live_model_calls=0 runtime_startups=0",plan.drafts.len(),assembly.claims.len(),assembly.lineage.len());
}

#[test]
#[ignore = "offline regression of frozen rejected consolidation request"]
fn whole_writer_original_consolidation_request_still_exceeds_native_context() {
    use crate::pipeline::llama_cpp::framing_tests::OfflineNativeCountRuntime;
    let inventory =
        std::path::PathBuf::from(std::env::var_os("DOC_SUM_OFFLINE_COUNT_INVENTORY").unwrap());
    let runtime = OfflineNativeCountRuntime::from_frozen_inventory(
        &inventory,
        &std::env::var("DOC_SUM_OFFLINE_COUNT_INVENTORY_SHA256").unwrap(),
    );
    let bytes =
        std::fs::read(std::env::var_os("DOC_SUM_FROZEN_CONSOLIDATION_MEASUREMENT").unwrap())
            .unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        std::env::var("DOC_SUM_FROZEN_CONSOLIDATION_MEASUREMENT_SHA256").unwrap()
    );
    let report: Value = serde_json::from_slice(&bytes).unwrap();
    let request = writer_request_from_value(&report["request"]);
    assert!(budget::admit(&runtime, &request, true).unwrap().is_none());
    assert_eq!(runtime.input_tokens(&request), 48_634);
    assert_eq!(runtime.context_tokens(PipelineStage::Synthesize), 32_768);
    println!("FROZEN_CONSOLIDATION_REJECTED input_tokens=48634 context_tokens=32768 live_model_calls=0 runtime_startups=0");
}

fn writer_request_from_value(value: &Value) -> ModelRequest {
    assert_eq!(value["stage"], json!(PipelineStage::Synthesize));
    ModelRequest {
        stage: PipelineStage::Synthesize,
        ordinal: u32::try_from(value["ordinal"].as_u64().unwrap()).unwrap(),
        seed: value["seed"].as_u64().unwrap(),
        system_prompt: value["system_prompt"].as_str().unwrap().into(),
        user_prompt: value["user_prompt"].as_str().unwrap().into(),
        max_output_tokens: u32::try_from(value["max_output_tokens"].as_u64().unwrap()).unwrap(),
        output_format: ModelOutputFormat::JsonSchema {
            name: value["output_format"]["name"].as_str().unwrap().into(),
            schema: value["output_format"]["schema"].clone(),
        },
    }
}

#[test]
fn whole_writer_progress_seals_requests_before_dispatch_and_rejects_stale_state() {
    use crate::pipeline::contracts::WholeDocumentSectionState;
    let normalized = public_document(&["The operator records inspection number 1.".into()]);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let runtime = WriterProjectionRuntime::new(usize::MAX);
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    let responses = inspection_control_responses(&input, &plan);
    let mut progress = input.new_progress(&plan, &runtime, 7).unwrap();
    assert!(matches!(
        progress.sections[0].state,
        WholeDocumentSectionState::Pending
    ));
    assert!(input.finish_progress(&mut progress).is_err());
    let request = input
        .dispatch_section(&mut progress, 0, &runtime, 7)
        .unwrap();
    assert_eq!(request, plan.drafts[0].request);
    assert!(matches!(
        progress.sections[0].state,
        WholeDocumentSectionState::Dispatched
    ));
    assert!(
        input
            .dispatch_section(&mut progress, 0, &runtime, 7)
            .is_err(),
        "a dispatched section must never be sent again"
    );
    let reloaded: crate::pipeline::contracts::WholeDocumentProgress =
        serde_json::from_slice(&serde_json::to_vec(&progress).unwrap()).unwrap();
    assert_eq!(progress, reloaded);
    let mut altered = reloaded.clone();
    altered.source_sha256.push('x');
    assert!(input.restore_progress(&altered).is_err());
    let mut altered = reloaded.clone();
    altered.sections[0].request.user_prompt.push(' ');
    assert!(input.restore_progress(&altered).is_err());
    let mut altered = reloaded.clone();
    altered.sections[0].request.user_prompt.push(' ');
    altered.sections[0].request_sha256 = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&altered.sections[0].request).unwrap())
    );
    assert!(input.restore_progress(&altered).is_err());
    let mut altered = reloaded.clone();
    altered.context_tokens = 0;
    assert!(input.restore_progress(&altered).is_err());
    let mut altered = reloaded.clone();
    altered.runtime_id.clear();
    assert!(input.restore_progress(&altered).is_err());
    let mut pending = input.new_progress(&plan, &runtime, 7).unwrap();
    assert!(input
        .dispatch_section(&mut pending, 0, &OfflineRuntime::default(), 7)
        .is_err());
    assert!(input
        .dispatch_section(&mut pending, 0, &runtime, 8)
        .is_err());
    let response = ModelResponse {
        text: responses[0].clone(),
        runtime_id: runtime.runtime_id().into(),
        model_id: runtime.model_id().into(),
        request_attempts: vec![],
    };
    input.record_section(&mut progress, 0, &response).unwrap();
    assert!(matches!(
        progress.sections[0].state,
        WholeDocumentSectionState::Completed { .. }
    ));
    let assembly = input.finish_progress(&mut progress).unwrap();
    assert_eq!(assembly.claims.len(), 1);
    assert_eq!(progress.assembly_lineage.as_ref(), Some(&assembly.lineage));
    assert!(input
        .dispatch_section(&mut progress, 0, &runtime, 7)
        .is_err());
    let mut failed = input.new_progress(&plan, &runtime, 7).unwrap();
    input.dispatch_section(&mut failed, 0, &runtime, 7).unwrap();
    input
        .fail_section(
            &mut failed,
            0,
            stage_failure(
                PipelineStage::Synthesize,
                "PUBLIC_FAILURE",
                "controlled failure",
                false,
            ),
        )
        .unwrap();
    assert!(input.dispatch_section(&mut failed, 0, &runtime, 7).is_err());
    assert!(input.finish_progress(&mut failed).is_err());
}

fn partial_progress_envelope(
    progress: crate::pipeline::contracts::WholeDocumentProgress,
    chunked: &ChunkedDocument,
) -> SynthesizedDocument {
    let (_, mut draft) = public_draft("The operator records inspection 1.");
    draft.document_id = progress.document_id.clone();
    draft.synthesis_version = WHOLE_DOCUMENT_VERSION.into();
    draft.runtime_id = progress.runtime_id.clone();
    draft.model_id = progress.model_id.clone();
    draft.summary_text.clear();
    draft.summary_claims.clear();
    draft.synthesis_evidence.clear();
    draft.source_chunk_ids = chunked.chunks.iter().map(|c| c.chunk_id.clone()).collect();
    draft.whole_document_progress = Some(progress);
    draft
}

#[test]
fn whole_writer_saved_result_requires_complete_exact_section_receipts() {
    let normalized = public_document(&["The operator records inspection number 1.".into()]);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let runtime = WriterProjectionRuntime::new(usize::MAX);
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    let responses = inspection_control_responses(&input, &plan);
    let mut draft =
        partial_progress_envelope(input.new_progress(&plan, &runtime, 7).unwrap(), &chunked);
    let progress = draft.whole_document_progress.as_mut().unwrap();
    input.dispatch_section(progress, 0, &runtime, 7).unwrap();
    input
        .record_section(
            progress,
            0,
            &ModelResponse {
                text: responses[0].clone(),
                runtime_id: runtime.runtime_id().into(),
                model_id: runtime.model_id().into(),
                request_attempts: vec![],
            },
        )
        .unwrap();
    let assembly = input.finish_progress(progress).unwrap();
    draft.summary_claims = assembly.claims;
    draft.synthesis_evidence = assembly.evidence;
    draft.summary_text =
        render_cited_summary_with_evidence(&draft.summary_claims, &draft.synthesis_evidence)
            .unwrap();
    input.validate_saved_synthesis(&draft).unwrap();
    let mut missing = draft.clone();
    missing.whole_document_progress = None;
    assert!(
        input.validate_saved_synthesis(&missing).is_err(),
        "saved whole-document results must require their sealed section receipts"
    );
    use crate::pipeline::contracts::WholeDocumentSectionState;
    for change in 0..10 {
        let mut altered = draft.clone();
        let progress = altered.whole_document_progress.as_mut().unwrap();
        match change {
            0 => progress.assembly_lineage = None,
            1 => progress.sections[0].state = WholeDocumentSectionState::Pending,
            2 => {
                let WholeDocumentSectionState::Completed { response } =
                    &mut progress.sections[0].state
                else {
                    panic!("completed control")
                };
                response.response_sha256.push('x');
            }
            3 => {
                let WholeDocumentSectionState::Completed { response } =
                    &mut progress.sections[0].state
                else {
                    panic!("completed control")
                };
                let before = response.text.clone();
                let mut wire: Value = serde_json::from_str(&response.text).unwrap();
                wire["units"][0]["text"] = json!("Operator records inspection 2.");
                response.text = wire.to_string();
                assert_ne!(response.text, before);
                response.response_sha256 =
                    format!("{:x}", Sha256::digest(response.text.as_bytes()));
            }
            4 => progress.assembly_lineage.as_mut().unwrap()[0].2 += 1,
            5 => altered.runtime_id.push('x'),
            6 => progress.source_sha256.push('x'),
            7 => {
                progress.sections[0].request.user_prompt.push(' ');
                progress.sections[0].request_sha256 =
                    progress.sections[0].request.integrity_sha256().unwrap();
            }
            8 => progress.sections[0].primary.push(0),
            _ => progress.sections.clear(),
        }
        assert!(
            input.validate_saved_synthesis(&altered).is_err(),
            "altered receipt {change} was accepted"
        );
    }
    let mut pending =
        partial_progress_envelope(input.new_progress(&plan, &runtime, 7).unwrap(), &chunked);
    let before = pending.clone();
    assert!(input.finalize_progress(&mut pending).is_err());
    assert_eq!(
        pending, before,
        "failed assembly must not partially mutate the envelope"
    );
    let reloaded = serde_json::from_slice(&serde_json::to_vec(&draft).unwrap()).unwrap();
    input.validate_saved_synthesis(&reloaded).unwrap();
}

#[test]
fn whole_writer_database_cannot_complete_a_partial_section_record() {
    use crate::pipeline::{db, ingest::prepare_pdf_ingestion};
    let normalized = public_document(&["The operator records inspection number 1.".into()]);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let runtime = WriterProjectionRuntime::new(usize::MAX);
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    let partial =
        partial_progress_envelope(input.new_progress(&plan, &runtime, 7).unwrap(), &chunked);
    let temp = tempfile::tempdir().unwrap();
    let mut conn = db::init_db(temp.path().join("partial.sqlite")).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
    let (mut document, mut run) = prepare_pdf_ingestion(fixture.to_str().unwrap(), None).unwrap();
    document.document_id = normalized.document_id.clone();
    run.document_id = normalized.document_id.clone();
    let ingested = db::persist_ingestion(&mut conn, &document, &run).unwrap();
    conn.execute(
        "UPDATE pipeline_runs SET state=?1 WHERE run_id=?2",
        rusqlite::params![
            serde_json::to_string(&crate::pipeline::contracts::PipelineState::Synthesizing)
                .unwrap(),
            run.run_id
        ],
    )
    .unwrap();
    assert!(
        db::complete_synthesis(
            &mut conn,
            &run.run_id,
            ingested.state_version,
            &partial,
            vec![]
        )
        .is_err(),
        "incomplete section progress must not create a completed synthesis artifact"
    );
    assert!(db::get_synthesis_attempt(&conn, &run.run_id, 0)
        .unwrap()
        .is_none());
}

#[test]
fn whole_writer_progress_database_reopen_and_concurrent_dispatch_are_atomic() {
    use crate::pipeline::contracts::{PipelineState, WholeDocumentSectionState};
    use crate::pipeline::{db, ingest::prepare_pdf_ingestion};
    let normalized = public_document(&["The operator records inspection number 1.".into()]);
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let runtime = WriterProjectionRuntime::new(usize::MAX);
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    let responses = inspection_control_responses(&input, &plan);
    let pending =
        partial_progress_envelope(input.new_progress(&plan, &runtime, 7).unwrap(), &chunked);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("progress.sqlite");
    let mut conn = db::init_db(&path).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
    let (mut document, mut run) = prepare_pdf_ingestion(fixture.to_str().unwrap(), None).unwrap();
    document.document_id = normalized.document_id.clone();
    run.document_id = normalized.document_id.clone();
    let ingested = db::persist_ingestion(&mut conn, &document, &run).unwrap();
    conn.execute(
        "UPDATE pipeline_runs SET state=?1 WHERE run_id=?2",
        rusqlite::params![
            serde_json::to_string(&PipelineState::Synthesizing).unwrap(),
            run.run_id
        ],
    )
    .unwrap();
    db::save_whole_synthesis_progress(
        &mut conn,
        &run.run_id,
        ingested.state_version,
        None,
        &pending,
    )
    .unwrap();
    assert!(db::get_synthesized_document(&conn, &run.run_id)
        .unwrap()
        .is_none());
    assert!(db::get_synthesis_attempt(&conn, &run.run_id, 0)
        .unwrap()
        .is_none());
    drop(conn);
    let conn = db::init_db(&path).unwrap();
    let reloaded = db::get_whole_synthesis_progress(&conn, &run.run_id)
        .unwrap()
        .unwrap();
    assert_eq!(pending, reloaded);
    let mut dispatched = pending.clone();
    input
        .dispatch_section(
            dispatched.whole_document_progress.as_mut().unwrap(),
            0,
            &runtime,
            7,
        )
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers = (0..2)
        .map(|_| {
            let path = path.clone();
            let run_id = run.run_id.clone();
            let before = pending.clone();
            let after = dispatched.clone();
            let barrier = barrier.clone();
            let version = ingested.state_version;
            std::thread::spawn(move || {
                let mut conn = db::init_db(path).unwrap();
                barrier.wait();
                match db::save_whole_synthesis_progress(
                    &mut conn,
                    &run_id,
                    version,
                    Some(&before),
                    &after,
                ) {
                    Ok(()) => true,
                    Err(db::StoreError::WholeDocumentProgressConflict { .. }) => false,
                    Err(error) => panic!("unexpected progress reservation error: {error}"),
                }
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .filter(|success| *success)
            .count(),
        1,
        "only one writer may reserve this dispatch"
    );
    let mut conn = conn;
    let stored = db::get_whole_synthesis_progress(&conn, &run.run_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored, dispatched);
    assert!(matches!(
        stored.whole_document_progress.as_ref().unwrap().sections[0].state,
        WholeDocumentSectionState::Dispatched
    ));
    assert!(
        db::save_whole_synthesis_progress(
            &mut conn,
            &run.run_id,
            ingested.state_version,
            Some(&stored),
            &pending
        )
        .is_err(),
        "dispatch cannot be reset to pending"
    );
    let mut complete = stored.clone();
    let response = ModelResponse {
        text: responses[0].clone(),
        runtime_id: runtime.runtime_id().into(),
        model_id: runtime.model_id().into(),
        request_attempts: vec![],
    };
    input
        .record_section(
            complete.whole_document_progress.as_mut().unwrap(),
            0,
            &response,
        )
        .unwrap();
    db::save_whole_synthesis_progress(
        &mut conn,
        &run.run_id,
        ingested.state_version,
        Some(&stored),
        &complete,
    )
    .unwrap();
    assert!(db::get_synthesized_document(&conn, &run.run_id)
        .unwrap()
        .is_none());
    input.finalize_progress(&mut complete).unwrap();
    db::complete_synthesis(
        &mut conn,
        &run.run_id,
        ingested.state_version,
        &complete,
        vec![],
    )
    .unwrap();
    let final_doc = db::get_synthesized_document(&conn, &run.run_id)
        .unwrap()
        .unwrap();
    assert_eq!(final_doc, complete);
    assert_eq!(
        final_doc,
        db::get_synthesis_attempt(&conn, &run.run_id, 0)
            .unwrap()
            .unwrap()
    );
    assert_eq!(
        db::get_pipeline_run(&conn, &run.run_id)
            .unwrap()
            .unwrap()
            .state,
        PipelineState::Synthesized
    );
}

// Exercises the real stage/DB owners with authored public responses. This is
// not a model qualification or a parsing/OCR test.
struct WholeWriterStageRuntime {
    admission: WriterProjectionRuntime,
    binding: std::sync::Mutex<Option<(std::path::PathBuf, String)>>,
    requests: std::sync::Mutex<Vec<ModelRequest>>,
    generation_requests: std::sync::Mutex<Vec<ModelRequest>>,
    fail_ordinal: Option<u32>,
    schema_limit: Option<usize>,
    saturate_first: std::sync::atomic::AtomicBool,
}
impl WholeWriterStageRuntime {
    fn new() -> Self {
        Self {
            admission: WriterProjectionRuntime::new(7_500),
            binding: std::sync::Mutex::new(None),
            requests: std::sync::Mutex::new(vec![]),
            generation_requests: std::sync::Mutex::new(vec![]),
            fail_ordinal: None,
            schema_limit: None,
            saturate_first: std::sync::atomic::AtomicBool::new(false),
        }
    }
}
impl ModelRuntime for WholeWriterStageRuntime {
    fn response_schema_byte_limit(&self, _: PipelineStage, name: &str) -> usize {
        self.schema_limit
            .filter(|_| name == comparisons::SCHEMA_NAME)
            .unwrap_or_else(|| crate::pipeline::contracts::response_schema_byte_limit(name))
    }
    fn runtime_id(&self) -> &str {
        "public-section-stage"
    }
    fn model_id(&self) -> &str {
        "public-section-stage"
    }
    fn context_tokens(&self, _: PipelineStage) -> u32 {
        32_768
    }
    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        Ok(())
    }
    fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        if request.stage == PipelineStage::Synthesize {
            self.admission.preflight_request(request)
        } else {
            Ok(())
        }
    }
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        self.generation_requests
            .lock()
            .unwrap()
            .push(request.clone());
        let text = if request.stage == PipelineStage::Synthesize {
            let binding = self.binding.lock().unwrap();
            let (path, run) = binding.as_ref().unwrap();
            let mut conn = db::init_db(path).unwrap();
            assert!(db::get_synthesized_document(&conn, run).unwrap().is_none());
            assert!(db::get_synthesis_attempt(&conn, run, 0).unwrap().is_none());
            let saved = db::get_whole_synthesis_progress(&conn, run)
                .unwrap()
                .unwrap();
            let section =
                &saved.whole_document_progress.as_ref().unwrap().sections[request.ordinal as usize];
            assert_eq!(section.request, *request);
            assert!(matches!(
                section.state,
                crate::pipeline::contracts::WholeDocumentSectionState::Dispatched
            ));
            assert!(matches!(
                synthesize_analyzed_document_for_policy(
                    &mut conn,
                    self,
                    run,
                    &UNCONTROLLED_EXECUTION,
                    None,
                    WHOLE_DOCUMENT_VERSION
                ),
                Err(SummaryPipelineError::Store(
                    StoreError::WholeDocumentProgressConflict { .. }
                ))
            ));
            assert_eq!(
                db::get_pipeline_run(&conn, run).unwrap().unwrap().state,
                crate::pipeline::contracts::PipelineState::Synthesizing,
                "declining a duplicate invocation must not fail the active owner's run"
            );
            self.requests.lock().unwrap().push(request.clone());
            if self.fail_ordinal == Some(request.ordinal) {
                "invalid public response".into()
            } else {
                let prompt: Value = serde_json::from_str(&request.user_prompt).unwrap();
                let mut units = prompt["units"].as_array().unwrap().iter().filter(|u|u["primary"]==true).map(|u| {
                    let index = u["index"].as_u64().unwrap();
                    let source = prompt["sources"].as_array().unwrap().iter().find(|s|s[4]==index).unwrap();
                    json!({"text":format!("Operator records inspection {}.",index+1), "source_ids":[source[0]]})
                }).collect::<Vec<_>>();
                if self.saturate_first.swap(false, Ordering::SeqCst) {
                    let limit = prompt["maximum_units"].as_u64().unwrap();
                    units=(0..limit).map(|n|json!({"text":format!("Saturated inspection proposition {n}."),"source_ids":[prompt["sources"][0][0]]})).collect();
                }
                json!({"units":units}).to_string()
            }
        } else {
            crate::pipeline::summary::fixture_model_output(request)
        };
        Ok(ModelResponse {
            text,
            runtime_id: self.runtime_id().into(),
            model_id: self.model_id().into(),
            request_attempts: vec![],
        })
    }
}

fn whole_writer_stage_fixture(
    path: &Path,
    runtime: &WholeWriterStageRuntime,
) -> (Connection, String, NormalizedDocument, ChunkedDocument) {
    let texts = (1..=4)
        .map(|n| {
            format!(
                "Section {n} Inspections\n{}",
                format!("The operator records inspection number {n}. ").repeat(50)
            )
        })
        .collect::<Vec<_>>();
    whole_writer_stage_fixture_from_texts(path, runtime, &texts)
}

fn whole_writer_stage_fixture_from_texts(
    path: &Path,
    runtime: &WholeWriterStageRuntime,
    texts: &[String],
) -> (Connection, String, NormalizedDocument, ChunkedDocument) {
    use crate::pipeline::{contracts::PipelineState, ingest::prepare_pdf_ingestion};
    let normalized = public_document(texts);
    let chunked = whole_writer_chunks(&normalized);
    let analyzed = analyze(runtime, &chunked, &normalized, 7, &UNCONTROLLED_EXECUTION).unwrap();
    let mut conn = db::init_db(path).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
    let (mut document, mut run) = prepare_pdf_ingestion(fixture.to_str().unwrap(), None).unwrap();
    document.document_id = normalized.document_id.clone();
    run.document_id = normalized.document_id.clone();
    let ingested = db::persist_ingestion(&mut conn, &document, &run).unwrap();
    conn.execute(
        "UPDATE pipeline_runs SET state=?1 WHERE run_id=?2",
        rusqlite::params![
            serde_json::to_string(&PipelineState::Normalizing).unwrap(),
            run.run_id
        ],
    )
    .unwrap();
    let stored = db::complete_normalization(
        &mut conn,
        &run.run_id,
        ingested.state_version,
        &normalized,
        vec![],
    )
    .unwrap();
    conn.execute(
        "UPDATE pipeline_runs SET state=?1 WHERE run_id=?2",
        rusqlite::params![
            serde_json::to_string(&PipelineState::Chunking).unwrap(),
            run.run_id
        ],
    )
    .unwrap();
    let stored = db::complete_chunking(
        &mut conn,
        &run.run_id,
        stored.state_version,
        &chunked,
        vec![],
    )
    .unwrap();
    conn.execute(
        "UPDATE pipeline_runs SET state=?1 WHERE run_id=?2",
        rusqlite::params![
            serde_json::to_string(&PipelineState::Analyzing).unwrap(),
            run.run_id
        ],
    )
    .unwrap();
    db::complete_analysis(
        &mut conn,
        &run.run_id,
        stored.state_version,
        &analyzed,
        vec![],
    )
    .unwrap();
    *runtime.binding.lock().unwrap() = Some((path.to_owned(), run.run_id.clone()));
    (conn, run.run_id, normalized, chunked)
}

#[test]
fn whole_writer_stage_persists_before_send_and_completes_all_sections_once() {
    let runtime = WholeWriterStageRuntime::new();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("actual-stage.sqlite");
    let (mut conn, run, normalized, chunked) = whole_writer_stage_fixture(&path, &runtime);
    let draft = synthesize_analyzed_document_for_policy(
        &mut conn,
        &runtime,
        &run,
        &UNCONTROLLED_EXECUTION,
        None,
        WHOLE_DOCUMENT_VERSION,
    )
    .unwrap();
    let progress = draft.whole_document_progress.as_ref().unwrap();
    assert!(progress.sections.len() > 1);
    assert_eq!(
        runtime.requests.lock().unwrap().len(),
        progress.sections.len()
    );
    assert_eq!(draft.summary_claims.len(), 4);
    whole_document::SourceInput::new(&normalized, &chunked)
        .unwrap()
        .validate_saved_synthesis(&draft)
        .unwrap();
    drop(conn);
    let conn = db::init_db(&path).unwrap();
    assert_eq!(
        db::get_synthesized_document(&conn, &run).unwrap().unwrap(),
        draft
    );
    assert_eq!(
        db::get_synthesis_attempt(&conn, &run, 0).unwrap().unwrap(),
        draft
    );
    assert_eq!(
        db::get_pipeline_run(&conn, &run).unwrap().unwrap().state,
        crate::pipeline::contracts::PipelineState::Synthesized
    );
}

#[test]
fn whole_writer_stage_stops_on_a_failed_section_without_retry_or_completion() {
    let mut runtime = WholeWriterStageRuntime::new();
    runtime.fail_ordinal = Some(1);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("failed-stage.sqlite");
    let (mut conn, run, _, _) = whole_writer_stage_fixture(&path, &runtime);
    assert!(synthesize_analyzed_document_for_policy(
        &mut conn,
        &runtime,
        &run,
        &UNCONTROLLED_EXECUTION,
        None,
        WHOLE_DOCUMENT_VERSION
    )
    .is_err());
    let calls = runtime.requests.lock().unwrap().len();
    assert_eq!(calls, 2);
    let saved = db::get_whole_synthesis_progress(&conn, &run)
        .unwrap()
        .unwrap();
    use crate::pipeline::contracts::WholeDocumentSectionState;
    assert!(matches!(
        saved.whole_document_progress.as_ref().unwrap().sections[0].state,
        WholeDocumentSectionState::Completed { .. }
    ));
    assert!(matches!(
        saved.whole_document_progress.as_ref().unwrap().sections[1].state,
        WholeDocumentSectionState::Failed { .. }
    ));
    assert!(db::get_synthesized_document(&conn, &run).unwrap().is_none());
    assert!(db::get_synthesis_attempt(&conn, &run, 0).unwrap().is_none());
    assert!(synthesize_analyzed_document_for_policy(
        &mut conn,
        &runtime,
        &run,
        &UNCONTROLLED_EXECUTION,
        None,
        WHOLE_DOCUMENT_VERSION
    )
    .is_err());
    assert_eq!(runtime.requests.lock().unwrap().len(), calls);
}

#[test]
fn whole_writer_stage_cancellation_retains_completed_section_without_sending_next() {
    struct CancelAfterOne<'a>(&'a WholeWriterStageRuntime);
    impl ExecutionControl for CancelAfterOne<'_> {
        fn cancellation_requested(&self) -> bool {
            !self.0.requests.lock().unwrap().is_empty()
        }
    }
    let runtime = WholeWriterStageRuntime::new();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cancelled-stage.sqlite");
    let (mut conn, run, _, _) = whole_writer_stage_fixture(&path, &runtime);
    assert!(matches!(
        synthesize_analyzed_document_for_policy(
            &mut conn,
            &runtime,
            &run,
            &CancelAfterOne(&runtime),
            None,
            WHOLE_DOCUMENT_VERSION
        ),
        Err(SummaryPipelineError::CancellationObserved)
    ));
    assert_eq!(runtime.requests.lock().unwrap().len(), 1);
    drop(conn);
    let conn = db::init_db(&path).unwrap();
    let saved = db::get_whole_synthesis_progress(&conn, &run)
        .unwrap()
        .unwrap();
    let progress = saved.whole_document_progress.as_ref().unwrap();
    use crate::pipeline::contracts::WholeDocumentSectionState;
    assert!(matches!(
        progress.sections[0].state,
        WholeDocumentSectionState::Completed { .. }
    ));
    assert!(progress.sections[1..]
        .iter()
        .all(|s| matches!(s.state, WholeDocumentSectionState::Pending)));
    assert!(progress.assembly_lineage.is_none());
    assert!(db::get_synthesized_document(&conn, &run).unwrap().is_none());
    assert!(db::get_synthesis_attempt(&conn, &run, 0).unwrap().is_none());
}

#[test]
fn whole_writer_assembly_applies_claim_ceiling_across_all_sections() {
    let runtime = WholeWriterStageRuntime::new();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("assembly-cap.sqlite");
    let (_, _, normalized, chunked) = whole_writer_stage_fixture(&path, &runtime);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    assert!(plan.drafts.len() > 1 && plan.drafts.len() < 32);
    for count in [63, 64, 65] {
        let mut responses = Vec::new();
        let mut remaining = count;
        let mut position = 0;
        for (i, draft) in plan.drafts.iter().enumerate() {
            let take = if i + 1 == plan.drafts.len() {
                remaining
            } else {
                remaining / (plan.drafts.len() - i)
            };
            assert!(take > 0 && take <= MAX_VERIFICATION_BATCHES);
            let units=(0..take).map(|_| {position+=1;json!({"text":format!("Operator records inspection {position}."),"source_ids":[draft.catalog.candidates[0].request_id]})}).collect::<Vec<_>>();
            remaining -= take;
            responses.push(json!({"units":units}).to_string());
        }
        let assembly = input.assemble(&plan, &responses, 7);
        if count <= MAX_VERIFICATION_BATCHES {
            let assembly = assembly.unwrap();
            assert_eq!(assembly.claims.len(), count);
            assert_eq!(assembly.lineage.len(), count);
        } else {
            assert_eq!(
                assembly.unwrap_err().code,
                "WHOLE_DOCUMENT_CLAIM_CAPACITY_EXCEEDED"
            );
        }
    }
}

#[test]
fn whole_writer_all_held_fails_completion_and_preserves_the_verification_attempt() {
    let mut runtime = WholeWriterStageRuntime::new();
    runtime.schema_limit = Some(0);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("all-held.sqlite");
    let (mut conn, run, _, _) = whole_writer_stage_fixture(&path, &runtime);
    synthesize_analyzed_document_for_policy(
        &mut conn,
        &runtime,
        &run,
        &UNCONTROLLED_EXECUTION,
        None,
        WHOLE_DOCUMENT_VERSION,
    )
    .unwrap();
    let failure =
        verify_synthesized_document_controlled(&mut conn, &runtime, &run, &UNCONTROLLED_EXECUTION)
            .unwrap_err();
    assert_eq!(failure.code(), "NO_SEMANTICALLY_SUPPORTED_CLAIMS");
    assert!(db::get_verified_document(&conn, &run).unwrap().is_none());
    assert!(db::get_summary_artifact(&conn, &run).unwrap().is_none());
    let verified = db::get_verification_attempt(&conn, &run, 0)
        .unwrap()
        .unwrap();
    assert_eq!(verified.summary_claim_verifications.len(), 4);
    assert!(verified
        .summary_claim_verifications
        .iter()
        .all(|v| v.verdict == ClaimVerdict::Ambiguous && v.admission_withhold.is_some()));
    assert!(verified.summary_claims.is_empty());
    assert_eq!(runtime.generation_requests.lock().unwrap().iter().filter(|r| matches!(&r.output_format,ModelOutputFormat::JsonSchema{name,..} if name==comparisons::SCHEMA_NAME || name=="document_trimmed_source_contribution_v1")).count(),0);
    assert_eq!(
        db::get_pipeline_run(&conn, &run).unwrap().unwrap().state,
        crate::pipeline::contracts::PipelineState::Failed
    );
}

#[test]
fn review132_bare_document_names_are_not_invented_dependencies() {
    use whole_clauses::DependencyProjection;
    for text in [
        "Section 1 Terms\nThe Prime Contract is included below.",
        "Section 1 Terms\nPrime Contract is the name of this document.",
    ] {
        let normalized = public_document(&[text.into()]);
        let projection = DependencyProjection::new(&normalized);
        assert!(
            projection.references.iter().all(Vec::is_empty),
            "bare names cannot establish an absent document"
        );
    }
}

#[test]
fn review132_unresolved_dependencies_block_the_writer_consumer() {
    use whole_clauses::DependencyProjection;
    for (reference, extra, reason) in [
        ("Section 9.4", None, "missing_target"),
        ("subsection (b)", None, "unsupported_form"),
        (
            "Section 1.2",
            Some("Section 1.2 Duplicate\nA second rule applies."),
            "duplicate_target",
        ),
    ] {
        let mut texts = vec![
            format!("Section 1.1 Duty\nThe operator follows {reference}."),
            "Section 1.2 Rule\nThe operator must inspect.".into(),
        ];
        if let Some(extra) = extra {
            texts.push(extra.into());
        }
        let normalized = public_document(&texts);
        let projection = DependencyProjection::new(&normalized);
        assert!(projection.blocking_reasons(0).contains(&reason));
        let chunked = whole_writer_chunks(&normalized);
        let failure = match whole_document::SourceInput::new(&normalized, &chunked) {
            Ok(_) => panic!("unresolved {reason} reached the writer planner"),
            Err(failure) => failure,
        };
        assert_eq!(failure.code, "WHOLE_DOCUMENT_DEPENDENCY_UNRESOLVED");
        assert!(failure.message.contains(reason));
    }
}

#[test]
fn review132_identical_claim_text_keeps_distinct_source_owners() {
    let normalized = public_document(
        &(1..=4)
            .map(|n| {
                format!(
                    "Section {n} Inspections\n{}",
                    "The operator must inspect the equipment. ".repeat(50)
                )
            })
            .collect::<Vec<_>>(),
    );
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let plan = input
        .plan(
            &WriterProjectionRuntime::new(7_500),
            7,
            &UNCONTROLLED_EXECUTION,
        )
        .unwrap();
    assert!(plan.drafts.len() > 1);
    let responses = plan.drafts.iter().map(|draft| {
        let unit = draft.primary[0];
        let (_, source) = input.anchors.iter().zip(&input.catalog.candidates)
            .find(|(anchor, _)|anchor.unit == unit).unwrap();
        json!({"units":[{"text":"The operator must inspect the equipment.","source_ids":[source.request_id]}]}).to_string()
    }).collect::<Vec<_>>();
    let assembled = input.assemble(&plan, &responses, 7).unwrap();
    assert_eq!(
        assembled.claims.len(),
        responses.len(),
        "identical wording with different source ownership is not a duplicate"
    );
    assert_eq!(
        assembled
            .claims
            .iter()
            .map(|c| &c.evidence_ids)
            .collect::<BTreeSet<_>>()
            .len(),
        responses.len()
    );
    assert_eq!(
        assembled
            .lineage
            .iter()
            .map(|(_, _, target)| *target)
            .collect::<BTreeSet<_>>()
            .len(),
        responses.len()
    );
}

#[test]
fn review132_sections_keep_full_claim_character_allowance() {
    struct TokenBudget;
    impl ModelRuntime for TokenBudget {
        fn generate(&self, _: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            panic!("offline planner must not generate")
        }
        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            panic!("offline planner must not start a runtime")
        }
        fn runtime_id(&self) -> &str {
            "public-token-boundary"
        }
        fn model_id(&self) -> &str {
            "public-token-boundary"
        }
        fn context_tokens(&self, _: PipelineStage) -> u32 {
            32_768
        }
        fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
            if request.user_prompt.len() / 4 + request.max_output_tokens as usize > 24_000 {
                Err(ModelRuntimeFailure {
                    code: "MODEL_CONTEXT_EXCEEDED".into(),
                    message: "public joint input/output boundary".into(),
                    recoverable: false,
                    request_attempts: vec![],
                })
            } else {
                Ok(())
            }
        }
    }
    let normalized = public_document(
        &(1..=4)
            .map(|n| {
                format!(
                    "Section {n} Inspections\n{}",
                    "The operator must inspect the equipment. ".repeat(50)
                )
            })
            .collect::<Vec<_>>(),
    );
    let chunked = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let plan = input
        .plan(&TokenBudget, 7, &UNCONTROLLED_EXECUTION)
        .unwrap();
    for draft in &plan.drafts {
        assert_eq!(
            budget::maximum_characters(&draft.request).unwrap(),
            MAX_UNIT_CHARACTERS,
            "section packing must split rather than shrink the claim schema"
        );
    }
    assert_eq!(
        plan.drafts.len(),
        1,
        "unused answer reservation must not force extra sections"
    );
    let indivisible = public_document(&[format!(
        "Section 1 Inspections\n{}",
        "The operator must inspect the equipment. ".repeat(4_000)
    )]);
    let chunks = whole_writer_chunks(&indivisible);
    let source = whole_document::SourceInput::new(&indivisible, &chunks).unwrap();
    assert_eq!(
        source
            .plan(&TokenBudget, 7, &UNCONTROLLED_EXECUTION)
            .unwrap_err()
            .code,
        "WHOLE_DOCUMENT_UNIT_TOO_LARGE",
        "an indivisible source must hold instead of shrinking claims"
    );
}

#[test]
fn review132_external_source_disclosures_reach_delivered_output_and_reload() {
    let runtime = WholeWriterStageRuntime::new();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("external-coverage.sqlite");
    let texts = vec!["Section 1 Inspections
The operator records inspection number 1. Equipment follows Schedule Z and Section 91 of the Lunar Port Standard.".into()];
    let (mut conn, run, _, _) = whole_writer_stage_fixture_from_texts(&path, &runtime, &texts);
    synthesize_analyzed_document_for_policy(
        &mut conn,
        &runtime,
        &run,
        &UNCONTROLLED_EXECUTION,
        None,
        WHOLE_DOCUMENT_VERSION,
    )
    .unwrap();
    verify_synthesized_document_controlled(&mut conn, &runtime, &run, &UNCONTROLLED_EXECUTION)
        .unwrap();
    complete_verified_document(&mut conn, &run).unwrap();
    let view = crate::pipeline::workspace::get_persisted_summary(&conn, &run).unwrap();
    for disclosure in [
        "attachment not included in this document",
        "text not in this document",
    ] {
        assert!(
            view.summary
                .warnings
                .iter()
                .any(|w| w.message.contains(disclosure)),
            "external source coverage notice is missing from delivered output: {disclosure}"
        );
    }
    drop(conn);
    let conn = db::init_db(&path).unwrap();
    assert_eq!(
        crate::pipeline::workspace::get_persisted_summary(&conn, &run).unwrap(),
        view
    );
    let mut draft = db::get_synthesized_document(&conn, &run).unwrap().unwrap();
    let normalized = db::get_normalized_document(&conn, &run).unwrap().unwrap();
    let chunked = db::get_chunked_document(&conn, &run).unwrap().unwrap();
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let position = draft
        .warnings
        .iter()
        .position(|w| w.message.contains("text not in this document"))
        .unwrap();
    draft.warnings.remove(position);
    assert!(
        input.validate_saved_synthesis(&draft).is_err(),
        "a dropped source coverage disclosure must fail saved validation"
    );
}

#[test]
fn review132_named_qualifiers_use_syntax_not_document_vocabulary() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    for reference in [
        "Section 7 of the Purple Reference Document",
        "Purple Reference Document Section 7",
    ] {
        let normalized = public_document(&[
            format!("Section 1 Scope\nEquipment follows {reference}."),
            "Section 7 Local\nThe unrelated local rule applies.".into(),
        ]);
        let projection = DependencyProjection::new(&normalized);
        assert!(projection.blocking_reasons(0).is_empty());
        assert!(matches!(
            projection.references[0][0].status,
            ReferenceStatus::External(_)
        ));
        assert_eq!(projection.references[0][0].text, reference);
        assert!(!projection
            .context(0, &normalized)
            .contains("unrelated local rule"));
    }
    let normalized = public_document(&[
        "Section 1 Scope\nEquipment follows Section 7 of this Document.".into(),
        "Section 7 Local\nThe local rule applies.".into(),
    ]);
    let projection = DependencyProjection::new(&normalized);
    assert!(matches!(
        projection.references[0][0].status,
        ReferenceStatus::Resolved(_)
    ));
}

#[test]
fn review132_preposed_names_preserve_namespace_boundaries() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    for (reference, name) in [
        ("of 42 XYZ Section 7", "42 XYZ"),
        ("17 X.Y.Z. Section 7", "17 X.Y.Z."),
    ] {
        let normalized = public_document(&[
            format!("Section 1 Scope\nEquipment follows requirements {reference}."),
            "Section 7 Local\nThe unrelated local rule applies.".into(),
        ]);
        let projection = DependencyProjection::new(&normalized);
        let ReferenceStatus::External(detail) = &projection.references[0][0].status else {
            panic!("explicit namespace missing")
        };
        assert_eq!(
            detail.document, name,
            "connectors/abbreviation punctuation changed namespace identity"
        );
    }
    let normalized = public_document(&[
        "Section 1 Scope\nForeign Reference Manual, Section 7 applies.".into(),
        "Section 7 Local\nThe local rule applies.".into(),
    ]);
    let projection = DependencyProjection::new(&normalized);
    assert!(
        matches!(
            projection.references[0][0].status,
            ReferenceStatus::Resolved(_)
        ),
        "a name across a comma is not an explicit namespace binding"
    );
}

#[test]
fn review132_capitalized_titles_do_not_prove_external_document_identity() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    for reference in ["Payment Terms Section 7", "Section 7 of the Payment Terms"] {
        let normalized = public_document(&[
            format!("Section 1 Scope\nEquipment follows {reference}."),
            "Section 7 Payment Terms\nThe operator must obtain written permission.".into(),
        ]);
        let projection = DependencyProjection::new(&normalized);
        let chunked = whole_writer_chunks(&normalized);
        assert!(
            matches!(
                projection.references[0][0].status,
                ReferenceStatus::Unresolved("ambiguous_document_qualifier")
            ),
            "a body heading names no document scope"
        );
        assert!(whole_document::SourceInput::new(&normalized, &chunked).is_err());
    }
}

#[test]
fn review132_scope_identity_requires_whole_document_or_annex_identity() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    for (texts, external) in [
        (
            vec![
                "FIRST AMENDMENT TO LEASE AGREEMENT",
                "Section 1 Scope\nEquipment follows Section 3 of the Lease Agreement.",
                "Section 3 Local\nThe amendment requires written permission.",
            ],
            true,
        ),
        (
            vec![
                "Section 1 Scope\nEquipment follows Section 7 of the Supply Agreement.",
                "Section 7 Supply Agreement Terms\nThe local terms require written permission.",
            ],
            true,
        ),
        (
            vec![
                "Section 1 Scope\nEquipment follows Section 7 of the Master Lease.",
                "Section 7 Master Lease\nThe sublease requires written permission.",
            ],
            false,
        ),
        (
            vec![
                "The name of this agreement is “First Amendment to Lease Agreement”.",
                "Section 1 Scope\nEquipment follows Section 3 of the Lease Agreement.",
                "Section 3 Local\nThe amendment requires written permission.",
            ],
            true,
        ),
    ] {
        let normalized = public_document(&texts.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let projection = DependencyProjection::new(&normalized);
        let reference = projection.references.iter().flatten().next().unwrap();
        assert!(
            if external {
                matches!(reference.status, ReferenceStatus::External(_))
            } else {
                matches!(reference.status, ReferenceStatus::Unresolved(_))
            },
            "a mentioned name cannot own the local numbering scope: {:?}",
            reference.status
        );
        let chunked = whole_writer_chunks(&normalized);
        assert_eq!(
            whole_document::SourceInput::new(&normalized, &chunked).is_ok(),
            external
        );
    }
    for title in [
        "LEASE AGREEMENT",
        "The name of this agreement is “Lease Agreement”.",
    ] {
        let normalized = public_document(&[
            title.into(),
            "Section 1 Scope\nEquipment follows Section 3 of the Lease Agreement.".into(),
            "Section 3 Local\nWritten permission is required.".into(),
        ]);
        let projection = DependencyProjection::new(&normalized);
        assert!(matches!(
            projection
                .references
                .iter()
                .flatten()
                .next()
                .unwrap()
                .status,
            ReferenceStatus::Resolved(_)
        ));
    }
}

#[test]
fn review132_current_document_qualifier_keeps_existing_scope_grammar() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    for (kind, supported) in [
        ("Document", true),
        ("Agreement", true),
        ("Contract", true),
        ("Article", false),
        ("Exhibit", false),
        ("Schedule", false),
    ] {
        let normalized = public_document(&[
            format!("Section 1 Scope\nEquipment follows Section 7 of this {kind}."),
            "Section 7 Local\nWritten permission is required.".into(),
        ]);
        let projection = DependencyProjection::new(&normalized);
        assert_eq!(
            matches!(
                projection.references[0][0].status,
                ReferenceStatus::Resolved(_)
            ),
            supported,
            "current-scope kind {kind}: {:?}",
            projection.references[0][0].status
        );
    }
}

#[test]
fn reference_identity_opening_parenthetical_alias_resolves_before_external_fallback() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    for (title, subject) in [
        ("SERVICE TERMS", "contract"),
        ("SERVICE TERMS", "document"),
        ("SERVICE TERMS", "agreement"),
        (
            "AMENDED AND RESTATED LEASE AGREEMENT",
            "Amended and Restated Lease Agreement",
        ),
    ] {
        for (open, close) in [('“', '”'), ('"', '"')] {
            let normalized = public_document(&[format!(
                "{title}\n\nThis {subject} (the {open}Service Agreement{close}) governs the services.\n\nSection 1 Scope\nEquipment follows Section 3 of the Service Agreement.\n\nSection 3 Local\nWritten permission is required."
            )]);
            let projection = DependencyProjection::new(&normalized);
            let reference = projection.references.iter().flatten().next().unwrap();
            let ReferenceStatus::Resolved(targets) = &reference.status else {
                panic!(
                    "self-alias must resolve: subject={subject} status={:?}",
                    reference.status
                );
            };
            assert!(targets
                .iter()
                .any(|i| projection.clauses[*i].number == Some(vec!["3".into()])));
            assert!(projection
                .closures
                .iter()
                .enumerate()
                .all(|(i, _)| projection.blocking_reasons(i).is_empty()));
        }
    }
}

#[test]
fn reference_identity_explicit_containment_is_external_but_opaque_containment_blocks() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    for (opening, heading, name, external) in [
        (
            "FIRST AMENDMENT TO LEASE AGREEMENT",
            "Local",
            "Lease Agreement",
            true,
        ),
        (
            "SERVICE TERMS",
            "Supply Agreement Terms",
            "Supply Agreement",
            true,
        ),
        (
            "The name of this agreement is “First Amendment to Lease Agreement”.",
            "Local",
            "Lease Agreement",
            true,
        ),
        ("SUBLEASE", "Master Lease", "Master Lease", false),
    ] {
        let normalized = public_document(&[format!(
            "{opening}\n\nSection 1 Scope\nEquipment follows Section 3 of the {name}.\n\nSection 3 {heading}\nUnrelated local rule applies."
        )]);
        let projection = DependencyProjection::new(&normalized);
        let reference = projection.references.iter().flatten().next().unwrap();
        if external {
            let ReferenceStatus::External(detail) = &reference.status else {
                panic!(
                    "containment is not ownership: name={name} status={:?}",
                    reference.status
                );
            };
            assert_eq!(detail.document, name);
            assert_eq!(detail.disclosure, "text not in this document");
        } else {
            assert!(matches!(reference.status, ReferenceStatus::Unresolved(_)));
        }
    }
}

#[test]
fn reference_identity_alias_requires_opening_self_subject_and_balanced_quotes() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    for declaration in [
        "This contract (the “Service Agreement\") governs services.",
        "This contract (the \"Service Agreement”) governs services.",
        "This contract (the “Service Agreement” governs services.",
        "This contract (the “”Service Agreement) governs services.",
        "This contract (the “Service Agreement” extra) governs services.",
        "This amendment modifies a separate contract (the “Service Agreement”).",
        "This Separate Lease Agreement (the “Service Agreement”) governs services.",
    ] {
        let normalized = public_document(&[format!(
            "SERVICE TERMS\n\n{declaration}\n\nSection 1 Scope\nEquipment follows Section 3 of the Service Agreement.\n\nSection 3 Local\nUnrelated local rule applies."
        )]);
        let projection = DependencyProjection::new(&normalized);
        let reference = projection.references.iter().flatten().next().unwrap();
        assert!(
            !matches!(reference.status, ReferenceStatus::Resolved(_)),
            "non-self or malformed alias must not bind locally: {declaration}: {:?}",
            reference.status
        );
    }
    for declaration in [
        "This contract (the “Service Agreement”) governs this section.",
        "The name of this contract is “Service Agreement”.",
    ] {
        let normalized = public_document(&[format!(
            "SERVICE TERMS\n\nSection 1 Scope\nEquipment follows Section 3 of the Service Agreement.\n\nSection 2 Local Definition\n{declaration}\n\nSection 3 Local\nUnrelated local rule applies."
        )]);
        let projection = DependencyProjection::new(&normalized);
        let reference = projection.references.iter().flatten().next().unwrap();
        assert!(
            !matches!(reference.status, ReferenceStatus::Resolved(_)),
            "body declaration must not supply whole-document scope: {declaration}"
        );
    }
}

#[test]
fn reference_identity_owned_alias_errors_do_not_fall_through_to_external() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    for tail in [
        "Section 2 Local\nNo third section is supplied.",
        "Section 3 Local\nWritten permission is required.\n\nSection 3 Duplicate\nAnother rule applies.",
        "Section 3 Local\nWritten permission is required.\n\nSchedule A Service Agreement\nSection 3 Annex\nAnother rule applies.",
        "Section 3 Local\nWritten permission is required.\n\nUNRECOGNISED HEADING\nAnother rule applies.",
    ] {
        let normalized = public_document(&[format!(
            "SERVICE TERMS\n\nThis contract (the “Service Agreement”) governs services.\n\nSection 1 Scope\nEquipment follows Section 3 of the Service Agreement.\n\n{tail}"
        )]);
        let projection = DependencyProjection::new(&normalized);
        let reference = projection.references.iter().flatten().next().unwrap();
        assert!(!matches!(reference.status, ReferenceStatus::External(_)),
            "an ownership error must not become absence: {tail}: {:?}", reference.status);
        let origin = projection.references.iter().position(|r| !r.is_empty()).unwrap();
        assert!(!projection.blocking_reasons(origin).is_empty(),
            "an owned but unclean target must block: {tail}");
        let chunked = whole_writer_chunks(&normalized);
        assert!(whole_document::SourceInput::new(&normalized, &chunked).is_err());
    }
}

#[test]
fn baseline_repair_owned_namespace_malformed_targets_remain_blocking() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    for target in ["b", "7(b)", "7 and b"] {
        let normalized = public_document(&[
            "The name of this agreement is “Lunar Port Agreement”.".into(),
            format!(
                "Section 1 Scope\nEquipment follows Section {target} of the Lunar Port Agreement."
            ),
            "Section 7 Inspection\nWritten permission is required.".into(),
        ]);
        let projection = DependencyProjection::new(&normalized);
        let references = projection.references.iter().flatten().collect::<Vec<_>>();
        assert!(
            !references.is_empty(),
            "malformed owned reference must not disappear: {target}"
        );
        assert!(references.iter().any(|r| matches!(r.status, ReferenceStatus::Unresolved(_))),
            "malformed owned reference must not become a whole-section approval: {target}: {references:?}");
        let chunked = whole_writer_chunks(&normalized);
        assert!(whole_document::SourceInput::new(&normalized, &chunked).is_err());
    }
}

#[test]
fn baseline_repair_owned_namespace_resolves_before_external_classification() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    for name in ["Lunar Port Standard", "Lunar Port Agreement"] {
        let normalized = public_document(&[
            format!("The name of this agreement is “{name}”."),
            format!("Section 1 Scope\nEquipment follows Section 7 of the {name}."),
            "Section 7 Inspection\nWritten permission is required.".into(),
        ]);
        let projection = DependencyProjection::new(&normalized);
        let reference = projection.references.iter().flatten().next().unwrap();
        assert!(
            matches!(&reference.status, ReferenceStatus::Resolved(targets) if !targets.is_empty()),
            "owned name with a clean target must resolve internally: {:?}",
            reference.status
        );
        assert!(projection
            .closures
            .iter()
            .enumerate()
            .all(|(i, _)| projection.blocking_reasons(i).is_empty()));
    }
    let normalized = public_document(&[
        "Section 1 Scope\nEquipment follows Section 7 of the Separate Supply Agreement.".into(),
        "Section 7 Inspection\nWritten permission is required.".into(),
    ]);
    let projection = DependencyProjection::new(&normalized);
    assert!(matches!(projection.references[0][0].status, ReferenceStatus::External(_)),
        "the existing explicit-document grammar owns a different namespace even with the same local number");

    let normalized = public_document(&[
        "Section 1 Scope\nEquipment follows Section 7 of the Lunar Port Standard.".into(),
        "Section 7 Local\nLocal unrelated text applies.".into(),
        "Schedule A Lunar Port Standard\nSection 7 Inspection\nWritten permission is required."
            .into(),
    ]);
    let projection = DependencyProjection::new(&normalized);
    let ReferenceStatus::Resolved(targets) = &projection.references[0][0].status else {
        panic!(
            "owned annex must resolve in its own numbering namespace: {:?}",
            projection.references[0][0].status
        );
    };
    assert!(targets.iter().any(|index| projection.clauses[*index]
        .headings
        .iter()
        .any(|h| h.contains("Inspection"))));
    assert!(!targets.iter().any(|index| projection.clauses[*index]
        .headings
        .iter()
        .any(|h| h.contains("Local"))));
}

#[test]
fn baseline_repair_unowned_explicit_namespace_is_external_but_partial_ownership_blocks() {
    use whole_clauses::{DependencyProjection, ReferenceStatus};
    let cases = [
        (
            "Section 91 of the Lunar Port Standard",
            "Section 7 Local\nWritten permission is required.",
            true,
        ),
        (
            "Article IX of the Lunar Port Standard",
            "ARTICLE II LOCAL\nWritten permission is required.",
            true,
        ),
        (
            "Section 91 of the Lunar Port Standard",
            "Section 7 Lunar Port Standard\nWritten permission is required.",
            false,
        ),
        (
            "Section 91 of the Lunar Port Standard",
            "Schedule A Lunar Port Standard\nWritten permission is required.",
            false,
        ),
        (
            "Section 7 of the Lunar Port Standard",
            "Section 7 Local\nWritten permission is required.",
            false,
        ),
        (
            "Section 91 of the Lunar Port Standard",
            "LUNAR PORT STANDARD\nThis document governs the operator.",
            false,
        ),
        (
            "Section 91 of the Lunar Port Agreement",
            "Section 7 Lunar Port Agreement\nWritten permission is required.",
            true,
        ),
        (
            "Section 91 of the Lunar Port Standard",
            "The name of this agreement is “Lunar Port Standard”.",
            false,
        ),
        (
            "Section 91 of the Lunar Port Standard",
            "This agreement is called “Lunar Port Standard”.",
            false,
        ),
        (
            "Section 91 of the Lunar Port Standard",
            "This Lunar Port Standard governs the operator.",
            false,
        ),
        (
            "Section 91 of the Lunar Port Standard",
            "This agreement incorporates the separate Lunar Port Standard.",
            true,
        ),
    ];
    for (reference, owner, external) in cases {
        let heading = if reference.starts_with("Article") {
            "ARTICLE I SCOPE"
        } else {
            "Section 1 Scope"
        };
        let normalized = public_document(&[
            format!("{heading}\nEquipment follows {reference}."),
            owner.into(),
        ]);
        let projection = DependencyProjection::new(&normalized);
        let status = &projection.references[0][0].status;
        assert_eq!(
            matches!(status, ReferenceStatus::External(_)),
            external,
            "reference={reference} owner={owner} status={status:?}"
        );
        if external {
            let ReferenceStatus::External(detail) = status else {
                unreachable!()
            };
            let (name, reason) = if reference == "Section 91 of the Lunar Port Agreement" {
                ("Lunar Port Agreement", "absent_document")
            } else {
                ("Lunar Port Standard", "unowned_named_document")
            };
            assert_eq!(detail.document, name);
            assert_eq!(detail.reason, reason);
            assert_eq!(detail.disclosure, "text not in this document");
            let chunked = whole_writer_chunks(&normalized);
            if let Err(error) = whole_document::SourceInput::new(&normalized, &chunked) {
                panic!("reference={reference} owner={owner} constructor={error:?}");
            }
        } else {
            assert!(!projection.blocking_reasons(0).is_empty());
        }
    }
}

#[test]
fn baseline_repair_section_reserves_only_the_claims_that_fit() {
    struct LimitedOutput;
    impl ModelRuntime for LimitedOutput {
        fn generate(&self, _: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            panic!("offline")
        }
        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            panic!("offline")
        }
        fn runtime_id(&self) -> &str {
            "bounded-output"
        }
        fn model_id(&self) -> &str {
            "bounded-output"
        }
        fn context_tokens(&self, _: PipelineStage) -> u32 {
            32_768
        }
        fn preflight_request(&self, r: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
            if r.user_prompt.len() / 4 + r.max_output_tokens as usize > 12_000 {
                Err(ModelRuntimeFailure {
                    code: "MODEL_CONTEXT_EXCEEDED".into(),
                    message: "joint token budget".into(),
                    recoverable: false,
                    request_attempts: vec![],
                })
            } else {
                Ok(())
            }
        }
    }
    let normalized =
        public_document(&["Section 1 Inspection\nThe operator must inspect the equipment.".into()]);
    let chunks = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunks).unwrap();
    let plan = input
        .plan(&LimitedOutput, 7, &UNCONTROLLED_EXECUTION)
        .expect("unused document-wide answer reservation must not reject a complete source unit");
    assert_eq!(plan.drafts.len(), 1);
    let request = &plan.drafts[0].request;
    let ModelOutputFormat::JsonSchema { schema, .. } = &request.output_format else {
        panic!("schema")
    };
    let count = schema["properties"]["units"]["maxItems"].as_u64().unwrap();
    assert!(count > 0 && count < MAX_VERIFICATION_BATCHES as u64);
    assert_eq!(
        budget::maximum_characters(request).unwrap(),
        MAX_UNIT_CHARACTERS
    );
    let payload: Value = serde_json::from_str(&request.user_prompt).unwrap();
    assert_eq!(payload["maximum_units"], json!(count));
    LimitedOutput.preflight_request(request).unwrap();
}

#[test]
fn baseline_repair_saturated_section_never_completes_silently() {
    use crate::pipeline::contracts::WholeDocumentSectionState;
    let normalized =
        public_document(&["Section 1 Inspection\nThe operator inspects the equipment.".into()]);
    let chunks = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunks).unwrap();
    let runtime = WriterProjectionRuntime::new(usize::MAX);
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    let mut progress = input.new_progress(&plan, &runtime, 7).unwrap();
    let request = input
        .dispatch_section(&mut progress, 0, &runtime, 7)
        .unwrap();
    let ModelOutputFormat::JsonSchema { schema, .. } = &request.output_format else {
        panic!("schema")
    };
    let limit = schema["properties"]["units"]["maxItems"].as_u64().unwrap() as usize;
    let text=json!({"units":(0..limit).map(|n|json!({"text":format!("The operator records inspection number {n}."),"source_ids":["s1"]})).collect::<Vec<_>>()}).to_string();
    input
        .record_section(
            &mut progress,
            0,
            &ModelResponse {
                text,
                runtime_id: runtime.runtime_id().into(),
                model_id: runtime.model_id().into(),
                request_attempts: vec![],
            },
        )
        .unwrap();
    assert!(
        !matches!(
            progress.sections[0].state,
            WholeDocumentSectionState::Completed { .. }
        ),
        "filling the schema allowance cannot certify complete source coverage"
    );
    assert!(input.finish_progress(&mut progress).is_err());
    assert_eq!(
        input
            .split_saturated(&mut progress, 0, &runtime, 7)
            .unwrap_err()
            .code,
        "WHOLE_DOCUMENT_CLAIM_CAPACITY_EXCEEDED"
    );
    assert!(matches!(
        progress.sections[0].state,
        WholeDocumentSectionState::Saturated { .. }
    ));
}

#[test]
fn review132_context_saturation_and_document_ceiling_have_distinct_stops() {
    let normalized =
        public_document(&["Section 1 Inspection\nThe operator inspects equipment.".into()]);
    let chunks = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunks).unwrap();
    let mut runtime = WriterProjectionRuntime::new(usize::MAX);
    runtime.context_tokens = 8_192;
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    let mut progress = input.new_progress(&plan, &runtime, 7).unwrap();
    let request = input
        .dispatch_section(&mut progress, 0, &runtime, 7)
        .unwrap();
    let prompt: Value = serde_json::from_str(&request.user_prompt).unwrap();
    let limit = prompt["maximum_units"].as_u64().unwrap();
    assert!(limit < MAX_VERIFICATION_BATCHES as u64);
    let text = json!({"units":(0..limit).map(|n|json!({"text":format!("Inspection proposition {n}."),"source_ids":["s1"]})).collect::<Vec<_>>()}).to_string();
    input
        .record_section(
            &mut progress,
            0,
            &ModelResponse {
                text,
                runtime_id: runtime.runtime_id().into(),
                model_id: runtime.model_id().into(),
                request_attempts: vec![],
            },
        )
        .unwrap();
    assert_eq!(
        input
            .split_saturated(&mut progress, 0, &runtime, 7)
            .unwrap_err()
            .code,
        "WHOLE_DOCUMENT_SECTION_SATURATED"
    );
}

#[test]
fn review132_last_child_at_document_ceiling_reports_capacity_without_delivery() {
    let normalized = public_document(&[
        "Section 1 Inspections\nThe operator inspects equipment.".into(),
        "Section 2 Records\nThe operator keeps records.".into(),
    ]);
    let chunks = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunks).unwrap();
    let runtime = WriterProjectionRuntime::new(usize::MAX);
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    let mut progress = input.new_progress(&plan, &runtime, 7).unwrap();
    let response = |request: &ModelRequest, count: usize| {
        let prompt: Value = serde_json::from_str(&request.user_prompt).unwrap();
        ModelResponse{text:json!({"units":(0..count).map(|n|json!({"text":format!("Inspection proposition {} number {n}.",request.ordinal),"source_ids":[prompt["sources"][0][0]]})).collect::<Vec<_>>()}).to_string(),runtime_id:runtime.runtime_id().into(),model_id:runtime.model_id().into(),request_attempts:vec![]}
    };
    let request = input
        .dispatch_section(&mut progress, 0, &runtime, 7)
        .unwrap();
    input
        .record_section(&mut progress, 0, &response(&request, 64))
        .unwrap();
    input
        .split_saturated(&mut progress, 0, &runtime, 7)
        .unwrap();
    let left = input
        .dispatch_section(&mut progress, 1, &runtime, 7)
        .unwrap();
    input
        .record_section(&mut progress, 1, &response(&left, 32))
        .unwrap();
    let right = input
        .dispatch_section(&mut progress, 2, &runtime, 7)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&right.user_prompt).unwrap()["maximum_units"],
        32
    );
    for count in [31, 32] {
        let mut candidate = progress.clone();
        input
            .record_section(&mut candidate, 2, &response(&right, count))
            .unwrap();
        if count == 31 {
            assert_eq!(
                input.finish_progress(&mut candidate).unwrap().claims.len(),
                63
            );
        } else {
            assert_eq!(
                input
                    .split_saturated(&mut candidate, 2, &runtime, 7)
                    .unwrap_err()
                    .code,
                "WHOLE_DOCUMENT_CLAIM_CAPACITY_EXCEEDED"
            );
            assert!(input.finish_progress(&mut candidate).is_err());
        }
    }
}

#[test]
fn review132_persisted_saturation_resumes_split_without_parent_generation() {
    use crate::pipeline::contracts::{PipelineState, WholeDocumentSectionState};
    let mut runtime = WholeWriterStageRuntime::new();
    runtime.admission = WriterProjectionRuntime::new(usize::MAX);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("crash-after-saturation.sqlite");
    let (mut conn, run, normalized, chunked) = whole_writer_stage_fixture(&path, &runtime);
    let analyzed = db::get_analyzed_document(&conn, &run).unwrap().unwrap();
    let seed = generation_seed_for_run(&run);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let plan = input.plan(&runtime, seed, &UNCONTROLLED_EXECUTION).unwrap();
    let mut pending =
        partial_progress_envelope(input.new_progress(&plan, &runtime, seed).unwrap(), &chunked);
    pending.claims = direct::source_ordered_claims(&analyzed).unwrap();
    pending.warnings = analyzed.warnings.clone();
    let version = db::get_pipeline_run(&conn, &run)
        .unwrap()
        .unwrap()
        .state_version;
    conn.execute(
        "UPDATE pipeline_runs SET state=?1 WHERE run_id=?2",
        rusqlite::params![
            serde_json::to_string(&PipelineState::Synthesizing).unwrap(),
            run
        ],
    )
    .unwrap();
    db::save_whole_synthesis_progress(&mut conn, &run, version, None, &pending).unwrap();
    let mut dispatched = pending.clone();
    let request = input
        .dispatch_section(
            dispatched.whole_document_progress.as_mut().unwrap(),
            0,
            &runtime,
            seed,
        )
        .unwrap();
    db::save_whole_synthesis_progress(&mut conn, &run, version, Some(&pending), &dispatched)
        .unwrap();
    let prompt: Value = serde_json::from_str(&request.user_prompt).unwrap();
    let text=json!({"units":(0..prompt["maximum_units"].as_u64().unwrap()).map(|n|json!({"text":format!("Persisted proposition {n}."),"source_ids":[prompt["sources"][0][0]]})).collect::<Vec<_>>()}).to_string();
    let mut saturated = dispatched.clone();
    input
        .record_section(
            saturated.whole_document_progress.as_mut().unwrap(),
            0,
            &ModelResponse {
                text,
                runtime_id: runtime.runtime_id().into(),
                model_id: runtime.model_id().into(),
                request_attempts: vec![],
            },
        )
        .unwrap();
    db::save_whole_synthesis_progress(&mut conn, &run, version, Some(&dispatched), &saturated)
        .unwrap();
    drop(conn);
    let mut reopened = db::init_db(&path).unwrap();
    let result = synthesize_analyzed_document_for_policy(
        &mut reopened,
        &runtime,
        &run,
        &UNCONTROLLED_EXECUTION,
        None,
        WHOLE_DOCUMENT_VERSION,
    );
    assert!(
        result.is_ok(),
        "a persisted saturated response owns no outstanding inference: {result:?}"
    );
    let draft = result.unwrap();
    assert_eq!(draft.summary_claims.len(), 4);
    assert!(matches!(
        draft.whole_document_progress.as_ref().unwrap().sections[0].state,
        WholeDocumentSectionState::Split { .. }
    ));
    assert_eq!(
        runtime
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.ordinal)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    input.validate_saved_synthesis(&draft).unwrap();
}

#[test]
fn baseline_repair_saturated_parent_splits_durably_without_redo_or_lost_sources() {
    use crate::pipeline::contracts::WholeDocumentSectionState;
    let mut runtime = WholeWriterStageRuntime::new();
    runtime.admission = WriterProjectionRuntime::new(usize::MAX);
    runtime.saturate_first.store(true, Ordering::SeqCst);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("split.sqlite");
    let (mut conn, run, normalized, chunked) = whole_writer_stage_fixture(&path, &runtime);
    synthesize_analyzed_document_for_policy(
        &mut conn,
        &runtime,
        &run,
        &UNCONTROLLED_EXECUTION,
        None,
        WHOLE_DOCUMENT_VERSION,
    )
    .unwrap();
    let draft = db::get_synthesized_document(&conn, &run).unwrap().unwrap();
    let progress = draft.whole_document_progress.as_ref().unwrap();
    assert_eq!(progress.sections.len(), 3);
    let WholeDocumentSectionState::Split { response } = &progress.sections[0].state else {
        panic!("parent must remain sealed history")
    };
    assert_eq!(
        format!("{:x}", Sha256::digest(response.text.as_bytes())),
        response.response_sha256
    );
    assert_eq!(
        serde_json::from_str::<Value>(&response.text).unwrap()["units"]
            .as_array()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(draft.summary_claims.len(), 4);
    assert_eq!(progress.assembly_lineage.as_ref().unwrap().len(), 4);
    assert!(progress
        .assembly_lineage
        .as_ref()
        .unwrap()
        .iter()
        .all(|(section, _, _)| *section > 0));
    assert_eq!(
        runtime
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(
        runtime
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| {
                let ModelOutputFormat::JsonSchema { schema, .. } = &r.output_format else {
                    panic!("schema")
                };
                schema["properties"]["units"]["maxItems"].as_u64().unwrap()
            })
            .collect::<Vec<_>>(),
        vec![64, 64, 62],
        "only accepted distinct claims consume the final document allowance"
    );
    assert!(progress.sections[1..]
        .iter()
        .all(|s| s.primary.len() < progress.sections[0].primary.len()));
    drop(conn);
    let reopened = db::init_db(&path).unwrap();
    let restored = db::get_synthesized_document(&reopened, &run)
        .unwrap()
        .unwrap();
    assert_eq!(restored, draft);
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    input.validate_saved_synthesis(&restored).unwrap();
    for change in 0..3 {
        let mut forged = restored.clone();
        let p = forged.whole_document_progress.as_mut().unwrap();
        match change {
            0 => {
                p.sections[0].state = WholeDocumentSectionState::Completed {
                    response: response.clone(),
                }
            }
            1 => p.sections[1].primary.clear(),
            _ => p.sections[0].state = WholeDocumentSectionState::Pending,
        }
        assert!(input.validate_saved_synthesis(&forged).is_err());
    }
}

#[test]
fn baseline_repair_split_transition_is_atomic_and_cannot_reset_parent() {
    use crate::pipeline::contracts::{PipelineState, WholeDocumentSectionState};
    use crate::pipeline::ingest::prepare_pdf_ingestion;
    let normalized = public_document(&[
        "Section 1 Scope\nThe operator inspects equipment.".into(),
        "Section 2 Records\nThe operator retains records.".into(),
    ]);
    let chunks = whole_writer_chunks(&normalized);
    let input = whole_document::SourceInput::new(&normalized, &chunks).unwrap();
    let runtime = WriterProjectionRuntime::new(usize::MAX);
    let plan = input.plan(&runtime, 7, &UNCONTROLLED_EXECUTION).unwrap();
    let pending =
        partial_progress_envelope(input.new_progress(&plan, &runtime, 7).unwrap(), &chunks);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("split-cas.sqlite");
    let mut conn = db::init_db(&path).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
    let (mut document, mut run) = prepare_pdf_ingestion(fixture.to_str().unwrap(), None).unwrap();
    document.document_id = normalized.document_id.clone();
    run.document_id = normalized.document_id.clone();
    let ingested = db::persist_ingestion(&mut conn, &document, &run).unwrap();
    conn.execute(
        "UPDATE pipeline_runs SET state=?1 WHERE run_id=?2",
        rusqlite::params![
            serde_json::to_string(&PipelineState::Synthesizing).unwrap(),
            run.run_id
        ],
    )
    .unwrap();
    db::save_whole_synthesis_progress(
        &mut conn,
        &run.run_id,
        ingested.state_version,
        None,
        &pending,
    )
    .unwrap();
    let mut dispatched = pending.clone();
    input
        .dispatch_section(
            dispatched.whole_document_progress.as_mut().unwrap(),
            0,
            &runtime,
            7,
        )
        .unwrap();
    db::save_whole_synthesis_progress(
        &mut conn,
        &run.run_id,
        ingested.state_version,
        Some(&pending),
        &dispatched,
    )
    .unwrap();
    let text=json!({"units":(0..64).map(|n|json!({"text":format!("Authored saturation item {n}."),"source_ids":["s1"]})).collect::<Vec<_>>()}).to_string();
    let mut saturated = dispatched.clone();
    input
        .record_section(
            saturated.whole_document_progress.as_mut().unwrap(),
            0,
            &ModelResponse {
                text,
                runtime_id: runtime.runtime_id().into(),
                model_id: runtime.model_id().into(),
                request_attempts: vec![],
            },
        )
        .unwrap();
    db::save_whole_synthesis_progress(
        &mut conn,
        &run.run_id,
        ingested.state_version,
        Some(&dispatched),
        &saturated,
    )
    .unwrap();
    let mut split = saturated.clone();
    input
        .split_saturated(
            split.whole_document_progress.as_mut().unwrap(),
            0,
            &runtime,
            7,
        )
        .unwrap();
    for change in 0..3 {
        let mut forged = split.clone();
        let progress = forged.whole_document_progress.as_mut().unwrap();
        match change {
            0 => progress.sections[1].primary.clear(),
            1 => progress.sections[0].state = WholeDocumentSectionState::Pending,
            _ => progress.sections[0].request.seed += 1,
        }
        assert!(db::save_whole_synthesis_progress(
            &mut conn,
            &run.run_id,
            ingested.state_version,
            Some(&saturated),
            &forged
        )
        .is_err());
    }
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers = (0..2)
        .map(|_| {
            let (path, run_id, old, next, barrier, version) = (
                path.clone(),
                run.run_id.clone(),
                saturated.clone(),
                split.clone(),
                barrier.clone(),
                ingested.state_version,
            );
            std::thread::spawn(move || {
                let mut conn = db::init_db(&path).unwrap();
                barrier.wait();
                match db::save_whole_synthesis_progress(
                    &mut conn,
                    &run_id,
                    version,
                    Some(&old),
                    &next,
                ) {
                    Ok(()) => true,
                    Err(StoreError::WholeDocumentProgressConflict { .. }) => false,
                    Err(error) => panic!("unexpected split error: {error}"),
                }
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        workers
            .into_iter()
            .filter_map(|w| w.join().unwrap().then_some(()))
            .count(),
        1
    );
    let stored = db::get_whole_synthesis_progress(&conn, &run.run_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored, split);
    let mut progress = stored.whole_document_progress.unwrap();
    input.restore_progress(&progress).unwrap();
    assert_eq!(
        input
            .dispatch_section(&mut progress, 0, &runtime, 7)
            .unwrap_err()
            .code,
        "WHOLE_DOCUMENT_SECTION_ALREADY_DISPATCHED"
    );
    assert!(db::get_synthesized_document(&conn, &run.run_id)
        .unwrap()
        .is_none());
}

#[derive(Clone, Copy)]
enum ResumeFixture {
    Completed,
    SplitCompleted,
    Pending,
    Mixed,
    Saturated,
}

// Every backend method is counted, including Verify requests made by Synthesize.
struct ResumeTripwire<'a> {
    setup: &'a WholeWriterStageRuntime,
    allow_synthesis: bool,
    allow_health: bool,
    allow_generation: bool,
    comparison_only: bool,
    preflights: std::sync::Mutex<Vec<ModelRequest>>,
    health_calls: AtomicUsize,
    generation_calls: AtomicUsize,
}
impl<'a> ResumeTripwire<'a> {
    fn new(setup: &'a WholeWriterStageRuntime) -> Self {
        Self {
            setup,
            allow_synthesis: false,
            allow_health: false,
            allow_generation: false,
            comparison_only: false,
            preflights: std::sync::Mutex::new(vec![]),
            health_calls: AtomicUsize::new(0),
            generation_calls: AtomicUsize::new(0),
        }
    }
    fn unavailable() -> ModelRuntimeFailure {
        ModelRuntimeFailure {
            code: "PUBLIC_BACKEND_OFFLINE".into(),
            message: "resume backend tripwire".into(),
            recoverable: true,
            request_attempts: vec![],
        }
    }
}
impl ModelRuntime for ResumeTripwire<'_> {
    fn runtime_id(&self) -> &str {
        self.setup.runtime_id()
    }
    fn model_id(&self) -> &str {
        self.setup.model_id()
    }
    fn context_tokens(&self, stage: PipelineStage) -> u32 {
        self.setup.context_tokens(stage)
    }
    fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        self.preflights.lock().unwrap().push(request.clone());
        if self.allow_synthesis && request.stage == PipelineStage::Synthesize
            || self.comparison_only
                && !matches!(&request.output_format,
                ModelOutputFormat::JsonSchema { name, .. } if name == comparisons::SCHEMA_NAME)
        {
            return self.setup.preflight_request(request);
        }
        Err(Self::unavailable())
    }
    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        self.health_calls.fetch_add(1, Ordering::SeqCst);
        if self.allow_health {
            Ok(())
        } else {
            Err(Self::unavailable())
        }
    }
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        self.generation_calls.fetch_add(1, Ordering::SeqCst);
        if self.allow_generation {
            self.setup.generate(request)
        } else {
            Err(Self::unavailable())
        }
    }
}

fn persist_resume_response(
    conn: &mut Connection,
    input: &whole_document::SourceInput<'_>,
    runtime: &WholeWriterStageRuntime,
    run: (&str, u32),
    draft: &mut SynthesizedDocument,
    index: usize,
    saturated: bool,
) {
    let seed = generation_seed_for_run(run.0);
    let previous = draft.clone();
    let request = input
        .dispatch_section(
            draft.whole_document_progress.as_mut().unwrap(),
            index,
            runtime,
            seed,
        )
        .unwrap();
    db::save_whole_synthesis_progress(conn, run.0, run.1, Some(&previous), draft).unwrap();
    let dispatched = draft.clone();
    let plan = input
        .restore_progress(draft.whole_document_progress.as_ref().unwrap())
        .unwrap();
    let position = plan
        .drafts
        .iter()
        .position(|d| d.request.ordinal as usize == index)
        .unwrap();
    let text = if saturated {
        let prompt: Value = serde_json::from_str(&request.user_prompt).unwrap();
        json!({"units":(0..prompt["maximum_units"].as_u64().unwrap()).map(|n|
            json!({"text":format!("Saved saturated proposition {n}."),"source_ids":[prompt["sources"][0][0]]})
        ).collect::<Vec<_>>()}).to_string()
    } else {
        inspection_control_responses(input, &plan)[position].clone()
    };
    input
        .record_section(
            draft.whole_document_progress.as_mut().unwrap(),
            index,
            &ModelResponse {
                text,
                runtime_id: runtime.runtime_id().into(),
                model_id: runtime.model_id().into(),
                request_attempts: vec![],
            },
        )
        .unwrap();
    db::save_whole_synthesis_progress(conn, run.0, run.1, Some(&dispatched), draft).unwrap();
}

fn persist_resume_fixture(
    path: &Path,
    runtime: &mut WholeWriterStageRuntime,
    kind: ResumeFixture,
) -> (
    String,
    NormalizedDocument,
    ChunkedDocument,
    SynthesizedDocument,
) {
    use crate::pipeline::contracts::WholeDocumentSectionState;
    if !matches!(kind, ResumeFixture::Pending | ResumeFixture::Mixed) {
        runtime.admission = WriterProjectionRuntime::new(usize::MAX);
    }
    let (mut conn, run, normalized, chunked) = whole_writer_stage_fixture(path, runtime);
    let version = db::get_pipeline_run(&conn, &run)
        .unwrap()
        .unwrap()
        .state_version;
    let (started, analyzed) = db::start_synthesis(&mut conn, &run, version).unwrap();
    let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
    let seed = generation_seed_for_run(&run);
    let plan = input.plan(runtime, seed, &UNCONTROLLED_EXECUTION).unwrap();
    let mut draft =
        partial_progress_envelope(input.new_progress(&plan, runtime, seed).unwrap(), &chunked);
    draft.claims = direct::source_ordered_claims(&analyzed).unwrap();
    draft.warnings = analyzed.warnings.clone();
    db::save_whole_synthesis_progress(&mut conn, &run, started.state_version, None, &draft)
        .unwrap();
    if matches!(
        kind,
        ResumeFixture::SplitCompleted | ResumeFixture::Saturated
    ) {
        assert_eq!(plan.drafts.len(), 1);
        persist_resume_response(
            &mut conn,
            &input,
            runtime,
            (&run, started.state_version),
            &mut draft,
            0,
            true,
        );
        if matches!(kind, ResumeFixture::SplitCompleted) {
            let saturated = draft.clone();
            input
                .split_saturated(
                    draft.whole_document_progress.as_mut().unwrap(),
                    0,
                    runtime,
                    seed,
                )
                .unwrap();
            db::save_whole_synthesis_progress(
                &mut conn,
                &run,
                started.state_version,
                Some(&saturated),
                &draft,
            )
            .unwrap();
        }
    }
    if matches!(
        kind,
        ResumeFixture::Completed | ResumeFixture::SplitCompleted | ResumeFixture::Mixed
    ) {
        if matches!(kind, ResumeFixture::Mixed) {
            assert!(plan.drafts.len() > 1);
        }
        for index in 0..draft
            .whole_document_progress
            .as_ref()
            .unwrap()
            .sections
            .len()
        {
            if matches!(
                draft.whole_document_progress.as_ref().unwrap().sections[index].state,
                WholeDocumentSectionState::Pending
            ) {
                persist_resume_response(
                    &mut conn,
                    &input,
                    runtime,
                    (&run, started.state_version),
                    &mut draft,
                    index,
                    false,
                );
                if matches!(kind, ResumeFixture::Mixed) {
                    break;
                }
            }
        }
    }
    assert_eq!(
        db::get_whole_synthesis_progress(&conn, &run)
            .unwrap()
            .unwrap(),
        draft
    );
    (run, normalized, chunked, draft)
}

fn assert_completed_resume_has_no_backend_calls(allow_synthesis: bool) {
    use crate::pipeline::contracts::PipelineState;
    for kind in [ResumeFixture::Completed, ResumeFixture::SplitCompleted] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("saved-resume.sqlite");
        let mut setup = WholeWriterStageRuntime::new();
        let (run, normalized, chunked, saved) = persist_resume_fixture(&path, &mut setup, kind);
        let input = whole_document::SourceInput::new(&normalized, &chunked).unwrap();
        let mut expected = saved.clone();
        input.finalize_progress(&mut expected).unwrap();
        let mut runtime = ResumeTripwire::new(&setup);
        runtime.allow_synthesis = allow_synthesis;
        let mut reopened = db::init_db(&path).unwrap();
        let result = synthesize_analyzed_document_for_policy(
            &mut reopened,
            &runtime,
            &run,
            &UNCONTROLLED_EXECUTION,
            None,
            WHOLE_DOCUMENT_VERSION,
        );
        if let Err(error) = &result {
            assert_eq!(
                db::get_whole_synthesis_progress(&reopened, &run)
                    .unwrap()
                    .unwrap(),
                saved,
                "failure must not delete or modify saved response bytes"
            );
            println!("RESUME_REPRO failure={error:?}; stages={:?}; state={:?}; saved_bytes_preserved=true",
                runtime.preflights.lock().unwrap().iter().map(|r| &r.stage).collect::<Vec<_>>(),
                db::get_pipeline_run(&reopened, &run).unwrap().unwrap().state);
        }
        assert!(
            result.is_ok(),
            "saved responses must assemble without a backend: {result:?}"
        );
        assert_eq!(result.unwrap(), expected);
        assert_eq!(
            db::get_synthesized_document(&reopened, &run)
                .unwrap()
                .unwrap(),
            expected
        );
        assert_eq!(
            db::get_pipeline_run(&reopened, &run)
                .unwrap()
                .unwrap()
                .state,
            PipelineState::Synthesized
        );
        assert!(db::get_verified_document(&reopened, &run)
            .unwrap()
            .is_none());
        assert!(db::get_summary_artifact(&reopened, &run).unwrap().is_none());
        assert!(runtime.preflights.lock().unwrap().is_empty());
        assert_eq!(runtime.health_calls.load(Ordering::SeqCst), 0);
        assert_eq!(runtime.generation_calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn completed_resume_never_preflights_historical_sections() {
    assert_completed_resume_has_no_backend_calls(false);
}

#[test]
fn completed_resume_never_preflights_future_verify_requests() {
    assert_completed_resume_has_no_backend_calls(true);
}

#[test]
fn completed_resume_pending_and_split_children_still_require_admission() {
    use crate::pipeline::contracts::WholeDocumentSectionState;
    for kind in [
        ResumeFixture::Pending,
        ResumeFixture::Mixed,
        ResumeFixture::Saturated,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("pending-resume.sqlite");
        let mut setup = WholeWriterStageRuntime::new();
        let (run, _, _, saved) = persist_resume_fixture(&path, &mut setup, kind);
        let mut runtime = ResumeTripwire::new(&setup);
        runtime.allow_health = true;
        let mut conn = db::init_db(&path).unwrap();
        let error = synthesize_analyzed_document_for_policy(
            &mut conn,
            &runtime,
            &run,
            &UNCONTROLLED_EXECUTION,
            None,
            WHOLE_DOCUMENT_VERSION,
        )
        .unwrap_err();
        let SummaryPipelineError::StageFailed(failure) = error else {
            panic!("wrong failure: {error:?}")
        };
        assert_eq!(failure.code, "PUBLIC_BACKEND_OFFLINE");
        assert_eq!(runtime.generation_calls.load(Ordering::SeqCst), 0);
        let requests = runtime.preflights.lock().unwrap();
        assert!(!requests.is_empty());
        for request in requests.iter() {
            assert_eq!(request.stage, PipelineStage::Synthesize);
            assert!(
                !saved
                    .whole_document_progress
                    .as_ref()
                    .unwrap()
                    .sections
                    .iter()
                    .any(|s| !matches!(s.state, WholeDocumentSectionState::Pending)
                        && s.request == *request),
                "historical request was preflighted"
            );
        }
        assert_eq!(
            db::get_whole_synthesis_progress(&conn, &run)
                .unwrap()
                .unwrap(),
            saved
        );
        if matches!(kind, ResumeFixture::Saturated) {
            assert_eq!(runtime.health_calls.load(Ordering::SeqCst), 1);
        }
    }
}

#[test]
fn completed_resume_verify_admission_still_blocks_delivery_before_generation() {
    use crate::pipeline::contracts::PipelineState;
    for comparison_only in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("verify-resume.sqlite");
        let mut setup = WholeWriterStageRuntime::new();
        let (run, _, _, _) = persist_resume_fixture(&path, &mut setup, ResumeFixture::Completed);
        let mut conn = db::init_db(&path).unwrap();
        let draft = synthesize_analyzed_document_for_policy(
            &mut conn,
            &setup,
            &run,
            &UNCONTROLLED_EXECUTION,
            None,
            WHOLE_DOCUMENT_VERSION,
        )
        .unwrap();
        let mut runtime = ResumeTripwire::new(&setup);
        runtime.comparison_only = comparison_only;
        let error = verify_synthesized_document(&mut conn, &runtime, &run).unwrap_err();
        let SummaryPipelineError::StageFailed(failure) = error else {
            panic!("wrong failure: {error:?}")
        };
        assert_eq!(failure.code, "PUBLIC_BACKEND_OFFLINE");
        assert_eq!(failure.stage, Some(PipelineStage::Verify));
        assert!(failure.message.starts_with("MODEL_VERIFICATION_ADMISSION:"));
        let requests = runtime.preflights.lock().unwrap();
        let rejected = requests.last().expect("Verify must preflight");
        let ModelOutputFormat::JsonSchema { name, .. } = &rejected.output_format else {
            panic!("missing schema")
        };
        assert_eq!(
            name == comparisons::SCHEMA_NAME,
            comparison_only,
            "ledger and comparison admission must each precede generation"
        );
        assert_eq!(runtime.generation_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            db::get_synthesized_document(&conn, &run).unwrap().unwrap(),
            draft
        );
        assert!(db::get_verified_document(&conn, &run).unwrap().is_none());
        assert!(db::get_summary_artifact(&conn, &run).unwrap().is_none());
        assert_eq!(
            db::get_pipeline_run(&conn, &run).unwrap().unwrap().state,
            PipelineState::Failed
        );
    }
}

#[test]
fn completed_resume_pending_work_still_requires_backend_health() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pending-health.sqlite");
    let mut setup = WholeWriterStageRuntime::new();
    let (run, _, _, saved) = persist_resume_fixture(&path, &mut setup, ResumeFixture::Pending);
    let mut runtime = ResumeTripwire::new(&setup);
    runtime.allow_synthesis = true;
    let mut conn = db::init_db(&path).unwrap();
    let error = synthesize_analyzed_document_for_policy(
        &mut conn,
        &runtime,
        &run,
        &UNCONTROLLED_EXECUTION,
        None,
        WHOLE_DOCUMENT_VERSION,
    )
    .unwrap_err();
    let SummaryPipelineError::StageFailed(failure) = error else {
        panic!("wrong failure: {error:?}")
    };
    assert_eq!(failure.code, "PUBLIC_BACKEND_OFFLINE");
    assert!(failure.message.starts_with("MODEL_HEALTH:"));
    assert!(!runtime.preflights.lock().unwrap().is_empty());
    assert_eq!(runtime.health_calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.generation_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        db::get_whole_synthesis_progress(&conn, &run)
            .unwrap()
            .unwrap(),
        saved
    );
}

#[test]
fn completed_resume_fresh_synthesis_also_leaves_verify_admission_to_verify() {
    use crate::pipeline::contracts::PipelineState;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("fresh-boundary.sqlite");
    let setup = WholeWriterStageRuntime::new();
    let (mut conn, run, _, _) = whole_writer_stage_fixture(&path, &setup);
    let mut runtime = ResumeTripwire::new(&setup);
    runtime.allow_synthesis = true;
    runtime.allow_health = true;
    runtime.allow_generation = true;
    let draft = synthesize_analyzed_document_for_policy(
        &mut conn,
        &runtime,
        &run,
        &UNCONTROLLED_EXECUTION,
        None,
        WHOLE_DOCUMENT_VERSION,
    )
    .unwrap();
    assert!(runtime
        .preflights
        .lock()
        .unwrap()
        .iter()
        .all(|r| r.stage == PipelineStage::Synthesize));
    assert_eq!(
        db::get_pipeline_run(&conn, &run).unwrap().unwrap().state,
        PipelineState::Synthesized
    );
    let generated = runtime.generation_calls.load(Ordering::SeqCst);
    assert!(generated > 0);
    runtime.allow_generation = false;
    assert!(verify_synthesized_document(&mut conn, &runtime, &run).is_err());
    assert_eq!(runtime.generation_calls.load(Ordering::SeqCst), generated);
    assert_eq!(
        db::get_synthesized_document(&conn, &run).unwrap().unwrap(),
        draft
    );
    assert!(db::get_verified_document(&conn, &run).unwrap().is_none());
    assert!(db::get_summary_artifact(&conn, &run).unwrap().is_none());
}
