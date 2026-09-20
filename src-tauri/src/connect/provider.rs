use crate::connect::contracts::{
    job_error, valid_uuid_v4, AppManifest, AuthRegistration, ErrorEnvelope, InputArtifact,
    JobError, JobRequest, JobResult, JobStatus, RuntimeRegistration, TransportRegistration, APP_ID,
    DEFAULT_MAX_INPUT_BYTES, MAX_REQUEST_JSON_BYTES, PROTOCOL_VERSION,
};
use crate::connect::entitlement::{EntitlementConfigurationError, EntitlementGate};
#[cfg(target_os = "linux")]
use crate::connect::lifecycle_control::{LifecycleControlError, TransitionStore};
#[cfg(target_os = "linux")]
use crate::connect::package_control;
use crate::connect::store::{self, ConnectStoreError, StoredConnectJob};
use crate::connect::v2;
#[cfg(windows)]
use crate::connect::windows_storage::{self, FileLockError, WindowsFileLock};
use crate::pipeline::chunk::DeterministicDocumentChunker;
use crate::pipeline::contracts::{
    AnalysisPageOmission, ModelRuntime, ModelRuntimeFailure, NormalizedDocument, SummaryArtifacts,
    SummaryProfile,
};
#[cfg(test)]
use crate::pipeline::contracts::{ModelProfileSnapshot, ModelStageProfileSnapshot};
use crate::pipeline::control::CancellationToken;
use crate::pipeline::db;
use crate::pipeline::ingest::prepare_pdf_ingestion;
#[cfg(feature = "connect-proof-runtime")]
use crate::pipeline::model_settings::connect_proof_runtime_from_environment;
use crate::pipeline::model_settings::{runtime_from_settings, settings_path};
use crate::pipeline::normalize::CanonicalNormalizer;
use crate::pipeline::parser::PdfExtractParser;
#[cfg(unix)]
use crate::pipeline::recovery::reconcile_interrupted_runs;
use crate::pipeline::service::{
    process_ingested_to_summary_with_delivery_policy_controlled, SummaryComponents,
};
use crate::pipeline::structure::DeterministicStructureInterpreter;
use crate::pipeline::summary::{
    delivery_claim_prefix_coverage_satisfied, render_citation_claim_lines, SummaryDeliveryPolicy,
    SummaryPipelineError,
};
use axum::extract::{DefaultBodyLimit, FromRequest, Multipart, Path as AxumPath, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::ffi::OsStr;
#[cfg(unix)]
use std::fs::TryLockError;
use std::fs::{self, File, OpenOptions};
#[cfg(windows)]
use std::future::IntoFuture;
use std::io::{self, Write};
#[cfg(unix)]
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(target_os = "linux")]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;
use uuid::Uuid;

type RuntimeFactory =
    Arc<dyn Fn() -> Result<Box<dyn ModelRuntime>, ModelRuntimeFailure> + Send + Sync + 'static>;
const V2_INSTANCE_ID_FILE: &str = "connect-v2-instance-id";
const MAX_REGISTRATION_BYTES: u64 = 64 * 1024;
#[cfg(unix)]
const MAX_PROBED_MANIFEST_BYTES: u64 = 64 * 1024;
#[cfg(unix)]
const REGISTRATION_PROBE_TIMEOUT: Duration = Duration::from_millis(750);
#[cfg(unix)]
const REGISTRATION_LOCK_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(unix)]
const REGISTRATION_LOCK_RETRY: Duration = Duration::from_millis(10);
const CONNECT_PROOF_MODE_ENV: &str = "DOC_SUM_CONNECT_PROOF_MODE";
const CONNECT_PROOF_MODE_V1: &str = "local-fixture-v1";
const CONNECT_JOB_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(35);
#[cfg(windows)]
const WINDOWS_SERVER_SHUTDOWN_GRACE: Duration = Duration::from_secs(1);

async fn wait_for_shutdown(mut receiver: watch::Receiver<bool>) {
    if *receiver.borrow() {
        return;
    }
    let _ = receiver.changed().await;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectRuntimeSource {
    PersistedSettings,
    ProofFixture,
}

fn select_connect_runtime_source(
    proof_runtime_compiled: bool,
    proof_mode: Option<&OsStr>,
) -> Result<ConnectRuntimeSource, ModelRuntimeFailure> {
    if !proof_runtime_compiled {
        return Ok(ConnectRuntimeSource::PersistedSettings);
    }
    match proof_mode {
        None => Ok(ConnectRuntimeSource::PersistedSettings),
        Some(value) if value == OsStr::new(CONNECT_PROOF_MODE_V1) => {
            Ok(ConnectRuntimeSource::ProofFixture)
        }
        Some(_) => Err(ModelRuntimeFailure {
            code: "MODEL_CONFIG_INVALID".to_string(),
            message: "Connect proof mode is not admitted".to_string(),
            recoverable: false,
            request_attempts: Vec::new(),
        }),
    }
}

fn provider_runtime_factory(
    model_settings_path: PathBuf,
    runtime_db_path: PathBuf,
) -> RuntimeFactory {
    let source = select_connect_runtime_source(
        cfg!(feature = "connect-proof-runtime"),
        env::var_os(CONNECT_PROOF_MODE_ENV).as_deref(),
    );
    Arc::new(move || match source.as_ref() {
        Ok(ConnectRuntimeSource::PersistedSettings) => {
            runtime_from_settings(&model_settings_path, &runtime_db_path)
        }
        Ok(ConnectRuntimeSource::ProofFixture) => {
            connect_proof_runtime().map(|runtime| Box::new(runtime) as Box<dyn ModelRuntime>)
        }
        Err(error) => Err(error.clone()),
    })
}

#[cfg(feature = "connect-proof-runtime")]
fn connect_proof_runtime(
) -> Result<crate::pipeline::model_settings::QwenProfileRuntime, ModelRuntimeFailure> {
    connect_proof_runtime_from_environment()
}

#[cfg(not(feature = "connect-proof-runtime"))]
fn connect_proof_runtime(
) -> Result<crate::pipeline::model_settings::QwenProfileRuntime, ModelRuntimeFailure> {
    Err(ModelRuntimeFailure {
        code: "MODEL_CONFIG_INVALID".to_string(),
        message: "Connect proof runtime is unavailable in this build".to_string(),
        recoverable: false,
        request_attempts: Vec::new(),
    })
}

#[derive(Clone)]
struct ProviderWorkerOwner {
    state: Arc<Mutex<ProviderWorkerState>>,
    cancellation: CancellationToken,
}

struct ProviderWorkerState {
    accepting: bool,
    handles: Vec<JoinHandle<()>>,
}

impl ProviderWorkerOwner {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ProviderWorkerState {
                accepting: true,
                handles: Vec::new(),
            })),
            cancellation: CancellationToken::new(),
        }
    }

    fn accepting(&self) -> bool {
        self.reap_finished();
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .accepting
    }

    fn spawn<F>(&self, name: String, worker: F) -> io::Result<()>
    where
        F: FnOnce(CancellationToken) + Send + 'static,
    {
        self.reap_finished();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.accepting {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "Connect provider is shutting down",
            ));
        }
        let cancellation = self
            .cancellation
            .child_with_deadline(CONNECT_JOB_REQUEST_TIMEOUT);
        let handle = thread::Builder::new()
            .name(name)
            .spawn(move || worker(cancellation))?;
        state.handles.push(handle);
        Ok(())
    }

    #[cfg(test)]
    fn shutdown(&self) {
        self.shutdown_until(Instant::now() + CONNECT_GRACEFUL_SHUTDOWN_TIMEOUT);
    }

    fn shutdown_until(&self, deadline: Instant) {
        let mut handles = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.accepting = false;
            std::mem::take(&mut state.handles)
        };
        self.cancellation.request();
        while !handles.is_empty() {
            let mut ordinal = 0;
            while ordinal < handles.len() {
                if handles[ordinal].is_finished() {
                    let handle = handles.swap_remove(ordinal);
                    if handle.join().is_err() {
                        eprintln!("Connect provider worker panicked during shutdown");
                    }
                } else {
                    ordinal += 1;
                }
            }
            if handles.is_empty() {
                break;
            }
            if Instant::now() >= deadline {
                eprintln!(
                    "Connect provider graceful deadline expired with {} worker(s); process shutdown will terminate them",
                    handles.len()
                );
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn reap_finished(&self) {
        let finished = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut finished = Vec::new();
            let mut ordinal = 0;
            while ordinal < state.handles.len() {
                if state.handles[ordinal].is_finished() {
                    finished.push(state.handles.swap_remove(ordinal));
                } else {
                    ordinal += 1;
                }
            }
            finished
        };
        for handle in finished {
            if handle.join().is_err() {
                eprintln!("Connect provider worker panicked after completion");
            }
        }
    }

    #[cfg(test)]
    fn retained_worker_count(&self) -> usize {
        self.reap_finished();
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .handles
            .len()
    }
}

#[derive(Clone)]
struct ProviderState {
    db_path: PathBuf,
    imports_dir: PathBuf,
    instance_id_v1: String,
    instance_id_v2: String,
    token: String,
    manifest_v1: AppManifest,
    manifest_v2: v2::AppManifest,
    max_input_bytes: u64,
    runtime_factory: RuntimeFactory,
    entitlement: EntitlementGate,
    workers: ProviderWorkerOwner,
    admission_authority: Arc<ProviderAdmissionAuthority>,
    #[cfg(target_os = "linux")]
    transition_store: Arc<TransitionStore>,
    #[cfg(all(target_os = "linux", test))]
    package_control_root: PathBuf,
}

#[derive(Default)]
struct ProviderAdmissionAuthority {
    stopped: Mutex<bool>,
}

impl ProviderAdmissionAuthority {
    fn enter(&self) -> Result<std::sync::MutexGuard<'_, bool>, ()> {
        let guard = self
            .stopped
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *guard {
            Err(())
        } else {
            Ok(guard)
        }
    }

    fn begin_stop(&self) {
        *self
            .stopped
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
    }

    fn stopped(&self) -> bool {
        *self
            .stopped
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Clone)]
pub(crate) struct ProviderStopControl {
    authority: Arc<ProviderAdmissionAuthority>,
}

impl ProviderStopControl {
    pub(crate) fn new() -> Self {
        Self {
            authority: Arc::new(ProviderAdmissionAuthority::default()),
        }
    }

    pub(crate) fn request_stop(&self) {
        self.authority.begin_stop();
    }

    pub(crate) fn requested(&self) -> bool {
        self.authority.stopped()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProviderMode {
    Foreground,
    Background,
}

struct ProviderStartup<'a> {
    mode: ProviderMode,
    stop_requested: &'a dyn Fn() -> bool,
    stop_control: Option<ProviderStopControl>,
}

#[cfg(unix)]
#[derive(Deserialize)]
struct ManifestIdentity {
    protocol_version: u32,
    instance_id: String,
    app: ManifestAppIdentity,
}

#[cfg(unix)]
#[derive(Deserialize)]
struct ManifestAppIdentity {
    id: String,
}

#[cfg(unix)]
enum ManifestProbe {
    Manifest(ManifestIdentity),
    EntitlementRequired,
}

#[derive(Deserialize)]
struct RegistrationIdentity {
    protocol_version: u32,
    instance_id: String,
    app_id: String,
    pid: u32,
    transport: RegistrationTransportIdentity,
    auth: RegistrationAuthIdentity,
}

#[cfg(unix)]
#[derive(Clone, Copy)]
pub(crate) struct ExpectedProviderProcess {
    uid: u32,
    executable_device: u64,
    executable_inode: u64,
}

#[cfg(target_os = "linux")]
pub(crate) fn expected_provider_process(
    uid: u32,
    executable: &Path,
) -> Result<ExpectedProviderProcess, ProviderStartError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(executable)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.nlink() == 0 {
        return Err(ProviderStartError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "provider executable identity is invalid",
        )));
    }
    Ok(ExpectedProviderProcess {
        uid,
        executable_device: metadata.dev(),
        executable_inode: metadata.ino(),
    })
}

#[derive(Deserialize)]
struct RegistrationTransportIdentity {
    kind: String,
    base_url: String,
}

#[derive(Deserialize)]
struct RegistrationAuthIdentity {
    scheme: String,
    token: String,
}

struct RegistrationLifecycleLock {
    #[cfg(unix)]
    file: File,
    #[cfg(windows)]
    _file: WindowsFileLock,
}

