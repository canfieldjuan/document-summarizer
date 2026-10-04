//! Local operator corrections are immutable source revisions, never edits to delivered results.
use crate::pipeline::contracts::{
    IngestedDocument, ParsedDocument, PipelineRun, PipelineStage, PipelineState, PipelineWarning,
    SourceType,
};
use crate::pipeline::db::{self, StoreError};
use crate::pipeline::ingest::prepare_received_run;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::PathBuf;
use thiserror::Error;
use uuid::Uuid;

const MAX_TEXT_BYTES: usize = 262_144;
pub const CORRECTION_WARNING: &str = "OPERATOR_CORRECTED_OCR_TEXT";

#[derive(Debug, Error)]
pub enum CorrectionError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("OCR correction persistence failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("OCR correction serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("This run has no editable OCR source checkpoint")]
    NotAllowed,
    #[error("The source changed while it was being reviewed; reopen it before saving")]
    StaleSource,
    #[error("A corrected successor already exists; open that run to make further corrections")]
    AlreadyCorrected,
    #[error("Invalid OCR correction: {0}")]
    Invalid(String),
    #[error("The stored OCR correction failed its integrity check")]
    Integrity,
    #[error("The retained source PDF is missing, changed, or cannot be opened")]
    SourceUnavailable,
}

impl CorrectionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotAllowed => "OCR_CORRECTION_NOT_ALLOWED",
            Self::StaleSource => "OCR_CORRECTION_STALE_SOURCE",
            Self::AlreadyCorrected => "OCR_CORRECTION_ALREADY_EXISTS",
            Self::Invalid(_) => "OCR_CORRECTION_INVALID",
            Self::Integrity => "OCR_CORRECTION_INTEGRITY_FAILED",
            Self::SourceUnavailable => "OCR_CORRECTION_SOURCE_UNAVAILABLE",
            Self::Store(_) | Self::Sqlite(_) | Self::Json(_) => "OCR_CORRECTION_STORE_ERROR",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageCorrection {
    pub page_number: u32,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveCorrection {
    pub run_id: String,
    pub expected_state_version: u32,
    pub source_parsed_hash: String,
    pub pages: Vec<PageCorrection>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrReview {
    pub run_id: String,
    pub expected_state_version: u32,
    pub source_parsed_hash: String,
    pub pages: Vec<PageCorrection>,
    pub correction_of_run_id: Option<String>,
    pub corrected_run_id: Option<String>,
}

fn hash(parsed: &ParsedDocument) -> Result<String, CorrectionError> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(parsed)?)))
}

