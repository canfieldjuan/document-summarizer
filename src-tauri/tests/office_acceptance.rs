use document_summarizer_lib::pipeline::chunk::{chunk_document, DeterministicDocumentChunker};
use document_summarizer_lib::pipeline::contracts::{
    ModelRequest, ModelResponse, ModelRuntime, ModelRuntimeFailure, NormalizedDocument,
    PipelineState, StructureNode,
};
use document_summarizer_lib::pipeline::db::{
    get_chunked_document, get_citation_artifact, get_normalized_document, get_parsed_document,
    get_pipeline_run, get_structured_document, get_summary_artifact, init_db, list_pipeline_events,
};
use document_summarizer_lib::pipeline::ingest::ingest_pdf;
use document_summarizer_lib::pipeline::model::OllamaRuntime;
use document_summarizer_lib::pipeline::normalize::{normalize_document, CanonicalNormalizer};
use document_summarizer_lib::pipeline::parser::{parse_document, PdfExtractParser};
use document_summarizer_lib::pipeline::service::{process_pdf_to_summary, SummaryComponents};
use document_summarizer_lib::pipeline::structure::{
    structure_document, DeterministicStructureInterpreter,
};
use document_summarizer_lib::pipeline::summary::analyze_chunked_document;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

struct TestDatabase(PathBuf);

impl TestDatabase {
    fn new(label: &str) -> Self {
        Self(env::temp_dir().join(format!("doc-sum-office-{label}-{}.db", Uuid::new_v4())))
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

struct RecordingRuntime<'a> {
    inner: &'a dyn ModelRuntime,
    responses: Mutex<Vec<ModelResponse>>,
}

impl<'a> RecordingRuntime<'a> {
    fn new(inner: &'a dyn ModelRuntime) -> Self {
        Self {
            inner,
            responses: Mutex::new(Vec::new()),
        }
    }

    fn responses(&self) -> Vec<ModelResponse> {
        self.responses
            .lock()
            .expect("recording runtime lock should not be poisoned")
            .clone()
    }
}

impl ModelRuntime for RecordingRuntime<'_> {
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        let response = self.inner.generate(request)?;
        self.responses
            .lock()
            .expect("recording runtime lock should not be poisoned")
            .push(response.clone());
        Ok(response)
    }

    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        self.inner.health()
    }

    fn runtime_id(&self) -> &str {
        self.inner.runtime_id()
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }
}

fn reveal_model_text() -> bool {
    env::var("DOC_SUM_OFFICE_TRACE_MODEL_RESPONSES").as_deref() == Ok("1")
}

fn add_optional_summary(report: &mut serde_json::Value, summary: &str, reveal_text: bool) {
    if reveal_text {
        report["summary"] = json!(summary);
    }
}

fn print_recorded_responses(runtime: &RecordingRuntime<'_>) {
    let reveal_text = reveal_model_text();
    eprintln!("OFFICE_LIVE_MODEL_RESPONSES");
    for (index, response) in runtime.responses().iter().enumerate() {
        if reveal_text {
            eprintln!("response[{index}]={}", response.text);
        } else {
            eprintln!(
                "response[{index}]: chars={}, sha256={:x}",
                response.text.chars().count(),
                Sha256::digest(response.text.as_bytes())
            );
        }
    }
}

#[derive(Serialize)]
struct DeterministicReport {
    filename: String,
    source_sha256: String,
    source_bytes: u64,
    pages: usize,
    native_text_characters: usize,
    visual_pages: Vec<u32>,
    normalized_blocks: usize,
    structure_nodes: usize,
    chunks: usize,
    chunk_character_counts: Vec<usize>,
    chunk_page_spans: Vec<Vec<String>>,
    warning_codes: Vec<String>,
    parsed_artifact_sha256: String,
    normalized_artifact_sha256: String,
    structured_artifact_sha256: String,
    chunked_artifact_sha256: String,
    final_state: String,
    state_version: u32,
    event_count: usize,
}