#[cfg(unix)]
impl Drop for RegistrationLifecycleLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[derive(Debug, Error)]
pub enum ProviderStartError {
    #[error("Connect runtime storage is unavailable")]
    RuntimeDirectoryUnavailable,
    #[error("Invalid DOC_SUM_CONNECT_MAX_BYTES configuration")]
    InvalidMaxInputBytes,
    #[error("Connect v2 instance identity is invalid")]
    InvalidInstanceIdentity,
    #[error("A live Document Summarizer Connect provider is already registered")]
    ProviderAlreadyRunning,
    #[error("Connect provider I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("Connect provider database setup failed: {0}")]
    Store(#[from] ConnectStoreError),
    #[error(transparent)]
    Entitlement(#[from] EntitlementConfigurationError),
    #[error("Connect registration serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Contract(#[from] crate::connect::contracts::ContractBuildError),
    #[error("Connect provider server failed to initialize: {0}")]
    Server(String),
    #[cfg(target_os = "linux")]
    #[error("Connect background lifecycle transition blocks provider startup")]
    LifecycleTransitionActive,
    #[cfg(target_os = "linux")]
    #[error("Connect background provider startup was cancelled")]
    StartupCancelled,
}

pub struct ConnectProvider {
    #[cfg(unix)]
    _provider_owner_lock: RegistrationLifecycleLock,
    #[cfg(unix)]
    registration_lock_path: PathBuf,
    #[cfg(windows)]
    _registration_lock_v1: RegistrationLifecycleLock,
    #[cfg(windows)]
    _registration_lock_v2: RegistrationLifecycleLock,
    #[cfg(windows)]
    private_storage_root: PathBuf,
    registration_path_v1: PathBuf,
    registration_path_v2: PathBuf,
    base_url: String,
    instance_id_v1: String,
    instance_id_v2: String,
    token: String,
    workers: ProviderWorkerOwner,
    admission_authority: Arc<ProviderAdmissionAuthority>,
    shutdown: Option<watch::Sender<bool>>,
    terminal_result: Mutex<mpsc::Receiver<Result<(), String>>>,
    #[cfg(test)]
    terminal_result_probe: mpsc::SyncSender<Result<(), String>>,
    server_thread: Option<JoinHandle<()>>,
    stopped: bool,
}

impl ConnectProvider {
    pub fn start(db_path: PathBuf, app_data_dir: PathBuf) -> Result<Self, ProviderStartError> {
        Self::start_in_mode(
            db_path,
            app_data_dir,
            ProviderMode::Foreground,
            &|| false,
            None,
        )
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn start_background_with_control(
        db_path: PathBuf,
        app_data_dir: PathBuf,
        stop_control: ProviderStopControl,
    ) -> Result<Self, ProviderStartError> {
        let probe_control = stop_control.clone();
        let stop_probe = || probe_control.requested();
        Self::start_in_mode(
            db_path,
            app_data_dir,
            ProviderMode::Background,
            &stop_probe,
            Some(stop_control),
        )
    }

    fn start_in_mode(
        db_path: PathBuf,
        app_data_dir: PathBuf,
        mode: ProviderMode,
        stop_requested: &dyn Fn() -> bool,
        stop_control: Option<ProviderStopControl>,
    ) -> Result<Self, ProviderStartError> {
        #[cfg(unix)]
        let runtime_root = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .ok_or(ProviderStartError::RuntimeDirectoryUnavailable)?;
        #[cfg(windows)]
        let runtime_root = windows_storage::local_app_data_root(env::var_os("LOCALAPPDATA"))
            .map_err(|_| ProviderStartError::RuntimeDirectoryUnavailable)?;
        let max_input_bytes = match env::var("DOC_SUM_CONNECT_MAX_BYTES") {
            Ok(value) => value
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0 && *value <= DEFAULT_MAX_INPUT_BYTES)
                .ok_or(ProviderStartError::InvalidMaxInputBytes)?,
            Err(_) => DEFAULT_MAX_INPUT_BYTES,
        };
        let runtime_factory =
            provider_runtime_factory(settings_path(&app_data_dir), db_path.clone());
        let entitlement = EntitlementGate::from_installation()?;
        Self::start_at_with_entitlement_mode(
            db_path,
            app_data_dir,
            runtime_root,
            max_input_bytes,
            runtime_factory,
            entitlement,
            ProviderStartup {
                mode,
                stop_requested,
                stop_control,
            },
        )
    }

    #[cfg(test)]
    fn start_at(
        db_path: PathBuf,
        app_data_dir: PathBuf,
        runtime_root: PathBuf,
        max_input_bytes: u64,
        runtime_factory: RuntimeFactory,
    ) -> Result<Self, ProviderStartError> {
        #[cfg(windows)]
        fs::create_dir_all(&runtime_root)?;
        Self::start_at_with_entitlement(
            db_path,
            app_data_dir,
            runtime_root,
            max_input_bytes,
            runtime_factory,
            EntitlementGate::always_active_for_test(),
        )
    }

    #[cfg(test)]
    fn start_at_with_entitlement(
        db_path: PathBuf,
        app_data_dir: PathBuf,
        runtime_root: PathBuf,
        max_input_bytes: u64,
        runtime_factory: RuntimeFactory,
        entitlement: EntitlementGate,
    ) -> Result<Self, ProviderStartError> {
        Self::start_at_with_entitlement_mode(
            db_path,
            app_data_dir,
            runtime_root,
            max_input_bytes,
            runtime_factory,
            entitlement,
            ProviderStartup {
                mode: ProviderMode::Foreground,
                stop_requested: &|| false,
                stop_control: None,
            },
        )
    }

    fn start_at_with_entitlement_mode(
        db_path: PathBuf,
        app_data_dir: PathBuf,
        runtime_root: PathBuf,
        max_input_bytes: u64,
        runtime_factory: RuntimeFactory,
        entitlement: EntitlementGate,
        startup: ProviderStartup<'_>,
    ) -> Result<Self, ProviderStartError> {
        #[cfg(target_os = "linux")]
        if (startup.stop_requested)() {
            return Err(ProviderStartError::StartupCancelled);
        }
        ensure_private_directory(&app_data_dir)?;
        #[cfg(target_os = "linux")]
        let _package_admission = package_admission_for_start(&app_data_dir, &runtime_root)?;
        #[cfg(target_os = "linux")]
        let transition_store = transition_store_for_start(&app_data_dir)?;
        #[cfg(target_os = "linux")]
        let transition_generation = if startup.mode == ProviderMode::Background {
            transition_store
                .current()
                .map_err(map_lifecycle_control_error)?
                .map(|record| record.generation)
        } else {
            None
        };
        #[cfg(target_os = "linux")]
        if !transition_store
            .admitted_transition_child(transition_generation.as_deref())
            .map_err(map_lifecycle_control_error)?
        {
            return Err(ProviderStartError::LifecycleTransitionActive);
        }
        #[cfg(unix)]
        let provider_owner_lock =
            acquire_provider_ownership_lock(&app_data_dir.join(".connect-provider-owner.lock"))?;
        #[cfg(target_os = "linux")]
        if (startup.stop_requested)() {
            return Err(ProviderStartError::StartupCancelled);
        }
        let imports_dir = app_data_dir.join("connect-imports");
        ensure_private_directory(&imports_dir)?;

        #[cfg(unix)]
        let providers_dir_v1 = runtime_root.join("local-connect/v1/providers");
        #[cfg(unix)]
        let providers_dir_v2 = runtime_root.join("local-connect/v2/providers");
        #[cfg(unix)]
        ensure_private_directory(&providers_dir_v1)?;
        #[cfg(unix)]
        ensure_private_directory(&providers_dir_v2)?;
        #[cfg(unix)]
        let registration_lock_path = providers_dir_v1.join(format!(".{APP_ID}.lifecycle.lock"));
        #[cfg(unix)]
        let registration_lock = acquire_registration_lock(&registration_lock_path, None, None)?;
        #[cfg(unix)]
        let removed_registrations = scavenge_stale_registrations(
            &registration_lock,
            [
                (&providers_dir_v1, WireVersion::V1),
                (&providers_dir_v2, WireVersion::V2),
            ],
            None,
        )?;
        #[cfg(unix)]
        if removed_registrations > 0 {
            eprintln!("Removed {removed_registrations} stale Connect registration(s)");
        }

        #[cfg(windows)]
        let private_connect_root = windows_storage::prepare_local_connect_root(&runtime_root)?;
        #[cfg(windows)]
        let providers_dir_v1 = private_connect_root.join("runtime/v1/providers");
        #[cfg(windows)]
        let providers_dir_v2 = private_connect_root.join("runtime/v2/providers");
        #[cfg(windows)]
        let locks_dir_v1 = private_connect_root.join("runtime/v1/locks");
        #[cfg(windows)]
        let locks_dir_v2 = private_connect_root.join("runtime/v2/locks");
        #[cfg(windows)]
        for directory in [
            &providers_dir_v1,
            &providers_dir_v2,
            &locks_dir_v1,
            &locks_dir_v2,
        ] {
            windows_storage::ensure_private_directory(directory, &runtime_root)?;
        }

        #[cfg(unix)]
        let mut conn = db::init_db(&db_path).map_err(ConnectStoreError::from)?;
        #[cfg(not(unix))]
        let conn = db::init_db(&db_path).map_err(ConnectStoreError::from)?;
        #[cfg(unix)]
        let recovered_runs =
            reconcile_interrupted_runs(&mut conn).map_err(ConnectStoreError::from)?;
        #[cfg(unix)]
        if !recovered_runs.is_empty() {
            eprintln!(
                "Reconciled {} interrupted pipeline run(s) before Connect publication",
                recovered_runs.len()
            );
        }
        let active_v2_instance_id = store::active_v2_provider_instance_id(&conn)?;
        let instance_id_v2 =
            load_or_create_v2_instance_id(&app_data_dir, active_v2_instance_id.as_deref())?;

        #[cfg(windows)]
        let registration_lock_v1 = acquire_registration_lock(
            &locks_dir_v1.join(format!(".local-connect-v1-{APP_ID}.lock")),
            Some(&runtime_root),
            None,
        )
        .map_err(map_windows_registration_lock_error)?;
        #[cfg(windows)]
        let registration_lock_v2 = acquire_registration_lock(
            &locks_dir_v2.join(format!(".local-connect-v2-{instance_id_v2}.lock")),
            Some(&runtime_root),
            None,
        )
        .map_err(map_windows_registration_lock_error)?;

        store::mark_interrupted_jobs_failed(
            &conn,
            &job_error(
                "PROVIDER_RESTARTED",
                "The provider restarted before the job completed.",
                true,
            ),
        )?;
        drop(conn);

        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let base_url = format!("http://127.0.0.1:{port}/");
        let instance_id_v1 = Uuid::new_v4().to_string();
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let manifest_v1 = AppManifest::new(&instance_id_v1, max_input_bytes);
        let manifest_v2 = v2::AppManifest::new(&instance_id_v2, max_input_bytes);
        let workers = ProviderWorkerOwner::new();
        let admission_authority = startup
            .stop_control
            .as_ref()
            .map(|control| Arc::clone(&control.authority))
            .unwrap_or_else(|| Arc::new(ProviderAdmissionAuthority::default()));
        let state = ProviderState {
            db_path,
            imports_dir,
            instance_id_v1: instance_id_v1.clone(),
            instance_id_v2: instance_id_v2.clone(),
            token: token.clone(),
            manifest_v1,
            manifest_v2,
            max_input_bytes,
            runtime_factory,
            entitlement,
            workers: workers.clone(),
            admission_authority: Arc::clone(&admission_authority),
            #[cfg(target_os = "linux")]
            transition_store: Arc::clone(&transition_store),
            #[cfg(all(target_os = "linux", test))]
            package_control_root: app_data_dir.join("test-package-control"),
        };
        let body_limit = usize::try_from(max_input_bytes)
            .unwrap_or(usize::MAX)
            .saturating_add(MAX_REQUEST_JSON_BYTES as usize)
            .saturating_add(1024 * 1024);
        let app = Router::new()
            .route("/v1/manifest", get(get_manifest))
            .route("/v1/jobs", post(create_job))
            .route("/v1/jobs/{job_id}", get(get_job_status))
            .route("/v2/manifest", get(get_manifest_v2))
            .route("/v2/jobs", post(create_job_v2))
            .route("/v2/jobs/{job_id}", get(get_job_status_v2))
            .layer(DefaultBodyLimit::max(body_limit))
            .with_state(state);

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (terminal_tx, terminal_rx) = mpsc::sync_channel(1);
        #[cfg(test)]
        let terminal_result_probe = terminal_tx.clone();
        let server_thread = thread::Builder::new()
            .name("document-summarizer-connect".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let runtime = match runtime {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                        let _ = terminal_tx.send(Err(error.to_string()));
                        return;
                    }
                };
                runtime.block_on(async move {
                    let listener = match tokio::net::TcpListener::from_std(listener) {
                        Ok(listener) => listener,
                        Err(error) => {
                            let _ = ready_tx.send(Err(error.to_string()));
                            let _ = terminal_tx.send(Err(error.to_string()));
                            return;
                        }
                    };
                    if ready_tx.send(Ok(())).is_err() {
                        return;
                    }
                    #[cfg(windows)]
                    let graceful_shutdown_rx = shutdown_rx.clone();
                    #[cfg(unix)]
                    let graceful_shutdown_rx = shutdown_rx;
                    let server = axum::serve(listener, app)
                        .with_graceful_shutdown(wait_for_shutdown(graceful_shutdown_rx));
                    #[cfg(windows)]
                    let server = server.into_future();
                    #[cfg(windows)]
                    let result = {
                        tokio::pin!(server);
                        tokio::select! {
                            result = &mut server => result,
                            () = async move {
                                wait_for_shutdown(shutdown_rx).await;
                                tokio::time::sleep(WINDOWS_SERVER_SHUTDOWN_GRACE).await;
                            } => {
                                eprintln!("Connect provider forced outstanding Windows connections closed after the shutdown grace period");
                                return;
                            }
                        }
                    };
                    #[cfg(unix)]
                    let result = server.await;
                    if let Err(error) = result {
                        eprintln!("Connect provider stopped with an error: {error}");
                        let _ = terminal_tx.send(Err(error.to_string()));
                    } else {
                        let _ = terminal_tx.send(Ok(()));
                    }
                });
            })?;

        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                let _ = server_thread.join();
                return Err(ProviderStartError::Server(error));
            }
            Err(error) => {
                let _ = server_thread.join();
                return Err(ProviderStartError::Server(error.to_string()));
            }
        }

        #[cfg(target_os = "linux")]
        if (startup.stop_requested)() {
            let _ = shutdown_tx.send(true);
            let _ = server_thread.join();
            return Err(ProviderStartError::StartupCancelled);
        }

        let started_at = Utc::now();
        let registration_v1 = RuntimeRegistration {
            protocol_version: PROTOCOL_VERSION,
            instance_id: instance_id_v1.clone(),
            app_id: APP_ID.to_string(),
            pid: std::process::id(),
            started_at,
            transport: TransportRegistration {
                kind: "http-loopback-v1".to_string(),
                base_url: base_url.clone(),
            },
            auth: AuthRegistration {
                scheme: "bearer".to_string(),
                token: token.clone(),
            },
        };
        let registration_v2 = v2::RuntimeRegistration {
            protocol_version: v2::PROTOCOL_VERSION,
            instance_id: instance_id_v2.clone(),
            app_id: APP_ID.to_string(),
            pid: std::process::id(),
            started_at,
            transport: TransportRegistration {
                kind: v2::TRANSPORT_KIND.to_string(),
                base_url: base_url.clone(),
            },
            auth: AuthRegistration {
                scheme: "bearer".to_string(),
                token: token.clone(),
            },
        };
        #[cfg(unix)]
        let registration_path_v1 = providers_dir_v1.join(format!("{APP_ID}-{instance_id_v1}.json"));
        #[cfg(unix)]
        let registration_path_v2 = providers_dir_v2.join(format!("{APP_ID}-{instance_id_v2}.json"));
        #[cfg(windows)]
        let registration_path_v1 = providers_dir_v1.join(format!("local-connect-v1-{APP_ID}.json"));
        #[cfg(windows)]
        let registration_path_v2 =
            providers_dir_v2.join(format!("local-connect-v2-{instance_id_v2}.json"));
        #[cfg(unix)]
        let publication_lock_v1 = &registration_lock;
        #[cfg(unix)]
        let publication_lock_v2 = &registration_lock;
        #[cfg(windows)]
        let publication_lock_v1 = &registration_lock_v1;
        #[cfg(windows)]
        let publication_lock_v2 = &registration_lock_v2;
        #[cfg(unix)]
        let publication_root = None;
        #[cfg(windows)]
        let publication_root = Some(runtime_root.as_path());
        let publication_authority = admission_authority
            .enter()
            .map_err(|()| ProviderStartError::StartupCancelled)?;
        #[cfg(target_os = "linux")]
        let cancellation_requested = startup.stop_control.is_none() && (startup.stop_requested)();
        #[cfg(target_os = "linux")]
        if cancellation_requested
            || !transition_store
                .admitted_transition_child(transition_generation.as_deref())
                .map_err(map_lifecycle_control_error)?
        {
            let _ = shutdown_tx.send(true);
            let _ = server_thread.join();
            return Err(if cancellation_requested {
                ProviderStartError::StartupCancelled
            } else {
                ProviderStartError::LifecycleTransitionActive
            });
        }
        if let Err(error) = write_registration(
            publication_lock_v1,
            &registration_path_v1,
            &registration_v1,
            publication_root,
        ) {
            let _ = shutdown_tx.send(true);
            let _ = server_thread.join();
            return Err(error);
        }
        #[cfg(target_os = "linux")]
        let cancellation_requested = startup.stop_control.is_none() && (startup.stop_requested)();
        #[cfg(target_os = "linux")]
        if cancellation_requested
            || !transition_store
                .admitted_transition_child(transition_generation.as_deref())
                .map_err(map_lifecycle_control_error)?
        {
            let _ = fs::remove_file(&registration_path_v1);
            let _ = shutdown_tx.send(true);
            let _ = server_thread.join();
            return Err(if cancellation_requested {
                ProviderStartError::StartupCancelled
            } else {
                ProviderStartError::LifecycleTransitionActive
            });
        }
        if let Err(error) = write_registration(
            publication_lock_v2,
            &registration_path_v2,
            &registration_v2,
            publication_root,
        ) {
            #[cfg(unix)]
            let _ = fs::remove_file(&registration_path_v1);
            #[cfg(windows)]
            let _ = windows_storage::remove_private_file(&registration_path_v1, &runtime_root);
            let _ = shutdown_tx.send(true);
            let _ = server_thread.join();
            return Err(error);
        }
        drop(publication_authority);

        Ok(Self {
            #[cfg(unix)]
            _provider_owner_lock: provider_owner_lock,
            #[cfg(unix)]
            registration_lock_path,
            #[cfg(windows)]
            _registration_lock_v1: registration_lock_v1,
            #[cfg(windows)]
            _registration_lock_v2: registration_lock_v2,
            #[cfg(windows)]
            private_storage_root: runtime_root,
            registration_path_v1,
            registration_path_v2,
            base_url,
            instance_id_v1,
            instance_id_v2,
            token,
            workers,
            admission_authority,
            shutdown: Some(shutdown_tx),
            terminal_result: Mutex::new(terminal_rx),
            #[cfg(test)]
            terminal_result_probe,
            server_thread: Some(server_thread),
            stopped: false,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id_v1
    }

    pub fn instance_id_v2(&self) -> &str {
        &self.instance_id_v2
    }

    pub fn registration_path(&self) -> &Path {
        &self.registration_path_v1
    }

    pub fn registration_path_v2(&self) -> &Path {
        &self.registration_path_v2
    }

    pub(crate) fn terminal_failure(&self) -> Option<ProviderStartError> {
        let receiver = self
            .terminal_result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match receiver.try_recv() {
            Ok(Err(error)) => Some(ProviderStartError::Server(error)),
            Ok(Ok(())) => Some(ProviderStartError::Server(
                "Connect provider server stopped unexpectedly".to_string(),
            )),
            Err(mpsc::TryRecvError::Disconnected) => Some(ProviderStartError::Server(
                "Connect provider server result channel closed".to_string(),
            )),
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }

    #[cfg(test)]
    fn force_terminal_failure(&self) {
        let _ = self
            .terminal_result_probe
            .send(Err("forced server failure".to_string()));
    }

    pub(crate) fn unregister(&self) {
        #[cfg(unix)]
        let registration_lock =
            match acquire_registration_lock(&self.registration_lock_path, None, None) {
                Ok(lock) => lock,
                Err(error) => {
                    eprintln!("Connect registration cleanup lock failed: {error}");
                    return;
                }
            };
        #[cfg(unix)]
        let cleanup_lock_v1 = &registration_lock;
        #[cfg(unix)]
        let cleanup_lock_v2 = &registration_lock;
        #[cfg(windows)]
        let cleanup_lock_v1 = &self._registration_lock_v1;
        #[cfg(windows)]
        let cleanup_lock_v2 = &self._registration_lock_v2;
        #[cfg(unix)]
        let cleanup_root = None;
        #[cfg(windows)]
        let cleanup_root = Some(self.private_storage_root.as_path());
        for (path, version, instance_id) in [
            (
                &self.registration_path_v1,
                WireVersion::V1,
                self.instance_id_v1.as_str(),
            ),
            (
                &self.registration_path_v2,
                WireVersion::V2,
                self.instance_id_v2.as_str(),
            ),
        ] {
            let cleanup_lock = match version {
                WireVersion::V1 => cleanup_lock_v1,
                WireVersion::V2 => cleanup_lock_v2,
            };
            if let Err(error) = remove_registration_if_owned(
                cleanup_lock,
                path,
                version,
                instance_id,
                &self.base_url,
                &self.token,
                cleanup_root,
            ) {
                eprintln!("Connect registration cleanup failed: {error}");
            }
        }
    }

    fn shutdown_inner(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.admission_authority.begin_stop();
        let deadline = Instant::now() + CONNECT_GRACEFUL_SHUTDOWN_TIMEOUT;
        self.workers.shutdown_until(deadline);
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(true);
        }
        if let Some(server_thread) = self.server_thread.take() {
            while !server_thread.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if server_thread.is_finished() {
                if server_thread.join().is_err() {
                    eprintln!("Connect provider server panicked during shutdown");
                }
            } else {
                eprintln!(
                    "Connect provider server exceeded the graceful deadline; process shutdown will terminate it"
                );
            }
        }
        self.unregister();
    }

    pub fn shutdown(mut self) {
        self.shutdown_inner();
    }

    #[cfg(test)]
    fn simulate_process_loss(mut self) {
        self.workers.shutdown();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(true);
        }
        if let Some(server_thread) = self.server_thread.take() {
            let _ = server_thread.join();
        }
        self.stopped = true;
    }
}

#[cfg(unix)]
pub(crate) fn reconcile_standalone_state_if_unowned(
    db_path: &Path,
    app_data_dir: &Path,
) -> Result<Option<usize>, ProviderStartError> {
    ensure_private_directory(app_data_dir)?;
    let _owner =
        match acquire_provider_ownership_lock(&app_data_dir.join(".connect-provider-owner.lock")) {
            Ok(owner) => owner,
            Err(ProviderStartError::ProviderAlreadyRunning) => return Ok(None),
            Err(error) => return Err(error),
        };
    let mut conn = db::init_db(db_path).map_err(ConnectStoreError::from)?;
    Ok(Some(
        reconcile_interrupted_runs(&mut conn)
            .map_err(ConnectStoreError::from)?
            .len(),
    ))
}

#[cfg(target_os = "linux")]
fn transition_store_for_start(
    app_data_dir: &Path,
) -> Result<Arc<TransitionStore>, ProviderStartError> {
    #[cfg(test)]
    let store = TransitionStore::new(app_data_dir.join("test-lifecycle-control"));
    #[cfg(not(test))]
    let store = {
        let _ = app_data_dir;
        TransitionStore::from_environment()
    };
    store.map(Arc::new).map_err(map_lifecycle_control_error)
}

#[cfg(target_os = "linux")]
fn package_admission_for_start(
    app_data_dir: &Path,
    runtime_root: &Path,
) -> Result<package_control::PackageAdmissionGuard, ProviderStartError> {
    #[cfg(test)]
    let guard = package_control::enter_startup_admission_at(
        &app_data_dir.join("test-package-control"),
        app_data_dir,
        runtime_root,
    );
    #[cfg(not(test))]
    let guard = { package_control::enter_startup_admission(app_data_dir, runtime_root) };
    guard.map_err(|_| ProviderStartError::LifecycleTransitionActive)
}

#[cfg(target_os = "linux")]
fn package_admission_for_job(
    state: &ProviderState,
) -> Result<package_control::PackageAdmissionGuard, package_control::PackageControlError> {
    #[cfg(test)]
    return package_control::enter_admission_at(&state.package_control_root);
    #[cfg(not(test))]
    {
        let _ = state;
        package_control::enter_admission()
    }
}

#[cfg(target_os = "linux")]
fn map_lifecycle_control_error(_error: LifecycleControlError) -> ProviderStartError {
    ProviderStartError::LifecycleTransitionActive
}

impl Drop for ConnectProvider {
    fn drop(&mut self) {
        self.shutdown_inner();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WireVersion {
    V1,
    V2,
}

impl WireVersion {
    fn protocol_version(self) -> u32 {
        match self {
            Self::V1 => PROTOCOL_VERSION,
            Self::V2 => v2::PROTOCOL_VERSION,
        }
    }

    fn transport_kind(self) -> &'static str {
        match self {
            Self::V1 => "http-loopback-v1",
            Self::V2 => v2::TRANSPORT_KIND,
        }
    }

    fn manifest_path(self) -> &'static str {
        match self {
            Self::V1 => "/v1/manifest",
            Self::V2 => "/v2/manifest",
        }
    }
}

fn parse_job_request(
    bytes: &[u8],
    version: WireVersion,
    max_input_bytes: u64,
) -> Result<(JobRequest, String, SummaryProfile), ProviderHttpError> {
    match version {
        WireVersion::V1 => {
            let request: JobRequest = serde_json::from_slice(bytes).map_err(|_| {
                ProviderHttpError::bad_request("REQUEST_INVALID", "The request JSON is invalid.")
            })?;
            request
                .validate(max_input_bytes)
                .map_err(ProviderHttpError::from_job_error)?;
            let request_hash = request.canonical_hash().map_err(ProviderHttpError::json)?;
            Ok((request, request_hash, SummaryProfile::General))
        }
        WireVersion::V2 => {
            let request: v2::JobRequest = serde_json::from_slice(bytes).map_err(|_| {
                ProviderHttpError::bad_request("REQUEST_INVALID", "The request JSON is invalid.")
            })?;
            request
                .validate(max_input_bytes)
                .map_err(ProviderHttpError::from_job_error)?;
            let summary_profile = request
                .summary_profile()
                .map_err(ProviderHttpError::from_job_error)?;
            let request_hash = request.canonical_hash().map_err(ProviderHttpError::json)?;
            let mut internal = request.as_internal();
            internal.protocol_version = v2::PROTOCOL_VERSION;
            Ok((internal, request_hash, summary_profile))
        }
    }
}

fn job_response(
    version: WireVersion,
    status: StatusCode,
    job: &StoredConnectJob,
) -> Result<Response, ProviderHttpError> {
    match version {
        WireVersion::V1 => Ok((status, Json(job.status())).into_response()),
        WireVersion::V2 => {
            let output = v2::JobStatus::from_v1(job.status()).map_err(|error| {
                ProviderHttpError::contract(error).with_protocol_version(v2::PROTOCOL_VERSION)
            })?;
            Ok((status, Json(output)).into_response())
        }
    }
}

async fn get_manifest(
    State(state): State<ProviderState>,
    headers: HeaderMap,
) -> Result<Json<AppManifest>, ProviderHttpError> {
    authorize(&state, &headers)?;
    require_entitlement(&state)?;
    Ok(Json(state.manifest_v1))
}

async fn get_manifest_v2(
    State(state): State<ProviderState>,
    headers: HeaderMap,
) -> Result<Json<v2::AppManifest>, ProviderHttpError> {
    authorize(&state, &headers)
        .map_err(|error| error.with_protocol_version(v2::PROTOCOL_VERSION))?;
    require_entitlement(&state)
        .map_err(|error| error.with_protocol_version(v2::PROTOCOL_VERSION))?;
    Ok(Json(state.manifest_v2))
}

