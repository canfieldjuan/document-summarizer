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
        || !coherent_synthesis_uses_clause_verification(&synthesized.synthesis_version)
    {
        return Ok(prompt);
    }
    let furniture = page_furniture::Furniture::for_synthesis_version(
        normalized,
        &synthesized.synthesis_version,
    );
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

pub(in crate::pipeline::summary) fn summary_verification_batches(
    profile: SummaryProfile,
    runtime: &dyn ModelRuntime,
    synthesized: &SynthesizedDocument,
    normalized: &NormalizedDocument,
) -> Result<Vec<VerificationBatch>, PipelineFailure> {
    let prompt = summary_verification_prompt(profile, synthesized, normalized)?;
    if comparisons::applies(profile, synthesized) {
        comparisons::plan(
            runtime,
            &prompt,
            &synthesized.summary_claims,
            MAX_SUMMARY_CLAIMS,
        )
    } else {
        verification_batches_for_runtime(
            runtime,
            &prompt,
            &synthesized.summary_claims,
            MAX_SUMMARY_CLAIMS,
        )
    }
}

#[cfg(test)]
mod tests;
