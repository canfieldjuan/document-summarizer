use crate::pipeline::contracts::{
    DocumentNormalizer, NormalizedBlock, NormalizedBlockKind, NormalizedDocument, NormalizedPage,
    ParsedDocument, PipelineFailure, PipelineStage, PipelineWarning, SourceSpan, SourceType,
};
use crate::pipeline::db::{self, StoreError};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use thiserror::Error;

pub const NORMALIZATION_VERSION: &str = "1.0.0";

#[derive(Default)]
pub struct CanonicalNormalizer;

impl CanonicalNormalizer {
    pub fn new() -> Self {
        Self
    }
}

impl DocumentNormalizer for CanonicalNormalizer {
    fn normalize(&self, parsed: &ParsedDocument) -> Result<NormalizedDocument, PipelineFailure> {
        validate_parsed_document(parsed)?;

        let mut pages = Vec::with_capacity(parsed.pages.len());
        for page in &parsed.pages {
            let (text, cleanup_applied) = clean_text(&page.text);
            let mut warnings = page.warnings.clone();
            if cleanup_applied {
                warnings.push(PipelineWarning {
                    code: "REPRESENTATION_CLEANUP_APPLIED".to_string(),
                    message: "Line endings or non-semantic control artifacts were normalized"
                        .to_string(),
                    stage: Some(PipelineStage::Normalize),
                });
            }

            let content = if text.trim().is_empty() {
                if !page.requires_visual_processing {
                    return Err(normalization_failure(
                        "INVALID_PARSED_DOCUMENT",
                        format!(
                            "Page {} has no content after safe cleanup but lacks visual routing",
                            page.page_number
                        ),
                        false,
                    ));
                }
                Vec::new()
            } else {
                vec![NormalizedBlock {
                    block_id: deterministic_block_id(
                        &parsed.document_id,
                        self.version(),
                        page.page_number,
                        0,
                    ),
                    kind: NormalizedBlockKind::Text,
                    text,
                    source: SourceSpan {
                        page_start: page.page_number,
                        page_end: page.page_number,
                        section_id: None,
                        source_type: SourceType::NativeText,
                    },
                }]
            };

            pages.push(NormalizedPage {
                page_number: page.page_number,
                content,
                warnings,
                requires_visual_processing: page.requires_visual_processing,
            });
        }

        let normalized = NormalizedDocument {
            document_id: parsed.document_id.clone(),
            normalization_version: self.version().to_string(),
            pages,
            warnings: parsed.warnings.clone(),
        };
        validate_normalized_document(&normalized, parsed, self.version())?;
        Ok(normalized)
    }

    fn version(&self) -> &'static str {
        NORMALIZATION_VERSION
    }
}

fn clean_text(input: &str) -> (String, bool) {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut changed = false;

    while let Some(character) = chars.next() {
        match character {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                output.push('\n');
                changed = true;
            }
            '\0' => {
                changed = true;
            }
            _ => output.push(character),
        }
    }

    (output, changed)
}

fn deterministic_block_id(
    document_id: &str,
    normalization_version: &str,
    page_number: u32,
    block_index: u32,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"normalized-block\0");
    hasher.update(document_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(normalization_version.as_bytes());
    hasher.update(b"\0");
    hasher.update(page_number.to_be_bytes());
    hasher.update(block_index.to_be_bytes());
    format!("nb-{:x}", hasher.finalize())
}

fn validate_parsed_document(parsed: &ParsedDocument) -> Result<(), PipelineFailure> {
    if parsed.document_id.trim().is_empty()
        || parsed.parser_id.trim().is_empty()
        || parsed.parser_version.trim().is_empty()
    {
        return Err(normalization_failure(
            "INVALID_PARSED_DOCUMENT",
            "Parsed document identity and parser identity must be present",
            false,
        ));
    }

    for (index, page) in parsed.pages.iter().enumerate() {
        let expected_page_number = u32::try_from(index + 1).map_err(|_| {
            normalization_failure(
                "INVALID_PARSED_DOCUMENT",
                "Parsed page count exceeds the supported page-number range",
                false,
            )
        })?;
        if page.page_number != expected_page_number {
            return Err(normalization_failure(
                "INVALID_PARSED_DOCUMENT",
                "Parsed pages are not in canonical one-based order",
                false,
            ));
        }
        if page.text.trim().is_empty()
            && (!page.requires_visual_processing
                || !page
                    .warnings
                    .iter()
                    .any(|warning| warning.code == "NO_NATIVE_TEXT"))
        {
            return Err(normalization_failure(
                "INVALID_PARSED_DOCUMENT",
                format!(
                    "Empty parsed page {} lacks its visual-routing marker or warning",
                    page.page_number
                ),
                false,
            ));
        }
    }

    Ok(())
}

