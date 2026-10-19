//! The verifier and its admission check consume the same source-owned context.
use super::*;

pub(in crate::pipeline::summary) fn summary_verification_prompt(
    profile: SummaryProfile,
    synthesized: &SynthesizedDocument,
    normalized: &NormalizedDocument,
    chunked: Option<&ChunkedDocument>,
) -> Result<VerificationPrompt, PipelineFailure> {
    if synthesized.synthesis_version == WHOLE_DOCUMENT_VERSION {
        if profile != SummaryProfile::General
            || synthesized.presentation_mode != SummaryPresentationMode::Coherent
        {
            return Err(invalid_document());
        }
        let chunked = chunked.ok_or_else(invalid_document)?;
        let input = whole_document::SourceInput::new(normalized, chunked)?;
        input.validate_saved_synthesis(synthesized)?;
        let contexts = input.evidence_contexts(&synthesized.synthesis_evidence)?;
        let mut prompt =
            verification_prompt(&synthesized.summary_claims, &synthesized.synthesis_evidence)?;
        for claim in &mut prompt.claims {
            for evidence in &mut claim.evidence {
                evidence.full_clause = Some(
                    contexts
                        .get(&evidence.evidence_id)
                        .ok_or_else(invalid_document)?
                        .full_clause
                        .clone(),
                );
            }
        }
        return Ok(prompt);
    }
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
    chunked: Option<&ChunkedDocument>,
) -> Result<Vec<VerificationBatch>, PipelineFailure> {
    let prompt = summary_verification_prompt(profile, synthesized, normalized, chunked)?;
    if comparisons::applies(profile, synthesized) {
        comparisons::plan(
            runtime,
            &prompt,
            &synthesized.summary_claims,
            maximum_claims_for_schema(schema_name_for_version(
                profile,
                &synthesized.synthesis_version,
            )),
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

pub(in crate::pipeline::summary) fn whole_document_verification_plan(
    runtime: &dyn ModelRuntime,
    synthesized: &SynthesizedDocument,
    normalized: &NormalizedDocument,
    chunked: &ChunkedDocument,
) -> Result<comparisons::AdmissionPlan, PipelineFailure> {
    let prompt = summary_verification_prompt(
        SummaryProfile::General,
        synthesized,
        normalized,
        Some(chunked),
    )?;
    let progress = synthesized
        .whole_document_progress
        .as_ref()
        .ok_or_else(invalid_document)?;
    if progress.verification_limits != comparisons::limits_for_runtime(runtime) {
        return Err(invalid_document());
    }
    comparisons::plan_admissions(
        runtime,
        &prompt,
        &synthesized.summary_claims,
        MAX_VERIFICATION_BATCHES,
    )
}

pub(in crate::pipeline::summary) fn validate_admission_records(
    synthesized: &SynthesizedDocument,
    normalized: &NormalizedDocument,
    chunked: &ChunkedDocument,
    verifications: &[ClaimVerification],
) -> Result<(), PipelineFailure> {
    if synthesized.synthesis_version != WHOLE_DOCUMENT_VERSION {
        if verifications.iter().any(|v| v.admission_withhold.is_some()) {
            return Err(invalid_document());
        }
        return Ok(());
    }
    let prompt = summary_verification_prompt(
        SummaryProfile::General,
        synthesized,
        normalized,
        Some(chunked),
    )?;
    let limits = &synthesized
        .whole_document_progress
        .as_ref()
        .ok_or_else(invalid_document)?
        .verification_limits;
    let plan = comparisons::plan_for_limits(
        &prompt,
        &synthesized.summary_claims,
        MAX_VERIFICATION_BATCHES,
        limits,
        false,
        true,
    )?;
    if plan.entries.len() != verifications.len() {
        return Err(invalid_document());
    }
    for (entry, actual) in plan.entries.iter().zip(verifications) {
        match entry {
            comparisons::AdmissionEntry::Admitted(_) if actual.admission_withhold.is_none() => {}
            comparisons::AdmissionEntry::Withheld(expected) if actual == expected => {}
            _ => return Err(invalid_document()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
