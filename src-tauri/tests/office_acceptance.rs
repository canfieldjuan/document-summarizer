use document_summarizer_lib::pipeline::chunk::{chunk_document, DeterministicDocumentChunker};
use document_summarizer_lib::pipeline::contracts::{
    AnalysisOmissionReason, ModelOutputFormat, ModelRequest, ModelResponse, ModelRuntime,
    ModelRuntimeFailure, NormalizedDocument, PipelineState, SourceType, StructureNode,
};
use document_summarizer_lib::pipeline::db::{
    get_analyzed_document, get_chunked_document, get_citation_artifact, get_normalized_document,
    get_parsed_document, get_pipeline_run, get_structured_document, get_summary_artifact,
    get_synthesis_attempt, get_verified_document, init_db, list_pipeline_events,
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

fn coverage_at_least_sixty_percent(cited: usize, total: usize) -> bool {
    total > 0 && cited <= total && (cited as u128) * 5 >= (total as u128) * 3
}

fn coverage_at_least_fifty_percent(cited: usize, total: usize) -> bool {
    total > 0 && cited <= total && (cited as u128) * 2 >= total as u128
}

fn omission_reduces_adjusted_denominator(reason: &AnalysisOmissionReason) -> bool {
    matches!(
        reason,
        AnalysisOmissionReason::NonSubstantivePageFurniture
            | AnalysisOmissionReason::NoSubstantiveContent
    )
}

fn evidence_coverage_accepted(
    retained: &HashSet<&str>,
    synthesized: &HashSet<&str>,
    supported: &HashSet<&str>,
) -> bool {
    synthesized == retained
        && supported.is_subset(retained)
        && coverage_at_least_sixty_percent(supported.len(), retained.len())
}

#[test]
fn coverage_gate_separates_synthesized_and_supported_evidence() {
    let retained = HashSet::from(["a", "b", "c", "d", "e"]);
    assert!(evidence_coverage_accepted(
        &retained,
        &retained,
        &HashSet::from(["a", "b", "c"])
    ));
    assert!(!evidence_coverage_accepted(
        &retained,
        &retained,
        &HashSet::from(["a", "b"])
    ));
    assert!(!evidence_coverage_accepted(
        &retained,
        &HashSet::from(["a", "b", "c", "d"]),
        &HashSet::from(["a", "b", "c"])
    ));
    assert!(!evidence_coverage_accepted(
        &retained,
        &retained,
        &HashSet::from(["a", "b", "foreign"])
    ));
    for (cited, total, expected) in [
        (0, 0, false),
        (0, 1, false),
        (1, 1, true),
        (2, 1, false),
        (59, 100, false),
        (60, 100, true),
        (61, 100, true),
        (66, 111, false),
        (67, 111, true),
        (600_000, 1_000_000, true),
    ] {
        assert_eq!(coverage_at_least_sixty_percent(cited, total), expected);
    }
    for (cited, total, expected) in [
        (0, 0, false),
        (0, 1, false),
        (1, 1, true),
        (2, 1, false),
        (49, 100, false),
        (50, 100, true),
        (51, 100, true),
    ] {
        assert_eq!(coverage_at_least_fifty_percent(cited, total), expected);
    }
    assert!(omission_reduces_adjusted_denominator(
        &AnalysisOmissionReason::NonSubstantivePageFurniture
    ));
    assert!(omission_reduces_adjusted_denominator(
        &AnalysisOmissionReason::NoSubstantiveContent
    ));
    assert!(!omission_reduces_adjusted_denominator(
        &AnalysisOmissionReason::ParaphraseUnrepairable
    ));
}

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
    requests: Mutex<Vec<ModelRequest>>,
    responses: Mutex<Vec<ModelResponse>>,
    failures: Mutex<Vec<ModelRuntimeFailure>>,
}

