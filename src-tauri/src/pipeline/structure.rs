use crate::pipeline::contracts::{
    NormalizedBlock, NormalizedDocument, PipelineFailure, PipelineStage, PipelineWarning,
    SourceSpan, StructureInterpreter, StructureNode, StructureNodeKind, StructurePage,
    StructuredDocument,
};
use crate::pipeline::db::{self, StoreError};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

pub const STRUCTURE_VERSION: &str = "1.0.0";
const MAX_HEADING_DEPTH: usize = 6;
const MAX_HEADING_LINE_CHARS: usize = 120;
const MAX_HEADING_TITLE_WORDS: usize = 12;
const MAX_HEADING_COMPONENT: u32 = 999;

#[derive(Default)]
pub struct DeterministicStructureInterpreter;

impl DeterministicStructureInterpreter {
    pub fn new() -> Self {
        Self
    }
}

impl StructureInterpreter for DeterministicStructureInterpreter {
    fn interpret(
        &self,
        normalized: &NormalizedDocument,
    ) -> Result<StructuredDocument, PipelineFailure> {
        validate_normalized_input(normalized)?;

        let mut arena = vec![DraftNode::document()];
        let mut heading_stack = Vec::<ActiveHeading>::new();
        let mut seen_numbering = HashSet::<Vec<u32>>::new();

        for page in &normalized.pages {
            for block in &page.content {
                let signal = detect_numbered_heading(&block.text);
                let placement = signal.as_ref().and_then(|heading| {
                    heading_parent(&heading.numbering, &heading_stack, &seen_numbering)
                });

                if let (Some(heading), Some((parent_id, retained_depth))) = (signal, placement) {
                    heading_stack.truncate(retained_depth);
                    let level = u32::try_from(heading.numbering.len()).map_err(|_| {
                        structure_failure(
                            "INVALID_NORMALIZED_DOCUMENT",
                            "Heading depth exceeds the supported structural range",
                            false,
                        )
                    })?;
                    let kind = if level == 1 {
                        StructureNodeKind::Section
                    } else {
                        StructureNodeKind::Subsection
                    };
                    let node_id = arena.len();
                    arena.push(DraftNode::with_block(kind, heading.title, level, block));
                    arena[parent_id].children.push(node_id);
                    seen_numbering.insert(heading.numbering.clone());
                    heading_stack.push(ActiveHeading {
                        numbering: heading.numbering,
                        node_id,
                    });
                } else {
                    append_content_block(&mut arena, &heading_stack, block);
                }
            }
        }

        let root = materialize_node(&arena, 0, &[0], &normalized.document_id, self.version());
        let structured = StructuredDocument {
            document_id: normalized.document_id.clone(),
            structure_version: self.version().to_string(),
            pages: normalized
                .pages
                .iter()
                .map(|page| StructurePage {
                    page_number: page.page_number,
                    warnings: page.warnings.clone(),
                    requires_visual_processing: page.requires_visual_processing,
                })
                .collect(),
            nodes: vec![root],
            warnings: normalized.warnings.clone(),
        };
        validate_structured_document(&structured, normalized, self.version())?;
        Ok(structured)
    }

    fn version(&self) -> &'static str {
        STRUCTURE_VERSION
    }
}

#[derive(Debug)]
struct HeadingSignal {
    numbering: Vec<u32>,
    title: Option<String>,
}

#[derive(Debug)]
struct ActiveHeading {
    numbering: Vec<u32>,
    node_id: usize,
}

#[derive(Debug)]
struct DraftNode {
    kind: StructureNodeKind,
    title: Option<String>,
    level: u32,
    block_ids: Vec<String>,
    source_spans: Vec<SourceSpan>,
    children: Vec<usize>,
}

impl DraftNode {
    fn document() -> Self {
        Self {
            kind: StructureNodeKind::Document,
            title: None,
            level: 0,
            block_ids: Vec::new(),
            source_spans: Vec::new(),
            children: Vec::new(),
        }
    }

    fn with_block(
        kind: StructureNodeKind,
        title: Option<String>,
        level: u32,
        block: &NormalizedBlock,
    ) -> Self {
        Self {
            kind,
            title,
            level,
            block_ids: vec![block.block_id.clone()],
            source_spans: vec![block.source.clone()],
            children: Vec::new(),
        }
    }

    fn push_block(&mut self, block: &NormalizedBlock) {
        self.block_ids.push(block.block_id.clone());
        self.source_spans.push(block.source.clone());
    }
}

fn heading_parent(
    numbering: &[u32],
    active: &[ActiveHeading],
    seen: &HashSet<Vec<u32>>,
) -> Option<(usize, usize)> {
    if numbering.is_empty() || numbering.len() > MAX_HEADING_DEPTH || seen.contains(numbering) {
        return None;
    }
    if numbering.len() == 1 {
        return Some((0, 0));
    }

    let parent_numbering = &numbering[..numbering.len() - 1];
    active
        .iter()
        .rposition(|heading| heading.numbering == parent_numbering)
        .map(|position| (active[position].node_id, position + 1))
}

fn append_content_block(
    arena: &mut Vec<DraftNode>,
    heading_stack: &[ActiveHeading],
    block: &NormalizedBlock,
) {
    if let Some(active) = heading_stack.last() {
        arena[active.node_id].push_block(block);
        return;
    }

    let existing_unstructured = arena[0]
        .children
        .last()
        .copied()
        .filter(|node_id| arena[*node_id].kind == StructureNodeKind::Unstructured);
    if let Some(node_id) = existing_unstructured {
        arena[node_id].push_block(block);
    } else {
        let node_id = arena.len();
        arena.push(DraftNode::with_block(
            StructureNodeKind::Unstructured,
            None,
            1,
            block,
        ));
        arena[0].children.push(node_id);
    }
}

