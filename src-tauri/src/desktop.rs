use crate::connect::ocr_consumer::{
    parsed_document_requires_ocr, process_scanned_document, recover_ocr_handoffs,
};
use crate::pipeline::chunk::DeterministicDocumentChunker;
use crate::pipeline::contracts::{
    CompletedSummary, ModelProfileSnapshot, ModelRuntime, ModelRuntimeFailure, PipelineFailure,
    PipelineRun, PipelineStage, PipelineState, SummaryProfile,
};
use crate::pipeline::control::{CancellationToken, ExecutionControl};
use crate::pipeline::db::{self, StoreError};
use crate::pipeline::model_settings::{runtime_from_settings, runtime_from_snapshot};
use crate::pipeline::normalize::CanonicalNormalizer;
#[cfg(test)]
use crate::pipeline::parser::PdfExtractParser;
use crate::pipeline::parser::{parse_started_document, SourceParserSet};
#[cfg(test)]
use crate::pipeline::service::SummaryComponents;
use crate::pipeline::service::{
    admit_pdf_for_background, admit_retry_for_background, continuation_plan,
    continue_run_to_summary_controlled, validate_retry_for_background, ContinuationComponents,
    DocumentServiceError,
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

type RuntimeFactory = Arc<
    dyn Fn(Option<&ModelProfileSnapshot>) -> Result<Box<dyn ModelRuntime>, ModelRuntimeFailure>
        + Send
        + Sync
        + 'static,
>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundRunAccepted {
    pub run_id: String,
    pub document_id: String,
    pub original_filename: String,
    pub byte_size: u64,
    pub state: PipelineState,
    pub state_version: u32,
    pub summary_profile: SummaryProfile,
}

#[derive(Debug, Error)]
pub enum DesktopJobError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Service(#[from] DocumentServiceError),
    #[error("Model runtime configuration failed: {0:?}")]
    Runtime(#[from] ModelRuntimeFailure),
    #[error("Pipeline run {0} has no immutable model profile for continued model work")]
    RuntimeProfileUnavailable(String),
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
            Self::RuntimeProfileUnavailable(_) => "CONTINUATION_RUNTIME_PROFILE_UNAVAILABLE",
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

struct ActiveRecoveryReservations {
    active: Arc<Mutex<HashMap<String, CancellationToken>>>,
    run_ids: Vec<String>,
}

struct ActiveRunClaim {
    active: Arc<Mutex<HashMap<String, CancellationToken>>>,
    run_id: String,
    token: CancellationToken,
    armed: bool,
}

impl Drop for ActiveRunClaim {
    fn drop(&mut self) {
        if self.armed {
            if let Ok(mut active) = self.active.lock() {
                active.remove(&self.run_id);
            }
        }
    }
}

impl Drop for ActiveRecoveryReservations {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            for run_id in &self.run_ids {
                active.remove(run_id);
            }
        }
    }
}

impl DesktopJobManager {
    pub fn new(db_path: PathBuf, settings_path: PathBuf) -> Self {
        let runtime_db_path = db_path.clone();
        Self::with_runtime_factory(
            db_path,
            Arc::new(move |snapshot| match snapshot {
                Some(snapshot) => runtime_from_snapshot(snapshot, &settings_path, &runtime_db_path),
                None => runtime_from_settings(&settings_path, &runtime_db_path),
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

    pub fn start_pdf(
        &self,
        file_path: &str,
        summary_profile: SummaryProfile,
        expected_content_hash: Option<&str>,
    ) -> Result<BackgroundRunAccepted, DesktopJobError> {
        let mut runtime = (self.runtime_factory)(None)?;
        let profile_snapshot = runtime
            .profile_snapshot()
            .ok_or_else(|| ModelRuntimeFailure {
                code: "MODEL_CONFIG_INVALID".to_string(),
                message: "Desktop runtime is missing its immutable model profile".to_string(),
                recoverable: false,
                request_attempts: Vec::new(),
            })?;
        let mut conn = db::init_db(&self.db_path)?;
        let (document, run) = admit_pdf_for_background(
            &mut conn,
            file_path,
            Some(&profile_snapshot),
            summary_profile,
            expected_content_hash,
        )?;
        runtime.bind_run(&run.run_id);
        let accepted = accepted_view(&document, &run, summary_profile);
        self.spawn(run.run_id, BackgroundWork::StartedParsing, Some(runtime))?;
        Ok(accepted)
    }

    pub fn start_retry(
        &self,
        source_run_id: &str,
        expected_source_version: u32,
    ) -> Result<BackgroundRunAccepted, DesktopJobError> {
        let mut conn = db::init_db(&self.db_path)?;
        validate_retry_for_background(&conn, source_run_id, expected_source_version)?;
        let snapshot = db::get_run_model_profile(&conn, source_run_id)?.ok_or_else(|| {
            StoreError::InvalidRetrySource {
                run_id: source_run_id.to_string(),
                reason: "the failed run has no immutable model profile to inherit".to_string(),
            }
        })?;
        let summary_profile =
            db::get_run_summary_profile(&conn, source_run_id)?.ok_or_else(|| {
                StoreError::InvalidRetrySource {
                    run_id: source_run_id.to_string(),
                    reason: "the failed run has no immutable summary profile to inherit"
                        .to_string(),
                }
            })?;
        let mut runtime = (self.runtime_factory)(Some(&snapshot))?;
        runtime.health()?;
        let (document, run) =
            admit_retry_for_background(&mut conn, source_run_id, expected_source_version)?;
        runtime.bind_run(&run.run_id);
        let accepted = accepted_view(&document, &run, summary_profile);
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
        let summary_profile = db::get_run_summary_profile(&conn, run_id)?
            .ok_or_else(|| StoreError::SummaryProfileUnavailable(run_id.to_string()))?;
        let runtime = if plan.requires_runtime {
            let snapshot = continuation_runtime_snapshot(
                run_id,
                plan.checkpoint,
                db::get_run_model_profile(&conn, run_id)?,
            )?;
            let mut runtime = (self.runtime_factory)(snapshot.as_ref())?;
            runtime.health()?;
            runtime.bind_run(&run.run_id);
            Some(runtime)
        } else {
            None
        };
        let accepted = accepted_view(&document, &run, summary_profile);
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

    pub fn status_activity(
        &self,
        requested_run_id: &str,
        projected_run_id: &str,
    ) -> Result<(bool, bool), DesktopJobError> {
        let active = self
            .active
            .lock()
            .map_err(|_| DesktopJobError::RegistryUnavailable)?;
        let projected_active = active.contains_key(projected_run_id);
        Ok((
            active.contains_key(requested_run_id) || projected_active,
            projected_active,
        ))
    }

    pub(crate) fn resume_ocr_child(&self, run_id: &str) -> Result<(), DesktopJobError> {
        let mut claim = self.claim_run(run_id)?;
        let mut conn = db::init_db(&self.db_path)?;
        let run = db::rewind_interrupted_ocr_child(&mut conn, run_id)?;
        if matches!(
            run.state,
            PipelineState::Complete | PipelineState::CompleteWithWarnings
        ) {
            db::mark_ocr_handoff_completed_for_child(&conn, run_id)?;
            return Ok(());
        }
        if run.state.is_terminal() {
            return Ok(());
        }
        let plan = continuation_plan(&conn, run_id, run.state_version)
            .map_err(DocumentServiceError::from)?;
        let runtime = if plan.requires_runtime {
            let snapshot = continuation_runtime_snapshot(
                run_id,
                plan.checkpoint,
                db::get_run_model_profile(&conn, run_id)?,
            )?;
            let mut runtime = (self.runtime_factory)(snapshot.as_ref())?;
            runtime.health()?;
            runtime.bind_run(run_id);
            Some(runtime)
        } else {
            None
        };
        drop(conn);
        self.spawn_claimed(
            run.run_id,
            BackgroundWork::Continue {
                expected_state_version: run.state_version,
            },
            runtime,
            claim.token.clone(),
        )?;
        claim.armed = false;
        Ok(())
    }

    pub(crate) fn start_ocr_recovery(&self, app_data_dir: PathBuf) -> Result<(), DesktopJobError> {
        let conn = db::init_db(&self.db_path)?;
        let root_run_ids = db::list_recoverable_ocr_handoffs(&conn)?
            .into_iter()
            .filter(|handoff| handoff.phase != "child_admitted")
            .map(|handoff| handoff.root_run_id)
            .collect();
        drop(conn);
        let manager = self.clone();
        let db_path = self.db_path.clone();
        self.start_ocr_recovery_task(root_run_ids, move || {
            let mut conn = match db::init_db(&db_path) {
                Ok(conn) => conn,
                Err(error) => {
                    eprintln!("OCR restart recovery could not open its database: {error}");
                    return;
                }
            };
            let recovery = match recover_ocr_handoffs(&mut conn, &app_data_dir) {
                Ok(recovery) => recovery,
                Err(error) => {
                    eprintln!("OCR restart recovery failed: {error}");
                    return;
                }
            };
            drop(conn);
            for warning in recovery.warnings {
                eprintln!("OCR handoff remains pending after restart: {warning}");
            }
            for child_run_id in recovery.child_run_ids {
                if let Err(error) = manager.resume_ocr_child(&child_run_id) {
                    eprintln!("OCR child {child_run_id} could not resume after restart: {error}");
                }
            }
        })
    }

    fn start_ocr_recovery_task(
        &self,
        mut root_run_ids: Vec<String>,
        task: impl FnOnce() + Send + 'static,
    ) -> Result<(), DesktopJobError> {
        root_run_ids.sort();
        root_run_ids.dedup();
        {
            let mut active = self
                .active
                .lock()
                .map_err(|_| DesktopJobError::RegistryUnavailable)?;
            if let Some(run_id) = root_run_ids
                .iter()
                .find(|run_id| active.contains_key(*run_id))
            {
                return Err(DesktopJobError::AlreadyRunning(run_id.clone()));
            }
            for run_id in &root_run_ids {
                active.insert(run_id.clone(), CancellationToken::new());
            }
        }
        let reservations = ActiveRecoveryReservations {
            active: Arc::clone(&self.active),
            run_ids: root_run_ids,
        };
        Self::spawn_ocr_recovery_task(move || {
            let _reservations = reservations;
            task();
        })
        .map_err(DesktopJobError::WorkerStart)
    }

    fn spawn_ocr_recovery_task(task: impl FnOnce() + Send + 'static) -> io::Result<()> {
        thread::Builder::new()
            .name("document-summary-ocr-recovery".to_string())
            .spawn(task)
            .map(|_| ())
    }

    fn spawn(
        &self,
        run_id: String,
        work: BackgroundWork,
        runtime: Option<Box<dyn ModelRuntime>>,
    ) -> Result<(), DesktopJobError> {
        let mut claim = self.claim_run(&run_id)?;
        let active_admission = matches!(work, BackgroundWork::StartedParsing);
        if let Err(error) = self.spawn_claimed(run_id.clone(), work, runtime, claim.token.clone()) {
            drop(claim);
            if active_admission {
                self.persist_worker_start_failure(&run_id)?;
            }
            return Err(error);
        }
        claim.armed = false;
        Ok(())
    }

    fn start_admitted_ocr_child(
        &self,
        conn: &mut rusqlite::Connection,
        child_run_id: &str,
        mut runtime: Box<dyn ModelRuntime>,
    ) -> Result<(), DocumentServiceError> {
        let unavailable = |error: DesktopJobError| DocumentServiceError::OcrHandoff {
            code: "OCR_CHILD_WORKER_UNAVAILABLE".to_string(),
            message: error.to_string(),
        };
        let mut claim = self.claim_run(child_run_id).map_err(unavailable)?;
        let child = db::get_pipeline_run(conn, child_run_id)?
            .ok_or_else(|| StoreError::RunNotFound(child_run_id.to_string()))?;
        runtime.bind_run(child_run_id);
        let (parsing, _) = match db::start_parsing(conn, child_run_id, child.state_version) {
            Ok(started) => started,
            Err(error) => {
                if let Some(current) = db::get_pipeline_run(conn, child_run_id)? {
                    if current.state == PipelineState::Cancelling {
                        db::complete_cancellation(conn, child_run_id, current.state_version)?;
                    }
                }
                return Err(error.into());
            }
        };
        if let Err(error) = self.spawn_claimed(
            parsing.run_id,
            BackgroundWork::StartedParsing,
            Some(runtime),
            claim.token.clone(),
        ) {
            drop(claim);
            self.persist_worker_start_failure(child_run_id)
                .map_err(unavailable)?;
            return Err(unavailable(error));
        }
        claim.armed = false;
        Ok(())
    }

    fn claim_run(&self, run_id: &str) -> Result<ActiveRunClaim, DesktopJobError> {
        let token = CancellationToken::new();
        {
            let mut active = self
                .active
                .lock()
                .map_err(|_| DesktopJobError::RegistryUnavailable)?;
            if active.contains_key(run_id) {
                return Err(DesktopJobError::AlreadyRunning(run_id.to_string()));
            }
            active.insert(run_id.to_string(), token.clone());
        }
        Ok(ActiveRunClaim {
            active: Arc::clone(&self.active),
            run_id: run_id.to_string(),
            token,
            armed: true,
        })
    }

    fn spawn_claimed(
        &self,
        run_id: String,
        work: BackgroundWork,
        runtime: Option<Box<dyn ModelRuntime>>,
        token: CancellationToken,
    ) -> Result<(), DesktopJobError> {
        let worker_manager = self.clone();
        let worker_run_id = run_id.clone();
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

        spawn_result.map_err(DesktopJobError::WorkerStart)?;
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
        let run = db::get_pipeline_run(&conn, run_id)?
            .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
        let document = db::get_document(&conn, &run.document_id)?
            .ok_or_else(|| StoreError::DocumentNotFound(run.document_id.clone()))?;
        let parsers = SourceParserSet::new();
        let parser = parsers.select(document.source_type);
        let normalizer = CanonicalNormalizer::new();
        let interpreter = DeterministicStructureInterpreter::new();
        let chunker = DeterministicDocumentChunker::new();

        match work {
            BackgroundWork::StartedParsing => {
                let runtime = runtime.ok_or_else(|| {
                    DocumentServiceError::RuntimeRequiredForBackground(run_id.to_string())
                })?;
                if token.cancellation_requested() {
                    return Err(DocumentServiceError::CancellationObserved);
                }
                let parsed = parse_started_document(
                    &mut conn,
                    parser,
                    run_id,
                    run.state_version,
                    &document,
                )?;
                if parsed_document_requires_ocr(&parsed) {
                    let app_data_dir =
                        self.db_path
                            .parent()
                            .ok_or_else(|| DocumentServiceError::OcrHandoff {
                                code: "OCR_STORAGE_UNAVAILABLE".to_string(),
                                message: "the application data directory is unavailable"
                                    .to_string(),
                            })?;
                    let child_run_id =
                        match process_scanned_document(&mut conn, run_id, app_data_dir, token) {
                            Ok(child_run_id) => child_run_id,
                            Err(crate::connect::ocr_consumer::OcrConsumerError::Cancelled) => {
                                return Err(DocumentServiceError::CancellationObserved);
                            }
                            Err(error) => {
                                return Err(DocumentServiceError::OcrHandoff {
                                    code: "OCR_HANDOFF_FAILED".to_string(),
                                    message: error.to_string(),
                                });
                            }
                        };
                    self.start_admitted_ocr_child(&mut conn, &child_run_id, runtime)?;
                    return Err(DocumentServiceError::OcrHandoff {
                        code: "OCR_DERIVED_HANDOFF".to_string(),
                        message: format!("processing continued in derived run {child_run_id}"),
                    });
                }
                let parsed_run = db::get_pipeline_run(&conn, run_id)?
                    .ok_or_else(|| StoreError::RunNotFound(run_id.to_string()))?;
                continue_run_to_summary_controlled(
                    &mut conn,
                    run_id,
                    parsed_run.state_version,
                    ContinuationComponents {
                        parser,
                        normalizer: &normalizer,
                        interpreter: &interpreter,
                        chunker: &chunker,
                        runtime: Some(runtime.as_ref()),
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
                    parser,
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
            if matches!(
                run.state,
                PipelineState::Complete | PipelineState::CompleteWithWarnings
            ) {
                db::mark_ocr_handoff_completed_for_child(&conn, run_id)?;
            }
            return Ok(());
        }
        if let Ok(Err(error)) = &outcome {
            if error.is_concurrent_ownership_loss() {
                return Ok(());
            }
            if matches!(error, DocumentServiceError::OcrHandoff { .. })
                && db::get_ocr_handoff_for_root(&conn, run_id)?
                    .is_some_and(|handoff| handoff.phase != "failed")
            {
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

fn continuation_runtime_snapshot(
    run_id: &str,
    checkpoint: crate::pipeline::contracts::ContinuationCheckpoint,
    snapshot: Option<ModelProfileSnapshot>,
) -> Result<Option<ModelProfileSnapshot>, DesktopJobError> {
    if checkpoint.requires_existing_model_profile() && snapshot.is_none() {
        return Err(DesktopJobError::RuntimeProfileUnavailable(
            run_id.to_string(),
        ));
    }
    Ok(snapshot)
}

#[derive(Clone, Copy)]
enum BackgroundWork {
    StartedParsing,
    Continue { expected_state_version: u32 },
}

fn accepted_view(
    document: &crate::pipeline::contracts::IngestedDocument,
    run: &PipelineRun,
    summary_profile: SummaryProfile,
) -> BackgroundRunAccepted {
    BackgroundRunAccepted {
        run_id: run.run_id.clone(),
        document_id: document.document_id.clone(),
        original_filename: document.original_filename.clone(),
        byte_size: document.byte_size,
        state: run.state.clone(),
        state_version: run.state_version,
        summary_profile,
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
    use crate::pipeline::contracts::{
        ModelRequest, ModelResponse, ModelStageProfileSnapshot, SummaryPresentationMode,
    };
    use crate::pipeline::db::{
        get_analyzed_document, get_normalized_document, get_parsed_document, get_pipeline_run,
        get_summary_artifact, get_synthesized_document, list_pipeline_events,
    };
    use crate::pipeline::ingest::prepare_pdf_ingestion_with_source_type;
    use crate::pipeline::service::ContinuationPipelineError;
    use crate::pipeline::state::TransitionError;
    use std::fs;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            Some(fixture_snapshot())
        }
    }

    struct CancelOnBindRuntime {
        manager: DesktopJobManager,
        expected_state_version: u32,
    }

    impl ModelRuntime for CancelOnBindRuntime {
        fn bind_run(&mut self, run_id: &str) {
            self.manager
                .request_cancellation(run_id, self.expected_state_version)
                .expect("cancellation should commit after the child claim");
        }

        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            FixtureRuntime.generate(request)
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            FixtureRuntime.health()
        }

        fn runtime_id(&self) -> &str {
            FixtureRuntime.runtime_id()
        }

        fn model_id(&self) -> &str {
            FixtureRuntime.model_id()
        }
    }

    struct SnapshotlessFixtureRuntime;

    impl ModelRuntime for SnapshotlessFixtureRuntime {
        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            FixtureRuntime.generate(request)
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }

        fn runtime_id(&self) -> &str {
            "snapshotless-fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "snapshotless-fixture-model"
        }
    }

    struct SnapshotlessRecoverableFailureRuntime;

    impl ModelRuntime for SnapshotlessRecoverableFailureRuntime {
        fn generate(&self, _request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            Err(ModelRuntimeFailure {
                code: "FIXTURE_RUNTIME_INTERRUPTED".to_string(),
                message: "Fixture runtime stopped before producing output.".to_string(),
                recoverable: true,
                request_attempts: Vec::new(),
            })
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }

        fn runtime_id(&self) -> &str {
            "snapshotless-recoverable-fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "snapshotless-recoverable-fixture-model"
        }
    }

    struct RunBindingFixtureRuntime {
        snapshot: ModelProfileSnapshot,
        observed: Arc<StdMutex<Option<String>>>,
    }

    impl ModelRuntime for RunBindingFixtureRuntime {
        fn bind_run(&mut self, run_id: &str) {
            *self
                .observed
                .lock()
                .expect("binding observation lock should remain available") =
                Some(run_id.to_string());
        }

        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            FixtureRuntime.generate(request)
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }

        fn runtime_id(&self) -> &str {
            "run-binding-fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "run-binding-fixture-model"
        }

        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            Some(self.snapshot.clone())
        }
    }

    struct UnavailableSnapshotRuntime(ModelProfileSnapshot);

    impl ModelRuntime for UnavailableSnapshotRuntime {
        fn generate(&self, _request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            panic!("unavailable snapshot runtime must fail during admission")
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Err(ModelRuntimeFailure {
                code: "MODEL_NOT_AVAILABLE".to_string(),
                message: "Fixture snapshot model is unavailable.".to_string(),
                recoverable: true,
                request_attempts: Vec::new(),
            })
        }

        fn runtime_id(&self) -> &str {
            "unavailable-snapshot-fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "unavailable-snapshot-fixture-model"
        }

        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            Some(self.0.clone())
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

        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            Some(fixture_snapshot())
        }
    }

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf")
    }

    #[test]
    fn desktop_worker_dispatches_ocr_reparse_from_durable_source_type() {
        let database = TestDatabase::new();
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        let mut conn = db::init_db(&database.0).expect("database should initialize");
        let (document, run) = prepare_pdf_ingestion_with_source_type(
            source.to_str().expect("OCR fixture path should be UTF-8"),
            Some("recognized.pdf"),
            crate::pipeline::contracts::SourceType::OcrText,
        )
        .expect("OCR fixture should prepare");
        let ingested = db::persist_ingestion_with_profiles(
            &mut conn,
            &document,
            &run,
            Some(&fixture_snapshot()),
            SummaryProfile::General,
        )
        .expect("OCR fixture should persist");
        let parsers = SourceParserSet::new();
        let normalizer = CanonicalNormalizer::new();
        let interpreter = DeterministicStructureInterpreter::new();
        let chunker = DeterministicDocumentChunker::new();
        let failure = crate::pipeline::service::process_ingested_to_summary(
            &mut conn,
            &ingested.run_id,
            SummaryComponents {
                parser: parsers.select(document.source_type),
                normalizer: &normalizer,
                interpreter: &interpreter,
                chunker: &chunker,
                runtime: &SnapshotlessRecoverableFailureRuntime,
            },
        )
        .expect_err("fixture runtime should interrupt the OCR run");
        drop(conn);
        let manager = fixture_manager(&database);
        manager
            .finalize(&run.run_id, Ok(Err(failure)))
            .expect("desktop finalizer should make the OCR run retryable");
        let conn = db::init_db(&database.0).expect("database should reopen");
        let failed = get_pipeline_run(&conn, &run.run_id)
            .expect("failed OCR run should load")
            .expect("failed OCR run should exist");
        drop(conn);

        let accepted = manager
            .start_retry(&run.run_id, failed.state_version)
            .expect("desktop retry should accept the OCR run");
        wait_until(|| !manager.is_active(&accepted.run_id).unwrap());

        let reopened = db::init_db(&database.0).expect("database should reopen");
        let parsed = get_parsed_document(&reopened, &accepted.run_id)
            .expect("parsed artifact should load")
            .expect("parsed artifact should persist");
        let completed = get_pipeline_run(&reopened, &accepted.run_id)
            .expect("completed OCR run should load")
            .expect("completed OCR run should exist");
        assert_eq!(parsed.parser_id, "local-connect-tagged-ocr");
        assert_eq!(
            parsed.source_type,
            crate::pipeline::contracts::SourceType::OcrText
        );
        assert_eq!(completed.state, PipelineState::Complete);
        assert!(db::summary_artifact_exists(&reopened, &accepted.run_id).unwrap());
    }

    fn fixture_manager(database: &TestDatabase) -> DesktopJobManager {
        DesktopJobManager::with_runtime_factory(
            database.0.clone(),
            Arc::new(|_| Ok(Box::new(FixtureRuntime))),
        )
    }

    fn fixture_snapshot() -> ModelProfileSnapshot {
        let stage = ModelStageProfileSnapshot {
            runtime_kind: Default::default(),
            profile_id: "fixture-profile-v1".to_string(),
            model_name: "fixture-model:latest".to_string(),
            model_digest: "fixture-digest".to_string(),
            context_tokens: 8_192,
            tokenizer_version: "fixture-tokenizer-v1".to_string(),
        };
        ModelProfileSnapshot {
            version: 1,
            preset_id: "fixture-preset-v1".to_string(),
            analysis: stage.clone(),
            verification: stage,
        }
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
    fn startup_ocr_recovery_dispatch_returns_before_blocked_work_finishes() {
        let gate = Arc::new(BlockingGate::new());
        let task_gate = Arc::clone(&gate);
        let completed = Arc::new(AtomicBool::new(false));
        let task_completed = Arc::clone(&completed);

        DesktopJobManager::spawn_ocr_recovery_task(move || {
            task_gate.entered.store(true, Ordering::Release);
            let mut released = task_gate.released.lock().unwrap();
            while !*released {
                released = task_gate.release_changed.wait(released).unwrap();
            }
            task_completed.store(true, Ordering::Release);
        })
        .expect("recovery task should dispatch");

        wait_until(|| gate.entered.load(Ordering::Acquire));
        assert!(!completed.load(Ordering::Acquire));
        gate.release();
        wait_until(|| completed.load(Ordering::Acquire));
    }

    #[test]
    fn startup_ocr_recovery_reserves_roots_until_task_finishes() {
        let database = TestDatabase::new();
        let manager = fixture_manager(&database);
        let root_run_id = Uuid::new_v4().to_string();
        let gate = Arc::new(BlockingGate::new());
        let task_gate = Arc::clone(&gate);

        manager
            .start_ocr_recovery_task(vec![root_run_id.clone()], move || {
                task_gate.entered.store(true, Ordering::Release);
                let mut released = task_gate.released.lock().unwrap();
                while !*released {
                    released = task_gate.release_changed.wait(released).unwrap();
                }
            })
            .expect("recovery task should dispatch");

        wait_until(|| gate.entered.load(Ordering::Acquire));
        assert!(manager.is_active(&root_run_id).unwrap());
        let projected_child_id = Uuid::new_v4().to_string();
        assert_eq!(
            manager
                .status_activity(&root_run_id, &projected_child_id)
                .unwrap(),
            (true, false),
            "the root keeps polling alive before the child is registered"
        );
        manager
            .active
            .lock()
            .unwrap()
            .insert(projected_child_id.clone(), CancellationToken::new());
        assert_eq!(
            manager
                .status_activity(&root_run_id, &projected_child_id)
                .unwrap(),
            (true, true),
            "a registered child can be cancelled"
        );
        assert!(matches!(
            manager.spawn(
                root_run_id.clone(),
                BackgroundWork::Continue {
                    expected_state_version: 0,
                },
                None,
            ),
            Err(DesktopJobError::AlreadyRunning(run_id)) if run_id == root_run_id
        ));
        gate.release();
        wait_until(|| !manager.is_active(&root_run_id).unwrap());
        assert_eq!(
            manager
                .status_activity(&root_run_id, &projected_child_id)
                .unwrap(),
            (true, true),
            "the child keeps polling alive after the root exits"
        );
        manager.active.lock().unwrap().remove(&projected_child_id);
        assert_eq!(
            manager
                .status_activity(&root_run_id, &projected_child_id)
                .unwrap(),
            (false, false)
        );
    }

    #[test]
    fn live_ocr_child_is_claimed_before_parsing_and_worker_dispatch() {
        let database = TestDatabase::new();
        let manager = fixture_manager(&database);
        let mut conn = db::init_db(&database.0).expect("database should initialize");
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        let (document, received) = prepare_pdf_ingestion_with_source_type(
            source.to_str().expect("OCR fixture path should be UTF-8"),
            Some("recognized.pdf"),
            crate::pipeline::contracts::SourceType::OcrText,
        )
        .expect("OCR fixture should prepare");
        let child = db::persist_ingestion_with_profiles(
            &mut conn,
            &document,
            &received,
            Some(&fixture_snapshot()),
            SummaryProfile::General,
        )
        .expect("OCR child should persist");
        let before = db::get_pipeline_run(&conn, &child.run_id)
            .expect("child should load")
            .expect("child should exist");

        let competing_claim = manager.claim_run(&child.run_id).expect("user claims first");
        let blocked =
            manager.start_admitted_ocr_child(&mut conn, &child.run_id, Box::new(FixtureRuntime));
        assert!(matches!(
            blocked,
            Err(DocumentServiceError::OcrHandoff {
                code,
                ..
            }) if code == "OCR_CHILD_WORKER_UNAVAILABLE"
        ));
        assert_eq!(
            db::get_pipeline_run(&conn, &child.run_id)
                .expect("child should load")
                .expect("child should exist"),
            before,
            "losing the claim must not advance the child"
        );
        drop(competing_claim);

        manager
            .start_admitted_ocr_child(&mut conn, &child.run_id, Box::new(FixtureRuntime))
            .expect("root claims before parsing and dispatches the child");
        drop(conn);
        wait_until(|| !manager.is_active(&child.run_id).unwrap());
        let conn = db::init_db(&database.0).expect("database should reopen");
        assert_eq!(
            db::get_pipeline_run(&conn, &child.run_id)
                .expect("child should load")
                .expect("child should exist")
                .state,
            PipelineState::Complete
        );
    }

    #[test]
    fn cancelled_live_ocr_child_completes_before_claim_release() {
        let database = TestDatabase::new();
        let manager = fixture_manager(&database);
        let mut conn = db::init_db(&database.0).expect("database should initialize");
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr_tagged.pdf");
        let (document, received) = prepare_pdf_ingestion_with_source_type(
            source.to_str().expect("OCR fixture path should be UTF-8"),
            Some("recognized.pdf"),
            crate::pipeline::contracts::SourceType::OcrText,
        )
        .expect("OCR fixture should prepare");
        let child = db::persist_ingestion_with_profiles(
            &mut conn,
            &document,
            &received,
            Some(&fixture_snapshot()),
            SummaryProfile::General,
        )
        .expect("OCR child should persist");

        assert!(manager
            .start_admitted_ocr_child(
                &mut conn,
                &child.run_id,
                Box::new(CancelOnBindRuntime {
                    manager: manager.clone(),
                    expected_state_version: child.state_version,
                }),
            )
            .is_err());
        let cancelled = db::get_pipeline_run(&conn, &child.run_id)
            .expect("child should load")
            .expect("child should exist");
        assert_eq!(cancelled.state, PipelineState::Cancelled);
        assert!(!manager.is_active(&child.run_id).unwrap());
    }

    #[test]
    fn resumed_ocr_child_rewinds_active_stage_and_completes_same_run() {
        let database = TestDatabase::new();
        let manager = fixture_manager(&database);
        let mut conn = db::init_db(&database.0).expect("database should initialize");
        let source = fixture_path();
        let source = source.to_str().expect("fixture path should be UTF-8");
        let (root_document, root) =
            crate::pipeline::ingest::ingest_pdf(&mut conn, source).expect("root should ingest");
        let (_, child) =
            crate::pipeline::ingest::ingest_pdf(&mut conn, source).expect("child should ingest");
        let (parsing, document) = db::start_parsing(&mut conn, &child.run_id, child.state_version)
            .expect("child parsing should start");
        parse_started_document(
            &mut conn,
            &PdfExtractParser::new(),
            &child.run_id,
            parsing.state_version,
            &document,
        )
        .expect("child parsing should complete");
        let parsed = get_pipeline_run(&conn, &child.run_id)
            .expect("child should reload")
            .expect("child should exist");
        conn.execute(
            "UPDATE pipeline_runs
             SET state = ?1, state_version = state_version + 1, current_stage = ?2
             WHERE run_id = ?3 AND state_version = ?4",
            rusqlite::params![
                serde_json::to_string(&PipelineState::Normalizing).unwrap(),
                serde_json::to_string(&PipelineStage::Normalize).unwrap(),
                child.run_id,
                parsed.state_version,
            ],
        )
        .expect("interrupted normalizing state should persist");
        let normalizing = get_pipeline_run(&conn, &child.run_id)
            .expect("active child should reload")
            .expect("active child should exist");
        let handoff_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO ocr_handoffs (
                handoff_id, root_run_id, root_document_id, source_artifact_id,
                source_byte_size, source_sha256, source_display_name, source_bytes,
                provider_app_id, provider_instance_id, provider_job_id,
                provider_request_json, provider_request_sha256, phase,
                child_document_id, child_run_id, derived_path, created_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, 1, ?5, 'scan.pdf', X'00', 'document-ocr',
                ?6, ?7, '{}', ?5, 'child_admitted', ?8, ?9, ?10, ?11, ?11
             )",
            rusqlite::params![
                handoff_id,
                root.run_id,
                root_document.document_id,
                Uuid::new_v4().to_string(),
                "0".repeat(64),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                normalizing.document_id,
                normalizing.run_id,
                database.0.with_extension("ocr.pdf").to_string_lossy(),
                now,
            ],
        )
        .expect("admitted OCR ownership should persist");
        drop(conn);

        manager
            .active
            .lock()
            .unwrap()
            .insert(normalizing.run_id.clone(), CancellationToken::new());
        assert!(matches!(
            manager.resume_ocr_child(&normalizing.run_id),
            Err(DesktopJobError::AlreadyRunning(run_id)) if run_id == normalizing.run_id
        ));
        let unchanged = db::init_db(&database.0).expect("database should reopen");
        let active_child = get_pipeline_run(&unchanged, &normalizing.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(active_child.state, PipelineState::Normalizing);
        assert_eq!(active_child.state_version, normalizing.state_version);
        drop(unchanged);
        manager.active.lock().unwrap().remove(&normalizing.run_id);

        manager
            .resume_ocr_child(&normalizing.run_id)
            .expect("OCR child should resume from its stable checkpoint");
        wait_until(|| !manager.is_active(&normalizing.run_id).unwrap());

        let reopened = db::init_db(&database.0).expect("database should reopen");
        let completed = get_pipeline_run(&reopened, &normalizing.run_id)
            .expect("OCR child should reload")
            .expect("OCR child should exist");
        assert_eq!(completed.run_id, normalizing.run_id);
        assert!(matches!(
            completed.state,
            PipelineState::Complete | PipelineState::CompleteWithWarnings
        ));
        assert!(db::summary_artifact_exists(&reopened, &completed.run_id).unwrap());
        let handoff = db::get_ocr_handoff(&reopened, &handoff_id)
            .expect("handoff should reload")
            .expect("handoff should exist");
        assert_eq!(handoff.phase, "completed");
        assert!(list_pipeline_events(&reopened, &completed.run_id)
            .expect("events should load")
            .iter()
            .any(|event| event.reason.as_deref() == Some("ocr_child_rewound_after_restart")));
    }

    #[test]
    fn failed_ocr_child_resume_releases_its_registry_claim() {
        let database = TestDatabase::new();
        let manager = fixture_manager(&database);
        let missing_run_id = Uuid::new_v4().to_string();

        assert!(matches!(
            manager.resume_ocr_child(&missing_run_id),
            Err(DesktopJobError::Store(StoreError::InvalidOcrHandoff(_)))
        ));
        assert!(!manager.is_active(&missing_run_id).unwrap());
    }

    #[test]
    fn story_background_run_returns_before_completion_and_persists_after_reopen() {
        let database = TestDatabase::new();
        let manager = fixture_manager(&database);
        let accepted = manager
            .start_pdf(
                fixture_path()
                    .to_str()
                    .expect("fixture path should be UTF-8"),
                SummaryProfile::Story,
                None,
            )
            .expect("background run should be accepted");
        assert_eq!(accepted.state, PipelineState::Parsing);
        assert_eq!(accepted.summary_profile, SummaryProfile::Story);
        let admitted = db::init_db(&database.0).expect("admitted run should be observable");
        assert_eq!(
            db::get_run_model_profile(&admitted, &accepted.run_id)
                .expect("admitted profile should load"),
            Some(fixture_snapshot())
        );
        assert_eq!(
            db::get_run_summary_profile(&admitted, &accepted.run_id)
                .expect("admitted summary profile should load"),
            Some(SummaryProfile::Story)
        );
        drop(admitted);

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
        let artifact = get_summary_artifact(&reopened, &accepted.run_id)
            .expect("summary query should succeed")
            .expect("Story summary should persist");
        assert!(!artifact.text.is_empty());
        let synthesized = get_synthesized_document(&reopened, &accepted.run_id)
            .expect("synthesis query should succeed")
            .expect("Story synthesis should persist");
        assert_eq!(
            synthesized.presentation_mode,
            SummaryPresentationMode::Coherent
        );
        assert!(!synthesized.summary_claims.is_empty());
        assert!(!synthesized.synthesis_evidence.is_empty());
        let normalized = get_normalized_document(&reopened, &accepted.run_id)
            .expect("normalized query should succeed")
            .expect("normalized Story source should persist");
        let normalized_text = normalized
            .pages
            .iter()
            .flat_map(|page| &page.content)
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(synthesized
            .synthesis_evidence
            .iter()
            .all(|evidence| normalized_text.contains(&evidence.exact_quote)));
        assert_eq!(
            db::get_run_summary_profile(&reopened, &accepted.run_id)
                .expect("summary profile should survive reopen"),
            Some(SummaryProfile::Story)
        );
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
    fn new_desktop_worker_binds_the_admitted_run() {
        let database = TestDatabase::new();
        let observed = Arc::new(StdMutex::new(None));
        let factory_observed = Arc::clone(&observed);
        let manager = DesktopJobManager::with_runtime_factory(
            database.0.clone(),
            Arc::new(move |_| {
                Ok(Box::new(RunBindingFixtureRuntime {
                    snapshot: fixture_snapshot(),
                    observed: Arc::clone(&factory_observed),
                }))
            }),
        );

        let accepted = manager
            .start_pdf(
                fixture_path().to_str().unwrap(),
                SummaryProfile::General,
                None,
            )
            .unwrap();

        assert_eq!(*observed.lock().unwrap(), Some(accepted.run_id.clone()));
        wait_until(|| !manager.is_active(&accepted.run_id).unwrap());
    }

    #[test]
    fn desktop_start_rejects_snapshotless_runtime_before_persistence() {
        let database = TestDatabase::new();
        let manager = DesktopJobManager::with_runtime_factory(
            database.0.clone(),
            Arc::new(|_| Ok(Box::new(SnapshotlessFixtureRuntime))),
        );

        let error = manager
            .start_pdf(
                fixture_path()
                    .to_str()
                    .expect("fixture path should be UTF-8"),
                SummaryProfile::General,
                None,
            )
            .expect_err("a product desktop start must require an immutable snapshot");

        assert_eq!(error.code(), "MODEL_CONFIG_INVALID");
        assert!(!database.0.exists());
    }

    #[test]
    fn snapshotless_historical_retry_is_hidden_and_rejected_before_runtime_or_lineage() {
        let database = TestDatabase::new();
        let mut conn = db::init_db(&database.0).expect("database should initialize");
        let (_, ingested) = crate::pipeline::ingest::ingest_pdf(
            &mut conn,
            fixture_path()
                .to_str()
                .expect("fixture path should be UTF-8"),
        )
        .expect("legacy fixture should ingest without a profile");
        let parser = PdfExtractParser::new();
        let normalizer = CanonicalNormalizer::new();
        let interpreter = DeterministicStructureInterpreter::new();
        let chunker = DeterministicDocumentChunker::new();
        crate::pipeline::service::process_ingested_to_summary(
            &mut conn,
            &ingested.run_id,
            SummaryComponents {
                parser: &parser,
                normalizer: &normalizer,
                interpreter: &interpreter,
                chunker: &chunker,
                runtime: &SnapshotlessRecoverableFailureRuntime,
            },
        )
        .expect_err("snapshotless fixture should fail recoverably during model work");
        let failed = get_pipeline_run(&conn, &ingested.run_id)
            .expect("failed run should load")
            .expect("failed run should exist");
        assert!(failed.retry_checkpoint().is_some());
        assert!(db::get_run_model_profile(&conn, &failed.run_id)
            .expect("profile lookup should succeed")
            .is_none());
        let history = crate::pipeline::workspace::get_run(&conn, &failed.run_id)
            .expect("history should remain readable");
        assert!(!history.can_retry);
        let events = list_pipeline_events(&conn, &failed.run_id).expect("events should load");
        drop(conn);

        let factory_calls = Arc::new(AtomicUsize::new(0));
        let observed_factory_calls = Arc::clone(&factory_calls);
        let manager = DesktopJobManager::with_runtime_factory(
            database.0.clone(),
            Arc::new(move |_| {
                observed_factory_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(FixtureRuntime))
            }),
        );
        let rejected = manager
            .start_retry(&failed.run_id, failed.state_version)
            .expect_err("desktop must reject a retry with no inherited profile");
        assert_eq!(rejected.code(), "RETRY_NOT_ALLOWED");
        assert_eq!(factory_calls.load(Ordering::SeqCst), 0);

        let mut conn = db::init_db(&database.0).expect("database should reopen");
        let store_rejected = crate::pipeline::service::retry_failed_run_to_summary(
            &mut conn,
            &failed.run_id,
            failed.state_version,
            SummaryComponents {
                parser: &parser,
                normalizer: &normalizer,
                interpreter: &interpreter,
                chunker: &chunker,
                runtime: &FixtureRuntime,
            },
        )
        .expect_err("the retry transaction must independently reject a missing profile");
        assert_eq!(store_rejected.code(), "RETRY_NOT_ALLOWED");
        assert!(db::get_retry_lineage_for_source(&conn, &failed.run_id)
            .expect("lineage lookup should succeed")
            .is_none());
        assert_eq!(
            get_pipeline_run(&conn, &failed.run_id)
                .expect("source should reload")
                .expect("source should exist"),
            failed
        );
        assert_eq!(
            list_pipeline_events(&conn, &failed.run_id).expect("events should reload"),
            events
        );
    }

    #[test]
    fn continuation_runtime_factory_receives_the_persisted_run_snapshot() {
        let database = TestDatabase::new();
        let mut conn = db::init_db(&database.0).expect("database should initialize");
        let (_, ingested) = crate::pipeline::ingest::ingest_pdf_with_profiles(
            &mut conn,
            fixture_path()
                .to_str()
                .expect("fixture path should be UTF-8"),
            None,
            SummaryProfile::Story,
            None,
        )
        .expect("Story fixture should ingest");
        let snapshot = fixture_snapshot();
        conn.execute(
            "INSERT INTO pipeline_run_model_profiles (run_id, profile_snapshot, created_at)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![
                ingested.run_id,
                serde_json::to_string(&snapshot).expect("snapshot should serialize"),
                chrono::Utc::now().to_rfc3339()
            ],
        )
        .expect("snapshot should persist");
        drop(conn);

        let observed_snapshot = Arc::new(StdMutex::new(None));
        let factory_observed_snapshot = Arc::clone(&observed_snapshot);
        let observed_binding = Arc::new(StdMutex::new(None));
        let factory_observed_binding = Arc::clone(&observed_binding);
        let manager = DesktopJobManager::with_runtime_factory(
            database.0.clone(),
            Arc::new(move |received| {
                let received = received
                    .cloned()
                    .expect("continuation must supply its persisted snapshot");
                *factory_observed_snapshot
                    .lock()
                    .expect("observation lock should remain available") = Some(received.clone());
                Ok(Box::new(RunBindingFixtureRuntime {
                    snapshot: received,
                    observed: Arc::clone(&factory_observed_binding),
                }))
            }),
        );
        let accepted = manager
            .start_continuation(&ingested.run_id, ingested.state_version)
            .expect("continuation should be accepted");
        assert_eq!(accepted.summary_profile, SummaryProfile::Story);
        assert_eq!(
            *observed_snapshot
                .lock()
                .expect("observation lock should remain available"),
            Some(snapshot)
        );
        assert_eq!(
            *observed_binding.lock().unwrap(),
            Some(accepted.run_id.clone())
        );
        wait_until(|| {
            !manager
                .is_active(&accepted.run_id)
                .expect("registry should remain readable")
        });
    }

    #[test]
    fn retry_desktop_worker_binds_the_new_retry_run() {
        let database = TestDatabase::new();
        let mut conn = db::init_db(&database.0).unwrap();
        let (_, ingested) = crate::pipeline::ingest::ingest_pdf_with_profiles(
            &mut conn,
            fixture_path().to_str().unwrap(),
            None,
            SummaryProfile::General,
            None,
        )
        .unwrap();
        let snapshot = fixture_snapshot();
        conn.execute(
            "INSERT INTO pipeline_run_model_profiles (run_id, profile_snapshot, created_at)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![
                ingested.run_id,
                serde_json::to_string(&snapshot).unwrap(),
                chrono::Utc::now().to_rfc3339()
            ],
        )
        .unwrap();
        drop(conn);

        let fixture = fixture_manager(&database);
        fixture
            .finalize(
                &ingested.run_id,
                Ok(Err(DocumentServiceError::RuntimeRequiredForBackground(
                    ingested.run_id.clone(),
                ))),
            )
            .unwrap();
        let conn = db::init_db(&database.0).unwrap();
        let failed = get_pipeline_run(&conn, &ingested.run_id).unwrap().unwrap();
        drop(conn);

        let observed = Arc::new(StdMutex::new(None));
        let factory_observed = Arc::clone(&observed);
        let manager = DesktopJobManager::with_runtime_factory(
            database.0.clone(),
            Arc::new(move |received| {
                Ok(Box::new(RunBindingFixtureRuntime {
                    snapshot: received.cloned().unwrap(),
                    observed: Arc::clone(&factory_observed),
                }))
            }),
        );
        let accepted = manager
            .start_retry(&failed.run_id, failed.state_version)
            .unwrap();

        assert_eq!(*observed.lock().unwrap(), Some(accepted.run_id.clone()));
        wait_until(|| !manager.is_active(&accepted.run_id).unwrap());
    }

    #[test]
    fn model_artifact_continuation_requires_a_snapshot_before_worker_admission() {
        use crate::pipeline::contracts::ContinuationCheckpoint;

        for checkpoint in [
            ContinuationCheckpoint::Analyzed,
            ContinuationCheckpoint::Synthesized,
        ] {
            let error = continuation_runtime_snapshot("legacy-run", checkpoint, None)
                .expect_err("model artifacts without a profile must not be activated");
            assert_eq!(error.code(), "CONTINUATION_RUNTIME_PROFILE_UNAVAILABLE");
        }
        for checkpoint in [
            ContinuationCheckpoint::Ingested,
            ContinuationCheckpoint::Parsed,
            ContinuationCheckpoint::Normalized,
            ContinuationCheckpoint::Structured,
            ContinuationCheckpoint::Chunked,
        ] {
            assert_eq!(
                continuation_runtime_snapshot("pre-model-run", checkpoint, None)
                    .expect("pre-model checkpoints may select a runtime on continuation"),
                None
            );
        }
    }

    #[test]
    fn unavailable_snapshot_runtime_cannot_mutate_continuation_or_retry_state() {
        let database = TestDatabase::new();
        let mut conn = db::init_db(&database.0).expect("database should initialize");
        let (_, ingested) = crate::pipeline::ingest::ingest_pdf(
            &mut conn,
            fixture_path()
                .to_str()
                .expect("fixture path should be UTF-8"),
        )
        .expect("fixture should ingest");
        let snapshot = fixture_snapshot();
        conn.execute(
            "INSERT INTO pipeline_run_model_profiles (run_id, profile_snapshot, created_at)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![
                ingested.run_id,
                serde_json::to_string(&snapshot).expect("snapshot should serialize"),
                chrono::Utc::now().to_rfc3339()
            ],
        )
        .expect("snapshot should persist");
        let continuation_events =
            list_pipeline_events(&conn, &ingested.run_id).expect("events should load");
        drop(conn);

        let manager = DesktopJobManager::with_runtime_factory(
            database.0.clone(),
            Arc::new(move |received| {
                let received = received
                    .cloned()
                    .expect("recovery must use the persisted snapshot");
                Ok(Box::new(UnavailableSnapshotRuntime(received)))
            }),
        );
        let continuation_error = manager
            .start_continuation(&ingested.run_id, ingested.state_version)
            .expect_err("unavailable snapshot runtime must reject continuation admission");
        assert_eq!(continuation_error.code(), "MODEL_NOT_AVAILABLE");
        assert!(!manager
            .is_active(&ingested.run_id)
            .expect("registry should remain readable"));

        let conn = db::init_db(&database.0).expect("database should reopen");
        assert_eq!(
            get_pipeline_run(&conn, &ingested.run_id)
                .expect("run should reload")
                .expect("run should exist"),
            ingested
        );
        assert_eq!(
            list_pipeline_events(&conn, &ingested.run_id).expect("events should reload"),
            continuation_events
        );

        let invalid_factory_calls = Arc::new(AtomicUsize::new(0));
        let observed_factory_calls = Arc::clone(&invalid_factory_calls);
        let invalid_retry_manager = DesktopJobManager::with_runtime_factory(
            database.0.clone(),
            Arc::new(move |_| {
                observed_factory_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(FixtureRuntime))
            }),
        );
        let invalid_retry = invalid_retry_manager
            .start_retry(&ingested.run_id, ingested.state_version)
            .expect_err("a nonfailed source must be rejected before runtime discovery");
        assert_eq!(invalid_retry.code(), "RETRY_NOT_ALLOWED");
        assert_eq!(invalid_factory_calls.load(Ordering::SeqCst), 0);

        manager
            .finalize(
                &ingested.run_id,
                Ok(Err(DocumentServiceError::RuntimeRequiredForBackground(
                    ingested.run_id.clone(),
                ))),
            )
            .expect("fixture source failure should persist");
        let failed = get_pipeline_run(&conn, &ingested.run_id)
            .expect("failed source should load")
            .expect("failed source should exist");
        let retry_events =
            list_pipeline_events(&conn, &ingested.run_id).expect("failed events should load");

        let stale_retry = invalid_retry_manager
            .start_retry(&failed.run_id, failed.state_version - 1)
            .expect_err("a stale retry must be rejected before runtime discovery");
        assert_eq!(stale_retry.code(), "RETRY_STALE_STATE");
        assert_eq!(invalid_factory_calls.load(Ordering::SeqCst), 0);

        let retry_error = manager
            .start_retry(&failed.run_id, failed.state_version)
            .expect_err("unavailable snapshot runtime must reject retry admission");
        assert_eq!(retry_error.code(), "MODEL_NOT_AVAILABLE");
        assert_eq!(
            get_pipeline_run(&conn, &failed.run_id)
                .expect("source should reload")
                .expect("source should exist"),
            failed
        );
        assert_eq!(
            list_pipeline_events(&conn, &failed.run_id).expect("events should remain stable"),
            retry_events
        );
        assert!(db::get_retry_lineage_for_source(&conn, &failed.run_id)
            .expect("retry lineage should remain readable")
            .is_none());
    }

    #[test]
    fn cancellation_wins_atomically_during_model_work_and_survives_reopen() {
        let database = TestDatabase::new();
        let gate = Arc::new(BlockingGate::new());
        let factory_gate = gate.clone();
        let manager = DesktopJobManager::with_runtime_factory(
            database.0.clone(),
            Arc::new(move |_| {
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
                SummaryProfile::General,
                None,
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
            None,
            SummaryProfile::General,
            None,
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

    #[test]
    fn pending_ocr_recovery_keeps_the_root_checkpoint_resumable() {
        let database = TestDatabase::new();
        let manager = fixture_manager(&database);
        let mut conn = db::init_db(&database.0).expect("database should initialize");
        let (document, ingested) = crate::pipeline::ingest::ingest_pdf(
            &mut conn,
            fixture_path()
                .to_str()
                .expect("fixture path should be UTF-8"),
        )
        .expect("fixture should ingest");
        let handoff_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO ocr_handoffs (
                handoff_id, root_run_id, root_document_id, source_artifact_id,
                source_byte_size, source_sha256, source_display_name, source_bytes,
                provider_app_id, provider_instance_id, provider_job_id,
                provider_request_json, provider_request_sha256, phase,
                child_document_id, child_run_id, derived_path, created_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, 1, ?5, 'scan.pdf', X'00', 'document-ocr',
                ?6, ?7, '{}', ?5, 'submission_uncertain', ?8, ?9, ?10, ?11, ?11
             )",
            rusqlite::params![
                handoff_id,
                ingested.run_id,
                document.document_id,
                Uuid::new_v4().to_string(),
                "0".repeat(64),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                Uuid::new_v4().to_string(),
                database.0.with_extension("ocr.pdf").to_string_lossy(),
                chrono::Utc::now().to_rfc3339(),
            ],
        )
        .expect("pending handoff should persist");
        drop(conn);

        manager
            .finalize(
                &ingested.run_id,
                Ok(Err(DocumentServiceError::OcrHandoff {
                    code: "OCR_HANDOFF_FAILED".to_string(),
                    message: "status is temporarily unavailable".to_string(),
                })),
            )
            .expect("pending OCR ownership should remain resumable");

        let conn = db::init_db(&database.0).expect("database should reopen");
        let unchanged = get_pipeline_run(&conn, &ingested.run_id)
            .expect("root should reload")
            .expect("root should exist");
        assert_eq!(unchanged.state, PipelineState::Ingested);
        assert!(unchanged.failure.is_none());
    }
}
