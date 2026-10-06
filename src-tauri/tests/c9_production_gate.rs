//! Integration tests link the ordinary library, where cfg(test) is false.
//! Unit tests alone cannot prove that the qualification candidate stays off.
use document_summarizer_lib::pipeline::{
    chunk::{chunk_document, DeterministicDocumentChunker},
    contracts::{
        DocumentParser, IngestedDocument, ModelOutputFormat, ModelRequest, ModelResponse,
        ModelRuntime, ModelRuntimeFailure, ParsedDocument, ParsedPage, PipelineFailure,
        PipelineStage, SourceType, SummaryPresentationMode,
    },
    db::{get_citation_artifact, get_summary_artifact, get_verified_document, init_db},
    ingest::ingest_pdf,
    normalize::{normalize_document, CanonicalNormalizer},
    parser::parse_document,
    structure::{structure_document, DeterministicStructureInterpreter},
    summary::{
        analyze_chunked_document, complete_verified_document, synthesize_analyzed_document,
        verify_synthesized_document,
    },
    workspace::get_persisted_summary,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Mutex};
use uuid::Uuid;

const SOURCE: &str = "The safety officer inspects each pump every Monday. A missing guard requires the affected pump to stay off until a technician signs a repair record.";

struct PublicParser;
impl DocumentParser for PublicParser {
    fn id(&self) -> &'static str {
        "qualification-gate-fixture"
    }
    fn version(&self) -> &'static str {
        "1"
    }
    fn parse(&self, document: &IngestedDocument) -> Result<ParsedDocument, PipelineFailure> {
        Ok(ParsedDocument {
            document_id: document.document_id.clone(),
            parser_id: self.id().into(),
            parser_version: self.version().into(),
            source_type: SourceType::NativeText,
            pages: vec![ParsedPage {
                page_number: 1,
                text: SOURCE.into(),
                warnings: vec![],
                requires_visual_processing: false,
            }],
            warnings: vec![],
        })
    }
}

#[derive(Default)]
struct Runtime {
    calls: Mutex<Vec<PipelineStage>>,
    forbid_inference: bool,
    reject_claims: bool,
}
impl ModelRuntime for Runtime {
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        assert!(
            !self.forbid_inference,
            "pending coherent checkpoint reached inference"
        );
        self.calls.lock().unwrap().push(request.stage.clone());
        let ModelOutputFormat::JsonSchema { name, .. } = &request.output_format else {
            panic!("structured request required")
        };
        let prompt: Value = serde_json::from_str(&request.user_prompt).unwrap();
        let response = match request.stage {
            PipelineStage::Analyze if name == "document_page_quote_selection_v4" => {
                json!({"selection": prompt["quote_candidates"][0]["quote_id"]})
            }
            PipelineStage::Analyze if name == "document_quote_paraphrase_v5" => {
                json!({"claim_text": prompt["exact_quote"]})
            }
            PipelineStage::Synthesize => panic!("unqualified synthesis reached runtime"),
            PipelineStage::Verify => {
                assert_ne!(
                    name, "document_claim_comparisons_v4",
                    "unqualified comparison reached runtime"
                );
                json!({"verdicts": prompt["claims"].as_array().unwrap().iter().map(|c| json!({"claim_id": c["claim_id"], "verdict": if self.reject_claims { "unsupported" } else { "supported" }})).collect::<Vec<_>>()})
            }
            _ => panic!("unexpected fixture request {name}"),
        };
        Ok(ModelResponse {
            text: response.to_string(),
            runtime_id: self.runtime_id().into(),
            model_id: self.model_id().into(),
            request_attempts: vec![],
        })
    }
    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        Ok(())
    }
    fn runtime_id(&self) -> &str {
        "gate-fixture"
    }
    fn model_id(&self) -> &str {
        "gate-fixture"
    }
    fn context_tokens(&self, _: PipelineStage) -> u32 {
        32_768
    }
    fn supports_response_schema(&self, _: &str) -> bool {
        true
    }
}