async fn get_job_status(
    State(state): State<ProviderState>,
    headers: HeaderMap,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<JobStatus>, ProviderHttpError> {
    authorize(&state, &headers)?;
    require_entitlement(&state)?;
    state.workers.reap_finished();
    if !valid_uuid_v4(&job_id) {
        return Err(ProviderHttpError::bad_request(
            "JOB_ID_INVALID",
            "The job identifier is invalid.",
        ));
    }
    let conn = db::init_db(&state.db_path).map_err(ProviderHttpError::store)?;
    let job = store::get_job(&conn, &job_id)
        .map_err(ProviderHttpError::store)?
        .filter(|job| job.protocol_version == PROTOCOL_VERSION)
        .ok_or_else(|| {
            ProviderHttpError::new(
                StatusCode::NOT_FOUND,
                "JOB_NOT_FOUND",
                "The requested job does not exist.",
                false,
            )
        })?;
    Ok(Json(job.status()))
}

async fn get_job_status_v2(
    State(state): State<ProviderState>,
    headers: HeaderMap,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Response, ProviderHttpError> {
    let result = (|| {
        authorize(&state, &headers)?;
        require_entitlement(&state)?;
        state.workers.reap_finished();
        if !valid_uuid_v4(&job_id) {
            return Err(ProviderHttpError::bad_request(
                "JOB_ID_INVALID",
                "The job identifier is invalid.",
            ));
        }
        let conn = db::init_db(&state.db_path).map_err(ProviderHttpError::store)?;
        let job = store::get_job(&conn, &job_id)
            .map_err(ProviderHttpError::store)?
            .filter(|job| job.protocol_version == v2::PROTOCOL_VERSION)
            .ok_or_else(|| {
                ProviderHttpError::new(
                    StatusCode::NOT_FOUND,
                    "JOB_NOT_FOUND",
                    "The requested job does not exist.",
                    false,
                )
            })?;
        job_response(WireVersion::V2, StatusCode::OK, &job)
    })();
    result.map_err(|error| error.with_protocol_version(v2::PROTOCOL_VERSION))
}

async fn create_job(
    State(state): State<ProviderState>,
    request: Request,
) -> Result<Response, ProviderHttpError> {
    create_job_for_request(WireVersion::V1, state, request).await
}

async fn create_job_v2(
    State(state): State<ProviderState>,
    request: Request,
) -> Result<Response, ProviderHttpError> {
    create_job_for_request(WireVersion::V2, state, request)
        .await
        .map_err(|error| error.with_protocol_version(v2::PROTOCOL_VERSION))
}

async fn create_job_for_request(
    version: WireVersion,
    state: ProviderState,
    request: Request,
) -> Result<Response, ProviderHttpError> {
    authorize(&state, request.headers())?;
    require_entitlement(&state)?;
    if !state.workers.accepting() {
        return Err(ProviderHttpError::new(
            StatusCode::CONFLICT,
            "PROVIDER_BUSY",
            "The provider is shutting down.",
            true,
        ));
    }
    tokio::time::timeout(CONNECT_JOB_REQUEST_TIMEOUT, async {
        let multipart = Multipart::from_request(request, &state)
            .await
            .map_err(ProviderHttpError::multipart)?;
        create_job_for(version, state, multipart).await
    })
    .await
    .map_err(|_| {
        ProviderHttpError::new(
            StatusCode::REQUEST_TIMEOUT,
            "REQUEST_TIMEOUT",
            "The provider request body did not arrive within the allowed time.",
            true,
        )
    })?
}

async fn create_job_for(
    version: WireVersion,
    state: ProviderState,
    mut multipart: Multipart,
) -> Result<Response, ProviderHttpError> {
    let request_field = multipart
        .next_field()
        .await
        .map_err(ProviderHttpError::multipart)?
        .ok_or_else(|| {
            ProviderHttpError::bad_request("REQUEST_MISSING", "The request part is missing.")
        })?;
    if request_field.name() != Some("request") {
        return Err(ProviderHttpError::bad_request(
            "REQUEST_ORDER_INVALID",
            "The request JSON must be the first multipart field.",
        ));
    }
    let request_bytes = read_field_limited(request_field, MAX_REQUEST_JSON_BYTES).await?;
    let (request, request_hash, summary_profile) =
        parse_job_request(&request_bytes, version, state.max_input_bytes)?;

    {
        let conn = db::init_db(&state.db_path).map_err(ProviderHttpError::store)?;
        if let Some(existing) =
            store::get_job(&conn, &request.job_id).map_err(ProviderHttpError::store)?
        {
            return idempotent_response(existing, &request_hash, version);
        }
    }

    let artifact_field = multipart
        .next_field()
        .await
        .map_err(ProviderHttpError::multipart)?
        .ok_or_else(|| {
            ProviderHttpError::bad_request("ARTIFACT_MISSING", "The artifact part is missing.")
        })?;
    if artifact_field.name() != Some("artifact")
        || artifact_field.content_type() != Some("application/pdf")
    {
        return Err(ProviderHttpError::bad_request(
            "ARTIFACT_INVALID",
            "The second multipart field must be an application/pdf artifact.",
        ));
    }
    let input = request.inputs[0].clone();
    let mut pending_import =
        receive_artifact(&state, &request.job_id, &input, artifact_field).await?;
    if multipart
        .next_field()
        .await
        .map_err(ProviderHttpError::multipart)?
        .is_some()
    {
        return Err(ProviderHttpError::bad_request(
            "MULTIPART_FIELDS_INVALID",
            "Unexpected multipart fields were provided.",
        ));
    }

    let staging_path_text = match pending_import.staging().to_str() {
        Some(path) => path,
        None => {
            return Err(ProviderHttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "PROVIDER_STORAGE_INVALID",
                "Provider storage path is unavailable.",
                true,
            ));
        }
    };
    let (mut document, run) =
        match prepare_pdf_ingestion(staging_path_text, Some(&input.display_name)) {
            Ok(prepared) => prepared,
            Err(error) => {
                return Err(ProviderHttpError::domain(error.code()));
            }
        };
    if document.byte_size != input.byte_size || document.content_hash != input.sha256 {
        return Err(ProviderHttpError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "ARTIFACT_IDENTITY_MISMATCH",
            "The promoted artifact no longer matches its declared identity.",
            false,
        ));
    }
    let mut runtime = match (state.runtime_factory)() {
        Ok(runtime) => runtime,
        Err(error) => {
            let conn = db::init_db(&state.db_path).map_err(ProviderHttpError::store)?;
            if let Some(response) = idempotent_response_after_admission_race(
                store::get_job(&conn, &request.job_id),
                &request_hash,
                version,
                pending_import.staging(),
            )
            .await?
            {
                return Ok(response);
            }
            return Err(ProviderHttpError::runtime(error));
        }
    };
    let profile_snapshot = match runtime.profile_snapshot() {
        Some(snapshot) => snapshot,
        None => {
            let conn = db::init_db(&state.db_path).map_err(ProviderHttpError::store)?;
            if let Some(response) = idempotent_response_after_admission_race(
                store::get_job(&conn, &request.job_id),
                &request_hash,
                version,
                pending_import.staging(),
            )
            .await?
            {
                return Ok(response);
            }
            return Err(ProviderHttpError::runtime(ModelRuntimeFailure {
                code: "MODEL_CONFIG_INVALID".to_string(),
                message: "Connect runtime is missing its immutable model profile".to_string(),
                recoverable: false,
                request_attempts: Vec::new(),
            }));
        }
    };
    let provider_instance_id = match version {
        WireVersion::V1 => &state.instance_id_v1,
        WireVersion::V2 => &state.instance_id_v2,
    };
    #[cfg(target_os = "linux")]
    let _package_admission = package_admission_for_job(&state).map_err(|_| {
        ProviderHttpError::new(
            StatusCode::CONFLICT,
            "PROVIDER_BUSY",
            "The provider package is changing lifecycle state.",
            true,
        )
    })?;
    #[cfg(target_os = "linux")]
    let _transition_admission = state.transition_store.enter_job_admission().map_err(|_| {
        ProviderHttpError::new(
            StatusCode::CONFLICT,
            "PROVIDER_BUSY",
            "The provider is changing background lifecycle state.",
            true,
        )
    })?;
    let _provider_admission = state.admission_authority.enter().map_err(|()| {
        ProviderHttpError::new(
            StatusCode::CONFLICT,
            "PROVIDER_BUSY",
            "The provider is shutting down.",
            true,
        )
    })?;
    if !state.workers.accepting() {
        drop(_provider_admission);
        return Err(ProviderHttpError::new(
            StatusCode::CONFLICT,
            "PROVIDER_BUSY",
            "The provider is shutting down.",
            true,
        ));
    }
    let (import_path, owns_import) = promote_staged_artifact(
        pending_import.staging(),
        pending_import.final_path(),
        &input,
        &state.imports_dir,
    )?;
    pending_import.mark_promoted(owns_import);
    let import_path_text = import_path.to_str().ok_or_else(|| {
        ProviderHttpError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PROVIDER_STORAGE_INVALID",
            "Provider storage path is unavailable.",
            true,
        )
    })?;
    document.local_source_path = import_path_text.to_string();
    let mut conn = db::init_db(&state.db_path).map_err(ProviderHttpError::store)?;
    if let Some(existing) =
        store::get_job(&conn, &request.job_id).map_err(ProviderHttpError::store)?
    {
        drop(_provider_admission);
        return idempotent_response(existing, &request_hash, version);
    }
    if store::has_active_job(&conn).map_err(ProviderHttpError::store)? {
        drop(_provider_admission);
        return Err(ProviderHttpError::new(
            StatusCode::CONFLICT,
            "PROVIDER_BUSY",
            "The provider is processing another job.",
            true,
        ));
    }
    let accepted_result = store::accept_job_with_ingestion_guarded(
        &mut conn,
        &request,
        &request_hash,
        import_path_text,
        provider_instance_id,
        &document,
        &run,
        summary_profile,
        Some(&profile_snapshot),
        || state.entitlement.decision().is_active(),
    );
    drop(_provider_admission);
    let accepted = match accepted_result {
        Ok(Some((_, accepted))) => accepted,
        Ok(None) => {
            return Err(entitlement_required_error());
        }
        Err(error) => {
            if let Some(response) = idempotent_response_after_admission_race(
                store::get_job(&conn, &request.job_id),
                &request_hash,
                version,
                &import_path,
            )
            .await?
            {
                return Ok(response);
            }
            if store::has_active_job(&conn).map_err(ProviderHttpError::store)? {
                return Err(ProviderHttpError::new(
                    StatusCode::CONFLICT,
                    "PROVIDER_BUSY",
                    "The provider accepted another job first.",
                    true,
                ));
            }
            return Err(ProviderHttpError::store(error));
        }
    };
    pending_import.commit();
    runtime.bind_run(&accepted.pipeline_run_id);

    let worker_state = state.clone();
    let worker_job_id = request.job_id.clone();
    if let Err(error) = state.workers.spawn(
        format!("connect-job-{}", &worker_job_id[..8]),
        move |cancellation| process_job(worker_state, worker_job_id, runtime, cancellation),
    ) {
        eprintln!("Connect provider worker could not start: {error}");
        let worker_error = job_error(
            "PROVIDER_WORKER_UNAVAILABLE",
            "The provider could not start the job worker.",
            true,
        );
        let _ = store::mark_failed(&conn, &request.job_id, &worker_error);
        return Err(ProviderHttpError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "PROVIDER_WORKER_UNAVAILABLE",
            "The provider worker could not start.",
            true,
        ));
    }
    job_response(version, StatusCode::ACCEPTED, &accepted)
}

fn process_job(
    state: ProviderState,
    job_id: String,
    runtime: Box<dyn ModelRuntime>,
    cancellation: CancellationToken,
) {
    let result = (|| -> Result<(), ProcessJobError> {
        let conn = db::init_db(&state.db_path)?;
        let job = store::mark_processing(&conn, &job_id)?;
        let mut pipeline_conn = db::init_db(&state.db_path)?;
        let parser = PdfExtractParser::new();
        let normalizer = CanonicalNormalizer::new();
        let interpreter = DeterministicStructureInterpreter::new();
        let chunker = DeterministicDocumentChunker::new();
        let summary = process_ingested_to_summary_with_delivery_policy_controlled(
            &mut pipeline_conn,
            &job.pipeline_run_id,
            SummaryComponents {
                parser: &parser,
                normalizer: &normalizer,
                interpreter: &interpreter,
                chunker: &chunker,
                runtime: runtime.as_ref(),
            },
            SummaryDeliveryPolicy::connect(),
            &cancellation,
        )?;
        let analyzed = db::get_analyzed_document(&pipeline_conn, &job.pipeline_run_id)?
            .ok_or_else(
                || crate::pipeline::db::StoreError::DownstreamArtifactNotFound {
                    artifact_kind: "analyzed".to_string(),
                    run_id: job.pipeline_run_id.clone(),
                },
            )?;
        let normalized = db::get_normalized_document(&pipeline_conn, &job.pipeline_run_id)?
            .ok_or_else(|| {
                crate::pipeline::db::StoreError::NormalizedArtifactNotFound(
                    job.pipeline_run_id.clone(),
                )
            })?;
        persist_completed_summary(&conn, &job, &summary, &analyzed.omissions, &normalized)?;
        Ok(())
    })();

    if let Err(error) = result {
        let public_error = error.public_error();
        match db::init_db(&state.db_path)
            .map_err(ConnectStoreError::from)
            .and_then(|conn| store::mark_failed(&conn, &job_id, &public_error).map(|_| ()))
        {
            Ok(()) => {}
            Err(persistence) => {
                eprintln!("Connect job {job_id} failure could not persist: {persistence}");
            }
        }
    }
}

fn persist_completed_summary(
    conn: &rusqlite::Connection,
    job: &StoredConnectJob,
    summary: &SummaryArtifacts,
    omissions: &[AnalysisPageOmission],
    normalized: &NormalizedDocument,
) -> Result<(), ProcessJobError> {
    let claim_lines = render_citation_claim_lines(&summary.citations)
        .map_err(crate::pipeline::summary::SummaryPipelineError::StageFailed)
        .map_err(crate::pipeline::service::DocumentServiceError::Summary)?;
    let (result, delivered_claim_count) =
        JobResult::from_summary_claim_lines(&job.input, &summary.summary, &claim_lines)?;
    if !delivery_claim_prefix_coverage_satisfied(
        &summary.citations,
        delivered_claim_count,
        omissions,
        normalized,
    ) {
        return Err(
            crate::connect::contracts::ContractBuildError::InvalidSummary(
                "delivered claim prefix does not satisfy Connect page coverage".to_string(),
            )
            .into(),
        );
    }
    store::mark_completed(conn, &job.job_id, &result)?;
    Ok(())
}

#[derive(Debug, Error)]
enum ProcessJobError {
    #[error(transparent)]
    PipelineStore(#[from] crate::pipeline::db::StoreError),
    #[error(transparent)]
    ConnectStore(#[from] ConnectStoreError),
    #[error(transparent)]
    Service(#[from] crate::pipeline::service::DocumentServiceError),
    #[error(transparent)]
    Contract(#[from] crate::connect::contracts::ContractBuildError),
}

impl ProcessJobError {
    fn public_error(&self) -> JobError {
        match self {
            Self::Service(crate::pipeline::service::DocumentServiceError::Summary(
                SummaryPipelineError::StageFailed(failure),
            )) => job_error(
                &failure.code,
                "Document summarization failed in the provider.",
                failure.recoverable,
            ),
            Self::Service(error) => job_error(
                error.code(),
                "Document summarization failed in the provider.",
                retryable_pipeline_code(error.code()),
            ),
            Self::Contract(_) => job_error(
                "PROVIDER_OUTPUT_INVALID",
                "The provider result did not satisfy the Connect output contract.",
                false,
            ),
            Self::PipelineStore(_) | Self::ConnectStore(_) => job_error(
                "PROVIDER_INTERNAL_ERROR",
                "The provider could not persist the job result.",
                true,
            ),
        }
    }
}

fn retryable_pipeline_code(code: &str) -> bool {
    matches!(
        code,
        "MODEL_RUNTIME_UNAVAILABLE"
            | "MODEL_RUNTIME_REJECTED"
            | "MODEL_RESPONSE_INVALID"
            | "MODEL_RESPONSE_EMPTY"
            | "MODEL_RESPONSE_TOO_LARGE"
            | "PIPELINE_STORE_ERROR"
            | "DATABASE_ERROR"
            | "SUMMARY_ARTIFACT_PERSISTENCE_FAILED"
            | "PIPELINE_CANCELLATION_OBSERVED"
    )
}

async fn receive_artifact(
    state: &ProviderState,
    job_id: &str,
    input: &InputArtifact,
    mut field: axum::extract::multipart::Field<'_>,
) -> Result<PendingImport, ProviderHttpError> {
    let (staging, final_path) = allocate_import_paths(
        &state.imports_dir,
        job_id,
        &input.artifact_id,
        Uuid::new_v4(),
    );
    let pending = PendingImport::new(staging, final_path);
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(pending.staging())
        .await
        .map_err(ProviderHttpError::io)?;
    set_private_file_permissions(pending.staging())
        .await
        .map_err(ProviderHttpError::io)?;

    let receive_result = async {
        let mut byte_size = 0u64;
        let mut hasher = Sha256::new();
        while let Some(chunk) = field.chunk().await.map_err(ProviderHttpError::multipart)? {
            byte_size = byte_size.checked_add(chunk.len() as u64).ok_or_else(|| {
                ProviderHttpError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "ARTIFACT_TOO_LARGE",
                    "The artifact exceeds provider limits.",
                    false,
                )
            })?;
            if byte_size > state.max_input_bytes || byte_size > input.byte_size {
                return Err(ProviderHttpError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "ARTIFACT_TOO_LARGE",
                    "The artifact exceeds its declared or provider size limit.",
                    false,
                ));
            }
            hasher.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(ProviderHttpError::io)?;
        }
        file.flush().await.map_err(ProviderHttpError::io)?;
        file.sync_all().await.map_err(ProviderHttpError::io)?;
        let digest = format!("{:x}", hasher.finalize());
        if byte_size != input.byte_size || digest != input.sha256 {
            return Err(ProviderHttpError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "ARTIFACT_IDENTITY_MISMATCH",
                "The streamed artifact does not match its declared size and digest.",
                false,
            ));
        }
        Ok(())
    }
    .await;
    drop(file);
    receive_result?;
    Ok(pending)
}

struct PendingImport {
    staging: PathBuf,
    final_path: PathBuf,
    promoted: bool,
    committed: bool,
}

impl PendingImport {
    fn new(staging: PathBuf, final_path: PathBuf) -> Self {
        Self {
            staging,
            final_path,
            promoted: false,
            committed: false,
        }
    }

    fn staging(&self) -> &Path {
        &self.staging
    }

    fn final_path(&self) -> &Path {
        &self.final_path
    }

    fn mark_promoted(&mut self, owns_import: bool) {
        self.promoted = owns_import;
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for PendingImport {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let _ = fs::remove_file(&self.staging);
        if self.promoted {
            let _ = fs::remove_file(&self.final_path);
        }
    }
}

fn allocate_import_paths(
    imports_dir: &Path,
    job_id: &str,
    artifact_id: &str,
    transfer_id: Uuid,
) -> (PathBuf, PathBuf) {
    // The database job identity provides idempotency. Import paths are private to
    // one transfer so a rejected concurrent request can never unlink the file
    // another request accepted for the same job and artifact identities.
    let stem = format!("{job_id}-{artifact_id}-{transfer_id}");
    (
        imports_dir.join(format!(".{stem}.pdf")),
        imports_dir.join(format!("{stem}.pdf")),
    )
}

fn promote_staged_artifact(
    staging: &Path,
    final_path: &Path,
    input: &InputArtifact,
    imports_dir: &Path,
) -> Result<(PathBuf, bool), ProviderHttpError> {
    match fs::hard_link(staging, final_path) {
        Ok(()) => {
            let _ = fs::remove_file(staging);
            sync_directory_now(imports_dir).map_err(ProviderHttpError::io)?;
            Ok((final_path.to_path_buf(), true))
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let existing = hash_file_now(final_path);
            let _ = fs::remove_file(staging);
            let (existing_size, existing_hash) = existing?;
            if existing_size != input.byte_size || existing_hash != input.sha256 {
                return Err(ProviderHttpError::new(
                    StatusCode::CONFLICT,
                    "ARTIFACT_STORAGE_CONFLICT",
                    "Provider storage already contains different bytes for this artifact.",
                    false,
                ));
            }
            sync_directory_now(imports_dir).map_err(ProviderHttpError::io)?;
            Ok((final_path.to_path_buf(), false))
        }
        Err(error) => {
            let _ = fs::remove_file(staging);
            Err(ProviderHttpError::io(error))
        }
    }
}

fn sync_directory_now(path: &Path) -> Result<(), io::Error> {
    #[cfg(windows)]
    {
        let _ = path;
        Ok(())
    }
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
}

fn hash_file_now(path: &Path) -> Result<(u64, String), ProviderHttpError> {
    let mut file = File::open(path).map_err(ProviderHttpError::io)?;
    let mut buffer = [0u8; 8192];
    let mut size = 0u64;
    let mut hasher = Sha256::new();
    loop {
        let count = std::io::Read::read(&mut file, &mut buffer).map_err(ProviderHttpError::io)?;
        if count == 0 {
            break;
        }
        size = size.checked_add(count as u64).ok_or_else(|| {
            ProviderHttpError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "ARTIFACT_TOO_LARGE",
                "The stored artifact exceeds provider limits.",
                false,
            )
        })?;
        hasher.update(&buffer[..count]);
    }
    Ok((size, format!("{:x}", hasher.finalize())))
}

async fn read_field_limited(
    mut field: axum::extract::multipart::Field<'_>,
    limit: u64,
) -> Result<Vec<u8>, ProviderHttpError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(ProviderHttpError::multipart)? {
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Err(ProviderHttpError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "REQUEST_TOO_LARGE",
                "The request metadata exceeds provider limits.",
                false,
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn idempotent_response(
    existing: StoredConnectJob,
    request_hash: &str,
    version: WireVersion,
) -> Result<Response, ProviderHttpError> {
    if existing_request_matches(&existing, request_hash, version) {
        job_response(version, StatusCode::OK, &existing)
    } else {
        Err(ProviderHttpError::new(
            StatusCode::CONFLICT,
            "JOB_ID_CONFLICT",
            "The job identifier was already used for different input.",
            false,
        ))
    }
}

fn existing_request_matches(
    existing: &StoredConnectJob,
    request_hash: &str,
    version: WireVersion,
) -> bool {
    existing.protocol_version == version.protocol_version() && existing.request_hash == request_hash
}

fn existing_job_owns_import_path(existing_import_path: &str, candidate: &Path) -> bool {
    Path::new(existing_import_path) == candidate
}

async fn idempotent_response_after_admission_race(
    existing: Result<Option<StoredConnectJob>, ConnectStoreError>,
    request_hash: &str,
    version: WireVersion,
    candidate_import_path: &Path,
) -> Result<Option<Response>, ProviderHttpError> {
    let existing = match existing {
        Ok(existing) => existing,
        Err(error) => {
            remove_file_quietly(candidate_import_path).await;
            return Err(ProviderHttpError::store(error));
        }
    };
    let Some(existing) = existing else {
        return Ok(None);
    };
    if !existing_job_owns_import_path(&existing.import_path, candidate_import_path) {
        remove_file_quietly(candidate_import_path).await;
    }
    idempotent_response(existing, request_hash, version).map(Some)
}

fn authorize(state: &ProviderState, headers: &HeaderMap) -> Result<(), ProviderHttpError> {
    if headers.contains_key(header::ORIGIN) {
        return Err(ProviderHttpError::new(
            StatusCode::FORBIDDEN,
            "BROWSER_ORIGIN_REJECTED",
            "Browser-origin requests are not accepted.",
            false,
        ));
    }
    let expected = format!("Bearer {}", state.token);
    let supplied = headers
        .get(header::AUTHORIZATION)
        .map(|value| value.as_bytes())
        .unwrap_or_default();
    if supplied.len() != expected.len() || supplied.ct_eq(expected.as_bytes()).unwrap_u8() != 1 {
        return Err(ProviderHttpError::new(
            StatusCode::UNAUTHORIZED,
            "AUTHENTICATION_REQUIRED",
            "A valid provider bearer token is required.",
            false,
        ));
    }
    Ok(())
}

fn require_entitlement(state: &ProviderState) -> Result<(), ProviderHttpError> {
    if state.entitlement.decision().is_active() {
        return Ok(());
    }
    Err(entitlement_required_error())
}

fn entitlement_required_error() -> ProviderHttpError {
    ProviderHttpError::new(
        StatusCode::FORBIDDEN,
        "CONNECT_ENTITLEMENT_REQUIRED",
        "An active Connect entitlement is required.",
        false,
    )
}

struct ProviderHttpError {
    status: StatusCode,
    error: JobError,
    protocol_version: u32,
}

impl ProviderHttpError {
    fn new(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            status,
            error: job_error(code, message, retryable),
            protocol_version: PROTOCOL_VERSION,
        }
    }

    fn bad_request(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message, false)
    }

    fn from_job_error(error: JobError) -> Self {
        let status = match error.code.as_str() {
            "PROTOCOL_VERSION_UNSUPPORTED" => StatusCode::UPGRADE_REQUIRED,
            "INPUT_ARTIFACT_INVALID" => StatusCode::UNPROCESSABLE_ENTITY,
            _ => StatusCode::BAD_REQUEST,
        };
        Self {
            status,
            error,
            protocol_version: PROTOCOL_VERSION,
        }
    }

    fn store(error: impl std::fmt::Display) -> Self {
        eprintln!("Connect provider store error: {error}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PROVIDER_INTERNAL_ERROR",
            "The provider could not access its job store.",
            true,
        )
    }

    fn json(error: serde_json::Error) -> Self {
        eprintln!("Connect provider JSON error: {error}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PROVIDER_INTERNAL_ERROR",
            "The provider could not encode job metadata.",
            true,
        )
    }

    fn contract(error: impl std::fmt::Display) -> Self {
        eprintln!("Connect provider contract conversion error: {error}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PROVIDER_OUTPUT_INVALID",
            "The provider result did not satisfy the Connect output contract.",
            false,
        )
    }

    fn runtime(failure: ModelRuntimeFailure) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            failure.code,
            "The configured local model runtime is unavailable.",
            failure.recoverable,
        )
    }

    fn with_protocol_version(mut self, protocol_version: u32) -> Self {
        self.protocol_version = protocol_version;
        self
    }

    fn multipart(error: impl std::fmt::Display) -> Self {
        eprintln!("Connect provider multipart error: {error}");
        Self::bad_request("MULTIPART_INVALID", "The multipart request is invalid.")
    }

    fn io(error: io::Error) -> Self {
        eprintln!("Connect provider artifact I/O error: {error}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PROVIDER_STORAGE_ERROR",
            "The provider could not persist the input artifact.",
            true,
        )
    }

    fn domain(code: &str) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            code,
            "The input artifact failed provider validation.",
            false,
        )
    }
}

