//! Request-local wire identities. Durable IDs never come from model text.
use super::*;

#[derive(Debug)]
pub(super) struct RequestIds {
    local: Vec<String>,
    durable: Vec<String>,
    stage: PipelineStage,
}

impl RequestIds {
    pub(super) fn new(
        prefix: &str,
        durable: Vec<String>,
        stage: PipelineStage,
    ) -> Result<Self, PipelineFailure> {
        if durable.is_empty()
            || durable.iter().any(|id| id.is_empty())
            || durable.iter().collect::<HashSet<_>>().len() != durable.len()
        {
            return Err(stage_failure(
                stage,
                "MODEL_REQUEST_INVALID",
                "Request identities must be nonempty and unique",
                false,
            ));
        }
        Ok(Self {
            local: (1..=durable.len())
                .map(|i| format!("{prefix}{i}"))
                .collect(),
            durable,
            stage,
        })
    }

    pub(super) fn vocabulary(&self) -> &[String] {
        &self.local
    }

    fn invalid(&self) -> PipelineFailure {
        stage_failure(
            self.stage.clone(),
            if self.stage == PipelineStage::Verify {
                "MODEL_VERIFICATION_RESPONSE_INVALID"
            } else {
                "MODEL_CLAIMS_RESPONSE_INVALID"
            },
            "Model reference is not in this request's identifier vocabulary",
            true,
        )
    }

    pub(super) fn local(&self, durable: &str) -> Result<String, PipelineFailure> {
        self.durable
            .iter()
            .position(|id| id == durable)
            .map(|i| self.local[i].clone())
            .ok_or_else(|| self.invalid())
    }

    fn restore(&self, local: &str) -> Result<String, PipelineFailure> {
        // Exact membership only: e01, wrong prefixes and durable IDs are invalid.
        self.local
            .iter()
            .position(|id| id == local)
            .map(|i| self.durable[i].clone())
            .ok_or_else(|| self.invalid())
    }

    pub(super) fn verdict_response(&self, text: &str) -> Result<String, PipelineFailure> {
        let mut response: RawVerificationResponse =
            serde_json::from_str(text).map_err(|_| self.invalid())?;
        for verdict in &mut response.verdicts {
            verdict.claim_id = self.restore(&verdict.claim_id)?;
        }
        serde_json::to_string(&response).map_err(|_| self.invalid())
    }

    #[cfg(test)]
    pub(super) fn localize_failure(&self, failure: PipelineFailure) -> PipelineFailure {
        self.map_failure(failure, true)
    }

    #[cfg(test)]
    pub(super) fn restore_failure(&self, failure: PipelineFailure) -> PipelineFailure {
        self.map_failure(failure, false)
    }

