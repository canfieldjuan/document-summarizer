use crate::pipeline::contracts::{
    ChunkedDocument, DocumentChunk, DocumentChunker, NormalizedBlock, NormalizedDocument,
    PipelineFailure, PipelineStage, PipelineWarning, StructureNode, StructuredDocument,
};
use crate::pipeline::db::{self, StoreError};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

pub const CHUNKING_VERSION: &str = "1.0.0";
pub const TARGET_CHUNK_CHARACTERS: usize = 12_000;

#[derive(Default)]
pub struct DeterministicDocumentChunker;

impl DeterministicDocumentChunker {
    pub fn new() -> Self {
        Self
    }
}

impl DocumentChunker for DeterministicDocumentChunker {
    fn chunk(
        &self,
        normalized: &NormalizedDocument,
        structured: &StructuredDocument,
    ) -> Result<ChunkedDocument, PipelineFailure> {
        let inputs = validate_inputs(normalized, structured)?;
        let mut chunks = Vec::new();
        let mut warnings = structured.warnings.clone();
        warnings.extend(
            structured
                .pages
                .iter()
                .flat_map(|page| page.warnings.iter().cloned()),
        );

        if inputs.canonical_block_ids.is_empty() {
            warnings.push(PipelineWarning {
                code: "NO_TEXT_TO_CHUNK".to_string(),
                message: "The document contains no native text blocks to chunk".to_string(),
                stage: Some(PipelineStage::Chunk),
            });
        }

        for group in inputs.groups {
            let mut pending = Vec::<&NormalizedBlock>::new();
            let mut pending_characters = 0usize;

            for block_id in group.block_ids {
                let block = inputs.blocks.get(block_id).ok_or_else(|| {
                    chunk_failure(
                        "INVALID_STRUCTURED_DOCUMENT",
                        format!("Structured block {block_id} does not exist"),
                        false,
                    )
                })?;
                let separator = if pending.is_empty() { 0 } else { 2 };
                let block_characters = block.text.chars().count();
                if !pending.is_empty()
                    && pending_characters + separator + block_characters > TARGET_CHUNK_CHARACTERS
                {
                    chunks.push(materialize_chunk(
                        normalized,
                        self.version(),
                        group.node_id,
                        &pending,
                        chunks.len(),
                    )?);
                    pending.clear();
                    pending_characters = 0;
                }
                if !pending.is_empty() {
                    pending_characters += 2;
                }
                pending.push(block);
                pending_characters += block_characters;
            }

            if !pending.is_empty() {
                chunks.push(materialize_chunk(
                    normalized,
                    self.version(),
                    group.node_id,
                    &pending,
                    chunks.len(),
                )?);
            }
        }

        let chunked = ChunkedDocument {
            document_id: normalized.document_id.clone(),
            chunking_version: self.version().to_string(),
            chunks,
            warnings,
        };
        validate_chunked_document(&chunked, normalized, structured, self.version())?;
        Ok(chunked)
    }

    fn version(&self) -> &'static str {
        CHUNKING_VERSION
    }
}

struct ChunkInputs<'a> {
    blocks: HashMap<&'a str, &'a NormalizedBlock>,
    canonical_block_ids: Vec<&'a str>,
    groups: Vec<StructureGroup<'a>>,
}

struct StructureGroup<'a> {
    node_id: &'a str,
    block_ids: Vec<&'a str>,
}

