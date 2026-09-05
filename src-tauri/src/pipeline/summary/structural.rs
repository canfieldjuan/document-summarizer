//! Model text/group choices become application-owned durable attribution.
use super::*;

fn invalid(message: &str) -> PipelineFailure {
    stage_failure(
        PipelineStage::Synthesize,
        "MODEL_CLAIMS_RESPONSE_INVALID",
        message,
        true,
    )
}

pub(super) fn evidence_response(
    text: &str,
    evidence: &[PromptEvidenceItem],
    bounds: ClaimBounds,
) -> Result<String, PipelineFailure> {
    let raw: RawClaimsResponse = serde_json::from_str(text)
        .map_err(|_| invalid("Synthesis must return text claims and integer assignments only"))?;
    if bounds.minimum == 0
        || bounds.minimum > bounds.maximum
        || bounds.maximum > LEGACY_MAX_SUMMARY_CLAIMS
        || raw.claims.len() < bounds.minimum
        || raw.claims.len() > bounds.maximum
        || raw.assignments.len() != evidence.len()
        || evidence.is_empty()
        || evidence.iter().any(|e| e.evidence_id.is_empty())
        || evidence
            .iter()
            .map(|e| &e.evidence_id)
            .collect::<HashSet<_>>()
            .len()
            != evidence.len()
    {
        return Err(invalid(
            "Claim counts, assignment length and request identities must be valid",
        ));
    }
    let mut claims = raw
        .claims
        .into_iter()
        .map(|claim| AttributedClaim {
            text: claim.text,
            evidence_ids: Vec::new(),
        })
        .collect::<Vec<_>>();
    for (item, index) in evidence.iter().zip(raw.assignments) {
        let claim = claims
            .get_mut(index)
            .ok_or_else(|| invalid("Assignment index is outside the returned claim set"))?;
        claim.evidence_ids.push(item.evidence_id.clone());
    }
    if claims.iter().any(|claim| {
        claim.evidence_ids.is_empty() || claim.evidence_ids.len() > MAX_EVIDENCE_PER_CLAIM
    }) {
        return Err(invalid(
            "Every claim needs a bounded nonempty evidence assignment",
        ));
    }
    serde_json::to_string(&AttributedClaimsResponse { claims })
        .map_err(|_| invalid("Attribution serialization failed"))
}

pub(super) fn candidate_response(
    text: &str,
    candidates: &[SynthesisCandidate],
) -> Result<String, PipelineFailure> {
    let raw: RawCandidateClaimsResponse = serde_json::from_str(text)
        .map_err(|_| invalid("Reduction must return exactly one text-only claim"))?;
    if raw.claims.len() != 1 || candidates.len() != 2 {
        return Err(invalid(
            "Reduction requires exactly two inputs and one output",
        ));
    }
    serde_json::to_string(&AttributedCandidateClaimsResponse {
        claims: raw
            .claims
            .into_iter()
            .map(|claim| AttributedCandidateClaim {
                text: claim.text,
                candidate_ids: candidates.iter().map(|c| c.candidate_id.clone()).collect(),
            })
            .collect(),
    })
    .map_err(|_| invalid("Pair attribution serialization failed"))
}