    #[cfg(test)]
    fn map_failure(&self, failure: PipelineFailure, to_wire: bool) -> PipelineFailure {
        if failure.code != "SYNTHESIS_MISSING_REFERENCES" {
            return failure;
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Missing {
            missing_reference_ids: Vec<String>,
        }
        let translated = serde_json::from_str::<Missing>(&failure.message)
            .map_err(|_| self.invalid())
            .and_then(|missing| {
                missing
                    .missing_reference_ids
                    .iter()
                    .map(|id| {
                        if to_wire {
                            self.local(id)
                        } else {
                            self.restore(id)
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()
            });
        match translated {
            Ok(ids) => repair::missing_references(ids),
            Err(error) => error,
        }
    }
}

pub(super) fn verification_prompt(
    claims: &[PromptVerificationClaim],
) -> Result<(String, RequestIds), PipelineFailure> {
    let ids = RequestIds::new(
        "k",
        claims.iter().map(|c| c.claim_id.clone()).collect(),
        PipelineStage::Verify,
    )?;
    let mut seen = HashSet::new();
    let evidence = RequestIds::new(
        "e",
        claims
            .iter()
            .flat_map(|c| &c.evidence)
            .filter(|e| seen.insert(e.evidence_id.clone()))
            .map(|e| e.evidence_id.clone())
            .collect(),
        PipelineStage::Verify,
    )?;
    let mut wire = claims.to_vec();
    for claim in &mut wire {
        claim.claim_id = ids.local(&claim.claim_id)?;
        for item in &mut claim.evidence {
            item.evidence_id = evidence.local(&item.evidence_id)?;
        }
    }
    let text =
        serde_json::to_string(&VerificationPrompt { claims: wire }).map_err(|_| ids.invalid())?;
    Ok((text, ids))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_vocabulary_rejects_foreign_wrong_kind_and_permissive_number_forms() {
        let ids = RequestIds::new(
            "e",
            vec!["durable-a".into(), "durable-b".into()],
            PipelineStage::Synthesize,
        )
        .unwrap();
        assert_eq!(ids.vocabulary(), &["e1", "e2"]);
        for (local, durable) in [("e2", "durable-b"), ("e1", "durable-a")] {
            assert_eq!(ids.restore(local).unwrap(), durable);
            assert_eq!(ids.local(durable).unwrap(), local);
        }
        for invalid in [
            "",
            "e0",
            "e3",
            "e01",
            "e+1",
            "e1 ",
            "E1",
            "c1",
            "k1",
            "durable-a",
        ] {
            assert!(!ids.vocabulary().contains(&invalid.to_string()));
            assert!(ids.restore(invalid).is_err());
        }
        assert!(RequestIds::new("e", vec![], PipelineStage::Synthesize).is_err());
        assert!(
            RequestIds::new("e", vec!["a".into(), "a".into()], PipelineStage::Synthesize).is_err()
        );
        let other =
            RequestIds::new("e", vec!["other-batch".into()], PipelineStage::Synthesize).unwrap();
        assert_eq!(other.restore("e1").unwrap(), "other-batch");
        assert!(other.restore("e2").is_err());
        assert!(other.restore("durable-a").is_err());
    }

    #[test]
    fn verifier_wire_has_consistent_evidence_ordinals_and_exact_size() {
        let claims = vec![
            PromptVerificationClaim {
                claim_id: "claim-private-a".into(),
                text: "First.".into(),
                evidence: vec![PromptVerificationEvidence {
                    evidence_id: "evidence-private-a".into(),
                    exact_quote: "Shared quotation.".into(),
                }],
            },
            PromptVerificationClaim {
                claim_id: "claim-private-b".into(),
                text: "Second.".into(),
                evidence: vec![
                    PromptVerificationEvidence {
                        evidence_id: "evidence-private-a".into(),
                        exact_quote: "Shared quotation.".into(),
                    },
                    PromptVerificationEvidence {
                        evidence_id: "evidence-private-b".into(),
                        exact_quote: "Other quotation.".into(),
                    },
                ],
            },
        ];
        let (serialized, ids) = verification_prompt(&claims).unwrap();
        assert!(!serialized.contains("private"));
        let wire: VerificationPrompt = serde_json::from_str(&serialized).unwrap();
        assert_eq!(wire.claims[0].claim_id, "k1");
        assert_eq!(wire.claims[1].claim_id, "k2");
        assert_eq!(wire.claims[0].evidence[0].evidence_id, "e1");
        assert_eq!(wire.claims[1].evidence[0].evidence_id, "e1");
        assert_eq!(wire.claims[1].evidence[1].evidence_id, "e2");
        let schema = verification_output_schema(ids.vocabulary());
        assert_eq!(
            schema["properties"]["verdicts"]["items"]["properties"]["claim_id"]["enum"],
            json!(["k1", "k2"])
        );
        let durable = claims
            .iter()
            .map(|c| CitedClaim {
                claim_id: c.claim_id.clone(),
                text: c.text.clone(),
                evidence_ids: c.evidence.iter().map(|e| e.evidence_id.clone()).collect(),
            })
            .collect::<Vec<_>>();
        let restored = ids.verdict_response(r#"{"verdicts":[{"claim_id":"k2","verdict":"ambiguous"},{"claim_id":"k1","verdict":"supported"}]}"#).unwrap();
        let verdicts = parse_verification_response(&restored, &durable).unwrap();
        assert_eq!(verdicts[0].claim_id, "claim-private-a");
        assert_eq!(verdicts[0].verdict, ClaimVerdict::Supported);
        assert_eq!(verdicts[1].claim_id, "claim-private-b");
        assert_eq!(verdicts[1].verdict, ClaimVerdict::Ambiguous);
        for raw in [
            r#"{"verdicts":[]}"#,
            r#"{"verdicts":[{"claim_id":"k1","verdict":"supported"}]}"#,
            r#"{"verdicts":[{"claim_id":"k1","verdict":"supported"},{"claim_id":"k1","verdict":"supported"}]}"#,
            r#"{"verdicts":[{"claim_id":"k1","verdict":"supported"},{"claim_id":"k3","verdict":"supported"}]}"#,
        ] {
            assert!(ids
                .verdict_response(raw)
                .and_then(|s| parse_verification_response(&s, &durable))
                .is_err());
        }
        let batch = materialize_verification_batch(claims, durable, 10_752).unwrap();
        assert_eq!(batch.user_prompt, serialized);
        let exact = VERIFICATION_SYSTEM_PROMPT.chars().count() + serialized.chars().count();
        assert_eq!(batch.model_facing_characters, exact);
        assert!(verification_request_within_bounds(2, exact, exact));
        assert!(!verification_request_within_bounds(2, exact, exact - 1));
    }
}