impl IntoResponse for ProviderHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorEnvelope {
                protocol_version: self.protocol_version,
                error: self.error,
            }),
        )
            .into_response()
    }
}

fn acquire_registration_lock(
    path: &Path,
    private_root: Option<&Path>,
    absolute_deadline: Option<Instant>,
) -> Result<RegistrationLifecycleLock, io::Error> {
    #[cfg(windows)]
    {
        let _ = absolute_deadline;
        let private_root = private_root.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows registration lock requires its private root",
            )
        })?;
        match WindowsFileLock::acquire(path, private_root) {
            Ok(file) => Ok(RegistrationLifecycleLock { _file: file }),
            Err(FileLockError::Busy) => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Connect provider ownership lock is busy",
            )),
            Err(FileLockError::Io(error)) => Err(error),
        }
    }

    #[cfg(unix)]
    let _ = private_root;
    #[cfg(unix)]
    {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        let file = options.open(path)?;
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;

        let deadline =
            absolute_deadline.unwrap_or_else(|| Instant::now() + REGISTRATION_LOCK_TIMEOUT);
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(RegistrationLifecycleLock { file }),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .min(REGISTRATION_LOCK_RETRY),
                    );
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "timed out waiting for the Connect registration lifecycle lock",
                    ));
                }
                Err(TryLockError::Error(error)) => return Err(error),
            }
        }
    }
}

#[cfg(unix)]
fn acquire_provider_ownership_lock(
    path: &Path,
) -> Result<RegistrationLifecycleLock, ProviderStartError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
    let file = options.open(path)?;
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    match file.try_lock() {
        Ok(()) => Ok(RegistrationLifecycleLock { file }),
        Err(TryLockError::WouldBlock) => Err(ProviderStartError::ProviderAlreadyRunning),
        Err(TryLockError::Error(error)) => Err(ProviderStartError::Io(error)),
    }
}

#[cfg(windows)]
fn map_windows_registration_lock_error(error: io::Error) -> ProviderStartError {
    if error.kind() == io::ErrorKind::WouldBlock {
        ProviderStartError::ProviderAlreadyRunning
    } else {
        ProviderStartError::Io(error)
    }
}

fn write_registration<T: Serialize>(
    _registration_lock: &RegistrationLifecycleLock,
    registration_path: &Path,
    registration: &T,
    private_root: Option<&Path>,
) -> Result<(), ProviderStartError> {
    #[cfg(unix)]
    let parent = registration_path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "registration has no parent"))?;
    #[cfg(unix)]
    let temporary = parent.join(format!(".{}.tmp", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(registration)?;
    #[cfg(windows)]
    {
        let private_root = private_root.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows registration publication requires its private root",
            )
        })?;
        bytes
            .len()
            .checked_add(1)
            .filter(|length| *length as u64 <= MAX_REGISTRATION_BYTES)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Connect registration is oversized",
                )
            })?;
        let mut published = bytes;
        published.push(b'\n');
        windows_storage::atomic_replace_bytes(
            registration_path,
            &published,
            MAX_REGISTRATION_BYTES,
            false,
            private_root,
        )?;
        Ok(())
    }
    #[cfg(unix)]
    let _ = private_root;
    #[cfg(unix)]
    {
        let write_result = (|| -> Result<(), io::Error> {
            let mut file = private_create_new(&temporary)?;
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, registration_path)?;
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result?;
        Ok(())
    }
}

#[cfg(unix)]
fn scavenge_stale_registrations(
    _registration_lock: &RegistrationLifecycleLock,
    provider_directories: [(&Path, WireVersion); 2],
    absolute_deadline: Option<Instant>,
) -> Result<usize, ProviderStartError> {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(REGISTRATION_PROBE_TIMEOUT)
        .build()
        .map_err(|error| ProviderStartError::Server(error.to_string()))?;
    let mut candidates = Vec::new();
    for (directory, version) in provider_directories {
        for entry in fs::read_dir(directory)? {
            if absolute_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err(readiness_deadline_error());
            }
            let path = entry?.path();
            if let Some(instance_id) = owned_registration_instance_id(&path) {
                candidates.push((path, version, instance_id));
            }
        }
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    if absolute_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(readiness_deadline_error());
    }

    for (path, version, instance_id) in &candidates {
        let deadline =
            absolute_deadline.unwrap_or_else(|| Instant::now() + REGISTRATION_PROBE_TIMEOUT);
        if Instant::now() >= deadline {
            return Err(readiness_deadline_error());
        }
        if registration_proves_live(&client, path, *version, instance_id, None, None, deadline) {
            return Err(ProviderStartError::ProviderAlreadyRunning);
        }
        if absolute_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(readiness_deadline_error());
        }
    }
    for (path, _, _) in &candidates {
        if absolute_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(readiness_deadline_error());
        }
        fs::remove_file(path)?;
    }
    if !candidates.is_empty() {
        for (directory, _) in provider_directories {
            if absolute_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err(readiness_deadline_error());
            }
            File::open(directory)?.sync_all()?;
        }
    }
    Ok(candidates.len())
}

#[cfg(unix)]
fn owned_registration_instance_id(path: &Path) -> Option<String> {
    let filename = path.file_name()?.to_str()?;
    let instance_id = filename
        .strip_prefix(APP_ID)?
        .strip_prefix('-')?
        .strip_suffix(".json")?;
    valid_uuid_v4(instance_id).then(|| instance_id.to_string())
}

#[cfg(unix)]
fn probe_time_remaining(deadline: Instant) -> Option<Duration> {
    let remaining = deadline.checked_duration_since(Instant::now())?;
    (!remaining.is_zero()).then_some(remaining.min(REGISTRATION_PROBE_TIMEOUT))
}

#[cfg(unix)]
fn readiness_deadline_error() -> ProviderStartError {
    ProviderStartError::Server("provider readiness deadline expired".to_string())
}

#[cfg(target_os = "linux")]
fn openat_file(directory: &File, name: &std::ffi::CStr, flags: i32) -> Option<File> {
    let descriptor = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
    (descriptor >= 0).then(|| unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(target_os = "linux")]
fn network_namespace_identity(directory: &File) -> Option<(u64, u64)> {
    let namespace = openat_file(directory, c"ns/net", libc::O_RDONLY | libc::O_CLOEXEC)?;
    let metadata = namespace.metadata().ok()?;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(target_os = "linux")]
fn current_network_namespace_identity() -> Option<(u64, u64)> {
    let namespace = File::open("/proc/self/ns/net").ok()?;
    let metadata = namespace.metadata().ok()?;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(target_os = "linux")]
fn loopback_listener_inode(base_url: &str, version: WireVersion, deadline: Instant) -> Option<u64> {
    probe_time_remaining(deadline)?;
    let port = validated_manifest_url(base_url, version)?.port()?;
    let local_endpoint = format!("0100007F:{port:04X}");
    let reader = BufReader::new(File::open("/proc/net/tcp").ok()?);
    let mut found = None;
    for line in reader.lines() {
        probe_time_remaining(deadline)?;
        let line = line.ok()?;
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        if fields.len() < 10
            || !fields[1].eq_ignore_ascii_case(&local_endpoint)
            || fields[3] != "0A"
        {
            continue;
        }
        let inode = fields[9].parse::<u64>().ok()?;
        if found.replace(inode).is_some() {
            return None;
        }
    }
    found
}

#[cfg(target_os = "linux")]
struct RegisteredProcessGuard {
    process_directory: File,
    process_path: PathBuf,
    process_device: u64,
    process_inode: u64,
    network_device: u64,
    network_inode: u64,
    expected: ExpectedProviderProcess,
}

#[cfg(target_os = "linux")]
impl RegisteredProcessGuard {
    fn open(pid: u32, expected: ExpectedProviderProcess) -> Option<Self> {
        if pid == 0 {
            return None;
        }
        let process_path = PathBuf::from(format!("/proc/{pid}"));
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let process_directory = options.open(&process_path).ok()?;
        let opened = process_directory.metadata().ok()?;
        let current = fs::symlink_metadata(&process_path).ok()?;
        if !opened.file_type().is_dir()
            || opened.uid() != expected.uid
            || opened.dev() != current.dev()
            || opened.ino() != current.ino()
        {
            return None;
        }
        let (network_device, network_inode) = network_namespace_identity(&process_directory)?;
        if current_network_namespace_identity()? != (network_device, network_inode) {
            return None;
        }
        let guard = Self {
            process_directory,
            process_path,
            process_device: opened.dev(),
            process_inode: opened.ino(),
            network_device,
            network_inode,
            expected,
        };
        guard.executable_matches().then_some(guard)
    }

    fn executable_matches(&self) -> bool {
        let Some(executable) = openat_file(
            &self.process_directory,
            c"exe",
            libc::O_PATH | libc::O_CLOEXEC,
        ) else {
            return false;
        };
        executable.metadata().is_ok_and(|metadata| {
            metadata.file_type().is_file()
                && metadata.dev() == self.expected.executable_device
                && metadata.ino() == self.expected.executable_inode
        })
    }

    fn owns_socket(&self, socket_inode: u64, deadline: Instant) -> bool {
        if probe_time_remaining(deadline).is_none() {
            return false;
        }
        let Some(descriptors) = openat_file(
            &self.process_directory,
            c"fd",
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        ) else {
            return false;
        };
        let descriptor_path = PathBuf::from(format!("/proc/self/fd/{}", descriptors.as_raw_fd()));
        let expected = format!("socket:[{socket_inode}]");
        let Ok(entries) = fs::read_dir(descriptor_path) else {
            return false;
        };
        for entry in entries {
            if probe_time_remaining(deadline).is_none() {
                return false;
            }
            if entry
                .ok()
                .and_then(|entry| fs::read_link(entry.path()).ok())
                .is_some_and(|target| target.as_os_str() == expected.as_str())
            {
                return true;
            }
        }
        false
    }

    fn revalidate(&self) -> bool {
        fs::symlink_metadata(&self.process_path).is_ok_and(|current| {
            current.file_type().is_dir()
                && current.uid() == self.expected.uid
                && current.dev() == self.process_device
                && current.ino() == self.process_inode
        }) && self.executable_matches()
            && network_namespace_identity(&self.process_directory)
                == Some((self.network_device, self.network_inode))
            && current_network_namespace_identity()
                == Some((self.network_device, self.network_inode))
    }
}

#[cfg(unix)]
fn registration_proves_live(
    client: &reqwest::blocking::Client,
    path: &Path,
    version: WireVersion,
    expected_instance_id: &str,
    private_root: Option<&Path>,
    expected_process: Option<&ExpectedProviderProcess>,
    deadline: Instant,
) -> bool {
    #[cfg(target_os = "linux")]
    if probe_time_remaining(deadline).is_none() {
        return false;
    }
    let Some(bytes) = read_bounded_regular_file(path, MAX_REGISTRATION_BYTES, private_root) else {
        return false;
    };
    let Ok(registration) = serde_json::from_slice::<RegistrationIdentity>(&bytes) else {
        return false;
    };
    if !registration_matches_candidate(&registration, version, expected_instance_id) {
        return false;
    }
    #[cfg(target_os = "linux")]
    let process_guard = match expected_process {
        Some(expected) => match RegisteredProcessGuard::open(registration.pid, *expected) {
            Some(guard) => Some(guard),
            None => return false,
        },
        None => None,
    };
    #[cfg(not(target_os = "linux"))]
    if expected_process.is_some() {
        return false;
    }
    #[cfg(target_os = "linux")]
    let listener_inode = match process_guard.as_ref() {
        Some(guard) => {
            let Some(inode) =
                loopback_listener_inode(&registration.transport.base_url, version, deadline)
            else {
                return false;
            };
            if !guard.owns_socket(inode, deadline) {
                return false;
            }
            Some(inode)
        }
        None => None,
    };
    let Some(probe) = probe_manifest(
        client,
        &registration.transport.base_url,
        &registration.auth.token,
        version,
        deadline,
    ) else {
        return false;
    };
    let manifest_matches = match probe {
        ManifestProbe::Manifest(manifest) => {
            manifest.protocol_version == version.protocol_version()
                && manifest.instance_id == expected_instance_id
                && manifest.app.id == APP_ID
        }
        ManifestProbe::EntitlementRequired => true,
    };
    #[cfg(target_os = "linux")]
    return manifest_matches
        && process_guard.as_ref().is_none_or(|guard| {
            guard.revalidate()
                && listener_inode.is_some_and(|inode| {
                    loopback_listener_inode(&registration.transport.base_url, version, deadline)
                        == Some(inode)
                        && guard.owns_socket(inode, deadline)
                })
        });
    #[cfg(not(target_os = "linux"))]
    manifest_matches
}

fn registration_matches_candidate(
    registration: &RegistrationIdentity,
    version: WireVersion,
    expected_instance_id: &str,
) -> bool {
    registration.protocol_version == version.protocol_version()
        && registration.instance_id == expected_instance_id
        && registration.app_id == APP_ID
        && registration.transport.kind == version.transport_kind()
        && registration.auth.scheme == "bearer"
        && !registration.auth.token.is_empty()
        && validated_manifest_url(&registration.transport.base_url, version).is_some()
}

#[cfg(target_os = "linux")]
pub(crate) fn stop_and_cleanup_registered_provider(
    app_data_dir: &Path,
    runtime_root: &Path,
    deadline: Instant,
) -> Result<(), ProviderStartError> {
    probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
    let providers_dir_v1 = runtime_root.join("local-connect/v1/providers");
    let providers_dir_v2 = runtime_root.join("local-connect/v2/providers");
    probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
    ensure_private_directory(&providers_dir_v1)?;
    probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
    ensure_private_directory(&providers_dir_v2)?;
    probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(REGISTRATION_PROBE_TIMEOUT)
        .build()
        .map_err(|error| ProviderStartError::Server(error.to_string()))?;
    let expected_process = expected_provider_process(
        unsafe { libc::geteuid() },
        &env::current_exe().map_err(ProviderStartError::Io)?,
    )?;
    let mut signaled = std::collections::BTreeSet::new();
    for (directory, version) in [
        (&providers_dir_v1, WireVersion::V1),
        (&providers_dir_v2, WireVersion::V2),
    ] {
        for entry in fs::read_dir(directory)? {
            probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
            let path = entry?.path();
            let Some(instance_id) = owned_registration_instance_id(&path) else {
                continue;
            };
            let Some(bytes) = read_bounded_regular_file(&path, MAX_REGISTRATION_BYTES, None) else {
                continue;
            };
            let Ok(registration) = serde_json::from_slice::<RegistrationIdentity>(&bytes) else {
                continue;
            };
            if registration_matches_candidate(&registration, version, &instance_id)
                && registration_proves_live(
                    &client,
                    &path,
                    version,
                    &instance_id,
                    None,
                    Some(&expected_process),
                    deadline,
                )
                && signaled.insert(registration.pid)
            {
                probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
                let pid = i32::try_from(registration.pid).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "provider pid is invalid")
                })?;
                if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(ProviderStartError::Io(error));
                    }
                }
            }
        }
    }

    let owner_path = app_data_dir.join(".connect-provider-owner.lock");
    let registration_lock_path = providers_dir_v1.join(format!(".{APP_ID}.lifecycle.lock"));
    loop {
        probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
        match acquire_provider_ownership_lock(&owner_path) {
            Ok(_owner) => {
                probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
                let registration_lock =
                    acquire_registration_lock(&registration_lock_path, None, Some(deadline))?;
                probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
                scavenge_stale_registrations(
                    &registration_lock,
                    [
                        (&providers_dir_v1, WireVersion::V1),
                        (&providers_dir_v2, WireVersion::V2),
                    ],
                    Some(deadline),
                )?;
                return Ok(());
            }
            Err(ProviderStartError::ProviderAlreadyRunning) if Instant::now() < deadline => {
                thread::sleep(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(50)),
                );
            }
            Err(ProviderStartError::ProviderAlreadyRunning) => {
                return Err(ProviderStartError::Server(
                    "provider stop deadline expired".to_string(),
                ));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegisteredProviderState {
    Absent,
    Ready,
    PresentUnready,
}

#[cfg(target_os = "linux")]
fn provider_probe_client() -> Result<reqwest::blocking::Client, ProviderStartError> {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(REGISTRATION_PROBE_TIMEOUT)
        .timeout(REGISTRATION_PROBE_TIMEOUT)
        .build()
        .map_err(|error| ProviderStartError::Server(error.to_string()))
}

#[cfg(target_os = "linux")]
fn registered_provider_state_with_client(
    client: &reqwest::blocking::Client,
    runtime_root: &Path,
    expected_v2_instance_id: Option<&str>,
    expected_process: &ExpectedProviderProcess,
    deadline: Instant,
) -> Result<RegisteredProviderState, ProviderStartError> {
    probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
    let directory_v2 = runtime_root.join("local-connect/v2/providers");
    ensure_private_directory(&directory_v2)?;
    let mut present = false;
    for entry in fs::read_dir(&directory_v2)? {
        probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
        let path = entry?.path();
        let name_is_provider = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(&format!("{APP_ID}-")) && name.ends_with(".json"));
        if !name_is_provider {
            continue;
        }
        present = true;
        let Some(instance_id) = owned_registration_instance_id(&path) else {
            continue;
        };
        if expected_v2_instance_id.is_some_and(|expected| expected != instance_id) {
            continue;
        }
        if registration_proves_live(
            client,
            &path,
            WireVersion::V2,
            &instance_id,
            None,
            Some(expected_process),
            deadline,
        ) {
            return Ok(RegisteredProviderState::Ready);
        }
        probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
    }
    probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
    let directory_v1 = runtime_root.join("local-connect/v1/providers");
    ensure_private_directory(&directory_v1)?;
    if !present {
        for entry in fs::read_dir(directory_v1)? {
            probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
            if entry.ok().is_some_and(|entry| {
                entry.file_name().to_str().is_some_and(|name| {
                    name.starts_with(&format!("{APP_ID}-")) && name.ends_with(".json")
                })
            }) {
                present = true;
                break;
            }
        }
    }
    probe_time_remaining(deadline).ok_or_else(readiness_deadline_error)?;
    Ok(if present {
        RegisteredProviderState::PresentUnready
    } else {
        RegisteredProviderState::Absent
    })
}

#[cfg(target_os = "linux")]
pub(crate) fn registered_provider_state(
    runtime_root: &Path,
    expected_v2_instance_id: Option<&str>,
    expected_process: &ExpectedProviderProcess,
    deadline: Instant,
) -> Result<RegisteredProviderState, ProviderStartError> {
    let client = provider_probe_client()?;
    registered_provider_state_with_client(
        &client,
        runtime_root,
        expected_v2_instance_id,
        expected_process,
        deadline,
    )
}

#[cfg(target_os = "linux")]
pub(crate) fn wait_for_registered_provider(
    runtime_root: &Path,
    expected_v2_instance_id: Option<&str>,
    expected_process: &ExpectedProviderProcess,
    deadline: Instant,
) -> Result<(), ProviderStartError> {
    let client = provider_probe_client()?;
    while Instant::now() < deadline {
        if registered_provider_state_with_client(
            &client,
            runtime_root,
            expected_v2_instance_id,
            expected_process,
            deadline,
        )? == RegisteredProviderState::Ready
        {
            return Ok(());
        }
        thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(50)),
        );
    }
    Err(ProviderStartError::Server(
        "provider readiness deadline expired".to_string(),
    ))
}

fn validated_manifest_url(base_url: &str, version: WireVersion) -> Option<reqwest::Url> {
    let mut url = reqwest::Url::parse(base_url).ok()?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    url.set_path(version.manifest_path());
    Some(url)
}

#[cfg(unix)]
fn probe_manifest(
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
    version: WireVersion,
    deadline: Instant,
) -> Option<ManifestProbe> {
    let url = validated_manifest_url(base_url, version)?;
    let timeout = probe_time_remaining(deadline)?;
    let response = client
        .get(url.clone())
        .header(header::ACCEPT, "application/json")
        .bearer_auth(token)
        .timeout(timeout)
        .send()
        .ok()?;
    let status = response.status();
    if !matches!(status, StatusCode::OK | StatusCode::FORBIDDEN)
        || response
            .content_length()
            .is_some_and(|length| length > MAX_PROBED_MANIFEST_BYTES)
    {
        return None;
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_PROBED_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    probe_time_remaining(deadline)?;
    if bytes.len() as u64 > MAX_PROBED_MANIFEST_BYTES {
        return None;
    }
    if status == StatusCode::OK {
        return serde_json::from_slice(&bytes)
            .ok()
            .map(ManifestProbe::Manifest);
    }
    let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).ok()?;
    if envelope.protocol_version != version.protocol_version()
        || envelope.error.code != "CONNECT_ENTITLEMENT_REQUIRED"
        || !probe_rejects_invalid_token(client, url, token, version, deadline)
    {
        return None;
    }
    Some(ManifestProbe::EntitlementRequired)
}