fn validate_inputs<'a>(
    normalized: &'a NormalizedDocument,
    structured: &'a StructuredDocument,
) -> Result<ChunkInputs<'a>, PipelineFailure> {
    if normalized.document_id.trim().is_empty()
        || normalized.normalization_version.trim().is_empty()
        || structured.document_id != normalized.document_id
        || structured.structure_version.trim().is_empty()
    {
        return Err(chunk_failure(
            "INVALID_CHUNK_INPUT",
            "Normalized and structured document identity/version must agree",
            false,
        ));
    }
    if normalized.pages.len() != structured.pages.len() {
        return Err(chunk_failure(
            "INVALID_CHUNK_INPUT",
            "Structured page count does not match normalized input",
            false,
        ));
    }

    let mut blocks = HashMap::new();
    let mut canonical_block_ids = Vec::new();
    for (index, page) in normalized.pages.iter().enumerate() {
        let expected_page = u32::try_from(index + 1).map_err(|_| {
            chunk_failure(
                "INVALID_CHUNK_INPUT",
                "Normalized page count exceeds the supported range",
                false,
            )
        })?;
        let structured_page = &structured.pages[index];
        if page.page_number != expected_page
            || structured_page.page_number != page.page_number
            || structured_page.warnings != page.warnings
            || structured_page.requires_visual_processing != page.requires_visual_processing
        {
            return Err(chunk_failure(
                "INVALID_CHUNK_INPUT",
                "Structured page metadata does not preserve normalized input",
                false,
            ));
        }
        for block in &page.content {
            if block.block_id.trim().is_empty()
                || block.text.trim().is_empty()
                || block.source.page_start != page.page_number
                || block.source.page_end != page.page_number
                || blocks.insert(block.block_id.as_str(), block).is_some()
            {
                return Err(chunk_failure(
                    "INVALID_CHUNK_INPUT",
                    "Normalized blocks must be unique, non-empty, and page-local",
                    false,
                ));
            }
            canonical_block_ids.push(block.block_id.as_str());
        }
    }

    if structured.nodes.len() != 1 {
        return Err(chunk_failure(
            "INVALID_STRUCTURED_DOCUMENT",
            "Structured document must contain exactly one root",
            false,
        ));
    }
    let root = &structured.nodes[0];
    if root.kind != crate::pipeline::contracts::StructureNodeKind::Document
        || root.level != 0
        || root.title.is_some()
        || !root.block_ids.is_empty()
        || !root.source_spans.is_empty()
    {
        return Err(chunk_failure(
            "INVALID_STRUCTURED_DOCUMENT",
            "Structured root metadata is invalid",
            false,
        ));
    }

    let mut groups = Vec::new();
    let mut structured_order = Vec::new();
    let mut node_ids = HashSet::new();
    if !node_ids.insert(root.node_id.as_str()) {
        return Err(chunk_failure(
            "INVALID_STRUCTURED_DOCUMENT",
            "Structure node IDs must be unique",
            false,
        ));
    }
    for child in &root.children {
        let mut group_ids = Vec::new();
        collect_group_blocks(child, &mut group_ids, &mut node_ids)?;
        validate_node_provenance(child, &blocks)?;
        structured_order.extend(group_ids.iter().copied());
        groups.push(StructureGroup {
            node_id: child.node_id.as_str(),
            block_ids: group_ids,
        });
    }

    if structured_order != canonical_block_ids {
        return Err(chunk_failure(
            "INVALID_STRUCTURED_DOCUMENT",
            "Structured block coverage must equal normalized source order exactly once",
            false,
        ));
    }

    Ok(ChunkInputs {
        blocks,
        canonical_block_ids,
        groups,
    })
}

fn validate_node_provenance(
    node: &StructureNode,
    blocks: &HashMap<&str, &NormalizedBlock>,
) -> Result<(), PipelineFailure> {
    for (block_id, source_span) in node.block_ids.iter().zip(&node.source_spans) {
        let block = blocks.get(block_id.as_str()).ok_or_else(|| {
            chunk_failure(
                "INVALID_STRUCTURED_DOCUMENT",
                "Structure node references an unknown normalized block",
                false,
            )
        })?;
        if block.source != *source_span {
            return Err(chunk_failure(
                "INVALID_STRUCTURED_DOCUMENT",
                "Structure node provenance does not match its normalized block",
                false,
            ));
        }
    }
    for child in &node.children {
        validate_node_provenance(child, blocks)?;
    }
    Ok(())
}

fn collect_group_blocks<'a>(
    node: &'a StructureNode,
    output: &mut Vec<&'a str>,
    node_ids: &mut HashSet<&'a str>,
) -> Result<(), PipelineFailure> {
    if node.node_id.trim().is_empty() || !node_ids.insert(node.node_id.as_str()) {
        return Err(chunk_failure(
            "INVALID_STRUCTURED_DOCUMENT",
            "Structure node IDs must be present and unique",
            false,
        ));
    }
    if node.block_ids.len() != node.source_spans.len() {
        return Err(chunk_failure(
            "INVALID_STRUCTURED_DOCUMENT",
            "Structure block and source-span counts must agree",
            false,
        ));
    }
    output.extend(node.block_ids.iter().map(String::as_str));
    for child in &node.children {
        collect_group_blocks(child, output, node_ids)?;
    }
    Ok(())
}