struct Database(PathBuf);
impl Database {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("docsum-gate-{}.db", Uuid::new_v4())))
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn analyzed(db: &Database, runtime: &Runtime) -> (Connection, String) {
    let mut conn = init_db(&db.0).unwrap();
    let pdf =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
    let (_, run) = ingest_pdf(&mut conn, pdf.to_str().unwrap()).unwrap();
    let id = run.run_id;
    parse_document(&mut conn, &PublicParser, &id).unwrap();
    normalize_document(&mut conn, &CanonicalNormalizer::new(), &id).unwrap();
    structure_document(&mut conn, &DeterministicStructureInterpreter::new(), &id).unwrap();
    chunk_document(&mut conn, &DeterministicDocumentChunker::new(), &id).unwrap();
    analyze_chunked_document(&mut conn, runtime, &id).unwrap();
    runtime.calls.lock().unwrap().clear();
    (conn, id)
}

#[test]
fn ordinary_library_withholds_unqualified_prose_before_drafting() {
    let db = Database::new();
    let runtime = Runtime::default();
    let (mut conn, id) = analyzed(&db, &runtime);
    let synthesized = synthesize_analyzed_document(&mut conn, &runtime, &id).unwrap();
    assert_eq!(
        synthesized.presentation_mode,
        SummaryPresentationMode::ClaimLedgerFallback
    );
    assert!(synthesized.summary_claims.is_empty());
    assert!(synthesized
        .warnings
        .iter()
        .any(|w| w.code == "COHERENT_SUMMARY_VERIFICATION_UNQUALIFIED"));
    assert!(runtime.calls.lock().unwrap().is_empty());
    let verified = verify_synthesized_document(&mut conn, &runtime, &id).unwrap();
    assert_eq!(
        verified.presentation_mode,
        SummaryPresentationMode::ClaimLedgerFallback
    );
    assert_ne!(verified.verification_version, "15.0.0");
    assert!(
        !runtime.calls.lock().unwrap().is_empty(),
        "source claims still require verification"
    );
    let completed = complete_verified_document(&mut conn, &id).unwrap();
    let view = get_persisted_summary(&conn, &id).unwrap();
    drop(conn);
    let conn = init_db(&db.0).unwrap();
    assert_eq!(get_persisted_summary(&conn, &id).unwrap(), view);
    assert_eq!(
        get_summary_artifact(&conn, &id).unwrap().unwrap(),
        completed.summary
    );
    assert_eq!(
        get_citation_artifact(&conn, &id).unwrap().unwrap(),
        completed.citations
    );
}

#[test]
fn fallback_still_withholds_unsupported_source_claims() {
    let db = Database::new();
    let runtime = Runtime {
        reject_claims: true,
        ..Default::default()
    };
    let (mut conn, id) = analyzed(&db, &runtime);
    synthesize_analyzed_document(&mut conn, &runtime, &id).unwrap();
    assert_eq!(
        verify_synthesized_document(&mut conn, &runtime, &id)
            .unwrap_err()
            .code(),
        "NO_SEMANTICALLY_SUPPORTED_CLAIMS"
    );
    assert!(get_verified_document(&conn, &id).unwrap().is_none());
    assert!(get_summary_artifact(&conn, &id).unwrap().is_none());
}