fn source(
    conn: &Connection,
    run_id: &str,
) -> Result<(PipelineRun, IngestedDocument, ParsedDocument), CorrectionError> {
    let run = db::get_pipeline_run(conn, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    let document = db::get_document(conn, &run.document_id)?
        .ok_or_else(|| StoreError::DocumentNotFound(run.document_id.clone()))?;
    if document.source_type != SourceType::OcrText
        || !matches!(
            run.state,
            PipelineState::Complete
                | PipelineState::CompleteWithWarnings
                | PipelineState::Failed
                | PipelineState::Parsed
        )
    {
        return Err(CorrectionError::NotAllowed);
    }
    let parsed = db::get_parsed_document(conn, run_id)?.ok_or(CorrectionError::NotAllowed)?;
    if parsed.source_type != SourceType::OcrText || parsed.document_id != document.document_id {
        return Err(CorrectionError::Integrity);
    }
    Ok((run, document, parsed))
}

pub fn get_review(conn: &Connection, run_id: &str) -> Result<Option<OcrReview>, CorrectionError> {
    let resolved =
        db::get_admitted_ocr_child_run_id(conn, run_id)?.unwrap_or_else(|| run_id.to_string());
    let (run, document, parsed) = match source(conn, &resolved) {
        Ok(source) => source,
        Err(CorrectionError::NotAllowed) => return Ok(None),
        Err(error) => return Err(error),
    };
    let correction_of_run_id = conn
        .query_row(
            "SELECT source_run_id FROM ocr_text_corrections WHERE document_id = ?1",
            [&document.document_id],
            |row| row.get(0),
        )
        .optional()?;
    let corrected_run_id = conn
        .query_row(
            "SELECT run_id FROM ocr_text_corrections WHERE source_document_id = ?1",
            [&document.document_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(Some(OcrReview {
        run_id: run.run_id,
        expected_state_version: run.state_version,
        source_parsed_hash: hash(&parsed)?,
        pages: parsed
            .pages
            .into_iter()
            .map(|page| PageCorrection {
                page_number: page.page_number,
                text: page.text,
            })
            .collect(),
        correction_of_run_id,
        corrected_run_id,
    }))
}

fn apply_pages(
    parsed: &mut ParsedDocument,
    changes: &[PageCorrection],
) -> Result<(), CorrectionError> {
    if changes.is_empty() || changes.len() > parsed.pages.len() {
        return Err(CorrectionError::Invalid(
            "supply changed pages only".to_string(),
        ));
    }
    let mut seen = HashSet::new();
    let mut changed = false;
    for edit in changes {
        if !seen.insert(edit.page_number) {
            return Err(CorrectionError::Invalid("duplicate page".to_string()));
        }
        let page = parsed
            .pages
            .iter_mut()
            .find(|page| page.page_number == edit.page_number)
            .ok_or_else(|| CorrectionError::Invalid("unknown page".to_string()))?;
        if edit.text.trim().is_empty() || edit.text.contains('\0') {
            return Err(CorrectionError::Invalid(
                "page text must be nonempty and contain no NUL".to_string(),
            ));
        }
        changed |= page.text != edit.text;
        page.text = edit.text.clone();
    }
    if !changed {
        return Err(CorrectionError::Invalid("no text changed".to_string()));
    }
    if parsed
        .pages
        .iter()
        .map(|page| page.text.len())
        .sum::<usize>()
        > MAX_TEXT_BYTES
    {
        return Err(CorrectionError::Invalid(
            "corrected text exceeds 262144 UTF-8 bytes".to_string(),
        ));
    }
    if !parsed
        .warnings
        .iter()
        .any(|warning| warning.code == CORRECTION_WARNING)
    {
        parsed.warnings.push(PipelineWarning {
            code: CORRECTION_WARNING.to_string(),
            message: "This summary uses operator-corrected OCR text. Citations refer to that corrected text; compare it with the retained source PDF. Previously delivered results are unchanged.".to_string(),
            stage: Some(PipelineStage::Parse),
        });
    }
    Ok(())
}

pub fn save(
    conn: &mut Connection,
    request: &SaveCorrection,
) -> Result<PipelineRun, CorrectionError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (source_run, mut document, mut parsed) = source(&tx, &request.run_id)?;
    if source_run.state_version != request.expected_state_version
        || hash(&parsed)? != request.source_parsed_hash
    {
        return Err(CorrectionError::StaleSource);
    }
    apply_pages(&mut parsed, &request.pages)?;
    let existing: Option<(String, String, String)> = tx.query_row(
        "SELECT run_id, document_id, source_run_id FROM ocr_text_corrections WHERE source_document_id = ?1",
        [&document.document_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).optional()?;
    if let Some((run_id, document_id, source_run_id)) = existing {
        parsed.document_id = document_id.clone();
        let prior = load_for_document(&tx, &document_id)?.ok_or(CorrectionError::Integrity)?;
        if source_run_id != request.run_id || parsed != prior {
            return Err(CorrectionError::AlreadyCorrected);
        }
        return db::get_pipeline_run(&tx, &run_id)?.ok_or(CorrectionError::Integrity);
    }
    let source_document_id = document.document_id.clone();
    document.document_id = Uuid::new_v4().to_string();
    document.created_at = Utc::now();
    parsed.document_id = document.document_id.clone();
    let new_run = prepare_received_run(document.document_id.clone(), document.created_at);
    let run = db::persist_corrected_document(&tx, &document, &new_run, &parsed, &request.run_id)?;
    tx.execute(
        "INSERT INTO ocr_text_corrections
         (document_id, run_id, source_document_id, source_run_id, source_version,
          source_parsed_hash, corrected_parsed_hash, corrected_parsed, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            document.document_id,
            run.run_id,
            source_document_id,
            request.run_id,
            request.expected_state_version,
            request.source_parsed_hash,
            hash(&parsed)?,
            serde_json::to_string(&parsed)?,
            document.created_at.to_rfc3339()
        ],
    )?;
    tx.commit()?;
    Ok(run)
}

pub(crate) fn load_for_document(
    conn: &Connection,
    document_id: &str,
) -> Result<Option<ParsedDocument>, CorrectionError> {
    let row: Option<(String, String)> = conn.query_row(
        "SELECT corrected_parsed, corrected_parsed_hash FROM ocr_text_corrections WHERE document_id = ?1",
        [document_id], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    row.map(|(json, expected_hash)| {
        let parsed: ParsedDocument = serde_json::from_str(&json)?;
        if hash(&parsed)? != expected_hash
            || parsed.document_id != document_id
            || parsed.source_type != SourceType::OcrText
            || !parsed
                .warnings
                .iter()
                .any(|warning| warning.code == CORRECTION_WARNING)
        {
            return Err(CorrectionError::Integrity);
        }
        Ok(parsed)
    })
    .transpose()
}

pub fn verified_source_path(conn: &Connection, run_id: &str) -> Result<PathBuf, CorrectionError> {
    let (_, document, _) = source(conn, run_id)?;
    let path = PathBuf::from(&document.local_source_path);
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
    {
        return Err(CorrectionError::SourceUnavailable);
    }
    let bytes = std::fs::read(&path).map_err(|_| CorrectionError::SourceUnavailable)?;
    if !bytes.starts_with(b"%PDF-")
        || bytes.len() as u64 != document.byte_size
        || format!("{:x}", Sha256::digest(&bytes)) != document.content_hash
    {
        return Err(CorrectionError::SourceUnavailable);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contracts::{
        DocumentNormalizer, ModelProfileSnapshot, ModelStageProfileSnapshot, PipelineFailure,
        SummaryProfile,
    };
    use crate::pipeline::ingest::prepare_pdf_ingestion_with_source_type;
    use crate::pipeline::normalize::CanonicalNormalizer;
    use crate::pipeline::parser::{parse_document, parse_started_document, SourceParserSet};

    fn setup(path: &std::path::Path) -> (Connection, SaveCorrection) {
        let mut conn = db::init_db(path).unwrap();
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ocr_tagged.pdf");
        let (document, run) =
            prepare_pdf_ingestion_with_source_type(fixture, None, SourceType::OcrText).unwrap();
        let stage = ModelStageProfileSnapshot {
            runtime_kind: Default::default(),
            profile_id: "fixture-v1".into(),
            model_name: "fixture".into(),
            model_digest: "fixture-digest".into(),
            context_tokens: 8192,
            tokenizer_version: "fixture-v1".into(),
        };
        let profile = ModelProfileSnapshot {
            version: 1,
            preset_id: "fixture-v1".into(),
            analysis: stage.clone(),
            verification: stage,
        };
        db::persist_ingestion_with_profiles(
            &mut conn,
            &document,
            &run,
            Some(&profile),
            SummaryProfile::General,
        )
        .unwrap();
        parse_document(
            &mut conn,
            SourceParserSet::new().select(SourceType::OcrText),
            &run.run_id,
        )
        .unwrap();
        let review = get_review(&conn, &run.run_id).unwrap().unwrap();
        let request = SaveCorrection {
            run_id: review.run_id,
            expected_state_version: review.expected_state_version,
            source_parsed_hash: review.source_parsed_hash,
            pages: vec![PageCorrection {
                page_number: 1,
                text:
                    "The contracting party is Cedar Services. Service began on 12 September 2026."
                        .to_string(),
            }],
        };
        (conn, request)
    }

    #[test]
    fn corrected_document_reparse_uses_operator_text() {
        let dir = tempfile::tempdir().unwrap();
        let (mut conn, request) = setup(&dir.path().join("test.db"));
        let original = db::get_parsed_document(&conn, &request.run_id)
            .unwrap()
            .unwrap();
        let corrected = save(&mut conn, &request).unwrap();
        let (normalizing, _) =
            db::start_normalizing(&mut conn, &corrected.run_id, corrected.state_version).unwrap();
        let failed = db::fail_normalization(
            &mut conn,
            &corrected.run_id,
            normalizing.state_version,
            PipelineFailure {
                code: "INJECTED".into(),
                message: "Test interruption".into(),
                stage: Some(PipelineStage::Normalize),
                recoverable: true,
            },
        )
        .unwrap();
        // Exercise the real retry admission and parser, not just an edited fixture.
        let retry = prepare_received_run(corrected.document_id.clone(), Utc::now());
        let (retry, document, _) =
            db::create_retry_run(&mut conn, &failed.run_id, failed.state_version, &retry).unwrap();
        let parsed = parse_started_document(
            &mut conn,
            SourceParserSet::new().select(SourceType::OcrText),
            &retry.run_id,
            retry.state_version,
            &document,
        )
        .unwrap();
        let normalized = CanonicalNormalizer::new().normalize(&parsed).unwrap();
        assert_eq!(
            parsed.pages[0].text, request.pages[0].text,
            "reparse must preserve operator correction"
        );
        assert!(normalized.pages[0].content[0]
            .text
            .contains("Cedar Services"));
        assert_eq!(
            db::get_parsed_document(&conn, &request.run_id)
                .unwrap()
                .unwrap(),
            original
        );
    }

    #[test]
    fn save_preserves_source_and_replays_without_duplicate_revision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let (mut conn, request) = setup(&path);
        let original = db::get_parsed_document(&conn, &request.run_id).unwrap();
        let run = save(&mut conn, &request).unwrap();
        assert_eq!(run.state, PipelineState::Parsed);
        assert_eq!(save(&mut conn, &request).unwrap().run_id, run.run_id);
        let mut competing = request.clone();
        competing.pages[0].text = "Different operator correction".to_string();
        assert!(matches!(
            save(&mut conn, &competing),
            Err(CorrectionError::AlreadyCorrected)
        ));
        assert_eq!(
            db::get_parsed_document(&conn, &request.run_id).unwrap(),
            original
        );
        assert_eq!(
            conn.query_row::<u32, _, _>("SELECT COUNT(*) FROM ocr_text_corrections", [], |row| row
                .get(0))
                .unwrap(),
            1
        );
        drop(conn);
        let conn = db::init_db(path).unwrap();
        assert_eq!(
            load_for_document(&conn, &run.document_id)
                .unwrap()
                .unwrap()
                .pages[0]
                .text,
            request.pages[0].text
        );
        assert!(conn
            .execute("UPDATE ocr_text_corrections SET source_version=1", [])
            .is_err());
    }

    #[test]
    fn stale_and_invalid_edits_leave_no_partial_state() {
        let dir = tempfile::tempdir().unwrap();
        let (mut conn, request) = setup(&dir.path().join("test.db"));
        let baseline: u32 = conn
            .query_row("SELECT COUNT(*) FROM pipeline_runs", [], |row| row.get(0))
            .unwrap();
        for pages in [
            vec![],
            vec![PageCorrection {
                page_number: 0,
                text: "bad page".into(),
            }],
            vec![PageCorrection {
                page_number: 1,
                text: " ".into(),
            }],
            vec![request.pages[0].clone(), request.pages[0].clone()],
            vec![PageCorrection {
                page_number: 1,
                text: "x".repeat(MAX_TEXT_BYTES + 1),
            }],
        ] {
            let invalid = SaveCorrection {
                pages,
                ..request.clone()
            };
            assert!(matches!(
                save(&mut conn, &invalid),
                Err(CorrectionError::Invalid(_))
            ));
        }
        let stale = SaveCorrection {
            expected_state_version: 0,
            ..request.clone()
        };
        assert!(matches!(
            save(&mut conn, &stale),
            Err(CorrectionError::StaleSource)
        ));
        let stale = SaveCorrection {
            source_parsed_hash: String::new(),
            ..request.clone()
        };
        assert!(matches!(
            save(&mut conn, &stale),
            Err(CorrectionError::StaleSource)
        ));
        // Simulate a late database failure, after the new run would be inserted.
        conn.execute_batch("CREATE TRIGGER reject_correction BEFORE INSERT ON ocr_text_corrections BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
        assert!(matches!(
            save(&mut conn, &request),
            Err(CorrectionError::Sqlite(_))
        ));
        assert_eq!(
            conn.query_row::<u32, _, _>("SELECT COUNT(*) FROM pipeline_runs", [], |row| row.get(0))
                .unwrap(),
            baseline
        );
        assert_eq!(
            conn.query_row::<u32, _, _>("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
                .unwrap(),
            baseline
        );
    }

    #[test]
    fn correction_boundary_preserves_pages_and_counts_utf8_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, request) = setup(&dir.path().join("test.db"));
        let parsed = db::get_parsed_document(&conn, &request.run_id)
            .unwrap()
            .unwrap();
        let remaining = parsed
            .pages
            .iter()
            .skip(1)
            .map(|page| page.text.len())
            .sum::<usize>();
        for bytes in [MAX_TEXT_BYTES - 1, MAX_TEXT_BYTES, MAX_TEXT_BYTES + 1] {
            let mut candidate = parsed.clone();
            let result = apply_pages(
                &mut candidate,
                &[PageCorrection {
                    page_number: 1,
                    text: "x".repeat(bytes - remaining),
                }],
            );
            assert_eq!(result.is_ok(), bytes <= MAX_TEXT_BYTES);
            assert_eq!(&candidate.pages[1..], &parsed.pages[1..]);
        }
        let mut candidate = parsed.clone();
        assert!(apply_pages(
            &mut candidate,
            &[PageCorrection {
                page_number: 1,
                text: "\u{00e9}".repeat(MAX_TEXT_BYTES / 2 + 1),
            }]
        )
        .is_err());
        let mut candidate = parsed.clone();
        assert!(apply_pages(
            &mut candidate,
            &[PageCorrection {
                page_number: 1,
                text: parsed.pages[0].text.clone(),
            }]
        )
        .is_err());
        conn.execute("UPDATE documents SET source_type='native_text'", [])
            .unwrap();
        assert!(get_review(&conn, &request.run_id).unwrap().is_none());
    }

    #[test]
    fn concurrent_editors_admit_only_one_successor() {
        use std::sync::{Arc, Barrier};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let (conn, request) = setup(&path);
        drop(conn);
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|index| {
                let path = path.clone();
                let mut request = request.clone();
                request.pages[0].text.push_str(&format!(" Editor {index}."));
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut conn = db::init_db(path).unwrap();
                    barrier.wait();
                    save(&mut conn, &request)
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| matches!(r, Err(CorrectionError::AlreadyCorrected)))
                .count(),
            1
        );
        let conn = db::init_db(path).unwrap();
        assert_eq!(
            conn.query_row::<u32, _, _>("SELECT COUNT(*) FROM ocr_text_corrections", [], |row| row
                .get(0))
                .unwrap(),
            1
        );
    }

    struct FixtureRuntime(std::sync::Mutex<Vec<String>>, ModelProfileSnapshot);

    impl crate::pipeline::contracts::ModelRuntime for FixtureRuntime {
        fn generate(
            &self,
            request: &crate::pipeline::contracts::ModelRequest,
        ) -> Result<
            crate::pipeline::contracts::ModelResponse,
            crate::pipeline::contracts::ModelRuntimeFailure,
        > {
            self.0.lock().unwrap().push(request.user_prompt.clone());
            Ok(crate::pipeline::contracts::ModelResponse {
                text: crate::pipeline::summary::fixture_model_output(request),
                runtime_id: self.runtime_id().into(),
                model_id: self.model_id().into(),
                request_attempts: vec![],
            })
        }
        fn health(&self) -> Result<(), crate::pipeline::contracts::ModelRuntimeFailure> {
            Ok(())
        }
        fn runtime_id(&self) -> &str {
            "fixture-runtime"
        }
        fn model_id(&self) -> &str {
            "fixture-model"
        }
        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            Some(self.1.clone())
        }
    }

    #[test]
    fn corrected_source_reaches_completed_summary_and_citations() {
        use crate::pipeline::chunk::DeterministicDocumentChunker;
        use crate::pipeline::service::{continue_run_to_summary, ContinuationComponents};
        use crate::pipeline::structure::DeterministicStructureInterpreter;
        use crate::pipeline::workspace;
        let dir = tempfile::tempdir().unwrap();
        let (mut conn, mut request) = setup(&dir.path().join("test.db"));
        let original = db::get_parsed_document(&conn, &request.run_id).unwrap();
        let profile = db::get_run_model_profile(&conn, &request.run_id)
            .unwrap()
            .unwrap();
        let runtime = FixtureRuntime(std::sync::Mutex::new(Vec::new()), profile);
        let parsers = SourceParserSet::new();
        let components = ContinuationComponents {
            parser: parsers.select(SourceType::OcrText),
            normalizer: &CanonicalNormalizer::new(),
            interpreter: &DeterministicStructureInterpreter::new(),
            chunker: &DeterministicDocumentChunker::new(),
            runtime: Some(&runtime),
        };
        continue_run_to_summary(
            &mut conn,
            &request.run_id,
            request.expected_state_version,
            components,
        )
        .unwrap();
        let previous_summary =
            serde_json::to_value(workspace::get_persisted_summary(&conn, &request.run_id).unwrap())
                .unwrap();
        let review = get_review(&conn, &request.run_id).unwrap().unwrap();
        request.expected_state_version = review.expected_state_version;
        let corrected = save(&mut conn, &request).unwrap();
        continue_run_to_summary(
            &mut conn,
            &corrected.run_id,
            corrected.state_version,
            components,
        )
        .unwrap();
        let view = workspace::get_persisted_summary(&conn, &corrected.run_id).unwrap();
        assert!(view
            .summary
            .warnings
            .iter()
            .any(|w| w.code == CORRECTION_WARNING));
        assert!(view
            .summary
            .claims
            .iter()
            .chain(&view.summary.summary_claims)
            .flat_map(|claim| &claim.citations)
            .any(|citation| citation.exact_quote.contains("Cedar Services")));
        assert!(runtime
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|prompt| prompt.contains("Cedar Services")));
        assert_eq!(
            db::get_parsed_document(&conn, &request.run_id).unwrap(),
            original
        );
        assert_eq!(
            serde_json::to_value(workspace::get_persisted_summary(&conn, &request.run_id).unwrap())
                .unwrap(),
            previous_summary
        );
        let profile = db::get_run_model_profile(&conn, &request.run_id).unwrap();
        assert_eq!(
            db::get_run_model_profile(&conn, &corrected.run_id).unwrap(),
            profile
        );
    }

    #[test]
    fn source_opening_checks_the_retained_pdf_identity() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, request) = setup(&dir.path().join("test.db"));
        let source_path = verified_source_path(&conn, &request.run_id).unwrap();
        let source_bytes = std::fs::read(&source_path).unwrap();
        let retained = dir.path().join("retained.pdf");
        std::fs::write(&retained, &source_bytes).unwrap();
        conn.execute(
            "UPDATE documents SET local_source_path=?1",
            [retained.to_str().unwrap()],
        )
        .unwrap();
        assert_eq!(
            verified_source_path(&conn, &request.run_id).unwrap(),
            retained
        );
        std::fs::write(&retained, b"%PDF-changed").unwrap();
        assert!(matches!(
            verified_source_path(&conn, &request.run_id),
            Err(CorrectionError::SourceUnavailable)
        ));
    }
}
