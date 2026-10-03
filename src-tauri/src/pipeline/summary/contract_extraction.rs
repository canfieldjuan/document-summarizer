//! Contract mode copies typed, source-bound clause records without prose generation.
use super::coherent::whole_clauses;
use super::*;
use crate::pipeline::contracts::{
    ContractExtraction, ContractKeyTerm, ContractSectionSelection, ContractSelectionRule,
    ContractTermCategory, ExtractedContractClause,
};

pub(super) const VERSION: &str = "contract-extraction-3.3.0";
const PRE_HEADING_VERSION: &str = "contract-extraction-3.2.0";
const PREVIOUS_VERSION: &str = "contract-extraction-3.1.2";

pub(super) fn version_supported(version: &str) -> bool {
    matches!(version, VERSION | PRE_HEADING_VERSION | PREVIOUS_VERSION)
}

fn furniture_policy(version: &str) -> Result<coherent::page_furniture::Policy, PipelineFailure> {
    match version {
        VERSION | PRE_HEADING_VERSION => Ok(coherent::page_furniture::Policy::RunningMetadata),
        PREVIOUS_VERSION => Ok(coherent::page_furniture::Policy::Original),
        _ => Err(invalid()),
    }
}
const SOURCE_VERSION: &str = "contract-extraction-3.0.2";
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
    version: &str,
) -> Result<(ContractExtraction, Vec<EvidenceItem>), PipelineFailure> {
    let blocks = validate_normalized_chunk_boundary(normalized, chunked)?;
    let mut extraction = ContractExtraction {
        clauses: Vec::new(),
        key_terms: Vec::new(),
    };
    let mut evidence = Vec::new();
    let sources = whole_clauses::contract_sources_for_policy(
        normalized,
        furniture_policy(version)?,
        if version == VERSION {
            whole_clauses::HeadingGrammar::SectionAndSplitArticle
        } else {
            whole_clauses::HeadingGrammar::Original
        },
    );
    for (ordinal, clause) in sources.iter().enumerate() {
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
                    SOURCE_VERSION,
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
        let text = clause.text(&blocks);
        let clause_id = deterministic_id(
            "contract-clause",
            &[
                SOURCE_VERSION,
                &normalized.document_id,
                &ordinal.to_string(),
                &text,
            ],
        );
        extraction.clauses.push(ExtractedContractClause {
            clause_id,
            heading: clause.headings.first().cloned(),
            text,
            evidence_ids,
        });
    }
    if extraction.clauses.is_empty() {
        return Err(invalid());
    }
    extraction.key_terms = select_key_terms(&sources, &extraction.clauses, &blocks);
    Ok((extraction, evidence))
}

pub(super) fn analyze(
    chunked: &ChunkedDocument,
    normalized: &NormalizedDocument,
    control: &dyn ExecutionControl,
) -> Result<AnalyzedDocument, PipelineFailure> {
    analyze_for_version(chunked, normalized, control, VERSION)
}