// Recorded through the public pipeline before the activation fix, with the
// public text above and scripted responses. No model qualification is implied.
fn recorded_checkpoint(db: &Database, state: &str) -> (Connection, String) {
    let mut conn = init_db(&db.0).unwrap();
    let mut fixture: Value =
        serde_json::from_str(include_str!("fixtures/c9-public-checkpoint.json")).unwrap();
    if state == "verified" {
        // Replay the recorded Verified transition, before artifact completion.
        let mut rows = fixture["completed"].clone();
        rows.as_object_mut().unwrap().remove("citation_artifacts");
        rows.as_object_mut().unwrap().remove("summary_artifacts");
        rows["pipeline_events"].as_array_mut().unwrap().pop();
        let event = rows["pipeline_events"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .clone();
        rows["pipeline_runs"][0]["state"] = event["next_state"].clone();
        rows["pipeline_runs"][0]["state_version"] =
            json!(event["sequence_no"].as_u64().unwrap() + 1);
        rows["pipeline_runs"][0]["updated_at"] = event["timestamp"].clone();
        rows["pipeline_runs"][0]["completed_at"] = Value::Null;
        fixture["verified"] = rows;
    }
    let tx = conn.transaction().unwrap();
    tx.execute_batch("PRAGMA defer_foreign_keys = ON").unwrap();
    for tables in [&fixture["common"], &fixture[state]] {
        for (table, rows) in tables.as_object().unwrap() {
            for row in rows.as_array().unwrap() {
                let object = row.as_object().unwrap();
                let columns = object
                    .keys()
                    .map(|k| format!("\"{k}\""))
                    .collect::<Vec<_>>()
                    .join(",");
                let marks = vec!["?"; object.len()].join(",");
                let values = object
                    .values()
                    .map(|v| match v {
                        Value::Null => rusqlite::types::Value::Null,
                        Value::Number(n) => rusqlite::types::Value::Integer(n.as_i64().unwrap()),
                        Value::String(s) => rusqlite::types::Value::Text(s.clone()),
                        _ => panic!("unexpected SQLite fixture value"),
                    })
                    .collect::<Vec<_>>();
                tx.execute(
                    &format!("INSERT INTO \"{table}\" ({columns}) VALUES ({marks})"),
                    rusqlite::params_from_iter(values),
                )
                .unwrap();
            }
        }
    }
    tx.commit().unwrap();
    let id = fixture[state]["pipeline_runs"][0]["run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    (conn, id)
}

#[test]
fn pending_coherent_checkpoint_stops_before_inference() {
    let db = Database::new();
    let (mut conn, id) = recorded_checkpoint(&db, "pending");
    let runtime = Runtime {
        forbid_inference: true,
        ..Default::default()
    };
    let error = verify_synthesized_document(&mut conn, &runtime, &id).unwrap_err();
    assert_eq!(error.code(), "VERIFICATION_NOT_QUALIFIED");
    assert!(runtime.calls.lock().unwrap().is_empty());
    assert!(get_verified_document(&conn, &id).unwrap().is_none());
    assert!(get_summary_artifact(&conn, &id).unwrap().is_none());
}

#[test]
fn verified_pending_checkpoint_cannot_create_a_new_summary() {
    let db = Database::new();
    let (mut conn, id) = recorded_checkpoint(&db, "verified");
    let result = complete_verified_document(&mut conn, &id);
    assert!(
        result.is_err(),
        "unqualified verified checkpoint created a new summary"
    );
    assert_eq!(result.unwrap_err().code(), "VERIFICATION_NOT_QUALIFIED");
    assert!(get_summary_artifact(&conn, &id).unwrap().is_none());
    assert!(get_citation_artifact(&conn, &id).unwrap().is_none());
}

#[test]
fn completed_coherent_artifacts_remain_readable() {
    let db = Database::new();
    let (conn, id) = recorded_checkpoint(&db, "completed");
    let summary = get_summary_artifact(&conn, &id).unwrap().unwrap();
    let citations = get_citation_artifact(&conn, &id).unwrap().unwrap();
    let view = get_persisted_summary(&conn, &id).unwrap();
    assert_eq!(
        citations.presentation_mode,
        SummaryPresentationMode::Coherent
    );
    assert!(summary.text.contains("safety officer"));
    assert!(!citations.summary_claims.is_empty());
    assert_eq!(
        summary.calculate_integrity_hash().unwrap(),
        summary.integrity_hash
    );
    drop(conn);
    let conn = init_db(&db.0).unwrap();
    assert_eq!(get_persisted_summary(&conn, &id).unwrap(), view);
    assert_eq!(get_summary_artifact(&conn, &id).unwrap().unwrap(), summary);
    assert_eq!(
        get_citation_artifact(&conn, &id).unwrap().unwrap(),
        citations
    );
}
