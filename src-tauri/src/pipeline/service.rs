use crate::pipeline::chunk::{chunk_document, ChunkPipelineError};
use crate::pipeline::contracts::{
    CompletedSummary, DocumentChunker, DocumentNormalizer, DocumentParser, ModelRuntime,
    StructureInterpreter,
};
use crate::pipeline::ingest::{ingest_pdf, IngestError};
use crate::pipeline::normalize::{normalize_document, NormalizePipelineError};
use crate::pipeline::parser::{parse_document, ParsePipelineError};
use crate::pipeline::structure::{structure_document, StructurePipelineError};
use crate::pipeline::summary::{summarize_chunked_document, SummaryPipelineError};
use rusqlite::Connection;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DocumentServiceError {
    #[error(transparent)]
    Ingest(#[from] IngestError),
    #[error(transparent)]
    Parse(#[from] ParsePipelineError),
    #[error(transparent)]
    Normalize(#[from] NormalizePipelineError),
    #[error(transparent)]
    Structure(#[from] StructurePipelineError),
    #[error(transparent)]
    Chunk(#[from] ChunkPipelineError),
    #[error(transparent)]
    Summary(#[from] SummaryPipelineError),
}

impl DocumentServiceError {
    pub fn code(&self) -> &str {
        match self {
            Self::Ingest(error) => error.code(),
            Self::Parse(error) => error.code(),
            Self::Normalize(error) => error.code(),
            Self::Structure(error) => error.code(),
            Self::Chunk(error) => error.code(),
            Self::Summary(error) => error.code(),
        }
    }
}

pub struct SummaryComponents<'a> {
    pub parser: &'a dyn DocumentParser,
    pub normalizer: &'a dyn DocumentNormalizer,
    pub interpreter: &'a dyn StructureInterpreter,
    pub chunker: &'a dyn DocumentChunker,
    pub runtime: &'a dyn ModelRuntime,
}

pub fn process_pdf_to_summary(
    conn: &mut Connection,
    file_path: &str,
    components: SummaryComponents<'_>,
) -> Result<CompletedSummary, DocumentServiceError> {
    let (document, run) = ingest_pdf(conn, file_path)?;
    let summary = process_ingested_to_summary(conn, &run.run_id, components)?;
    Ok(CompletedSummary {
        run_id: run.run_id,
        document,
        summary: summary.summary,
        citations: summary.citations,
    })
}

pub fn process_ingested_to_summary(
    conn: &mut Connection,
    run_id: &str,
    components: SummaryComponents<'_>,
) -> Result<crate::pipeline::contracts::SummaryArtifacts, DocumentServiceError> {
    parse_document(conn, components.parser, run_id)?;
    normalize_document(conn, components.normalizer, run_id)?;
    structure_document(conn, components.interpreter, run_id)?;
    chunk_document(conn, components.chunker, run_id)?;
    Ok(summarize_chunked_document(
        conn,
        components.runtime,
        run_id,
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::chunk::DeterministicDocumentChunker;
    use crate::pipeline::contracts::{
        ModelRequest, ModelResponse, ModelRuntimeFailure, PipelineState,
    };
    use crate::pipeline::db::{
        get_citation_artifact, get_normalized_document, get_pipeline_run, get_summary_artifact,
        get_synthesized_document, init_db,
    };
    use crate::pipeline::model::OllamaRuntime;
    use crate::pipeline::normalize::CanonicalNormalizer;
    use crate::pipeline::parser::PdfExtractParser;
    use crate::pipeline::structure::DeterministicStructureInterpreter;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestDatabase(PathBuf);

    impl TestDatabase {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("doc-sum-live-citation-{}.db", Uuid::new_v4())))
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    struct FixtureRuntime;

    impl ModelRuntime for FixtureRuntime {
        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            Ok(ModelResponse {
                text: crate::pipeline::summary::fixture_model_output(request),
                runtime_id: self.runtime_id().to_string(),
                model_id: self.model_id().to_string(),
            })
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }

        fn runtime_id(&self) -> &str {
            "fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "fixture-model"
        }
    }

    #[test]
    fn application_service_runs_the_real_pdf_pipeline_to_a_durable_summary() {
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let mut conn = init_db(":memory:").expect("schema should initialize");
        let parser = PdfExtractParser::new();
        let normalizer = CanonicalNormalizer::new();
        let interpreter = DeterministicStructureInterpreter::new();
        let chunker = DeterministicDocumentChunker::new();
        let result = process_pdf_to_summary(
            &mut conn,
            source.to_str().expect("fixture path should be UTF-8"),
            SummaryComponents {
                parser: &parser,
                normalizer: &normalizer,
                interpreter: &interpreter,
                chunker: &chunker,
                runtime: &FixtureRuntime,
            },
        )
        .expect("real PDF path should complete");

        assert_eq!(
            get_pipeline_run(&conn, &result.run_id)
                .expect("run should load")
                .expect("run should exist")
                .state,
            PipelineState::CompleteWithWarnings
        );
        assert_eq!(
            get_summary_artifact(&conn, &result.run_id)
                .expect("summary should load")
                .expect("summary should exist"),
            result.summary
        );
        assert_eq!(
            get_citation_artifact(&conn, &result.run_id)
                .expect("citations should load")
                .expect("citations should exist"),
            result.citations
        );
    }

    #[test]
    #[ignore = "requires the configured local Ollama runtime and selected model"]
    fn live_ollama_pipeline_persists_exact_citations_across_database_reopen() {
        let database = TestDatabase::new();
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let runtime = OllamaRuntime::from_environment().expect("Ollama runtime should configure");
        let parser = PdfExtractParser::new();
        let normalizer = CanonicalNormalizer::new();
        let interpreter = DeterministicStructureInterpreter::new();
        let chunker = DeterministicDocumentChunker::new();
        let (run_id, summary_hash, citation_hash, claim_count, evidence_count) = {
            let mut conn = init_db(&database.0).expect("live database should initialize");
            let result = process_pdf_to_summary(
                &mut conn,
                source.to_str().expect("fixture path should be UTF-8"),
                SummaryComponents {
                    parser: &parser,
                    normalizer: &normalizer,
                    interpreter: &interpreter,
                    chunker: &chunker,
                    runtime: &runtime,
                },
            )
            .expect("configured Ollama should complete the real PDF pipeline");
            let normalized = get_normalized_document(&conn, &result.run_id)
                .expect("normalized artifact should load")
                .expect("normalized artifact should exist");
            let blocks = normalized
                .pages
                .iter()
                .flat_map(|page| page.content.iter())
                .map(|block| (block.block_id.as_str(), block))
                .collect::<HashMap<_, _>>();
            assert!(!result.citations.claims.is_empty());
            assert!(!result.citations.evidence.is_empty());
            for evidence in &result.citations.evidence {
                let block = blocks
                    .get(evidence.block_id.as_str())
                    .expect("live citation block should exist");
                assert!(block.text.contains(&evidence.exact_quote));
                assert_eq!(evidence.source_span, block.source);
            }
            let synthesized = get_synthesized_document(&conn, &result.run_id)
                .expect("synthesis should load")
                .expect("synthesis should exist");
            assert_eq!(synthesized.model_id, runtime.model_id());
            (
                result.run_id,
                result.summary.integrity_hash,
                result.citations.integrity_hash,
                result.citations.claims.len(),
                result.citations.evidence.len(),
            )
        };

        let reopened = init_db(&database.0).expect("live database should independently reopen");
        assert_eq!(
            reopened
                .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                .expect("quick check should run"),
            "ok"
        );
        let summary = get_summary_artifact(&reopened, &run_id)
            .expect("summary should load")
            .expect("summary should persist");
        let citations = get_citation_artifact(&reopened, &run_id)
            .expect("citations should load")
            .expect("citations should persist");
        assert_eq!(summary.integrity_hash, summary_hash);
        assert_eq!(citations.integrity_hash, citation_hash);
        assert_eq!(citations.summary_integrity_hash, summary.integrity_hash);
        assert_eq!(citations.claims.len(), claim_count);
        assert_eq!(citations.evidence.len(), evidence_count);
        assert_eq!(
            get_pipeline_run(&reopened, &run_id)
                .expect("run should load")
                .expect("run should persist")
                .state,
            PipelineState::CompleteWithWarnings
        );
    }
}
