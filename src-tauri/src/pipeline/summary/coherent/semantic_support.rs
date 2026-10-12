//! Deterministic high-precision checks that can only downgrade a coherent-summary
//! model verdict when a source relationship changes mechanically.
use super::*;

#[cfg(test)]
mod relation_guard_tests {
    use super::*;
    use serde_json::Value;

    fn evidence(text: &str) -> EvidenceItem {
        EvidenceItem {
            evidence_id: "relation-e1".into(),
            chunk_id: "relation-c1".into(),
            block_id: "relation-b1".into(),
            claim_text: text.into(),
            exact_quote: text.into(),
            source_span: SourceSpan {
                page_start: 1,
                page_end: 1,
                section_id: None,
                source_type: crate::pipeline::contracts::SourceType::NativeText,
            },
        }
    }

    #[test]
    #[ignore = "offline private trace; explicit packet path required"]
    fn compressed_actor_owner_trace() {
        let path = std::env::var("DOC_SUM_GUARD_TRACE_PACKET").unwrap();
        let packet: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for row in packet["changed_claims"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| {
                row["alias"] == "ART0051"
                    && (row["previous_independent_expected_supported"] == true
                        || row["new_flip"] == true)
            })
        {
            println!("OWNER_TRACE claim_id={}", row["claim"]["claim_id"]);
            for (label, text) in [
                ("claim", row["claim"]["text"].as_str().unwrap()),
                (
                    "source",
                    row["owned_evidence"][0]["exact_quote"].as_str().unwrap(),
                ),
            ] {
                for clause in semantic_clauses(text) {
                    println!(
                        "OWNER_TRACE {label} clause={clause:?} relations={:?} permission={:?}",
                        evaluation_relations(&clause, EVALUATIVE_CONCEPTS[1]),
                        permission_propositions(&clause)
                    );
                }
            }
        }
    }

    #[test]
    fn approved_relations_preserve_meaning() {
        let fixtures: Value =
            serde_json::from_str(include_str!("relation_guard_fixtures.json")).unwrap();
        let mut wrong_approvals = Vec::new();
        let mut faithful_rejections = Vec::new();
        for row in fixtures["cases"].as_array().unwrap() {
            let name = row["case"].as_str().unwrap();
            let item = evidence(row["source"].as_str().unwrap());
            let claim = CitedClaim {
                claim_id: "relation-k1".into(),
                text: row["claim"].as_str().unwrap().into(),
                evidence_ids: vec![item.evidence_id.clone()],
            };
            let mut verification = [ClaimVerification {
                claim_id: claim.claim_id.clone(),
                evidence_ids: claim.evidence_ids.clone(),
                verdict: ClaimVerdict::Supported,
            }];
            apply_semantic_fidelity_guards(&[claim], &[item], &mut verification).unwrap();
            let supported = verification[0].verdict == ClaimVerdict::Supported;
            let expected = row["expected_supported"].as_bool().unwrap();
            println!(
                "RELATION_REGRESSION case={name} expected_supported={expected} verdict={:?}",
                verification[0].verdict
            );
            if supported && !expected {
                wrong_approvals.push(name);
            } else if !supported && expected {
                faithful_rejections.push(name);
            }
        }
        assert!(
            faithful_rejections.is_empty(),
            "faithful controls rejected: {faithful_rejections:?}"
        );
        assert!(
            wrong_approvals.is_empty(),
            "false approvals: {wrong_approvals:?}"
        );
    }

    #[test]
    fn form_boundaries_preserve_extent_polarity_and_ownership() {
        let supported = |source: &str, claim_text: &str| {
            let item = evidence(source);
            let claim = CitedClaim {
                claim_id: "boundary".into(),
                text: claim_text.into(),
                evidence_ids: vec![item.evidence_id.clone()],
            };
            let mut verdict = [ClaimVerification {
                claim_id: claim.claim_id.clone(),
                evidence_ids: claim.evidence_ids.clone(),
                verdict: ClaimVerdict::Supported,
            }];
            apply_semantic_fidelity_guards(&[claim], &[item], &mut verdict).unwrap();
            verdict[0].verdict == ClaimVerdict::Supported
        };
        for (source,claim,expected) in [
            ("The supplier guarantees to deliver ¾ of the inventory.", "The supplier guarantees delivering fraction3over4 of the inventory.", false),
            ("Ages 14 and 15 may work outside school hours.", "The rules permit ages integers14through15 to work outside school hours.", false),
            ("The supplier guarantees to deliver 0/4 of the inventory.", "The supplier guarantees delivering zero quarters of the inventory.", true),
            ("The supplier guarantees to deliver 4/4 of the inventory.", "The supplier guarantees delivering four quarters of the inventory.", true),
            ("The supplier guarantees to deliver 4/4 of the inventory.", "The supplier guarantees delivering three quarters of the inventory.", false),
            ("The supplier guarantees to deliver 3/0 of the inventory.", "The supplier guarantees delivering three quarters of the inventory.", false),
            ("The supplier guarantees to deliver 18446744073709551616/4 of the inventory.", "The supplier guarantees delivering three quarters of the inventory.", false),
            ("The supplier guarantees to deliver 18446744073709551615/18446744073709551615 of the inventory.", "The supplier guarantees delivering four quarters of the inventory.", true),
            ("The supplier guarantees to deliver ¾ of the inventory.", "The supplier does not guarantee delivering three quarters of the inventory.", false),
            ("The supplier guarantees to deliver ¾ of the inventory.", "The customer guarantees delivering three quarters of the inventory.", false),
            ("The supplier guarantees to deliver the inventory.", "The supplier ensures delivering the inventory.", false),
            ("The supplier guarantees that pumping stops.", "The supplier guarantees that pumps stop.", false),
            ("Ages 0 and 1 may work outside school hours.", "The rules permit ages 0-1 to work outside school hours.", true),
            ("Ages 0 and 1 may work outside school hours.", "The rules permit ages 0-2 to work outside school hours.", false),
            ("Ages 14 and 15 and 17 may work outside school hours.", "The rules permit ages 14-17 to work outside school hours.", false),
            ("Ages 15 and 14 may work outside school hours.", "The rules permit ages 14-15 to work outside school hours.", false),
            ("Ages 18446744073709551614 and 18446744073709551615 may work outside school hours.", "The rules permit ages 18446744073709551614-18446744073709551615 to work outside school hours.", true),
            ("Ages 14 and 15 may work outside school hours.", "The rules permit ages 14-15 to work at any time.", false),
            ("Students may work on farms employing their parent(s).", "The rules permit students to work on farms employing their parents without consent.", false),
            ("Failure to deliver would require the supplier to refund the payment.", "Failure to deliver requires the supplier to withhold the payment.", false),
            ("The supplier would refund the payment if the customer agreed.", "The supplier would refund the payment if the customer agreed.", true),
            ("The official checks records.", "The tribunal guarantees accuracy.", false),
        ] {
            assert_eq!(supported(source,claim),expected,"boundary source={source:?} claim={claim:?}");
        }
        for qualifier in [
            "only if the parties agree",
            "only when notice is given",
            "unless notice is given",
            "until notice is given",
            "provided notice is given",
            "so long as notice is given",
        ] {
            let source = format!("The term shall be from June to August, but {qualifier}.");
            let claim = format!("The term is from June to August, but {qualifier}.");
            assert_eq!(
                claim_propositions(&claim).count(),
                1,
                "qualifying conjunct must stay attached: {qualifier}"
            );
            assert!(
                supported(&source, &claim),
                "unchanged qualifier must remain supported: {qualifier}"
            );
            assert!(
                !supported("The term shall be from June to August.", &claim),
                "invented qualifier must be withheld: {qualifier}"
            );
        }
        for text in [
            "",
            "0",
            "false",
            "unknown",
            "3/0",
            "18446744073709551616/4",
            "(s)",
        ] {
            assert_ne!(
                relation_form_words(text),
                relation_form_words("¾"),
                "no falsy/unknown fraction defaults"
            );
        }
        let left = evidence("The supplier guarantees to deliver ¾ of the inventory.");
        let mut right = evidence("The customer guarantees to deliver ¾ of the inventory.");
        right.evidence_id = "uncited".into();
        let claim = CitedClaim {
            claim_id: "ownership".into(),
            text: "The customer guarantees delivering three quarters of the inventory.".into(),
            evidence_ids: vec![left.evidence_id.clone()],
        };
        let mut verdict = [ClaimVerification {
            claim_id: claim.claim_id.clone(),
            evidence_ids: claim.evidence_ids.clone(),
            verdict: ClaimVerdict::Supported,
        }];
        apply_semantic_fidelity_guards(&[claim], &[left, right], &mut verdict).unwrap();
        assert_eq!(
            verdict[0].verdict,
            ClaimVerdict::Unsupported,
            "uncited form match cannot supply evidence"
        );
    }

    #[test]
    fn approved_forms_preserve_final_verdicts() {
        let fixtures: Value =
            serde_json::from_str(include_str!("relation_guard_form_fixtures.json")).unwrap();
        let mut failures = Vec::new();
        for row in fixtures["cases"].as_array().unwrap() {
            let name = row["case"].as_str().unwrap();
            let item = evidence(row["source"].as_str().unwrap());
            let claim = CitedClaim {
                claim_id: "form-k1".into(),
                text: row["claim"].as_str().unwrap().into(),
                evidence_ids: vec![item.evidence_id.clone()],
            };
            let mut verification = [ClaimVerification {
                claim_id: claim.claim_id.clone(),
                evidence_ids: claim.evidence_ids.clone(),
                verdict: ClaimVerdict::Supported,
            }];
            apply_semantic_fidelity_guards(
                &[claim],
                std::slice::from_ref(&item),
                &mut verification,
            )
            .unwrap();
            let expected = row["expected_supported"].as_bool().unwrap();
            println!("FORM_REGRESSION case={name} expected_supported={expected} verdict={:?} reasons={:?}",
                verification[0].verdict, relation_guard_rejection_reasons(row["claim"].as_str().unwrap(), &[&item]));
            if expected != (verification[0].verdict == ClaimVerdict::Supported) {
                println!(
                    "FORM_ORIGIN_TRACE case={name} propositions={:?}",
                    claim_propositions(row["claim"].as_str().unwrap())
                        .map(|clause| {
                            let relations = evaluation_relations(&clause, EVALUATIVE_CONCEPTS[1]);
                            (clause, relations)
                        })
                        .collect::<Vec<_>>()
                );
                failures.push(name);
            }
        }
        assert!(
            failures.is_empty(),
            "form final verdict mismatch: {failures:?}"
        );
    }