fn configured_paths(variable: &str) -> Vec<PathBuf> {
    let raw = env::var_os(variable)
        .unwrap_or_else(|| panic!("{variable} must contain one or more PDF paths"));
    let paths = env::split_paths(&raw).collect::<Vec<_>>();
    assert!(!paths.is_empty(), "{variable} must not be empty");
    for path in &paths {
        assert!(path.is_file(), "configured PDF does not exist: {path:?}");
    }
    paths
}

fn file_identity(path: &Path) -> (Vec<u8>, String, u64) {
    let bytes = fs::read(path).expect("configured PDF should be readable");
    let byte_size = u64::try_from(bytes.len()).expect("PDF byte size should fit in u64");
    let hash = format!("{:x}", Sha256::digest(&bytes));
    (bytes, hash, byte_size)
}

fn artifact_hash<T: Serialize>(artifact: &T) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(artifact).expect("acceptance artifact should serialize"))
    )
}

fn collect_structure_blocks<'a>(
    node: &'a StructureNode,
    block_ids: &mut Vec<&'a str>,
    node_count: &mut usize,
) {
    *node_count += 1;
    block_ids.extend(node.block_ids.iter().map(String::as_str));
    for child in &node.children {
        collect_structure_blocks(child, block_ids, node_count);
    }
}

fn normalized_blocks(document: &NormalizedDocument) -> HashMap<&str, &str> {
    document
        .pages
        .iter()
        .flat_map(|page| page.content.iter())
        .map(|block| (block.block_id.as_str(), block.text.as_str()))
        .collect()
}

fn trace_matching_source_lines(document: &NormalizedDocument) {
    let Ok(needle) = env::var("DOC_SUM_OFFICE_TRACE_SOURCE_CONTAINS") else {
        return;
    };
    for block in document.pages.iter().flat_map(|page| page.content.iter()) {
        let lines = block.text.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            if line.contains(&needle) {
                let first = index.saturating_sub(1);
                let last = (index + 2).min(lines.len());
                eprintln!(
                    "OFFICE_SOURCE_MATCH block_id={} lines={}..{} text={:?}",
                    block.block_id,
                    first + 1,
                    last,
                    lines[first..last].join("\n")
                );
            }
        }
    }
}

