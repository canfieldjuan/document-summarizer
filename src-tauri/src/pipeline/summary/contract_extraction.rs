//! Contract mode copies typed, source-bound clause records without prose generation.
use super::*;
use crate::pipeline::contracts::{ContractExtraction, ExtractedContractClause};

pub(super) const VERSION: &str = "contract-extraction-1.0.0";
const RUNTIME: &str = "deterministic-source-extraction";

fn invalid() -> PipelineFailure {
    stage_failure(
        PipelineStage::Verify,
        "INVALID_CONTRACT_EXTRACTION",
        "Contract records must exactly match complete source clauses and provenance",
        false,
    )
}

fn records(
    chunked: &ChunkedDocument,
    normalized: &NormalizedDocument,
) -> Result<(ContractExtraction, Vec<EvidenceItem>), PipelineFailure> {
    let blocks = validate_normalized_chunk_boundary(normalized, chunked)?;
    let mut extraction = ContractExtraction {
        clauses: Vec::new(),
    };
    let mut evidence = Vec::new();
    let ordered = normalized
        .pages
        .iter()
        .flat_map(|p| &p.content)
        .collect::<Vec<_>>();
    for (ordinal, clause) in clauses::build_ordered(&ordered).iter().enumerate() {
        let mut evidence_ids = Vec::new();
        for fragment in &clause.fragments {
            let block = blocks[fragment.block_id.as_str()];
            let chunk = chunked
                .chunks
                .iter()
                .find(|c| c.block_ids.contains(&fragment.block_id))
                .ok_or_else(invalid)?;
            let exact_quote = block.text[fragment.start..fragment.end].to_string();
            let evidence_id = deterministic_id(
                "contract-source",
                &[
                    VERSION,
                    &normalized.document_id,
                    &chunk.chunk_id,
                    &fragment.block_id,
                    &fragment.start.to_string(),
                    &fragment.end.to_string(),
                    &exact_quote,
                ],
            );
            evidence_ids.push(evidence_id.clone());
            evidence.push(EvidenceItem {
                evidence_id,
                chunk_id: chunk.chunk_id.clone(),
                block_id: fragment.block_id.clone(),
                claim_text: exact_quote.clone(),
                exact_quote,
                source_span: block.source.clone(),
            });
        }
        let text = clauses::text(clause, &blocks);
        let clause_id = deterministic_id(
            "contract-clause",
            &[
                VERSION,
                &normalized.document_id,
                &ordinal.to_string(),
                &text,
            ],
        );
        extraction.clauses.push(ExtractedContractClause {
            clause_id,
            heading: clause.heading.clone(),
            text,
            evidence_ids,
        });
    }
    if extraction.clauses.is_empty() {
        return Err(invalid());
    }
    Ok((extraction, evidence))
}