#[cfg(unix)]
fn probe_rejects_invalid_token(
    client: &reqwest::blocking::Client,
    url: reqwest::Url,
    token: &str,
    version: WireVersion,
    deadline: Instant,
) -> bool {
    let Some(timeout) = probe_time_remaining(deadline) else {
        return false;
    };
    let response = match client
        .get(url)
        .header(header::ACCEPT, "application/json")
        .bearer_auth(format!("{token}-invalid"))
        .timeout(timeout)
        .send()
    {
        Ok(response) => response,
        Err(_) => return false,
    };
    if response.status() != StatusCode::UNAUTHORIZED
        || response
            .content_length()
            .is_some_and(|length| length > MAX_PROBED_MANIFEST_BYTES)
    {
        return false;
    }
    let mut bytes = Vec::new();
    if response
        .take(MAX_PROBED_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_PROBED_MANIFEST_BYTES
    {
        return false;
    }
    if probe_time_remaining(deadline).is_none() {
        return false;
    }
    serde_json::from_slice::<ErrorEnvelope>(&bytes).is_ok_and(|envelope| {
        envelope.protocol_version == version.protocol_version()
            && envelope.error.code == "AUTHENTICATION_REQUIRED"
    })
}

fn read_bounded_regular_file(
    path: &Path,
    max_bytes: u64,
    private_root: Option<&Path>,
) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        windows_storage::read_bounded_regular_file(path, max_bytes, false, private_root).ok()
    }
    #[cfg(unix)]
    let _ = private_root;
    #[cfg(unix)]
    {
        let metadata = fs::symlink_metadata(path).ok()?;
        if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > max_bytes {
            return None;
        }
        let file = File::open(path).ok()?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(max_bytes + 1).read_to_end(&mut bytes).ok()?;
        (bytes.len() as u64 <= max_bytes).then_some(bytes)
    }
}

fn remove_registration_if_owned(
    _registration_lock: &RegistrationLifecycleLock,
    path: &Path,
    version: WireVersion,
    expected_instance_id: &str,
    expected_base_url: &str,
    expected_token: &str,
    private_root: Option<&Path>,
) -> Result<bool, io::Error> {
    if !registration_belongs_to_provider(
        path,
        version,
        expected_instance_id,
        expected_base_url,
        expected_token,
        private_root,
    ) {
        return Ok(false);
    }
    #[cfg(windows)]
    {
        let private_root = private_root.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows registration cleanup requires its private root",
            )
        })?;
        windows_storage::remove_private_file(path, private_root)
    }
    #[cfg(unix)]
    let _ = private_root;
    #[cfg(unix)]
    {
        match fs::remove_file(path) {
            Ok(()) => {
                let parent = path.parent().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "registration has no parent")
                })?;
                File::open(parent)?.sync_all()?;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}

fn registration_belongs_to_provider(
    path: &Path,
    version: WireVersion,
    expected_instance_id: &str,
    expected_base_url: &str,
    expected_token: &str,
    private_root: Option<&Path>,
) -> bool {
    let Some(bytes) = read_bounded_regular_file(path, MAX_REGISTRATION_BYTES, private_root) else {
        return false;
    };
    serde_json::from_slice::<RegistrationIdentity>(&bytes)
        .ok()
        .is_some_and(|registration| {
            registration_matches_candidate(&registration, version, expected_instance_id)
                && registration.transport.base_url == expected_base_url
                && registration.auth.token == expected_token
        })
}

fn ensure_private_directory(path: &Path) -> Result<(), io::Error> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn parse_v2_instance_id(value: &str) -> Result<String, ProviderStartError> {
    let candidate = value.strip_suffix('\n').unwrap_or(value);
    if candidate.contains('\n') || candidate.contains('\r') || !valid_uuid_v4(candidate) {
        return Err(ProviderStartError::InvalidInstanceIdentity);
    }
    Ok(candidate.to_string())
}