#[test]
#[ignore = "requires one or more external PDFs in DOC_SUM_OFFICE_PDFS"]
fn office_pdf_deterministic_checkpoints_survive_reopen() {
    let mut reports = Vec::new();

    for source in configured_paths("DOC_SUM_OFFICE_PDFS") {
        let database = TestDatabase::new("deterministic");
        let (source_bytes, source_hash, source_size) = file_identity(&source);
        let parser = PdfExtractParser::new();
        let normalizer = CanonicalNormalizer::new();
        let interpreter = DeterministicStructureInterpreter::new();
        let chunker = DeterministicDocumentChunker::new();

        let mut conn = init_db(&database.0).expect("acceptance database should initialize");
        let (document, ingested_run) = ingest_pdf(
            &mut conn,
            source
                .to_str()
                .expect("configured PDF path should be UTF-8"),
        )
        .expect("office PDF should ingest");
        assert_eq!(document.content_hash, source_hash);
        assert_eq!(document.byte_size, source_size);

        let parsed = parse_document(&mut conn, &parser, &ingested_run.run_id)
            .expect("office PDF should parse deterministically");
        let normalized = normalize_document(&mut conn, &normalizer, &ingested_run.run_id)
            .expect("parsed office PDF should normalize");
        trace_matching_source_lines(&normalized);
        let structured = structure_document(&mut conn, &interpreter, &ingested_run.run_id)
            .expect("normalized office PDF should structure");
        let chunked = chunk_document(&mut conn, &chunker, &ingested_run.run_id)
            .expect("structured office PDF should chunk");

        assert_eq!(parsed.pages.len(), normalized.pages.len());
        assert_eq!(normalized.pages.len(), structured.pages.len());
        for (index, ((parsed_page, normalized_page), structured_page)) in parsed
            .pages
            .iter()
            .zip(&normalized.pages)
            .zip(&structured.pages)
            .enumerate()
        {
            let expected_page = u32::try_from(index + 1).expect("page number should fit in u32");
            assert_eq!(parsed_page.page_number, expected_page);
            assert_eq!(normalized_page.page_number, expected_page);
            assert_eq!(structured_page.page_number, expected_page);
            assert_eq!(
                parsed_page.requires_visual_processing,
                normalized_page.requires_visual_processing
            );
            assert_eq!(
                normalized_page.requires_visual_processing,
                structured_page.requires_visual_processing
            );
            assert!(normalized_page.content.iter().all(|block| {
                block.source.page_start == expected_page && block.source.page_end == expected_page
            }));
        }

        let normalized_by_id = normalized_blocks(&normalized);
        let expected_block_ids = normalized_by_id.keys().copied().collect::<HashSet<_>>();
        let mut represented_block_ids = Vec::new();
        let mut structure_node_count = 0;
        for node in &structured.nodes {
            collect_structure_blocks(node, &mut represented_block_ids, &mut structure_node_count);
        }
        let represented_unique = represented_block_ids
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        assert_eq!(represented_block_ids.len(), represented_unique.len());
        assert_eq!(represented_unique, expected_block_ids);

        let run = get_pipeline_run(&conn, &ingested_run.run_id)
            .expect("chunked run should load")
            .expect("chunked run should exist");
        assert_eq!(run.state, PipelineState::Chunked);
        let events =
            list_pipeline_events(&conn, &ingested_run.run_id).expect("pipeline events should load");
        assert!(events
            .windows(2)
            .all(|pair| pair[1].sequence_no == pair[0].sequence_no + 1));

        let parsed_hash = artifact_hash(&parsed);
        let normalized_hash = artifact_hash(&normalized);
        let structured_hash = artifact_hash(&structured);
        let chunked_hash = artifact_hash(&chunked);
        drop(conn);

        let reopened =
            init_db(&database.0).expect("acceptance database should independently reopen");
        assert_eq!(
            get_parsed_document(&reopened, &ingested_run.run_id)
                .expect("parsed artifact should reload")
                .expect("parsed artifact should persist"),
            parsed
        );
        assert_eq!(
            get_normalized_document(&reopened, &ingested_run.run_id)
                .expect("normalized artifact should reload")
                .expect("normalized artifact should persist"),
            normalized
        );
        assert_eq!(
            get_structured_document(&reopened, &ingested_run.run_id)
                .expect("structured artifact should reload")
                .expect("structured artifact should persist"),
            structured
        );
        assert_eq!(
            get_chunked_document(&reopened, &ingested_run.run_id)
                .expect("chunked artifact should reload")
                .expect("chunked artifact should persist"),
            chunked
        );
        assert_eq!(
            list_pipeline_events(&reopened, &ingested_run.run_id)
                .expect("events should reload after reopen"),
            events
        );
        let reopened_run = get_pipeline_run(&reopened, &ingested_run.run_id)
            .expect("run should reload after reopen")
            .expect("run should persist");
        assert_eq!(reopened_run, run);
        assert_eq!(
            fs::read(&source).expect("source should remain readable"),
            source_bytes
        );

        let mut warning_codes = parsed
            .warnings
            .iter()
            .chain(normalized.warnings.iter())
            .chain(structured.warnings.iter())
            .chain(chunked.warnings.iter())
            .map(|warning| warning.code.clone())
            .collect::<Vec<_>>();
        warning_codes.sort();
        warning_codes.dedup();
        reports.push(DeterministicReport {
            filename: document.original_filename,
            source_sha256: source_hash,
            source_bytes: source_size,
            pages: parsed.pages.len(),
            native_text_characters: parsed
                .pages
                .iter()
                .map(|page| page.text.chars().count())
                .sum(),
            visual_pages: parsed
                .pages
                .iter()
                .filter(|page| page.requires_visual_processing)
                .map(|page| page.page_number)
                .collect(),
            normalized_blocks: normalized_by_id.len(),
            structure_nodes: structure_node_count,
            chunks: chunked.chunks.len(),
            chunk_character_counts: chunked
                .chunks
                .iter()
                .map(|chunk| chunk.text.chars().count())
                .collect(),
            chunk_page_spans: chunked
                .chunks
                .iter()
                .map(|chunk| {
                    chunk
                        .source_spans
                        .iter()
                        .map(|span| format!("{}-{}", span.page_start, span.page_end))
                        .collect()
                })
                .collect(),
            warning_codes,
            parsed_artifact_sha256: parsed_hash,
            normalized_artifact_sha256: normalized_hash,
            structured_artifact_sha256: structured_hash,
            chunked_artifact_sha256: chunked_hash,
            final_state: format!("{:?}", reopened_run.state),
            state_version: reopened_run.state_version,
            event_count: events.len(),
        });
    }

    println!(
        "OFFICE_DETERMINISTIC_REPORT\n{}",
        serde_json::to_string_pretty(&reports).expect("acceptance report should serialize")
    );
}