fn validate_normalized_document(
    normalized: &NormalizedDocument,
    parsed: &ParsedDocument,
    expected_version: &str,
) -> Result<(), PipelineFailure> {
    if normalized.document_id != parsed.document_id {
        return Err(invalid_normalized(
            "Normalized document identity does not match parsed input",
        ));
    }
    if normalized.normalization_version.trim().is_empty()
        || normalized.normalization_version != expected_version
    {
        return Err(invalid_normalized(
            "Normalization version is missing or does not match the selected normalizer",
        ));
    }
    if normalized.pages.len() != parsed.pages.len() {
        return Err(invalid_normalized(
            "Normalization changed the number of source pages",
        ));
    }
    if !normalized.warnings.starts_with(&parsed.warnings) {
        return Err(invalid_normalized(
            "Normalization did not preserve document warnings",
        ));
    }

    let source_pages = parsed
        .pages
        .iter()
        .map(|page| page.page_number)
        .collect::<HashSet<_>>();
    let mut block_ids = HashSet::new();

    for (normalized_page, parsed_page) in normalized.pages.iter().zip(&parsed.pages) {
        if normalized_page.page_number != parsed_page.page_number {
            return Err(invalid_normalized(
                "Normalization changed source page ordering or numbering",
            ));
        }
        if normalized_page.requires_visual_processing != parsed_page.requires_visual_processing {
            return Err(invalid_normalized(
                "Normalization changed a page visual-routing marker",
            ));
        }
        if !normalized_page.warnings.starts_with(&parsed_page.warnings) {
            return Err(invalid_normalized(
                "Normalization did not preserve page warnings",
            ));
        }

        let (expected_text, cleanup_applied) = clean_text(&parsed_page.text);
        let cleanup_warning_present = normalized_page
            .warnings
            .iter()
            .any(|warning| warning.code == "REPRESENTATION_CLEANUP_APPLIED");
        if cleanup_applied && !cleanup_warning_present {
            return Err(invalid_normalized(
                "Representation cleanup was not recorded as a page warning",
            ));
        }

        if expected_text.trim().is_empty() {
            if !normalized_page.content.is_empty() {
                return Err(invalid_normalized(
                    "An empty source page gained fabricated normalized content",
                ));
            }
            continue;
        }
        if normalized_page.content.len() != 1 {
            return Err(invalid_normalized(
                "Normalization must preserve each native-text page as one conservative block",
            ));
        }

        for (block_index, block) in normalized_page.content.iter().enumerate() {
            let block_index = u32::try_from(block_index).map_err(|_| {
                invalid_normalized("Normalized block count exceeds the supported identity range")
            })?;
            if block.block_id
                != deterministic_block_id(
                    &normalized.document_id,
                    &normalized.normalization_version,
                    normalized_page.page_number,
                    block_index,
                )
                || !block_ids.insert(block.block_id.clone())
            {
                return Err(invalid_normalized(
                    "Normalized block identity is invalid or duplicated",
                ));
            }
            if block.text != expected_text {
                return Err(invalid_normalized(
                    "Normalized text differs from the permitted representation cleanup",
                ));
            }
            if block.source.page_start != normalized_page.page_number
                || block.source.page_end != normalized_page.page_number
                || block.source.page_start != block.source.page_end
                || !source_pages.contains(&block.source.page_start)
                || block.source.section_id.is_some()
                || block.source.source_type != SourceType::NativeText
            {
                return Err(invalid_normalized(
                    "Normalized block source provenance is invalid or crosses a page boundary",
                ));
            }
        }
    }

    Ok(())
}