fn detect_numbered_heading(text: &str) -> Option<HeadingSignal> {
    let line = text.lines().find(|line| !line.trim().is_empty())?.trim();
    if line.chars().count() > MAX_HEADING_LINE_CHARS {
        return None;
    }

    let bytes = line.as_bytes();
    let mut cursor = 0;
    let mut numbering = Vec::new();
    let title_start;

    loop {
        let number_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == number_start {
            return None;
        }
        let component = line[number_start..cursor].parse::<u32>().ok()?;
        if component == 0 || component > MAX_HEADING_COMPONENT {
            return None;
        }
        numbering.push(component);
        if numbering.len() > MAX_HEADING_DEPTH {
            return None;
        }

        if cursor == bytes.len() {
            title_start = cursor;
            break;
        }

        match bytes[cursor] {
            b'.' if cursor + 1 < bytes.len() && bytes[cursor + 1].is_ascii_digit() => {
                cursor += 1;
            }
            b'.' => {
                cursor += 1;
                title_start = skip_ascii_whitespace(bytes, cursor);
                break;
            }
            b')' if numbering.len() == 1 => {
                cursor += 1;
                title_start = skip_ascii_whitespace(bytes, cursor);
                break;
            }
            byte if byte.is_ascii_whitespace() => {
                title_start = skip_ascii_whitespace(bytes, cursor);
                break;
            }
            _ => return None,
        }
    }

    let title = line.get(title_start..)?.trim();
    if title.is_empty() {
        return Some(HeadingSignal {
            numbering,
            title: None,
        });
    }
    if !is_conservative_heading_title(title) {
        return None;
    }

    Some(HeadingSignal {
        numbering,
        title: Some(title.to_string()),
    })
}

fn skip_ascii_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

fn is_conservative_heading_title(title: &str) -> bool {
    let Some(first) = title.chars().next() else {
        return false;
    };
    first.is_uppercase()
        && title.split_whitespace().count() <= MAX_HEADING_TITLE_WORDS
        && !matches!(title.chars().last(), Some('.' | '!' | '?' | ';'))
}

fn materialize_node(
    arena: &[DraftNode],
    node_id: usize,
    path: &[u32],
    document_id: &str,
    structure_version: &str,
) -> StructureNode {
    let draft = &arena[node_id];
    let children = draft
        .children
        .iter()
        .enumerate()
        .map(|(child_index, child_id)| {
            let mut child_path = path.to_vec();
            child_path.push(u32::try_from(child_index).unwrap_or(u32::MAX));
            materialize_node(
                arena,
                *child_id,
                &child_path,
                document_id,
                structure_version,
            )
        })
        .collect();
    StructureNode {
        node_id: deterministic_node_id(
            document_id,
            structure_version,
            path,
            &draft.kind,
            draft.title.as_deref(),
            draft.level,
            &draft.block_ids,
        ),
        kind: draft.kind.clone(),
        title: draft.title.clone(),
        level: draft.level,
        block_ids: draft.block_ids.clone(),
        source_spans: draft.source_spans.clone(),
        children,
    }
}

fn deterministic_node_id(
    document_id: &str,
    structure_version: &str,
    path: &[u32],
    kind: &StructureNodeKind,
    title: Option<&str>,
    level: u32,
    block_ids: &[String],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"structure-node\0");
    hasher.update(document_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(structure_version.as_bytes());
    hasher.update(b"\0");
    hasher.update(node_kind_tag(kind));
    hasher.update(level.to_be_bytes());
    for component in path {
        hasher.update(component.to_be_bytes());
    }
    hasher.update(b"\0");
    if let Some(title) = title {
        hasher.update(title.as_bytes());
    }
    for block_id in block_ids {
        hasher.update(b"\0");
        hasher.update(block_id.as_bytes());
    }
    format!("sn-{:x}", hasher.finalize())
}

fn node_kind_tag(kind: &StructureNodeKind) -> &'static [u8] {
    match kind {
        StructureNodeKind::Document => b"document",
        StructureNodeKind::Section => b"section",
        StructureNodeKind::Subsection => b"subsection",
        StructureNodeKind::Unstructured => b"unstructured",
    }
}

fn validate_normalized_input(normalized: &NormalizedDocument) -> Result<(), PipelineFailure> {
    if normalized.document_id.trim().is_empty()
        || normalized.normalization_version.trim().is_empty()
    {
        return Err(structure_failure(
            "INVALID_NORMALIZED_DOCUMENT",
            "Normalized document identity and version must be present",
            false,
        ));
    }

    let mut block_ids = HashSet::new();
    for (page_index, page) in normalized.pages.iter().enumerate() {
        let expected_page = u32::try_from(page_index + 1).map_err(|_| {
            structure_failure(
                "INVALID_NORMALIZED_DOCUMENT",
                "Normalized page count exceeds the supported range",
                false,
            )
        })?;
        if page.page_number != expected_page {
            return Err(structure_failure(
                "INVALID_NORMALIZED_DOCUMENT",
                "Normalized pages are not in canonical one-based order",
                false,
            ));
        }
        if page.content.is_empty()
            && (!page.requires_visual_processing
                || !page
                    .warnings
                    .iter()
                    .any(|warning| warning.code == "NO_NATIVE_TEXT"))
        {
            return Err(structure_failure(
                "INVALID_NORMALIZED_DOCUMENT",
                format!(
                    "Empty normalized page {} lacks visual-routing metadata",
                    page.page_number
                ),
                false,
            ));
        }
        for block in &page.content {
            if block.block_id.trim().is_empty()
                || !block_ids.insert(block.block_id.clone())
                || block.text.trim().is_empty()
                || block.source.page_start != page.page_number
                || block.source.page_end != page.page_number
            {
                return Err(structure_failure(
                    "INVALID_NORMALIZED_DOCUMENT",
                    format!(
                        "Normalized block on page {} has invalid identity, text, or provenance",
                        page.page_number
                    ),
                    false,
                ));
            }
        }
    }
    Ok(())
}