#[test]
#[ignore = "requires one external PDF in DOC_SUM_OFFICE_PDF and configured Ollama"]
fn office_pdf_live_ollama_analysis_satisfies_evidence_contract() {
    let paths = configured_paths("DOC_SUM_OFFICE_PDF");
    assert_eq!(paths.len(), 1, "DOC_SUM_OFFICE_PDF must contain one path");
    let source = &paths[0];
    let database = TestDatabase::new("analysis");
    let ollama = OllamaRuntime::from_environment().expect("Ollama should configure");
    ollama
        .health()
        .expect("selected Ollama model should be available");
    let runtime = RecordingRuntime::new(&ollama);
    let mut conn = init_db(&database.0).expect("analysis database should initialize");
    let (_, run) = ingest_pdf(
        &mut conn,
        source
            .to_str()
            .expect("configured PDF path should be UTF-8"),
    )
    .expect("office PDF should ingest");
    parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
        .expect("office PDF should parse");
    normalize_document(&mut conn, &CanonicalNormalizer::new(), &run.run_id)
        .expect("office PDF should normalize");
    structure_document(
        &mut conn,
        &DeterministicStructureInterpreter::new(),
        &run.run_id,
    )
    .expect("office PDF should structure");
    let chunked = chunk_document(&mut conn, &DeterministicDocumentChunker::new(), &run.run_id)
        .expect("office PDF should chunk");

    let result = analyze_chunked_document(&mut conn, &runtime, &run.run_id);
    print_recorded_responses(&runtime);
    let analyzed = result.unwrap_or_else(|error| {
        panic!("live analysis should satisfy the evidence contract: {error:?}")
    });
    assert_eq!(analyzed.chunks.len(), chunked.chunks.len());
    assert!(analyzed
        .chunks
        .iter()
        .all(|analysis| !analysis.evidence.is_empty()));
    println!(
        "OFFICE_LIVE_ANALYSIS_REPORT\n{}",
        serde_json::to_string_pretty(&json!({
            "filename": source.file_name().and_then(|name| name.to_str()),
            "model_calls": runtime.responses().len(),
            "chunks": analyzed.chunks.len(),
            "evidence_items": analyzed
                .chunks
                .iter()
                .map(|analysis| analysis.evidence.len())
                .sum::<usize>(),
        }))
        .expect("analysis report should serialize")
    );
}