fn materialize_chunk(
    normalized: &NormalizedDocument,
    chunking_version: &str,
    structure_node_id: &str,
    blocks: &[&NormalizedBlock],
    zero_based_ordinal: usize,
) -> Result<DocumentChunk, PipelineFailure> {
    let ordinal = u32::try_from(zero_based_ordinal + 1).map_err(|_| {
        chunk_failure(
            "CHUNK_COUNT_EXCEEDED",
            "Chunk count exceeds the supported range",
            false,
        )
    })?;
    let text = blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let block_ids = blocks
        .iter()
        .map(|block| block.block_id.clone())
        .collect::<Vec<_>>();
    let source_spans = blocks
        .iter()
        .map(|block| block.source.clone())
        .collect::<Vec<_>>();
    let warnings = if text.chars().count() > TARGET_CHUNK_CHARACTERS {
        vec![PipelineWarning {
            code: "CHUNK_EXCEEDS_TARGET".to_string(),
            message:
                "A single normalized block exceeds the target chunk size and was preserved intact"
                    .to_string(),
            stage: Some(PipelineStage::Chunk),
        }]
    } else {
        Vec::new()
    };
    let chunk_id = deterministic_chunk_id(
        &normalized.document_id,
        chunking_version,
        ordinal,
        structure_node_id,
        &block_ids,
        &text,
    );
    Ok(DocumentChunk {
        chunk_id,
        ordinal,
        structure_node_id: structure_node_id.to_string(),
        text,
        block_ids,
        source_spans,
        warnings,
    })
}

fn deterministic_chunk_id(
    document_id: &str,
    chunking_version: &str,
    ordinal: u32,
    structure_node_id: &str,
    block_ids: &[String],
    text: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"document-chunk\0");
    hasher.update(document_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(chunking_version.as_bytes());
    hasher.update(b"\0");
    hasher.update(ordinal.to_be_bytes());
    hasher.update(structure_node_id.as_bytes());
    for block_id in block_ids {
        hasher.update(b"\0");
        hasher.update(block_id.as_bytes());
    }
    hasher.update(b"\0");
    hasher.update(text.as_bytes());
    format!("ch-{:x}", hasher.finalize())
}