fn invalid_normalized(message: impl Into<String>) -> PipelineFailure {
    normalization_failure("INVALID_NORMALIZED_DOCUMENT", message, false)
}

fn normalization_failure(
    code: impl Into<String>,
    message: impl Into<String>,
    recoverable: bool,
) -> PipelineFailure {
    PipelineFailure {
        code: code.into(),
        message: message.into(),
        stage: Some(PipelineStage::Normalize),
        recoverable,
    }
}

#[derive(Debug, Error)]
pub enum NormalizePipelineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("Normalization failed: {0}")]
    NormalizerFailed(PipelineFailure),
    #[error("Normalized artifact persistence failed: {0}")]
    ArtifactPersistence(StoreError),
    #[error("{primary}; the failure state could not be persisted: {persistence}")]
    FailurePersistence {
        primary: String,
        #[source]
        persistence: StoreError,
    },
}

impl NormalizePipelineError {
    pub fn code(&self) -> &str {
        match self {
            Self::Store(_) => "PIPELINE_STORE_ERROR",
            Self::NormalizerFailed(failure) => &failure.code,
            Self::ArtifactPersistence(_) => "NORMALIZED_ARTIFACT_PERSISTENCE_FAILED",
            Self::FailurePersistence { .. } => "NORMALIZATION_FAILURE_PERSISTENCE_FAILED",
        }
    }
}