#[test]
#[ignore = "requires one external PDF in DOC_SUM_OFFICE_PDF and configured Ollama"]
fn office_pdf_live_ollama_summary_has_exact_durable_evidence() {
    let paths = configured_paths("DOC_SUM_OFFICE_PDF");
    assert_eq!(paths.len(), 1, "DOC_SUM_OFFICE_PDF must contain one path");
    let source = &paths[0];
    let database = TestDatabase::new("live");
    let (source_bytes, source_hash, source_size) = file_identity(source);
    let ollama = OllamaRuntime::from_environment().expect("Ollama should configure");
    ollama
        .health()
        .expect("selected Ollama model should be available");
    let runtime = RecordingRuntime::new(&ollama);
    let parser = PdfExtractParser::new();
    let normalizer = CanonicalNormalizer::new();
    let interpreter = DeterministicStructureInterpreter::new();
    let chunker = DeterministicDocumentChunker::new();

    let mut conn = init_db(&database.0).expect("live acceptance database should initialize");
    let result = process_pdf_to_summary(
        &mut conn,
        source
            .to_str()
            .expect("configured PDF path should be UTF-8"),
        SummaryComponents {
            parser: &parser,
            normalizer: &normalizer,
            interpreter: &interpreter,
            chunker: &chunker,
            runtime: &runtime,
        },
    );
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            print_recorded_responses(&runtime);
            panic!("live office PDF pipeline should complete: {error:?}");
        }
    };
    assert_eq!(result.document.content_hash, source_hash);
    assert_eq!(result.document.byte_size, source_size);
    assert_eq!(result.summary.text, result.citations.rendered_text);
    assert_eq!(
        result.summary.integrity_hash,
        result.citations.summary_integrity_hash
    );
    assert!(!result.citations.claims.is_empty());
    assert!(!result.citations.evidence.is_empty());

    let normalized = get_normalized_document(&conn, &result.run_id)
        .expect("normalized artifact should load")
        .expect("normalized artifact should exist");
    let blocks = normalized
        .pages
        .iter()
        .flat_map(|page| page.content.iter())
        .map(|block| (block.block_id.as_str(), block))
        .collect::<HashMap<_, _>>();
    for evidence in &result.citations.evidence {
        let block = blocks
            .get(evidence.block_id.as_str())
            .expect("citation must reference a normalized block");
        assert!(block.text.contains(&evidence.exact_quote));
        assert_eq!(block.source, evidence.source_span);
    }

    let run = get_pipeline_run(&conn, &result.run_id)
        .expect("completed run should load")
        .expect("completed run should exist");
    assert!(matches!(
        run.state,
        PipelineState::Complete | PipelineState::CompleteWithWarnings
    ));
    let events =
        list_pipeline_events(&conn, &result.run_id).expect("completed pipeline events should load");
    let run_id = result.run_id.clone();
    let expected_summary = result.summary.clone();
    let expected_citations = result.citations.clone();
    drop(conn);

    let reopened = init_db(&database.0).expect("live acceptance database should reopen");
    assert_eq!(
        get_summary_artifact(&reopened, &run_id)
            .expect("summary should reload")
            .expect("summary should persist"),
        expected_summary
    );
    assert_eq!(
        get_citation_artifact(&reopened, &run_id)
            .expect("citations should reload")
            .expect("citations should persist"),
        expected_citations
    );
    assert_eq!(
        list_pipeline_events(&reopened, &run_id).expect("events should reload"),
        events
    );
    assert_eq!(
        fs::read(source).expect("source should remain readable"),
        source_bytes
    );

    let mut report = json!({
        "filename": result.document.original_filename,
        "source_sha256": source_hash,
        "source_bytes": source_size,
        "runtime_id": runtime.runtime_id(),
        "model_id": runtime.model_id(),
        "final_state": format!("{:?}", run.state),
        "state_version": run.state_version,
        "event_count": events.len(),
        "claim_count": expected_citations.claims.len(),
        "evidence_count": expected_citations.evidence.len(),
        "warning_codes": expected_summary
            .warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect::<Vec<_>>(),
        "summary_integrity_hash": expected_summary.integrity_hash,
        "citation_integrity_hash": expected_citations.integrity_hash,
        "summary_characters": expected_summary.text.chars().count(),
        "summary_sha256": format!("{:x}", Sha256::digest(expected_summary.text.as_bytes())),
    });
    add_optional_summary(&mut report, &expected_summary.text, reveal_model_text());
    println!(
        "OFFICE_LIVE_REPORT\n{}",
        serde_json::to_string_pretty(&report).expect("live acceptance report should serialize")
    );
}

#[test]
fn office_report_hides_summary_text_unless_explicitly_revealed() {
    let sentinel = "PRIVATE-OFFICE-SUMMARY-SENTINEL";
    let base_report = json!({
        "summary_characters": sentinel.chars().count(),
        "summary_sha256": format!("{:x}", Sha256::digest(sentinel.as_bytes())),
    });

    let mut hidden = base_report.clone();
    add_optional_summary(&mut hidden, sentinel, false);
    let hidden_output = serde_json::to_string(&hidden).expect("hidden report should serialize");
    assert!(hidden.get("summary").is_none());
    assert!(!hidden_output.contains(sentinel));

    let mut revealed = base_report;
    add_optional_summary(&mut revealed, sentinel, true);
    assert_eq!(revealed["summary"], sentinel);
}