impl<'a> RecordingRuntime<'a> {
    fn new(inner: &'a dyn ModelRuntime) -> Self {
        Self {
            inner,
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(Vec::new()),
            failures: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<ModelRequest> {
        self.requests
            .lock()
            .expect("recording runtime lock should not be poisoned")
            .clone()
    }

    fn responses(&self) -> Vec<ModelResponse> {
        self.responses
            .lock()
            .expect("recording runtime lock should not be poisoned")
            .clone()
    }

    fn failures(&self) -> Vec<ModelRuntimeFailure> {
        self.failures
            .lock()
            .expect("recording runtime lock should not be poisoned")
            .clone()
    }
}

impl ModelRuntime for RecordingRuntime<'_> {
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        self.requests
            .lock()
            .expect("recording runtime lock should not be poisoned")
            .push(request.clone());
        match self.inner.generate(request) {
            Ok(response) => {
                self.responses
                    .lock()
                    .expect("recording runtime lock should not be poisoned")
                    .push(response.clone());
                Ok(response)
            }
            Err(failure) => {
                self.failures
                    .lock()
                    .expect("recording runtime lock should not be poisoned")
                    .push(failure.clone());
                Err(failure)
            }
        }
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
    eprintln!("OFFICE_LIVE_MODEL_REQUESTS");
    for (index, request) in runtime.requests().iter().enumerate() {
        let schema = match &request.output_format {
            ModelOutputFormat::JsonSchema { name, .. } => name.as_str(),
            ModelOutputFormat::Text => "text",
        };
        eprintln!(
            "request[{index}]: schema={schema}, system_chars={}, user_chars={}",
            request.system_prompt.chars().count(),
            request.user_prompt.chars().count()
        );
    }
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
    let requests = runtime.requests();
    let responses = runtime.responses();
    let paraphrase_requests = requests.iter().filter(|r| matches!(&r.output_format,
        ModelOutputFormat::JsonSchema { name, .. } if name.starts_with("document_quote_paraphrase_"))).collect::<Vec<_>>();
    let paraphrases = requests
        .iter()
        .zip(&responses)
        .filter_map(|(request, response)| paraphrase_metric(request, response))
        .collect::<Vec<_>>();
    eprintln!(
        "OFFICE_LIVE_PARAPHRASE_METRICS {}",
        json!({
            "calls": paraphrase_requests.len(),
            "repairs": paraphrase_requests.iter().filter(|r| serde_json::from_str::<serde_json::Value>(&r.user_prompt).is_ok_and(|u| u.get("repair").is_some())).count(),
            "responses": paraphrases
        })
    );
    eprintln!("OFFICE_LIVE_MODEL_REQUEST_ATTEMPTS");
    let responses = runtime.responses();
    let failures = runtime.failures();
    for attempt in responses
        .iter()
        .flat_map(|response| response.request_attempts.iter())
        .chain(
            failures
                .iter()
                .flat_map(|failure| failure.request_attempts.iter()),
        )
    {
        eprintln!(
            "stage={:?}, request_ordinal={}, attempt_ordinal={}, transport={:?}, elapsed_ms={}, configured_output_tokens={}, prompt_tokens={:?}, completion_tokens={:?}, total_tokens={:?}, succeeded={}",
            attempt.stage,
            attempt.request_ordinal,
            attempt.attempt_ordinal,
            attempt.transport_attempt,
            attempt.elapsed_milliseconds,
            attempt.configured_output_tokens,
            attempt.provider_usage.prompt_tokens,
            attempt.provider_usage.completion_tokens,
            attempt.provider_usage.total_tokens,
            attempt.succeeded,
        );
    }
}

fn paraphrase_metric(
    request: &ModelRequest,
    response: &ModelResponse,
) -> Option<serde_json::Value> {
    let ModelOutputFormat::JsonSchema { name, .. } = &request.output_format else {
        return None;
    };
    if !name.starts_with("document_quote_paraphrase_") {
        return None;
    }
    let user: serde_json::Value = serde_json::from_str(&request.user_prompt).ok()?;
    let output: serde_json::Value = serde_json::from_str(&response.text).ok()?;
    Some(json!({
        "request_ordinal":request.ordinal,
        "is_retry":user.get("repair").is_some(),
        "draft_characters":user.get("rejected_draft").and_then(|s| s.as_str()).map(|s| s.chars().count()),
        "claim_characters":output.get("claim_text").and_then(|s| s.as_str()).map(|s| s.chars().count())
    }))
}

#[test]
fn paraphrase_length_metrics_do_not_expose_source_or_rejected_draft() {
    let request = ModelRequest {
        stage:document_summarizer_lib::pipeline::contracts::PipelineStage::Analyze,
        ordinal:4, seed:42, max_output_tokens:2048, system_prompt:"private instruction".into(),
        user_prompt:json!({"exact_quote":"private source", "rejected_draft":"secret draft", "repair":{"target_characters":384}}).to_string(),
        output_format:ModelOutputFormat::JsonSchema { name:"document_quote_paraphrase_v3".into(), schema:json!({}) }
    };
    let response = ModelResponse {
        text: json!({"claim_text":"A claim."}).to_string(),
        runtime_id: "fixture".into(),
        model_id: "fixture".into(),
        request_attempts: Vec::new(),
    };
    let metric = paraphrase_metric(&request, &response).unwrap();
    assert_eq!(
        metric,
        json!({"request_ordinal":4,"is_retry":true,"draft_characters":12,"claim_characters":8})
    );
    for secret in ["private", "secret draft", "A claim."] {
        assert!(!metric.to_string().contains(secret));
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
    let started = std::time::Instant::now();
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
    print_recorded_responses(&runtime);
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
    let analyzed = get_analyzed_document(&conn, &result.run_id)
        .expect("analyzed artifact should load")
        .expect("analyzed artifact should exist");
    let blocks = normalized
        .pages
        .iter()
        .flat_map(|page| page.content.iter())
        .map(|block| (block.block_id.as_str(), block))
        .collect::<HashMap<_, _>>();
    let visual_pages = normalized
        .pages
        .iter()
        .filter(|page| page.requires_visual_processing)
        .map(|page| page.page_number)
        .collect::<Vec<_>>();
    let mut cited_pages = result
        .citations
        .evidence
        .iter()
        .flat_map(|evidence| evidence.source_span.page_start..=evidence.source_span.page_end)
        .collect::<Vec<_>>();
    cited_pages.sort_unstable();
    cited_pages.dedup();
    let native_text_pages = normalized
        .pages
        .iter()
        .filter(|page| {
            page.content.iter().any(|block| {
                block.source.source_type == SourceType::NativeText && !block.text.trim().is_empty()
            })
        })
        .map(|page| page.page_number)
        .collect::<HashSet<_>>();
    let cited_native_text_pages = cited_pages
        .iter()
        .filter(|page| native_text_pages.contains(page))
        .copied()
        .collect::<HashSet<_>>();
    let evidence_ids = analyzed
        .chunks
        .iter()
        .flat_map(|analysis| analysis.evidence.iter())
        .map(|evidence| evidence.evidence_id.as_str())
        .collect::<HashSet<_>>();
    let cited_evidence_ids = result
        .citations
        .claims
        .iter()
        .flat_map(|claim| claim.evidence_ids.iter().map(String::as_str))
        .collect::<HashSet<_>>();
    let verified = get_verified_document(&conn, &result.run_id)
        .expect("verification should load")
        .expect("verification should exist");
    let mut synthesis_attempts = Vec::new();
    for ordinal in 0..=1 {
        if let Some(attempt) = get_synthesis_attempt(&conn, &result.run_id, ordinal)
            .expect("synthesis attempt should load")
        {
            synthesis_attempts.push((ordinal, attempt));
        }
    }
    let selected_synthesis = &synthesis_attempts
        .iter()
        .find(|(ordinal, _)| *ordinal == verified.synthesis_attempt_ordinal)
        .expect("selected persisted synthesis must exist")
        .1;
    let synthesized_evidence_ids = selected_synthesis
        .claims
        .iter()
        .flat_map(|claim| claim.evidence_ids.iter().map(String::as_str))
        .collect::<HashSet<_>>();
    let claim_budget = 512;
    let claim_floor = 1;
    let omitted_pages = analyzed
        .omissions
        .iter()
        .map(|o| o.page_number)
        .collect::<HashSet<_>>();
    assert!(omitted_pages.is_subset(&native_text_pages));
    assert!(omitted_pages.is_disjoint(&cited_native_text_pages));
    let material_omitted_pages = analyzed
        .omissions
        .iter()
        .filter(|omission| omission_reduces_adjusted_denominator(&omission.reason))
        .map(|omission| omission.page_number)
        .collect::<HashSet<_>>();
    let technical_omitted_pages = analyzed
        .omissions
        .iter()
        .filter(|omission| !omission_reduces_adjusted_denominator(&omission.reason))
        .map(|omission| omission.page_number)
        .collect::<HashSet<_>>();
    let adjusted_native_text_pages = native_text_pages
        .difference(&material_omitted_pages)
        .copied()
        .collect::<HashSet<_>>();
    let acceptance_pages = (native_text_pages.len() * 3).div_ceil(5);
    let desired_headroom = 16;
    let retention_target = native_text_pages
        .len()
        .min(acceptance_pages + desired_headroom)
        .min(claim_budget);
    let retained_pages = analyzed
        .chunks
        .iter()
        .flat_map(|chunk| &chunk.evidence)
        .map(|item| item.source_span.page_start)
        .collect::<HashSet<_>>();
    let withheld_claim_count = verified
        .claim_verifications
        .iter()
        .filter(|v| {
            v.verdict != document_summarizer_lib::pipeline::contracts::ClaimVerdict::Supported
        })
        .count();
    // Print delivered metrics before quality assertions: a thin result must not
    // disappear from the report merely because the acceptance gate rejects it.
    eprintln!(
        "OFFICE_LIVE_DELIVERED_METRICS {}",
        json!({
            "claim_count": result.citations.claims.len(),
            "claim_budget": claim_budget,
            "claim_floor": claim_floor,
            "validated_evidence_count": evidence_ids.len(),
            "inspected_page_count": analyzed.inspected_pages.len(),
            "omitted_page_count": analyzed.omissions.len(),
            "omitted_pages": analyzed.omissions.iter().map(|omission| omission.page_number).collect::<Vec<_>>(),
            "material_omitted_pages": material_omitted_pages,
            "technical_omitted_pages": technical_omitted_pages,
            "cited_evidence_count": cited_evidence_ids.len(),
            "synthesized_evidence_count": synthesized_evidence_ids.len(),
            "supported_evidence_fraction": cited_evidence_ids.len() as f64 / evidence_ids.len() as f64,
            "supported_evidence_threshold": 0.6,
            "cited_native_text_page_count": cited_native_text_pages.len(),
            "native_text_page_count": native_text_pages.len(),
            "raw_cited_page_fraction": cited_native_text_pages.len() as f64 / native_text_pages.len() as f64,
            "adjusted_native_text_page_count": adjusted_native_text_pages.len(),
            "adjusted_cited_page_fraction": if adjusted_native_text_pages.is_empty() { None } else { Some(cited_native_text_pages.len() as f64 / adjusted_native_text_pages.len() as f64) },
            "omissions": analyzed.omissions,
            "acceptance_page_target": acceptance_pages,
            "desired_retention_headroom": desired_headroom,
            "retention_target": retention_target,
            "retained_page_count": retained_pages.len(),
            "actual_retained_margin": retained_pages.len().saturating_sub(acceptance_pages),
            "full_retention_reserve": retained_pages.len() >= acceptance_pages + desired_headroom,
            "withheld_claim_count": withheld_claim_count,
            "lost_page_count": retained_pages.difference(&cited_native_text_pages).count(),
            "wall_time_ms": started.elapsed().as_millis(),
            "request_count": runtime.requests().len(),
            "warning_codes": result.summary.warnings.iter().map(|warning| warning.code.as_str()).collect::<Vec<_>>(),
        })
    );
    if reveal_model_text() {
        println!(
            "OFFICE_LIVE_DELIVERED_SUMMARY\n{}\nOFFICE_LIVE_DELIVERED_SUMMARY_END",
            result.summary.text
        );
    }
    assert!(result.citations.claims.len() >= claim_floor);
    assert!(result.citations.claims.len() <= claim_budget);
    for (_, attempt) in &synthesis_attempts {
        let cited = attempt
            .claims
            .iter()
            .flat_map(|claim| claim.evidence_ids.iter().map(String::as_str))
            .collect::<HashSet<_>>();
        assert_eq!(
            cited, evidence_ids,
            "every synthesized attempt must cover all retained evidence"
        );
    }
    assert!(
        evidence_coverage_accepted(
            &evidence_ids,
            &synthesized_evidence_ids,
            &cited_evidence_ids
        ),
        "complete synthesized coverage and at least 60 percent supported evidence required"
    );
    if cited_evidence_ids != evidence_ids {
        assert!(
            result
                .summary
                .warnings
                .iter()
                .any(|warning| warning.code == "SUMMARY_COVERAGE_SHORTFALL"),
            "withheld evidence must remain visible as a durable warning"
        );
    }
    assert!(
        coverage_at_least_fifty_percent(cited_native_text_pages.len(), native_text_pages.len()),
        "at least 50 percent of all native-text pages must be cited"
    );
    assert!(
        coverage_at_least_sixty_percent(
            cited_native_text_pages.len(),
            adjusted_native_text_pages.len()
        ),
        "at least 60 percent of materially non-omitted native-text pages must be cited"
    );
    if source_hash == "290840e408f2769b9a0ed65b73aa15c116cae3685f125358717ad15cfbd29ec8" {
        assert!(
            result
                .summary
                .warnings
                .iter()
                .any(|warning| warning.code == "OCR_TEXT_LAYER_STRUCTURE_RISK"),
            "the scanned NARA robustness fixture must expose OCR text-layer structure risk"
        );
    }
    for evidence in &result.citations.evidence {
        let block = blocks
            .get(evidence.block_id.as_str())
            .expect("citation must reference a normalized block");
        assert!(block.text.contains(&evidence.exact_quote));
        assert_eq!(block.source, evidence.source_span);
    }
    for visual_page in &visual_pages {
        assert!(
            !cited_pages.contains(visual_page),
            "a visual-only page must not be cited as native text"
        );
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
        "claim_budget": claim_budget,
        "claim_floor": claim_floor,
        "evidence_count": expected_citations.evidence.len(),
        "native_text_page_count": native_text_pages.len(),
        "cited_native_text_page_count": cited_native_text_pages.len(),
        "visual_pages": visual_pages,
        "cited_pages": cited_pages,
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
