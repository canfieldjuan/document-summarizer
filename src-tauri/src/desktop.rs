use crate::pipeline::chunk::DeterministicDocumentChunker;
use crate::pipeline::contracts::{
    CompletedSummary, ModelRuntime, ModelRuntimeFailure, PipelineFailure, PipelineRun,
    PipelineStage, PipelineState,
};
use crate::pipeline::control::CancellationToken;
use crate::pipeline::db::{self, StoreError};
use crate::pipeline::model_settings::runtime_from_settings;
use crate::pipeline::normalize::CanonicalNormalizer;
use crate::pipeline::parser::PdfExtractParser;
use crate::pipeline::service::{
    admit_pdf_for_background, admit_retry_for_background, continuation_plan,
    continue_run_to_summary_controlled, process_started_parsing_to_summary_controlled,
    ContinuationComponents, DocumentServiceError, SummaryComponents,
};
use crate::pipeline::structure::DeterministicStructureInterpreter;
use serde::Serialize;
use std::collections::HashMap;
use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use thiserror::Error;

type RuntimeFactory =
    Arc<dyn Fn() -> Result<Box<dyn ModelRuntime>, ModelRuntimeFailure> + Send + Sync + 'static>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundRunAccepted {
    pub run_id: String,
    pub document_id: String,
    pub original_filename: String,
    pub byte_size: u64,
    pub state: PipelineState,
    pub state_version: u32,
}