fn validate_chunked_document(
    chunked: &ChunkedDocument,
    normalized: &NormalizedDocument,
    structured: &StructuredDocument,
    expected_version: &str,
) -> Result<(), PipelineFailure> {
    let inputs = validate_inputs(normalized, structured)?;
    if chunked.document_id != normalized.document_id
        || chunked.chunking_version != expected_version
        || chunked.chunking_version.trim().is_empty()
    {
        return Err(chunk_failure(
            "INVALID_CHUNKED_DOCUMENT",
            "Chunked document identity/version does not match its input",
            false,
        ));
    }

    let group_blocks = inputs
        .groups
        .iter()
        .map(|group| (group.node_id, group.block_ids.as_slice()))
        .collect::<HashMap<_, _>>();
    let mut covered = Vec::<&str>::new();
    let mut chunk_ids = HashSet::new();
    for (index, chunk) in chunked.chunks.iter().enumerate() {
        let expected_ordinal = u32::try_from(index + 1).map_err(|_| {
            chunk_failure(
                "INVALID_CHUNKED_DOCUMENT",
                "Chunk count exceeds the supported range",
                false,
            )
        })?;
        if chunk.ordinal != expected_ordinal
            || chunk.chunk_id.trim().is_empty()
            || !chunk_ids.insert(chunk.chunk_id.as_str())
            || chunk.text.is_empty()
            || chunk.block_ids.is_empty()
            || chunk.block_ids.len() != chunk.source_spans.len()
        {
            return Err(chunk_failure(
                "INVALID_CHUNKED_DOCUMENT",
                "Chunks must have deterministic unique identities, canonical order, and source blocks",
                false,
            ));
        }
        let Some(allowed_group) = group_blocks.get(chunk.structure_node_id.as_str()) else {
            return Err(chunk_failure(
                "INVALID_CHUNKED_DOCUMENT",
                "Chunk references an unknown top-level structure node",
                false,
            ));
        };
        let mut expected_text = Vec::new();
        for (position, block_id) in chunk.block_ids.iter().enumerate() {
            if !allowed_group.contains(&block_id.as_str()) {
                return Err(chunk_failure(
                    "INVALID_CHUNKED_DOCUMENT",
                    "Chunk crosses an unsupported structure boundary",
                    false,
                ));
            }
            let block = inputs.blocks.get(block_id.as_str()).ok_or_else(|| {
                chunk_failure(
                    "INVALID_CHUNKED_DOCUMENT",
                    "Chunk references an unknown normalized block",
                    false,
                )
            })?;
            if chunk.source_spans[position] != block.source {
                return Err(chunk_failure(
                    "INVALID_CHUNKED_DOCUMENT",
                    "Chunk source provenance does not match its normalized block",
                    false,
                ));
            }
            expected_text.push(block.text.as_str());
            covered.push(block_id.as_str());
        }
        let expected_text = expected_text.join("\n\n");
        let expected_id = deterministic_chunk_id(
            &chunked.document_id,
            expected_version,
            chunk.ordinal,
            &chunk.structure_node_id,
            &chunk.block_ids,
            &expected_text,
        );
        if chunk.text != expected_text || chunk.chunk_id != expected_id {
            return Err(chunk_failure(
                "INVALID_CHUNKED_DOCUMENT",
                "Chunk text or identity does not match its authoritative normalized blocks",
                false,
            ));
        }
    }

    if covered != inputs.canonical_block_ids {
        return Err(chunk_failure(
            "INVALID_CHUNKED_DOCUMENT",
            "Chunk coverage must equal normalized blocks exactly once in source order",
            false,
        ));
    }
    Ok(())
}

fn chunk_failure(
    code: impl Into<String>,
    message: impl Into<String>,
    recoverable: bool,
) -> PipelineFailure {
    PipelineFailure {
        code: code.into(),
        message: message.into(),
        stage: Some(PipelineStage::Chunk),
        recoverable,
    }
}

#[derive(Debug, Error)]
pub enum ChunkPipelineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("Chunking failed: {0}")]
    ChunkerFailed(PipelineFailure),
    #[error("Chunked artifact persistence failed: {0}")]
    ArtifactPersistence(StoreError),
    #[error("{primary}; the failure state could not be persisted: {persistence}")]
    FailurePersistence {
        primary: String,
        #[source]
        persistence: StoreError,
    },
}

impl ChunkPipelineError {
    pub fn code(&self) -> &str {
        match self {
            Self::Store(_) => "PIPELINE_STORE_ERROR",
            Self::ChunkerFailed(failure) => &failure.code,
            Self::ArtifactPersistence(_) => "CHUNKED_ARTIFACT_PERSISTENCE_FAILED",
            Self::FailurePersistence { .. } => "CHUNK_FAILURE_PERSISTENCE_FAILED",
        }
    }
}