fn validate_structured_document(
    structured: &StructuredDocument,
    normalized: &NormalizedDocument,
    expected_version: &str,
) -> Result<(), PipelineFailure> {
    validate_normalized_input(normalized)?;
    if structured.document_id != normalized.document_id {
        return Err(invalid_structured(
            "Structured document identity does not match normalized input",
        ));
    }
    if structured.structure_version.trim().is_empty()
        || structured.structure_version != expected_version
    {
        return Err(invalid_structured(
            "Structure version is missing or does not match the interpreter",
        ));
    }
    if structured.warnings != normalized.warnings {
        return Err(invalid_structured(
            "Structured output did not preserve document warnings",
        ));
    }
    if structured.pages.len() != normalized.pages.len()
        || !structured.pages.iter().zip(&normalized.pages).all(
            |(structured_page, normalized_page)| {
                structured_page.page_number == normalized_page.page_number
                    && structured_page.warnings == normalized_page.warnings
                    && structured_page.requires_visual_processing
                        == normalized_page.requires_visual_processing
            },
        )
    {
        return Err(invalid_structured(
            "Structured output did not preserve page order, warnings, or visual routing",
        ));
    }
    if structured.nodes.len() != 1 {
        return Err(invalid_structured(
            "Structured output must contain exactly one document root",
        ));
    }

    let expected_blocks = normalized
        .pages
        .iter()
        .flat_map(|page| page.content.iter())
        .map(|block| (block.block_id.clone(), block.source.clone()))
        .collect::<Vec<_>>();
    let block_lookup = normalized
        .pages
        .iter()
        .flat_map(|page| page.content.iter())
        .map(|block| (block.block_id.clone(), block))
        .collect::<HashMap<_, _>>();
    let mut referenced_blocks = Vec::new();
    let mut node_ids = HashSet::new();
    let mut heading_numbering = HashSet::new();
    validate_node(
        &structured.nodes[0],
        None,
        &[0],
        structured,
        &block_lookup,
        &mut referenced_blocks,
        &mut node_ids,
        &mut heading_numbering,
    )?;

    let expected_ids = expected_blocks
        .iter()
        .map(|(block_id, _)| block_id)
        .collect::<Vec<_>>();
    if referenced_blocks != expected_ids {
        return Err(invalid_structured(
            "Structured block coverage is incomplete, duplicated, foreign, or out of order",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_node<'a>(
    node: &'a StructureNode,
    parent: Option<&StructureNode>,
    path: &[u32],
    structured: &StructuredDocument,
    block_lookup: &HashMap<String, &NormalizedBlock>,
    referenced_blocks: &mut Vec<&'a String>,
    node_ids: &mut HashSet<String>,
    heading_numbering: &mut HashSet<Vec<u32>>,
) -> Result<(), PipelineFailure> {
    validate_node_shape(node, parent)?;
    validate_heading_source(node, parent, block_lookup, heading_numbering)?;
    if node.block_ids.len() != node.source_spans.len() {
        return Err(invalid_structured(
            "Structure node block and provenance counts differ",
        ));
    }
    if node.node_id
        != deterministic_node_id(
            &structured.document_id,
            &structured.structure_version,
            path,
            &node.kind,
            node.title.as_deref(),
            node.level,
            &node.block_ids,
        )
        || !node_ids.insert(node.node_id.clone())
    {
        return Err(invalid_structured(
            "Structure node identity is invalid or duplicated",
        ));
    }

    for (block_id, source_span) in node.block_ids.iter().zip(&node.source_spans) {
        let Some(expected_block) = block_lookup.get(block_id) else {
            return Err(invalid_structured(
                "Structure node references a block outside the normalized document",
            ));
        };
        if source_span != &expected_block.source {
            return Err(invalid_structured(
                "Structure node source span does not match its normalized block",
            ));
        }
        referenced_blocks.push(block_id);
    }

    for (child_index, child) in node.children.iter().enumerate() {
        let child_index = u32::try_from(child_index).map_err(|_| {
            invalid_structured("Structure node fan-out exceeds the supported range")
        })?;
        let mut child_path = path.to_vec();
        child_path.push(child_index);
        validate_node(
            child,
            Some(node),
            &child_path,
            structured,
            block_lookup,
            referenced_blocks,
            node_ids,
            heading_numbering,
        )?;
    }
    Ok(())
}

fn validate_heading_source(
    node: &StructureNode,
    parent: Option<&StructureNode>,
    block_lookup: &HashMap<String, &NormalizedBlock>,
    heading_numbering: &mut HashSet<Vec<u32>>,
) -> Result<(), PipelineFailure> {
    if !matches!(
        node.kind,
        StructureNodeKind::Section | StructureNodeKind::Subsection
    ) {
        return Ok(());
    }

    let signal = heading_signal_for_node(node, block_lookup)?;
    if u32::try_from(signal.numbering.len()).ok() != Some(node.level)
        || signal.title != node.title
        || !heading_numbering.insert(signal.numbering.clone())
    {
        return Err(invalid_structured(
            "Structure heading title, level, or numbering does not match its source block",
        ));
    }

    if node.kind == StructureNodeKind::Subsection {
        let parent = parent.ok_or_else(|| {
            invalid_structured("Structured subsection is missing its heading parent")
        })?;
        let parent_signal = heading_signal_for_node(parent, block_lookup)?;
        if signal.numbering[..signal.numbering.len() - 1] != parent_signal.numbering {
            return Err(invalid_structured(
                "Structured subsection numbering does not match its parent prefix",
            ));
        }
    }
    Ok(())
}

fn heading_signal_for_node(
    node: &StructureNode,
    block_lookup: &HashMap<String, &NormalizedBlock>,
) -> Result<HeadingSignal, PipelineFailure> {
    let block_id = node
        .block_ids
        .first()
        .ok_or_else(|| invalid_structured("Structured heading has no source block"))?;
    let block = block_lookup
        .get(block_id)
        .ok_or_else(|| invalid_structured("Structured heading references a foreign block"))?;
    detect_numbered_heading(&block.text)
        .ok_or_else(|| invalid_structured("Structured heading lacks a valid source signal"))
}

fn validate_node_shape(
    node: &StructureNode,
    parent: Option<&StructureNode>,
) -> Result<(), PipelineFailure> {
    match (&node.kind, parent) {
        (StructureNodeKind::Document, None)
            if node.level == 0
                && node.title.is_none()
                && node.block_ids.is_empty()
                && node.source_spans.is_empty() =>
        {
            Ok(())
        }
        (StructureNodeKind::Section, Some(parent))
            if parent.kind == StructureNodeKind::Document
                && node.level == 1
                && !node.block_ids.is_empty() =>
        {
            Ok(())
        }
        (StructureNodeKind::Subsection, Some(parent))
            if matches!(
                parent.kind,
                StructureNodeKind::Section | StructureNodeKind::Subsection
            ) && parent.level.checked_add(1) == Some(node.level)
                && usize::try_from(node.level).is_ok_and(|level| level <= MAX_HEADING_DEPTH)
                && !node.block_ids.is_empty() =>
        {
            Ok(())
        }
        (StructureNodeKind::Unstructured, Some(parent))
            if parent.kind == StructureNodeKind::Document
                && node.level == 1
                && node.title.is_none()
                && !node.block_ids.is_empty() =>
        {
            Ok(())
        }
        _ => Err(invalid_structured(
            "Structure node kind, level, ownership, or parent relationship is invalid",
        )),
    }
}

fn invalid_structured(message: impl Into<String>) -> PipelineFailure {
    structure_failure("INVALID_STRUCTURED_DOCUMENT", message, false)
}

fn structure_failure(
    code: impl Into<String>,
    message: impl Into<String>,
    recoverable: bool,
) -> PipelineFailure {
    PipelineFailure {
        code: code.into(),
        message: message.into(),
        stage: Some(PipelineStage::Structure),
        recoverable,
    }
}

#[derive(Debug, Error)]
pub enum StructurePipelineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("Structural interpretation failed: {0}")]
    InterpreterFailed(PipelineFailure),
    #[error("Structured artifact persistence failed: {0}")]
    ArtifactPersistence(StoreError),
    #[error("{primary}; the failure state could not be persisted: {persistence}")]
    FailurePersistence {
        primary: String,
        #[source]
        persistence: StoreError,
    },
}

impl StructurePipelineError {
    pub fn code(&self) -> &str {
        match self {
            Self::Store(_) => "PIPELINE_STORE_ERROR",
            Self::InterpreterFailed(failure) => &failure.code,
            Self::ArtifactPersistence(_) => "STRUCTURED_ARTIFACT_PERSISTENCE_FAILED",
            Self::FailurePersistence { .. } => "STRUCTURE_FAILURE_PERSISTENCE_FAILED",
        }
    }
}

pub fn structure_document(
    conn: &mut Connection,
    interpreter: &dyn StructureInterpreter,
    run_id: &str,
) -> Result<StructuredDocument, StructurePipelineError> {
    let run = db::get_pipeline_run(conn, run_id)?
        .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
    let (structuring_run, normalized) = db::start_structuring(conn, run_id, run.state_version)?;

    if let Err(failure) = validate_normalized_input(&normalized) {
        return Err(persist_structure_failure(
            conn,
            run_id,
            structuring_run.state_version,
            failure,
        ));
    }

    let structured = match interpreter.interpret(&normalized) {
        Ok(structured) => structured,
        Err(failure) => {
            return Err(persist_structure_failure(
                conn,
                run_id,
                structuring_run.state_version,
                failure,
            ));
        }
    };
    if let Err(failure) =
        validate_structured_document(&structured, &normalized, interpreter.version())
    {
        return Err(persist_structure_failure(
            conn,
            run_id,
            structuring_run.state_version,
            failure,
        ));
    }

    let warnings = structured
        .warnings
        .iter()
        .chain(
            structured
                .pages
                .iter()
                .flat_map(|page| page.warnings.iter()),
        )
        .cloned()
        .collect::<Vec<PipelineWarning>>();
    if let Err(persistence) = db::complete_structuring(
        conn,
        run_id,
        structuring_run.state_version,
        &structured,
        warnings,
    ) {
        let failure = structure_failure(
            "STRUCTURED_ARTIFACT_PERSISTENCE_FAILED",
            "Structured output could not be committed atomically",
            true,
        );
        return match db::fail_structuring(conn, run_id, structuring_run.state_version, failure) {
            Ok(_) => Err(StructurePipelineError::ArtifactPersistence(persistence)),
            Err(failure_persistence) => Err(StructurePipelineError::FailurePersistence {
                primary: persistence.to_string(),
                persistence: failure_persistence,
            }),
        };
    }

    Ok(structured)
}

fn persist_structure_failure(
    conn: &mut Connection,
    run_id: &str,
    expected_version: u32,
    failure: PipelineFailure,
) -> StructurePipelineError {
    match db::fail_structuring(conn, run_id, expected_version, failure.clone()) {
        Ok(_) => StructurePipelineError::InterpreterFailed(failure),
        Err(persistence) => StructurePipelineError::FailurePersistence {
            primary: failure.to_string(),
            persistence,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contracts::{
        DocumentParser, NormalizedBlockKind, NormalizedPage, ParsedDocument, ParsedPage,
        PipelineState, SourceType,
    };
    use crate::pipeline::ingest::ingest_pdf;
    use crate::pipeline::normalize::{
        normalize_document, CanonicalNormalizer, NORMALIZATION_VERSION,
    };
    use crate::pipeline::parser::{parse_document, PdfExtractParser};
    use crate::pipeline::state::TransitionError;
    use rusqlite::params;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestPath(PathBuf);

    impl TestPath {
        fn new(extension: &str) -> Self {
            Self(std::env::temp_dir().join(format!(
                "doc-sum-structure-{}.{}",
                Uuid::new_v4(),
                extension
            )))
        }

        fn write_pdf_candidate() -> Self {
            let path = Self::new("pdf");
            fs::write(&path.0, b"%PDF-1.4\nstructure fixture").expect("fixture should be writable");
            path
        }
    }

    impl Drop for TestPath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    struct FixtureParser {
        pages: Vec<ParsedPage>,
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
                warnings: Vec::new(),
            })
        }

        fn id(&self) -> &'static str {
            "structure-fixture-parser"
        }

        fn version(&self) -> &'static str {
            "test"
        }
    }

    struct MissingCoverageInterpreter;

    impl StructureInterpreter for MissingCoverageInterpreter {
        fn interpret(
            &self,
            normalized: &NormalizedDocument,
        ) -> Result<StructuredDocument, PipelineFailure> {
            let mut structured = DeterministicStructureInterpreter::new().interpret(normalized)?;
            let owner = &mut structured.nodes[0].children[0];
            owner.block_ids.clear();
            owner.source_spans.clear();
            Ok(structured)
        }

        fn version(&self) -> &'static str {
            STRUCTURE_VERSION
        }
    }

    fn parsed_page(page_number: u32, text: &str) -> ParsedPage {
        let requires_visual_processing = text.trim().is_empty();
        ParsedPage {
            page_number,
            text: text.to_string(),
            warnings: if requires_visual_processing {
                vec![PipelineWarning {
                    code: "NO_NATIVE_TEXT".to_string(),
                    message: "NO_NATIVE_TEXT".to_string(),
                    stage: Some(PipelineStage::Parse),
                }]
            } else {
                Vec::new()
            },
            requires_visual_processing,
        }
    }

    fn normalized_page(page_number: u32, texts: &[&str]) -> NormalizedPage {
        NormalizedPage {
            page_number,
            content: texts
                .iter()
                .enumerate()
                .map(|(index, text)| NormalizedBlock {
                    block_id: format!("block-{page_number}-{index}"),
                    kind: NormalizedBlockKind::Text,
                    text: (*text).to_string(),
                    source: SourceSpan {
                        page_start: page_number,
                        page_end: page_number,
                        section_id: None,
                        source_type: SourceType::NativeText,
                    },
                })
                .collect(),
            warnings: Vec::new(),
            requires_visual_processing: false,
        }
    }

    fn visual_page(page_number: u32) -> NormalizedPage {
        NormalizedPage {
            page_number,
            content: Vec::new(),
            warnings: vec![PipelineWarning {
                code: "NO_NATIVE_TEXT".to_string(),
                message: "NO_NATIVE_TEXT".to_string(),
                stage: Some(PipelineStage::Parse),
            }],
            requires_visual_processing: true,
        }
    }

    fn normalized_document(pages: Vec<NormalizedPage>) -> NormalizedDocument {
        NormalizedDocument {
            document_id: "normalized-structure-fixture".to_string(),
            normalization_version: NORMALIZATION_VERSION.to_string(),
            pages,
            warnings: Vec::new(),
        }
    }

    fn create_normalized_run(
        conn: &mut Connection,
        pages: Vec<ParsedPage>,
    ) -> (String, NormalizedDocument) {
        let source = TestPath::write_pdf_candidate();
        let (_, run) = ingest_pdf(conn, source.0.to_str().expect("UTF-8 fixture path"))
            .expect("candidate should ingest");
        parse_document(conn, &FixtureParser { pages }, &run.run_id)
            .expect("fixture parser should persist output");
        let normalized = normalize_document(conn, &CanonicalNormalizer::new(), &run.run_id)
            .expect("fixture should normalize");
        (run.run_id, normalized)
    }

    fn root(structured: &StructuredDocument) -> &StructureNode {
        &structured.nodes[0]
    }

    #[test]
    fn simple_numbered_document_has_deterministic_section_boundaries() {
        let normalized = normalized_document(vec![normalized_page(
            1,
            &[
                "1. Introduction",
                "Introduction body",
                "2. Findings",
                "Findings body",
            ],
        )]);
        let structured = DeterministicStructureInterpreter::new()
            .interpret(&normalized)
            .expect("numbered document should structure");

        assert_eq!(root(&structured).children.len(), 2);
        assert_eq!(
            root(&structured).children[0].kind,
            StructureNodeKind::Section
        );
        assert_eq!(
            root(&structured).children[0].title.as_deref(),
            Some("Introduction")
        );
        assert_eq!(
            root(&structured).children[0].block_ids,
            vec!["block-1-0", "block-1-1"]
        );
        assert_eq!(
            root(&structured).children[1].title.as_deref(),
            Some("Findings")
        );
        assert_eq!(
            root(&structured).children[1].block_ids,
            vec!["block-1-2", "block-1-3"]
        );
    }

    #[test]
    fn nested_numbering_builds_parent_child_hierarchy() {
        let normalized = normalized_document(vec![normalized_page(
            1,
            &["1", "1.1", "Purpose body", "1.2", "2", "2.1"],
        )]);
        let structured = DeterministicStructureInterpreter::new()
            .interpret(&normalized)
            .expect("nested numbering should structure");
        let document = root(&structured);

        assert_eq!(document.children.len(), 2);
        assert_eq!(document.children[0].level, 1);
        assert_eq!(document.children[0].children.len(), 2);
        assert_eq!(document.children[0].children[0].level, 2);
        assert_eq!(
            document.children[0].children[0].block_ids,
            vec!["block-1-1", "block-1-2"]
        );
        assert_eq!(
            document.children[0].children[1].block_ids,
            vec!["block-1-3"]
        );
        assert_eq!(document.children[1].children.len(), 1);
        assert_eq!(
            document.children[1].children[0].block_ids,
            vec!["block-1-5"]
        );
    }

    #[test]
    fn plain_and_ambiguous_text_remains_unstructured() {
        let normalized = normalized_document(vec![normalized_page(
            1,
            &[
                "Summary",
                "1. revenue increased by 4.2%.",
                "7.4.3 Orphaned",
                "2027 OPERATIONS REPORT",
            ],
        )]);
        let structured = DeterministicStructureInterpreter::new()
            .interpret(&normalized)
            .expect("plain text is a valid structured result");

        assert_eq!(root(&structured).children.len(), 1);
        let unstructured = &root(&structured).children[0];
        assert_eq!(unstructured.kind, StructureNodeKind::Unstructured);
        assert_eq!(unstructured.block_ids.len(), 4);
        assert!(unstructured.children.is_empty());
    }

    #[test]
    fn active_numbered_section_continues_across_pages_with_exact_spans() {
        let normalized = normalized_document(vec![
            normalized_page(1, &["1. Introduction"]),
            normalized_page(2, &["First continuation"]),
            normalized_page(3, &["Second continuation"]),
            normalized_page(4, &["2. Findings"]),
        ]);
        let structured = DeterministicStructureInterpreter::new()
            .interpret(&normalized)
            .expect("multi-page section should structure");
        let first = &root(&structured).children[0];

        assert_eq!(first.block_ids, vec!["block-1-0", "block-2-0", "block-3-0"]);
        assert_eq!(
            first
                .source_spans
                .iter()
                .map(|span| span.page_start)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(root(&structured).children[1].block_ids, vec!["block-4-0"]);
    }

    #[test]
    fn visual_page_metadata_survives_without_invented_content() {
        let normalized = normalized_document(vec![
            normalized_page(1, &["Plain content"]),
            visual_page(2),
            normalized_page(3, &["More content"]),
        ]);
        let structured = DeterministicStructureInterpreter::new()
            .interpret(&normalized)
            .expect("visual page should not fail structure");

        assert_eq!(structured.pages.len(), 3);
        assert!(structured.pages[1].requires_visual_processing);
        assert!(structured.pages[1]
            .warnings
            .iter()
            .any(|warning| warning.code == "NO_NATIVE_TEXT"));
        assert_eq!(root(&structured).children[0].block_ids.len(), 2);
    }

    #[test]
    fn structure_is_deterministic_and_accounts_for_every_block_once() {
        let normalized = normalized_document(vec![
            normalized_page(1, &["Preamble", "1. Section", "body"]),
            normalized_page(2, &["1.1 Subsection", "more body"]),
        ]);
        let original = normalized.clone();
        let interpreter = DeterministicStructureInterpreter::new();
        let first = interpreter
            .interpret(&normalized)
            .expect("input should structure");
        let second = interpreter
            .interpret(&normalized)
            .expect("same input should structure again");

        assert_eq!(first, second);
        assert_eq!(normalized, original);
        validate_structured_document(&first, &normalized, STRUCTURE_VERSION)
            .expect("coverage and provenance should validate");
        assert!(first.nodes[0].node_id.starts_with("sn-"));
    }

    #[test]
    fn validation_rejects_missing_foreign_duplicate_and_wrong_provenance_references() {
        let normalized = normalized_document(vec![normalized_page(1, &["plain", "content"])]);
        let valid = DeterministicStructureInterpreter::new()
            .interpret(&normalized)
            .expect("control output should validate");

        let mut missing = valid.clone();
        missing.nodes[0].children[0].block_ids.pop();
        missing.nodes[0].children[0].source_spans.pop();
        assert!(validate_structured_document(&missing, &normalized, STRUCTURE_VERSION).is_err());

        let mut foreign = valid.clone();
        foreign.nodes[0].children[0].block_ids[0] = "foreign-block".to_string();
        assert!(validate_structured_document(&foreign, &normalized, STRUCTURE_VERSION).is_err());

        let mut duplicate = valid.clone();
        duplicate.nodes[0].children[0]
            .block_ids
            .push("block-1-0".to_string());
        duplicate.nodes[0].children[0]
            .source_spans
            .push(normalized.pages[0].content[0].source.clone());
        assert!(validate_structured_document(&duplicate, &normalized, STRUCTURE_VERSION).is_err());

        let mut wrong_span = valid;
        wrong_span.nodes[0].children[0].source_spans[0].page_end = 2;
        assert!(validate_structured_document(&wrong_span, &normalized, STRUCTURE_VERSION).is_err());

        let heading_input = normalized_document(vec![normalized_page(1, &["1. Exact Title"])]);
        let mut fabricated_title = DeterministicStructureInterpreter::new()
            .interpret(&heading_input)
            .expect("heading control output should validate");
        fabricated_title.nodes[0].children[0].title = Some("Rewritten Title".to_string());
        assert!(
            validate_structured_document(&fabricated_title, &heading_input, STRUCTURE_VERSION)
                .is_err()
        );
    }

    #[test]
    fn invalid_persisted_normalized_input_fails_without_structured_artifact() {
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (run_id, mut normalized) =
            create_normalized_run(&mut conn, vec![parsed_page(1, "valid normalized source")]);
        normalized.pages[0].page_number = 2;
        let artifact_json = serde_json::to_string(&normalized).expect("fixture should serialize");
        let artifact_hash = format!("{:x}", Sha256::digest(artifact_json.as_bytes()));
        conn.execute(
            "UPDATE normalized_documents
             SET normalized_artifact = ?1, artifact_hash = ?2
             WHERE run_id = ?3",
            params![artifact_json, artifact_hash, run_id],
        )
        .expect("test should inject inconsistent normalized output");

        let error = structure_document(
            &mut conn,
            &DeterministicStructureInterpreter::new(),
            &run_id,
        )
        .expect_err("invalid normalized artifact must fail");

        assert_eq!(error.code(), "INVALID_NORMALIZED_DOCUMENT");
        let failed = db::get_pipeline_run(&conn, &run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert!(db::get_structured_document(&conn, &run_id)
            .expect("artifact query should work")
            .is_none());
        assert!(!db::list_pipeline_events(&conn, &run_id)
            .expect("events should load")
            .iter()
            .any(|event| event.next_state == PipelineState::Structured));
    }

    #[test]
    fn invalid_interpreter_output_fails_without_structured_artifact() {
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (run_id, _) = create_normalized_run(&mut conn, vec![parsed_page(1, "plain text")]);

        let error = structure_document(&mut conn, &MissingCoverageInterpreter, &run_id)
            .expect_err("missing block coverage must fail");

        assert_eq!(error.code(), "INVALID_STRUCTURED_DOCUMENT");
        assert_eq!(
            db::get_pipeline_run(&conn, &run_id)
                .expect("run should load")
                .expect("run should exist")
                .state,
            PipelineState::Failed
        );
        assert!(db::get_structured_document(&conn, &run_id)
            .expect("artifact query should work")
            .is_none());
    }

    #[test]
    fn structured_event_failure_rolls_back_artifact_state_and_version_atomically() {
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (run_id, _) = create_normalized_run(&mut conn, vec![parsed_page(1, "1. Atomicity")]);
        conn.execute_batch(
            "CREATE TRIGGER test_fail_structured_event
             BEFORE INSERT ON pipeline_events
             WHEN NEW.next_state = '\"Structured\"'
             BEGIN
                 SELECT RAISE(ABORT, 'injected structured event failure');
             END;",
        )
        .expect("failure trigger should install");

        let error = structure_document(
            &mut conn,
            &DeterministicStructureInterpreter::new(),
            &run_id,
        )
        .expect_err("structured event failure should fail the run");

        assert_eq!(error.code(), "STRUCTURED_ARTIFACT_PERSISTENCE_FAILED");
        assert!(db::get_structured_document(&conn, &run_id)
            .expect("artifact query should work")
            .is_none());
        let failed = db::get_pipeline_run(&conn, &run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert_eq!(failed.state_version, 9);
        let events = db::list_pipeline_events(&conn, &run_id).expect("events should load");
        assert!(!events
            .iter()
            .any(|event| event.next_state == PipelineState::Structured));
        assert_eq!(
            events
                .last()
                .expect("failure event should exist")
                .next_state,
            PipelineState::Failed
        );
    }

    #[test]
    fn structured_artifact_and_hash_survive_independent_reopen() {
        let database = TestPath::new("db");
        let run_id;
        let expected_artifact;
        let expected_hash;
        {
            let mut conn = db::init_db(&database.0).expect("schema should initialize");
            let created = create_normalized_run(
                &mut conn,
                vec![parsed_page(1, "1. Persisted"), parsed_page(2, "body")],
            );
            run_id = created.0;
            expected_artifact = structure_document(
                &mut conn,
                &DeterministicStructureInterpreter::new(),
                &run_id,
            )
            .expect("document should structure");
            expected_hash = conn
                .query_row(
                    "SELECT artifact_hash FROM structured_documents WHERE run_id = ?1",
                    [&run_id],
                    |row| row.get::<_, String>(0),
                )
                .expect("artifact hash should persist");
        }

        let reopened = db::init_db(&database.0).expect("database should reopen");
        assert_eq!(
            db::schema_version(&reopened).expect("version should load"),
            4
        );
        assert_eq!(
            db::get_structured_document(&reopened, &run_id)
                .expect("artifact should load")
                .expect("artifact should exist"),
            expected_artifact
        );
        assert_eq!(
            reopened
                .query_row(
                    "SELECT artifact_hash FROM structured_documents WHERE run_id = ?1",
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
            PipelineState::Structured
        );
    }

    #[test]
    fn structured_artifact_integrity_is_checked() {
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (run_id, _) = create_normalized_run(&mut conn, vec![parsed_page(1, "integrity")]);
        structure_document(
            &mut conn,
            &DeterministicStructureInterpreter::new(),
            &run_id,
        )
        .expect("document should structure");
        conn.execute(
            "UPDATE structured_documents
             SET structured_artifact = structured_artifact || ' '
             WHERE run_id = ?1",
            [&run_id],
        )
        .expect("test should tamper with stored artifact");

        assert!(matches!(
            db::get_structured_document(&conn, &run_id),
            Err(StoreError::StructuredArtifactIntegrityMismatch { .. })
        ));
    }

    #[test]
    fn structured_artifact_retrieval_rejects_run_document_mismatch() {
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (run_id, _) = create_normalized_run(&mut conn, vec![parsed_page(1, "association")]);
        let mut structured = structure_document(
            &mut conn,
            &DeterministicStructureInterpreter::new(),
            &run_id,
        )
        .expect("document should structure");
        let other_source = TestPath::write_pdf_candidate();
        let (other_document, _) = ingest_pdf(
            &mut conn,
            other_source.0.to_str().expect("UTF-8 fixture path"),
        )
        .expect("other document should ingest");
        structured.document_id = other_document.document_id.clone();
        let artifact_json =
            serde_json::to_string(&structured).expect("tampered artifact should serialize");
        let artifact_hash = format!("{:x}", Sha256::digest(artifact_json.as_bytes()));
        conn.execute(
            "UPDATE structured_documents
             SET document_id = ?1, structured_artifact = ?2, artifact_hash = ?3
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
            db::get_structured_document(&conn, &run_id),
            Err(StoreError::StructuredArtifactMetadataMismatch { .. })
        ));
    }

    #[test]
    fn two_connections_reject_stale_structuring_transition() {
        let database = TestPath::new("db");
        let mut caller_a = db::init_db(&database.0).expect("schema should initialize");
        let (run_id, _) = create_normalized_run(&mut caller_a, vec![parsed_page(1, "one winner")]);
        let mut caller_b = db::init_db(&database.0).expect("second connection should open");
        let observed_a = db::get_pipeline_run(&caller_a, &run_id)
            .expect("run should load")
            .expect("run should exist");
        let observed_b = db::get_pipeline_run(&caller_b, &run_id)
            .expect("run should load")
            .expect("run should exist");

        let (advanced, _) = db::start_structuring(&mut caller_a, &run_id, observed_a.state_version)
            .expect("first caller should advance");
        let stale = db::start_structuring(&mut caller_b, &run_id, observed_b.state_version);

        assert!(matches!(
            stale,
            Err(StoreError::Transition(
                TransitionError::StaleExpectedState { .. }
            ))
        ));
        assert_eq!(advanced.state, PipelineState::Structuring);
        assert_eq!(
            db::get_pipeline_run(&caller_b, &run_id)
                .expect("run should load")
                .expect("run should exist")
                .state_version,
            advanced.state_version
        );
    }

    #[test]
    fn structured_completion_cannot_bypass_the_structuring_state() {
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (run_id, normalized) =
            create_normalized_run(&mut conn, vec![parsed_page(1, "1. Guarded")]);
        let structured = DeterministicStructureInterpreter::new()
            .interpret(&normalized)
            .expect("control artifact should structure");
        let run = db::get_pipeline_run(&conn, &run_id)
            .expect("run should load")
            .expect("run should exist");

        let bypass = db::complete_structuring(
            &mut conn,
            &run_id,
            run.state_version,
            &structured,
            Vec::new(),
        );

        assert!(matches!(
            bypass,
            Err(StoreError::Transition(
                TransitionError::StaleExpectedState { .. }
            ))
        ));
        let unchanged = db::get_pipeline_run(&conn, &run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(unchanged.state, PipelineState::Normalized);
        assert_eq!(unchanged.state_version, run.state_version);
        assert!(db::get_structured_document(&conn, &run_id)
            .expect("artifact query should work")
            .is_none());
    }

    #[test]
    fn realistic_pdf_runs_through_the_real_pipeline_with_conservative_structure() {
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/structured_report.pdf"
        );
        let mut conn = db::init_db(":memory:").expect("schema should initialize");
        let (_, run) = ingest_pdf(&mut conn, fixture).expect("real fixture should ingest");
        let parsed = parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id)
            .expect("real fixture should parse");
        let normalized = normalize_document(&mut conn, &CanonicalNormalizer::new(), &run.run_id)
            .expect("real fixture should normalize");
        let structured = structure_document(
            &mut conn,
            &DeterministicStructureInterpreter::new(),
            &run.run_id,
        )
        .expect("real fixture should structure");

        assert_eq!(parsed.pages.len(), 6);
        assert_eq!(normalized.pages.len(), 6);
        assert_eq!(structured.pages.len(), 6);
        assert!(structured.pages[4].requires_visual_processing);
        assert!(normalized.pages[4].content.is_empty());
        let document = root(&structured);
        assert_eq!(document.children.len(), 3);
        assert_eq!(document.children[0].kind, StructureNodeKind::Unstructured);
        assert_eq!(document.children[0].block_ids.len(), 1);
        assert_eq!(document.children[1].title.as_deref(), Some("Introduction"));
        assert_eq!(document.children[1].block_ids.len(), 2);
        assert_eq!(document.children[1].children.len(), 1);
        assert_eq!(
            document.children[1].children[0].title.as_deref(),
            Some("Purpose")
        );
        assert_eq!(document.children[2].title.as_deref(), Some("Findings"));
        let persisted_run = db::get_pipeline_run(&conn, &run.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert_eq!(persisted_run.state, PipelineState::Structured);
        assert_eq!(persisted_run.state_version, 9);
        let events = db::list_pipeline_events(&conn, &run.run_id).expect("events should load");
        assert_eq!(
            events[events.len() - 2].next_state,
            PipelineState::Structuring
        );
        assert_eq!(
            events[events.len() - 1].next_state,
            PipelineState::Structured
        );
        println!(
            "{}",
            serde_json::to_string_pretty(&structured).expect("structure should serialize")
        );
    }
}