fn load_or_create_v2_instance_id(
    app_data_dir: &Path,
    bootstrap_instance_id: Option<&str>,
) -> Result<String, ProviderStartError> {
    let path = app_data_dir.join(V2_INSTANCE_ID_FILE);
    let bootstrap_instance_id = bootstrap_instance_id
        .map(parse_v2_instance_id)
        .transpose()?;
    match fs::read_to_string(&path) {
        Ok(value) => {
            let persisted = parse_v2_instance_id(&value)?;
            if let Some(expected) = bootstrap_instance_id.as_deref() {
                if expected != persisted {
                    return Err(ProviderStartError::InvalidInstanceIdentity);
                }
            }
            return Ok(persisted);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let instance_id = bootstrap_instance_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let temporary = app_data_dir.join(format!(".{V2_INSTANCE_ID_FILE}.{}.tmp", Uuid::new_v4()));
    let write_result = (|| -> Result<(), io::Error> {
        let mut file = private_create_new(&temporary)?;
        file.write_all(instance_id.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        #[cfg(unix)]
        File::open(app_data_dir)?.sync_all()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(temporary);
        return Err(error.into());
    }
    Ok(instance_id)
}

fn private_create_new(path: &Path) -> Result<File, io::Error> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

async fn set_private_file_permissions(_path: &Path) -> Result<(), io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(_path, fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(())
}

async fn remove_file_quietly(path: &Path) {
    if let Err(error) = tokio::fs::remove_file(path).await {
        if error.kind() != io::ErrorKind::NotFound {
            eprintln!("Connect temporary artifact cleanup failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::contracts::{
        CapabilityRef, InputArtifact, JobState, CAPABILITY_ID, CAPABILITY_VERSION, INPUT_MEDIA_TYPE,
    };
    use crate::connect::entitlement::{EntitlementGate, ENTITLEMENT_FILE_NAME, FEATURE_ID};
    use crate::pipeline::contracts::{
        CitationArtifact, CitedClaim, EvidenceItem, ModelRequest, ModelResponse, PipelineFailure,
        PipelineStage, PipelineState, SourceSpan, SourceType, SummaryArtifact,
    };
    use crate::pipeline::control::ExecutionControl;
    use crate::pipeline::ingest::ingest_pdf;
    use crate::pipeline::normalize::normalize_document;
    use crate::pipeline::parser::parse_document;
    use base64::{
        engine::general_purpose::{STANDARD as BASE64, URL_SAFE_NO_PAD},
        Engine as _,
    };
    use chrono::DateTime;
    use reqwest::blocking::{multipart, Client};
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::collections::BTreeMap;
    #[cfg(any(windows, target_os = "linux"))]
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn connect_proof_runtime_requires_both_build_feature_and_exact_mode() {
        let exact = OsStr::new(CONNECT_PROOF_MODE_V1);
        assert_eq!(
            select_connect_runtime_source(false, None).unwrap(),
            ConnectRuntimeSource::PersistedSettings
        );
        assert_eq!(
            select_connect_runtime_source(false, Some(exact)).unwrap(),
            ConnectRuntimeSource::PersistedSettings
        );
        assert_eq!(
            select_connect_runtime_source(false, Some(OsStr::new("invalid"))).unwrap(),
            ConnectRuntimeSource::PersistedSettings
        );
        assert_eq!(
            select_connect_runtime_source(true, None).unwrap(),
            ConnectRuntimeSource::PersistedSettings
        );
        assert_eq!(
            select_connect_runtime_source(true, Some(exact)).unwrap(),
            ConnectRuntimeSource::ProofFixture
        );
        for invalid in ["", "local-fixture", "local-fixture-v2", " local-fixture-v1"] {
            assert_eq!(
                select_connect_runtime_source(true, Some(OsStr::new(invalid)))
                    .unwrap_err()
                    .code,
                "MODEL_CONFIG_INVALID"
            );
        }
    }

    #[test]
    fn accepted_job_ownership_preserves_only_its_shared_import() {
        let shared = PathBuf::from("imports").join("job-artifact.pdf");
        let loser_only = PathBuf::from("imports").join("other-artifact.pdf");

        assert!(existing_job_owns_import_path(
            shared.to_str().unwrap(),
            &shared
        ));
        assert!(!existing_job_owns_import_path(
            shared.to_str().unwrap(),
            &loser_only
        ));
    }

    #[test]
    fn concurrent_transfers_never_share_import_paths() {
        let imports = PathBuf::from("imports");
        let job_id = "33333333-3333-4333-8333-333333333333";
        let artifact_id = "22222222-2222-4222-8222-222222222222";
        let first = allocate_import_paths(
            &imports,
            job_id,
            artifact_id,
            Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap(),
        );
        let second = allocate_import_paths(
            &imports,
            job_id,
            artifact_id,
            Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap(),
        );

        assert_ne!(first.0, second.0);
        assert_ne!(first.1, second.1);
        assert!(first.0.starts_with(&imports));
        assert!(first.1.starts_with(&imports));
        assert_eq!(
            first.0.extension().and_then(|value| value.to_str()),
            Some("pdf")
        );
        assert_eq!(
            first.1.extension().and_then(|value| value.to_str()),
            Some("pdf")
        );
    }

    #[test]
    fn staged_artifact_promotion_never_overwrites_existing_bytes() {
        let root = TestDirectory::new("doc-sum-connect-promotion-race");
        let imports = root.0.join("imports");
        fs::create_dir_all(&imports).unwrap();
        let final_path = imports.join("job-artifact.pdf");
        let staging = imports.join(".job-artifact.part");
        let winner = b"winner bytes";
        let loser = b"different loser bytes";
        fs::write(&final_path, winner).unwrap();
        fs::write(&staging, loser).unwrap();
        let input = InputArtifact {
            artifact_id: Uuid::new_v4().to_string(),
            media_type: INPUT_MEDIA_TYPE.to_string(),
            byte_size: loser.len() as u64,
            sha256: format!("{:x}", Sha256::digest(loser)),
            display_name: "report.pdf".to_string(),
            source_app_id: "email-watcher".to_string(),
        };
        let error = promote_staged_artifact(&staging, &final_path, &input, &imports)
            .expect_err("conflicting promotion should fail");

        assert_eq!(error.error.code, "ARTIFACT_STORAGE_CONFLICT");
        assert_eq!(fs::read(&final_path).unwrap(), winner);
        assert!(!staging.exists());

        let matching_staging = imports.join(".job-artifact-retry.part");
        fs::write(&matching_staging, winner).unwrap();
        let matching_input = InputArtifact {
            byte_size: winner.len() as u64,
            sha256: format!("{:x}", Sha256::digest(winner)),
            ..input
        };
        let (promoted, owns_import) = match promote_staged_artifact(
            &matching_staging,
            &final_path,
            &matching_input,
            &imports,
        ) {
            Ok(result) => result,
            Err(_) => panic!("matching promotion should reuse the existing import"),
        };

        assert_eq!(promoted, final_path);
        assert!(!owns_import);
        assert_eq!(fs::read(&promoted).unwrap(), winner);
        assert!(!matching_staging.exists());
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!("{label}-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("test directory should be created");
            #[cfg(windows)]
            crate::connect::windows_storage::protect_path_for_test(&path, true)
                .expect("Windows test root should be private");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn provider_workers_carry_job_deadline_and_shutdown_is_bounded() {
        let workers = ProviderWorkerOwner::new();
        let (observed_tx, observed_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        workers
            .spawn("blocked-provider-worker".to_string(), move |control| {
                observed_tx.send(control.request_timeout()).unwrap();
                let _ = release_rx.recv();
            })
            .unwrap();
        let observed_timeout = observed_rx.recv().unwrap().unwrap();
        assert!(observed_timeout <= CONNECT_JOB_REQUEST_TIMEOUT);
        assert!(observed_timeout > Duration::from_secs(29));

        let started = Instant::now();
        workers.shutdown_until(Instant::now() + Duration::from_millis(25));
        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(!workers.accepting());
        release_tx.send(()).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn slow_authenticated_body_does_not_block_package_quiesce_or_commit_a_job() {
        let root = TestDirectory::new("doc-sum-connect-slow-body-quiesce");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        fs::create_dir_all(&runtime_root).unwrap();
        fs::create_dir_all(&app_data).unwrap();
        let db_path = app_data.join("summarizer.db");
        let provider = ConnectProvider::start_at(
            db_path.clone(),
            app_data.clone(),
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        )
        .unwrap();
        let registration: RuntimeRegistration =
            serde_json::from_slice(&fs::read(provider.registration_path()).unwrap()).unwrap();
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let bytes = fs::read(source).unwrap();
        let request = fixture_request(&bytes);
        let request_json = serde_json::to_vec(&request).unwrap();
        let boundary = "stalled";
        let mut body = Vec::new();
        write!(
            body,
            "--{boundary}\r\nContent-Disposition: form-data; name=\"request\"\r\nContent-Type: application/json\r\n\r\n"
        )
        .unwrap();
        let request_start = body.len();
        body.extend_from_slice(&request_json);
        write!(
            body,
            "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"artifact\"; filename=\"attachment.pdf\"\r\nContent-Type: application/pdf\r\n\r\n"
        )
        .unwrap();
        body.extend_from_slice(&bytes);
        write!(body, "\r\n--{boundary}--\r\n").unwrap();
        let address = provider
            .base_url()
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/')
            .parse::<std::net::SocketAddr>()
            .unwrap();
        let mut stalled = std::net::TcpStream::connect(address).unwrap();
        write!(
            stalled,
            "POST /v1/jobs HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            registration.auth.token,
            body.len(),
        )
        .unwrap();
        stalled.write_all(&body[..request_start + 1]).unwrap();
        stalled.flush().unwrap();
        thread::sleep(Duration::from_millis(50));

        let started = Instant::now();
        package_control::begin_quiesce_at(&app_data.join("test-package-control")).unwrap();
        assert!(started.elapsed() < Duration::from_millis(500));
        stalled.write_all(&body[request_start + 1..]).unwrap();
        stalled
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut response = String::new();
        stalled.read_to_string(&mut response).unwrap();
        assert!(
            response.starts_with("HTTP/1.1 409 Conflict"),
            "unexpected response: {response:?}"
        );
        let conn = db::init_db(&db_path).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM connect_jobs", [], |row| row
                .get::<_, u32>(0))
                .unwrap(),
            0
        );
        assert!(fs::read_dir(app_data.join("connect-imports"))
            .unwrap()
            .next()
            .is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn startup_cancellation_before_publication_leaves_no_registration() {
        let root = TestDirectory::new("doc-sum-connect-startup-cancel");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        ensure_private_directory(&runtime_root).unwrap();
        ensure_private_directory(&app_data).unwrap();
        let checks = AtomicUsize::new(0);
        let result = ConnectProvider::start_at_with_entitlement_mode(
            app_data.join("summarizer.db"),
            app_data.clone(),
            runtime_root.clone(),
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
            EntitlementGate::always_active_for_test(),
            ProviderStartup {
                mode: ProviderMode::Foreground,
                stop_requested: &|| checks.fetch_add(1, Ordering::SeqCst) >= 2,
                stop_control: None,
            },
        );
        assert!(matches!(result, Err(ProviderStartError::StartupCancelled)));
        for version in ["v1", "v2"] {
            let providers = runtime_root.join(format!("local-connect/{version}/providers"));
            if providers.exists() {
                assert_eq!(
                    fs::read_dir(providers)
                        .unwrap()
                        .filter_map(Result::ok)
                        .filter(|entry| {
                            entry.path().extension().and_then(OsStr::to_str) == Some("json")
                        })
                        .count(),
                    0
                );
            }
        }
    }

    #[test]
    fn terminal_server_failure_is_observable_and_cleanup_removes_registration() {
        let root = TestDirectory::new("doc-sum-connect-terminal-server");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        #[cfg(unix)]
        ensure_private_directory(&runtime_root).unwrap();
        let provider = ConnectProvider::start_at(
            app_data.join("summarizer.db"),
            app_data,
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        )
        .unwrap();
        let registration_v1 = provider.registration_path().to_path_buf();
        let registration_v2 = provider.registration_path_v2().to_path_buf();
        provider.force_terminal_failure();
        assert!(matches!(
            provider.terminal_failure(),
            Some(ProviderStartError::Server(message)) if message == "forced server failure"
        ));
        provider.shutdown();
        assert!(!registration_v1.exists());
        assert!(!registration_v2.exists());
    }

    struct FixtureRuntime;

    fn fixture_profile_snapshot() -> ModelProfileSnapshot {
        let stage = ModelStageProfileSnapshot {
            runtime_kind: Default::default(),
            profile_id: "connect-fixture-profile".to_string(),
            model_name: "connect-fixture-model".to_string(),
            model_digest: "connect-fixture-digest".to_string(),
            context_tokens: 8_192,
            tokenizer_version: "connect-fixture-tokenizer".to_string(),
        };
        ModelProfileSnapshot {
            version: 1,
            preset_id: "connect-fixture-preset".to_string(),
            analysis: stage.clone(),
            verification: stage,
        }
    }

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
            "connect-fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "connect-fixture-model"
        }

        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            Some(fixture_profile_snapshot())
        }
    }

    struct BindingFixtureRuntime {
        observed: Arc<Mutex<Option<String>>>,
    }

    impl ModelRuntime for BindingFixtureRuntime {
        fn bind_run(&mut self, run_id: &str) {
            *self.observed.lock().unwrap() = Some(run_id.to_string());
        }

        fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            FixtureRuntime.generate(request)
        }

        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }

        fn runtime_id(&self) -> &str {
            FixtureRuntime.runtime_id()
        }

        fn model_id(&self) -> &str {
            FixtureRuntime.model_id()
        }

        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            Some(fixture_profile_snapshot())
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

    fn client() -> Client {
        Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .expect("test client should build")
    }

    #[cfg(target_os = "linux")]
    struct SlowLoopbackServer {
        port: u16,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    #[cfg(target_os = "linux")]
    impl SlowLoopbackServer {
        fn start() -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            listener.set_nonblocking(true).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let thread_stop = stop.clone();
            let thread = thread::spawn(move || {
                let mut accepted = Vec::new();
                while !thread_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => accepted.push(stream),
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                port,
                stop,
                thread: Some(thread),
            }
        }

        fn base_url(&self) -> String {
            format!("http://127.0.0.1:{}/", self.port)
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for SlowLoopbackServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn write_slow_v2_registrations(runtime_root: &Path, count: usize) -> Vec<SlowLoopbackServer> {
        let providers_v1 = runtime_root.join("local-connect/v1/providers");
        let providers_v2 = runtime_root.join("local-connect/v2/providers");
        ensure_private_directory(&providers_v1).unwrap();
        ensure_private_directory(&providers_v2).unwrap();
        let lock = acquire_registration_lock(
            &providers_v1.join(format!(".{APP_ID}.lifecycle.lock")),
            None,
            None,
        )
        .unwrap();
        (0..count)
            .map(|_| {
                let server = SlowLoopbackServer::start();
                let instance_id = Uuid::new_v4().to_string();
                write_registration(
                    &lock,
                    &providers_v2.join(format!("{APP_ID}-{instance_id}.json")),
                    &v2::RuntimeRegistration {
                        protocol_version: v2::PROTOCOL_VERSION,
                        instance_id,
                        app_id: APP_ID.to_string(),
                        pid: std::process::id(),
                        started_at: Utc::now(),
                        transport: TransportRegistration {
                            kind: v2::TRANSPORT_KIND.to_string(),
                            base_url: server.base_url(),
                        },
                        auth: AuthRegistration {
                            scheme: "bearer".to_string(),
                            token: "slow-probe-token".to_string(),
                        },
                    },
                    None,
                )
                .unwrap();
                server
            })
            .collect()
    }

    fn fixture_request(bytes: &[u8]) -> JobRequest {
        JobRequest {
            protocol_version: PROTOCOL_VERSION,
            job_id: Uuid::new_v4().to_string(),
            capability: CapabilityRef {
                id: CAPABILITY_ID.to_string(),
                version: CAPABILITY_VERSION.to_string(),
            },
            inputs: vec![InputArtifact {
                artifact_id: Uuid::new_v4().to_string(),
                media_type: INPUT_MEDIA_TYPE.to_string(),
                byte_size: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
                display_name: "quarterly-report.pdf".to_string(),
                source_app_id: "email-watcher".to_string(),
            }],
        }
    }

    fn fixture_request_v2(bytes: &[u8]) -> v2::JobRequest {
        v2::JobRequest {
            protocol_version: v2::PROTOCOL_VERSION,
            job_id: Uuid::new_v4().to_string(),
            capability: CapabilityRef {
                id: CAPABILITY_ID.to_string(),
                version: CAPABILITY_VERSION.to_string(),
            },
            inputs: vec![InputArtifact {
                artifact_id: Uuid::new_v4().to_string(),
                media_type: INPUT_MEDIA_TYPE.to_string(),
                byte_size: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
                display_name: "quarterly-report.pdf".to_string(),
                source_app_id: "email-watcher".to_string(),
            }],
            parameters: BTreeMap::new(),
        }
    }

    #[test]
    fn completed_connect_job_persists_a_bounded_whole_claim_prefix() {
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let bytes = fs::read(&source).unwrap();
        let request = fixture_request(&bytes);
        let (document, run) = prepare_pdf_ingestion(
            source.to_str().unwrap(),
            Some(&request.inputs[0].display_name),
        )
        .unwrap();
        let mut conn = db::init_db(":memory:").unwrap();
        let (_, accepted) = store::accept_job_with_ingestion(
            &mut conn,
            &request,
            &request.canonical_hash().unwrap(),
            source.to_str().unwrap(),
            &Uuid::new_v4().to_string(),
            &document,
            &run,
        )
        .unwrap();
        let processing = store::mark_processing(&conn, &accepted.job_id).unwrap();
        parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id).unwrap();
        normalize_document(&mut conn, &CanonicalNormalizer::new(), &run.run_id).unwrap();
        let normalized = db::get_normalized_document(&conn, &run.run_id)
            .unwrap()
            .unwrap();

        let claim_texts = ['a', 'b', 'c', 'd']
            .into_iter()
            .map(|letter| format!("{}.", letter.to_string().repeat(329_999)))
            .collect::<Vec<_>>();
        let evidence = (1u32..=4)
            .into_iter()
            .map(|page| EvidenceItem {
                evidence_id: format!("evidence-{page}"),
                chunk_id: format!("chunk-{page}"),
                block_id: format!("block-{page}"),
                claim_text: claim_texts[(page - 1) as usize].clone(),
                exact_quote: format!("quote-{page}"),
                source_span: SourceSpan {
                    page_start: page,
                    page_end: page,
                    section_id: None,
                    source_type: SourceType::NativeText,
                },
            })
            .collect::<Vec<_>>();
        let claims = (1u32..=4)
            .map(|page| CitedClaim {
                claim_id: format!("claim-{page}"),
                text: claim_texts[(page - 1) as usize].clone(),
                evidence_ids: vec![format!("evidence-{page}")],
            })
            .collect::<Vec<_>>();
        let rendered_text = claims
            .iter()
            .enumerate()
            .map(|(index, claim)| format!("{} [p. {}]", claim.text, index + 1))
            .collect::<Vec<_>>()
            .join("\n\n");
        let now = Utc::now();
        let summary = SummaryArtifacts {
            summary: SummaryArtifact {
                document_id: document.document_id.clone(),
                summary_version: crate::pipeline::summary::SUMMARY_VERSION.to_string(),
                text: rendered_text.clone(),
                warnings: Vec::new(),
                created_at: now,
                integrity_hash: "summary-integrity".to_string(),
            },
            citations: CitationArtifact {
                document_id: document.document_id,
                citation_version: crate::pipeline::summary::CITATION_VERSION.to_string(),
                summary_integrity_hash: "summary-integrity".to_string(),
                rendered_text,
                presentation_mode:
                    crate::pipeline::contracts::SummaryPresentationMode::LegacyClaimList,
                summary_claims: Vec::new(),
                claims,
                evidence,
                created_at: now,
                integrity_hash: "citation-integrity".to_string(),
            },
        };

        persist_completed_summary(&conn, &processing, &summary, &[], &normalized).unwrap();

        let persisted = store::get_job(&conn, &processing.job_id).unwrap().unwrap();
        assert_eq!(persisted.state, JobState::Completed);
        let content = &persisted.result.unwrap().outputs[0].content;
        let expected = summary.citations.claims[..3]
            .iter()
            .enumerate()
            .map(|(index, claim)| format!("{} [p. {}]", claim.text, index + 1))
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_eq!(content.text, expected);
        assert!(content.text.len() <= crate::connect::contracts::MAX_SUMMARY_TEXT_BYTES);
        assert_eq!(
            content
                .warnings
                .iter()
                .filter(|warning| {
                    warning.code
                        == crate::pipeline::summary::SUMMARY_TRUNCATED_FOR_DELIVERY_WARNING_CODE
                })
                .count(),
            1
        );
    }

    #[test]
    fn json_bounded_prefix_cannot_complete_below_page_coverage() {
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let bytes = fs::read(&source).unwrap();
        let request = fixture_request(&bytes);
        let (document, run) = prepare_pdf_ingestion(
            source.to_str().unwrap(),
            Some(&request.inputs[0].display_name),
        )
        .unwrap();
        let mut conn = db::init_db(":memory:").unwrap();
        let (_, accepted) = store::accept_job_with_ingestion(
            &mut conn,
            &request,
            &request.canonical_hash().unwrap(),
            source.to_str().unwrap(),
            &Uuid::new_v4().to_string(),
            &document,
            &run,
        )
        .unwrap();
        let processing = store::mark_processing(&conn, &accepted.job_id).unwrap();
        parse_document(&mut conn, &PdfExtractParser::new(), &run.run_id).unwrap();
        normalize_document(&mut conn, &CanonicalNormalizer::new(), &run.run_id).unwrap();
        let normalized = db::get_normalized_document(&conn, &run.run_id)
            .unwrap()
            .unwrap();

        let fixed_bytes = (1u32..=3)
            .map(|page| format!("Done. [p. {page}]").len())
            .sum::<usize>()
            + 4;
        let quote_bytes = crate::connect::contracts::MAX_SUMMARY_TEXT_BYTES - fixed_bytes - 1;
        let base = quote_bytes / 3;
        let remainder = quote_bytes % 3;
        let claim_texts = (0..3)
            .map(|index| {
                format!(
                    "{}Done.",
                    "\"".repeat(base + usize::from(index < remainder))
                )
            })
            .collect::<Vec<_>>();
        let evidence = (1u32..=3)
            .map(|page| EvidenceItem {
                evidence_id: format!("evidence-{page}"),
                chunk_id: format!("chunk-{page}"),
                block_id: format!("block-{page}"),
                claim_text: claim_texts[(page - 1) as usize].clone(),
                exact_quote: format!("quote-{page}"),
                source_span: SourceSpan {
                    page_start: page,
                    page_end: page,
                    section_id: None,
                    source_type: SourceType::NativeText,
                },
            })
            .collect::<Vec<_>>();
        let claims = (1u32..=3)
            .map(|page| CitedClaim {
                claim_id: format!("claim-{page}"),
                text: claim_texts[(page - 1) as usize].clone(),
                evidence_ids: vec![format!("evidence-{page}")],
            })
            .collect::<Vec<_>>();
        let rendered_text = claims
            .iter()
            .enumerate()
            .map(|(index, claim)| format!("{} [p. {}]", claim.text, index + 1))
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_eq!(
            rendered_text.len(),
            crate::connect::contracts::MAX_SUMMARY_TEXT_BYTES - 1
        );
        let now = Utc::now();
        let summary = SummaryArtifacts {
            summary: SummaryArtifact {
                document_id: document.document_id.clone(),
                summary_version: crate::pipeline::summary::SUMMARY_VERSION.to_string(),
                text: rendered_text.clone(),
                warnings: Vec::new(),
                created_at: now,
                integrity_hash: "summary-integrity".to_string(),
            },
            citations: CitationArtifact {
                document_id: document.document_id,
                citation_version: crate::pipeline::summary::CITATION_VERSION.to_string(),
                summary_integrity_hash: "summary-integrity".to_string(),
                rendered_text,
                presentation_mode:
                    crate::pipeline::contracts::SummaryPresentationMode::LegacyClaimList,
                summary_claims: Vec::new(),
                claims,
                evidence,
                created_at: now,
                integrity_hash: "citation-integrity".to_string(),
            },
        };
        let claim_lines = render_citation_claim_lines(&summary.citations).unwrap();
        let (_, delivered_claim_count) =
            JobResult::from_summary_claim_lines(&processing.input, &summary.summary, &claim_lines)
                .unwrap();
        assert_eq!(delivered_claim_count, 2);

        let error = persist_completed_summary(&conn, &processing, &summary, &[], &normalized)
            .expect_err("a JSON-bounded prefix below coverage must not complete");
        assert!(matches!(error, ProcessJobError::Contract(_)));
        assert_eq!(
            store::get_job(&conn, &processing.job_id)
                .unwrap()
                .unwrap()
                .state,
            JobState::Processing
        );
    }

    fn signed_test_entitlement(
        key: &Ed25519KeyPair,
        not_before: &str,
        expires_at: &str,
    ) -> Vec<u8> {
        let payload = serde_json::to_vec(&serde_json::json!({
            "format_version": 1,
            "entitlement_id": Uuid::new_v4().to_string(),
            "subject": "provider-test-customer",
            "features": [FEATURE_ID],
            "issued_at": not_before,
            "not_before": not_before,
            "expires_at": expires_at,
        }))
        .unwrap();
        serde_json::to_vec(&serde_json::json!({
            "format_version": 1,
            "key_id": "provider-test-key",
            "payload_base64url": URL_SAFE_NO_PAD.encode(&payload),
            "signature_base64url": URL_SAFE_NO_PAD.encode(key.sign(&payload).as_ref()),
        }))
        .unwrap()
    }

    fn write_private_entitlement(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        #[cfg(windows)]
        crate::connect::windows_storage::protect_path_for_test(path, false).unwrap();
    }

    #[test]
    fn entitlement_gates_manifest_jobs_and_status_while_registration_stays_owned() {
        let root = TestDirectory::new("doc-sum-connect-entitlement-gate");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        let entitlement_dir = root.0.join("entitlement");
        fs::create_dir_all(&entitlement_dir).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&entitlement_dir, fs::Permissions::from_mode(0o700)).unwrap();
        #[cfg(windows)]
        crate::connect::windows_storage::protect_path_for_test(&entitlement_dir, true).unwrap();
        #[cfg(windows)]
        fs::create_dir(&runtime_root).unwrap();
        let entitlement_path = entitlement_dir.join(ENTITLEMENT_FILE_NAME);
        let key_document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(key_document.as_ref()).unwrap();
        let active = signed_test_entitlement(&key, "2026-01-01T00:00:00Z", "2027-01-01T00:00:00Z");
        let expired = signed_test_entitlement(&key, "2025-01-01T00:00:00Z", "2026-01-01T00:00:00Z");
        write_private_entitlement(&entitlement_path, &active);
        let mut keys = BTreeMap::new();
        keys.insert(
            "provider-test-key".to_string(),
            key.public_key().as_ref().to_vec(),
        );
        let entitlement = EntitlementGate::for_test(
            entitlement_path.clone(),
            keys,
            DateTime::parse_from_rfc3339("2026-08-31T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        );
        let runtime_factory: RuntimeFactory =
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>));
        let provider = ConnectProvider::start_at_with_entitlement(
            app_data.join("summarizer.db"),
            app_data.clone(),
            runtime_root.clone(),
            DEFAULT_MAX_INPUT_BYTES,
            runtime_factory.clone(),
            entitlement.clone(),
        )
        .unwrap();
        let registration: RuntimeRegistration =
            serde_json::from_slice(&fs::read(provider.registration_path()).unwrap()).unwrap();
        let http = client();

        for version in [WireVersion::V1, WireVersion::V2] {
            let response = http
                .get(format!(
                    "{}{}",
                    provider.base_url(),
                    &version.manifest_path()[1..]
                ))
                .bearer_auth(&registration.auth.token)
                .send()
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let response = http
            .post(format!("{}v2/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .body("not multipart")
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: ErrorEnvelope = response.json().unwrap();
        assert_eq!(error.error.code, "MULTIPART_INVALID");

        write_private_entitlement(&entitlement_path, &expired);
        for version in [WireVersion::V1, WireVersion::V2] {
            let response = http
                .get(format!(
                    "{}{}",
                    provider.base_url(),
                    &version.manifest_path()[1..]
                ))
                .bearer_auth(&registration.auth.token)
                .send()
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            let error: ErrorEnvelope = response.json().unwrap();
            assert_eq!(error.protocol_version, version.protocol_version());
            assert_eq!(error.error.code, "CONNECT_ENTITLEMENT_REQUIRED");
        }
        let response = http
            .post(format!("{}v2/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .body("not multipart")
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let error: ErrorEnvelope = response.json().unwrap();
        assert_eq!(error.error.code, "CONNECT_ENTITLEMENT_REQUIRED");
        let response = http
            .post(format!("{}v2/jobs", provider.base_url()))
            .bearer_auth("not-the-registered-token")
            .body("not multipart")
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let error: ErrorEnvelope = response.json().unwrap();
        assert_eq!(error.error.code, "AUTHENTICATION_REQUIRED");
        #[cfg(unix)]
        assert!(probe_manifest(
            &http,
            provider.base_url(),
            "not-the-registered-token",
            WireVersion::V1,
            Instant::now() + Duration::from_secs(5),
        )
        .is_none());

        let bytes = b"%PDF-1.4\nentitlement denial\n%%EOF".to_vec();
        let request = fixture_request_v2(&bytes);
        let response = http
            .post(format!("{}v2/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .multipart(form_v2(&request, bytes))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let error: ErrorEnvelope = response.json().unwrap();
        assert_eq!(error.protocol_version, v2::PROTOCOL_VERSION);
        assert_eq!(error.error.code, "CONNECT_ENTITLEMENT_REQUIRED");

        let response = http
            .get(format!("{}v2/jobs/{}", provider.base_url(), request.job_id))
            .bearer_auth(&registration.auth.token)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        assert!(matches!(
            ConnectProvider::start_at_with_entitlement(
                app_data.join("summarizer.db"),
                app_data,
                runtime_root,
                DEFAULT_MAX_INPUT_BYTES,
                runtime_factory,
                entitlement,
            ),
            Err(ProviderStartError::ProviderAlreadyRunning)
        ));

        write_private_entitlement(&entitlement_path, &active);
        let response = http
            .get(format!("{}v2/manifest", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(provider.registration_path().exists());
        assert!(provider.registration_path_v2().exists());
    }

    #[test]
    fn entitlement_is_rechecked_before_job_persistence() {
        let root = TestDirectory::new("doc-sum-connect-entitlement-expiry-race");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        let entitlement_dir = root.0.join("entitlement");
        fs::create_dir_all(&entitlement_dir).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&entitlement_dir, fs::Permissions::from_mode(0o700)).unwrap();
        #[cfg(windows)]
        crate::connect::windows_storage::protect_path_for_test(&entitlement_dir, true).unwrap();
        #[cfg(windows)]
        fs::create_dir(&runtime_root).unwrap();
        let entitlement_path = entitlement_dir.join(ENTITLEMENT_FILE_NAME);
        let key_document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(key_document.as_ref()).unwrap();
        write_private_entitlement(
            &entitlement_path,
            &signed_test_entitlement(&key, "2026-01-01T00:00:00Z", "2027-01-01T00:00:00Z"),
        );
        let mut keys = BTreeMap::new();
        keys.insert(
            "provider-test-key".to_string(),
            key.public_key().as_ref().to_vec(),
        );
        let clock_calls = Arc::new(AtomicUsize::new(0));
        let entitlement = EntitlementGate::for_test_with_clock(entitlement_path, keys, {
            let clock_calls = Arc::clone(&clock_calls);
            move || {
                let timestamp = if clock_calls.fetch_add(1, Ordering::SeqCst) < 2 {
                    "2026-08-31T00:00:00Z"
                } else {
                    "2027-01-01T00:00:00Z"
                };
                DateTime::parse_from_rfc3339(timestamp)
                    .unwrap()
                    .with_timezone(&Utc)
            }
        });
        let runtime_factory: RuntimeFactory =
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>));
        let provider = ConnectProvider::start_at_with_entitlement(
            app_data.join("summarizer.db"),
            app_data.clone(),
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            runtime_factory,
            entitlement,
        )
        .unwrap();
        let registration: RuntimeRegistration =
            serde_json::from_slice(&fs::read(provider.registration_path()).unwrap()).unwrap();
        let bytes = b"%PDF-1.4\nentitlement expiry race\n%%EOF".to_vec();
        let request = fixture_request_v2(&bytes);
        let response = client()
            .post(format!("{}v2/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .multipart(form_v2(&request, bytes))
            .send()
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let error: ErrorEnvelope = response.json().unwrap();
        assert_eq!(error.error.code, "CONNECT_ENTITLEMENT_REQUIRED");
        assert_eq!(clock_calls.load(Ordering::SeqCst), 3);
        let conn = db::init_db(app_data.join("summarizer.db")).unwrap();
        assert!(store::get_job(&conn, &request.job_id).unwrap().is_none());
        assert_eq!(
            fs::read_dir(app_data.join("connect-imports"))
                .unwrap()
                .count(),
            0
        );
    }

    fn form(request: &JobRequest, bytes: Vec<u8>) -> multipart::Form {
        multipart::Form::new()
            .part(
                "request",
                multipart::Part::text(serde_json::to_string(request).unwrap())
                    .mime_str("application/json")
                    .unwrap(),
            )
            .part(
                "artifact",
                multipart::Part::bytes(bytes)
                    .file_name("attachment.pdf")
                    .mime_str("application/pdf")
                    .unwrap(),
            )
    }

    fn form_v2(request: &v2::JobRequest, bytes: Vec<u8>) -> multipart::Form {
        multipart::Form::new()
            .part(
                "request",
                multipart::Part::text(serde_json::to_string(request).unwrap())
                    .mime_str("application/json")
                    .unwrap(),
            )
            .part(
                "artifact",
                multipart::Part::bytes(bytes)
                    .file_name("attachment.pdf")
                    .mime_str("application/pdf")
                    .unwrap(),
            )
    }

    fn wait_for_terminal(client: &Client, base_url: &str, token: &str, job_id: &str) -> JobStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let status = client
                .get(format!("{base_url}v1/jobs/{job_id}"))
                .bearer_auth(token)
                .send()
                .expect("status request should succeed")
                .error_for_status()
                .expect("status request should be successful")
                .json::<JobStatus>()
                .expect("status should decode");
            if matches!(status.status, JobState::Completed | JobState::Failed) {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "job did not finish before timeout"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn wait_for_terminal_v2(
        client: &Client,
        base_url: &str,
        token: &str,
        job_id: &str,
    ) -> v2::JobStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let status = client
                .get(format!("{base_url}v2/jobs/{job_id}"))
                .bearer_auth(token)
                .send()
                .expect("v2 status request should succeed")
                .error_for_status()
                .expect("v2 status request should be successful")
                .json::<v2::JobStatus>()
                .expect("v2 status should decode");
            if matches!(status.status, JobState::Completed | JobState::Failed) {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "v2 job did not finish before timeout"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    #[cfg(unix)]
    #[test]
    fn provider_start_scavenges_stale_owned_registrations_and_preserves_foreign_files() {
        let root = TestDirectory::new("doc-sum-connect-registration-scavenge");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        let providers_v1 = runtime_root.join("local-connect/v1/providers");
        let providers_v2 = runtime_root.join("local-connect/v2/providers");
        ensure_private_directory(&providers_v1).unwrap();
        ensure_private_directory(&providers_v2).unwrap();
        ensure_private_directory(&app_data).unwrap();
        let registration_lock = acquire_registration_lock(
            &providers_v1.join(format!(".{APP_ID}.lifecycle.lock")),
            None,
            None,
        )
        .unwrap();

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let dead_base_url = format!(
            "http://127.0.0.1:{}/",
            listener.local_addr().unwrap().port()
        );
        drop(listener);
        let stale_v1_id = Uuid::new_v4().to_string();
        let stale_v2_id = Uuid::new_v4().to_string();
        let stale_v1_path = providers_v1.join(format!("{APP_ID}-{stale_v1_id}.json"));
        let stale_v2_path = providers_v2.join(format!("{APP_ID}-{stale_v2_id}.json"));
        let malformed_owned_path = providers_v1.join(format!("{APP_ID}-{}.json", Uuid::new_v4()));
        let empty_owned_path = providers_v1.join(format!("{APP_ID}-{}.json", Uuid::new_v4()));
        let max_sized_owned_path = providers_v1.join(format!("{APP_ID}-{}.json", Uuid::new_v4()));
        let oversized_owned_path = providers_v1.join(format!("{APP_ID}-{}.json", Uuid::new_v4()));
        let foreign_path = providers_v1.join(format!("translator-{}.json", Uuid::new_v4()));
        let similar_name_path = providers_v1.join(format!("{APP_ID}-not-a-uuid.json"));
        let auth = AuthRegistration {
            scheme: "bearer".to_string(),
            token: "stale-registration-token".to_string(),
        };
        write_registration(
            &registration_lock,
            &stale_v1_path,
            &RuntimeRegistration {
                protocol_version: PROTOCOL_VERSION,
                instance_id: stale_v1_id,
                app_id: APP_ID.to_string(),
                pid: u32::MAX,
                started_at: Utc::now(),
                transport: TransportRegistration {
                    kind: "http-loopback-v1".to_string(),
                    base_url: dead_base_url.clone(),
                },
                auth: auth.clone(),
            },
            None,
        )
        .unwrap();
        write_registration(
            &registration_lock,
            &stale_v2_path,
            &v2::RuntimeRegistration {
                protocol_version: v2::PROTOCOL_VERSION,
                instance_id: stale_v2_id,
                app_id: APP_ID.to_string(),
                pid: u32::MAX,
                started_at: Utc::now(),
                transport: TransportRegistration {
                    kind: v2::TRANSPORT_KIND.to_string(),
                    base_url: dead_base_url,
                },
                auth,
            },
            None,
        )
        .unwrap();
        fs::write(&malformed_owned_path, b"not-json").unwrap();
        fs::write(&empty_owned_path, b"").unwrap();
        let mut max_sized_registration = fs::read(&stale_v1_path).unwrap();
        max_sized_registration.resize(MAX_REGISTRATION_BYTES as usize, b' ');
        fs::write(&max_sized_owned_path, max_sized_registration).unwrap();
        fs::write(
            &oversized_owned_path,
            vec![b' '; MAX_REGISTRATION_BYTES as usize + 1],
        )
        .unwrap();
        fs::write(&foreign_path, b"foreign-provider").unwrap();
        fs::write(&similar_name_path, b"similar-name").unwrap();
        drop(registration_lock);

        let provider = ConnectProvider::start_at(
            app_data.join("summarizer.db"),
            app_data,
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        )
        .expect("provider should replace stale registrations");

        assert!(!stale_v1_path.exists());
        assert!(!stale_v2_path.exists());
        assert!(!malformed_owned_path.exists());
        assert!(!empty_owned_path.exists());
        assert!(!max_sized_owned_path.exists());
        assert!(!oversized_owned_path.exists());
        assert!(foreign_path.exists());
        assert!(similar_name_path.exists());
        assert!(provider.registration_path().exists());
        assert!(provider.registration_path_v2().exists());
    }

    #[cfg(unix)]
    #[test]
    fn provider_start_preserves_live_and_serializes_replacement_cleanup() {
        let root = TestDirectory::new("doc-sum-connect-live-registration");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        ensure_private_directory(&runtime_root).unwrap();
        ensure_private_directory(&app_data).unwrap();
        let db_path = app_data.join("summarizer.db");
        let runtime_factory: RuntimeFactory =
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>));
        let first = Arc::new(
            ConnectProvider::start_at(
                db_path.clone(),
                app_data.clone(),
                runtime_root.clone(),
                DEFAULT_MAX_INPUT_BYTES,
                runtime_factory.clone(),
            )
            .expect("first provider should start"),
        );
        let registration_path_v1 = first.registration_path().to_path_buf();
        let registration_path_v2 = first.registration_path_v2().to_path_buf();
        let registration_bytes_v1 = fs::read(&registration_path_v1).unwrap();
        let registration_bytes_v2 = fs::read(&registration_path_v2).unwrap();
        let registration: RuntimeRegistration =
            serde_json::from_slice(&registration_bytes_v1).unwrap();

        let second = ConnectProvider::start_at(
            db_path,
            app_data,
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            runtime_factory,
        );

        assert!(matches!(
            second,
            Err(ProviderStartError::ProviderAlreadyRunning)
        ));
        assert_eq!(
            fs::read(&registration_path_v1).unwrap(),
            registration_bytes_v1
        );
        assert_eq!(
            fs::read(&registration_path_v2).unwrap(),
            registration_bytes_v2
        );
        assert_eq!(
            client()
                .get(format!("{}v1/manifest", first.base_url()))
                .bearer_auth(&registration.auth.token)
                .send()
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let replacement_base_url = "http://127.0.0.1:49152/";
        let replacement_token = "replacement-provider-token";
        let mut replacement: v2::RuntimeRegistration =
            serde_json::from_slice(&registration_bytes_v2).unwrap();
        replacement.transport.base_url = replacement_base_url.to_string();
        replacement.auth.token = replacement_token.to_string();
        let publication_lock =
            acquire_registration_lock(&first.registration_lock_path, None, None).unwrap();
        let cleanup_provider = Arc::clone(&first);
        let (cleanup_started_tx, cleanup_started_rx) = mpsc::sync_channel(1);
        let (cleanup_finished_tx, cleanup_finished_rx) = mpsc::sync_channel(1);
        let cleanup_thread = thread::spawn(move || {
            cleanup_started_tx.send(()).unwrap();
            cleanup_provider.unregister();
            cleanup_finished_tx.send(()).unwrap();
        });
        cleanup_started_rx.recv().unwrap();
        assert!(matches!(
            cleanup_finished_rx.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        write_registration(&publication_lock, &registration_path_v2, &replacement, None).unwrap();
        drop(publication_lock);
        cleanup_finished_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        cleanup_thread.join().unwrap();

        assert!(!registration_path_v1.exists());
        assert!(registration_path_v2.exists());
        let removal_lock =
            acquire_registration_lock(&first.registration_lock_path, None, None).unwrap();
        assert!(remove_registration_if_owned(
            &removal_lock,
            &registration_path_v2,
            WireVersion::V2,
            first.instance_id_v2(),
            replacement_base_url,
            replacement_token,
            None,
        )
        .unwrap());
        assert!(!registration_path_v2.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn provider_identity_helper_process() {
        if env::var_os("DOC_SUM_PROVIDER_IDENTITY_HELPER").is_some() {
            thread::sleep(Duration::from_secs(30));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn readiness_rejects_correct_executable_pid_with_foreign_listener() {
        let root = TestDirectory::new("doc-sum-connect-readiness-socket-owner");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        ensure_private_directory(&runtime_root).unwrap();
        ensure_private_directory(&app_data).unwrap();
        let provider = ConnectProvider::start_at(
            app_data.join("summarizer.db"),
            app_data,
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        )
        .unwrap();
        let path = provider.registration_path_v2();
        let actual: v2::RuntimeRegistration =
            serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let mut helper = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "connect::provider::tests::provider_identity_helper_process",
                "--nocapture",
            ])
            .env("DOC_SUM_PROVIDER_IDENTITY_HELPER", "1")
            .spawn()
            .unwrap();
        let mut fake = actual.clone();
        fake.pid = helper.id();
        fs::write(path, serde_json::to_vec_pretty(&fake).unwrap()).unwrap();
        assert_eq!(unsafe { libc::kill(helper.id() as i32, libc::SIGSTOP) }, 0);
        let expected =
            expected_provider_process(unsafe { libc::geteuid() }, &env::current_exe().unwrap())
                .unwrap();

        let accepted = registration_proves_live(
            &client(),
            path,
            WireVersion::V2,
            &fake.instance_id,
            None,
            Some(&expected),
            Instant::now() + Duration::from_secs(5),
        );

        helper.kill().unwrap();
        helper.wait().unwrap();
        assert!(!accepted);
        fs::write(path, serde_json::to_vec_pretty(&actual).unwrap()).unwrap();
        assert!(registration_proves_live(
            &client(),
            path,
            WireVersion::V2,
            &actual.instance_id,
            None,
            Some(&expected),
            Instant::now() + Duration::from_secs(5),
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn readiness_deadline_bounds_many_slow_candidates() {
        let root = TestDirectory::new("doc-sum-connect-readiness-deadline");
        let runtime_root = root.0.join("runtime");
        ensure_private_directory(&runtime_root).unwrap();
        let _servers = write_slow_v2_registrations(&runtime_root, 4);
        let expected =
            expected_provider_process(unsafe { libc::geteuid() }, &env::current_exe().unwrap())
                .unwrap();
        let started = Instant::now();

        assert!(wait_for_registered_provider(
            &runtime_root,
            None,
            &expected,
            started + Duration::from_millis(120),
        )
        .is_err());
        assert!(started.elapsed() < Duration::from_millis(350));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cleanup_deadline_bounds_many_slow_candidates() {
        let root = TestDirectory::new("doc-sum-connect-cleanup-deadline");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        ensure_private_directory(&runtime_root).unwrap();
        ensure_private_directory(&app_data).unwrap();
        let _servers = write_slow_v2_registrations(&runtime_root, 4);
        let started = Instant::now();

        assert!(stop_and_cleanup_registered_provider(
            &app_data,
            &runtime_root,
            started + Duration::from_millis(120),
        )
        .is_err());
        assert!(started.elapsed() < Duration::from_millis(350));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn readiness_rejects_live_manifest_with_wrong_process_executable() {
        let root = TestDirectory::new("doc-sum-connect-readiness-process-identity");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        ensure_private_directory(&runtime_root).unwrap();
        ensure_private_directory(&app_data).unwrap();
        let provider = ConnectProvider::start_at(
            app_data.join("summarizer.db"),
            app_data,
            runtime_root.clone(),
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        )
        .unwrap();
        let path = provider.registration_path_v2();
        let actual: v2::RuntimeRegistration =
            serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let mut fake = actual.clone();
        fake.pid = unsafe { libc::getppid() as u32 };
        fs::write(path, serde_json::to_vec_pretty(&fake).unwrap()).unwrap();
        let client = client();
        let expected =
            expected_provider_process(unsafe { libc::geteuid() }, &env::current_exe().unwrap())
                .unwrap();

        assert!(!registration_proves_live(
            &client,
            path,
            WireVersion::V2,
            &fake.instance_id,
            None,
            Some(&expected),
            Instant::now() + Duration::from_secs(5),
        ));
        assert_eq!(
            registered_provider_state(
                &runtime_root,
                Some(&fake.instance_id),
                &expected,
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap(),
            RegisteredProviderState::PresentUnready
        );

        fs::write(path, serde_json::to_vec_pretty(&actual).unwrap()).unwrap();
        assert!(registration_proves_live(
            &client,
            path,
            WireVersion::V2,
            &actual.instance_id,
            None,
            Some(&expected),
            Instant::now() + Duration::from_secs(5),
        ));
        assert_eq!(
            registered_provider_state(
                &runtime_root,
                Some(&actual.instance_id),
                &expected,
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap(),
            RegisteredProviderState::Ready
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn registered_process_guard_rejects_process_exit_after_identity_capture() {
        let root = TestDirectory::new("doc-sum-connect-process-exit-race");
        let executable = root.0.join("provider-process");
        fs::copy("/usr/bin/sleep", &executable).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let mut child = std::process::Command::new(&executable)
            .arg("30")
            .spawn()
            .unwrap();
        let expected = expected_provider_process(unsafe { libc::geteuid() }, &executable).unwrap();
        let guard = RegisteredProcessGuard::open(child.id(), expected).unwrap();

        child.kill().unwrap();
        child.wait().unwrap();

        assert!(!guard.revalidate());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn socket_identity_probes_release_held_process_descriptors() {
        if env::var_os("DOC_SUM_PROVIDER_FD_PROBE_HELPER").is_none() {
            let status = Command::new(env::current_exe().unwrap())
                .args([
                    "--exact",
                    "connect::provider::tests::socket_identity_probes_release_held_process_descriptors",
                    "--nocapture",
                ])
                .env("DOC_SUM_PROVIDER_FD_PROBE_HELPER", "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let root = TestDirectory::new("doc-sum-connect-readiness-fd-lifetime");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        ensure_private_directory(&runtime_root).unwrap();
        ensure_private_directory(&app_data).unwrap();
        let provider = ConnectProvider::start_at(
            app_data.join("summarizer.db"),
            app_data,
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        )
        .unwrap();
        let path = provider.registration_path_v2();
        let registration: v2::RuntimeRegistration =
            serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let expected =
            expected_provider_process(unsafe { libc::geteuid() }, &env::current_exe().unwrap())
                .unwrap();
        let http = client();
        assert!(registration_proves_live(
            &http,
            path,
            WireVersion::V2,
            &registration.instance_id,
            None,
            Some(&expected),
            Instant::now() + Duration::from_secs(5),
        ));
        let baseline = fs::read_dir("/proc/self/fd").unwrap().count();

        for _ in 0..20 {
            assert!(registration_proves_live(
                &http,
                path,
                WireVersion::V2,
                &registration.instance_id,
                None,
                Some(&expected),
                Instant::now() + Duration::from_secs(5),
            ));
        }

        assert_eq!(fs::read_dir("/proc/self/fd").unwrap().count(), baseline);
    }

    #[cfg(windows)]
    #[test]
    fn windows_provider_uses_fixed_private_registrations_and_lifetime_locks() {
        let root = TestDirectory::new("doc-sum-connect-windows-provider");
        let app_data = root.0.join("app-data");
        fs::create_dir(&app_data).unwrap();
        let provider = ConnectProvider::start_at(
            app_data.join("summarizer.db"),
            app_data,
            root.0.clone(),
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        )
        .unwrap();
        let connect_root = root.0.join(windows_storage::LOCAL_CONNECT_DIRECTORY);
        assert_eq!(
            provider.registration_path(),
            connect_root
                .join("runtime/v1/providers")
                .join(format!("local-connect-v1-{APP_ID}.json"))
        );
        assert_eq!(
            provider.registration_path_v2(),
            connect_root.join("runtime/v2/providers").join(format!(
                "local-connect-v2-{}.json",
                provider.instance_id_v2()
            ))
        );
        let v1_lock = connect_root
            .join("runtime/v1/locks")
            .join(format!(".local-connect-v1-{APP_ID}.lock"));
        let v2_lock = connect_root.join("runtime/v2/locks").join(format!(
            ".local-connect-v2-{}.lock",
            provider.instance_id_v2()
        ));
        assert!(matches!(
            WindowsFileLock::acquire(&v1_lock, &root.0),
            Err(FileLockError::Busy)
        ));
        assert!(matches!(
            WindowsFileLock::acquire(&v2_lock, &root.0),
            Err(FileLockError::Busy)
        ));
        let server_address = provider
            .base_url()
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/')
            .parse::<std::net::SocketAddr>()
            .unwrap();
        let registration_v1 = provider.registration_path().to_path_buf();
        let registration_v2 = provider.registration_path_v2().to_path_buf();
        drop(provider);
        assert!(std::net::TcpStream::connect(server_address).is_err());
        assert!(!registration_v1.exists());
        assert!(!registration_v2.exists());
        assert_eq!(fs::read(v1_lock).unwrap(), [0]);
        assert_eq!(fs::read(v2_lock).unwrap(), [0]);
    }

    #[cfg(windows)]
    #[test]
    fn windows_provider_shutdown_is_bounded_with_an_incomplete_request_body() {
        let root = TestDirectory::new("doc-sum-connect-windows-bounded-shutdown");
        let app_data = root.0.join("app-data");
        fs::create_dir(&app_data).unwrap();
        let provider = ConnectProvider::start_at(
            app_data.join("summarizer.db"),
            app_data,
            root.0.clone(),
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        )
        .unwrap();
        let server_address = provider
            .base_url()
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/')
            .parse::<std::net::SocketAddr>()
            .unwrap();
        let mut stalled = std::net::TcpStream::connect(server_address).unwrap();
        std::io::Write::write_all(
            &mut stalled,
            b"POST /v1/jobs HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: multipart/form-data; boundary=stalled\r\nContent-Length: 1048576\r\n\r\n--stalled\r\nContent-Disposition: form-data; name=\"request\"\r\n\r\n",
        )
        .unwrap();
        std::io::Write::flush(&mut stalled).unwrap();
        thread::sleep(Duration::from_millis(50));

        let (finished_tx, finished_rx) = mpsc::sync_channel(1);
        let shutdown = thread::spawn(move || {
            drop(provider);
            finished_tx.send(()).unwrap();
        });
        finished_rx
            .recv_timeout(WINDOWS_SERVER_SHUTDOWN_GRACE + Duration::from_secs(2))
            .expect("provider shutdown should force an incomplete connection closed");
        drop(stalled);
        shutdown.join().unwrap();
        assert!(std::net::TcpStream::connect(server_address).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_provider_rejects_hostile_registration_ancestor() {
        let root = TestDirectory::new("doc-sum-connect-windows-hostile-acl");
        let connect_root = windows_storage::prepare_local_connect_root(&root.0).unwrap();
        let providers = connect_root.join("runtime/v1/providers");
        windows_storage::ensure_private_directory(&providers, &root.0).unwrap();
        let status = Command::new("icacls")
            .arg(&providers)
            .args(["/grant", "*S-1-1-0:R"])
            .status()
            .unwrap();
        assert!(status.success());
        let app_data = root.0.join("app-data");
        fs::create_dir(&app_data).unwrap();

        let result = ConnectProvider::start_at(
            app_data.join("summarizer.db"),
            app_data,
            root.0.clone(),
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        );

        assert!(matches!(result, Err(ProviderStartError::Io(_))));
    }

    #[cfg(windows)]
    #[test]
    fn competing_windows_provider_cannot_fail_the_live_owners_active_job() {
        let root = TestDirectory::new("doc-sum-connect-windows-provider-race");
        let app_data = root.0.join("app-data");
        fs::create_dir(&app_data).unwrap();
        let db_path = app_data.join("summarizer.db");
        let runtime_factory: RuntimeFactory =
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>));
        let provider = ConnectProvider::start_at(
            db_path.clone(),
            app_data.clone(),
            root.0.clone(),
            DEFAULT_MAX_INPUT_BYTES,
            runtime_factory.clone(),
        )
        .unwrap();
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let bytes = fs::read(&source).unwrap();
        let import_path = app_data.join("connect-imports/owned-active.pdf");
        fs::copy(&source, &import_path).unwrap();
        let (document, run) =
            prepare_pdf_ingestion(import_path.to_str().unwrap(), Some("owned-active.pdf")).unwrap();
        let request = fixture_request(&bytes);
        let mut conn = db::init_db(&db_path).unwrap();
        store::accept_job_with_ingestion(
            &mut conn,
            &request,
            "live-owner-request",
            import_path.to_str().unwrap(),
            provider.instance_id(),
            &document,
            &run,
        )
        .unwrap();
        drop(conn);

        let competing = ConnectProvider::start_at(
            db_path.clone(),
            app_data,
            root.0.clone(),
            DEFAULT_MAX_INPUT_BYTES,
            runtime_factory,
        );
        assert!(matches!(
            competing,
            Err(ProviderStartError::ProviderAlreadyRunning)
        ));

        let conn = db::init_db(&db_path).unwrap();
        let stored = store::get_job(&conn, &request.job_id).unwrap().unwrap();
        assert_eq!(stored.state, JobState::Accepted);
        assert!(stored.error.is_none());
        drop(provider);
    }

    #[test]
    fn supervised_restart_reuses_v2_identity_and_exposes_interrupted_failure() {
        let root = TestDirectory::new("doc-sum-connect-v2-restart");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        fs::create_dir_all(&runtime_root).unwrap();
        fs::create_dir_all(&app_data).unwrap();
        let db_path = app_data.join("summarizer.db");
        let runtime_factory: RuntimeFactory =
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>));
        let first = ConnectProvider::start_at(
            db_path.clone(),
            app_data.clone(),
            runtime_root.clone(),
            DEFAULT_MAX_INPUT_BYTES,
            runtime_factory.clone(),
        )
        .unwrap();
        let first_v1: RuntimeRegistration =
            serde_json::from_slice(&fs::read(first.registration_path()).unwrap()).unwrap();
        let first_v2: v2::RuntimeRegistration =
            serde_json::from_slice(&fs::read(first.registration_path_v2()).unwrap()).unwrap();

        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let bytes = fs::read(&source).unwrap();
        let import_path = app_data.join("connect-imports/interrupted.pdf");
        fs::copy(&source, &import_path).unwrap();
        let (document, run) =
            prepare_pdf_ingestion(import_path.to_str().unwrap(), Some("interrupted.pdf")).unwrap();
        let mut request = fixture_request(&bytes);
        request.protocol_version = v2::PROTOCOL_VERSION;
        let mut conn = db::init_db(&db_path).unwrap();
        store::accept_job_with_ingestion(
            &mut conn,
            &request,
            "seeded-v2-request",
            import_path.to_str().unwrap(),
            &first_v2.instance_id,
            &document,
            &run,
        )
        .unwrap();
        let persisted = db::get_pipeline_run(&conn, &run.run_id).unwrap().unwrap();
        db::start_parsing(&mut conn, &run.run_id, persisted.state_version).unwrap();
        drop(conn);
        let stale_v1 = first.registration_path().to_path_buf();
        let stale_v2 = first.registration_path_v2().to_path_buf();
        first.simulate_process_loss();
        assert!(stale_v1.exists());
        assert!(stale_v2.exists());
        fs::remove_file(app_data.join(V2_INSTANCE_ID_FILE)).unwrap();

        let second = ConnectProvider::start_at(
            db_path,
            app_data.clone(),
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            runtime_factory,
        )
        .unwrap();
        let second_v1: RuntimeRegistration =
            serde_json::from_slice(&fs::read(second.registration_path()).unwrap()).unwrap();
        let second_v2: v2::RuntimeRegistration =
            serde_json::from_slice(&fs::read(second.registration_path_v2()).unwrap()).unwrap();

        assert_ne!(first_v1.instance_id, second_v1.instance_id);
        assert_eq!(first_v2.instance_id, second_v2.instance_id);
        assert_ne!(first_v2.auth.token, second_v2.auth.token);
        assert_eq!(
            fs::read_to_string(app_data.join(V2_INSTANCE_ID_FILE)).unwrap(),
            format!("{}\n", second_v2.instance_id)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(app_data.join(V2_INSTANCE_ID_FILE))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }

        let status = client()
            .get(format!("{}v2/jobs/{}", second.base_url(), request.job_id))
            .bearer_auth(&second_v2.auth.token)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<v2::JobStatus>()
            .unwrap();
        assert_eq!(status.provider.instance_id, first_v2.instance_id);
        assert_eq!(status.status, JobState::Failed);
        assert_eq!(status.error.unwrap().code, "PROVIDER_RESTARTED");
        let recovered_run = db::get_pipeline_run(
            &db::init_db(app_data.join("summarizer.db")).unwrap(),
            &run.run_id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(recovered_run.state, PipelineState::Failed);
        assert_eq!(
            recovered_run.failure.unwrap().code,
            crate::pipeline::recovery::INTERRUPTION_FAILURE_CODE
        );
    }

    #[cfg(unix)]
    #[test]
    fn connect_owned_provider_remains_available_after_foreground_exit() {
        let root = TestDirectory::new("doc-sum-connect-background-owner");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        ensure_private_directory(&runtime_root).unwrap();
        ensure_private_directory(&app_data).unwrap();
        let db_path = app_data.join("summarizer.db");
        let provider = ConnectProvider::start_at(
            db_path.clone(),
            app_data.clone(),
            runtime_root.clone(),
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        )
        .unwrap();
        let registration: RuntimeRegistration =
            serde_json::from_slice(&fs::read(provider.registration_path()).unwrap()).unwrap();
        let registration_v1 = provider.registration_path().to_path_buf();
        let registration_v2 = provider.registration_path_v2().to_path_buf();
        let server_address = provider
            .base_url()
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/')
            .parse::<std::net::SocketAddr>()
            .unwrap();

        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let mut conn = db::init_db(&db_path).unwrap();
        let (_, active_run) = ingest_pdf(&mut conn, source.to_str().unwrap()).unwrap();
        let (active_run, _) =
            db::start_parsing(&mut conn, &active_run.run_id, active_run.state_version).unwrap();
        drop(conn);

        let foreground = ConnectProvider::start_at(
            db_path.clone(),
            app_data.clone(),
            runtime_root.clone(),
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        );
        assert!(matches!(
            foreground,
            Err(ProviderStartError::ProviderAlreadyRunning)
        ));
        assert_eq!(
            reconcile_standalone_state_if_unowned(&db_path, &app_data).unwrap(),
            None
        );
        let conn = db::init_db(&db_path).unwrap();
        assert_eq!(
            db::get_pipeline_run(&conn, &active_run.run_id)
                .unwrap()
                .unwrap()
                .state,
            PipelineState::Parsing
        );

        assert_eq!(
            client()
                .get(format!("{}v1/manifest", provider.base_url()))
                .bearer_auth(registration.auth.token)
                .send()
                .unwrap()
                .status(),
            StatusCode::OK
        );
        provider.shutdown();
        assert!(std::net::TcpStream::connect(server_address).is_err());
        assert!(!registration_v1.exists());
        assert!(!registration_v2.exists());
        assert_eq!(
            reconcile_standalone_state_if_unowned(&db_path, &app_data).unwrap(),
            Some(1)
        );
        assert_eq!(
            db::get_pipeline_run(&db::init_db(&db_path).unwrap(), &active_run.run_id)
                .unwrap()
                .unwrap()
                .state,
            PipelineState::Failed
        );
    }

    #[test]
    fn deliberate_owner_shutdown_cancels_and_joins_every_worker() {
        use crate::pipeline::control::ExecutionControl;
        use std::sync::atomic::{AtomicBool, Ordering};

        let workers = ProviderWorkerOwner::new();
        let worker_exited = Arc::new(AtomicBool::new(false));
        let exited = Arc::clone(&worker_exited);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        workers
            .spawn("lifecycle-test".to_string(), move |cancellation| {
                started_tx.send(()).unwrap();
                while !cancellation.cancellation_requested() {
                    thread::sleep(Duration::from_millis(5));
                }
                exited.store(true, Ordering::Release);
            })
            .unwrap();
        started_rx.recv().unwrap();

        workers.shutdown();

        assert!(worker_exited.load(Ordering::Acquire));
        assert_eq!(workers.retained_worker_count(), 0);
        assert!(workers.spawn("late-worker".to_string(), |_| {}).is_err());
    }

    #[test]
    fn completed_workers_are_reaped_during_steady_state() {
        let workers = ProviderWorkerOwner::new();
        for ordinal in 0..64 {
            workers
                .spawn(format!("completed-worker-{ordinal}"), |_| {})
                .unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while workers.retained_worker_count() != 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(workers.retained_worker_count(), 0);
    }

    #[test]
    fn provider_rejects_invalid_persisted_v2_instance_identity() {
        let root = TestDirectory::new("doc-sum-connect-v2-invalid-instance");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        fs::create_dir_all(&runtime_root).unwrap();
        fs::create_dir_all(&app_data).unwrap();
        fs::write(app_data.join(V2_INSTANCE_ID_FILE), "not-a-uuid\n").unwrap();

        let result = ConnectProvider::start_at(
            app_data.join("summarizer.db"),
            app_data,
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        );

        assert!(matches!(
            result,
            Err(ProviderStartError::InvalidInstanceIdentity)
        ));
    }

    #[test]
    fn provider_rejects_persisted_v2_identity_that_conflicts_with_active_job() {
        let root = TestDirectory::new("doc-sum-connect-v2-conflicting-instance");
        let app_data = root.0.join("app-data");
        ensure_private_directory(&app_data).unwrap();
        fs::write(
            app_data.join(V2_INSTANCE_ID_FILE),
            format!("{}\n", Uuid::new_v4()),
        )
        .unwrap();

        let result = load_or_create_v2_instance_id(&app_data, Some(&Uuid::new_v4().to_string()));

        assert!(matches!(
            result,
            Err(ProviderStartError::InvalidInstanceIdentity)
        ));
    }

    #[test]
    fn connect_runtime_is_selected_before_job_acceptance() {
        let root = TestDirectory::new("doc-sum-connect-runtime-admission");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        fs::create_dir_all(&runtime_root).unwrap();
        fs::create_dir_all(&app_data).unwrap();
        let db_path = app_data.join("summarizer.db");
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let bytes = fs::read(source).expect("fixture should read");
        let request = fixture_request(&bytes);
        let observed_before_acceptance = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&observed_before_acceptance);
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&factory_calls);
        let factory_db_path = db_path.clone();
        let job_id = request.job_id.clone();
        let observed_binding = Arc::new(Mutex::new(None));
        let factory_observed_binding = Arc::clone(&observed_binding);
        let runtime_factory: RuntimeFactory = Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            let conn = db::init_db(&factory_db_path).expect("provider database should open");
            observed.store(
                store::get_job(&conn, &job_id)
                    .expect("job lookup should succeed")
                    .is_none(),
                Ordering::SeqCst,
            );
            Ok(Box::new(BindingFixtureRuntime {
                observed: Arc::clone(&factory_observed_binding),
            }) as Box<dyn ModelRuntime>)
        });
        let provider = ConnectProvider::start_at(
            db_path.clone(),
            app_data,
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            runtime_factory,
        )
        .expect("provider should start");
        let registration: RuntimeRegistration =
            serde_json::from_slice(&fs::read(provider.registration_path()).unwrap()).unwrap();
        let http = client();

        let accepted = http
            .post(format!("{}v1/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .multipart(form(&request, bytes))
            .send()
            .expect("job submission should succeed");
        assert_eq!(accepted.status(), StatusCode::ACCEPTED);
        assert_eq!(factory_calls.load(Ordering::SeqCst), 1);
        assert!(observed_before_acceptance.load(Ordering::SeqCst));
        let conn = db::init_db(&db_path).expect("provider database should reopen");
        let accepted_job = store::get_job(&conn, &request.job_id)
            .expect("accepted job should be readable")
            .expect("accepted job should exist");
        assert_eq!(
            *observed_binding.lock().unwrap(),
            Some(accepted_job.pipeline_run_id.clone())
        );
        assert_eq!(
            db::get_run_model_profile(&conn, &accepted_job.pipeline_run_id)
                .expect("accepted profile should be readable"),
            Some(fixture_profile_snapshot())
        );
        assert_eq!(
            wait_for_terminal(
                &http,
                provider.base_url(),
                &registration.auth.token,
                &request.job_id,
            )
            .status,
            JobState::Completed
        );
    }

    #[test]
    fn connect_rejects_a_snapshotless_runtime_before_job_acceptance() {
        let root = TestDirectory::new("doc-sum-connect-snapshotless-runtime");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        fs::create_dir_all(&runtime_root).unwrap();
        fs::create_dir_all(&app_data).unwrap();
        let db_path = app_data.join("summarizer.db");
        let provider = ConnectProvider::start_at(
            db_path.clone(),
            app_data.clone(),
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(SnapshotlessFixtureRuntime) as Box<dyn ModelRuntime>)),
        )
        .expect("provider should start");
        let registration: RuntimeRegistration =
            serde_json::from_slice(&fs::read(provider.registration_path()).unwrap()).unwrap();
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let bytes = fs::read(source).expect("fixture should read");
        let request = fixture_request(&bytes);

        let response = client()
            .post(format!("{}v1/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .multipart(form(&request, bytes))
            .send()
            .expect("snapshot rejection should return a response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let error = response.json::<ErrorEnvelope>().unwrap().error;
        assert_eq!(error.code, "MODEL_CONFIG_INVALID");
        assert!(!error.retryable);
        let conn = db::init_db(&db_path).expect("provider database should reopen");
        assert!(store::get_job(&conn, &request.job_id).unwrap().is_none());
        assert!(fs::read_dir(app_data.join("connect-imports"))
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn process_job_errors_preserve_typed_pipeline_recoverability() {
        for (code, recoverable) in [
            ("MODEL_NOT_AVAILABLE", true),
            ("MODEL_TOKENIZER_INVALID", false),
        ] {
            let error =
                ProcessJobError::Service(crate::pipeline::service::DocumentServiceError::Summary(
                    SummaryPipelineError::StageFailed(PipelineFailure {
                        code: code.to_string(),
                        message: "fixture failure".to_string(),
                        stage: Some(PipelineStage::Analyze),
                        recoverable,
                    }),
                ))
                .public_error();
            assert_eq!(error.code, code);
            assert_eq!(error.retryable, recoverable);
        }
    }

    #[test]
    fn connect_runtime_failure_precedes_admission_and_removes_the_import() {
        let root = TestDirectory::new("doc-sum-connect-runtime-failure");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        fs::create_dir_all(&runtime_root).unwrap();
        fs::create_dir_all(&app_data).unwrap();
        let db_path = app_data.join("summarizer.db");
        let provider = ConnectProvider::start_at(
            db_path.clone(),
            app_data.clone(),
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| {
                Err(ModelRuntimeFailure {
                    code: "MODEL_NOT_AVAILABLE".to_string(),
                    message: "Fixture model is unavailable.".to_string(),
                    recoverable: true,
                    request_attempts: Vec::new(),
                })
            }),
        )
        .expect("provider should start");
        let registration: RuntimeRegistration =
            serde_json::from_slice(&fs::read(provider.registration_path()).unwrap()).unwrap();
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let bytes = fs::read(source).expect("fixture should read");
        let request = fixture_request(&bytes);

        let response = client()
            .post(format!("{}v1/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .multipart(form(&request, bytes))
            .send()
            .expect("runtime rejection should return a response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.json::<ErrorEnvelope>().unwrap().error.code,
            "MODEL_NOT_AVAILABLE"
        );
        let conn = db::init_db(&db_path).expect("provider database should reopen");
        assert!(store::get_job(&conn, &request.job_id).unwrap().is_none());
        assert!(fs::read_dir(app_data.join("connect-imports"))
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn concurrent_identical_acceptance_wins_over_a_late_runtime_failure() {
        let root = TestDirectory::new("doc-sum-connect-runtime-idempotency-race");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        fs::create_dir_all(&runtime_root).unwrap();
        fs::create_dir_all(&app_data).unwrap();
        let db_path = app_data.join("summarizer.db");
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let bytes = fs::read(fixture).expect("fixture should read");
        let request = fixture_request(&bytes);
        let existing_import = root.0.join("accepted-source.pdf");
        fs::write(&existing_import, &bytes).expect("accepted source should write");
        let factory_db_path = db_path.clone();
        let factory_request = request.clone();
        let factory_import = existing_import.clone();
        let runtime_factory: RuntimeFactory = Arc::new(move || {
            let mut conn = db::init_db(&factory_db_path).expect("provider database should open");
            let (document, run) = prepare_pdf_ingestion(
                factory_import
                    .to_str()
                    .expect("accepted path should be UTF-8"),
                Some(&factory_request.inputs[0].display_name),
            )
            .expect("concurrent ingestion should prepare");
            store::accept_job_with_ingestion_guarded(
                &mut conn,
                &factory_request,
                &factory_request.canonical_hash().unwrap(),
                factory_import.to_str().unwrap(),
                "concurrent-provider",
                &document,
                &run,
                SummaryProfile::General,
                Some(&fixture_profile_snapshot()),
                || true,
            )
            .expect("concurrent acceptance should persist")
            .expect("concurrent acceptance should be admitted");
            Err(ModelRuntimeFailure {
                code: "MODEL_NOT_AVAILABLE".to_string(),
                message: "late fixture model failure".to_string(),
                recoverable: true,
                request_attempts: Vec::new(),
            })
        });
        let provider = ConnectProvider::start_at(
            db_path.clone(),
            app_data.clone(),
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            runtime_factory,
        )
        .expect("provider should start");
        let registration: RuntimeRegistration =
            serde_json::from_slice(&fs::read(provider.registration_path()).unwrap()).unwrap();

        let response = client()
            .post(format!("{}v1/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .multipart(form(&request, bytes))
            .send()
            .expect("duplicate submission should return its stored job");
        assert_eq!(response.status(), StatusCode::OK);
        let status = response
            .json::<JobStatus>()
            .expect("job status should decode");
        assert_eq!(status.job_id, request.job_id);
        assert_eq!(status.status, JobState::Accepted);
        assert!(existing_import.exists());
        assert!(fs::read_dir(app_data.join("connect-imports"))
            .unwrap()
            .next()
            .is_none());
        let conn = db::init_db(&db_path).expect("provider database should reopen");
        assert!(store::get_job(&conn, &request.job_id).unwrap().is_some());
    }

    #[test]
    fn provider_auth_handoff_idempotency_persistence_and_removal_work_end_to_end() {
        let root = TestDirectory::new("doc-sum-connect-provider");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        fs::create_dir_all(&runtime_root).unwrap();
        fs::create_dir_all(&app_data).unwrap();
        let db_path = app_data.join("summarizer.db");
        let runtime_factory: RuntimeFactory =
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>));
        let provider = ConnectProvider::start_at(
            db_path.clone(),
            app_data.clone(),
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            runtime_factory,
        )
        .expect("provider should start");
        let registration: RuntimeRegistration = serde_json::from_slice(
            &fs::read(provider.registration_path()).expect("registration should exist"),
        )
        .expect("registration should decode");
        let registration_v2: v2::RuntimeRegistration = serde_json::from_slice(
            &fs::read(provider.registration_path_v2()).expect("v2 registration should exist"),
        )
        .expect("v2 registration should decode");
        assert_eq!(registration.instance_id, provider.instance_id());
        assert_eq!(registration.transport.base_url, provider.base_url());
        assert_eq!(registration_v2.instance_id, provider.instance_id_v2());
        assert_eq!(registration_v2.protocol_version, v2::PROTOCOL_VERSION);
        assert_eq!(registration_v2.transport.kind, v2::TRANSPORT_KIND);
        assert_eq!(registration_v2.transport.base_url, provider.base_url());
        assert_eq!(registration_v2.auth.token, registration.auth.token);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [
                provider.registration_path(),
                provider.registration_path_v2(),
            ] {
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }

        let client = client();
        assert_eq!(
            client
                .get(format!("{}v1/manifest", provider.base_url()))
                .send()
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            client
                .get(format!("{}v1/manifest", provider.base_url()))
                .bearer_auth(&registration.auth.token)
                .header(header::ORIGIN, "http://127.0.0.1:1420")
                .send()
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let manifest = client
            .get(format!("{}v1/manifest", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<AppManifest>()
            .unwrap();
        assert_eq!(manifest.instance_id, provider.instance_id());
        assert_eq!(manifest.capabilities[0].id, CAPABILITY_ID);
        let unauthorized_v2 = client
            .get(format!("{}v2/manifest", provider.base_url()))
            .send()
            .unwrap();
        assert_eq!(unauthorized_v2.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            unauthorized_v2
                .json::<v2::ErrorEnvelope>()
                .unwrap()
                .protocol_version,
            v2::PROTOCOL_VERSION
        );
        let manifest_v2 = client
            .get(format!("{}v2/manifest", provider.base_url()))
            .bearer_auth(&registration_v2.auth.token)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<v2::AppManifest>()
            .unwrap();
        assert_eq!(manifest_v2.instance_id, provider.instance_id_v2());
        assert_eq!(manifest_v2.capabilities[0].id, CAPABILITY_ID);
        assert_eq!(manifest_v2.capabilities[0].action.label, "Summarize");

        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let bytes = fs::read(source).expect("fixture should read");
        let request = fixture_request(&bytes);
        let accepted = client
            .post(format!("{}v1/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .multipart(form(&request, bytes.clone()))
            .send()
            .expect("job submission should succeed");
        assert_eq!(accepted.status(), StatusCode::ACCEPTED);
        let terminal = wait_for_terminal(
            &client,
            provider.base_url(),
            &registration.auth.token,
            &request.job_id,
        );
        assert_eq!(terminal.status, JobState::Completed);
        assert!(terminal.error.is_none());
        assert!(terminal.result.as_ref().unwrap().outputs[0]
            .content
            .text
            .contains("[p. "));

        let duplicate = client
            .post(format!("{}v1/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .multipart(form(&request, bytes.clone()))
            .send()
            .expect("idempotent submission should succeed");
        assert_eq!(duplicate.status(), StatusCode::OK);

        let mut conflicting = request.clone();
        conflicting.inputs[0].artifact_id = Uuid::new_v4().to_string();
        conflicting.inputs[0].sha256 = "b".repeat(64);
        let conflict = client
            .post(format!("{}v1/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .multipart(form(&conflicting, bytes.clone()))
            .send()
            .expect("conflicting submission should return a response");
        assert_eq!(conflict.status(), StatusCode::CONFLICT);
        assert_eq!(
            conflict.json::<ErrorEnvelope>().unwrap().error.code,
            "JOB_ID_CONFLICT"
        );

        assert_eq!(
            client
                .get(format!("{}v2/jobs/{}", provider.base_url(), request.job_id))
                .bearer_auth(&registration_v2.auth.token)
                .send()
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );

        let mut request_v2 = fixture_request_v2(&bytes);
        request_v2.inputs[0].display_name = "quarterly-report".to_string();
        request_v2.parameters.insert(
            "mode".to_string(),
            serde_json::Value::String("contract".to_string()),
        );
        let accepted_v2 = client
            .post(format!("{}v2/jobs", provider.base_url()))
            .bearer_auth(&registration_v2.auth.token)
            .multipart(form_v2(&request_v2, bytes.clone()))
            .send()
            .expect("v2 job submission should succeed");
        assert_eq!(accepted_v2.status(), StatusCode::ACCEPTED);
        let terminal_v2 = wait_for_terminal_v2(
            &client,
            provider.base_url(),
            &registration_v2.auth.token,
            &request_v2.job_id,
        );
        assert_eq!(
            terminal_v2.status,
            JobState::Completed,
            "v2 terminal error: {:?}",
            terminal_v2.error
        );
        assert_eq!(terminal_v2.protocol_version, v2::PROTOCOL_VERSION);
        assert!(terminal_v2.error.is_none());
        let output_v2 = &terminal_v2.result.as_ref().unwrap().outputs[0];
        let output_bytes = BASE64.decode(&output_v2.payload_base64).unwrap();
        assert_eq!(output_bytes.len() as u64, output_v2.byte_size);
        assert_eq!(
            format!("{:x}", Sha256::digest(&output_bytes)),
            output_v2.sha256
        );
        assert_eq!(
            client
                .get(format!(
                    "{}v1/jobs/{}",
                    provider.base_url(),
                    request_v2.job_id
                ))
                .bearer_auth(&registration.auth.token)
                .send()
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        let duplicate_v2 = client
            .post(format!("{}v2/jobs", provider.base_url()))
            .bearer_auth(&registration_v2.auth.token)
            .multipart(form_v2(&request_v2, bytes.clone()))
            .send()
            .expect("v2 idempotent submission should succeed");
        assert_eq!(duplicate_v2.status(), StatusCode::OK);

        let mut conflicting_mode_v2 = request_v2.clone();
        conflicting_mode_v2.parameters.insert(
            "mode".to_string(),
            serde_json::Value::String("general".to_string()),
        );
        let conflicting_mode_response = client
            .post(format!("{}v2/jobs", provider.base_url()))
            .bearer_auth(&registration_v2.auth.token)
            .multipart(form_v2(&conflicting_mode_v2, bytes.clone()))
            .send()
            .expect("v2 conflicting mode submission should return a response");
        assert_eq!(conflicting_mode_response.status(), StatusCode::CONFLICT);
        assert_eq!(
            conflicting_mode_response
                .json::<ErrorEnvelope>()
                .unwrap()
                .error
                .code,
            "JOB_ID_CONFLICT"
        );

        let conn = db::init_db(&db_path).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM connect_jobs", [], |row| {
                row.get::<_, u32>(0)
            })
            .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM documents", [], |row| {
                row.get::<_, u32>(0)
            })
            .unwrap(),
            2
        );
        let stored = store::get_job(&conn, &request.job_id)
            .unwrap()
            .expect("job should persist");
        assert_eq!(
            db::get_run_summary_profile(&conn, &stored.pipeline_run_id).unwrap(),
            Some(SummaryProfile::General)
        );
        let run = db::get_pipeline_run(&conn, &stored.pipeline_run_id)
            .unwrap()
            .expect("pipeline run should persist");
        assert_eq!(
            run.state,
            crate::pipeline::contracts::PipelineState::CompleteWithWarnings
        );
        let document = db::get_document(&conn, &run.document_id)
            .unwrap()
            .expect("document should persist");
        assert_eq!(document.original_filename, "quarterly-report.pdf");
        assert_eq!(fs::read(stored.import_path).unwrap(), bytes);
        let stored_v2 = store::get_job(&conn, &request_v2.job_id)
            .unwrap()
            .expect("v2 job should persist");
        assert_eq!(stored_v2.protocol_version, v2::PROTOCOL_VERSION);
        assert_eq!(
            db::get_run_summary_profile(&conn, &stored_v2.pipeline_run_id).unwrap(),
            Some(SummaryProfile::Contract)
        );
        let run_v2 = db::get_pipeline_run(&conn, &stored_v2.pipeline_run_id)
            .unwrap()
            .expect("v2 pipeline run should persist");
        let document_v2 = db::get_document(&conn, &run_v2.document_id)
            .unwrap()
            .expect("v2 document should persist");
        assert_eq!(document_v2.original_filename, "quarterly-report");
        drop(conn);

        let malformed_bytes = b"%PDF-1.4\nnot a structurally valid PDF".to_vec();
        let malformed_request = fixture_request(&malformed_bytes);
        let malformed_accepted = client
            .post(format!("{}v1/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .multipart(form(&malformed_request, malformed_bytes))
            .send()
            .expect("malformed PDF submission should be accepted for parsing");
        assert_eq!(malformed_accepted.status(), StatusCode::ACCEPTED);
        let malformed_terminal = wait_for_terminal(
            &client,
            provider.base_url(),
            &registration.auth.token,
            &malformed_request.job_id,
        );
        assert_eq!(malformed_terminal.status, JobState::Failed);
        assert!(malformed_terminal.result.is_none());
        assert!(malformed_terminal.error.is_some());

        let registration_path = provider.registration_path().to_path_buf();
        let registration_path_v2 = provider.registration_path_v2().to_path_buf();
        provider.unregister();
        assert!(!registration_path.exists());
        assert!(!registration_path_v2.exists());
        provider.unregister();
        drop(provider);
    }

    #[test]
    fn provider_rejects_stream_identity_mismatch_without_a_job_or_import() {
        let root = TestDirectory::new("doc-sum-connect-rejection");
        let runtime_root = root.0.join("runtime");
        let app_data = root.0.join("app-data");
        fs::create_dir_all(&runtime_root).unwrap();
        fs::create_dir_all(&app_data).unwrap();
        let db_path = app_data.join("summarizer.db");
        let provider = ConnectProvider::start_at(
            db_path.clone(),
            app_data.clone(),
            runtime_root,
            DEFAULT_MAX_INPUT_BYTES,
            Arc::new(|| Ok(Box::new(FixtureRuntime) as Box<dyn ModelRuntime>)),
        )
        .unwrap();
        let registration: RuntimeRegistration = serde_json::from_slice(
            &fs::read(provider.registration_path()).expect("registration should exist"),
        )
        .unwrap();
        let bytes = b"%PDF-1.4\nidentity mismatch".to_vec();
        let mut request = fixture_request(&bytes);
        request.inputs[0].sha256 = "b".repeat(64);
        let response = client()
            .post(format!("{}v1/jobs", provider.base_url()))
            .bearer_auth(&registration.auth.token)
            .multipart(form(&request, bytes))
            .send()
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

        let conn = db::init_db(&db_path).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM connect_jobs", [], |row| {
                row.get::<_, u32>(0)
            })
            .unwrap(),
            0
        );
        let imports = app_data.join("connect-imports");
        assert_eq!(fs::read_dir(imports).unwrap().count(), 0);
    }
}
#[test]
fn stop_authority_linearizes_publication_and_job_commit() {
    let authority = Arc::new(ProviderAdmissionAuthority::default());
    let commit = authority.enter().unwrap();
    let stopping = Arc::clone(&authority);
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    let (stopped_tx, stopped_rx) = std::sync::mpsc::sync_channel(1);
    let stop = thread::spawn(move || {
        started_tx.send(()).unwrap();
        stopping.begin_stop();
        stopped_tx.send(()).unwrap();
    });
    started_rx.recv().unwrap();
    assert!(stopped_rx.try_recv().is_err());
    drop(commit);
    stopped_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    stop.join().unwrap();
    assert!(authority.enter().is_err());
}