pub fn normalize_document(
    conn: &mut Connection,
    normalizer: &dyn DocumentNormalizer,
    run_id: &str,
) -> Result<NormalizedDocument, NormalizePipelineError> {
    let run = db::get_pipeline_run(conn, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    let (normalizing_run, parsed) = db::start_normalizing(conn, run_id, run.state_version)?;

    let normalized = match normalizer.normalize(&parsed) {
        Ok(normalized) => normalized,
        Err(failure) => {
            return Err(persist_normalization_failure(
                conn,
                run_id,
                normalizing_run.state_version,
                failure,
            ));
        }
    };
    if let Err(failure) = validate_normalized_document(&normalized, &parsed, normalizer.version()) {
        return Err(persist_normalization_failure(
            conn,
            run_id,
            normalizing_run.state_version,
            failure,
        ));
    }

    let warnings = normalized
        .warnings
        .iter()
        .chain(
            normalized
                .pages
                .iter()
                .flat_map(|page| page.warnings.iter()),
        )
        .cloned()
        .collect::<Vec<_>>();
    if let Err(persistence) = db::complete_normalization(
        conn,
        run_id,
        normalizing_run.state_version,
        &normalized,
        warnings,
    ) {
        let failure = normalization_failure(
            "NORMALIZED_ARTIFACT_PERSISTENCE_FAILED",
            "Normalized output could not be committed atomically",
            true,
        );
        return match db::fail_normalization(conn, run_id, normalizing_run.state_version, failure) {
            Ok(_) => Err(NormalizePipelineError::ArtifactPersistence(persistence)),
            Err(failure_persistence) => Err(NormalizePipelineError::FailurePersistence {
                primary: persistence.to_string(),
                persistence: failure_persistence,
            }),
        };
    }

    Ok(normalized)
}

fn persist_normalization_failure(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    failure: PipelineFailure,
) -> NormalizePipelineError {
    match db::fail_normalization(conn, run_id, expected_version, failure.clone()) {
        Ok(_) => NormalizePipelineError::NormalizerFailed(failure),
        Err(persistence) => NormalizePipelineError::FailurePersistence {
            primary: failure.to_string(),
            persistence,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contracts::{DocumentParser, ParsedPage, PipelineState, SourceSpan};
    use crate::pipeline::ingest::ingest_pdf;
    use crate::pipeline::parser::parse_document as parse_pipeline_document;
    use crate::pipeline::state::TransitionError;
    use rusqlite::params;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestPath(PathBuf);

    impl TestPath {
        fn new(extension: &str) -> Self {
            Self(
                std::env::temp_dir()
                    .join(format!("doc-sum-normalize-{}.{extension}", Uuid::new_v4())),
            )
        }

        fn write(extension: &str, bytes: &[u8]) -> Self {
            let path = Self::new(extension);
            fs::write(&path.0, bytes).expect("fixture should be writable");
            path
        }
    }

    impl Drop for TestPath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[derive(Clone)]
    struct FixtureParser {
        pages: Vec<ParsedPage>,
        warnings: Vec<PipelineWarning>,
    }

    impl DocumentParser for FixtureParser {
        fn parse(
            &self,
            document: &crate::pipeline::contracts::IngestedDocument,
        ) -> Result<ParsedDocument, PipelineFailure> {
            Ok(ParsedDocument {
                document_id: document.document_id.clone(),
                parser_id: self.id().to_string(),
                parser_version: self.version().to_string(),
                pages: self.pages.clone(),
                warnings: self.warnings.clone(),
            })
        }

        fn id(&self) -> &'static str {
            "fixture-parser"
        }

        fn version(&self) -> &'static str {
            "test"
        }
    }

    fn text_page(page_number: u32, text: &str) -> ParsedPage {
        ParsedPage {
            page_number,
            text: text.to_string(),
            warnings: Vec::new(),
            requires_visual_processing: false,
        }
    }

    fn empty_page(page_number: u32) -> ParsedPage {
        ParsedPage {
            page_number,
            text: String::new(),
            warnings: vec![PipelineWarning {
                code: "NO_NATIVE_TEXT".to_string(),
                message: "NO_NATIVE_TEXT".to_string(),
                stage: Some(PipelineStage::Parse),
            }],
            requires_visual_processing: true,
        }
    }

    fn parsed_document(pages: Vec<ParsedPage>) -> ParsedDocument {
        ParsedDocument {
            document_id: "document-for-normalization".to_string(),
            parser_id: "fixture-parser".to_string(),
            parser_version: "test".to_string(),
            pages,
            warnings: Vec::new(),
        }
    }

    fn create_parsed_run(
        conn: &mut Connection,
        pages: Vec<ParsedPage>,
    ) -> (String, ParsedDocument) {
        let source = TestPath::write("pdf", b"%PDF-1.4\nnormalization fixture");
        let (_, run) = ingest_pdf(conn, source.0.to_str().expect("UTF-8 fixture path"))
            .expect("candidate should ingest");
        let parser = FixtureParser {
            pages,
            warnings: Vec::new(),
        };
        let parsed = parse_pipeline_document(conn, &parser, &run.run_id)
            .expect("fixture parser output should persist");
        (run.run_id, parsed)
    }

    #[test]
    fn multi_page_normalization_preserves_facts_provenance_empty_page_and_lifecycle() {
        let fidelity_text = "Revenue was $1,247,392.17.\r\nMargin changed by -4.75%.\r\nAs of January 31, 2027.\r\nJosé Álvarez — contact: jose.alvarez+audit@example.com.\r\nThe customer shall NOT be liable.\r\nInvoice INV-2027-001847 remains at -0.25.";
        let pages = vec![
            text_page(1, fidelity_text),
            empty_page(2),
            text_page(3, "Unicode remains exact: 東京, résumé, naïve, and ✓."),
        ];
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (run_id, parsed) = create_parsed_run(&mut conn, pages);

        let normalized = normalize_document(&mut conn, &CanonicalNormalizer::new(), &run_id)
            .expect("parsed document should normalize");

        assert_eq!(normalized.document_id, parsed.document_id);
        assert_eq!(normalized.normalization_version, NORMALIZATION_VERSION);
        assert_eq!(normalized.pages.len(), parsed.pages.len());
        assert_eq!(
            normalized
                .pages
                .iter()
                .map(|page| page.page_number)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        let normalized_fidelity = &normalized.pages[0].content[0].text;
        assert_eq!(normalized_fidelity, &fidelity_text.replace("\r\n", "\n"));
        for exact_value in [
            "$1,247,392.17",
            "-4.75%",
            "January 31, 2027",
            "José Álvarez",
            "jose.alvarez+audit@example.com",
            "shall NOT be liable",
            "INV-2027-001847",
            "-0.25",
        ] {
            assert!(normalized_fidelity.contains(exact_value));
        }
        assert_eq!(
            normalized.pages[0].content[0].source,
            SourceSpan {
                page_start: 1,
                page_end: 1,
                section_id: None,
                source_type: SourceType::NativeText,
            }
        );
        assert!(normalized.pages[1].content.is_empty());
        assert!(normalized.pages[1].requires_visual_processing);
        assert!(normalized.pages[1]
            .warnings
            .iter()
            .any(|warning| warning.code == "NO_NATIVE_TEXT"));
        assert_eq!(
            normalized.pages[2].content[0].text,
            "Unicode remains exact: 東京, résumé, naïve, and ✓."
        );

        let persisted_run = db::get_pipeline_run(&conn, &run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(persisted_run.state, PipelineState::Normalized);
        assert_eq!(persisted_run.state_version, 7);
        assert_eq!(persisted_run.current_stage, Some(PipelineStage::Normalize));
        let events = db::list_pipeline_events(&conn, &run_id).expect("events should load");
        assert_eq!(
            events
                .iter()
                .map(|event| event.next_state.clone())
                .collect::<Vec<_>>(),
            vec![
                PipelineState::Received,
                PipelineState::Ingesting,
                PipelineState::Ingested,
                PipelineState::Parsing,
                PipelineState::Parsed,
                PipelineState::Normalizing,
                PipelineState::Normalized,
            ]
        );
        assert_eq!(
            db::get_normalized_document(&conn, &run_id)
                .expect("artifact should load")
                .expect("artifact should exist"),
            normalized
        );
    }

    #[test]
    fn normalization_is_deterministic_and_cleaning_does_not_collapse_layout_whitespace() {
        let parsed = parsed_document(vec![text_page(1, "A\r\nB\rC\0D\t  E\n\nF")]);
        let normalizer = CanonicalNormalizer::new();

        let first = normalizer
            .normalize(&parsed)
            .expect("input should normalize");
        let second = normalizer
            .normalize(&parsed)
            .expect("same input should normalize again");

        assert_eq!(first, second);
        assert_eq!(first.pages[0].content[0].text, "A\nB\nCD\t  E\n\nF");
        assert!(first.pages[0]
            .warnings
            .iter()
            .any(|warning| warning.code == "REPRESENTATION_CLEANUP_APPLIED"));
        assert!(first.pages[0].content[0].block_id.starts_with("nb-"));
    }

    #[test]
    fn normalization_validation_rejects_both_malformed_input_and_invalid_output_boundaries() {
        let normalizer = CanonicalNormalizer::new();
        let parsed = parsed_document(vec![text_page(1, "Exact source text")]);
        let valid = normalizer
            .normalize(&parsed)
            .expect("control artifact should be valid");
        validate_normalized_document(&valid, &parsed, NORMALIZATION_VERSION)
            .expect("known-good normalized artifact should validate");

        let mut malformed_input = parsed.clone();
        malformed_input.pages[0].page_number = 2;
        let input_error = normalizer
            .normalize(&malformed_input)
            .expect_err("out-of-order parsed page must fail");
        assert_eq!(input_error.code, "INVALID_PARSED_DOCUMENT");

        let mut wrong_text = valid.clone();
        wrong_text.pages[0].content[0].text = "Paraphrased text".to_string();
        assert!(validate_normalized_document(&wrong_text, &parsed, NORMALIZATION_VERSION).is_err());

        let mut crossing_span = valid.clone();
        crossing_span.pages[0].content[0].source.page_end = 2;
        assert!(
            validate_normalized_document(&crossing_span, &parsed, NORMALIZATION_VERSION).is_err()
        );

        let mixed_parsed = parsed_document(vec![
            text_page(1, "valid first page"),
            text_page(2, "second page with wrong provenance"),
        ]);
        let mut mixed_output = normalizer
            .normalize(&mixed_parsed)
            .expect("mixed-page control artifact should normalize");
        mixed_output.pages[1].content[0].source.page_start = 1;
        mixed_output.pages[1].content[0].source.page_end = 1;
        assert!(
            validate_normalized_document(&mixed_output, &mixed_parsed, NORMALIZATION_VERSION)
                .is_err()
        );

        let mut lost_marker_parsed = parsed_document(vec![empty_page(1)]);
        let mut lost_marker = normalizer
            .normalize(&lost_marker_parsed)
            .expect("empty page should normalize");
        lost_marker.pages[0].requires_visual_processing = false;
        assert!(validate_normalized_document(
            &lost_marker,
            &lost_marker_parsed,
            NORMALIZATION_VERSION
        )
        .is_err());
        lost_marker_parsed.pages[0].warnings.clear();
        assert!(normalizer.normalize(&lost_marker_parsed).is_err());

        let mut missing_version = valid;
        missing_version.normalization_version.clear();
        assert!(
            validate_normalized_document(&missing_version, &parsed, NORMALIZATION_VERSION).is_err()
        );
    }

    #[test]
    fn inconsistent_persisted_parsed_document_fails_without_normalized_artifact() {
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (run_id, mut parsed) =
            create_parsed_run(&mut conn, vec![text_page(1, "valid parser output")]);
        parsed.pages[0].page_number = 2;
        conn.execute(
            "UPDATE parsed_documents SET parsed_artifact = ?1 WHERE run_id = ?2",
            params![
                serde_json::to_string(&parsed).expect("fixture should serialize"),
                run_id
            ],
        )
        .expect("test should inject inconsistent parser output");

        let error = normalize_document(&mut conn, &CanonicalNormalizer::new(), &run_id)
            .expect_err("inconsistent parser artifact must fail");

        assert_eq!(error.code(), "INVALID_PARSED_DOCUMENT");
        let failed = db::get_pipeline_run(&conn, &run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert_eq!(
            failed.failure.expect("failure should persist").code,
            "INVALID_PARSED_DOCUMENT"
        );
        assert!(db::get_normalized_document(&conn, &run_id)
            .expect("artifact query should work")
            .is_none());
        assert!(!db::list_pipeline_events(&conn, &run_id)
            .expect("events should load")
            .iter()
            .any(|event| event.next_state == PipelineState::Normalized));
    }

    #[test]
    fn normalized_event_failure_rolls_back_artifact_state_and_version_atomically() {
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (run_id, _) = create_parsed_run(&mut conn, vec![text_page(1, "atomic normalization")]);
        conn.execute_batch(
            "CREATE TRIGGER test_fail_normalized_event
             BEFORE INSERT ON pipeline_events
             WHEN NEW.next_state = '\"Normalized\"'
             BEGIN
                 SELECT RAISE(ABORT, 'injected normalized event failure');
             END;",
        )
        .expect("failure trigger should install");

        let error = normalize_document(&mut conn, &CanonicalNormalizer::new(), &run_id)
            .expect_err("normalized event failure should fail the run");

        assert_eq!(error.code(), "NORMALIZED_ARTIFACT_PERSISTENCE_FAILED");
        assert!(db::get_normalized_document(&conn, &run_id)
            .expect("artifact query should work")
            .is_none());
        let failed = db::get_pipeline_run(&conn, &run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert_eq!(failed.state_version, 7);
        assert_eq!(
            failed.failure.expect("failure should persist").code,
            "NORMALIZED_ARTIFACT_PERSISTENCE_FAILED"
        );
        let events = db::list_pipeline_events(&conn, &run_id).expect("events should load");
        assert!(!events
            .iter()
            .any(|event| event.next_state == PipelineState::Normalized));
        assert_eq!(
            events
                .last()
                .expect("failure event should exist")
                .next_state,
            PipelineState::Failed
        );
    }

    #[test]
    fn normalized_artifact_and_integrity_hash_survive_independent_reopen() {
        let database = TestPath::new("db");
        let run_id;
        let expected_artifact;
        let expected_events;
        let expected_hash;
        {
            let mut conn = db::init_db(&database.0).expect("schema should initialize");
            let created = create_parsed_run(
                &mut conn,
                vec![text_page(1, "Persist exactly: INV-2027-001847")],
            );
            run_id = created.0;
            expected_artifact = normalize_document(&mut conn, &CanonicalNormalizer::new(), &run_id)
                .expect("document should normalize");
            expected_events = db::list_pipeline_events(&conn, &run_id).expect("events should load");
            expected_hash = conn
                .query_row(
                    "SELECT artifact_hash FROM normalized_documents WHERE run_id = ?1",
                    [&run_id],
                    |row| row.get::<_, String>(0),
                )
                .expect("artifact hash should persist");
        }

        let reopened = db::init_db(&database.0).expect("database should reopen");
        assert_eq!(
            db::schema_version(&reopened).expect("version should load"),
            3
        );
        assert_eq!(
            db::get_normalized_document(&reopened, &run_id)
                .expect("artifact should load")
                .expect("artifact should exist"),
            expected_artifact
        );
        assert_eq!(
            db::list_pipeline_events(&reopened, &run_id).expect("events should load"),
            expected_events
        );
        assert_eq!(
            reopened
                .query_row(
                    "SELECT artifact_hash FROM normalized_documents WHERE run_id = ?1",
                    [&run_id],
                    |row| row.get::<_, String>(0),
                )
                .expect("artifact hash should reload"),
            expected_hash
        );
        assert_eq!(
            db::get_pipeline_run(&reopened, &run_id)
                .expect("run should load")
                .expect("run should exist")
                .state,
            PipelineState::Normalized
        );
    }

    #[test]
    fn normalized_artifact_hash_rejects_tampered_storage() {
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (run_id, _) = create_parsed_run(&mut conn, vec![text_page(1, "hash protected")]);
        normalize_document(&mut conn, &CanonicalNormalizer::new(), &run_id)
            .expect("document should normalize");
        conn.execute(
            "UPDATE normalized_documents
             SET normalized_artifact = normalized_artifact || ' '
             WHERE run_id = ?1",
            [&run_id],
        )
        .expect("test should tamper with stored artifact");

        assert!(matches!(
            db::get_normalized_document(&conn, &run_id),
            Err(StoreError::NormalizedArtifactIntegrityMismatch { .. })
        ));
    }

    #[test]
    fn normalized_artifact_retrieval_rejects_run_document_mismatch() {
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (run_id, _) = create_parsed_run(&mut conn, vec![text_page(1, "run association")]);
        let mut normalized = normalize_document(&mut conn, &CanonicalNormalizer::new(), &run_id)
            .expect("document should normalize");
        let other_source = TestPath::write("pdf", b"%PDF-1.4\nother document");
        let (other_document, _) = ingest_pdf(
            &mut conn,
            other_source.0.to_str().expect("UTF-8 fixture path"),
        )
        .expect("other document should ingest");
        normalized.document_id = other_document.document_id.clone();
        let artifact_json =
            serde_json::to_string(&normalized).expect("tampered artifact should serialize");
        let artifact_hash = format!("{:x}", Sha256::digest(artifact_json.as_bytes()));
        conn.execute(
            "UPDATE normalized_documents
             SET document_id = ?1, normalized_artifact = ?2, artifact_hash = ?3
             WHERE run_id = ?4",
            params![
                other_document.document_id,
                artifact_json,
                artifact_hash,
                run_id
            ],
        )
        .expect("test should inject a mismatched run association");

        assert!(matches!(
            db::get_normalized_document(&conn, &run_id),
            Err(StoreError::NormalizedArtifactMetadataMismatch { .. })
        ));
    }

    #[test]
    fn two_connections_reject_stale_normalizing_transition() {
        let database = TestPath::new("db");
        let mut caller_a = db::init_db(&database.0).expect("schema should initialize");
        let (run_id, _) = create_parsed_run(&mut caller_a, vec![text_page(1, "one winner")]);
        let mut caller_b = db::init_db(&database.0).expect("second connection should open");
        let observed_a = db::get_pipeline_run(&caller_a, &run_id)
            .expect("run should load")
            .expect("run should exist");
        let observed_b = db::get_pipeline_run(&caller_b, &run_id)
            .expect("run should load")
            .expect("run should exist");

        let (advanced, _) = db::start_normalizing(&mut caller_a, &run_id, observed_a.state_version)
            .expect("first caller should advance");
        let stale = db::start_normalizing(&mut caller_b, &run_id, observed_b.state_version);

        assert!(matches!(
            stale,
            Err(StoreError::Transition(
                TransitionError::StaleExpectedState { .. }
            ))
        ));
        assert_eq!(advanced.state, PipelineState::Normalizing);
        assert_eq!(
            db::get_pipeline_run(&caller_b, &run_id)
                .expect("run should load")
                .expect("run should exist")
                .state_version,
            advanced.state_version
        );
    }
}