#[derive(Debug, Error)]
pub enum DesktopJobError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Service(#[from] DocumentServiceError),
    #[error("Model runtime configuration failed: {0:?}")]
    Runtime(#[from] ModelRuntimeFailure),
    #[error("Pipeline run already has a desktop worker: {0}")]
    AlreadyRunning(String),
    #[error("Pipeline run does not have an active desktop worker: {0}")]
    NotRunning(String),
    #[error("The desktop worker registry is unavailable")]
    RegistryUnavailable,
    #[error("The desktop worker could not start: {0}")]
    WorkerStart(#[source] io::Error),
    #[error("The desktop worker could not start and its active run could not be failed: {0}")]
    WorkerStartFailurePersistence(StoreError),
}

impl DesktopJobError {
    pub fn code(&self) -> &str {
        match self {
            Self::Store(StoreError::RunNotFound(_)) => "BACKGROUND_RUN_NOT_FOUND",
            Self::Store(StoreError::CancellationNotAllowed { .. }) => {
                "BACKGROUND_CANCELLATION_NOT_ALLOWED"
            }
            Self::Store(StoreError::Transition(_)) | Self::Store(StoreError::StaleWrite { .. }) => {
                "BACKGROUND_STALE_STATE"
            }
            Self::Store(_) => "PIPELINE_STORE_ERROR",
            Self::Service(error) => error.code(),
            Self::Runtime(failure) => &failure.code,
            Self::AlreadyRunning(_) => "BACKGROUND_JOB_ALREADY_RUNNING",
            Self::NotRunning(_) => "BACKGROUND_JOB_NOT_RUNNING",
            Self::RegistryUnavailable => "BACKGROUND_JOB_REGISTRY_UNAVAILABLE",
            Self::WorkerStart(_) => "BACKGROUND_WORKER_UNAVAILABLE",
            Self::WorkerStartFailurePersistence(_) => {
                "BACKGROUND_WORKER_FAILURE_PERSISTENCE_FAILED"
            }
        }
    }
}

#[derive(Clone)]
pub struct DesktopJobManager {
    db_path: PathBuf,
    active: Arc<Mutex<HashMap<String, CancellationToken>>>,
    runtime_factory: RuntimeFactory,
}

impl DesktopJobManager {
    pub fn new(db_path: PathBuf, settings_path: PathBuf) -> Self {
        Self::with_runtime_factory(
            db_path,
            Arc::new(move || {
                runtime_from_settings(&settings_path)
                    .map(|runtime| Box::new(runtime) as Box<dyn ModelRuntime>)
            }),
        )
    }

    fn with_runtime_factory(db_path: PathBuf, runtime_factory: RuntimeFactory) -> Self {
        Self {
            db_path,
            active: Arc::new(Mutex::new(HashMap::new())),
            runtime_factory,
        }
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn start_pdf(&self, file_path: &str) -> Result<BackgroundRunAccepted, DesktopJobError> {
        let runtime = (self.runtime_factory)()?;
        let mut conn = db::init_db(&self.db_path)?;
        let (document, run) = admit_pdf_for_background(&mut conn, file_path)?;
        let accepted = accepted_view(&document, &run);
        self.spawn(run.run_id, BackgroundWork::StartedParsing, Some(runtime))?;
        Ok(accepted)
    }

    pub fn start_retry(
        &self,
        source_run_id: &str,
        expected_source_version: u32,
    ) -> Result<BackgroundRunAccepted, DesktopJobError> {
        let runtime = (self.runtime_factory)()?;
        let mut conn = db::init_db(&self.db_path)?;
        let (document, run) =
            admit_retry_for_background(&mut conn, source_run_id, expected_source_version)?;
        let accepted = accepted_view(&document, &run);
        self.spawn(run.run_id, BackgroundWork::StartedParsing, Some(runtime))?;
        Ok(accepted)
    }

    pub fn start_continuation(
        &self,
        run_id: &str,
        expected_state_version: u32,
    ) -> Result<BackgroundRunAccepted, DesktopJobError> {
        let conn = db::init_db(&self.db_path)?;
        let plan = continuation_plan(&conn, run_id, expected_state_version)
            .map_err(DocumentServiceError::from)?;
        let run = db::get_pipeline_run(&conn, run_id)?
            .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
        let document = db::get_document(&conn, &run.document_id)?
            .ok_or_else(|| StoreError::DocumentNotFound(run.document_id.clone()))?;
        let runtime = plan
            .requires_runtime
            .then(|| (self.runtime_factory)())
            .transpose()?;
        let accepted = accepted_view(&document, &run);
        self.spawn(
            run.run_id,
            BackgroundWork::Continue {
                expected_state_version,
            },
            runtime,
        )?;
        Ok(accepted)
    }

    pub fn request_cancellation(
        &self,
        run_id: &str,
        expected_state_version: u32,
    ) -> Result<PipelineRun, DesktopJobError> {
        let token = self
            .active
            .lock()
            .map_err(|_| DesktopJobError::RegistryUnavailable)?
            .get(run_id)
            .cloned()
            .ok_or_else(|| DesktopJobError::NotRunning(run_id.to_string()))?;
        let mut conn = db::init_db(&self.db_path)?;
        let cancelling = db::request_cancellation(&mut conn, run_id, expected_state_version)?;
        token.request();
        Ok(cancelling)
    }

    pub fn is_active(&self, run_id: &str) -> Result<bool, DesktopJobError> {
        Ok(self
            .active
            .lock()
            .map_err(|_| DesktopJobError::RegistryUnavailable)?
            .contains_key(run_id))
    }

    fn spawn(
        &self,
        run_id: String,
        work: BackgroundWork,
        runtime: Option<Box<dyn ModelRuntime>>,
    ) -> Result<(), DesktopJobError> {
        let token = CancellationToken::new();
        {
            let mut active = self
                .active
                .lock()
                .map_err(|_| DesktopJobError::RegistryUnavailable)?;
            if active.contains_key(&run_id) {
                return Err(DesktopJobError::AlreadyRunning(run_id));
            }
            active.insert(run_id.clone(), token.clone());
        }

        let worker_manager = self.clone();
        let worker_run_id = run_id.clone();
        let active_admission = matches!(work, BackgroundWork::StartedParsing);
        let spawn_result = thread::Builder::new()
            .name(format!("document-summary-{}", short_run_id(&run_id)))
            .spawn(move || {
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    worker_manager.execute(&worker_run_id, work, runtime, &token)
                }));
                if let Err(error) = worker_manager.finalize(&worker_run_id, outcome) {
                    eprintln!("Desktop background run {worker_run_id} could not finalize: {error}");
                }
                if let Ok(mut active) = worker_manager.active.lock() {
                    active.remove(&worker_run_id);
                }
            });

        if let Err(error) = spawn_result {
            if let Ok(mut active) = self.active.lock() {
                active.remove(&run_id);
            }
            if active_admission {
                self.persist_worker_start_failure(&run_id)?;
            }
            return Err(DesktopJobError::WorkerStart(error));
        }
        Ok(())
    }

    fn execute(
        &self,
        run_id: &str,
        work: BackgroundWork,
        runtime: Option<Box<dyn ModelRuntime>>,
        token: &CancellationToken,
    ) -> Result<CompletedSummary, DocumentServiceError> {
        let mut conn = db::init_db(&self.db_path)
            .map_err(crate::pipeline::parser::ParsePipelineError::from)?;
        let parser = PdfExtractParser::new();
        let normalizer = CanonicalNormalizer::new();
        let interpreter = DeterministicStructureInterpreter::new();
        let chunker = DeterministicDocumentChunker::new();

        match work {
            BackgroundWork::StartedParsing => {
                let runtime = runtime.as_deref().ok_or_else(|| {
                    DocumentServiceError::RuntimeRequiredForBackground(run_id.to_string())
                })?;
                process_started_parsing_to_summary_controlled(
                    &mut conn,
                    run_id,
                    SummaryComponents {
                        parser: &parser,
                        normalizer: &normalizer,
                        interpreter: &interpreter,
                        chunker: &chunker,
                        runtime,
                    },
                    token,
                )
            }
            BackgroundWork::Continue {
                expected_state_version,
            } => continue_run_to_summary_controlled(
                &mut conn,
                run_id,
                expected_state_version,
                ContinuationComponents {
                    parser: &parser,
                    normalizer: &normalizer,
                    interpreter: &interpreter,
                    chunker: &chunker,
                    runtime: runtime.as_deref(),
                },
                token,
            ),
        }
    }

    fn finalize(
        &self,
        run_id: &str,
        outcome: Result<
            Result<CompletedSummary, DocumentServiceError>,
            Box<dyn std::any::Any + Send>,
        >,
    ) -> Result<(), StoreError> {
        let mut conn = db::init_db(&self.db_path)?;
        let run = db::get_pipeline_run(&conn, run_id)?
            .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
        if run.state == PipelineState::Cancelling {
            db::complete_cancellation(&mut conn, run_id, run.state_version)?;
            return Ok(());
        }
        if run.state.is_terminal() {
            return Ok(());
        }
        if let Ok(Err(error)) = &outcome {
            if error.is_concurrent_ownership_loss() {
                return Ok(());
            }
        }

        let (code, message) = match outcome {
            Ok(Ok(_)) => (
                "BACKGROUND_WORKER_INCOMPLETE".to_string(),
                "Background processing returned without reaching a terminal state".to_string(),
            ),
            Ok(Err(error)) => (error.code().to_string(), error.to_string()),
            Err(_) => (
                "BACKGROUND_WORKER_PANIC".to_string(),
                "Background processing stopped unexpectedly".to_string(),
            ),
        };
        let stage = stage_for_run(&run);
        match db::fail_background_execution(
            &mut conn,
            run_id,
            run.state,
            run.state_version,
            PipelineFailure {
                code,
                message,
                stage: Some(stage),
                recoverable: true,
            },
        ) {
            Ok(_) => Ok(()),
            Err(error) if error.is_stale_transition() => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn persist_worker_start_failure(&self, run_id: &str) -> Result<(), DesktopJobError> {
        let mut conn = db::init_db(&self.db_path)?;
        let run = db::get_pipeline_run(&conn, run_id)?
            .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
        let stage = stage_for_run(&run);
        db::fail_background_execution(
            &mut conn,
            run_id,
            run.state,
            run.state_version,
            PipelineFailure {
                code: "BACKGROUND_WORKER_UNAVAILABLE".to_string(),
                message: "The desktop background worker could not start".to_string(),
                stage: Some(stage),
                recoverable: true,
            },
        )
        .map(|_| ())
        .map_err(DesktopJobError::WorkerStartFailurePersistence)
    }
}

#[derive(Clone, Copy)]
enum BackgroundWork {
    StartedParsing,
    Continue { expected_state_version: u32 },
}

fn accepted_view(
    document: &crate::pipeline::contracts::IngestedDocument,
    run: &PipelineRun,
) -> BackgroundRunAccepted {
    BackgroundRunAccepted {
        run_id: run.run_id.clone(),
        document_id: document.document_id.clone(),
        original_filename: document.original_filename.clone(),
        byte_size: document.byte_size,
        state: run.state.clone(),
        state_version: run.state_version,
    }
}

fn short_run_id(run_id: &str) -> &str {
    run_id.get(..8).unwrap_or(run_id)
}

fn stage_for_run(run: &PipelineRun) -> PipelineStage {
    if let Some(stage) = run.state.active_stage() {
        return stage;
    }
    match &run.state {
        PipelineState::Ingested => PipelineStage::Parse,
        PipelineState::Parsed => PipelineStage::Normalize,
        PipelineState::Normalized => PipelineStage::Structure,
        PipelineState::Structured => PipelineStage::Chunk,
        PipelineState::Chunked => PipelineStage::Analyze,
        PipelineState::Analyzed => PipelineStage::Synthesize,
        PipelineState::Synthesized | PipelineState::Verified => PipelineStage::Verify,
        _ => run.current_stage.clone().unwrap_or(PipelineStage::Parse),
    }
}

trait TerminalState {
    fn is_terminal(&self) -> bool;
}

impl TerminalState for PipelineState {
    fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Complete | Self::CompleteWithWarnings | Self::Failed | Self::Cancelled
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contracts::{ModelRequest, ModelResponse};
    use crate::pipeline::db::{
        get_analyzed_document, get_pipeline_run, get_summary_artifact, list_pipeline_events,
    };
    use crate::pipeline::service::ContinuationPipelineError;
    use crate::pipeline::state::TransitionError;
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Condvar, Mutex as StdMutex};
    use std::time::{Duration, Instant};
    use uuid::Uuid;

    struct TestDatabase(PathBuf);

    impl TestDatabase {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("doc-sum-background-{}.db", Uuid::new_v4())))
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
                request_attempts: Vec::new(),
            })
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }

        fn runtime_id(&self) -> &str {
            "background-fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "background-fixture-model"
        }
    }

    struct BlockingRuntime {
        gate: Arc<BlockingGate>,
    }

    struct BlockingGate {
        entered: AtomicBool,
        released: StdMutex<bool>,
        release_changed: Condvar,
    }

    impl BlockingGate {
        fn new() -> Self {
            Self {
                entered: AtomicBool::new(false),
                released: StdMutex::new(false),
                release_changed: Condvar::new(),
            }
        }

        fn release(&self) {
            if let Ok(mut released) = self.released.lock() {
                *released = true;
                self.release_changed.notify_all();
            }
        }
    }

    impl ModelRuntime for BlockingRuntime {
        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            self.gate.entered.store(true, Ordering::Release);
            let mut released = self.gate.released.lock().map_err(|_| ModelRuntimeFailure {
                code: "FIXTURE_GATE_FAILED".to_string(),
                message: "fixture gate was poisoned".to_string(),
                recoverable: true,
                request_attempts: Vec::new(),
            })?;
            while !*released {
                released =
                    self.gate
                        .release_changed
                        .wait(released)
                        .map_err(|_| ModelRuntimeFailure {
                            code: "FIXTURE_GATE_FAILED".to_string(),
                            message: "fixture gate was poisoned".to_string(),
                            recoverable: true,
                            request_attempts: Vec::new(),
                        })?;
            }
            Ok(ModelResponse {
                text: crate::pipeline::summary::fixture_model_output(request),
                runtime_id: self.runtime_id().to_string(),
                model_id: self.model_id().to_string(),
                request_attempts: Vec::new(),
            })
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }

        fn runtime_id(&self) -> &str {
            "blocking-fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "blocking-fixture-model"
        }
    }

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf")
    }

    fn fixture_manager(database: &TestDatabase) -> DesktopJobManager {
        DesktopJobManager::with_runtime_factory(
            database.0.clone(),
            Arc::new(|| Ok(Box::new(FixtureRuntime))),
        )
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if condition() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("background condition did not become true before the test deadline");
    }

    #[test]
    fn background_run_returns_before_completion_and_persists_after_reopen() {
        let database = TestDatabase::new();
        let manager = fixture_manager(&database);
        let accepted = manager
            .start_pdf(
                fixture_path()
                    .to_str()
                    .expect("fixture path should be UTF-8"),
            )
            .expect("background run should be accepted");
        assert_eq!(accepted.state, PipelineState::Parsing);

        wait_until(|| {
            !manager
                .is_active(&accepted.run_id)
                .expect("registry should remain readable")
        });

        let reopened = db::init_db(&database.0).expect("database should reopen independently");
        let run = get_pipeline_run(&reopened, &accepted.run_id)
            .expect("run should load")
            .expect("run should exist");
        assert!(matches!(
            run.state,
            PipelineState::Complete | PipelineState::CompleteWithWarnings
        ));
        assert!(get_summary_artifact(&reopened, &accepted.run_id)
            .expect("summary query should succeed")
            .is_some());
        assert_eq!(
            reopened
                .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                .expect("quick_check should run"),
            "ok"
        );

        let events_before = list_pipeline_events(&reopened, &accepted.run_id)
            .expect("events should load after reopen");
        let rejected = manager
            .request_cancellation(&accepted.run_id, run.state_version)
            .expect_err("a completed worker must not accept cancellation");
        assert!(matches!(rejected, DesktopJobError::NotRunning(_)));
        let events_after = list_pipeline_events(&reopened, &accepted.run_id)
            .expect("events should remain readable");
        assert_eq!(events_after, events_before);
    }

    #[test]
    fn cancellation_wins_atomically_during_model_work_and_survives_reopen() {
        let database = TestDatabase::new();
        let gate = Arc::new(BlockingGate::new());
        let factory_gate = gate.clone();
        let manager = DesktopJobManager::with_runtime_factory(
            database.0.clone(),
            Arc::new(move || {
                Ok(Box::new(BlockingRuntime {
                    gate: factory_gate.clone(),
                }))
            }),
        );
        let accepted = manager
            .start_pdf(
                fixture_path()
                    .to_str()
                    .expect("fixture path should be UTF-8"),
            )
            .expect("background run should be accepted");

        wait_until(|| gate.entered.load(Ordering::Acquire));
        let observer = db::init_db(&database.0).expect("observer connection should open");
        let analyzing = get_pipeline_run(&observer, &accepted.run_id)
            .expect("active run should load")
            .expect("active run should exist");
        assert_eq!(analyzing.state, PipelineState::Analyzing);

        let events_before =
            list_pipeline_events(&observer, &accepted.run_id).expect("active events should load");
        let stale = manager
            .request_cancellation(&accepted.run_id, analyzing.state_version.saturating_sub(1))
            .expect_err("stale cancellation must fail");
        assert!(matches!(
            stale,
            DesktopJobError::Store(StoreError::Transition(
                TransitionError::ConcurrentModification { .. }
            ))
        ));
        assert_eq!(
            get_pipeline_run(&observer, &accepted.run_id)
                .expect("run should reload")
                .expect("run should exist"),
            analyzing
        );
        assert_eq!(
            list_pipeline_events(&observer, &accepted.run_id)
                .expect("events should remain unchanged"),
            events_before
        );

        let cancelling = manager
            .request_cancellation(&accepted.run_id, analyzing.state_version)
            .expect("fresh cancellation should commit");
        assert_eq!(cancelling.state, PipelineState::Cancelling);
        assert!(cancelling.cancellation_requested);
        assert_eq!(cancelling.state_version, analyzing.state_version + 1);
        assert!(get_analyzed_document(&observer, &accepted.run_id)
            .expect("analysis query should succeed")
            .is_none());

        gate.release();
        wait_until(|| {
            !manager
                .is_active(&accepted.run_id)
                .expect("registry should remain readable")
        });
        drop(observer);

        let reopened = db::init_db(&database.0).expect("database should reopen independently");
        let cancelled = get_pipeline_run(&reopened, &accepted.run_id)
            .expect("cancelled run should load")
            .expect("cancelled run should exist");
        assert_eq!(cancelled.state, PipelineState::Cancelled);
        assert!(cancelled.cancellation_requested);
        assert_eq!(cancelled.state_version, analyzing.state_version + 2);
        assert!(cancelled.completed_at.is_some());
        assert!(get_analyzed_document(&reopened, &accepted.run_id)
            .expect("analysis query should succeed")
            .is_none());
        assert!(get_summary_artifact(&reopened, &accepted.run_id)
            .expect("summary query should succeed")
            .is_none());

        let events = list_pipeline_events(&reopened, &accepted.run_id)
            .expect("cancelled events should load");
        let request_event = &events[events.len() - 2];
        let completion_event = &events[events.len() - 1];
        assert_eq!(request_event.previous_state, Some(PipelineState::Analyzing));
        assert_eq!(request_event.next_state, PipelineState::Cancelling);
        assert_eq!(
            request_event.reason.as_deref(),
            Some("cancellation_requested")
        );
        assert_eq!(
            completion_event.previous_state,
            Some(PipelineState::Cancelling)
        );
        assert_eq!(completion_event.next_state, PipelineState::Cancelled);
        assert_eq!(
            completion_event.reason.as_deref(),
            Some("cancellation_completed")
        );
        assert_eq!(
            reopened
                .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                .expect("quick_check should run"),
            "ok"
        );
    }

    #[test]
    fn stale_worker_outcome_cannot_fail_state_owned_by_a_newer_caller() {
        let database = TestDatabase::new();
        let manager = fixture_manager(&database);
        let mut conn = db::init_db(&database.0).expect("database should initialize");
        let (_, parsing) = admit_pdf_for_background(
            &mut conn,
            fixture_path()
                .to_str()
                .expect("fixture path should be UTF-8"),
        )
        .expect("newer caller should admit parsing work");
        let stale_version = parsing.state_version.saturating_sub(1);
        let events_before =
            list_pipeline_events(&conn, &parsing.run_id).expect("newer caller events should load");

        manager
            .finalize(
                &parsing.run_id,
                Ok(Err(DocumentServiceError::Continuation(
                    ContinuationPipelineError::StaleState {
                        run_id: parsing.run_id.clone(),
                        expected_version: stale_version,
                        found_version: parsing.state_version,
                    },
                ))),
            )
            .expect("stale worker finalization should be non-mutating");

        assert_eq!(
            get_pipeline_run(&conn, &parsing.run_id)
                .expect("run should reload")
                .expect("run should exist"),
            parsing
        );
        assert_eq!(
            list_pipeline_events(&conn, &parsing.run_id).expect("events should reload"),
            events_before
        );
    }

    #[test]
    fn nonstale_worker_error_fails_a_nonterminal_run() {
        let database = TestDatabase::new();
        let manager = fixture_manager(&database);
        let mut conn = db::init_db(&database.0).expect("database should initialize");
        let (_, ingested) = crate::pipeline::ingest::ingest_pdf(
            &mut conn,
            fixture_path()
                .to_str()
                .expect("fixture path should be UTF-8"),
        )
        .expect("fixture should ingest");

        manager
            .finalize(
                &ingested.run_id,
                Ok(Err(DocumentServiceError::RuntimeRequiredForBackground(
                    ingested.run_id.clone(),
                ))),
            )
            .expect("genuine worker failure should persist");

        let failed = get_pipeline_run(&conn, &ingested.run_id)
            .expect("run should reload")
            .expect("run should exist");
        assert_eq!(failed.state, PipelineState::Failed);
        assert_eq!(failed.state_version, ingested.state_version + 1);
        assert_eq!(
            failed.failure.as_ref().map(|failure| failure.code.as_str()),
            Some("BACKGROUND_RUNTIME_REQUIRED")
        );
    }
}
