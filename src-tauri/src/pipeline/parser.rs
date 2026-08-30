use crate::pipeline::contracts::{
    DocumentParser, IngestedDocument, ParsedDocument, ParsedPage, PipelineFailure, PipelineStage,
    PipelineWarning,
};
use crate::pipeline::db::{self, StoreError};
use pdf_extract::{output_doc_page, Document as PdfDocument, Error as LopdfError, PlainTextOutput};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use thiserror::Error;

#[derive(Default)]
pub struct PdfExtractParser;

impl PdfExtractParser {
    pub fn new() -> Self {
        Self
    }
}

impl DocumentParser for PdfExtractParser {
    fn parse(&self, document: &IngestedDocument) -> Result<ParsedDocument, PipelineFailure> {
        catch_unwind(AssertUnwindSafe(|| self.parse_inner(document))).unwrap_or_else(|_| {
            Err(parse_failure(
                "PDF_PARSER_PANIC",
                "The native PDF parser aborted while reading malformed page data",
                false,
            ))
        })
    }

    fn id(&self) -> &'static str {
        "pdf-extract"
    }

    fn version(&self) -> &'static str {
        "0.12.0"
    }
}

impl PdfExtractParser {
    fn parse_inner(&self, document: &IngestedDocument) -> Result<ParsedDocument, PipelineFailure> {
        if document.file_type != "pdf" {
            return Err(parse_failure(
                "UNSUPPORTED_PARSER_INPUT",
                "The native PDF parser only accepts ingested PDF documents",
                false,
            ));
        }

        let source_bytes = fs::read(&document.local_source_path).map_err(|error| {
            parse_failure(
                "SOURCE_IO_ERROR",
                format!("The ingested source could not be reopened: {error}"),
                true,
            )
        })?;
        let source_size = u64::try_from(source_bytes.len()).map_err(|_| {
            parse_failure(
                "SOURCE_CONTENT_CHANGED",
                "The source no longer matches the ingested document identity",
                true,
            )
        })?;
        let source_hash = format!("{:x}", Sha256::digest(&source_bytes));
        if source_size != document.byte_size || source_hash != document.content_hash {
            return Err(parse_failure(
                "SOURCE_CONTENT_CHANGED",
                "The source no longer matches the ingested document identity",
                true,
            ));
        }

        let pdf = PdfDocument::load_mem(&source_bytes).map_err(map_load_error)?;
        if pdf.was_encrypted() || pdf.is_encrypted() {
            return Err(parse_failure(
                "ENCRYPTED_PDF_UNSUPPORTED",
                "Encrypted PDFs are not supported in the native-text parser",
                false,
            ));
        }

        let source_page_numbers = pdf.get_pages().keys().copied().collect::<Vec<_>>();
        let mut parsed_pages = Vec::new();
        let mut document_warnings = Vec::new();
        let mut empty_page_count = 0;

        for (index, source_page_number) in source_page_numbers.into_iter().enumerate() {
            let mut text = String::new();
            let mut output = PlainTextOutput::new(&mut text);
            output_doc_page(&pdf, &mut output, source_page_number).map_err(|error| {
                parse_failure(
                    "PDF_TEXT_EXTRACTION_FAILED",
                    format!(
                        "Native text extraction failed on page {}: {error}",
                        index + 1
                    ),
                    false,
                )
            })?;

            let page_number = u32::try_from(index + 1).map_err(|_| {
                parse_failure(
                    "PDF_PAGE_COUNT_OVERFLOW",
                    "The PDF has more pages than the parsed-document contract supports",
                    false,
                )
            })?;
            let mut page_warnings = Vec::new();
            let requires_visual_processing = text.trim().is_empty();
            if requires_visual_processing {
                empty_page_count += 1;
                page_warnings.push(PipelineWarning {
                    code: "NO_NATIVE_TEXT".to_string(),
                    message: "NO_NATIVE_TEXT".to_string(),
                    stage: Some(PipelineStage::Parse),
                });
            }

            parsed_pages.push(ParsedPage {
                page_number,
                text,
                warnings: page_warnings,
                requires_visual_processing,
            });
        }

        if empty_page_count > 0 && empty_page_count == parsed_pages.len() {
            document_warnings.push(PipelineWarning {
                code: "NO_NATIVE_TEXT_IN_DOCUMENT".to_string(),
                message: "NO_NATIVE_TEXT_IN_DOCUMENT".to_string(),
                stage: Some(PipelineStage::Parse),
            });
        }

        Ok(ParsedDocument {
            document_id: document.document_id.clone(),
            parser_id: self.id().to_string(),
            parser_version: self.version().to_string(),
            pages: parsed_pages,
            warnings: document_warnings,
        })
    }
}

