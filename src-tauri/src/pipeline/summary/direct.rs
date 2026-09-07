//! Default summary: preserve quote-bound paraphrases, then verify once.
use super::*;

pub(super) const VERSION: &str = DIRECT_SYNTHESIS_VERSION;
pub(super) const MAX_CLAIMS: usize = 512;

pub(super) fn retention_target(pages: usize) -> Result<usize, PipelineFailure> {
    let acceptance = analysis_scope_minimum(pages)?;
    if pages == 0 || acceptance > MAX_CLAIMS {
        return Err(stage_failure(
            PipelineStage::Analyze,
            "ANALYSIS_COVERAGE_CAPACITY_UNSATISFIABLE",
            "Raw page acceptance exceeds the one-to-one claim capacity",
            false,
        ));
    }
    Ok(pages.min(acceptance.saturating_add(16)).min(MAX_CLAIMS))
}

pub(super) fn source_ordered_claims(
    analyzed: &AnalyzedDocument,
) -> Result<Vec<CitedClaim>, PipelineFailure> {
    let mut evidence = analyzed
        .chunks
        .iter()
        .flat_map(|c| &c.evidence)
        .collect::<Vec<_>>();
    if evidence.is_empty() || evidence.len() > MAX_CLAIMS {
        return Err(stage_failure(
            PipelineStage::Synthesize,
            "DIRECT_EVIDENCE_CAPACITY_INVALID",
            "Direct synthesis requires between one and 512 retained evidence items",
            false,
        ));
    }
    evidence.sort_by_key(|e| e.source_span.page_start);
    materialize_cited_claims(
        &analyzed.document_id,
        VERSION,
        evidence
            .into_iter()
            .map(|e| ValidatedClaim {
                text: e.claim_text.clone(),
                evidence_ids: vec![e.evidence_id.clone()],
            })
            .collect(),
    )
}

pub(super) fn validate_claim_set(
    synthesized: &SynthesizedDocument,
    analyzed: &AnalyzedDocument,
) -> Result<(), PipelineFailure> {
    if synthesized.claims != source_ordered_claims(analyzed)? {
        return Err(stage_failure(
            PipelineStage::Synthesize,
            "INVALID_SYNTHESIZED_DOCUMENT",
            "Direct synthesis must preserve each paraphrase and its single source in page order",
            false,
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn validate_claim_set_for_runtime(
    synthesized: &SynthesizedDocument,
    analyzed: &AnalyzedDocument,
    runtime: &dyn ModelRuntime,
) -> Result<(), PipelineFailure> {
    validate_claim_set(synthesized, analyzed)?;
    let evidence = analyzed
        .chunks
        .iter()
        .flat_map(|c| &c.evidence)
        .map(|e| PromptEvidenceItem {
            evidence_id: e.evidence_id.clone(),
            claim_text: e.claim_text.clone(),
            exact_quote: e.exact_quote.clone(),
        })
        .collect::<Vec<_>>();
    ensure_claim_catalog_is_verifiable_for_context(
        &synthesized.claims,
        &evidence,
        MAX_CLAIMS,
        runtime.context_tokens(PipelineStage::Verify),
    )
}

#[cfg(test)]
pub(super) fn synthesize(
    runtime: &dyn ModelRuntime,
    analyzed: &AnalyzedDocument,
    chunked: &ChunkedDocument,
    normalized: &NormalizedDocument,
    control: &dyn ExecutionControl,
) -> Result<SynthesizedDocument, PipelineFailure> {
    cancellation_checkpoint(control, PipelineStage::Synthesize)?;
    validate_analyzed_document(analyzed, chunked, normalized, runtime)?;
    let claims = source_ordered_claims(analyzed)?;
    let result = SynthesizedDocument {
        document_id: analyzed.document_id.clone(),
        synthesis_version: VERSION.into(),
        runtime_id: runtime.runtime_id_for_stage(PipelineStage::Analyze).into(),
        model_id: runtime.model_id_for_stage(PipelineStage::Analyze).into(),
        presentation_mode: SummaryPresentationMode::LegacyClaimList,
        summary_text: render_cited_summary(&claims, analyzed)?,
        source_chunk_ids: analyzed.chunks.iter().map(|c| c.chunk_id.clone()).collect(),
        summary_claims: Vec::new(),
        synthesis_evidence: Vec::new(),
        claims,
        warnings: analyzed.warnings.clone(),
    };
    validate_synthesized_document(&result, analyzed, chunked, normalized, runtime)?;
    cancellation_checkpoint(control, PipelineStage::Synthesize)?;
    Ok(result)
}