pub fn chunk_document(
    conn: &mut Connection,
    chunker: &dyn DocumentChunker,
    run_id: &str,
) -> Result<ChunkedDocument, ChunkPipelineError> {
    let run = db::get_pipeline_run(conn, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    let (chunking_run, normalized, structured) =
        db::start_chunking(conn, run_id, run.state_version)?;

    if let Err(failure) = validate_inputs(&normalized, &structured) {
        return Err(persist_chunk_failure(
            conn,
            run_id,
            chunking_run.state_version,
            failure,
        ));
    }
    let chunked = match chunker.chunk(&normalized, &structured) {
        Ok(chunked) => chunked,
        Err(failure) => {
            return Err(persist_chunk_failure(
                conn,
                run_id,
                chunking_run.state_version,
                failure,
            ));
        }
    };
    if let Err(failure) =
        validate_chunked_document(&chunked, &normalized, &structured, chunker.version())
    {
        return Err(persist_chunk_failure(
            conn,
            run_id,
            chunking_run.state_version,
            failure,
        ));
    }

    let warnings = chunked
        .warnings
        .iter()
        .chain(
            chunked
                .chunks
                .iter()
                .flat_map(|chunk| chunk.warnings.iter()),
        )
        .cloned()
        .collect::<Vec<_>>();
    if let Err(persistence) =
        db::complete_chunking(conn, run_id, chunking_run.state_version, &chunked, warnings)
    {
        let failure = chunk_failure(
            "CHUNKED_ARTIFACT_PERSISTENCE_FAILED",
            "Chunked output could not be committed atomically",
            true,
        );
        return match db::fail_chunking(conn, run_id, chunking_run.state_version, failure) {
            Ok(_) => Err(ChunkPipelineError::ArtifactPersistence(persistence)),
            Err(failure_persistence) => Err(ChunkPipelineError::FailurePersistence {
                primary: persistence.to_string(),
                persistence: failure_persistence,
            }),
        };
    }
    Ok(chunked)
}

fn persist_chunk_failure(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    failure: PipelineFailure,
) -> ChunkPipelineError {
    match db::fail_chunking(conn, run_id, expected_version, failure.clone()) {
        Ok(_) => ChunkPipelineError::ChunkerFailed(failure),
        Err(persistence) => ChunkPipelineError::FailurePersistence {
            primary: failure.to_string(),
            persistence,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contracts::{
        NormalizedBlockKind, NormalizedPage, PipelineState, SourceSpan, SourceType,
        StructureNodeKind, StructurePage,
    };
    use crate::pipeline::db::{
        get_chunked_document, get_pipeline_run, init_db, list_pipeline_events,
    };
    use crate::pipeline::ingest::ingest_pdf;
    use crate::pipeline::normalize::{normalize_document, CanonicalNormalizer};
    use crate::pipeline::parser::{parse_document, PdfExtractParser};
    use crate::pipeline::structure::{structure_document, DeterministicStructureInterpreter};
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestDatabase(PathBuf);

    impl TestDatabase {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("doc-sum-chunk-{}.db", Uuid::new_v4())))
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn structured_run(database: &TestDatabase) -> (Connection, String) {
        let mut conn = init_db(&database.0).expect("test database should initialize");
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let (_, ingested_run) = ingest_pdf(
            &mut conn,
            source.to_str().expect("fixture path should be UTF-8"),
        )
        .expect("fixture should ingest");
        parse_document(&mut conn, &PdfExtractParser::new(), &ingested_run.run_id)
            .expect("fixture should parse");
        normalize_document(&mut conn, &CanonicalNormalizer::new(), &ingested_run.run_id)
            .expect("fixture should normalize");
        structure_document(
            &mut conn,
            &DeterministicStructureInterpreter::new(),
            &ingested_run.run_id,
        )
        .expect("fixture should structure");
        (conn, ingested_run.run_id)
    }

    fn span(page: u32) -> SourceSpan {
        SourceSpan {
            page_start: page,
            page_end: page,
            section_id: None,
            source_type: SourceType::NativeText,
        }
    }

    fn block(id: &str, page: u32, text: &str) -> NormalizedBlock {
        NormalizedBlock {
            block_id: id.to_string(),
            kind: NormalizedBlockKind::Text,
            text: text.to_string(),
            source: span(page),
        }
    }

    fn fixture() -> (NormalizedDocument, StructuredDocument) {
        let first = block("b1", 1, "1. Introduction\n\nSource facts remain exact.");
        let second = block("b2", 2, "Continuation on page two.");
        let third = block("b3", 3, "2. Findings\n\nRevenue was $1,247,392.17.");
        let normalized = NormalizedDocument {
            document_id: "document-1".to_string(),
            normalization_version: "1.0.0".to_string(),
            pages: vec![
                NormalizedPage {
                    page_number: 1,
                    content: vec![first.clone()],
                    warnings: vec![],
                    requires_visual_processing: false,
                },
                NormalizedPage {
                    page_number: 2,
                    content: vec![second.clone()],
                    warnings: vec![],
                    requires_visual_processing: false,
                },
                NormalizedPage {
                    page_number: 3,
                    content: vec![third.clone()],
                    warnings: vec![],
                    requires_visual_processing: false,
                },
            ],
            warnings: vec![],
        };
        let section = |node_id: &str, blocks: Vec<&NormalizedBlock>| StructureNode {
            node_id: node_id.to_string(),
            kind: StructureNodeKind::Section,
            title: None,
            level: 1,
            block_ids: blocks.iter().map(|value| value.block_id.clone()).collect(),
            source_spans: blocks.iter().map(|value| value.source.clone()).collect(),
            children: vec![],
        };
        let structured = StructuredDocument {
            document_id: normalized.document_id.clone(),
            structure_version: "1.0.0".to_string(),
            pages: normalized
                .pages
                .iter()
                .map(|page| StructurePage {
                    page_number: page.page_number,
                    warnings: page.warnings.clone(),
                    requires_visual_processing: page.requires_visual_processing,
                })
                .collect(),
            nodes: vec![StructureNode {
                node_id: "root".to_string(),
                kind: StructureNodeKind::Document,
                title: None,
                level: 0,
                block_ids: vec![],
                source_spans: vec![],
                children: vec![
                    section("section-1", vec![&first, &second]),
                    section("section-2", vec![&third]),
                ],
            }],
            warnings: vec![],
        };
        (normalized, structured)
    }

    #[test]
    fn chunks_respect_top_level_structure_and_preserve_provenance() {
        let (normalized, structured) = fixture();
        let chunked = DeterministicDocumentChunker::new()
            .chunk(&normalized, &structured)
            .expect("fixture should chunk");
        assert_eq!(chunked.chunks.len(), 2);
        assert_eq!(chunked.chunks[0].block_ids, vec!["b1", "b2"]);
        assert_eq!(chunked.chunks[0].source_spans, vec![span(1), span(2)]);
        assert_eq!(chunked.chunks[1].block_ids, vec!["b3"]);
        assert_eq!(
            chunked.chunks[1].text,
            "2. Findings\n\nRevenue was $1,247,392.17."
        );
    }

    #[test]
    fn output_is_deterministic() {
        let (normalized, structured) = fixture();
        let chunker = DeterministicDocumentChunker::new();
        assert_eq!(
            chunker.chunk(&normalized, &structured),
            chunker.chunk(&normalized, &structured)
        );
    }

    #[test]
    fn invalid_or_duplicate_coverage_is_rejected() {
        let (normalized, mut structured) = fixture();
        structured.nodes[0].children[0]
            .block_ids
            .push("b1".to_string());
        structured.nodes[0].children[0].source_spans.push(span(1));
        let error = DeterministicDocumentChunker::new()
            .chunk(&normalized, &structured)
            .expect_err("duplicate coverage must fail");
        assert_eq!(error.code, "INVALID_STRUCTURED_DOCUMENT");

        let (normalized, mut wrong_provenance) = fixture();
        wrong_provenance.nodes[0].children[0].source_spans[0] = span(2);
        let error = DeterministicDocumentChunker::new()
            .chunk(&normalized, &wrong_provenance)
            .expect_err("wrong structure provenance must fail");
        assert_eq!(error.code, "INVALID_STRUCTURED_DOCUMENT");
    }

    #[test]
    fn empty_visual_document_succeeds_without_inventing_chunks() {
        let normalized = NormalizedDocument {
            document_id: "visual".to_string(),
            normalization_version: "1.0.0".to_string(),
            pages: vec![NormalizedPage {
                page_number: 1,
                content: vec![],
                warnings: vec![PipelineWarning {
                    code: "NO_NATIVE_TEXT".to_string(),
                    message: "No native text".to_string(),
                    stage: Some(PipelineStage::Normalize),
                }],
                requires_visual_processing: true,
            }],
            warnings: vec![],
        };
        let structured = StructuredDocument {
            document_id: "visual".to_string(),
            structure_version: "1.0.0".to_string(),
            pages: vec![StructurePage {
                page_number: 1,
                warnings: normalized.pages[0].warnings.clone(),
                requires_visual_processing: true,
            }],
            nodes: vec![StructureNode {
                node_id: "root".to_string(),
                kind: StructureNodeKind::Document,
                title: None,
                level: 0,
                block_ids: vec![],
                source_spans: vec![],
                children: vec![],
            }],
            warnings: vec![],
        };
        let chunked = DeterministicDocumentChunker::new()
            .chunk(&normalized, &structured)
            .expect("visual document should still chunk successfully");
        assert!(chunked.chunks.is_empty());
        assert!(chunked
            .warnings
            .iter()
            .any(|warning| warning.code == "NO_TEXT_TO_CHUNK"));
        assert!(chunked
            .warnings
            .iter()
            .any(|warning| warning.code == "NO_NATIVE_TEXT"));
    }

    #[test]
    fn lifecycle_persists_artifact_state_version_and_event_atomically() {
        let database = TestDatabase::new();
        let (mut conn, run_id) = structured_run(&database);
        let before = get_pipeline_run(&conn, &run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(before.state, PipelineState::Structured);

        let chunked = chunk_document(&mut conn, &DeterministicDocumentChunker::new(), &run_id)
            .expect("structured fixture should chunk");
        let after = get_pipeline_run(&conn, &run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(after.state, PipelineState::Chunked);
        assert_eq!(after.state_version, before.state_version + 2);
        assert_eq!(
            get_chunked_document(&conn, &run_id)
                .expect("artifact should load")
                .expect("artifact should exist"),
            chunked
        );
        let events = list_pipeline_events(&conn, &run_id).expect("events should load");
        assert_eq!(events[events.len() - 2].next_state, PipelineState::Chunking);
        assert_eq!(events[events.len() - 1].next_state, PipelineState::Chunked);
    }

    #[test]
    fn chunked_artifact_survives_independent_database_reopen() {
        let database = TestDatabase::new();
        let (mut conn, run_id) = structured_run(&database);
        let expected = chunk_document(&mut conn, &DeterministicDocumentChunker::new(), &run_id)
            .expect("fixture should chunk");
        drop(conn);

        let reopened = init_db(&database.0).expect("database should reopen");
        let actual = get_chunked_document(&reopened, &run_id)
            .expect("artifact should load after reopen")
            .expect("artifact should remain after reopen");
        assert_eq!(actual, expected);
        let run = get_pipeline_run(&reopened, &run_id)
            .expect("run should load after reopen")
            .expect("run should remain after reopen");
        assert_eq!(run.state, PipelineState::Chunked);
    }

    #[test]
    fn artifact_insert_failure_never_commits_chunked_state_or_event() {
        let database = TestDatabase::new();
        let (mut conn, run_id) = structured_run(&database);
        conn.execute_batch(
            "CREATE TRIGGER fail_chunk_artifact
             BEFORE INSERT ON chunked_documents
             BEGIN SELECT RAISE(ABORT, 'injected chunk artifact failure'); END;",
        )
        .expect("failure trigger should install");

        let result = chunk_document(&mut conn, &DeterministicDocumentChunker::new(), &run_id);
        assert!(matches!(
            result,
            Err(ChunkPipelineError::ArtifactPersistence(_))
        ));
        assert!(get_chunked_document(&conn, &run_id)
            .expect("artifact query should succeed")
            .is_none());
        let run = get_pipeline_run(&conn, &run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(run.state, PipelineState::Failed);
        let events = list_pipeline_events(&conn, &run_id).expect("events should load");
        assert!(!events
            .iter()
            .any(|event| event.next_state == PipelineState::Chunked));
    }

    struct InvalidChunker;

    impl DocumentChunker for InvalidChunker {
        fn chunk(
            &self,
            normalized: &NormalizedDocument,
            _structured: &StructuredDocument,
        ) -> Result<ChunkedDocument, PipelineFailure> {
            Ok(ChunkedDocument {
                document_id: normalized.document_id.clone(),
                chunking_version: self.version().to_string(),
                chunks: vec![],
                warnings: vec![],
            })
        }

        fn version(&self) -> &'static str {
            CHUNKING_VERSION
        }
    }

    #[test]
    fn invalid_chunker_output_transitions_to_failed_without_artifact() {
        let database = TestDatabase::new();
        let (mut conn, run_id) = structured_run(&database);
        let result = chunk_document(&mut conn, &InvalidChunker, &run_id);
        assert!(matches!(result, Err(ChunkPipelineError::ChunkerFailed(_))));
        let run = get_pipeline_run(&conn, &run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(run.state, PipelineState::Failed);
        assert!(get_chunked_document(&conn, &run_id)
            .expect("artifact query should succeed")
            .is_none());
    }
}