fn map_load_error(error: LopdfError) -> PipelineFailure {
    match error {
        LopdfError::IO(io_error) => parse_failure(
            "SOURCE_IO_ERROR",
            format!("The ingested source could not be reopened: {io_error}"),
            true,
        ),
        LopdfError::InvalidPassword => parse_failure(
            "ENCRYPTED_PDF_UNSUPPORTED",
            "Encrypted PDFs are not supported in the native-text parser",
            false,
        ),
        other => parse_failure(
            "MALFORMED_PDF",
            format!("The PDF structure could not be parsed: {other}"),
            false,
        ),
    }
}

fn parse_failure(
    code: impl Into<String>,
    message: impl Into<String>,
    recoverable: bool,
) -> PipelineFailure {
    PipelineFailure {
        code: code.into(),
        message: message.into(),
        stage: Some(PipelineStage::Parse),
        recoverable,
    }
}

#[derive(Debug, Error)]
pub enum ParsePipelineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("Parsing failed: {0}")]
    ParserFailed(PipelineFailure),
    #[error("Parsed artifact persistence failed: {0}")]
    ArtifactPersistence(StoreError),
    #[error("{primary}; the failure state could not be persisted: {persistence}")]
    FailurePersistence {
        primary: String,
        #[source]
        persistence: StoreError,
    },
}

impl ParsePipelineError {
    pub fn code(&self) -> &str {
        match self {
            Self::Store(_) => "PIPELINE_STORE_ERROR",
            Self::ParserFailed(failure) => &failure.code,
            Self::ArtifactPersistence(_) => "PARSED_ARTIFACT_PERSISTENCE_FAILED",
            Self::FailurePersistence { .. } => "PARSE_FAILURE_PERSISTENCE_FAILED",
        }
    }
}