pub(super) fn validate_pair(
    candidates: &[SynthesisCandidate],
    analyzed: &AnalyzedDocument,
    bounds: ClaimBounds,
) -> Result<(), PipelineFailure> {
    if candidates.len() != 2
        || bounds
            != (ClaimBounds {
                minimum: 1,
                maximum: 1,
            })
    {
        return Err(invalid(
            "Reduction admission requires an exact pair and one claim",
        ));
    }
    // Reuse every durable validator before inference as well as on the result.
    let attributed = candidate_response(r#"{"claims":[{"text":"Pair admission."}]}"#, candidates)?;
    parse_candidate_claims_response(&attributed, candidates, analyzed, 1, 1).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(count: usize) -> Vec<PromptEvidenceItem> {
        (0..count)
            .map(|i| PromptEvidenceItem {
                evidence_id: format!("private-evidence-{i}"),
                claim_text: format!("Record {i} must be retained."),
                exact_quote: format!("Record {i} must be retained."),
            })
            .collect()
    }

    #[test]
    fn assignment_schema_binds_length_and_index_to_each_actual_claim_count() {
        let schema = synthesis_output_schema(1, 8, 8);
        let alternatives = schema["anyOf"].as_array().unwrap();
        assert_eq!(alternatives.len(), 8);
        assert!(schema.get("properties").is_none());
        for (i, branch) in alternatives.iter().enumerate() {
            let k = i + 1;
            assert_eq!(branch["properties"]["claims"]["minItems"], k);
            assert_eq!(branch["properties"]["claims"]["maxItems"], k);
            let assignments = &branch["properties"]["assignments"];
            assert_eq!(assignments["minItems"], 8);
            assert_eq!(assignments["maxItems"], 8);
            assert_eq!(assignments["items"]["type"], "integer");
            let vocabulary = assignments["items"]["enum"].as_array().unwrap();
            assert_eq!(*vocabulary, (0..k).map(|j| json!(j)).collect::<Vec<_>>());
            assert!(!vocabulary.contains(&json!(-1)));
            assert!(!vocabulary.contains(&json!(k)));
            assert_eq!(branch["additionalProperties"], false);
            assert_eq!(
                branch["properties"]["claims"]["items"]["required"],
                json!(["text"])
            );
        }
        assert_eq!(
            synthesis_output_schema(4, 8, 5)["anyOf"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn assignments_reject_short_long_mixed_orphan_empty_and_foreign_inputs() {
        let items = evidence(3);
        let bounds = ClaimBounds {
            minimum: 1,
            maximum: 3,
        };
        let parse = |value: Value| evidence_response(&value.to_string(), &items, bounds);
        for slots in [
            json!([0, 1]),
            json!([0, 1, 0, 1]),
            json!([0, 1, 2]),
            json!([0, 1, -1]),
            json!([0, 1, "0"]),
            json!([0, 1, null]),
            json!([0, 1, false]),
            json!([0, 1, 0.5]),
            json!([0, 0, 0]),
        ] {
            assert!(
                parse(json!({"claims":[{"text":"One."},{"text":"Two."}],"assignments":slots}))
                    .is_err()
            );
        }
        for value in [
            json!({"claims":[],"assignments":[0,0,0]}),
            json!({"claims":[{"text":"One.","evidence_ids":["foreign"]}],"assignments":[0,0,0]}),
            json!({"claims":[{"text":"One."}],"assignments":[0,0,0],"evidence_ids":["other-batch"]}),
        ] {
            assert!(parse(value).is_err());
        }
        for slots in [json!([1, 0, 1]), json!([0, 1, 0])] {
            let decoded: AttributedClaimsResponse = serde_json::from_str(
                &parse(json!({"claims":[{"text":"One."},{"text":"Two."}],"assignments":slots}))
                    .unwrap(),
            )
            .unwrap();
            for (i, item) in items.iter().enumerate() {
                assert!(decoded.claims[slots[i].as_u64().unwrap() as usize]
                    .evidence_ids
                    .contains(&item.evidence_id));
            }
        }
        let mut duplicate = items.clone();
        duplicate[1].evidence_id = duplicate[0].evidence_id.clone();
        assert!(evidence_response(
            r#"{"claims":[{"text":"One."}],"assignments":[0,0,0]}"#,
            &duplicate,
            bounds
        )
        .is_err());
        let many = evidence(MAX_EVIDENCE_PER_CLAIM + 1);
        assert!(evidence_response(
            &json!({"claims":[{"text":"One."}],"assignments":vec![0;many.len()]}).to_string(),
            &many,
            bounds
        )
        .is_err());
    }

    #[test]
    fn reduction_wire_has_no_reference_fields_and_restores_both_candidates() {
        let pair = vec![
            SynthesisCandidate {
                candidate_id: "private-left".into(),
                text: "One.".into(),
                evidence_ids: vec!["evidence-a".into()],
            },
            SynthesisCandidate {
                candidate_id: "private-right".into(),
                text: "Two.".into(),
                evidence_ids: vec!["evidence-b".into()],
            },
        ];
        let decoded: AttributedCandidateClaimsResponse = serde_json::from_str(
            &candidate_response(r#"{"claims":[{"text":"Both."}]}"#, &pair).unwrap(),
        )
        .unwrap();
        assert_eq!(
            decoded.claims[0].candidate_ids,
            vec!["private-left", "private-right"]
        );
        for value in [
            json!({"claims":[]}),
            json!({"claims":[{"text":"One."},{"text":"Two."}]}),
            json!({"claims":[{"text":"Both.","candidate_ids":["c1"]}]}),
            json!({"claims":[{"text":"Both.","candidate_ids":["private-other-batch"]}]}),
        ] {
            assert!(candidate_response(&value.to_string(), &pair).is_err());
        }
        assert!(candidate_response(r#"{"claims":[{"text":"Both."}]}"#, &pair[..1]).is_err());
    }

    #[test]
    #[ignore = "requires configured live Ollama; probes the actual production schema and projection"]
    fn live_structural_schemas_use_primary_transport() {
        use crate::pipeline::{contracts::ModelTransportAttempt, model::OllamaRuntime};
        let runtime = OllamaRuntime::from_environment().unwrap();
        runtime.health().unwrap();
        let items = evidence(8);
        let request = ModelRequest {
            stage: PipelineStage::Synthesize,
            ordinal: 0,
            seed: TEST_SEED,
            system_prompt: SYNTHESIS_SYSTEM_PROMPT.into(),
            user_prompt: serialize_evidence_prompt(&items, 1, 8).unwrap(),
            max_output_tokens: SYNTHESIS_OUTPUT_TOKENS,
            output_format: ModelOutputFormat::JsonSchema {
                name: SYNTHESIS_SCHEMA_NAME.into(),
                schema: synthesis_output_schema(1, 8, 8),
            },
        };
        let response = runtime.generate(&request).unwrap();
        eprintln!(
            "STRUCTURAL_SCHEMA_PROBE response={} metrics={:?}",
            response.text, response.request_attempts
        );
        assert!(response
            .request_attempts
            .iter()
            .all(|a| a.transport_attempt == ModelTransportAttempt::Primary));
        evidence_response(
            &response.text,
            &items,
            ClaimBounds {
                minimum: 1,
                maximum: 8,
            },
        )
        .unwrap();
    }
    const TEST_SEED: u64 = 42;
}