fn analyze_for_version(
    chunked: &ChunkedDocument,
    normalized: &NormalizedDocument,
    control: &dyn ExecutionControl,
    version: &str,
) -> Result<AnalyzedDocument, PipelineFailure> {
    cancellation_checkpoint(control, PipelineStage::Analyze)?;
    let (_, evidence) = records(chunked, normalized, version)?;
    let mut warnings = normalized.warnings.clone();
    if let Some(warning) =
        coherent::page_furniture::Furniture::for_policy(normalized, furniture_policy(version)?)
            .warning()
    {
        warnings.push(warning);
    }
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
        analysis_version: version.into(),
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
    if *analyzed
        != analyze_for_version(
            chunked,
            normalized,
            &UNCONTROLLED_EXECUTION,
            &analyzed.analysis_version,
        )?
    {
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
    let (extraction, _) = records(chunked, normalized, &analyzed.analysis_version)?;
    let claims = extraction
        .clauses
        .iter()
        .map(|c| CitedClaim {
            claim_id: c.clause_id.clone(),
            text: c.text.clone(),
            evidence_ids: c.evidence_ids.clone(),
        })
        .collect::<Vec<_>>();
    let summary_text = render_units(
        &extraction,
        &claims,
        &analyzed
            .chunks
            .iter()
            .flat_map(|c| c.evidence.clone())
            .collect::<Vec<_>>(),
    )?
    .into_iter()
    .map(|unit| unit.text)
    .collect::<Vec<_>>()
    .join("\n\n");
    Ok(SynthesizedDocument {
        contract_extraction: Some(extraction),
        document_id: analyzed.document_id.clone(),
        synthesis_version: analyzed.analysis_version.clone(),
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
        verification_version: synthesized.synthesis_version.clone(),
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

fn heading_matches(category: ContractTermCategory, heading: &str) -> bool {
    use ContractTermCategory::*;
    let normalized = heading
        .to_ascii_lowercase()
        .replace('&', " and ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let headings: &[&str] = match category {
        Parties => &["parties", "parties to the agreement", "contracting parties"],
        Payment => &[
            "payment",
            "payments",
            "payment terms",
            "fees",
            "compensation",
            "fees and payment",
            "contract price",
            "pricing",
        ],
        TermRenewal => &[
            "term",
            "duration",
            "renewal",
            "term and renewal",
            "term/renewal",
            "commencement and duration",
            "initial term",
            "renewal term",
            "term and termination",
        ],
        Termination => &[
            "termination",
            "termination of agreement",
            "termination of the agreement",
            "cancellation",
            "term and termination",
        ],
        Insurance => &["insurance", "insurance requirements", "property insurance"],
        LiabilityIndemnity => &[
            "liability",
            "limitation of liability",
            "indemnity",
            "indemnification",
            "liability and indemnity",
            "liability and indemnification",
            "hold harmless",
            "limitations of liability",
        ],
    };
    if headings.contains(&normalized.as_str()) {
        return true;
    }
    let conjuncts = normalized
        .split([',', ';'])
        .flat_map(|part| part.split(" and "))
        .flat_map(|part| part.split(" or "))
        .map(str::trim)
        .collect::<Vec<_>>();
    conjuncts.split_last().is_some_and(|(last, earlier)| {
        // Earlier words can share the final head: "Payment and Performance Bonds".
        headings.contains(last)
            || (last.split_whitespace().count() == 1
                && earlier.iter().any(|part| headings.contains(part)))
    })
}

fn select_key_terms(
    sources: &[whole_clauses::SourceClause],
    clauses: &[ExtractedContractClause],
    blocks: &HashMap<&str, &NormalizedBlock>,
) -> Vec<ContractKeyTerm> {
    ContractTermCategory::ALL
        .into_iter()
        .map(|category| {
            let mut selected = HashSet::new();
            let mut selections = Vec::new();
            let mut uncertain_category = false;
            for (index, source) in sources.iter().enumerate() {
                if selected.contains(&index)
                    || !source.headings.iter().any(|h| heading_matches(category, h))
                {
                    continue;
                }
                let members = whole_clauses::section_members(sources, index);
                if members
                    .iter()
                    .any(|&index| sources[index].boundary_uncertain)
                {
                    uncertain_category = true;
                    continue;
                }
                if members.len() == 1 && source.heading_only {
                    continue;
                }
                selected.extend(members.iter().copied());
                selections.push(ContractSectionSelection {
                    rule: ContractSelectionRule::Heading,
                    root_clause_id: clauses[index].clause_id.clone(),
                    clause_ids: members
                        .iter()
                        .map(|&i| clauses[i].clause_id.clone())
                        .collect(),
                });
            }
            if category == ContractTermCategory::Parties && selections.is_empty() {
                let first = sources.iter().position(|s| !s.opening);
                if first.is_some_and(|i| {
                    sources[i].article == Some(1)
                        || sources[i].number.as_deref().is_some_and(|n| n == ["1"])
                }) {
                    let prefix = &sources[..first.unwrap()];
                    if prefix.iter().any(|source| source.boundary_uncertain) {
                        uncertain_category = true;
                    } else if prefix.iter().any(|s| {
                        s.text(blocks)
                            .split(|c: char| !c.is_alphanumeric() && c != '_')
                            .any(|w| w.eq_ignore_ascii_case("between"))
                    }) {
                        let ids = clauses[..prefix.len()]
                            .iter()
                            .map(|c| c.clause_id.clone())
                            .collect::<Vec<_>>();
                        if let Some(root) = ids.first() {
                            selections.push(ContractSectionSelection {
                                rule: ContractSelectionRule::OpeningBetween,
                                root_clause_id: root.clone(),
                                clause_ids: ids,
                            });
                        }
                    }
                }
            }
            if uncertain_category {
                selections.clear();
            }
            ContractKeyTerm {
                category,
                selections,
            }
        })
        .collect()
}

pub(super) struct RenderUnit {
    pub text: String,
    pub clause_ids: Vec<String>,
}

// Canonical text, atomic delivery boundaries and source accounting share one owner.
pub(super) fn render_units(
    extraction: &ContractExtraction,
    claims: &[CitedClaim],
    evidence: &[EvidenceItem],
) -> Result<Vec<RenderUnit>, PipelineFailure> {
    if extraction
        .key_terms
        .iter()
        .map(|t| t.category)
        .collect::<Vec<_>>()
        != ContractTermCategory::ALL
        || claims.len() != extraction.clauses.len()
    {
        return Err(invalid());
    }
    let by_evidence = evidence
        .iter()
        .map(|e| (e.evidence_id.as_str(), e))
        .collect::<HashMap<_, _>>();
    let lines = render_claim_lines(claims, &by_evidence)?;
    let mut by_id = HashMap::new();
    for (clause, (claim, line)) in extraction.clauses.iter().zip(claims.iter().zip(lines)) {
        if clause.clause_id != claim.claim_id
            || clause.text != claim.text
            || clause.evidence_ids != claim.evidence_ids
            || by_id
                .insert(
                    clause.clause_id.as_str(),
                    format!("{}\n{}", clause.clause_id, line),
                )
                .is_some()
        {
            return Err(invalid());
        }
    }
    let mut result = Vec::new();
    let mut pending = "Key terms".to_string();
    for term in &extraction.key_terms {
        if term.selections.is_empty() {
            pending.push_str(&format!("\n\n{}: not identified", term.category.label()));
            continue;
        }
        pending.push_str(&format!("\n\n{}", term.category.label()));
        for selection in &term.selections {
            if selection.clause_ids.first() != Some(&selection.root_clause_id) {
                return Err(invalid());
            }
            let mut unique = HashSet::new();
            let quoted = selection
                .clause_ids
                .iter()
                .map(|id| {
                    if !unique.insert(id) {
                        return Err(invalid());
                    }
                    by_id
                        .get(id.as_str())
                        .map(|s| s.as_str())
                        .ok_or_else(invalid)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("\n\n");
            let text = if pending.is_empty() {
                quoted
            } else {
                format!("{pending}\n\n{quoted}")
            };
            result.push(RenderUnit {
                text,
                clause_ids: selection.clause_ids.clone(),
            });
            pending.clear();
        }
    }
    pending.push_str(if pending.is_empty() {
        "Full clause list"
    } else {
        "\n\nFull clause list"
    });
    for clause in &extraction.clauses {
        let line = by_id[clause.clause_id.as_str()].clone();
        let text = if pending.is_empty() {
            line
        } else {
            format!("{pending}\n\n{line}")
        };
        result.push(RenderUnit {
            text,
            clause_ids: vec![clause.clause_id.clone()],
        });
        pending.clear();
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    #[test]
    fn heading_grammar_ordinal_prefix_boundaries_preserve_saved_versions() {
        let mut failures = Vec::new();
        for marker in ["2:", "IV:", "2-1", "IV-1", "2–1", "IV—1", "iv:"] {
            for separator in [" ", "\t"] {
                let text = format!("1. Limitation of Liability. Neither party is liable for indirect damages.\n{marker}{separator}Payment Terms\nClient shall pay invoices within thirty days.");
                let (normalized, chunked) = fixture("ordinal-prefix-boundary", &[&text]);
                for version in [PREVIOUS_VERSION, PRE_HEADING_VERSION, VERSION] {
                    let (extraction, _) = records(&chunked, &normalized, version).unwrap();
                    let liability = selected(&extraction, ContractTermCategory::LiabilityIndemnity);
                    println!(
                        "ORDINAL_PREFIX {version} {marker:?} {separator:?}: liability={:?}",
                        liability
                            .iter()
                            .map(|c| c.text.as_str())
                            .collect::<Vec<_>>()
                    );
                    if !liability.is_empty() {
                        failures.push((version, marker, separator));
                    }
                    assert!(selected(&extraction, ContractTermCategory::Payment).is_empty());
                    assert_eq!(extraction.clauses.len(), 1);
                    assert_eq!(extraction.clauses[0].text, text);
                }
            }
        }
        assert!(
            failures.is_empty(),
            "ordinal-prefix lookalikes silently extended liability: {failures:?}"
        );
    }

    #[test]
    fn heading_grammar_ordinal_prefix_clean_controls() {
        let liability = "1. Limitation of Liability. Neither party is liable for indirect damages.";
        for heading in [
            "2. Payment Terms",
            "ARTICLE 2: Payment Terms",
            "ARTICLE IV - Payment Terms",
            "ARTICLE IV PAYMENT TERMS",
        ] {
            let text =
                format!("{liability}\n{heading}\nClient shall pay invoices within thirty days.");
            let (normalized, chunked) = fixture("ordinal-prefix-control", &[&text]);
            for version in [PREVIOUS_VERSION, PRE_HEADING_VERSION, VERSION] {
                let (extraction, _) = records(&chunked, &normalized, version).unwrap();
                let terms = selected(&extraction, ContractTermCategory::LiabilityIndemnity);
                assert_eq!(terms.len(), 1, "{version}: {heading}");
                assert_eq!(terms[0].text, liability);
                let payment = selected(&extraction, ContractTermCategory::Payment);
                assert_eq!(payment.len(), 1, "{version}: {heading}");
                assert_eq!(
                    payment[0].text,
                    format!("{heading}\nClient shall pay invoices within thirty days.")
                );
            }
        }
        for marker in ["0:", "1000:", "IIII:", "MMMM:", "-2", ":IV"] {
            let text = format!("ARTICLE {marker}\nPAYMENT TERMS\nClient shall pay.");
            let extraction = output("invalid-split-ordinal", &[&text]).0;
            assert!(selected(&extraction, ContractTermCategory::Payment).is_empty());
        }
        for marker in ["2:", "IV:", "2-1"] {
            let text = format!("ARTICLE {marker}\nPAYMENT TERMS\nClient shall pay.");
            let extraction = output("unsupported-split-ordinal", &[&text]).0;
            assert!(selected(&extraction, ContractTermCategory::Payment).is_empty());
        }
    }

    #[test]
    fn heading_origin_reproduction_section_prefix() {
        for prefix in ["Section ", "SECTION\t"] {
            let source = format!("{prefix}5 PAYMENT\n{prefix}5.1 Fees. Client shall pay after acceptance.\n{prefix}5.2 Timing. Payment is due in thirty days.\n{prefix}6 Notices. Written notice is required.");
            let (extraction, _, _) = output("public-section-layout", &[&source]);
            let payment = selected(&extraction, ContractTermCategory::Payment);
            assert_eq!(
                payment.len(),
                3,
                "Section parent and children were not identified: {prefix:?}"
            );
            assert!(payment[1].text.contains("after acceptance"));
            assert!(payment[2].text.contains("thirty days"));
            assert!(payment.iter().all(|c| !c.text.contains("Written notice")));
        }
    }

    #[test]
    fn heading_origin_reproduction_split_article() {
        let source = "ARTICLE III\nPAYMENT\nClient shall pay after acceptance.\nARTICLE IV\nNOTICES\nWritten notice is required.";
        let (extraction, _, _) = output("public-article-layout", &[source]);
        let payment = selected(&extraction, ContractTermCategory::Payment);
        assert_eq!(payment.len(), 1, "split ARTICLE was not identified");
        assert_eq!(
            payment[0].text,
            "ARTICLE III\nPAYMENT\nClient shall pay after acceptance."
        );
        assert!(!payment[0].text.contains("NOTICES"));
    }

    #[test]
    fn heading_grammar_keeps_clean_forms_and_rejects_lookalikes() {
        for prefix in ["Section ", "SECTION\t", "§ ", "§", ""] {
            let source = format!("{prefix}5.1 Payment. Client shall pay.\n{prefix}5.2 Notices. Written notice is required.");
            let (extraction, _, _) = output("decimal-forms", &[&source]);
            let terms = selected(&extraction, ContractTermCategory::Payment);
            assert_eq!(terms.len(), 1, "{prefix:?}");
            assert!(!terms[0].text.contains("Notices"));
        }
        for source in [
            "Section Five Payment\nClient shall pay.",
            "Section 5..1 Payment\nClient shall pay.",
            "Section 1000 Payment\nClient shall pay.",
            "section 5 Payment\nClient shall pay.",
            "See Section 5 Payment for the applicable rate.",
            "ARTICLE IIII\nPAYMENT\nClient shall pay.",
            "ARTICLE 0\nPAYMENT\nClient shall pay.",
            "ARTICLE 1000\nPAYMENT\nClient shall pay.",
            "ARTICLE III\nPayment\nClient shall pay.",
            "article III\nPAYMENT\nClient shall pay.",
            "ARTICLE III\nPAYMENT",
            "ARTICLE III\nPAYMENT ... 5",
            "TABLE OF CONTENTS\nARTICLE III\nPAYMENT\nARTICLE IV\nINSURANCE",
            "Section 5. Payment Terms ... 4\nSection 6. Insurance ... 5",
            "Section 5 Payment\nClient shall pay.\nSection Five Conditions\nApproval required.",
        ] {
            let (extraction, _, _) = output("heading-lookalikes", &[source]);
            assert!(
                extraction.key_terms.iter().all(|t| t.selections.is_empty()),
                "{source}"
            );
            for line in source.lines().filter(|line| !line.is_empty()) {
                assert!(
                    extraction.clauses.iter().any(|c| c.text.contains(line)),
                    "lost line: {line}"
                );
            }
        }
    }

    #[test]
    fn heading_grammar_split_article_scope_and_original_ranges() {
        let pages = ["ARTICLE III", "PAYMENT\n3.1 Amount. Client shall pay.\n3.2 Timing. Due on acceptance.\nARTICLE IV\nNOTICES\n4.1 Delivery. Written notice is required."];
        let (mut normalized, mut chunked) = fixture("split-blocks", &pages);
        // A page boundary is deliberately unsupported and keeps payment uncertain.
        let (old, _) = records(&chunked, &normalized, VERSION).unwrap();
        assert!(selected(&old, ContractTermCategory::Payment).is_empty());
        let mut second = normalized.pages.pop().unwrap();
        for block in &mut second.content {
            block.source.page_start = 1;
            block.source.page_end = 1;
        }
        chunked.chunks[1].source_spans = second.content.iter().map(|b| b.source.clone()).collect();
        normalized.pages[0].content.extend(second.content);
        let (extraction, evidence) = records(&chunked, &normalized, VERSION).unwrap();
        let payment = selected(&extraction, ContractTermCategory::Payment);
        assert_eq!(payment.len(), 3);
        assert_eq!(payment[0].text, "ARTICLE III\n\nPAYMENT");
        assert_eq!(payment[0].evidence_ids.len(), 2);
        assert!(payment.iter().all(|c| !c.text.contains("NOTICES")));
        for item in evidence {
            assert!(normalized.pages[0]
                .content
                .iter()
                .any(|b| b.block_id == item.block_id && b.text.contains(&item.exact_quote)));
        }
        let (extraction, _, _) = output("unfinished-boundary", &["2. Liability. Neither party is liable unless\nARTICLE III\nPAYMENT\nClient shall pay."]);
        assert!(selected(&extraction, ContractTermCategory::LiabilityIndemnity).is_empty());
        assert!(selected(&extraction, ContractTermCategory::Payment).is_empty());
        assert!(extraction.clauses[0].text.ends_with("unless"));
    }

    #[test]
    fn heading_grammar_wrapped_named_reference_is_uncertain() {
        let mut failures = Vec::new();
        for marker in ["Section 6 Termination", "ARTICLE VI\nTERMINATION"] {
            let text = format!("1. Payment\nClient shall pay according to\n{marker}\nprocedures printed in the attached agreement.\n7. Other. Other obligations.");
            let (extraction, _, _) = output("wrapped-named-section", &[&text]);
            if !selected(&extraction, ContractTermCategory::Termination).is_empty() {
                failures.push(marker);
            }
            assert!(selected(&extraction, ContractTermCategory::Payment).is_empty());
            for line in text.lines() {
                assert!(extraction.clauses.iter().any(|c| c.text.contains(line)));
            }
        }
        assert!(
            failures.is_empty(),
            "wrapped named references gained false termination labels: {failures:?}"
        );
    }

    #[test]
    fn heading_grammar_mixed_numeric_scope_abstains() {
        let text = "Section 2. Fees. The rates are:\n1. Setup fee is $10.\n2. Weekly fee is $20.\nSection 3. Notices. Send written notice.";
        let (extraction, _, _) = output("mixed-numbering", &[text]);
        assert!(
            selected(&extraction, ContractTermCategory::Payment).is_empty(),
            "mixed numbering selected fee introduction without its prices"
        );
        for line in text.lines() {
            assert!(extraction.clauses.iter().any(|c| c.text.contains(line)));
        }
        for child_prefix in ["Section ", "SECTION\t", "§ ", ""] {
            let clean = format!("Section 2. Fees. The rates are:\n{child_prefix}2.1 Setup fee is $10.\n{child_prefix}2.2 Weekly fee is $20.\nSection 3. Notices. Send written notice.");
            let (extraction, _, _) = output("decimal-children", &[&clean]);
            let payment = selected(&extraction, ContractTermCategory::Payment);
            assert_eq!(payment.len(), 3, "{child_prefix}");
            assert!(payment[1].text.contains("$10."));
            assert!(payment[2].text.contains("$20."));
        }
    }

    #[test]
    fn heading_grammar_reloads_both_historical_versions() {
        let (normalized, chunked) = fixture("versioned-headings", &["ARTICLE III\nPAYMENT\nClient shall pay.\nSection 4 Insurance. Coverage is required."]);
        for version in [PREVIOUS_VERSION, PRE_HEADING_VERSION, VERSION] {
            let analyzed =
                analyze_for_version(&chunked, &normalized, &UNCONTROLLED_EXECUTION, version)
                    .unwrap();
            let synthesized =
                synthesize(&analyzed, &chunked, &normalized, &UNCONTROLLED_EXECUTION).unwrap();
            let verified = verify(
                &synthesized,
                &analyzed,
                &chunked,
                &normalized,
                &UNCONTROLLED_EXECUTION,
            )
            .unwrap();
            let saved: VerifiedDocument =
                serde_json::from_str(&serde_json::to_string(&verified).unwrap()).unwrap();
            super::super::validate_verified_document(
                &saved,
                &synthesized,
                &analyzed,
                &chunked,
                &normalized,
            )
            .unwrap();
            let extraction = saved.contract_extraction.as_ref().unwrap();
            assert_eq!(
                !selected(extraction, ContractTermCategory::Payment).is_empty(),
                version == VERSION
            );
            assert_eq!(
                !selected(extraction, ContractTermCategory::Insurance).is_empty(),
                version == VERSION
            );
        }
        assert!(analyze_for_version(
            &chunked,
            &normalized,
            &UNCONTROLLED_EXECUTION,
            "contract-extraction-unknown"
        )
        .is_err());
    }

    #[test]
    fn truncation_reader_retains_caps_tails_across_units() {
        let prefix = "15. Limitation of Liability: Neither party is liable unless it acted in an";
        let caps_prefix = "15. Limitation of Liability: Neither party is liable\nUNLESS SUCH PARTY HAS ACTED IN AN";
        let tail = "INTENTIONALLY WRONGFUL OR GROSSLY NEGLIGENT MANNER.\n16. Force Majeure: Delays are excused.";
        let mut failures = Vec::new();
        for (name, first, same_page) in [
            ("cross-page", prefix, false),
            ("cross-block", prefix, true),
            ("caps-predecessor", caps_prefix, true),
        ] {
            let (mut normalized, mut chunked) = fixture(name, &[first, tail]);
            if same_page {
                let mut second = normalized.pages.pop().unwrap();
                for block in &mut second.content {
                    block.source.page_start = 1;
                    block.source.page_end = 1;
                }
                chunked.chunks[1].source_spans =
                    second.content.iter().map(|b| b.source.clone()).collect();
                normalized.pages[0].content.extend(second.content);
            }
            let (extraction, _) = records(&chunked, &normalized, VERSION).unwrap();
            let terms = selected(&extraction, ContractTermCategory::LiabilityIndemnity);
            let complete = terms.len() == 1
                && terms[0].text.contains("GROSSLY NEGLIGENT MANNER.")
                && !terms[0].text.contains("16.");
            println!("TRUNCATION_TAIL {name}: complete={complete}");
            if !complete {
                failures.push(name);
            }
        }
        let same_block = format!("{caps_prefix}\n{tail}");
        let extraction = output("same-block-caps", &[&same_block]).0;
        let terms = selected(&extraction, ContractTermCategory::LiabilityIndemnity);
        let complete = terms.len() == 1
            && terms[0].text.contains("GROSSLY NEGLIGENT MANNER.")
            && !terms[0].text.contains("16.");
        println!("TRUNCATION_TAIL same-block: complete={complete}");
        if !complete {
            failures.push("same-block");
        }
        assert!(
            failures.is_empty(),
            "caps exception truncated: {failures:?}"
        );
    }

    #[test]
    fn truncation_reader_marks_unfinished_source_at_admitted_heading() {
        let mut failures = Vec::new();
        for (name, next) in [
            ("numbered", "3. Other. Other obligations."),
            ("article", "ARTICLE III - OTHER\nOther obligations."),
            ("roman", "III. OTHER\n3.1 Other obligations."),
        ] {
            let first = "2. Payment. Pay only after the client has";
            let extraction = output(name, &[first, next]).0;
            let abstains = selected(&extraction, ContractTermCategory::Payment).is_empty();
            println!("TRUNCATION_BOUNDARY {name}: abstains={abstains}");
            if !abstains {
                failures.push(name);
            }
        }
        assert!(
            failures.is_empty(),
            "unfinished source selected: {failures:?}"
        );
    }

    #[test]
    fn truncation_reader_preserves_completed_title_controls() {
        for last in [".", ":", ";", "?", "!", ")"] {
            let first = format!("1. Services. Deliver work{last}");
            let next = "Payment\n2.1 Pay after acceptance.\n3. Other. Other obligations.";
            let extraction = output("completed-title", &[&first, next]).0;
            let terms = selected(&extraction, ContractTermCategory::Payment);
            assert!(!terms.is_empty(), "clean title rejected after {last}");
            assert!(terms
                .iter()
                .any(|c| c.text.contains("Pay after acceptance.")));
        }
        let first = "1. Payment\n1.1 Pay for work.";
        let extraction = output(
            "complete-parent",
            &[first, "2. Insurance\n2.1 Maintain coverage."],
        )
        .0;
        assert!(!selected(&extraction, ContractTermCategory::Payment).is_empty());
        assert!(!selected(&extraction, ContractTermCategory::Insurance).is_empty());
        let extraction = output("mixed-truncated", &["1. Payment. Pay upon receipt.\n2. Payment. Pay only after\n3. Insurance. Maintain coverage."]).0;
        assert!(selected(&extraction, ContractTermCategory::Payment).is_empty());
        assert!(!selected(&extraction, ContractTermCategory::Insurance).is_empty());
    }

    #[test]
    fn public_boundary_slot_proofs() {
        let fixtures = [
            ("tab-payment", ContractTermCategory::Payment, "2. Payment\n2.1 Pay after acceptance.\nIII.\tOTHER\n3.1 Other obligations.", "2. Payment\n2.1 Pay after acceptance.\n3. Other. Excluded."),
            ("tab-term", ContractTermCategory::TermRenewal, "2. Term\n2.1 The agreement continues.\nIII.\tOTHER\n3.1 Other obligations.", "2. Term\n2.1 The agreement continues.\n3. Other. Excluded."),
            ("tab-liability", ContractTermCategory::LiabilityIndemnity, "2. Liability\n2.1 Cover losses.\nIII.\tOTHER\n3.1 Other obligations.", "2. Liability\n2.1 Cover losses.\n3. Other. Excluded."),
            ("appendix-payment", ContractTermCategory::Payment, "2. Payment. Pay after acceptance.\nAPPENDIX A\nI. Services\nOther obligations.\n3. Other. Excluded.", "2. Payment. Pay after acceptance.\n3. Other. Excluded."),
            ("toc-opening", ContractTermCategory::Parties, "Agreement between Alpha and Beta.\nTABLE OF CONTENTS\nARTICLE I\nSERVICES\n1.1 Services 3", "Agreement between Alpha and Beta.\n1. Services\n1.1 Deliver services."),
            ("toc-payment", ContractTermCategory::Payment, "TABLE OF CONTENTS\nARTICLE II\nPAYMENT\n2.1 Terms 3", "2. Payment\n2.1 Pay after acceptance.\n3. Other. Excluded."),
            ("toc-termination", ContractTermCategory::Termination, "TABLE OF CONTENTS\nARTICLE III\nTERMINATION\n3.1 Terms 5", "3. Termination\n3.1 Give written notice.\n4. Other. Excluded."),
        ];
        let mut failures = Vec::new();
        for (name, category, bad, good) in fixtures {
            let positive = !selected(&output(name, &[good]).0, category).is_empty();
            let abstained = selected(&output(name, &[bad]).0, category).is_empty();
            println!(
                "PUBLIC_SLOT {name}: clean_positive={positive}, uncertain_abstained={abstained}"
            );
            assert!(positive, "clean positive control failed: {name}");
            if !abstained {
                failures.push(name);
            }
        }
        assert!(
            failures.is_empty(),
            "unrecognized boundary fixtures retained wrong selections: {failures:?}"
        );
    }

    use super::*;
    use crate::pipeline::contracts::{
        DocumentChunk, NormalizedBlockKind, NormalizedPage, SourceType,
    };

    fn fixture(id: &str, pages: &[&str]) -> (NormalizedDocument, ChunkedDocument) {
        let normalized = NormalizedDocument {
            document_id: id.into(),
            normalization_version: "1".into(),
            warnings: vec![],
            pages: pages
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    let page = i as u32 + 1;
                    NormalizedPage {
                        page_number: page,
                        warnings: vec![],
                        requires_visual_processing: false,
                        content: vec![NormalizedBlock {
                            block_id: format!("{id}-block-{page}"),
                            kind: NormalizedBlockKind::Text,
                            text: (*text).into(),
                            source: SourceSpan {
                                page_start: page,
                                page_end: page,
                                section_id: None,
                                source_type: SourceType::NativeText,
                            },
                        }],
                    }
                })
                .collect(),
        };
        let chunked = ChunkedDocument {
            document_id: id.into(),
            chunking_version: "1".into(),
            warnings: vec![],
            chunks: normalized
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let block = &p.content[0];
                    DocumentChunk {
                        chunk_id: format!("{id}-chunk-{i}"),
                        ordinal: i as u32,
                        structure_node_id: "public-node".into(),
                        text: block.text.clone(),
                        block_ids: vec![block.block_id.clone()],
                        source_spans: vec![block.source.clone()],
                        warnings: vec![],
                    }
                })
                .collect(),
        };
        (normalized, chunked)
    }
    fn output(
        id: &str,
        pages: &[&str],
    ) -> (ContractExtraction, SynthesizedDocument, Vec<EvidenceItem>) {
        let (normalized, chunked) = fixture(id, pages);
        let analyzed = analyze(&chunked, &normalized, &UNCONTROLLED_EXECUTION).unwrap();
        let synthesized =
            synthesize(&analyzed, &chunked, &normalized, &UNCONTROLLED_EXECUTION).unwrap();
        verify(
            &synthesized,
            &analyzed,
            &chunked,
            &normalized,
            &UNCONTROLLED_EXECUTION,
        )
        .unwrap();
        (
            synthesized.contract_extraction.clone().unwrap(),
            synthesized,
            analyzed
                .chunks
                .into_iter()
                .flat_map(|c| c.evidence)
                .collect(),
        )
    }
    fn selected(
        extraction: &ContractExtraction,
        category: ContractTermCategory,
    ) -> Vec<&ExtractedContractClause> {
        extraction
            .key_terms
            .iter()
            .find(|t| t.category == category)
            .unwrap()
            .selections
            .iter()
            .flat_map(|s| &s.clause_ids)
            .map(|id| {
                extraction
                    .clauses
                    .iter()
                    .find(|c| &c.clause_id == id)
                    .unwrap()
            })
            .collect()
    }

    #[test]
    fn revised_policy_wrapped_title_stays_with_its_sentence() {
        let (extraction, _, _) = output("wrapped-title", &["10. General\n10.1 These duties are agreed between the\nParties.\n10.2 Performance. Deliver the goods."]);
        assert!(
            selected(&extraction, ContractTermCategory::Parties).is_empty(),
            "wrapped sentence tail must not select an unrelated parties section"
        );
        assert!(extraction.clauses[1]
            .text
            .ends_with("between the\nParties."));
        assert!(extraction.clauses[2].text.starts_with("10.2 Performance."));

        for ending in [".", ":", ";", "?", "!", ")"] {
            let text = format!("10. General\n10.1 Prior provision ends here{ending}\nParties\n10.2 Named entities sign.");
            let extraction = output("real-title", &[&text]).0;
            assert_eq!(
                selected(&extraction, ContractTermCategory::Parties).len(),
                1,
                "terminal {ending}"
            );
        }
        for previous in [
            "10.",
            "10. General",
            "GENERAL TERMS.",
            "Article X - GENERAL",
        ] {
            let text = format!("{previous}\nParties\n10.2 Named entities sign.");
            assert!(
                !selected(
                    &output("structural-predecessor", &[&text]).0,
                    ContractTermCategory::Parties
                )
                .is_empty(),
                "{previous}"
            );
        }
        assert_eq!(
            selected(
                &output(
                    "page-start",
                    &[
                        "10. General\n10.1 Prior text continues",
                        "Parties\n10.2 Named entities sign."
                    ]
                )
                .0,
                ContractTermCategory::Parties
            )
            .len(),
            0
        );
        let (mut normalized, _) =
            fixture("block-start", &["10. General\n10.1 Prior text continues"]);
        let mut next = normalized.pages[0].content[0].clone();
        next.block_id = "block-start-second".into();
        next.text = "Parties\n10.2 Named entities sign.".into();
        normalized.pages[0].content.push(next);
        let sources = whole_clauses::contract_sources(&normalized);
        assert!(!sources
            .iter()
            .any(|c| c.headings.iter().any(|h| h == "Parties")));
        assert!(sources[1].boundary_uncertain);
        assert_eq!(sources[1].fragments.len(), 2);
    }

    #[test]
    fn revised_policy_wrapped_number_preserves_payment_parent() {
        let (extraction, _, _) = output("wrapped-number", &["5. Payment\n5.1 Client shall pay within\n30 days of invoice.\n5.2 Approval is required.\n6. Other. Next section."]);
        let payment = selected(&extraction, ContractTermCategory::Payment);
        assert_eq!(
            payment.len(),
            3,
            "wrapped number must not close the payment parent"
        );
        assert!(payment[1].text.contains("within\n30 days of invoice."));
        assert!(payment[2].text.starts_with("5.2 Approval"));
        assert!(!payment.iter().any(|c| c.text.contains("Next section")));
        for text in [
            "5.4 of the prior section",
            "30 days after acceptance",
            "7 (calendar days)",
        ] {
            let unfinished =
                format!("5. Payment\n5.1 Conditions continue\n{text}\n5.2 Approval is required.");
            let extraction = output("number-continuation", &[&unfinished]).0;
            let expected = if text.ends_with(')') { 3 } else { 0 };
            assert_eq!(
                selected(&extraction, ContractTermCategory::Payment).len(),
                expected
            );
            assert!(extraction.clauses[1].text.contains(text));
            let completed =
                format!("5. Payment\n5.1 Conditions continue\n{text}.\n5.2 Approval is required.");
            assert_eq!(
                selected(
                    &output("number-completed", &[&completed]).0,
                    ContractTermCategory::Payment
                )
                .len(),
                3
            );
        }
        assert_eq!(
            selected(
                &output(
                    "number-control",
                    &["5. Payment\n5.1 Client shall pay.\n5.2 Approval is required."]
                )
                .0,
                ContractTermCategory::Payment
            )
            .len(),
            3
        );
    }

    #[test]
    fn revised_policy_combined_headings_match_whole_conjuncts_only() {
        use ContractTermCategory::*;
        for separator in [" and ", ", ", " or ", "; ", " & "] {
            let text = format!(
                "5. Payment{separator}Insurance. Printed terms apply.\n6. Other. Excluded."
            );
            let extraction = output("combined", &[&text]).0;
            let payment = selected(&extraction, Payment);
            assert_eq!(
                payment.len(),
                1,
                "combined separator {separator:?} must match payment"
            );
            assert_eq!(payment, selected(&extraction, Insurance));
            assert_eq!(extraction.clauses.len(), 2);
        }
        for title in [
            "Assignment and Payment",
            "Insurance and Bonds",
            "Term and Termination",
            "Fees and Payment",
            "Commencement and Duration",
            "Liability and Indemnity",
        ] {
            let text = format!("5. {title}. Printed terms apply.");
            assert!(
                output("mixed", &[&text])
                    .0
                    .key_terms
                    .iter()
                    .any(|t| !t.selections.is_empty()),
                "{title}"
            );
        }
        for title in [
            "payment history example",
            "Repayment and Payment history example",
            "Terminated or Insurance records example",
            "Payment|Insurance",
            "",
            "and",
            ", ; &",
        ] {
            assert!(
                ContractTermCategory::ALL
                    .iter()
                    .all(|category| !heading_matches(*category, title)),
                "{title}"
            );
        }
        let extraction = output(
            "duplicate-conjunct",
            &["5. Payment and Payment. Printed terms apply."],
        )
        .0;
        assert_eq!(extraction.key_terms[1].selections.len(), 1);
    }

    #[test]
    fn combined_heading_rejects_shared_bond_head() {
        assert_unselected_combined_heading("Payment and Performance Bonds");
    }

    #[test]
    fn combined_heading_rejects_shared_service_head() {
        assert_unselected_combined_heading("Duration and Frequency of Services");
    }

    #[test]
    fn combined_heading_rejects_multiword_list_tail() {
        assert_unselected_combined_heading("Permits, Fees, Licenses, and Other Obligations");
    }

    #[test]
    fn combined_heading_shared_tail_is_an_explained_miss() {
        assert_unselected_combined_heading(
            "Termination, Suspension or Assignment of the Subcontract",
        );
    }

    fn assert_unselected_combined_heading(title: &str) {
        let text = format!("5. {title}. Printed terms remain in the source inventory.");
        let (extraction, _, _) = output("shared-head", &[&text]);
        assert!(
            extraction.key_terms.iter().all(|t| t.selections.is_empty()),
            "shared multi-word head must not select a topic: {title}"
        );
        assert_eq!(extraction.clauses.len(), 1);
        assert_eq!(extraction.clauses[0].text, text);
    }

    #[test]
    fn combined_heading_retains_supported_controls() {
        use ContractTermCategory::*;
        for (title, expected) in [
            ("Invoicing and Payment", vec![Payment]),
            ("Survival and Termination", vec![Termination]),
            ("Insurance and Bonds", vec![Insurance]),
            ("Term and Termination", vec![TermRenewal, Termination]),
            ("Fees and Payment Terms", vec![Payment]),
            ("Insurance and Payment Terms", vec![Payment]),
        ] {
            let text = format!("5. {title}. Printed terms apply.");
            let (extraction, _, _) = output("combined-control", &[&text]);
            let actual = extraction
                .key_terms
                .iter()
                .filter(|t| !t.selections.is_empty())
                .map(|t| t.category)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{title}");
            for term in &extraction.key_terms {
                if !term.selections.is_empty() {
                    assert_eq!(term.selections.len(), 1);
                    assert_eq!(
                        selected(&extraction, term.category),
                        vec![&extraction.clauses[0]]
                    );
                }
            }
        }
        for title in ["Payment,", "Payment;", "Payment &", "Payment, ;"] {
            assert!(!heading_matches(Payment, title), "{title}");
        }
    }

    #[test]
    fn classifier_false_withdrawals_preserve_clean_terms() {
        let cases = [
            ("admitted-matching-roman", "2. Payment\n2.1 Pay after acceptance.\nIII. OTHER\n3.1 Other obligations.", ContractTermCategory::Payment),
            ("admitted-sequence-roman", "II. PAYMENT\n2.1 Pay after acceptance.\nIII. OTHER\nUnnumbered other obligations.", ContractTermCategory::Payment),
            ("admitted-roman-term", "II. PAYMENT\n2.1 Pay after acceptance.\nIII. TERM AND TERMINATION\n3.1 Term obligations.\nIV. OTHER\n4.1 Other obligations.", ContractTermCategory::Termination),
            ("wrapped-sentence-tail", "1. Termination\n1.1 Either party may terminate this\nAgreement.\n2. Other. Other obligations.", ContractTermCategory::Termination),
            ("long-caps-body", "1. Insurance\n1.1 Maintain coverage:\nCONTRACTOR WILL MAINTAIN THE FOLLOWING INSURANCE POLICIES IN FULL FORCE AND EFFECT\nfor the duration.\n2. Other. Other obligations.", ContractTermCategory::Insurance),
            ("field-label", "1. Termination\n1.1 Send notices to the following address.\nName/Title:\nAn authorized representative.\n2. Other. Other obligations.", ContractTermCategory::Termination),
            ("opening-title", "SERVICE AGREEMENT\nAgreement between Alpha and Beta.\n1. Services\n1.1 Deliver the services.", ContractTermCategory::Parties),
            ("opening-party-name", "ALPHA CORPORATION\nBETA CORPORATION\nAgreement between Alpha and Beta.\n1. Services\n1.1 Deliver the services.", ContractTermCategory::Parties),
        ];
        let failures = cases
            .into_iter()
            .filter_map(|(name, text, category)| {
                selected(&output(name, &[text]).0, category)
                    .is_empty()
                    .then_some(name)
            })
            .collect::<Vec<_>>();
        assert!(
            failures.is_empty(),
            "classifier falsely withdrew clean terms: {failures:?}"
        );
    }

    #[test]
    fn classifier_body_context_and_amount_fields() {
        let cases = [
            ("wrapped-exhibit-reference", "1. Payment\n1.1 Pay at the rates in\nExhibit A. Submit an invoice for services.\n2. Other. Other obligations.", ContractTermCategory::Payment),
            ("wrapped-article-reference", "1. Insurance\n1.1 Coverage is pursuant to\nArticle 11. Certificates protect the client.\n2. Other. Other obligations.", ContractTermCategory::Insurance),
            ("wrapped-section-reference", "1. Termination\n1.1 Apply the terms of\nSection 6. The parties shall give written notice.\n2. Other. Other obligations.", ContractTermCategory::Termination),
            ("letter-list-body", "1. Payment\n1.1 Pay the invoice.\na. Contractor shall pay for all of the services upon receipt of an invoice.\nb. The parties shall reconcile any disputed items before making final payment.\n2. Other. Other obligations.", ContractTermCategory::Payment),
            ("roman-list-body", "1. Payment\n1.1 Pay the invoice.\ni. all required documentation is submitted;\nii. all work has been completed and delivered to the client;\n2. Other. Other obligations.", ContractTermCategory::Payment),
        ];
        let failures = cases
            .into_iter()
            .filter_map(|(name, text, category)| {
                selected(&output(name, &[text]).0, category)
                    .is_empty()
                    .then_some(name)
            })
            .collect::<Vec<_>>();
        assert!(
            failures.is_empty(),
            "body context falsely marked uncertain: {failures:?}"
        );
    }

    #[test]
    fn unterminated_fields_before_heading_abstain_without_becoming_headings() {
        for field in [
            "Payment Terms Net 30",
            "2026 Public Service Agreement 6",
            "Subcontract 022500",
        ] {
            let text = format!(
                "1. Termination\n1.1 Give written notice.\n{field}\n2. Other. Other obligations."
            );
            let extraction = output("field-boundary", &[&text]).0;
            assert!(selected(&extraction, ContractTermCategory::Termination).is_empty());
            assert_eq!(extraction.clauses.len(), 3);
            assert!(extraction.clauses[1].text.contains(field));
            let completed = text.replace(field, &format!("{field}."));
            assert!(!selected(
                &output("field-completed", &[&completed]).0,
                ContractTermCategory::Termination
            )
            .is_empty());
        }
    }

    #[test]
    fn recognized_section_starts_after_unknown_boundary() {
        let text = "1. Other. Ordinary obligations.\nIII.\tOTHER\n3.1 Payment. Pay after acceptance.\n4. Other. Other obligations.";
        let extraction = output("incoming-unknown-boundary", &[text]).0;
        assert!(
            !selected(&extraction, ContractTermCategory::Payment).is_empty(),
            "unknown line outside selected extent contaminated its recognized start"
        );
        let (normalized, _) = fixture("incoming-unknown-boundary", &[text]);
        let sources = whole_clauses::contract_sources(&normalized);
        assert!(sources[0].boundary_uncertain);
        assert!(!sources[1].boundary_uncertain);
    }

    #[test]
    fn short_list_bodies_are_continuations() {
        let cases=[
            ("1. Insurance\n1.1 Maintain coverage.\nc. General coverage applies only to workers\nwhile on site.\n2. Other. Other obligations.",ContractTermCategory::Insurance),
            ("1. Liability\n1.1 Cover losses.\ng. Contractor shall maintain coverage for all workers\nwhile on site.\n2. Other. Other obligations.",ContractTermCategory::LiabilityIndemnity),
            ("1. Payment\n1.1 Pay for services.\ni. all workers must maintain current insurance\nwhile on site.\n2. Other. Other obligations.",ContractTermCategory::Payment),
        ];
        let failures = cases
            .into_iter()
            .enumerate()
            .filter_map(|(i, (text, category))| {
                selected(&output("short-list-body", &[text]).0, category)
                    .is_empty()
                    .then_some(i)
            })
            .collect::<Vec<_>>();
        assert!(
            failures.is_empty(),
            "short list bodies falsely marked uncertain: {failures:?}"
        );
    }

    #[test]
    fn classifier_bounds_and_unknown_markers() {
        for boundary in [
            "III OTHER",
            "III.",
            "iii. Other",
            "iii\tOther",
            "A. Other",
            "SECTION Other",
            "SCHEDULE Other",
            "ATTACHMENT Other",
            "ANNEX Other",
            "2.1 Payment ........ 3",
            "2.1 Payment 3",
            "II. PAYMENT 3",
            "OTHER EXTRA SECTION TITLE WITH EIGHT TOTAL WORDS",
        ] {
            let text = format!("1. Payment\n1.1 Pay after acceptance.\n{boundary}\nOrdinary prose.\n3. Other. Other obligations.");
            assert!(
                selected(
                    &output("unknown-boundary", &[&text]).0,
                    ContractTermCategory::Payment
                )
                .is_empty(),
                "unknown boundary passed: {boundary}"
            );
        }
        for continuation in [
            "OTHER EXTRA SECTION TITLE WITH NINE TOTAL WORDS HERE",
            "OTHER TERMS APPLY.",
            "Name/Title:",
            "Representative:",
            "Civil liability continues.",
        ] {
            let text = format!("1. Payment\n1.1 Pay after acceptance.\n{continuation}\nOrdinary prose.\n3. Other. Other obligations.");
            assert!(
                !selected(
                    &output("body-continuation", &[&text]).0,
                    ContractTermCategory::Payment
                )
                .is_empty(),
                "body falsely rejected: {continuation}"
            );
        }
        let wrapped = "1. Payment\n1.1 Pay after acceptance of the\nOTHER MATERIAL\nby the client.\n3. Other. Other obligations.";
        assert!(!selected(
            &output("wrapped-caps", &[wrapped]).0,
            ContractTermCategory::Payment
        )
        .is_empty());
    }

    #[test]
    fn boundary_layouts_follow_versioned_admission() {
        for (layout, text) in [
            ("tab-roman", "2. Payment\n2.1 Pay after acceptance.\nIII.\tOTHER\n3.1 Other obligations."),
            ("prose-roman", "2. Payment\n2.1 Pay after acceptance.\nIII. OTHER\nOther obligations."),
            ("split-article", "2. Payment\n2.1 Pay after acceptance.\nARTICLE III\nOTHER\n3.1 Other obligations."),
            ("toc", "TABLE OF CONTENTS\n2. Payment\n2.1 Terms 3\n3. Other 4"),
            ("appendix", "2. Payment. Pay after acceptance.\nAPPENDIX A\nI. Services\nOther obligations.\n3. Other. Excluded."),
        ] {
            let extraction = output(layout, &[text]).0;
            assert_eq!(selected(&extraction, ContractTermCategory::Payment).is_empty(), layout != "split-article", "boundary result: {layout}");
            if layout == "split-article" {
                let (normalized, chunked) = fixture(layout, &[text]);
                for version in [PREVIOUS_VERSION, PRE_HEADING_VERSION] {
                    assert!(selected(&records(&chunked, &normalized, version).unwrap().0, ContractTermCategory::Payment).is_empty());
                }
                assert!(selected(&extraction, ContractTermCategory::Payment).iter().all(|c| !c.text.contains("OTHER")));
            }
            assert!(!extraction.clauses.is_empty());
        }
        for text in ["2. Payment\n2.1 Pay after acceptance.\n3. Other. Excluded.", "ARTICLE II - PAYMENT\n2.1 Pay after acceptance.\nARTICLE III - OTHER\n3.1 Other obligations."] {
            assert!(!selected(&output("clean", &[text]).0, ContractTermCategory::Payment).is_empty());
        }
    }

    #[test]
    fn uncertain_boundaries_cover_all_affected_term_categories() {
        for (category, heading) in [
            (ContractTermCategory::Payment, "Payment"),
            (ContractTermCategory::TermRenewal, "Term and Renewal"),
            (ContractTermCategory::Termination, "Termination"),
            (
                ContractTermCategory::LiabilityIndemnity,
                "Liability and Indemnity",
            ),
        ] {
            for boundary in [
                "III.\tOTHER",
                "III. Other",
                "ARTICLE III\nOTHER",
                "APPENDIX A\nI. Services",
            ] {
                let text = format!(
                    "2. {heading}\n2.1 Operative obligation.\n{boundary}\n3.1 Other obligations."
                );
                let extraction = output("category-boundary", &[&text]).0;
                assert_eq!(
                    selected(&extraction, category).is_empty(),
                    boundary != "ARTICLE III\nOTHER"
                );
                if boundary == "ARTICLE III\nOTHER" {
                    assert!(selected(&extraction, category)
                        .iter()
                        .all(|c| !c.text.contains("OTHER")));
                    let (normalized, chunked) = fixture("category-boundary", &[&text]);
                    for version in [PREVIOUS_VERSION, PRE_HEADING_VERSION] {
                        assert!(selected(
                            &records(&chunked, &normalized, version).unwrap().0,
                            category
                        )
                        .is_empty());
                    }
                }
            }
        }
        let text = "Agreement between Alpha and Beta.\nTABLE OF CONTENTS\n1. Services\n1.1 Services 3\n2. Payment 4";
        assert!(selected(
            &output("opening-toc", &[text]).0,
            ContractTermCategory::Parties
        )
        .is_empty());
    }

    #[test]
    fn cached_roman_lookahead_keeps_inventory_admission() {
        let text = "2. Payment. Pay as agreed.\nIII. OTHER\n§ 3.1 Other obligations.";
        let extraction = output("section-symbol-lookahead", &[text]).0;
        assert_eq!(extraction.clauses.len(), 2);
        assert!(extraction.clauses[0].text.ends_with("III. OTHER"));
        assert!(selected(&extraction, ContractTermCategory::Payment).is_empty());
    }

    #[test]
    fn uncertainty_abstains_the_whole_category_and_preserves_clean_siblings() {
        let text = "1. Payment. Clean obligation.\n2. Other. Other obligation.\n3. Payment. Uncertain obligation.\nAPPENDIX A\nI. Services\nUnnumbered prose.\n4. Insurance. Maintain coverage.";
        let (normalized, _) = fixture("mixed-boundary", &[text]);
        let sources = whole_clauses::contract_sources(&normalized);
        assert!(sources.iter().any(|source| source.boundary_uncertain));
        let extraction = output("mixed-boundary", &[text]).0;
        assert!(selected(&extraction, ContractTermCategory::Payment).is_empty());
        assert_eq!(
            selected(&extraction, ContractTermCategory::Insurance).len(),
            1
        );
    }

    #[test]
    fn bare_roman_sections_stop_selected_extent() {
        let text = "2. Payment\n2.1 Client shall pay.\nIII. TERM AND TERMINATION\n3.1 Either party may terminate.\nIV. GENERAL PROVISIONS\n4.1 Notices shall be written.";
        let extraction = output("roman-boundaries", &[text]).0;
        assert_eq!(
            selected(&extraction, ContractTermCategory::Payment).len(),
            2
        );
        assert_eq!(
            selected(&extraction, ContractTermCategory::Termination).len(),
            2
        );
        assert_eq!(extraction.clauses.len(), 6);
        assert!(extraction
            .clauses
            .iter()
            .any(|c| c.text == "III. TERM AND TERMINATION"));
    }

    #[test]
    fn bare_roman_first_section_preserves_opening_parties() {
        let text = "Agreement between Alpha and Beta.\nI. SERVICES\n1.1 Services shall be delivered.\nII. OTHER\n2.1 Other terms apply.";
        let extraction = output("roman-opening", &[text]).0;
        assert_eq!(
            selected(&extraction, ContractTermCategory::Parties).len(),
            1
        );
        assert_eq!(
            extraction.clauses[0].text,
            "Agreement between Alpha and Beta."
        );
    }

    #[test]
    fn bare_roman_sequence_closes_before_unnumbered_prose() {
        for (heading, accepted) in [
            ("III. GENERAL", true),
            ("IV. GENERAL", false),
            ("II. GENERAL", false),
            ("III. General", false),
        ] {
            let text =
                format!("II. PAYMENT\n2.1 Client shall pay.\n{heading}\nWritten notices apply.");
            let (normalized, _) = fixture("roman-sequence", &[&text]);
            let clauses = whole_clauses::contract_sources(&normalized);
            assert_eq!(
                clauses.iter().filter(|c| c.article.is_some()).count(),
                if accepted { 2 } else { 1 },
                "sequence boundary: {heading}"
            );
        }
    }

    #[test]
    fn bare_roman_admission_boundaries() {
        for (heading, next, accepted) in [
            ("I. PARTIES", "1.1 Named parties sign.", true),
            ("MMMCMXCIX. PAYMENT", "3999.1 Pay.", true),
            ("IIII. PAYMENT", "4.1 Pay.", false),
            ("MMMM. PAYMENT", "4000.1 Pay.", false),
            ("III. Payment", "3.1 Pay.", false),
            ("III. PAYMENT", "4.1 Pay.", false),
            ("III. 123", "3.1 Pay.", false),
            ("III. PAYMENT", "", false),
            ("I. shall apply", "1.1 Pay.", false),
        ] {
            let (normalized, _) = fixture("roman-admission", &[heading, next]);
            let clauses = whole_clauses::contract_sources(&normalized);
            assert_eq!(
                clauses.iter().any(|c| c.article.is_some()),
                accepted,
                "{heading}: {next}"
            );
            assert!(clauses.iter().flat_map(|c| &c.fragments).any(|f| normalized
                .pages
                .iter()
                .flat_map(|p| &p.content)
                .any(|b| b.block_id == f.block_id && b.text[f.start..f.end].contains(heading))));
        }
    }

    #[test]
    fn revised_policy_whitespace_article_requires_uppercase_keyword_and_title() {
        use ContractTermCategory::Payment;
        for header in [
            "ARTICLE 10    PAYMENTS",
            "ARTICLE X\tPAYMENTS",
            "ARTICLE 10 PAYMENTS:",
        ] {
            let text = format!("{header}\n10.1 Progress. Pay after acceptance.\n10.2 Conditions apply.\nARTICLE 11    OTHER\n11.1 Excluded.");
            let payment = selected(&output("article-spaces", &[&text]).0, Payment)
                .iter()
                .map(|c| c.text.clone())
                .collect::<Vec<_>>();
            assert_eq!(
                payment.len(),
                3,
                "whitespace ARTICLE must select its whole section: {header}"
            );
            assert!(!payment.iter().any(|c| c.contains("Excluded")));
        }
        for header in [
            "Article 5 shall apply",
            "Article 5 PAYMENT",
            "ARTICLE 5 Payment",
            "ARTICLE 5 123",
            "ARTICLE 0 PAYMENTS",
            "ARTICLE 1000 PAYMENTS",
            "ARTICLE IIII PAYMENTS",
        ] {
            let text = format!("{header}\n5.1 Printed obligation.");
            let (normalized, _) = fixture("article-miss", &[&text]);
            assert!(
                whole_clauses::contract_sources(&normalized)
                    .iter()
                    .all(|clause| clause.article.is_none()),
                "rejected text must not open an article: {header}"
            );
            assert!(
                selected(&output("article-miss", &[&text]).0, Payment).is_empty(),
                "{header}"
            );
        }
        let extraction = output(
            "explicit-article",
            &["Article V - Payment\n5.1 Pay after acceptance."],
        )
        .0;
        assert_eq!(selected(&extraction, Payment).len(), 2);
    }

    #[test]
    fn accepted_heading_forms_and_aliases_select_whole_source() {
        use ContractTermCategory::*;
        for (heading, categories) in [
            ("Payment", vec![Payment]),
            ("Pricing", vec![Payment]),
            ("Initial Term", vec![TermRenewal]),
            ("Renewal Term", vec![TermRenewal]),
            ("Hold Harmless", vec![LiabilityIndemnity]),
            ("Limitations of Liability", vec![LiabilityIndemnity]),
            ("Term and Termination", vec![TermRenewal, Termination]),
        ] {
            for text in [format!("5. {heading}. Client shall comply with the printed conditions.\n6. Notices. Notices shall be written."),
                format!("{heading}\n5.1 Client shall comply with the printed conditions.\n6. Notices. Notices shall be written."),
                format!("ARTICLE V - {}\nClient shall comply with the printed conditions.\nARTICLE VI - NOTICES\nNotices shall be written.",heading.to_uppercase())] {
                let (extraction,_,_)=output("forms", &[&text]);
                for category in &categories {
                    let clauses=selected(&extraction,*category);
                    assert!(!clauses.is_empty(), "{heading}: {category:?}");
                    let joined=clauses.iter().map(|c|c.text.as_str()).collect::<Vec<_>>().join("\n");
                    assert!(joined.contains("Client shall comply with the printed conditions."));
                    assert!(!joined.contains("Notices shall"));
                }
                if categories.len()==2 {
                    assert_eq!(selected(&extraction,TermRenewal),selected(&extraction,Termination));
                    assert_eq!(extraction.clauses.iter().map(|c|&c.clause_id).collect::<HashSet<_>>().len(),extraction.clauses.len());
                }
            }
        }
        for separator in ["-", "–", "—", ":"] {
            let text=format!("ARTICLE V {separator} PAYMENT\nClient shall pay.\nARTICLE VI {separator} OTHER\nOther text.");
            assert_eq!(selected(&output("article", &[&text]).0, Payment).len(), 1);
        }
    }

    #[test]
    fn unlisted_and_unsupported_heading_forms_never_gain_body_keyword_matches() {
        for text in [
            "5. Payment history example. Client shall pay.",
            "5. Other. Payment terms are printed here.",
            "5. The Client shall pay $1,000.00 per month.",
            "(a) Payment\nClient shall pay.",
            "payment terms\n5.1 Client shall pay.",
            "5. Payment Client shall pay.",
            "ARTICLE IIII - PAYMENT\nClient shall pay.",
        ] {
            let (extraction, _, _) = output("miss", &[text]);
            assert!(
                extraction.key_terms.iter().all(|t| t.selections.is_empty()),
                "{text}"
            );
            for line in text.lines().filter(|line| !line.trim().is_empty()) {
                assert!(
                    extraction.clauses.iter().any(|c| c.text.contains(line)),
                    "source line was lost: {line}"
                );
            }
        }
    }

    #[test]
    fn parent_selection_includes_all_children_without_peer_or_duplicate_references() {
        let (extraction,_,evidence)=output("hierarchy",&["4. Other. Before the payment section.\nPayment Terms\n5.1 First obligation.\n5.1.1 Condition continues", "after the page break.\n5.2 Payment. Second obligation.\n6. Notices. Excluded."]);
        let payment = selected(&extraction, ContractTermCategory::Payment);
        assert_eq!(payment.len(), 4);
        assert!(!extraction.clauses[0].text.contains("Payment Terms"));
        assert!(payment[0].text.starts_with("Payment Terms"));
        assert!(payment[2].text.contains("after the page break"));
        assert_eq!(
            payment[2]
                .evidence_ids
                .iter()
                .map(|id| evidence
                    .iter()
                    .find(|e| &e.evidence_id == id)
                    .unwrap()
                    .source_span
                    .page_start)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(payment.iter().all(|c| !c.text.contains("Excluded")));
        assert_eq!(extraction.key_terms[1].selections.len(), 1);
        for (body, count) in [
            ("5. PAYMENT\n5.1 First.\n5.2 Second.\n50.1 Excluded.", 3),
            ("5. Other\nPayment Terms\n5.1 First.\n5.2 Excluded.", 1),
            ("5. PAYMENT\n6. Other. Content.", 0),
            ("5. PAYMENT\n5.1 First.\n6. OTHER\n6.1 Second.", 2),
        ] {
            assert_eq!(
                selected(&output("parent", &[body]).0, ContractTermCategory::Payment).len(),
                count
            );
        }
    }

    #[test]
    fn article_scopes_keep_repeated_numbers_and_equal_text_occurrences_distinct() {
        let (extraction, _, _) = output("articles", &["ARTICLE I - PAYMENT\n1. Payment. Client shall pay.\n2. Condition. Approval required.\nARTICLE II - OTHER\n1. Payment. Client shall pay.\n2. Other. Excluded."]);
        let payment = &extraction.key_terms[1].selections;
        assert_eq!(payment.len(), 2);
        assert_eq!(payment[0].clause_ids.len(), 3);
        assert_eq!(payment[1].clause_ids.len(), 1);
        let clauses = selected(&extraction, ContractTermCategory::Payment);
        assert_eq!(clauses[1].text, clauses[3].text);
        assert_ne!(clauses[1].clause_id, clauses[3].clause_id);
        assert!(clauses.iter().all(|c| !c.text.contains("Excluded")));
        for text in [
            "Unlisted Title\n5.1 Printed obligation.",
            "ARTICLE V - UNLISTED\nPrinted obligation.",
        ] {
            assert!(output("unlisted", &[text])
                .0
                .key_terms
                .iter()
                .all(|t| t.selections.is_empty()));
        }
    }

    #[test]
    fn furniture_policy_versions_preserve_saved_contract_artifacts() {
        let pages = (1..=3).map(|n| format!("Exhibit 10.25\n{n}. Payment\nClient shall pay after acceptance.\nPublic Service Agreement Page {n} of 3\nInitials and Date\nPUBLIC COMPANY")).collect::<Vec<_>>();
        let (normalized, chunked) = fixture(
            "versioned-furniture",
            &pages.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        for version in [PREVIOUS_VERSION, PRE_HEADING_VERSION, VERSION] {
            let analyzed =
                analyze_for_version(&chunked, &normalized, &UNCONTROLLED_EXECUTION, version)
                    .unwrap();
            let synthesized =
                synthesize(&analyzed, &chunked, &normalized, &UNCONTROLLED_EXECUTION).unwrap();
            let verified = verify(
                &synthesized,
                &analyzed,
                &chunked,
                &normalized,
                &UNCONTROLLED_EXECUTION,
            )
            .unwrap();
            let saved: VerifiedDocument =
                serde_json::from_str(&serde_json::to_string(&verified).unwrap()).unwrap();
            super::super::validate_verified_document(
                &saved,
                &synthesized,
                &analyzed,
                &chunked,
                &normalized,
            )
            .unwrap();
            assert_eq!(saved.verification_version, version);
            let quotes = analyzed
                .chunks
                .iter()
                .flat_map(|c| &c.evidence)
                .map(|e| e.exact_quote.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(
                quotes.contains("Exhibit 10.25"),
                version == PREVIOUS_VERSION
            );
            assert_eq!(
                quotes.contains("Initials and Date"),
                version == PREVIOUS_VERSION
            );
            assert!(quotes.contains("Client shall pay after acceptance."));
            assert!(
                coherent::furniture_coverage_exclusions(&analyzed.warnings, &normalized).is_empty()
            );
            assert_eq!(
                analyzed
                    .warnings
                    .iter()
                    .any(|w| w.code == "SUMMARY_FURNITURE_PAGES_EXCLUDED"),
                version != PREVIOUS_VERSION
            );
            let mut forged = analyzed;
            forged.analysis_version = if version != PREVIOUS_VERSION {
                PREVIOUS_VERSION
            } else {
                VERSION
            }
            .into();
            assert!(validate_analysis(&forged, &chunked, &normalized).is_err());
        }
    }

    #[test]
    fn contract_source_furniture_disclosure_preserves_delivery_coverage() {
        let (normalized, chunked) = fixture(
            "furniture",
            &["1. Payment. Client shall pay.\nPage 1 of 2", "Page 2 of 2"],
        );
        let analyzed = analyze(&chunked, &normalized, &UNCONTROLLED_EXECUTION).unwrap();
        assert!(analyzed
            .chunks
            .iter()
            .flat_map(|c| &c.evidence)
            .all(|e| !e.exact_quote.contains("Page ")));
        assert!(analyzed.warnings.iter().any(|w| w.code == "SUMMARY_FURNITURE_PAGES_EXCLUDED"), "Contract extraction must disclose the existing source owner's excluded furniture-only page");
        let excluded = coherent::furniture_coverage_exclusions(&analyzed.warnings, &normalized);
        assert_eq!(excluded, HashSet::from([2]));
    }

    #[test]
    fn opening_between_fallback_is_narrow_and_headed_parties_take_precedence() {
        use ContractTermCategory::Parties;
        for marker in [
            "1. Other. Body.",
            "ARTICLE I - OTHER\nBody.",
            "Other Title\n1.1 Body.",
        ] {
            let text = format!("This agreement is BETWEEN two parties.\n{marker}");
            let (extraction, _, _) = output("opening", &[&text]);
            assert_eq!(selected(&extraction, Parties).len(), 1);
            assert_eq!(
                selected(&extraction, Parties)[0].text,
                "This agreement is BETWEEN two parties."
            );
            assert_eq!(
                extraction.key_terms[0].selections[0].rule,
                ContractSelectionRule::OpeningBetween
            );
        }
        for text in [
            "1. Other. Between the periods.",
            "Opening without that word.\n1. Other. Between parties.",
            "An inbetween item.\n1. Other. Body.",
            "Opening between parties.\n2. Other. Body.",
            "Opening between_parties.\n1. Other. Body.",
        ] {
            assert!(selected(&output("opening-miss", &[text]).0, Parties).is_empty());
        }
        let (extraction,_,_)=output("headed",&["Opening between two parties.\n1. Parties. Named entities sign this agreement.\n2. Other. Body."]);
        assert_eq!(selected(&extraction, Parties).len(), 1);
        assert_eq!(
            extraction.key_terms[0].selections[0].rule,
            ContractSelectionRule::Heading
        );
    }

    #[test]
    fn source_reconstruction_rejects_altered_text_missing_children_and_cross_document_ids() {
        let (normalized, chunked) = fixture(
            "mutation",
            &["5. Payment\n5.1 Client shall pay.\n5.2 Conditions apply.\n6. Other. Excluded."],
        );
        let analyzed = analyze(&chunked, &normalized, &UNCONTROLLED_EXECUTION).unwrap();
        let base = synthesize(&analyzed, &chunked, &normalized, &UNCONTROLLED_EXECUTION).unwrap();
        for kind in 0..5 {
            let mut changed = base.clone();
            let extraction = changed.contract_extraction.as_mut().unwrap();
            match kind {
                0 => {
                    extraction.key_terms[1].selections[0].clause_ids.pop();
                }
                1 => extraction.key_terms[1].selections[0]
                    .clause_ids
                    .push(extraction.clauses.last().unwrap().clause_id.clone()),
                2 => {
                    extraction.clauses[1].text = "Invented payment.".into();
                    changed.claims[1].text = "Invented payment.".into();
                }
                3 => {
                    extraction.key_terms[1].selections[0].clause_ids[0] =
                        "other-document-clause".into()
                }
                _ => extraction.key_terms.swap(0, 1),
            }
            assert!(validate_synthesis(&changed, &analyzed, &chunked, &normalized).is_err());
        }
        let first = output("independent-A", &["1. Payment. Client shall pay."]).0;
        let second = output("independent-B", &["1. Payment. Client shall pay."]).0;
        assert_ne!(first.clauses[0].clause_id, second.clauses[0].clause_id);
        assert_eq!(
            first,
            output("independent-A", &["1. Payment. Client shall pay."]).0
        );
    }

    #[test]
    fn atomic_key_term_sections_respect_connect_byte_limits() {
        use crate::connect::contracts::{InputArtifact, JobResult, MAX_SUMMARY_TEXT_BYTES};
        let input = InputArtifact {
            artifact_id: uuid::Uuid::new_v4().to_string(),
            media_type: "application/pdf".into(),
            byte_size: 1,
            sha256: "a".repeat(64),
            display_name: "public.pdf".into(),
            source_app_id: "test".into(),
        };
        let make = |text: &str| {
            let (extraction, synthesized, evidence) = output("cap", &[text]);
            let units = render_units(&extraction, &synthesized.claims, &evidence).unwrap();
            let summary = SummaryArtifact {
                document_id: "cap".into(),
                summary_version: "9.0.0".into(),
                text: synthesized.summary_text,
                contract_extraction: Some(extraction),
                warnings: vec![],
                created_at: Utc::now(),
                integrity_hash: String::new(),
            };
            (summary, units)
        };
        let (small, units) = make("5. Payment\n5.1 Body.");
        let fixed = units[0].text.len();
        for delta in [-1isize, 0, 1] {
            let target = (MAX_SUMMARY_TEXT_BYTES as isize + delta) as usize;
            let padding = target - fixed;
            let text = format!("5. Payment\n5.1 {}Body.", "X".repeat(padding));
            let (summary, units) = make(&text);
            assert_eq!(units[0].text.len(), target);
            assert!(units[0].text.contains("5.1"));
            let result = JobResult::from_summary_claim_lines(
                &input,
                &summary,
                &units.iter().map(|u| u.text.clone()).collect::<Vec<_>>(),
            );
            if delta > 0 {
                assert!(result.is_err());
            } else {
                let (result, count) = result.unwrap();
                assert_eq!(count, 1);
                assert!(result.outputs[0]
                    .content
                    .warnings
                    .iter()
                    .any(|w| w.code == SUMMARY_TRUNCATED_FOR_DELIVERY_WARNING_CODE));
            }
        }
        assert!(small.text.starts_with("Key terms"));
        let text = format!(
            "5. Payment\n5.1 {}Body.",
            "É".repeat(MAX_SUMMARY_TEXT_BYTES / 2)
        );
        let (summary, units) = make(&text);
        assert!(JobResult::from_summary_claim_lines(
            &input,
            &summary,
            &units.iter().map(|u| u.text.clone()).collect::<Vec<_>>()
        )
        .is_err());
    }
    #[test]
    #[ignore = "requires the private saved-source A/B fixture and local output path"]
    fn replay_saved_contract_sources_through_production_extraction() {
        use crate::connect::contracts::{InputArtifact, JobResult};
        let input = std::env::var("DOC_SUM_CONTRACT_REPLAY_INPUT").unwrap();
        let bytes = std::fs::read(input).unwrap();
        let saved: Value = serde_json::from_slice(&bytes).unwrap();
        let mut reports = Vec::new();
        for (index, record) in saved.as_array().unwrap().iter().enumerate() {
            let alias = record["alias"]
                .as_str()
                .unwrap_or(if index == 0 { "A" } else { "B" });
            let normalized: NormalizedDocument =
                serde_json::from_value(record["normalized"].clone()).unwrap();
            let chunked: ChunkedDocument =
                serde_json::from_value(record["chunked"].clone()).unwrap();
            let historical = record
                .get("historical_artifacts")
                .and_then(Value::as_array)
                .map(|items| items.iter().collect::<Vec<_>>())
                .unwrap_or_else(|| {
                    if record.get("analyzed").is_some() {
                        vec![record]
                    } else {
                        vec![]
                    }
                });
            let mut historical_versions = Vec::new();
            for saved in &historical {
                let old_analysis: AnalyzedDocument =
                    serde_json::from_value(saved["analyzed"].clone()).unwrap();
                let old_synthesis: SynthesizedDocument =
                    serde_json::from_value(saved["synthesized"].clone()).unwrap();
                let old_verified: VerifiedDocument =
                    serde_json::from_value(saved["verified"].clone()).unwrap();
                super::super::validate_analyzed_content(&old_analysis, &chunked, &normalized)
                    .unwrap();
                super::super::validate_verified_document(
                    &old_verified,
                    &old_synthesis,
                    &old_analysis,
                    &chunked,
                    &normalized,
                )
                .unwrap();
                historical_versions.push(old_analysis.analysis_version);
            }
            let historical_replay = !historical.is_empty();
            let analyzed = analyze(&chunked, &normalized, &UNCONTROLLED_EXECUTION).unwrap();
            let synthesized =
                synthesize(&analyzed, &chunked, &normalized, &UNCONTROLLED_EXECUTION).unwrap();
            let verified = verify(
                &synthesized,
                &analyzed,
                &chunked,
                &normalized,
                &UNCONTROLLED_EXECUTION,
            )
            .unwrap();
            let mut summary = SummaryArtifact {
                document_id: normalized.document_id.clone(),
                summary_version: "9.0.0".into(),
                text: verified.summary_text.clone(),
                contract_extraction: verified.contract_extraction.clone(),
                warnings: verified.warnings.clone(),
                created_at: Utc::now(),
                integrity_hash: String::new(),
            };
            summary.integrity_hash = summary.calculate_integrity_hash().unwrap();
            let citations = build_citation_artifact(
                &summary,
                &verified,
                &synthesized,
                &analyzed,
                &chunked,
                &normalized,
            )
            .unwrap();
            let lines = render_citation_claim_lines(&citations).unwrap();
            let descriptor = InputArtifact {
                artifact_id: uuid::Uuid::new_v4().to_string(),
                media_type: "application/pdf".into(),
                byte_size: 1,
                sha256: "a".repeat(64),
                display_name: format!("contract-{alias}.pdf"),
                source_app_id: "saved-source-replay".into(),
            };
            let (wire, count) =
                JobResult::from_summary_claim_lines(&descriptor, &summary, &lines).unwrap();
            assert!(delivery_claim_prefix_coverage_satisfied(
                &citations,
                count,
                &analyzed.omissions,
                &normalized,
                &summary.warnings
            ));
            let extraction = summary.contract_extraction.as_ref().unwrap();
            let clauses = whole_clauses::contract_sources(&normalized);
            let blocks = validate_normalized_chunk_boundary(&normalized, &chunked).unwrap();
            assert_eq!(clauses.len(), extraction.clauses.len());
            for (source, clause) in clauses.iter().zip(&extraction.clauses) {
                assert_eq!(source.text(&blocks), clause.text);
            }
            let selected=extraction.key_terms.iter().map(|term| {
                let sections=term.selections.iter().map(|selection| {
                    let selected=selection.clause_ids.iter().map(|id| {
                        let clause=extraction.clauses.iter().find(|c| &c.clause_id==id).unwrap();
                        let pages=clause.evidence_ids.iter().map(|id| citations.evidence.iter().find(|e| &e.evidence_id==id).unwrap().source_span.page_start).collect::<std::collections::BTreeSet<_>>();
                        json!({"clause_id":id,"heading":clause.heading,"pages":pages})
                    }).collect::<Vec<_>>();
                    json!({"rule":selection.rule,"root_clause_id":selection.root_clause_id,"clauses":selected})
                }).collect::<Vec<_>>();
                json!({"category":term.category,"status":if sections.is_empty() {"not identified"} else {"selected"},"sections":sections})
            }).collect::<Vec<_>>();
            let boundaries=clauses.iter().enumerate().map(|(i,c)|json!({"clause_id":extraction.clauses[i].clause_id,"number":c.number,"article":c.article,"parent":c.parent,"headings":c.headings,"opening":c.opening,"boundary_uncertain":c.boundary_uncertain,"fragments":c.fragments.iter().map(|f|json!({"block_id":f.block_id,"start":f.start,"end":f.end})).collect::<Vec<_>>()})).collect::<Vec<_>>();
            reports.push(json!({"alias":alias,"policy":VERSION,"model_calls":0,"historical_replay":historical_replay,"historical_versions":historical_versions,"analyzed":analyzed,"synthesized":synthesized,"verified":verified,"normalized_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&normalized).unwrap())),"chunked_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&chunked).unwrap())),"clauses":extraction.clauses.len(),"categories":selected,"section_boundaries":boundaries,"summary":summary,"citations":citations,"delivered":wire,"delivered_units":count,"total_units":lines.len(),"source_reconstruction":"passed","page_coverage":"passed"}));
            println!("CONTRACT_REPLAY {alias}: {} source clauses, {} of {} render units delivered; source reconstruction and coverage passed",extraction.clauses.len(),count,lines.len());
        }
        let report = json!({"input_sha256":format!("{:x}",Sha256::digest(&bytes)),"input_scope":"saved normalized and chunked sources; Connect descriptor is a public test fixture, not original PDF provenance", "cases":reports});
        std::fs::write(
            std::env::var("DOC_SUM_CONTRACT_REPLAY_OUTPUT").unwrap(),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
}