pub fn parse_document(
    conn: &mut Connection,
    parser: &dyn DocumentParser,
    run_id: &str,
) -> Result<ParsedDocument, ParsePipelineError> {
    let run = db::get_pipeline_run(conn, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    let (parsing_run, document) = db::start_parsing(conn, run_id, run.state_version)?;

    parse_started_document(conn, parser, run_id, parsing_run.state_version, &document)
}

pub(crate) fn parse_started_document(
    conn: &mut Connection,
    parser: &dyn DocumentParser,
    run_id: &str,
    parsing_state_version: u32,
    document: &IngestedDocument,
) -> Result<ParsedDocument, ParsePipelineError> {
    let parsed = match parser.parse(document) {
        Ok(parsed) => parsed,
        Err(failure) => {
            return Err(persist_parser_failure(
                conn,
                run_id,
                parsing_state_version,
                failure,
            ));
        }
    };
    if let Err(failure) = validate_parsed_document(&parsed, document, parser) {
        return Err(persist_parser_failure(
            conn,
            run_id,
            parsing_state_version,
            failure,
        ));
    }

    let warnings = parsed
        .warnings
        .iter()
        .chain(parsed.pages.iter().flat_map(|page| page.warnings.iter()))
        .cloned()
        .collect::<Vec<_>>();
    if let Err(persistence) =
        db::complete_parsing(conn, run_id, parsing_state_version, &parsed, warnings)
    {
        let failure = parse_failure(
            "PARSED_ARTIFACT_PERSISTENCE_FAILED",
            "Parsed output could not be committed atomically",
            true,
        );
        return match db::fail_parsing(conn, run_id, parsing_state_version, failure) {
            Ok(_) => Err(ParsePipelineError::ArtifactPersistence(persistence)),
            Err(failure_persistence) => Err(ParsePipelineError::FailurePersistence {
                primary: persistence.to_string(),
                persistence: failure_persistence,
            }),
        };
    }

    Ok(parsed)
}

fn persist_parser_failure(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    failure: PipelineFailure,
) -> ParsePipelineError {
    match db::fail_parsing(conn, run_id, expected_version, failure.clone()) {
        Ok(_) => ParsePipelineError::ParserFailed(failure),
        Err(persistence) => ParsePipelineError::FailurePersistence {
            primary: failure.to_string(),
            persistence,
        },
    }
}

fn validate_parsed_document(
    parsed: &ParsedDocument,
    document: &IngestedDocument,
    parser: &dyn DocumentParser,
) -> Result<(), PipelineFailure> {
    if parsed.document_id != document.document_id {
        return Err(parse_failure(
            "INVALID_PARSED_ARTIFACT",
            "The parser returned an artifact for a different document",
            false,
        ));
    }
    if parsed.parser_id != parser.id() || parsed.parser_version != parser.version() {
        return Err(parse_failure(
            "INVALID_PARSED_ARTIFACT",
            "The parser artifact identity does not match the selected parser",
            false,
        ));
    }

    for (index, page) in parsed.pages.iter().enumerate() {
        let expected_page_number = u32::try_from(index + 1).map_err(|_| {
            parse_failure(
                "INVALID_PARSED_ARTIFACT",
                "The parsed page count exceeds the contract's page-number range",
                false,
            )
        })?;
        if page.page_number != expected_page_number {
            return Err(parse_failure(
                "INVALID_PARSED_ARTIFACT",
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
            return Err(parse_failure(
                "INVALID_PARSED_ARTIFACT",
                "A page without native text must preserve its visual-routing marker and warning",
                false,
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contracts::PipelineState;
    use crate::pipeline::ingest::ingest_pdf;
    use pdf_extract::content::{Content, Operation};
    use pdf_extract::{
        dictionary, EncryptionState, EncryptionVersion, Object, Permissions, Stream, StringFormat,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    enum FixturePage {
        Text(Vec<u8>),
        Empty,
        ImageOnly,
    }

    struct TestPath(PathBuf);

    impl TestPath {
        fn new(extension: &str) -> Self {
            Self(
                std::env::temp_dir().join(format!("doc-sum-parser-{}.{extension}", Uuid::new_v4())),
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

    fn build_pdf(pages: Vec<FixturePage>) -> PdfDocument {
        let mut document = PdfDocument::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let mut page_ids = Vec::new();

        for page in pages {
            let mut resources = dictionary! {
                "Font" => dictionary! { "F1" => font_id },
            };
            let operations = match page {
                FixturePage::Text(bytes) => vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Td", vec![72.into(), 720.into()]),
                    Operation::new("Tj", vec![Object::String(bytes, StringFormat::Literal)]),
                    Operation::new("ET", vec![]),
                ],
                FixturePage::Empty => Vec::new(),
                FixturePage::ImageOnly => {
                    let image_id = document.add_object(Stream::new(
                        dictionary! {
                            "Type" => "XObject",
                            "Subtype" => "Image",
                            "Width" => 1,
                            "Height" => 1,
                            "ColorSpace" => "DeviceRGB",
                            "BitsPerComponent" => 8,
                        },
                        vec![0, 0, 0],
                    ));
                    resources.set("XObject", dictionary! { "Im1" => image_id });
                    vec![
                        Operation::new("q", vec![]),
                        Operation::new(
                            "cm",
                            vec![
                                72.into(),
                                0.into(),
                                0.into(),
                                72.into(),
                                72.into(),
                                72.into(),
                            ],
                        ),
                        Operation::new("Do", vec!["Im1".into()]),
                        Operation::new("Q", vec![]),
                    ]
                }
            };
            let content = Content { operations }
                .encode()
                .expect("content should encode");
            let content_id = document.add_object(Stream::new(dictionary! {}, content));
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            });
            page_ids.push(Object::Reference(page_id));
        }

        let page_count = page_ids.len() as i64;
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => page_ids,
                "Count" => page_count,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        document
    }

    fn save_pdf(mut document: PdfDocument) -> TestPath {
        let path = TestPath::new("pdf");
        document.save(&path.0).expect("PDF fixture should save");
        path
    }

    fn ingest_fixture(
        conn: &mut Connection,
        path: &Path,
    ) -> crate::pipeline::contracts::PipelineRun {
        let (_, run) = ingest_pdf(conn, path.to_str().expect("UTF-8 path"))
            .expect("PDF candidate should ingest");
        run
    }

    #[test]
    fn native_multi_page_pdf_preserves_page_order_text_and_unicode() {
        let pdf = save_pdf(build_pdf(vec![
            FixturePage::Text(b"PAGE_ONE".to_vec()),
            FixturePage::Text(b"caf\xe9 r\xe9sum\xe9".to_vec()),
            FixturePage::Text(b"PAGE_THREE".to_vec()),
        ]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);

        let parsed = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect("PDF should parse");

        assert_eq!(parsed.pages.len(), 3);
        assert_eq!(
            parsed
                .pages
                .iter()
                .map(|page| page.page_number)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(parsed.pages[0].text.contains("PAGE_ONE"));
        assert!(parsed.pages[1].text.contains("café résumé"));
        assert!(parsed.pages[2].text.contains("PAGE_THREE"));
        assert_eq!(parsed.parser_id, "pdf-extract");
        assert_eq!(parsed.parser_version, "0.12.0");

        let persisted_run = db::get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(persisted_run.state, PipelineState::Parsed);
        assert_eq!(persisted_run.state_version, 5);
        let events = db::list_pipeline_events(&conn, &run.run_id).expect("events should load");
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
            ]
        );
    }

    #[test]
    fn empty_page_is_preserved_with_visual_routing_marker() {
        let pdf = save_pdf(build_pdf(vec![
            FixturePage::Text(b"BEFORE_EMPTY".to_vec()),
            FixturePage::Empty,
            FixturePage::Text(b"AFTER_EMPTY".to_vec()),
        ]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);

        let parsed = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect("PDF should parse");

        assert_eq!(parsed.pages.len(), 3);
        assert_eq!(parsed.pages[1].page_number, 2);
        assert!(parsed.pages[1].text.trim().is_empty());
        assert!(parsed.pages[1].requires_visual_processing);
        assert!(parsed.pages[1]
            .warnings
            .iter()
            .any(|warning| warning.code == "NO_NATIVE_TEXT"));
        assert!(parsed.pages[2].text.contains("AFTER_EMPTY"));
    }

    #[test]
    fn image_only_pdf_reaches_parsed_with_no_native_text_warnings() {
        let pdf = save_pdf(build_pdf(vec![FixturePage::ImageOnly]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);

        let parsed = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect("image-only PDF should still parse");

        assert_eq!(parsed.pages.len(), 1);
        assert!(parsed.pages[0].text.trim().is_empty());
        assert!(parsed.pages[0].requires_visual_processing);
        assert!(parsed
            .warnings
            .iter()
            .any(|warning| warning.code == "NO_NATIVE_TEXT_IN_DOCUMENT"));
        assert_eq!(
            db::get_pipeline_run(&conn, &run.run_id)
                .expect("run should load")
                .expect("run should exist")
                .state,
            PipelineState::Parsed
        );
    }

    #[test]
    fn malformed_pdf_fails_structurally_and_never_persists_an_artifact() {
        let pdf = TestPath::write("pdf", b"%PDF-1.7\nnot a structurally valid document");
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);

        let error = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect_err("malformed PDF should fail");

        assert_eq!(error.code(), "MALFORMED_PDF");
        let failed_run = db::get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed_run.state, PipelineState::Failed);
        assert_eq!(
            failed_run.failure.expect("failure should persist").code,
            "MALFORMED_PDF"
        );
        assert!(db::get_parsed_document(&conn, &run.run_id)
            .expect("artifact query should work")
            .is_none());
        assert!(!db::list_pipeline_events(&conn, &run.run_id)
            .expect("events should load")
            .iter()
            .any(|event| event.next_state == PipelineState::Parsed));
    }

    #[test]
    fn encrypted_pdf_is_rejected_deterministically() {
        let mut document = build_pdf(vec![FixturePage::Text(b"ENCRYPTED".to_vec())]);
        document.trailer.set(
            "ID",
            Object::Array(vec![
                Object::string_literal("fixture-id-one"),
                Object::string_literal("fixture-id-two"),
            ]),
        );
        let encryption = EncryptionState::try_from(EncryptionVersion::V1 {
            document: &document,
            owner_password: "owner-password",
            user_password: "user-password",
            permissions: Permissions::PRINTABLE,
        })
        .expect("encryption state should build");
        document
            .encrypt(&encryption)
            .expect("fixture should encrypt");
        let pdf = save_pdf(document);
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);

        let error = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect_err("encrypted PDF should be unsupported");

        assert_eq!(error.code(), "ENCRYPTED_PDF_UNSUPPORTED");
        assert_eq!(
            db::get_pipeline_run(&conn, &run.run_id)
                .expect("run should load")
                .expect("run should exist")
                .failure
                .expect("failure should persist")
                .code,
            "ENCRYPTED_PDF_UNSUPPORTED"
        );
    }

    #[test]
    fn missing_source_during_parse_becomes_a_structured_failed_run() {
        let pdf = save_pdf(build_pdf(vec![FixturePage::Text(b"DISAPPEARS".to_vec())]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);
        fs::remove_file(&pdf.0).expect("fixture should be removable");

        let error = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect_err("missing source should fail");

        assert_eq!(error.code(), "SOURCE_IO_ERROR");
        let failed = db::get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert!(failed.failure.expect("failure should persist").recoverable);
    }

    #[test]
    fn same_size_source_replacement_is_rejected_against_ingested_identity() {
        let original_pdf = save_pdf(build_pdf(vec![FixturePage::Text(b"ORIGINAL_A".to_vec())]));
        let replacement_pdf = save_pdf(build_pdf(vec![FixturePage::Text(b"MODIFIED_B".to_vec())]));
        let original_bytes = fs::read(&original_pdf.0).expect("original fixture should read");
        let replacement_bytes =
            fs::read(&replacement_pdf.0).expect("replacement fixture should read");
        assert_eq!(original_bytes.len(), replacement_bytes.len());
        assert_ne!(original_bytes, replacement_bytes);

        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (ingested, run) = ingest_pdf(
            &mut conn,
            original_pdf.0.to_str().expect("UTF-8 fixture path"),
        )
        .expect("original PDF should ingest");
        fs::write(&original_pdf.0, &replacement_bytes)
            .expect("fixture replacement should be writable");

        let error = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect_err("changed source bytes must not parse under the original identity");

        assert_eq!(error.code(), "SOURCE_CONTENT_CHANGED");
        let failed = db::get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert!(failed.failure.expect("failure should persist").recoverable);
        let persisted_document = db::get_document(&conn, &ingested.document_id)
            .expect("document should load")
            .expect("document should exist");
        assert_eq!(persisted_document.content_hash, ingested.content_hash);
        assert_eq!(persisted_document.byte_size, ingested.byte_size);
        assert!(db::get_parsed_document(&conn, &run.run_id)
            .expect("artifact query should work")
            .is_none());
    }

    #[test]
    fn parsed_artifact_and_history_survive_connection_reopen() {
        let pdf = save_pdf(build_pdf(vec![
            FixturePage::Text(b"PERSIST_ONE".to_vec()),
            FixturePage::Empty,
        ]));
        let database = TestPath::new("db");
        let run_id;
        let expected_artifact;
        let expected_events;
        {
            let mut conn = db::init_db(&database.0).expect("schema should initialize");
            let run = ingest_fixture(&mut conn, &pdf.0);
            expected_artifact = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
                .expect("PDF should parse");
            expected_events =
                db::list_pipeline_events(&conn, &run.run_id).expect("events should load");
            run_id = run.run_id;
        }

        let reopened = db::init_db(&database.0).expect("database should reopen");
        let artifact = db::get_parsed_document(&reopened, &run_id)
            .expect("artifact should load")
            .expect("artifact should exist");
        assert_eq!(artifact, expected_artifact);
        assert_eq!(
            db::list_pipeline_events(&reopened, &run_id).expect("events should load"),
            expected_events
        );
        assert_eq!(
            db::get_pipeline_run(&reopened, &run_id)
                .expect("run should load")
                .expect("run should exist")
                .state,
            PipelineState::Parsed
        );
    }

    #[test]
    fn parsed_event_failure_rolls_back_artifact_before_marking_run_failed() {
        let pdf = save_pdf(build_pdf(vec![FixturePage::Text(
            b"ROLLBACK_PARSE".to_vec(),
        )]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);
        conn.execute_batch(
            "CREATE TRIGGER test_fail_parsed_event
             BEFORE INSERT ON pipeline_events
             WHEN NEW.next_state = '\"Parsed\"'
             BEGIN
                 SELECT RAISE(ABORT, 'injected parsed event failure');
             END;",
        )
        .expect("failure trigger should install");

        let error = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect_err("parsed event failure should fail the run");

        assert_eq!(error.code(), "PARSED_ARTIFACT_PERSISTENCE_FAILED");
        assert!(db::get_parsed_document(&conn, &run.run_id)
            .expect("artifact query should work")
            .is_none());
        let failed = db::get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert_eq!(
            failed.failure.expect("failure should persist").code,
            "PARSED_ARTIFACT_PERSISTENCE_FAILED"
        );
    }

    #[test]
    fn parsing_cannot_bypass_state_machine_or_reparse_a_parsed_run() {
        let pdf = save_pdf(build_pdf(vec![FixturePage::Text(b"ONCE_ONLY".to_vec())]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);
        parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect("first parse should succeed");
        let expected_events =
            db::list_pipeline_events(&conn, &run.run_id).expect("events should load");

        let second = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id);
        assert!(matches!(second, Err(ParsePipelineError::Store(_))));
        assert_eq!(
            db::get_pipeline_run(&conn, &run.run_id)
                .expect("run should load")
                .expect("run should exist")
                .state,
            PipelineState::Parsed
        );
        assert_eq!(
            db::list_pipeline_events(&conn, &run.run_id).expect("events should load"),
            expected_events
        );
    }

    struct InvalidOrderParser;

    impl DocumentParser for InvalidOrderParser {
        fn parse(&self, document: &IngestedDocument) -> Result<ParsedDocument, PipelineFailure> {
            Ok(ParsedDocument {
                document_id: document.document_id.clone(),
                parser_id: self.id().to_string(),
                parser_version: self.version().to_string(),
                pages: vec![ParsedPage {
                    page_number: 2,
                    text: "wrong ordinal".to_string(),
                    warnings: Vec::new(),
                    requires_visual_processing: false,
                }],
                warnings: Vec::new(),
            })
        }

        fn id(&self) -> &'static str {
            "invalid-order-test-parser"
        }

        fn version(&self) -> &'static str {
            "test"
        }
    }

    #[test]
    fn invalid_parser_artifact_is_rejected_before_persistence() {
        let pdf = save_pdf(build_pdf(vec![FixturePage::Text(b"VALID_SOURCE".to_vec())]));
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let run = ingest_fixture(&mut conn, &pdf.0);

        let error = parse_document(&mut conn, &InvalidOrderParser, &run.run_id)
            .expect_err("invalid page order should fail validation");

        assert_eq!(error.code(), "INVALID_PARSED_ARTIFACT");
        assert!(db::get_parsed_document(&conn, &run.run_id)
            .expect("artifact query should work")
            .is_none());
        assert_eq!(
            db::get_pipeline_run(&conn, &run.run_id)
                .expect("run should load")
                .expect("run should exist")
                .state,
            PipelineState::Failed
        );
    }
}