pub(super) fn analyze(
    chunked: &ChunkedDocument,
    normalized: &NormalizedDocument,
    control: &dyn ExecutionControl,
) -> Result<AnalyzedDocument, PipelineFailure> {
    cancellation_checkpoint(control, PipelineStage::Analyze)?;
    let (_, evidence) = records(chunked, normalized)?;
    let mut warnings = normalized.warnings.clone();
    warnings.push(PipelineWarning { code: "CONTRACT_SOURCE_EXTRACTION".into(),
        message: "Contract clauses are extracted verbatim with source citations. They are not a legal interpretation; OCR-derived text still requires source review.".into(), stage: Some(PipelineStage::Analyze) });
    let chunks = chunked
        .chunks
        .iter()
        .map(|chunk| {
            let evidence = evidence
                .iter()
                .filter(|e| e.chunk_id == chunk.chunk_id)
                .cloned()
                .collect::<Vec<_>>();
            ChunkAnalysis {
                chunk_id: chunk.chunk_id.clone(),
                summary_text: evidence
                    .iter()
                    .map(|e| e.exact_quote.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                source_spans: chunk.source_spans.clone(),
                evidence,
            }
        })
        .collect();
    Ok(AnalyzedDocument {
        document_id: normalized.document_id.clone(),
        analysis_version: VERSION.into(),
        runtime_id: RUNTIME.into(),
        model_id: "none".into(),
        chunks,
        warnings,
        omissions: Vec::new(),
        inspected_pages: normalized
            .pages
            .iter()
            .filter(|p| p.content.iter().any(|b| !b.text.trim().is_empty()))
            .map(|p| p.page_number)
            .collect(),
    })
}

pub(super) fn validate_analysis(
    analyzed: &AnalyzedDocument,
    chunked: &ChunkedDocument,
    normalized: &NormalizedDocument,
) -> Result<(), PipelineFailure> {
    if *analyzed != analyze(chunked, normalized, &UNCONTROLLED_EXECUTION)? {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn synthesize(
    analyzed: &AnalyzedDocument,
    chunked: &ChunkedDocument,
    normalized: &NormalizedDocument,
    control: &dyn ExecutionControl,
) -> Result<SynthesizedDocument, PipelineFailure> {
    cancellation_checkpoint(control, PipelineStage::Synthesize)?;
    validate_analysis(analyzed, chunked, normalized)?;
    let (extraction, _) = records(chunked, normalized)?;
    let claims = extraction
        .clauses
        .iter()
        .map(|c| CitedClaim {
            claim_id: c.clause_id.clone(),
            text: c.text.clone(),
            evidence_ids: c.evidence_ids.clone(),
        })
        .collect::<Vec<_>>();
    let summary_text = render_cited_summary(&claims, analyzed)?;
    Ok(SynthesizedDocument {
        contract_extraction: Some(extraction),
        document_id: analyzed.document_id.clone(),
        synthesis_version: VERSION.into(),
        runtime_id: RUNTIME.into(),
        model_id: "none".into(),
        presentation_mode: SummaryPresentationMode::StructuredExtraction,
        summary_text,
        source_chunk_ids: chunked.chunks.iter().map(|c| c.chunk_id.clone()).collect(),
        summary_claims: Vec::new(),
        synthesis_evidence: Vec::new(),
        claims,
        warnings: analyzed.warnings.clone(),
    })
}

pub(super) fn validate_synthesis(
    synthesized: &SynthesizedDocument,
    analyzed: &AnalyzedDocument,
    chunked: &ChunkedDocument,
    normalized: &NormalizedDocument,
) -> Result<(), PipelineFailure> {
    if *synthesized != synthesize(analyzed, chunked, normalized, &UNCONTROLLED_EXECUTION)? {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn verify(
    synthesized: &SynthesizedDocument,
    analyzed: &AnalyzedDocument,
    chunked: &ChunkedDocument,
    normalized: &NormalizedDocument,
    control: &dyn ExecutionControl,
) -> Result<VerifiedDocument, PipelineFailure> {
    cancellation_checkpoint(control, PipelineStage::Verify)?;
    validate_synthesis(synthesized, analyzed, chunked, normalized)?;
    // Supported here means exact source equality, not a probabilistic legal judgment.
    let claim_verifications = synthesized
        .claims
        .iter()
        .map(|c| ClaimVerification {
            claim_id: c.claim_id.clone(),
            evidence_ids: c.evidence_ids.clone(),
            verdict: ClaimVerdict::Supported,
        })
        .collect();
    Ok(VerifiedDocument {
        contract_extraction: synthesized.contract_extraction.clone(),
        document_id: synthesized.document_id.clone(),
        verification_version: VERSION.into(),
        synthesis_attempt_ordinal: 0,
        runtime_id: RUNTIME.into(),
        model_id: "none".into(),
        presentation_mode: SummaryPresentationMode::StructuredExtraction,
        summary_text: synthesized.summary_text.clone(),
        source_chunk_ids: synthesized.source_chunk_ids.clone(),
        summary_claims: Vec::new(),
        synthesis_evidence: Vec::new(),
        summary_claim_verifications: Vec::new(),
        claims: synthesized.claims.clone(),
        claim_verifications,
        key_point_claim_ids: Vec::new(),
        warnings: synthesized.warnings.clone(),
    })
}

pub(super) fn validate_verified(
    verified: &VerifiedDocument,
    synthesized: &SynthesizedDocument,
    analyzed: &AnalyzedDocument,
    chunked: &ChunkedDocument,
    normalized: &NormalizedDocument,
) -> Result<(), PipelineFailure> {
    if *verified
        != verify(
            synthesized,
            analyzed,
            chunked,
            normalized,
            &UNCONTROLLED_EXECUTION,
        )?
    {
        return Err(invalid());
    }
    Ok(())
}