    #[test]
    fn permission_qualifier_fixtures_preserve_final_verdicts() {
        let fixtures: Value =
            serde_json::from_str(include_str!("relation_guard_permission_fixtures.json")).unwrap();
        let mut failures = Vec::new();
        let mut admitted_controls = 0;
        for row in fixtures["cases"].as_array().unwrap() {
            let name = row["case"].as_str().unwrap();
            if row["verification_seam"] == "production_admission" {
                assert_eq!(name, "empty_evidence_twin");
                continue; // Exercised by the real Verify preflight regression.
            }
            admitted_controls += 1;
            let mut items = row["owned_evidence"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    let mut item = evidence(text.as_str().unwrap());
                    item.evidence_id = format!("permission-e{i}");
                    item
                })
                .collect::<Vec<_>>();
            let ids = items
                .iter()
                .map(|item| item.evidence_id.clone())
                .collect::<Vec<_>>();
            if let Some(extra) = row["uncited_evidence"].as_array() {
                for (i, text) in extra.iter().enumerate() {
                    let mut item = evidence(text.as_str().unwrap());
                    item.evidence_id = format!("uncited-e{i}");
                    items.push(item);
                }
            }
            let claim = CitedClaim {
                claim_id: "permission-k1".into(),
                text: row["claim"].as_str().unwrap().into(),
                evidence_ids: ids.clone(),
            };
            let mut verdict = [ClaimVerification {
                claim_id: claim.claim_id.clone(),
                evidence_ids: ids,
                verdict: ClaimVerdict::Supported,
            }];
            apply_semantic_fidelity_guards(&[claim], &items, &mut verdict).unwrap();
            let expected = row["expected_supported"].as_bool().unwrap();
            println!(
                "PERMISSION_REGRESSION case={name} expected_supported={expected} verdict={:?}",
                verdict[0].verdict
            );
            if expected != (verdict[0].verdict == ClaimVerdict::Supported) {
                failures.push(name);
                println!(
                    "PERMISSION_ORIGIN source={:?} claim={:?}",
                    items
                        .iter()
                        .map(|item| permission_propositions(&item.exact_quote))
                        .collect::<Vec<_>>(),
                    permission_propositions(row["claim"].as_str().unwrap())
                );
            }
        }
        assert_eq!(admitted_controls, 36);
        assert!(
            failures.is_empty(),
            "permission final verdict mismatch: {failures:?}"
        );
    }

    #[test]
    fn permission_grant_diagnostics_report_actual_final_verdicts() {
        let fixtures: Value =
            serde_json::from_str(include_str!("relation_guard_permission_fixtures.json")).unwrap();
        for row in fixtures["diagnostics"].as_array().unwrap() {
            let item = evidence(row["owned_evidence"][0].as_str().unwrap());
            assert!(!item.exact_quote.trim().is_empty());
            let claim = CitedClaim {
                claim_id: row["case"].as_str().unwrap().into(),
                text: row["claim"].as_str().unwrap().into(),
                evidence_ids: vec![item.evidence_id.clone()],
            };
            let mut verdict = [ClaimVerification {
                claim_id: claim.claim_id.clone(),
                evidence_ids: claim.evidence_ids.clone(),
                verdict: ClaimVerdict::Supported,
            }];
            apply_semantic_fidelity_guards(&[claim], &[item], &mut verdict).unwrap();
            println!(
                "PERMISSION_DIAGNOSTIC case={} verdict={:?}",
                row["case"], verdict[0].verdict
            );
            if row["gate"] == "must_withhold" {
                assert_ne!(verdict[0].verdict, ClaimVerdict::Supported,
                    "STOP: the obligation diagnostic must withhold through the existing modal owner");
            }
        }
    }

    #[test]
    fn approved_proposition_relations_preserve_final_verdicts() {
        let fixtures: Value =
            serde_json::from_str(include_str!("relation_guard_proposition_fixtures.json")).unwrap();
        let mut failures = Vec::new();
        for row in fixtures["cases"].as_array().unwrap() {
            let name = row["case"].as_str().unwrap();
            let item = evidence(row["source"].as_str().unwrap());
            let claim = CitedClaim {
                claim_id: "proposition-k1".into(),
                text: row["claim"].as_str().unwrap().into(),
                evidence_ids: vec![item.evidence_id.clone()],
            };
            let mut verification = [ClaimVerification {
                claim_id: claim.claim_id.clone(),
                evidence_ids: claim.evidence_ids.clone(),
                verdict: ClaimVerdict::Supported,
            }];
            apply_semantic_fidelity_guards(&[claim], &[item], &mut verification).unwrap();
            let expected = row["expected_supported"].as_bool().unwrap();
            println!(
                "PROPOSITION_REGRESSION case={name} expected_supported={expected} verdict={:?}",
                verification[0].verdict
            );
            if expected != (verification[0].verdict == ClaimVerdict::Supported) {
                failures.push(name);
            }
        }
        assert!(
            failures.is_empty(),
            "proposition final verdict mismatch: {failures:?}"
        );
    }

    #[test]
    fn approved_precision_relations_preserve_final_verdicts() {
        let fixtures: Value =
            serde_json::from_str(include_str!("relation_guard_precision_fixtures.json")).unwrap();
        let mut wrong_approvals = Vec::new();
        let mut faithful_rejections = Vec::new();
        for row in fixtures["cases"].as_array().unwrap() {
            let name = row["case"].as_str().unwrap();
            let item = evidence(row["source"].as_str().unwrap());
            let claim = CitedClaim {
                claim_id: "relation-k1".into(),
                text: row["claim"].as_str().unwrap().into(),
                evidence_ids: vec![item.evidence_id.clone()],
            };
            let mut verification = [ClaimVerification {
                claim_id: claim.claim_id.clone(),
                evidence_ids: claim.evidence_ids.clone(),
                verdict: ClaimVerdict::Supported,
            }];
            apply_semantic_fidelity_guards(&[claim], &[item], &mut verification).unwrap();
            let supported = verification[0].verdict == ClaimVerdict::Supported;
            let expected = row["expected_supported"].as_bool().unwrap();
            println!(
                "PRECISION_REGRESSION case={name} expected_supported={expected} verdict={:?}",
                verification[0].verdict
            );
            if !supported && expected {
                println!(
                    "PRECISION_DIAGNOSTIC case={name} reasons={:?}",
                    relation_guard_rejection_reasons(
                        row["claim"].as_str().unwrap(),
                        &[&evidence(row["source"].as_str().unwrap())]
                    )
                );
                if name == "required_necessary" {
                    println!(
                        "NECESSITY_SOURCE {:?}",
                        evaluation_relations(
                            row["source"].as_str().unwrap(),
                            EVALUATIVE_CONCEPTS[0]
                        )
                    );
                    println!(
                        "NECESSITY_CLAIM {:?}",
                        evaluation_relations(
                            row["claim"].as_str().unwrap(),
                            EVALUATIVE_CONCEPTS[0]
                        )
                    );
                }
            }
            if supported && !expected {
                wrong_approvals.push(name);
            } else if !supported && expected {
                faithful_rejections.push(name);
            }
        }
        assert!(
            faithful_rejections.is_empty(),
            "faithful controls rejected: {faithful_rejections:?}"
        );
        assert!(
            wrong_approvals.is_empty(),
            "false approvals: {wrong_approvals:?}"
        );
    }

    #[test]
    fn precision_boundaries_keep_assignment_values_and_evidence_ownership() {
        for (source, claim, supported) in [
            (
                "The term shall be June-August.",
                "The term is from June to August.",
                true,
            ),
            (
                "The term shall be from June through August.",
                "The term is from June through September.",
                false,
            ),
            (
                "The term is from June to August.",
                "The term is from August to June.",
                false,
            ),
            (
                "The term is from June to August.",
                "The term is from june to August.",
                false,
            ),
            ("The category is Basic.", "The category is Premium.", false),
            (
                "The category is not Basic.",
                "The category is Basic.",
                false,
            ),
            (
                "The category is Basic during inspection.",
                "The category is Basic.",
                false,
            ),
            (
                "The category is Basic during inspection.",
                "The category is Basic during inspection.",
                true,
            ),
            (
                "Inspection:\nA panel\nis critical.",
                "A panel is critical.",
                true,
            ),
            ("A panel\n\nis critical.", "A panel is critical.", false),
            ("- A panel\n- is critical.", "A panel is critical.", false),
        ] {
            let item = evidence(source);
            assert_eq!(
                semantic_fidelity_supported(claim, &[&item]),
                supported,
                "{source} -> {claim}"
            );
        }
        let first = evidence("A panel");
        let second = evidence("is critical.");
        assert!(!semantic_fidelity_supported(
            "A panel is critical.",
            &[&first, &second]
        ));
        let first = evidence("The category is Basic.");
        let second = evidence("The category is Premium.");
        assert!(!definition_value_decision("The category is Basic.", &[&first, &second]).supported);
        for text in [
            "",
            "The operator is present during inspection.",
            "The term shall be extended by written notice.",
            "The contractor is required to supply services.",
            "The operator is assigned to health services.",
        ] {
            assert!(
                definition_relation(text.trim_end_matches('.')).is_none(),
                "{text}"
            );
            let item = evidence(text);
            let decision = definition_value_decision(text, &[&item]);
            assert!(
                decision.supported && decision.released_modals.is_empty(),
                "{text}"
            );
        }
        let required = evidence("The operator must be from June through August.");
        assert!(!semantic_fidelity_supported(
            "The operator is from June through August.",
            &[&required]
        ));
        let parsed = semantic_clauses(
            "A panel\r\nis critical.\r\n\r\n- Panel B is safe.\r\n- Panel C is healthy.",
        )
        .collect::<Vec<_>>();
        assert_eq!(
            parsed,
            [
                "A panel is critical",
                "Panel B is safe",
                "Panel C is healthy"
            ]
        );
    }

    #[test]
    fn relation_boundaries_preserve_polarity_and_do_not_join_evidence() {
        let valve = evidence("Closing the valve stops water flow.");
        let mut other = evidence("Opening the valve ensures that alarms sound.");
        other.evidence_id = "relation-e2".into();
        let sources = [&valve, &other];
        assert!(evaluative_conclusions_supported(
            "Closing the valve ensures that water flow stops.",
            &sources
        ));
        for claim in [
            "Closing the valve ensures that alarms sound.",
            "Closing the valve ensures that water flow does not stop.",
            "Closing the valve ensures.",
            "It ensures that water flow stops.",
            "Opening the valve ensures that alarms sound. It ensures that water flow stops.",
        ] {
            assert!(
                !evaluative_conclusions_supported(claim, &sources),
                "{claim}"
            );
        }
        let observation = evidence("The operator sees water flow.");
        assert!(!evaluative_conclusions_supported(
            "The operator ensures that water flow sees.",
            &[&observation]
        ));
        let required = evidence("The operator must not close the valve.");
        assert!(modal_force_supported(
            "The operator must not close the valve.",
            &[&required]
        ));
        assert!(!modal_force_supported(
            "The operator closes the valve.",
            &[&required]
        ));
        assert!(!modal_force_supported(
            "The operator does not close the valve.",
            &[&required]
        ));
        assert!(!modal_force_supported(
            "The operator should not close the valve.",
            &[&required]
        ));
        assert!(modal_force_supported(
            "The guard watches the door.",
            &[&required]
        ));
    }

    #[test]
    #[ignore = "CPU-only original A1 guard replay; requires DOC_SUM_GUARD_A1_DRAFT"]
    fn original_a1_unit_is_withheld() {
        let path = std::env::var_os("DOC_SUM_GUARD_A1_DRAFT").unwrap();
        let saved: SynthesizedDocument =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let mut verifications = saved
            .summary_claims
            .iter()
            .map(|claim| ClaimVerification {
                claim_id: claim.claim_id.clone(),
                evidence_ids: claim.evidence_ids.clone(),
                verdict: ClaimVerdict::Supported,
            })
            .collect::<Vec<_>>();
        apply_semantic_fidelity_guards(
            &saved.summary_claims,
            &saved.synthesis_evidence,
            &mut verifications,
        )
        .unwrap();
        println!("ORIGINAL_A1_GUARD_VERDICT {:?}", verifications[0].verdict);
        assert_ne!(
            verifications[0].verdict,
            ClaimVerdict::Supported,
            "unfaithful A1 unit must be withheld"
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumericRelation {
    LessThan,
    AtMost,
    GreaterThan,
    AtLeast,
    Equal,
    NotEqual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NumericConstraint {
    value: String,
    relation: NumericRelation,
    unit: Option<String>,
    context: Vec<String>,
    trailing_subject: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NumericReference {
    context: Vec<String>,
    trailing_subject: Vec<String>,
}

fn ends_with_words(words: &[String], suffix: &[&str]) -> bool {
    words.len() >= suffix.len()
        && words[words.len() - suffix.len()..]
            .iter()
            .map(String::as_str)
            .eq(suffix.iter().copied())
}

fn numeric_value(word: &str) -> Option<String> {
    let (negative, word) = if let Some(value) = word.strip_prefix('-') {
        (true, value)
    } else if let Some(value) = word.strip_prefix('+') {
        (false, value)
    } else {
        (false, word)
    };
    let mut parts = word.split('.');
    let integer = parts.next()?;
    let fraction = parts.next();
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.chars().all(|character| character.is_ascii_digit())
        || fraction.is_some_and(|value| {
            value.is_empty() || !value.chars().all(|character| character.is_ascii_digit())
        })
    {
        return None;
    }
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let fraction = fraction.map(|value| value.trim_end_matches('0'));
    let normalized = match fraction {
        Some("") | None => integer.to_string(),
        Some(value) => format!("{integer}.{value}"),
    };
    Some(if negative && normalized != "0" {
        format!("-{normalized}")
    } else {
        normalized
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NumericMention {
    value: String,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardinalKind {
    Small,
    Tens,
    Hundred,
    Scale,
}

fn small_cardinal(word: &str) -> Option<u64> {
    Some(match word {
        "zero" => 0,
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        "thirteen" => 13,
        "fourteen" => 14,
        "fifteen" => 15,
        "sixteen" => 16,
        "seventeen" => 17,
        "eighteen" => 18,
        "nineteen" => 19,
        _ => return None,
    })
}

fn tens_cardinal(word: &str) -> Option<u64> {
    Some(match word {
        "twenty" => 20,
        "thirty" => 30,
        "forty" => 40,
        "fifty" => 50,
        "sixty" => 60,
        "seventy" => 70,
        "eighty" => 80,
        "ninety" => 90,
        _ => return None,
    })
}

fn cardinal_scale(word: &str) -> Option<u64> {
    Some(match word {
        "thousand" => 1_000,
        "million" => 1_000_000,
        "billion" => 1_000_000_000,
        _ => return None,
    })
}

fn spelled_numeric_value(tokens: &[String], start: usize) -> Option<NumericMention> {
    let mut index = start;
    let negative = tokens
        .get(index)
        .is_some_and(|word| matches!(word.as_str(), "minus" | "negative"));
    if negative {
        index += 1;
    }
    let mut total = 0_u64;
    let mut group = 0_u64;
    let mut last_kind = None;
    let mut last_scale = u64::MAX;
    let mut saw_number = false;
    while let Some(word) = tokens.get(index).map(String::as_str) {
        if word == "and"
            && saw_number
            && tokens
                .get(index + 1)
                .is_some_and(|next| small_cardinal(next).is_some() || tens_cardinal(next).is_some())
        {
            index += 1;
            continue;
        }
        if let Some(value) = small_cardinal(word) {
            if matches!(last_kind, Some(CardinalKind::Small))
                || matches!(last_kind, Some(CardinalKind::Tens)) && value > 9
            {
                break;
            }
            group = group.checked_add(value)?;
            last_kind = Some(CardinalKind::Small);
        } else if let Some(value) = tens_cardinal(word) {
            if matches!(last_kind, Some(CardinalKind::Small | CardinalKind::Tens)) {
                break;
            }
            group = group.checked_add(value)?;
            last_kind = Some(CardinalKind::Tens);
        } else if word == "hundred" {
            if !matches!(last_kind, Some(CardinalKind::Small)) || !(1..=9).contains(&group) {
                break;
            }
            group = group.checked_mul(100)?;
            last_kind = Some(CardinalKind::Hundred);
        } else if let Some(scale) = cardinal_scale(word) {
            if !saw_number || group == 0 || scale >= last_scale {
                break;
            }
            total = total.checked_add(group.checked_mul(scale)?)?;
            group = 0;
            last_scale = scale;
            last_kind = Some(CardinalKind::Scale);
        } else {
            break;
        }
        saw_number = true;
        index += 1;
    }
    if !saw_number || index == start + usize::from(negative) {
        return None;
    }
    let value = total.checked_add(group)?;
    Some(NumericMention {
        value: if negative && value != 0 {
            format!("-{value}")
        } else {
            value.to_string()
        },
        start,
        end: index,
    })
}

fn numeric_mentions(tokens: &[String]) -> Vec<NumericMention> {
    let mut mentions = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if currency_token_unit(&tokens[index]).is_some() {
            if let Some(value) = tokens.get(index + 1).and_then(|word| numeric_value(word)) {
                mentions.push(NumericMention {
                    value,
                    start: index,
                    end: index + 2,
                });
                index += 2;
            } else if let Some(mut mention) = spelled_numeric_value(tokens, index + 1) {
                mention.start = index;
                index = mention.end;
                mentions.push(mention);
            } else {
                index += 1;
            }
        } else if let Some(value) = numeric_value(&tokens[index]) {
            mentions.push(NumericMention {
                value,
                start: index,
                end: index + 1,
            });
            index += 1;
        } else if let Some(mention) = spelled_numeric_value(tokens, index) {
            index = mention.end;
            mentions.push(mention);
        } else {
            index += 1;
        }
    }
    mentions
}

fn negation_present(tokens: &[String]) -> bool {
    tokens.iter().enumerate().any(|(index, word)| {
        let additive_not =
            word == "not" && tokens.get(index + 1).is_some_and(|next| next == "only");
        (!additive_not && matches!(word.as_str(), "not" | "no" | "never" | "without" | "cannot"))
            || (matches!(
                word.as_str(),
                "isn"
                    | "aren"
                    | "wasn"
                    | "weren"
                    | "don"
                    | "doesn"
                    | "didn"
                    | "won"
                    | "wouldn"
                    | "shouldn"
                    | "couldn"
                    | "mustn"
                    | "hasn"
                    | "haven"
                    | "hadn"
                    | "can"
            ) && tokens.get(index + 1).is_some_and(|next| next == "t"))
    })
}

fn numeric_relation(words: &[String], start: usize, end: usize) -> Option<NumericRelation> {
    let before = &words[..start];
    let after = &words[end..];
    if let Some(relation) = before.last().and_then(|word| match word.as_str() {
        "__lt" => Some(NumericRelation::LessThan),
        "__le" => Some(NumericRelation::AtMost),
        "__gt" => Some(NumericRelation::GreaterThan),
        "__ge" => Some(NumericRelation::AtLeast),
        _ => None,
    }) {
        let prefix = &before[..before.len().saturating_sub(1)];
        return Some(
            if negation_present(&prefix[prefix.len().saturating_sub(4)..]) {
                match relation {
                    NumericRelation::LessThan => NumericRelation::AtLeast,
                    NumericRelation::AtMost => NumericRelation::GreaterThan,
                    NumericRelation::GreaterThan => NumericRelation::AtMost,
                    NumericRelation::AtLeast => NumericRelation::LessThan,
                    NumericRelation::Equal | NumericRelation::NotEqual => relation,
                }
            } else {
                relation
            },
        );
    }
    if ends_with_words(before, &["at", "most"]) {
        let prefix = &before[..before.len().saturating_sub(2)];
        return Some(
            if negation_present(&prefix[prefix.len().saturating_sub(4)..]) {
                NumericRelation::GreaterThan
            } else {
                NumericRelation::AtMost
            },
        );
    }
    if ends_with_words(before, &["at", "least"]) {
        let prefix = &before[..before.len().saturating_sub(2)];
        return Some(
            if negation_present(&prefix[prefix.len().saturating_sub(4)..]) {
                NumericRelation::LessThan
            } else {
                NumericRelation::AtLeast
            },
        );
    }
    if ends_with_words(before, &["no", "more", "than"])
        || ends_with_words(before, &["not", "more", "than"])
        || ends_with_words(before, &["up", "to"])
        || before.last().is_some_and(|word| word == "maximum")
        || ends_with_words(before, &["maximum", "of"])
        || after.starts_with(&["or".to_string(), "fewer".to_string()])
        || after.starts_with(&["or".to_string(), "less".to_string()])
    {
        return Some(NumericRelation::AtMost);
    }
    if ends_with_words(before, &["no", "less", "than"])
        || ends_with_words(before, &["not", "less", "than"])
        || before.last().is_some_and(|word| word == "minimum")
        || ends_with_words(before, &["minimum", "of"])
        || after.starts_with(&["or".to_string(), "more".to_string()])
        || after.starts_with(&["or".to_string(), "greater".to_string()])
    {
        return Some(NumericRelation::AtLeast);
    }
    if ends_with_words(before, &["more", "than"]) || ends_with_words(before, &["greater", "than"]) {
        let prefix = &before[..before.len().saturating_sub(2)];
        let negated = negation_present(&prefix[prefix.len().saturating_sub(4)..]);
        return Some(if negated {
            NumericRelation::AtMost
        } else {
            NumericRelation::GreaterThan
        });
    }
    if ends_with_words(before, &["less", "than"]) || ends_with_words(before, &["fewer", "than"]) {
        let prefix = &before[..before.len().saturating_sub(2)];
        let negated = negation_present(&prefix[prefix.len().saturating_sub(4)..]);
        return Some(if negated {
            NumericRelation::AtLeast
        } else {
            NumericRelation::LessThan
        });
    }
    if before
        .last()
        .is_some_and(|word| matches!(word.as_str(), "under" | "below"))
    {
        let prefix = &before[..before.len().saturating_sub(1)];
        return Some(
            if negation_present(&prefix[prefix.len().saturating_sub(4)..]) {
                NumericRelation::AtLeast
            } else {
                NumericRelation::LessThan
            },
        );
    }
    if before
        .last()
        .is_some_and(|word| matches!(word.as_str(), "over" | "above"))
    {
        let prefix = &before[..before.len().saturating_sub(1)];
        return Some(
            if negation_present(&prefix[prefix.len().saturating_sub(4)..]) {
                NumericRelation::AtMost
            } else {
                NumericRelation::GreaterThan
            },
        );
    }
    if before.last().is_some_and(|word| word == "exactly") {
        let prefix = &before[..before.len().saturating_sub(1)];
        return Some(
            if negation_present(&prefix[prefix.len().saturating_sub(4)..]) {
                NumericRelation::NotEqual
            } else {
                NumericRelation::Equal
            },
        );
    }
    let is_equality_copula =
        |word: &String| matches!(word.as_str(), "is" | "are" | "was" | "were" | "equals");
    let equality_copula = before
        .last()
        .filter(|word| is_equality_copula(word))
        .map(|_| (before.len() - 1, false))
        .or_else(|| {
            (before.last().is_some_and(|word| word == "not"))
                .then(|| before.len().checked_sub(2))
                .flatten()
                .filter(|index| is_equality_copula(&before[*index]))
                .map(|index| (index, true))
        });
    if let Some((copula, intervening_negation)) = equality_copula {
        let prefix = &before[..copula];
        return Some(
            if intervening_negation || negation_present(&prefix[prefix.len().saturating_sub(4)..]) {
                NumericRelation::NotEqual
            } else {
                NumericRelation::Equal
            },
        );
    }
    None
}

// Layout boundaries belong to the relation reader. A single wrapped line is
// not a proposition boundary; paragraphs, list entries and marked headings are.
// The iterator is invoked independently for each owned EvidenceItem.
fn semantic_clauses(text: &str) -> impl Iterator<Item = String> {
    let mut paragraphs = Vec::new();
    let mut paragraph = String::new();
    for line in text.lines() {
        let line = line.trim();
        let bullet = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .or_else(|| line.strip_prefix("\u{2022} "));
        let marked = marked_heading_title(line);
        let heading = line.ends_with(':') || framing_from_heading(line).is_some();
        if line.is_empty() || bullet.is_some() || marked.is_some() || heading {
            if !paragraph.is_empty() {
                paragraphs.push(std::mem::take(&mut paragraph));
            }
            if line.is_empty() || heading {
                continue;
            }
        }
        let body = bullet.or(marked).unwrap_or(line);
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(body);
    }
    if !paragraph.is_empty() {
        paragraphs.push(paragraph);
    }
    paragraphs.into_iter().flat_map(|paragraph| {
        paragraph
            .split(['.', '?', '!', ';'])
            .map(str::trim)
            .filter(|clause| !clause.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    })
}

fn claim_propositions(text: &str) -> impl Iterator<Item = String> {
    semantic_clauses(text).flat_map(|clause| independent_propositions(&clause))
}

// Only explicit coordinated independent propositions introduce an actor
// boundary. Conjoined values and shared-subject verb lists remain intact.
fn has_finite_predicate(tokens: &[String]) -> bool {
    tokens.iter().enumerate().any(|(index, word)| {
        index > 0
            && (is_modal_anchor(word)
                || matches!(
                    word.as_str(),
                    "is" | "are"
                        | "was"
                        | "were"
                        | "has"
                        | "have"
                        | "begins"
                        | "ends"
                        | "ensures"
                        | "guarantees"
                        | "provides"
                        | "makes"
                        | "make"
                        | "commenced"
                ))
    })
}

fn qualifying_conjunct(tokens: &[String]) -> bool {
    tokens.first().is_some_and(|word| {
        matches!(
            word.as_str(),
            "only" | "if" | "unless" | "until" | "when" | "provided"
        )
    }) || tokens.starts_with(&["so".into(), "long".into(), "as".into()])
}

fn independent_propositions(clause: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut start = 0;
    for (comma, _) in clause.match_indices(',') {
        let tail = clause[comma + 1..].trim_start();
        let absolute = tail.strip_prefix("with ").filter(|body| {
            let tokens = words(body.split(',').next().unwrap_or(body));
            tokens
                .iter()
                .enumerate()
                .any(|(i, word)| i > 0 && matches!(word.as_str(), "beginning" | "ending"))
        });
        let Some(body) = absolute
            .or_else(|| tail.strip_prefix("and "))
            .or_else(|| tail.strip_prefix("but "))
        else {
            continue;
        };
        let candidate = body.split(',').next().unwrap_or(body);
        let tokens = words(candidate);
        if qualifying_conjunct(&tokens) {
            continue;
        }
        if absolute.is_none() && !has_finite_predicate(&tokens)
            || !(has_finite_predicate(&words(&clause[start..comma]))
                || words(&clause[start..comma])
                    .iter()
                    .any(|word| matches!(word.as_str(), "beginning" | "ending")))
        {
            continue;
        }
        result.push(clause[start..comma].trim().to_owned());
        start = clause.len() - body.len();
    }
    if start < clause.len() {
        result.push(clause[start..].trim().to_owned());
    }
    result
}

// Identity is structural: discard articles, and canonicalize a single owned
// noun-of-owner construction to the corresponding attributive noun phrase.
// Actor nouns and their roles remain ordered; this is not a bag of words.
fn relation_subject_identity(tokens: &[String]) -> Vec<String> {
    let mut result = tokens
        .iter()
        .filter(|word| !matches!(word.as_str(), "a" | "an" | "the"))
        .cloned()
        .collect::<Vec<_>>();
    let mut index = 0;
    while index + 2 < result.len() {
        if result[index + 1] == "of" && index + 3 == result.len() {
            let owner = result.remove(index + 2);
            result.remove(index + 1);
            result.insert(index, owner);
        }
        index += 1;
    }
    result
}

// Canonical form projection is local to owned Verify relations. Raw source
// tokens/offsets and the writer's shared predicate parser remain unchanged.
fn fraction_identity(numerator: u64, denominator: u64) -> Option<String> {
    if denominator == 0 {
        return None;
    }
    let (mut a, mut b) = (numerator, denominator);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    Some(format!(
        "\0fraction{}over{}",
        numerator / a,
        denominator / a
    ))
}

fn fraction_denominator(word: &str) -> Option<u64> {
    Some(match word {
        "half" | "halves" => 2,
        "third" | "thirds" => 3,
        "quarter" | "quarters" | "fourth" | "fourths" => 4,
        "fifth" | "fifths" => 5,
        "sixth" | "sixths" => 6,
        "seventh" | "sevenths" => 7,
        "eighth" | "eighths" => 8,
        "ninth" | "ninths" => 9,
        "tenth" | "tenths" => 10,
        _ => return None,
    })
}

fn relation_form_words(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for raw in text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && !matches!(c, '/' | '-' | '\u{2013}' | '(' | ')'))
        .filter(|word| !word.is_empty())
    {
        let vulgar = match raw {
            "¼" => Some((1, 4)),
            "½" => Some((1, 2)),
            "¾" => Some((3, 4)),
            "⅐" => Some((1, 7)),
            "⅑" => Some((1, 9)),
            "⅒" => Some((1, 10)),
            "⅓" => Some((1, 3)),
            "⅔" => Some((2, 3)),
            "⅕" => Some((1, 5)),
            "⅖" => Some((2, 5)),
            "⅗" => Some((3, 5)),
            "⅘" => Some((4, 5)),
            "⅙" => Some((1, 6)),
            "⅚" => Some((5, 6)),
            "⅛" => Some((1, 8)),
            "⅜" => Some((3, 8)),
            "⅝" => Some((5, 8)),
            "⅞" => Some((7, 8)),
            _ => None,
        };
        if let Some((n, d)) = vulgar {
            tokens.push(fraction_identity(n, d).expect("vulgar denominator is nonzero"));
        } else if raw.contains('/') {
            let fraction = raw
                .split_once('/')
                .and_then(|(n, d)| fraction_identity(n.parse().ok()?, d.parse().ok()?));
            // Malformed/overflow fractions retain their spelling, never equality
            // with a recognized fraction or a numeric default.
            tokens.push(fraction.unwrap_or_else(|| raw.to_owned()));
        } else if let Some(base) = raw.strip_suffix("(s)") {
            if !base.is_empty() && base.chars().all(char::is_alphabetic) {
                tokens.push(format!("{base}s"));
            } else {
                tokens.extend(words(raw));
            }
        } else if let Some((start, end)) = raw
            .split_once(['-', '\u{2013}'])
            .and_then(|(a, b)| Some((a.parse::<u64>().ok()?, b.parse::<u64>().ok()?)))
        {
            if start <= end {
                tokens.push(format!("\0integers{start}through{end}"));
            } else {
                tokens.push(raw.to_owned());
            }
        } else {
            tokens.extend(words(raw));
        }
    }
    let mut result = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if let Some(number) = spelled_numeric_value(&tokens, index) {
            if let Some(denominator) = tokens
                .get(number.end)
                .and_then(|word| fraction_denominator(word))
            {
                if let Some(value) = number
                    .value
                    .parse::<u64>()
                    .ok()
                    .and_then(|n| fraction_identity(n, denominator))
                {
                    result.push(value);
                    index = number.end + 1;
                    continue;
                }
            }
        }
        if let Ok(start) = tokens[index].parse::<u64>() {
            let mut end = start;
            let mut next = index + 1;
            while tokens.get(next).is_some_and(|word| word == "and") {
                let Some(value) = tokens
                    .get(next + 1)
                    .and_then(|word| word.parse::<u64>().ok())
                else {
                    break;
                };
                if end.checked_add(1) != Some(value) {
                    break;
                }
                end = value;
                next += 2;
            }
            if next > index + 1 {
                // Do not compress a prefix of a nonconsecutive coordinated list.
                if !tokens.get(next).is_some_and(|word| {
                    word == "and"
                        && tokens
                            .get(next + 1)
                            .is_some_and(|w| w.parse::<u64>().is_ok())
                }) {
                    result.push(format!("\0integers{start}through{end}"));
                    index = next;
                    continue;
                }
            }
        }
        result.push(tokens[index].clone());
        index += 1;
    }
    result
}

fn owned_complement_matches(source: &[String], claim: &[String], same_governor: bool) -> bool {
    let source = causal_terms(source);
    let claim = causal_terms(claim);
    if source == claim {
        return true;
    }
    if !same_governor {
        return false;
    }
    // Infinitive and gerund are compared only as the complement of the same
    // transitive relation, not by stripping -ing from arbitrary noun phrases.
    let gerund_matches = |infinitive: &[String], gerund: &[String]| {
        infinitive.first().is_some_and(|word| word == "to")
            && infinitive.len() == gerund.len() + 1
            && infinitive.get(1).is_some_and(|base| {
                gerund.first().is_some_and(|verb| {
                    verb == &format!("{base}ing")
                        || base
                            .strip_suffix('e')
                            .is_some_and(|stem| verb == &format!("{stem}ing"))
                })
            })
            && infinitive[2..] == gerund[1..]
    };
    gerund_matches(&source, &claim) || gerund_matches(&claim, &source)
}

fn nominal_evaluation(tokens: &[String], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    if matches!(
        tokens[index - 1].as_str(),
        "a" | "an" | "the" | "no" | "any" | "some" | "each" | "every" | "all"
    ) {
        return true;
    }
    // A nominal guarantee can end its proposition or introduce a rationale.
    // A direct complement instead belongs to the finite guarantee verb.
    let nominal_tail = index + 1 == tokens.len()
        || tokens
            .get(index + 1)
            .is_some_and(|word| matches!(word.as_str(), "because" | "due"));
    if !nominal_tail {
        return false;
    }
    // Quantified object guarantees need a determiner; arbitrary adjective
    // suffixes cannot identify a noun phrase (official is also a noun).
    tokens[..index].iter().any(|word| {
        matches!(
            word.as_str(),
            "a" | "an" | "the" | "no" | "any" | "some" | "each" | "every" | "all"
        )
    })
}

fn currency_symbol_token(character: char) -> Option<&'static str> {
    Some(match character {
        '$' => "__currency_dollar",
        '€' => "__currency_euro",
        '£' => "__currency_pound_sterling",
        '¥' => "__currency_yen",
        '₹' => "__currency_rupee",
        _ => return None,
    })
}

fn currency_token_unit(token: &str) -> Option<&'static str> {
    Some(match token {
        "__currency_dollar" => "dollar",
        "__currency_euro" => "euro",
        "__currency_pound_sterling" => "pound sterling",
        "__currency_yen" => "yen",
        "__currency_rupee" => "rupee",
        _ => return None,
    })
}

fn comparison_clauses(text: &str) -> Vec<Vec<String>> {
    let characters = text.chars().collect::<Vec<_>>();
    let mut clauses = Vec::new();
    let mut clause = Vec::new();
    let mut token = String::new();
    let flush_token = |token: &mut String, clause: &mut Vec<String>| {
        if !token.is_empty() {
            clause.push(std::mem::take(token));
        }
    };
    for (index, character) in characters.iter().copied().enumerate() {
        if character.is_alphanumeric() {
            token.extend(character.to_lowercase());
            continue;
        }
        let next_is_digit = characters
            .get(index + 1)
            .is_some_and(|value| value.is_ascii_digit());
        let next_is_leading_decimal = characters.get(index + 1).is_some_and(|value| *value == '.')
            && characters
                .get(index + 2)
                .is_some_and(|value| value.is_ascii_digit());
        if matches!(character, '+' | '-' | '−')
            && token.is_empty()
            && (next_is_digit || next_is_leading_decimal)
        {
            token.push(if character == '+' { '+' } else { '-' });
            continue;
        }
        let unsigned_token = token.strip_prefix(['+', '-']).unwrap_or(token.as_str());
        let inside_number = unsigned_token.chars().all(|value| value.is_ascii_digit())
            && !unsigned_token.is_empty()
            && characters
                .get(index + 1)
                .is_some_and(|value| value.is_ascii_digit());
        if character == ',' && inside_number {
            continue;
        }
        let leading_decimal = matches!(token.as_str(), "" | "+" | "-") && next_is_digit;
        if character == '.' && (inside_number || leading_decimal) {
            if leading_decimal {
                token.push('0');
            }
            token.push('.');
            continue;
        }
        flush_token(&mut token, &mut clause);
        if let Some(currency) = currency_symbol_token(character) {
            clause.push(currency.into());
            continue;
        }
        if character == '%' {
            clause.push("percent".into());
            continue;
        }
        if matches!(character, '<' | '>' | '≤' | '≥') {
            let followed_by_equals = characters.get(index + 1).is_some_and(|value| *value == '=');
            clause.push(
                match (character, followed_by_equals) {
                    ('<', false) => "__lt",
                    ('<', true) | ('≤', _) => "__le",
                    ('>', false) => "__gt",
                    ('>', true) | ('≥', _) => "__ge",
                    _ => unreachable!("comparison operator is exhaustively matched"),
                }
                .into(),
            );
            continue;
        }
        if matches!(character, '.' | '?' | '!' | ';' | '\n' | '\r') && !clause.is_empty() {
            clauses.push(std::mem::take(&mut clause));
        }
    }
    flush_token(&mut token, &mut clause);
    if !clause.is_empty() {
        clauses.push(clause);
    }
    clauses
}

fn numeric_context(tokens: &[String], index: usize) -> Vec<String> {
    const COMPARISON_WORDS: &[&str] = &[
        "no", "not", "more", "less", "fewer", "greater", "than", "at", "most", "least", "up", "to",
        "maximum", "minimum", "of", "exactly", "under", "below", "over", "above", "a", "an", "the",
    ];
    let mut end = index;
    while end > 0 && COMPARISON_WORDS.contains(&tokens[end - 1].as_str()) {
        end -= 1;
    }
    tokens[end.saturating_sub(8)..end].to_vec()
}

fn numeric_trailing_subject(tokens: &[String], end: usize) -> Vec<String> {
    let trailing = &tokens[end..];
    let Some(by) = trailing.iter().position(|word| word == "by") else {
        return Vec::new();
    };
    trailing[by + 1..]
        .iter()
        .take_while(|word| {
            !matches!(
                word.as_str(),
                "and" | "or" | "when" | "if" | "unless" | "under" | "because" | "while"
            )
        })
        .take(4)
        .cloned()
        .collect()
}

fn numeric_unit_word(word: &str) -> String {
    match word {
        "dollar" | "dollars" | "usd" => "dollar",
        "euro" | "euros" | "eur" => "euro",
        "percent" | "percentage" | "percentages" => "percent",
        "application" | "applications" => "application",
        "report" | "reports" => "report",
        "degree" | "degrees" => "degree",
        "unit" | "units" => "unit",
        "year" | "years" => "year",
        "day" | "days" => "day",
        "case" | "cases" => "case",
        "worker" | "workers" => "worker",
        "kilogram" | "kilograms" | "kg" | "kgs" => "kilogram",
        "pound" | "pounds" | "lb" | "lbs" => "pound",
        "foot" | "feet" | "ft" => "foot",
        "meter" | "meters" | "metre" | "metres" | "m" => "meter",
        "yen" | "jpy" => "yen",
        "rupee" | "rupees" | "inr" => "rupee",
        _ => word.strip_suffix('s').unwrap_or(word),
    }
    .to_string()
}

fn numeric_unit_stop(word: &str) -> bool {
    matches!(
        word,
        "a" | "an"
            | "the"
            | "and"
            | "or"
            | "but"
            | "if"
            | "when"
            | "unless"
            | "that"
            | "which"
            | "who"
            | "where"
            | "from"
            | "to"
            | "by"
            | "of"
            | "in"
            | "on"
            | "at"
            | "for"
            | "with"
            | "without"
            | "as"
            | "than"
            | "is"
            | "are"
            | "was"
            | "were"
            | "be"
            | "been"
            | "being"
            | "has"
            | "have"
            | "had"
            | "do"
            | "does"
            | "did"
            | "can"
            | "could"
            | "may"
            | "might"
            | "must"
            | "shall"
            | "should"
            | "will"
            | "would"
    )
}

fn structured_unit_component(tokens: &[String], index: usize) -> Option<(String, usize)> {
    let raw_unit = tokens.get(index)?.as_str();
    if raw_unit == "per" || numeric_unit_stop(raw_unit) {
        return None;
    }
    if matches!(raw_unit, "square" | "cubic") {
        let component = tokens.get(index + 1)?.as_str();
        if component == "per" || numeric_unit_stop(component) {
            return None;
        }
        return Some((
            format!("{raw_unit} {}", numeric_unit_word(component)),
            index + 2,
        ));
    }
    Some((numeric_unit_word(raw_unit), index + 1))
}

fn numeric_unit(tokens: &[String], start: usize, end: usize) -> Option<String> {
    if let Some(unit) = tokens.get(start).and_then(|word| currency_token_unit(word)) {
        return Some(unit.to_string());
    }
    if let Some(unit) = tokens.get(end).and_then(|word| currency_token_unit(word)) {
        return Some(unit.to_string());
    }
    let mut index = end;
    if tokens.get(index).is_some_and(|word| word == "or")
        && tokens
            .get(index + 1)
            .is_some_and(|word| matches!(word.as_str(), "fewer" | "less" | "more" | "greater"))
    {
        index += 2;
    }
    let (mut unit, mut index) = structured_unit_component(tokens, index)?;
    if unit == "degree"
        && tokens.get(index).is_some_and(|word| {
            matches!(
                word.as_str(),
                "celsius" | "centigrade" | "fahrenheit" | "kelvin"
            )
        })
    {
        unit.push(' ');
        unit.push_str(&numeric_unit_word(&tokens[index]));
        index += 1;
    }
    if tokens.get(index).is_some_and(|word| word == "per") {
        if let Some((component, _)) = structured_unit_component(tokens, index + 1) {
            unit.push_str(" per ");
            unit.push_str(&component);
        }
    }
    Some(unit)
}

fn numeric_constraints(text: &str) -> Vec<NumericConstraint> {
    comparison_clauses(text)
        .into_iter()
        .flat_map(|tokens| {
            numeric_mentions(&tokens)
                .into_iter()
                .filter_map(|mention| {
                    numeric_relation(&tokens, mention.start, mention.end).map(|relation| {
                        NumericConstraint {
                            value: mention.value,
                            relation,
                            unit: numeric_unit(&tokens, mention.start, mention.end),
                            context: numeric_context(&tokens, mention.start),
                            trailing_subject: numeric_trailing_subject(&tokens, mention.end),
                        }
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn numeric_mention_contexts(text: &str) -> Vec<NumericReference> {
    comparison_clauses(text)
        .into_iter()
        .flat_map(|tokens| {
            numeric_mentions(&tokens)
                .into_iter()
                .map(|mention| NumericReference {
                    context: numeric_context(&tokens, mention.start),
                    trailing_subject: numeric_trailing_subject(&tokens, mention.end),
                })
                .collect::<Vec<_>>()
        })
        .filter(|reference| !reference.context.is_empty() || !reference.trailing_subject.is_empty())
        .collect()
}

fn contains_sequence(haystack: &[String], needle: &[String]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn strip_leading_article(context: &[String]) -> &[String] {
    if context
        .first()
        .is_some_and(|word| matches!(word.as_str(), "a" | "an" | "the"))
    {
        &context[1..]
    } else {
        context
    }
}

fn numeric_contexts_match(source: &NumericReference, claim: &NumericReference) -> bool {
    let source_context = strip_leading_article(&source.context);
    let claim_context = strip_leading_article(&claim.context);
    let shared_subject_prefix = source_context
        .iter()
        .zip(claim_context)
        .take_while(|(source, claim)| source == claim)
        .count();
    let is_auxiliary = |word: &String| {
        matches!(
            word.as_str(),
            "is" | "are"
                | "was"
                | "were"
                | "be"
                | "been"
                | "being"
                | "has"
                | "have"
                | "had"
                | "can"
                | "cannot"
                | "could"
                | "may"
                | "might"
                | "must"
                | "shall"
                | "should"
                | "will"
                | "would"
        )
    };
    let predicate_word = |context: &[String]| {
        context
            .iter()
            .find(|word| !is_auxiliary(word))
            .map(|word| word.strip_suffix('s').unwrap_or(word).to_string())
    };
    let remaining_predicates_match = shared_subject_prefix >= 1
        && predicate_word(&source_context[shared_subject_prefix..])
            == predicate_word(&claim_context[shared_subject_prefix..]);
    (!source_context.is_empty() && source_context == claim_context)
        || remaining_predicates_match
        || (!source.trailing_subject.is_empty()
            && (source.trailing_subject == claim.trailing_subject
                || contains_sequence(&claim.context, &source.trailing_subject)))
        || (!claim.trailing_subject.is_empty()
            && contains_sequence(&source.context, &claim.trailing_subject))
}

fn normalized_numeric_context_word(word: &str) -> Option<String> {
    if matches!(
        word,
        "a" | "an"
            | "the"
            | "is"
            | "are"
            | "was"
            | "were"
            | "be"
            | "been"
            | "being"
            | "has"
            | "have"
            | "had"
            | "do"
            | "does"
            | "did"
            | "can"
            | "could"
            | "may"
            | "might"
            | "must"
            | "shall"
            | "should"
            | "will"
            | "would"
            | "not"
            | "no"
            | "never"
            | "when"
            | "if"
            | "unless"
            | "provided"
    ) {
        return None;
    }
    Some(if let Some(stem) = word.strip_suffix("ies") {
        format!("{stem}y")
    } else if let Some(stem) = word.strip_suffix('d').filter(|stem| stem.ends_with('e')) {
        stem.to_string()
    } else if let Some(stem) = word.strip_suffix("ed") {
        stem.to_string()
    } else if let Some(stem) = word.strip_suffix("ing") {
        stem.to_string()
    } else {
        word.strip_suffix('s').unwrap_or(word).to_string()
    })
}

fn numeric_context_subject_anchor(context: &[String]) -> Option<Vec<String>> {
    let local_start = context
        .iter()
        .rposition(|word| matches!(word.as_str(), "when" | "if" | "unless" | "provided"))
        .map_or(0, |connector| connector + 1);
    let context = strip_leading_article(&context[local_start..]);
    let predicate = context.iter().position(|word| {
        matches!(
            word.as_str(),
            "is" | "are"
                | "was"
                | "were"
                | "be"
                | "been"
                | "being"
                | "has"
                | "have"
                | "had"
                | "do"
                | "does"
                | "did"
                | "can"
                | "could"
                | "may"
                | "might"
                | "must"
                | "shall"
                | "should"
                | "will"
                | "would"
                | "require"
                | "requires"
                | "required"
                | "use"
                | "uses"
                | "used"
                | "remain"
                | "remains"
                | "remained"
                | "isn"
                | "aren"
                | "wasn"
                | "weren"
                | "don"
                | "doesn"
                | "didn"
                | "won"
                | "wouldn"
                | "shouldn"
                | "couldn"
                | "mustn"
                | "hasn"
                | "haven"
                | "hadn"
                | "__lt"
                | "__le"
                | "__gt"
                | "__ge"
        )
    })?;
    let anchor = context[..predicate]
        .iter()
        .filter_map(|word| normalized_numeric_context_word(word))
        .collect::<Vec<_>>();
    (!anchor.is_empty()).then_some(anchor)
}

fn numeric_context_subjects_match(source: &[String], claim: &[String]) -> bool {
    numeric_context_subject_anchor(source)
        .zip(numeric_context_subject_anchor(claim))
        .is_some_and(|(source_anchor, claim_anchor)| source_anchor == claim_anchor)
}

fn constraint_reference(constraint: &NumericConstraint) -> NumericReference {
    NumericReference {
        context: constraint.context.clone(),
        trailing_subject: constraint.trailing_subject.clone(),
    }
}

fn numeric_value_parts(value: &str) -> Option<(bool, &str, &str)> {
    let (negative, unsigned) = value
        .strip_prefix('-')
        .map_or((false, value), |unsigned| (true, unsigned));
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    (!integer.is_empty()
        && integer.chars().all(|character| character.is_ascii_digit())
        && fraction.chars().all(|character| character.is_ascii_digit()))
    .then_some((negative, integer, fraction))
}

fn absolute_numeric_value_order(
    left_integer: &str,
    left_fraction: &str,
    right_integer: &str,
    right_fraction: &str,
) -> std::cmp::Ordering {
    left_integer
        .len()
        .cmp(&right_integer.len())
        .then_with(|| left_integer.cmp(right_integer))
        .then_with(|| {
            let length = left_fraction.len().max(right_fraction.len());
            (0..length)
                .map(|index| {
                    left_fraction
                        .as_bytes()
                        .get(index)
                        .copied()
                        .unwrap_or(b'0')
                        .cmp(
                            &right_fraction
                                .as_bytes()
                                .get(index)
                                .copied()
                                .unwrap_or(b'0'),
                        )
                })
                .find(|order| !order.is_eq())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn numeric_value_order(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let (left_negative, left_integer, left_fraction) = numeric_value_parts(left)?;
    let (right_negative, right_integer, right_fraction) = numeric_value_parts(right)?;
    Some(match (left_negative, right_negative) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => {
            absolute_numeric_value_order(left_integer, left_fraction, right_integer, right_fraction)
        }
        (true, true) => {
            absolute_numeric_value_order(left_integer, left_fraction, right_integer, right_fraction)
                .reverse()
        }
    })
}

fn numeric_relation_supports(
    source: NumericRelation,
    claim: NumericRelation,
    source_to_claim: std::cmp::Ordering,
) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    match (source, claim) {
        (NumericRelation::Equal, NumericRelation::Equal) => source_to_claim == Equal,
        (NumericRelation::Equal, NumericRelation::AtMost) => source_to_claim != Greater,
        (NumericRelation::Equal, NumericRelation::LessThan) => source_to_claim == Less,
        (NumericRelation::Equal, NumericRelation::AtLeast) => source_to_claim != Less,
        (NumericRelation::Equal, NumericRelation::GreaterThan) => source_to_claim == Greater,
        (NumericRelation::Equal, NumericRelation::NotEqual) => source_to_claim != Equal,
        (NumericRelation::AtMost, NumericRelation::AtMost) => source_to_claim != Greater,
        (NumericRelation::AtMost, NumericRelation::LessThan) => source_to_claim == Less,
        (NumericRelation::AtMost, NumericRelation::NotEqual) => source_to_claim == Less,
        (
            NumericRelation::LessThan,
            NumericRelation::LessThan | NumericRelation::AtMost | NumericRelation::NotEqual,
        ) => source_to_claim != Greater,
        (NumericRelation::AtLeast, NumericRelation::AtLeast) => source_to_claim != Less,
        (NumericRelation::AtLeast, NumericRelation::GreaterThan) => source_to_claim == Greater,
        (NumericRelation::AtLeast, NumericRelation::NotEqual) => source_to_claim == Greater,
        (
            NumericRelation::GreaterThan,
            NumericRelation::GreaterThan | NumericRelation::AtLeast | NumericRelation::NotEqual,
        ) => source_to_claim != Less,
        (NumericRelation::NotEqual, NumericRelation::NotEqual) => source_to_claim == Equal,
        _ => false,
    }
}

fn numeric_units_match(source: &NumericConstraint, claim: &NumericConstraint) -> bool {
    source.unit.is_none() || claim.unit.is_none() || source.unit == claim.unit
}

fn comparison_boundaries_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let source_constraints = evidence
        .iter()
        .flat_map(|item| numeric_constraints(&item.exact_quote))
        .collect::<Vec<_>>();
    let source_contexts = evidence
        .iter()
        .flat_map(|item| numeric_mention_contexts(&item.exact_quote))
        .collect::<Vec<_>>();
    numeric_constraints(claim).into_iter().all(|constraint| {
        let claim_reference = constraint_reference(&constraint);
        let matching_relation = source_constraints
            .iter()
            .filter(|source| {
                numeric_value_order(&source.value, &constraint.value).is_some_and(|order| {
                    numeric_relation_supports(source.relation, constraint.relation, order)
                }) && numeric_units_match(source, &constraint)
            })
            .collect::<Vec<_>>();
        if matching_relation
            .iter()
            .any(|source| numeric_contexts_match(&constraint_reference(source), &claim_reference))
        {
            return true;
        }
        let contextual_same_value = source_constraints
            .iter()
            .filter(|source| {
                source.value == constraint.value
                    && ((source.context.is_empty()
                        && source.trailing_subject.is_empty()
                        && constraint.context.is_empty()
                        && constraint.trailing_subject.is_empty())
                        || numeric_contexts_match(&constraint_reference(source), &claim_reference)
                        || numeric_context_subjects_match(&source.context, &constraint.context))
            })
            .collect::<Vec<_>>();
        let same_value_entails = contextual_same_value.iter().any(|source| {
            numeric_value_order(&source.value, &constraint.value).is_some_and(|order| {
                numeric_relation_supports(source.relation, constraint.relation, order)
            }) && numeric_units_match(source, &constraint)
        });
        let same_value_conflicts = contextual_same_value.iter().any(|source| {
            !numeric_value_order(&source.value, &constraint.value).is_some_and(|order| {
                numeric_relation_supports(source.relation, constraint.relation, order)
            }) || !numeric_units_match(source, &constraint)
        });
        if same_value_conflicts && !same_value_entails {
            return false;
        }
        (constraint.context.is_empty() && constraint.trailing_subject.is_empty())
            || !source_contexts
                .iter()
                .any(|source| numeric_contexts_match(source, &claim_reference))
    })
}

fn contains_words(text: &str, phrase: &[&str]) -> bool {
    let tokens = words(text);
    tokens
        .windows(phrase.len())
        .any(|window| window.iter().map(String::as_str).eq(phrase.iter().copied()))
}

fn broader_enumeration_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let has_family_member = |text: &str| {
        contains_words(text, &["family", "member"]) || contains_words(text, &["family", "members"])
    };
    if has_family_member(claim)
        && !evidence
            .iter()
            .any(|item| has_family_member(&item.exact_quote))
    {
        return false;
    }
    let family_scopes = |text: &str| {
        let tokens = words(text);
        tokens
            .windows(2)
            .enumerate()
            .filter(|(_, pair)| {
                pair[0] == "family" && matches!(pair[1].as_str(), "member" | "members")
            })
            .map(|(index, _)| {
                tokens
                    .get(index.wrapping_sub(1))
                    .is_some_and(|word| word == "immediate")
            })
            .collect::<Vec<_>>()
    };
    let source_scopes = evidence
        .iter()
        .flat_map(|item| family_scopes(&item.exact_quote))
        .collect::<Vec<_>>();
    let source_has_immediate_family = source_scopes.contains(&true);
    let source_has_unqualified_family = source_scopes.contains(&false);
    let claim_has_family = contains_words(claim, &["family", "member"])
        || contains_words(claim, &["family", "members"]);
    let claim_preserves_immediate = contains_words(claim, &["immediate", "family", "member"])
        || contains_words(claim, &["immediate", "family", "members"]);
    if source_has_immediate_family
        && !source_has_unqualified_family
        && claim_has_family
        && !claim_preserves_immediate
    {
        return false;
    }
    let kinship_relative = |text: &str| {
        let tokens = words(text);
        tokens.iter().enumerate().any(|(index, word)| {
            matches!(word.as_str(), "relative" | "relatives")
                && (tokens.get(index + 1).is_some_and(|next| next == "of")
                    || tokens.get(index.wrapping_sub(1)).is_some_and(|previous| {
                        matches!(
                            previous.as_str(),
                            "his" | "her" | "their" | "our" | "your" | "s"
                        )
                    }))
        })
    };
    if kinship_relative(claim)
        && !evidence
            .iter()
            .any(|item| kinship_relative(&item.exact_quote))
    {
        return false;
    }
    true
}

fn endpoint_word(word: &str) -> Option<String> {
    if matches!(
        word,
        "a" | "an" | "the" | "their" | "his" | "her" | "its" | "any"
    ) {
        return None;
    }
    Some(word.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectionalRelation {
    origin: Vec<String>,
    destination: Vec<String>,
    source_scope: Vec<String>,
    actor: Vec<String>,
    prefix: Vec<String>,
}

fn directional_endpoint_and_scope(tokens: &[String]) -> (Vec<String>, Vec<String>) {
    let hard_boundary = tokens
        .iter()
        .position(|word| {
            matches!(
                word.as_str(),
                "and" | "or" | "by" | "from" | "to" | "unless" | "absent" | "if" | "when"
            )
        })
        .unwrap_or(tokens.len());
    let tokens = &tokens[..hard_boundary];
    let scope_start = tokens
        .iter()
        .position(|word| matches!(word.as_str(), "during" | "until" | "throughout"))
        .unwrap_or(tokens.len());
    let endpoint = tokens[..scope_start]
        .iter()
        .take(8)
        .filter_map(|word| endpoint_word(word))
        .collect::<Vec<_>>();
    let source_scope = tokens[scope_start..].to_vec();
    (endpoint, source_scope)
}

fn directional_actor(tokens: &[String], anchor: usize) -> Vec<String> {
    let context = &tokens[..anchor];
    if let Some(by) = context.iter().rposition(|word| word == "by") {
        let agent = strip_leading_article(&context[by + 1..]);
        if !agent.is_empty() {
            return agent.iter().take(4).cloned().collect();
        }
    }
    let is_route_predicate = |word: &String| {
        matches!(
            word.as_str(),
            "transport"
                | "transports"
                | "transported"
                | "transporting"
                | "carry"
                | "carries"
                | "carried"
                | "carrying"
                | "provide"
                | "provides"
                | "provided"
                | "providing"
        )
    };
    let Some(predicate) = context.iter().rposition(is_route_predicate) else {
        return Vec::new();
    };
    let start = context[..predicate]
        .iter()
        .enumerate()
        .rfind(|(index, word)| {
            matches!(word.as_str(), "but" | "while" | "whereas")
                || (word.as_str() == "and"
                    && context[..*index].iter().any(|prior| {
                        is_route_predicate(prior) || matches!(prior.as_str(), "from" | "to")
                    }))
        })
        .map(|(index, _)| index)
        .map_or(0, |index| index + 1);
    let mut actor = context[start..predicate].to_vec();
    if actor
        .first()
        .is_some_and(|word| matches!(word.as_str(), "a" | "an" | "the"))
    {
        actor.remove(0);
    }
    while actor.last().is_some_and(|word| {
        matches!(
            word.as_str(),
            "can" | "could" | "may" | "might" | "must" | "shall" | "should" | "will" | "would"
        )
    }) {
        actor.pop();
    }
    actor
}

fn directional_relations_in_clause(clause: &str) -> Vec<DirectionalRelation> {
    let tokens = words(clause);
    let mut relations = Vec::new();
    for (from, token) in tokens.iter().enumerate() {
        if token != "from" {
            continue;
        }
        let Some(to) =
            (from + 1..(from + 13).min(tokens.len())).find(|index| tokens[*index] == "to")
        else {
            continue;
        };
        let origin = tokens[from + 1..to]
            .iter()
            .filter_map(|word| endpoint_word(word))
            .collect::<Vec<_>>();
        let (destination, source_scope) = directional_endpoint_and_scope(&tokens[to + 1..]);
        if !origin.is_empty() && !destination.is_empty() {
            relations.push(DirectionalRelation {
                origin,
                destination,
                source_scope,
                actor: directional_actor(&tokens, from),
                prefix: strip_leading_article(&tokens[..from]).to_vec(),
            });
        }
    }
    for (to, token) in tokens.iter().enumerate() {
        if token != "to" {
            continue;
        }
        let Some(from) =
            (to + 1..(to + 13).min(tokens.len())).find(|index| tokens[*index] == "from")
        else {
            continue;
        };
        let destination = tokens[to + 1..from]
            .iter()
            .filter_map(|word| endpoint_word(word))
            .collect::<Vec<_>>();
        let (origin, source_scope) = directional_endpoint_and_scope(&tokens[from + 1..]);
        if !origin.is_empty() && !destination.is_empty() {
            relations.push(DirectionalRelation {
                origin,
                destination,
                source_scope,
                actor: directional_actor(&tokens, to),
                prefix: strip_leading_article(&tokens[..to]).to_vec(),
            });
        }
    }
    relations
}

fn normalized_endpoint(words: &[String]) -> Vec<String> {
    let modifier_start = words.iter().position(|word| {
        matches!(
            word.as_str(),
            "each" | "every" | "daily" | "weekly" | "monthly" | "annually"
        )
    });
    let words = &words[..modifier_start.unwrap_or(words.len())];
    let mut normalized = Vec::new();
    let mut index = 0;
    while index < words.len() {
        let remaining = &words[index..];
        if remaining.starts_with(&["living".into(), "quarters".into()]) {
            normalized.push("housing".into());
            index += 2;
        } else if remaining.starts_with(&["work".into(), "site".into()]) {
            normalized.push("worksite".into());
            index += 2;
        } else if remaining.starts_with(&["place".into(), "of".into(), "employment".into()]) {
            normalized.push("worksite".into());
            index += 3;
        } else {
            normalized.push(match words[index].as_str() {
                "workplace" => "worksite".into(),
                "home" => "housing".into(),
                other => other.to_string(),
            });
            index += 1;
        }
    }
    normalized
}

fn endpoints_match(left: &[String], right: &[String]) -> bool {
    normalized_endpoint(left) == normalized_endpoint(right)
}

fn endpoint_pairs_match(left: &DirectionalRelation, right: &DirectionalRelation) -> bool {
    endpoints_match(&left.origin, &right.origin)
        && endpoints_match(&left.destination, &right.destination)
        && (left.source_scope.is_empty() || left.source_scope == right.source_scope)
}

fn relations_match(left: &DirectionalRelation, right: &DirectionalRelation) -> bool {
    endpoint_pairs_match(left, right)
        && ((left.actor.is_empty() && right.actor.is_empty())
            || directional_actor_supports(&left.actor, &right.actor))
}

fn directional_actor_components(actor: &[String]) -> Vec<&[String]> {
    actor
        .split(|word| word == "and")
        .filter(|component| !component.is_empty())
        .collect()
}

fn disjunctive_directional_actor_contains(source: &[String], claim: &[String]) -> bool {
    source.iter().any(|word| word == "or")
        && source
            .split(|word| word == "or")
            .filter(|component| !component.is_empty())
            .any(|component| component == claim)
}

fn directional_actor_supports(source: &[String], claim: &[String]) -> bool {
    source == claim
        || directional_actor_components(source)
            .into_iter()
            .any(|component| component == claim)
}

fn directional_relations(text: &str) -> Vec<DirectionalRelation> {
    semantic_clauses(text)
        .flat_map(|clause| directional_relations_in_clause(&clause))
        .collect()
}

fn directional_actor_is_cited(actor: &[String], evidence: &[&EvidenceItem]) -> bool {
    !actor.is_empty()
        && evidence.iter().any(|item| {
            semantic_clauses(&item.exact_quote).any(|clause| {
                let tokens = words(&clause);
                strip_leading_article(&tokens).starts_with(actor)
            })
        })
}

fn effective_directional_actor(
    relation: &DirectionalRelation,
    source_relations: &[DirectionalRelation],
    evidence: &[&EvidenceItem],
) -> Vec<String> {
    if !relation.actor.is_empty() {
        return relation.actor.clone();
    }
    let source_actor = source_relations
        .iter()
        .filter(|source| !source.actor.is_empty())
        .flat_map(|source| directional_actor_components(&source.actor))
        .find(|actor| relation.prefix.starts_with(actor))
        .map(<[String]>::to_vec)
        .unwrap_or_default();
    if !source_actor.is_empty() {
        return source_actor;
    }
    let prefix = strip_leading_article(&relation.prefix);
    evidence
        .iter()
        .flat_map(|item| semantic_clauses(&item.exact_quote))
        .map(|clause| words(&clause))
        .map(|tokens| strip_leading_article(&tokens).to_vec())
        .filter_map(|tokens| {
            let shared = prefix
                .iter()
                .zip(&tokens)
                .take_while(|(left, right)| left == right)
                .count();
            (shared >= 2).then(|| tokens[..shared].to_vec())
        })
        .max_by_key(Vec::len)
        .unwrap_or_default()
}

fn directional_endpoints_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let source_relations = evidence
        .iter()
        .flat_map(|item| directional_relations(&item.exact_quote))
        .collect::<Vec<_>>();
    directional_relations(claim).into_iter().all(|relation| {
        if source_relations
            .iter()
            .any(|source| relations_match(source, &relation))
        {
            return true;
        }
        if source_relations
            .iter()
            .any(|source| endpoint_pairs_match(source, &relation))
        {
            let relation_actor =
                effective_directional_actor(&relation, &source_relations, evidence);
            if relation_actor.is_empty() {
                return true;
            }
            if source_relations.iter().any(|source| {
                endpoint_pairs_match(source, &relation)
                    && directional_actor_supports(&source.actor, &relation_actor)
            }) {
                return true;
            }
            if source_relations.iter().any(|source| {
                endpoint_pairs_match(source, &relation)
                    && disjunctive_directional_actor_contains(&source.actor, &relation_actor)
            }) {
                return true;
            }
            let actor_is_known = source_relations
                .iter()
                .any(|source| directional_actor_supports(&source.actor, &relation_actor))
                || directional_actor_is_cited(&relation_actor, evidence);
            let route_has_known_actor = source_relations
                .iter()
                .any(|source| endpoint_pairs_match(source, &relation) && !source.actor.is_empty());
            return !(actor_is_known && route_has_known_actor);
        }
        let origin_is_source_origin = source_relations
            .iter()
            .any(|source| endpoints_match(&source.origin, &relation.origin));
        let origin_is_source_destination = source_relations
            .iter()
            .any(|source| endpoints_match(&source.destination, &relation.origin));
        let destination_is_source_origin = source_relations
            .iter()
            .any(|source| endpoints_match(&source.origin, &relation.destination));
        let destination_is_source_destination = source_relations
            .iter()
            .any(|source| endpoints_match(&source.destination, &relation.destination));
        let reversed_known_endpoint = (origin_is_source_destination && !origin_is_source_origin)
            || (destination_is_source_origin && !destination_is_source_destination);
        let both_endpoints_known = (origin_is_source_origin || origin_is_source_destination)
            && (destination_is_source_origin || destination_is_source_destination);
        !(reversed_known_endpoint || both_endpoints_known)
    })
}

fn acronym(word: &str) -> Option<String> {
    let word = word.strip_suffix('s').unwrap_or(word);
    ((2..=10).contains(&word.len()) && word.chars().all(|character| character.is_ascii_uppercase()))
        .then(|| word.to_string())
}

fn defined_actor_labels(text: &str) -> HashMap<String, Vec<String>> {
    let mut labels = HashMap::new();
    for (open, _) in text.match_indices('(') {
        let Some(relative_close) = text[open + 1..].find(')') else {
            continue;
        };
        let close = open + 1 + relative_close;
        let Some(key) = acronym(text[open + 1..close].trim()) else {
            continue;
        };
        let before = &text[..open];
        let punctuation_start = before
            .rfind(['.', ';', ':', ',', '\n', '\r'])
            .map_or(0, |index| index + 1);
        let conjunction_start = before.to_ascii_lowercase()[punctuation_start..]
            .rfind(" and ")
            .map_or(punctuation_start, |index| punctuation_start + index + 5);
        let mut label = words(&before[conjunction_start..]);
        if label.len() > 6 {
            label.drain(..label.len() - 6);
        }
        if !label.is_empty()
            && !label.iter().any(|word| {
                matches!(
                    word.as_str(),
                    "act" | "law" | "program" | "standard" | "regulation"
                )
            })
        {
            labels.insert(key, label);
        }
    }
    labels
}

fn tokens_contain_any(tokens: &[String], alternatives: &[&str]) -> bool {
    tokens
        .iter()
        .any(|word| alternatives.contains(&word.as_str()))
}

fn actor_mention_end(tokens: &[String], actor: &str, label: &[String]) -> Option<usize> {
    let actor = actor.to_ascii_lowercase();
    let acronym_end = tokens
        .iter()
        .enumerate()
        .filter(|(_, word)| {
            *word == &actor
                || word
                    .strip_suffix('s')
                    .is_some_and(|word| word == actor.as_str())
        })
        .map(|(index, _)| index + 1)
        .max();
    let label_end = tokens
        .windows(label.len())
        .enumerate()
        .filter(|(_, window)| *window == label)
        .map(|(index, _)| index + label.len())
        .max();
    acronym_end.into_iter().chain(label_end).max()
}

fn tokens_mention_actor(tokens: &[String], actor: &str, label: &[String]) -> bool {
    actor_mention_end(tokens, actor, label).is_some()
}

fn actor_relation_segments(
    evidence: &[&EvidenceItem],
    actors: &[(&str, &[String])],
    concept: &[&str],
) -> Vec<Vec<String>> {
    evidence
        .iter()
        .flat_map(|item| semantic_clauses(&item.exact_quote))
        .flat_map(|clause| actor_relation_token_segments(&words(&clause), actors, concept))
        .collect()
}

fn actor_relation_token_segments(
    tokens: &[String],
    actors: &[(&str, &[String])],
    concept: &[&str],
) -> Vec<Vec<String>> {
    let mut segments = Vec::new();
    let mut start = 0;
    for index in 0..tokens.len() {
        let contrast = matches!(
            tokens[index].as_str(),
            "while" | "whereas" | "but" | "although"
        );
        let left = &tokens[start..index];
        let right_mentions_actor = actors
            .iter()
            .any(|(actor, label)| tokens_mention_actor(&tokens[index + 1..], actor, label));
        let completed_coordination = matches!(tokens[index].as_str(), "and" | "or")
            && right_mentions_actor
            && (tokens_contain_any(left, concept)
                || actors
                    .iter()
                    .filter_map(|(actor, label)| actor_mention_end(left, actor, label))
                    .max()
                    .is_some_and(|actor_end| actor_end < left.len()));
        if (contrast || completed_coordination) && start < index {
            segments.push(tokens[start..index].to_vec());
            start = index + 1;
        }
    }
    if start < tokens.len() {
        segments.push(tokens[start..].to_vec());
    }
    segments
}

const ACTOR_QUALIFIER_CONCEPTS: &[&[&str]] = &[
    &[
        "compensation",
        "compensated",
        "consideration",
        "money",
        "paid",
        "payment",
        "fee",
    ],
    &["only", "solely", "exclusively"],
    &["unless", "except", "excluding", "absent", "without"],
];

const ACTOR_QUALIFIER_WORDS: &[&str] = &[
    "compensation",
    "compensated",
    "consideration",
    "money",
    "paid",
    "payment",
    "fee",
    "only",
    "solely",
    "exclusively",
    "unless",
    "except",
    "excluding",
    "absent",
    "without",
];

fn qualifier_polarities(tokens: &[String], concept: &[&str]) -> HashSet<bool> {
    tokens
        .iter()
        .enumerate()
        .filter(|(_, word)| concept.contains(&word.as_str()))
        .map(|(index, _)| {
            let context = &tokens[index.saturating_sub(6)..index];
            negation_present(context)
                || context
                    .iter()
                    .any(|word| ACTOR_QUALIFIER_CONCEPTS[2].contains(&word.as_str()))
        })
        .collect()
}

fn is_actor_condition_connector(word: &str) -> bool {
    matches!(word, "if" | "when" | "provided") || ACTOR_QUALIFIER_CONCEPTS[2].contains(&word)
}

fn actor_qualification_clause_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let claim_words = words(claim);
    let Some(condition_index) = claim_words
        .iter()
        .position(|word| is_actor_condition_connector(word))
    else {
        return true;
    };
    let leading_condition = condition_index == 0;
    let condition_start = condition_index
        .checked_sub(1)
        .filter(|previous| ACTOR_QUALIFIER_CONCEPTS[1].contains(&claim_words[*previous].as_str()))
        .unwrap_or(condition_index);
    let (subject_words, condition) = if leading_condition {
        if let Some(comma) = claim.find(',') {
            (words(&claim[comma + 1..]), claim[..comma].to_string())
        } else if let Some(then) = claim_words.iter().position(|word| word == "then") {
            (
                claim_words[then + 1..].to_vec(),
                claim_words[..then].join(" "),
            )
        } else {
            return true;
        }
    } else {
        (
            claim_words[..condition_start].to_vec(),
            claim_words[condition_start..].join(" "),
        )
    };
    let subject_end = subject_words
        .iter()
        .position(|word| {
            matches!(
                word.as_str(),
                "subject"
                    | "is"
                    | "are"
                    | "was"
                    | "were"
                    | "must"
                    | "shall"
                    | "may"
                    | "can"
                    | "will"
                    | "required"
                    | "requires"
            )
        })
        .unwrap_or(subject_words.len());
    let subject = &subject_words[..subject_end];
    let defined_labels = evidence
        .iter()
        .flat_map(|item| defined_actor_labels(&item.exact_quote))
        .collect::<HashMap<_, _>>();
    let source_actors = defined_labels
        .iter()
        .map(|(actor, label)| (actor.as_str(), label.as_slice()))
        .collect::<Vec<_>>();
    let claimed_actors = defined_labels
        .iter()
        .filter(|(actor, label)| tokens_mention_actor(subject, actor, label))
        .map(|(actor, label)| (actor.as_str(), label.as_slice()))
        .collect::<Vec<_>>();
    if claimed_actors.is_empty() {
        return true;
    }
    let condition_tokens = words(&condition);
    ACTOR_QUALIFIER_CONCEPTS.iter().all(|concept| {
        let claim_polarities = qualifier_polarities(&condition_tokens, concept);
        if claim_polarities.is_empty() {
            if *concept == ACTOR_QUALIFIER_CONCEPTS[1]
                && tokens_contain_any(&condition_tokens, ACTOR_QUALIFIER_CONCEPTS[0])
            {
                let source_relations = actor_relation_segments(evidence, &source_actors, concept);
                let source_has_matching_restriction = source_relations.iter().any(|relation| {
                    tokens_contain_any(relation, concept)
                        && tokens_contain_any(relation, ACTOR_QUALIFIER_CONCEPTS[0])
                        && claimed_actors
                            .iter()
                            .any(|(actor, label)| tokens_mention_actor(relation, actor, label))
                });
                return !source_has_matching_restriction;
            }
            return true;
        }
        let source_relations = actor_relation_segments(evidence, &source_actors, concept);
        let supports_actor = |actor: &str, label: &[String]| {
            source_relations.iter().any(|relation| {
                tokens_mention_actor(relation, actor, label)
                    && tokens_contain_any(relation, concept)
                    && !qualifier_polarities(relation, concept).is_disjoint(&claim_polarities)
            })
        };
        let source_has_actor_qualifier = source_relations.iter().any(|relation| {
            tokens_contain_any(relation, concept)
                && source_actors
                    .iter()
                    .any(|(actor, label)| tokens_mention_actor(relation, actor, label))
        });
        let every_actor_is_supported = claimed_actors
            .iter()
            .all(|(actor, label)| supports_actor(actor, label));
        if *concept == ACTOR_QUALIFIER_CONCEPTS[1]
            && tokens_contain_any(&condition_tokens, ACTOR_QUALIFIER_CONCEPTS[0])
        {
            every_actor_is_supported
        } else {
            !source_has_actor_qualifier || every_actor_is_supported
        }
    })
}

fn actor_qualifications_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let defined_labels = evidence
        .iter()
        .flat_map(|item| defined_actor_labels(&item.exact_quote))
        .collect::<HashMap<_, _>>();
    let source_actors = defined_labels
        .iter()
        .map(|(actor, label)| (actor.as_str(), label.as_slice()))
        .collect::<Vec<_>>();
    claim_propositions(claim)
        .flat_map(|clause| {
            let tokens = words(&clause);
            if tokens
                .first()
                .is_some_and(|word| is_actor_condition_connector(word))
            {
                vec![clause.to_string()]
            } else {
                actor_relation_token_segments(&tokens, &source_actors, ACTOR_QUALIFIER_WORDS)
                    .into_iter()
                    .map(|segment| segment.join(" "))
                    .collect()
            }
        })
        .all(|segment| actor_qualification_clause_supported(&segment, evidence))
}

fn original_words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EvaluationRelation {
    governor: Option<String>,
    subject: Vec<String>,
    negated: bool,
    consequence: Option<Vec<String>>,
}

const EVALUATIVE_CONCEPTS: &[&[&str]] = &[
    &[
        "essential",
        "critical",
        "vital",
        "necessary",
        "nonessential",
        "unnecessary",
    ],
    &["ensure", "ensures", "ensuring", "guarantee", "guarantees"],
    &["important", "importance", "unimportant"],
    &[
        "effective",
        "effectiveness",
        "ineffective",
        "ineffectiveness",
    ],
    &["safe", "safety", "unsafe"],
    &["healthy", "health", "unhealthy"],
];

fn is_lexically_negated_evaluation(word: &str) -> bool {
    matches!(
        word,
        "nonessential"
            | "unnecessary"
            | "unimportant"
            | "ineffective"
            | "ineffectiveness"
            | "unsafe"
            | "unhealthy"
    )
}

fn is_evaluative_word(word: &str) -> bool {
    EVALUATIVE_CONCEPTS
        .iter()
        .any(|concept| concept.contains(&word))
}

fn is_evaluation_link(word: &str) -> bool {
    matches!(
        word,
        "is" | "are"
            | "was"
            | "were"
            | "be"
            | "been"
            | "being"
            | "seems"
            | "seemed"
            | "remains"
            | "remained"
            | "isn"
            | "aren"
            | "wasn"
            | "weren"
    )
}

fn evaluation_subject_start(tokens: &[String], linking_index: usize) -> usize {
    let Some(previous_evaluation) = tokens[..linking_index]
        .iter()
        .rposition(|word| is_evaluative_word(word))
    else {
        return 0;
    };
    let Some(conjunction) = tokens[previous_evaluation + 1..linking_index]
        .iter()
        .rposition(|word| matches!(word.as_str(), "and" | "or" | "but" | "while" | "whereas"))
    else {
        return 0;
    };
    let subject_start = previous_evaluation + conjunction + 2;
    if subject_start < linking_index {
        return subject_start;
    }
    tokens[..previous_evaluation]
        .iter()
        .rposition(|word| is_evaluation_link(word))
        .map_or(0, |previous_link| {
            evaluation_subject_start(tokens, previous_link)
        })
}

fn causal_terms(tokens: &[String]) -> Vec<String> {
    let start = usize::from(tokens.first().is_some_and(|word| word == "that"));
    let tokens = &tokens[start..];
    let article = usize::from(tokens.first().is_some_and(|word| {
        matches!(
            word.as_str(),
            "a" | "an" | "the" | "this" | "these" | "that" | "those"
        )
    }));
    let mut terms = tokens[article..].to_vec();
    // Present-tense inflection of the explicitly represented Stop event.
    if terms.last().is_some_and(|word| word == "stops") {
        *terms.last_mut().expect("last term exists") = "stop".into();
    }
    terms
}

fn explicit_causal_relations(clause: &str) -> Vec<EvaluationRelation> {
    let mut relations = evaluation_relations(clause, EVALUATIVE_CONCEPTS[1]);
    let tokens = relation_form_words(clause);
    if let Some(index) = tokens.iter().position(|word| word == "because") {
        let subject = causal_terms(&tokens[index + 1..]);
        let consequence = causal_terms(&tokens[..index]);
        if !subject.is_empty() && !consequence.is_empty() {
            relations.push(EvaluationRelation {
                governor: None,
                subject,
                consequence: Some(consequence),
                negated: false,
            });
        }
    }
    // A causative Stop statement owns both the cause and the resulting event:
    // "X stops Y" -> cause X, event "Y stops". This is not a rule that turns
    // arbitrary transitive predicates into causal assertions.
    if !tokens
        .iter()
        .any(|word| EVALUATIVE_CONCEPTS[1].contains(&word.as_str()))
    {
        if let Some(index) = tokens
            .iter()
            .position(|word| matches!(word.as_str(), "stop" | "stops"))
        {
            let subject = causal_terms(&tokens[..index]);
            let mut consequence = causal_terms(&tokens[index + 1..]);
            if !subject.is_empty() && !consequence.is_empty() {
                consequence.push("stop".into());
                relations.push(EvaluationRelation {
                    governor: None,
                    subject,
                    consequence: Some(consequence),
                    negated: false,
                });
            }
        }
    }
    relations
}

struct NecessityProjection {
    tokens: Vec<String>,
    qualifications: Vec<EvaluationRelation>,
    indices: Vec<usize>,
}

// Conditional necessity is a qualification of its owning proposition, not an
// independent prescribed action. All Verify readers consume this classification.
fn necessity_projection(tokens: &[String]) -> NecessityProjection {
    let mut canonical = tokens.to_vec();
    let mut indices = Vec::new();
    for (index, word) in tokens.iter().enumerate() {
        let conditional = index > 0
            && tokens[index - 1] == "unless"
            && matches!(word.as_str(), "necessary" | "required")
            && tokens.get(index + 1).is_some_and(|word| word == "to");
        let discretionary = index > 0 && tokens[index - 1] == "as" && word == "necessary";
        if conditional || discretionary {
            if conditional {
                canonical[index] = "necessary".into();
            }
            indices.push(index);
        }
    }
    let qualifications = indices
        .iter()
        .map(|index| EvaluationRelation {
            governor: None,
            subject: strip_leading_article(&canonical).to_vec(),
            negated: negation_present(&canonical[..*index]),
            consequence: None,
        })
        .collect();
    NecessityProjection {
        tokens: canonical,
        qualifications,
        indices,
    }
}

fn evaluation_relations(clause: &str, concept: &[&str]) -> Vec<EvaluationRelation> {
    let tokens = relation_form_words(clause);
    let necessity = necessity_projection(&tokens);
    let qualifications = if concept.contains(&"necessary") {
        necessity.qualifications
    } else {
        Vec::new()
    };
    let qualified_indices = necessity.indices;
    let concept_indices = tokens
        .iter()
        .enumerate()
        .filter(|(index, word)| {
            concept.contains(&word.as_str()) && !qualified_indices.contains(index)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    concept_indices
        .into_iter()
        .filter_map(|concept_index| {
            let transitive = matches!(
                tokens[concept_index].as_str(),
                "ensure" | "ensures" | "ensuring" | "guarantee" | "guarantees"
            );
            // Nominal guarantee uses have a determiner/modifier or no verb
            // complement. Ensure has no such nominal ambiguity.
            if matches!(tokens[concept_index].as_str(), "guarantee" | "guarantees")
                && nominal_evaluation(&tokens, concept_index)
            {
                return None;
            }
            let subject_end = if transitive {
                concept_index
            } else {
                // These nouns name domains/qualities; safe/healthy are the
                // corresponding predicative evaluations.
                if matches!(tokens[concept_index].as_str(), "safety" | "health") {
                    return None;
                }
                let link = tokens[..concept_index]
                    .iter()
                    .rposition(|word| is_evaluation_link(word))?;
                let predicate_context = &tokens[link + 1..concept_index];
                if predicate_context.iter().any(|word| {
                    matches!(
                        word.as_str(),
                        "for" | "to" | "in" | "of" | "by" | "unless" | "as"
                    )
                }) {
                    return None;
                }
                link
            };
            let subject_start = evaluation_subject_start(&tokens, subject_end);
            let mut subject = tokens[subject_start..subject_end].to_vec();
            if transitive {
                while subject.last().is_some_and(|word| {
                    matches!(
                        word.as_str(),
                        "do" | "does"
                            | "did"
                            | "can"
                            | "could"
                            | "may"
                            | "might"
                            | "must"
                            | "shall"
                            | "should"
                            | "will"
                            | "would"
                            | "not"
                            | "never"
                    )
                }) {
                    subject.pop();
                }
            } else if let Some(previous_evaluation) =
                subject.iter().position(|word| is_evaluative_word(word))
            {
                if let Some(previous_link) = subject[..previous_evaluation]
                    .iter()
                    .rposition(|word| is_evaluation_link(word))
                {
                    subject.truncate(previous_link);
                }
            }
            if subject.first().is_some_and(|word| {
                matches!(
                    word.as_str(),
                    "a" | "an" | "the" | "this" | "these" | "that" | "those"
                )
            }) {
                subject.remove(0);
            }
            let subject_is_concrete = !subject.is_empty()
                && !matches!(
                    subject.as_slice(),
                    [word] if matches!(word.as_str(), "it" | "they" | "he" | "she")
                );
            Some(EvaluationRelation {
                governor: transitive.then(|| match tokens[concept_index].as_str() {
                    "guarantee" | "guarantees" => "guarantee".to_owned(),
                    _ => "ensure".to_owned(),
                }),
                // Preserve the recognized relation even when its actor cannot
                // be owned. The comparator withholds that unknown relation,
                // including when another clause in the same unit is known.
                subject: if !subject_is_concrete {
                    Vec::new()
                } else if transitive {
                    causal_terms(&subject)
                } else {
                    subject
                },
                negated: negation_present(&tokens[subject_end.saturating_sub(3)..=concept_index])
                    ^ is_lexically_negated_evaluation(&tokens[concept_index]),
                consequence: transitive.then(|| causal_terms(&tokens[concept_index + 1..])),
            })
        })
        .chain(qualifications)
        .collect()
}

fn evaluated_subject_matches(source: &[String], claim: &[String]) -> bool {
    relation_subject_identity(source) == relation_subject_identity(claim)
        || evaluation_subject_components(source)
            .into_iter()
            .any(|component| {
                relation_subject_identity(component) == relation_subject_identity(claim)
            })
}

fn evaluation_subject_components(subject: &[String]) -> Vec<&[String]> {
    subject
        .split(|word| word == "and")
        .filter(|component| !component.is_empty())
        .collect()
}

fn evaluative_conclusions_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let source_clauses = evidence
        .iter()
        .flat_map(|item| semantic_clauses(&item.exact_quote))
        .collect::<Vec<_>>();
    // Explicit rationale is a causal relation even when its consequence is
    // nominal. Pair by the owned consequence before comparing causes; unknown
    // consequence constructions do not acquire a fabricated conflict.
    let rationales = source_clauses
        .iter()
        .flat_map(|clause| explicit_causal_relations(clause))
        .collect::<Vec<_>>();
    for clause in claim_propositions(claim) {
        if let Some((consequence, cause)) = clause.split_once(" because ") {
            let consequence =
                relation_subject_identity(&causal_terms(&relation_form_words(consequence)));
            let cause = relation_subject_identity(&causal_terms(&relation_form_words(cause)));
            let paired = rationales
                .iter()
                .filter(|source| {
                    source
                        .consequence
                        .as_ref()
                        .is_some_and(|owned| relation_subject_identity(owned) == consequence)
                })
                .collect::<Vec<_>>();
            if !paired.is_empty()
                && !paired
                    .iter()
                    .any(|source| relation_subject_identity(&source.subject) == cause)
            {
                return false;
            }
        }
    }
    for concept in EVALUATIVE_CONCEPTS {
        let claim_relations = claim_propositions(claim)
            .flat_map(|clause| evaluation_relations(&clause, concept))
            .collect::<Vec<_>>();
        let source_relations = source_clauses
            .iter()
            .flat_map(|clause| {
                if *concept == EVALUATIVE_CONCEPTS[1] {
                    explicit_causal_relations(clause)
                } else {
                    evaluation_relations(clause, concept)
                }
            })
            .collect::<Vec<_>>();
        for relation in claim_relations {
            if relation.subject.is_empty() {
                return false;
            }
            let supported = if let Some(consequence) = &relation.consequence {
                !consequence.is_empty()
                    && source_relations.iter().any(|source| {
                        relation_subject_identity(&source.subject)
                            == relation_subject_identity(&relation.subject)
                            && source.negated == relation.negated
                            && source
                                .consequence
                                .as_ref()
                                .is_some_and(|source_consequence| {
                                    owned_complement_matches(
                                        source_consequence,
                                        consequence,
                                        source.governor.is_some()
                                            && source.governor == relation.governor,
                                    )
                                })
                    })
            } else {
                evaluation_subject_components(&relation.subject)
                    .iter()
                    .all(|component| {
                        source_relations.iter().any(|source| {
                            source.consequence.is_none()
                                && evaluated_subject_matches(&source.subject, component)
                                && source.negated == relation.negated
                        })
                    })
            };
            if !supported {
                return false;
            }
        }
    }
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AssignmentValue {
    Range { start: String, end: String },
    Literal(String),
}

#[derive(Debug, Clone)]
struct DefinitionRelation {
    subject: Vec<String>,
    value: AssignmentValue,
    scope: Vec<String>,
    negated: bool,
    prescribed_assignment: Option<ModalPredicate>,
    plain_assignment: bool,
}

fn definition_relation(clause: &str) -> Option<DefinitionRelation> {
    let tokens = original_words(clause);
    let lowered = words(clause);
    let (copula, value_start, prescribed) = lowered
        .windows(2)
        .position(|pair| pair == ["shall", "be"])
        .map(|index| (index, index + 2, true))
        .or_else(|| {
            lowered
                .iter()
                .position(|word| matches!(word.as_str(), "is" | "are"))
                .map(|index| (index, index + 1, false))
        })?;
    let subject = strip_leading_article(&lowered[..copula]).to_vec();
    if subject.is_empty()
        || subject.iter().any(|word| is_modal_anchor(word))
        || matches!(subject.as_slice(), [word] if matches!(word.as_str(), "it" | "they"))
    {
        return None;
    }
    let negated = lowered.get(value_start).is_some_and(|word| word == "not");
    let value_start = value_start + usize::from(negated);
    let first = tokens.get(value_start)?;
    // Retain punctuation and endpoint spelling. Token normalization is for
    // subject/scope identity only, never for the assigned literal or endpoints.
    let offset = clause
        .match_indices(first)
        .find(|(offset, _)| original_words(&clause[..*offset]).len() == value_start)?
        .0;
    let raw_value = clause[offset..].trim();
    let scope_start = (value_start..lowered.len()).find(|index| {
        qualifying_conjunct(&lowered[*index..])
            || matches!(lowered[*index].as_str(), "during" | "while" | "under")
    });
    let (raw_value, scope) = if let Some(index) = scope_start {
        let scope_word = tokens.get(index)?;
        let scope_offset = raw_value
            .match_indices(scope_word)
            .find(|(offset, _)| original_words(&raw_value[..*offset]).len() == index - value_start)?
            .0;
        let extent = raw_value[..scope_offset]
            .trim()
            .trim_end_matches(',')
            .trim();
        let extent = extent
            .strip_suffix(", but")
            .or_else(|| extent.strip_suffix(", and"))
            .unwrap_or(extent)
            .trim();
        (extent, lowered[index..].to_vec())
    } else {
        (raw_value, Vec::new())
    };
    let compact = raw_value.split_whitespace().collect::<Vec<_>>().join(" ");
    let range_body = compact.strip_prefix("from ").unwrap_or(&compact);
    let range = (compact.starts_with("from ")
        || range_body
            .chars()
            .next()
            .is_some_and(|first| first.is_uppercase() || first.is_numeric()))
    .then(|| {
        [" through ", " to ", "\u{2013}", "-"]
            .iter()
            .find_map(|connector| range_body.split_once(connector))
    })
    .flatten();
    let value = if let Some((start, end)) = range {
        if start.trim().is_empty()
            || end.trim().is_empty()
            || has_finite_predicate(&words(start))
            || has_finite_predicate(&words(end))
        {
            // An endpoint still containing a proposition is unclassified. It
            // cannot become a conflicting value or release source modality.
            return None;
        }
        AssignmentValue::Range {
            start: start.trim().into(),
            end: end.trim().into(),
        }
    } else if original_words(&compact).len() == 1
        && compact.chars().next().is_some_and(char::is_uppercase)
    {
        AssignmentValue::Literal(compact)
    } else {
        // Passive actions, prescribed states and arbitrary prose are not
        // recognized value assignments and receive no decision from this owner.
        return None;
    };
    Some(DefinitionRelation {
        subject,
        value,
        scope,
        negated,
        prescribed_assignment: prescribed
            .then(|| modal_predicates(clause, true))
            .and_then(|mut predicates| (predicates.len() == 1).then(|| predicates.remove(0))),
        plain_assignment: !prescribed,
    })
}

struct DefinitionDecision {
    supported: bool,
    released_modals: Vec<ModalPredicate>,
}

fn definition_value_decision(claim: &str, evidence: &[&EvidenceItem]) -> DefinitionDecision {
    let sources = evidence
        .iter()
        .flat_map(|item| semantic_clauses(&item.exact_quote))
        .filter_map(|clause| definition_relation(&clause))
        .collect::<Vec<_>>();
    let mut decision = DefinitionDecision {
        supported: true,
        released_modals: Vec::new(),
    };
    for assignment in claim_propositions(claim).filter_map(|clause| definition_relation(&clause)) {
        let paired = sources
            .iter()
            .filter(|source| source.subject == assignment.subject)
            .collect::<Vec<_>>();
        if paired.is_empty() {
            continue;
        }
        let equal = paired.iter().all(|source| {
            source.value == assignment.value
                && source.scope == assignment.scope
                && source.negated == assignment.negated
        });
        if !equal {
            decision.supported = false;
            continue;
        }
        if assignment.plain_assignment {
            decision.released_modals.extend(
                paired
                    .into_iter()
                    .filter_map(|source| source.prescribed_assignment.clone()),
            );
        }
    }
    decision
}

// Use the existing predicate/context owner to recognize an assertion of the
// same source proposition with its modal removed. No writer parser is changed.
fn plain_assertion_for_modal(claim: &str, source: &ModalPredicate) -> Option<ModalPredicate> {
    for clause in claim_propositions(claim)
        .flat_map(|clause| clause.split(',').map(str::to_owned).collect::<Vec<_>>())
    {
        let tokens = necessity_projection(&words(&clause)).tokens;
        for (index, word) in tokens.iter().enumerate() {
            if word != &source.predicate && word != &format!("{}s", source.predicate) {
                continue;
            }
            if tokens[..index].iter().any(|word| is_modal_anchor(word)) {
                continue;
            }
            let mut predicate = modal_occurrence(&tokens, index, index, None)?;
            // Canonicalize only the inflection of this already-owned action.
            predicate.predicate.clone_from(&source.predicate);
            if modal_statements_match(&predicate, source, false) {
                return Some(predicate);
            }
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PermissionProposition {
    subject: Vec<String>,
    action: Vec<String>,
    negated: bool,
}

// Full permission propositions, including every condition/scope token, own
// equivalence. ModalPredicate's bounded contexts are not sufficient for this.
fn permission_propositions(text: &str) -> Option<Vec<PermissionProposition>> {
    let mut result = Vec::new();
    for clause in semantic_clauses(text) {
        let tokens = original_words(&clause)
            .into_iter()
            .map(|word| word.to_lowercase())
            .collect::<Vec<_>>();
        let permission = tokens
            .iter()
            .position(|word| matches!(word.as_str(), "allow" | "allows" | "permit" | "permits"));
        if let Some(index) = permission {
            if index > 0 && tokens[index - 1] == "to" {
                continue;
            }
            if negation_present(&tokens[..index]) {
                return None;
            }
            // The governing permission applies only to explicit actor-to-action
            // items. A bare or shared-actor list receives no invented alignment.
            // Recover comma boundaries from the original clause, not token bags.
            let first = original_words(&clause).get(index + 1)?.clone();
            let offset = clause
                .match_indices(&first)
                .find(|(offset, _)| original_words(&clause[..*offset]).len() == index + 1)?
                .0;
            for item in clause[offset..].split(',') {
                let item = relation_form_words(item);
                let item = item.strip_prefix(&["and".into()]).unwrap_or(&item);
                let action = item.iter().position(|word| word == "to")?;
                if action == 0 || action + 1 >= item.len() {
                    return None;
                }
                result.push(PermissionProposition {
                    subject: relation_subject_identity(&item[..action]),
                    action: item[action + 1..].to_vec(),
                    negated: false,
                });
            }
        } else {
            let tokens = relation_form_words(&clause);
            let Some(index) = tokens.iter().position(|word| word == "may") else {
                continue;
            };
            if index == 0 || index + 1 >= tokens.len() || tokens[index + 1] == "be" {
                // Stative may-be is epistemic/conditional, not an owned grant.
                continue;
            }
            let negated = tokens.get(index + 1).is_some_and(|word| word == "not");
            result.push(PermissionProposition {
                subject: relation_subject_identity(&tokens[..index]),
                action: tokens[index + 1 + usize::from(negated)..].to_vec(),
                negated,
            });
        }
    }
    Some(result)
}

fn permission_equivalence(claim: &str, evidence: &[&EvidenceItem]) -> Option<Vec<ModalPredicate>> {
    let has_form = |text: &str| {
        words(text)
            .iter()
            .any(|word| matches!(word.as_str(), "allow" | "allows" | "permit" | "permits"))
    };
    let explicit_claim = has_form(claim);
    let sources = evidence
        .iter()
        .flat_map(|item| semantic_clauses(&item.exact_quote))
        .filter_map(|clause| permission_propositions(&clause))
        .flatten()
        .collect::<Vec<_>>();
    // A permission elsewhere in the evidence is not a decision about this
    // claim. Only a recognized explicit grant or its aligned may counterpart
    // engages this comparator.
    if !explicit_claim {
        if !evidence.iter().any(|item| has_form(&item.exact_quote)) {
            return Some(Vec::new());
        }
        let requests =
            permission_propositions(&claim_propositions(claim).collect::<Vec<_>>().join("; "))
                .unwrap_or_default();
        for request in requests {
            let paired = sources
                .iter()
                .filter(|source| {
                    source.subject == request.subject
                        && source.action.first() == request.action.first()
                })
                .collect::<Vec<_>>();
            if !paired.is_empty() && !paired.contains(&&request) {
                return None;
            }
        }
        return Some(Vec::new());
    }
    if !words(claim).iter().any(|word| word == "to")
        || sources.is_empty()
        || !evidence
            .iter()
            .any(|item| words(&item.exact_quote).iter().any(|word| word == "may"))
    {
        return Some(Vec::new());
    }
    let requests =
        permission_propositions(&claim_propositions(claim).collect::<Vec<_>>().join("; "))?;
    let mut used = HashSet::new();
    let mut projected = Vec::new();
    for request in requests {
        let index = sources
            .iter()
            .enumerate()
            .position(|(index, source)| !used.contains(&index) && source == &request)?;
        used.insert(index);
        let text = format!(
            "{} may {}{}",
            request.subject.join(" "),
            if request.negated { "not " } else { "" },
            request.action.join(" ")
        );
        projected.extend(modal_predicates(&text, false));
    }
    if projected.is_empty() {
        return None;
    }
    Some(projected)
}

#[derive(Debug, PartialEq, Eq)]
struct RuleConsequence {
    event: Vec<String>,
    obligor: Vec<String>,
    action: Vec<String>,
    negated: bool,
}

fn rule_consequence(clause: &str) -> Option<RuleConsequence> {
    let tokens = relation_form_words(clause);
    // A stated failure consequence, not a hypothetical would assertion.
    if !tokens.starts_with(&["failure".into(), "to".into()]) {
        return None;
    }
    let predicate = tokens
        .iter()
        .position(|word| matches!(word.as_str(), "require" | "requires"))?;
    let mut event = tokens[..predicate].to_vec();
    while event
        .last()
        .is_some_and(|word| matches!(word.as_str(), "would" | "does" | "do" | "not"))
    {
        event.pop();
    }
    let tail = &tokens[predicate + 1..];
    let action = tail.iter().position(|word| word == "to")?;
    if action == 0 || action + 1 >= tail.len() {
        return None;
    }
    Some(RuleConsequence {
        event,
        obligor: relation_subject_identity(&tail[..action]),
        action: tail[action + 1..].to_vec(),
        negated: negation_present(&tokens[..=predicate]),
    })
}

fn stated_rule_consequences_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let sources = evidence
        .iter()
        .flat_map(|item| semantic_clauses(&item.exact_quote))
        .filter_map(|clause| rule_consequence(&clause))
        .collect::<Vec<_>>();
    claim_propositions(claim)
        .filter_map(|clause| rule_consequence(&clause))
        .all(|request| {
            let paired = sources
                .iter()
                .filter(|source| source.event == request.event)
                .collect::<Vec<_>>();
            paired.is_empty() || paired.iter().any(|source| **source == request)
        })
}

#[cfg(test)]
fn modal_force_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let definitions = definition_value_decision(claim, evidence);
    definitions.supported && modal_force_with_definitions(claim, evidence, &definitions)
}

fn modal_force_with_definitions(
    claim: &str,
    evidence: &[&EvidenceItem],
    definitions: &DefinitionDecision,
) -> bool {
    if !stated_rule_consequences_supported(claim, evidence) {
        return false;
    }
    let guarded_modal_predicates = |text: &str, strong: bool, claim_side: bool| {
        let clauses = if claim_side {
            claim_propositions(text).collect::<Vec<_>>()
        } else {
            semantic_clauses(text).collect::<Vec<_>>()
        };
        clauses
            .into_iter()
            .flat_map(|clause| clause.split(',').map(str::to_owned).collect::<Vec<_>>())
            .flat_map(|clause| {
                let mut tokens = necessity_projection(&words(&clause)).tokens;
                if rule_consequence(&clause).is_some() {
                    tokens.retain(|word| word != "would");
                }
                let mut normalized = Vec::with_capacity(tokens.len());
                let mut index = 0;
                while index < tokens.len() {
                    if tokens[index] == "cannot" {
                        normalized.push("can".into());
                        normalized.push("not".into());
                        index += 1;
                        continue;
                    }
                    let contraction = tokens.get(index + 1).is_some_and(|word| word == "t")
                        && matches!(
                            tokens[index].as_str(),
                            "can"
                                | "couldn"
                                | "mayn"
                                | "mightn"
                                | "mustn"
                                | "shalln"
                                | "shouldn"
                                | "won"
                                | "wouldn"
                        );
                    if contraction {
                        normalized.push(
                            match tokens[index].as_str() {
                                "couldn" => "could",
                                "mayn" => "may",
                                "mightn" => "might",
                                "mustn" => "must",
                                "shalln" => "shall",
                                "shouldn" => "should",
                                "won" => "will",
                                "wouldn" => "would",
                                "can" => "can",
                                _ => unreachable!("modal contraction is exhaustively matched"),
                            }
                            .to_string(),
                        );
                        normalized.push("not".into());
                        index += 2;
                    } else {
                        normalized.push(tokens[index].clone());
                        index += 1;
                    }
                }
                modal_predicates(&normalized.join(" "), strong)
            })
            .collect::<Vec<_>>()
    };
    let weak_source = evidence
        .iter()
        .flat_map(|item| guarded_modal_predicates(&item.exact_quote, false, false))
        .collect::<Vec<_>>();
    let strong_source = evidence
        .iter()
        .flat_map(|item| guarded_modal_predicates(&item.exact_quote, true, false))
        .collect::<Vec<_>>();
    let strong_claim = guarded_modal_predicates(claim, true, true);
    let mut weak_claim = guarded_modal_predicates(claim, false, true);
    let Some(permission_claim) = permission_equivalence(claim, evidence) else {
        return false;
    };
    weak_claim.extend(permission_claim);
    let strengthening_supported = strong_claim.iter().all(|predicate| {
        let matching_weak_source = weak_source
            .iter()
            .any(|source| modal_statements_match(predicate, source, false));
        let matching_strong_source = strong_source
            .iter()
            .any(|source| modal_statements_match(predicate, source, false));
        let matching_strong_polarity = strong_source
            .iter()
            .any(|source| modal_statements_match(predicate, source, true));
        (!matching_weak_source || matching_strong_polarity)
            && (!matching_strong_source || matching_strong_polarity)
    });
    if !strengthening_supported {
        return false;
    }
    for (source_strong, source) in strong_source
        .iter()
        .map(|predicate| (true, predicate))
        .chain(weak_source.iter().map(|predicate| (false, predicate)))
    {
        if source_strong && definitions.released_modals.contains(source) {
            continue;
        }
        let matching_strong = strong_claim
            .iter()
            .any(|claim| modal_statements_match(claim, source, true));
        let matching_weak = weak_claim
            .iter()
            .any(|claim| modal_statements_match(claim, source, true));
        let asserted = strong_claim
            .iter()
            .chain(&weak_claim)
            .any(|claim| modal_statements_match(claim, source, false))
            || plain_assertion_for_modal(claim, source).is_some();
        if !asserted {
            continue;
        }
        let same_strong_source = strong_source
            .iter()
            .any(|strong| modal_statements_match(strong, source, true));
        if source_strong && !matching_strong
            || !source_strong && !matching_weak && !(matching_strong && same_strong_source)
        {
            return false;
        }
    }
    true
}

fn semantic_fidelity_supported(claim: &str, evidence: &[&EvidenceItem]) -> bool {
    let definitions = definition_value_decision(claim, evidence);
    definitions.supported
        && comparison_boundaries_supported(claim, evidence)
        && broader_enumeration_supported(claim, evidence)
        && directional_endpoints_supported(claim, evidence)
        && actor_qualifications_supported(claim, evidence)
        && evaluative_conclusions_supported(claim, evidence)
        && modal_force_with_definitions(claim, evidence, &definitions)
}

#[cfg(test)]
pub(in crate::pipeline::summary) fn relation_guard_rejection_reasons(
    claim: &str,
    evidence: &[&EvidenceItem],
) -> Vec<&'static str> {
    // Diagnostics call the decision owners; they never decide delivery.
    let mut reasons = Vec::new();
    if !evaluative_conclusions_supported(claim, evidence) {
        reasons.push("evaluation_or_causal_relation");
    }
    let definitions = definition_value_decision(claim, evidence);
    if !definitions.supported {
        reasons.push("definition_value");
    }
    if !modal_force_with_definitions(claim, evidence, &definitions) {
        reasons.push("modal_preservation");
    }
    reasons
}

pub(in crate::pipeline::summary) fn apply_semantic_fidelity_guards(
    claims: &[CitedClaim],
    evidence: &[EvidenceItem],
    verifications: &mut [ClaimVerification],
) -> Result<(), PipelineFailure> {
    if claims.len() != verifications.len() {
        return Err(stage_failure(
            PipelineStage::Verify,
            "INVALID_VERIFICATION_RESPONSE",
            "Semantic-fidelity verification coverage must match the coherent claim catalog",
            false,
        ));
    }
    let evidence = evidence
        .iter()
        .map(|item| (item.evidence_id.as_str(), item))
        .collect::<HashMap<_, _>>();
    for (claim, verification) in claims.iter().zip(verifications) {
        if verification.claim_id != claim.claim_id
            || verification.evidence_ids != claim.evidence_ids
        {
            return Err(stage_failure(
                PipelineStage::Verify,
                "INVALID_VERIFICATION_RESPONSE",
                "Semantic-fidelity verification identity must match the coherent claim catalog",
                false,
            ));
        }
        if verification.verdict != ClaimVerdict::Supported {
            continue;
        }
        let cited = claim
            .evidence_ids
            .iter()
            .map(|evidence_id| evidence.get(evidence_id.as_str()).copied())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| {
                stage_failure(
                    PipelineStage::Verify,
                    "INVALID_SYNTHESIZED_DOCUMENT",
                    "Semantic-fidelity verification references unknown synthesis evidence",
                    false,
                )
            })?;
        if !semantic_fidelity_supported(&claim.text, &cited) {
            verification.verdict = ClaimVerdict::Unsupported;
        }
    }
    Ok(())
}
