//! The verifier and its admission check consume the same source-owned context.
use super::*;

pub(in crate::pipeline::summary) fn summary_verification_prompt(
    profile: SummaryProfile,
    synthesized: &SynthesizedDocument,
    normalized: &NormalizedDocument,
) -> Result<VerificationPrompt, PipelineFailure> {
    let framing = verification_source_framing(profile, &synthesized.synthesis_evidence, normalized);
    let mut prompt = verification_prompt_with_source_framing(
        &synthesized.summary_claims,
        &synthesized.synthesis_evidence,
        &framing,
    )?;
    if profile != SummaryProfile::General
        || synthesized.presentation_mode != SummaryPresentationMode::Coherent
        || synthesized.synthesis_version != VERSION
    {
        return Ok(prompt);
    }
    let furniture = page_furniture::Furniture::new(normalized);
    let clauses = whole_clauses::Clauses::new(normalized, &furniture, ANALYSIS_VERSION);
    let contexts = synthesized
        .synthesis_evidence
        .iter()
        .filter_map(|item| {
            clauses
                .context(&item.block_id, &item.exact_quote)
                .map(|context| (item.evidence_id.as_str(), context))
        })
        .collect::<HashMap<_, _>>();
    for claim in &mut prompt.claims {
        for evidence in &mut claim.evidence {
            evidence.full_clause = contexts.get(evidence.evidence_id.as_str()).cloned();
        }
    }
    Ok(prompt)
}

#[cfg(test)]
mod tests;
