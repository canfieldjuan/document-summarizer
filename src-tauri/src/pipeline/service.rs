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
    parse_document(conn, components.parser, &run.run_id)?;
    normalize_document(conn, components.normalizer, &run.run_id)?;
    structure_document(conn, components.interpreter, &run.run_id)?;
    chunk_document(conn, components.chunker, &run.run_id)?;
    let summary = summarize_chunked_document(conn, components.runtime, &run.run_id)?;
    Ok(CompletedSummary {
        run_id: run.run_id,
        document,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::chunk::DeterministicDocumentChunker;
    use crate::pipeline::contracts::{
        ModelRequest, ModelResponse, ModelRuntimeFailure, PipelineState,
    };
    use crate::pipeline::db::{get_pipeline_run, get_summary_artifact, init_db};
    use crate::pipeline::normalize::CanonicalNormalizer;
    use crate::pipeline::parser::PdfExtractParser;
    use crate::pipeline::structure::DeterministicStructureInterpreter;
    use std::path::PathBuf;

    struct FixtureRuntime;

    impl ModelRuntime for FixtureRuntime {
        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            let text = if request.system_prompt.contains("synthesize chunk notes") {
                "A grounded summary from the realistic PDF fixture."
            } else {
                "Grounded source-chunk notes."
            };
            Ok(ModelResponse {
                text: text.to_string(),
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
    }
}
