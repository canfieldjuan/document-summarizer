use crate::connect::contracts::{
    job_error, valid_uuid_v4, AppManifest, AuthRegistration, ErrorEnvelope, InputArtifact,
    JobError, JobRequest, JobResult, JobStatus, RuntimeRegistration, TransportRegistration, APP_ID,
    DEFAULT_MAX_INPUT_BYTES, MAX_REQUEST_JSON_BYTES, PROTOCOL_VERSION,
};
use crate::connect::store::{self, ConnectStoreError, StoredConnectJob};
use crate::connect::v2;
use crate::pipeline::chunk::DeterministicDocumentChunker;
use crate::pipeline::contracts::{ModelRuntime, ModelRuntimeFailure};
use crate::pipeline::db;
use crate::pipeline::ingest::prepare_pdf_ingestion;
use crate::pipeline::model::OllamaRuntime;
use crate::pipeline::normalize::CanonicalNormalizer;
use crate::pipeline::parser::PdfExtractParser;
use crate::pipeline::service::{process_ingested_to_summary, SummaryComponents};
use crate::pipeline::structure::DeterministicStructureInterpreter;
use axum::extract::{DefaultBodyLimit, Multipart, Path as AxumPath, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;
use uuid::Uuid;

type RuntimeFactory =
    Arc<dyn Fn() -> Result<Box<dyn ModelRuntime>, ModelRuntimeFailure> + Send + Sync + 'static>;
const V2_INSTANCE_ID_FILE: &str = "connect-v2-instance-id";

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
}

#[derive(Debug, Error)]
pub enum ProviderStartError {
    #[error("Connect requires XDG_RUNTIME_DIR")]
    RuntimeDirectoryUnavailable,
    #[error("Invalid DOC_SUM_CONNECT_MAX_BYTES configuration")]
    InvalidMaxInputBytes,
    #[error("Connect v2 instance identity is invalid")]
    InvalidInstanceIdentity,
    #[error("Connect provider I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("Connect provider database setup failed: {0}")]
    Store(#[from] ConnectStoreError),
    #[error("Connect registration serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Contract(#[from] crate::connect::contracts::ContractBuildError),
    #[error("Connect provider server failed to initialize: {0}")]
    Server(String),
}

pub struct ConnectProvider {
    registration_path_v1: PathBuf,
    registration_path_v2: PathBuf,
    base_url: String,
    instance_id_v1: String,
    instance_id_v2: String,
    shutdown: Option<oneshot::Sender<()>>,
    server_thread: Option<JoinHandle<()>>,
}

impl ConnectProvider {
    pub fn start(db_path: PathBuf, app_data_dir: PathBuf) -> Result<Self, ProviderStartError> {
        let runtime_root = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .ok_or(ProviderStartError::RuntimeDirectoryUnavailable)?;
        let max_input_bytes = match env::var("DOC_SUM_CONNECT_MAX_BYTES") {
            Ok(value) => value
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0 && *value <= DEFAULT_MAX_INPUT_BYTES)
                .ok_or(ProviderStartError::InvalidMaxInputBytes)?,
            Err(_) => DEFAULT_MAX_INPUT_BYTES,
        };
        let runtime_factory: RuntimeFactory = Arc::new(|| {
            OllamaRuntime::from_environment()
                .map(|runtime| Box::new(runtime) as Box<dyn ModelRuntime>)
        });
        Self::start_at(
            db_path,
            app_data_dir,
            runtime_root,
            max_input_bytes,
            runtime_factory,
        )
    }

    fn start_at(
        db_path: PathBuf,
        app_data_dir: PathBuf,
        runtime_root: PathBuf,
        max_input_bytes: u64,
        runtime_factory: RuntimeFactory,
    ) -> Result<Self, ProviderStartError> {
        ensure_private_directory(&app_data_dir)?;
        let imports_dir = app_data_dir.join("connect-imports");
        ensure_private_directory(&imports_dir)?;
        let instance_id_v2 = load_or_create_v2_instance_id(&app_data_dir)?;
        let providers_dir_v1 = runtime_root.join("local-connect/v1/providers");
        let providers_dir_v2 = runtime_root.join("local-connect/v2/providers");
        ensure_private_directory(&providers_dir_v1)?;
        ensure_private_directory(&providers_dir_v2)?;

        let conn = db::init_db(&db_path).map_err(ConnectStoreError::from)?;
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

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
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
                        return;
                    }
                };
                runtime.block_on(async move {
                    let listener = match tokio::net::TcpListener::from_std(listener) {
                        Ok(listener) => listener,
                        Err(error) => {
                            let _ = ready_tx.send(Err(error.to_string()));
                            return;
                        }
                    };
                    if ready_tx.send(Ok(())).is_err() {
                        return;
                    }
                    let result = axum::serve(listener, app)
                        .with_graceful_shutdown(async {
                            let _ = shutdown_rx.await;
                        })
                        .await;
                    if let Err(error) = result {
                        eprintln!("Connect provider stopped with an error: {error}");
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
                token,
            },
        };
        let registration_path_v1 = providers_dir_v1.join(format!("{APP_ID}-{instance_id_v1}.json"));
        let registration_path_v2 = providers_dir_v2.join(format!("{APP_ID}-{instance_id_v2}.json"));
        if let Err(error) = write_registration(&registration_path_v1, &registration_v1) {
            let _ = shutdown_tx.send(());
            let _ = server_thread.join();
            return Err(error);
        }
        if let Err(error) = write_registration(&registration_path_v2, &registration_v2) {
            let _ = fs::remove_file(&registration_path_v1);
            let _ = shutdown_tx.send(());
            let _ = server_thread.join();
            return Err(error);
        }

        Ok(Self {
            registration_path_v1,
            registration_path_v2,
            base_url,
            instance_id_v1,
            instance_id_v2,
            shutdown: Some(shutdown_tx),
            server_thread: Some(server_thread),
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
}

impl Drop for ConnectProvider {
    fn drop(&mut self) {
        for path in [&self.registration_path_v1, &self.registration_path_v2] {
            if let Err(error) = fs::remove_file(path) {
                if error.kind() != io::ErrorKind::NotFound {
                    eprintln!("Connect registration cleanup failed: {error}");
                }
            }
        }
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.server_thread.take();
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
}

fn parse_job_request(
    bytes: &[u8],
    version: WireVersion,
    max_input_bytes: u64,
) -> Result<(JobRequest, String), ProviderHttpError> {
    match version {
        WireVersion::V1 => {
            let request: JobRequest = serde_json::from_slice(bytes).map_err(|_| {
                ProviderHttpError::bad_request("REQUEST_INVALID", "The request JSON is invalid.")
            })?;
            request
                .validate(max_input_bytes)
                .map_err(ProviderHttpError::from_job_error)?;
            let request_hash = request.canonical_hash().map_err(ProviderHttpError::json)?;
            Ok((request, request_hash))
        }
        WireVersion::V2 => {
            let request: v2::JobRequest = serde_json::from_slice(bytes).map_err(|_| {
                ProviderHttpError::bad_request("REQUEST_INVALID", "The request JSON is invalid.")
            })?;
            request
                .validate(max_input_bytes)
                .map_err(ProviderHttpError::from_job_error)?;
            let request_hash = request.canonical_hash().map_err(ProviderHttpError::json)?;
            let mut internal = request.as_internal();
            internal.protocol_version = v2::PROTOCOL_VERSION;
            Ok((internal, request_hash))
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
    Ok(Json(state.manifest_v1))
}

async fn get_manifest_v2(
    State(state): State<ProviderState>,
    headers: HeaderMap,
) -> Result<Json<v2::AppManifest>, ProviderHttpError> {
    authorize(&state, &headers)
        .map_err(|error| error.with_protocol_version(v2::PROTOCOL_VERSION))?;
    Ok(Json(state.manifest_v2))
}

async fn get_job_status(
    State(state): State<ProviderState>,
    headers: HeaderMap,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<JobStatus>, ProviderHttpError> {
    authorize(&state, &headers)?;
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
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, ProviderHttpError> {
    create_job_for(WireVersion::V1, state, headers, multipart).await
}

async fn create_job_v2(
    State(state): State<ProviderState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, ProviderHttpError> {
    create_job_for(WireVersion::V2, state, headers, multipart)
        .await
        .map_err(|error| error.with_protocol_version(v2::PROTOCOL_VERSION))
}

async fn create_job_for(
    version: WireVersion,
    state: ProviderState,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Response, ProviderHttpError> {
    authorize(&state, &headers)?;
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
    let (request, request_hash) =
        parse_job_request(&request_bytes, version, state.max_input_bytes)?;

    let mut conn = db::init_db(&state.db_path).map_err(ProviderHttpError::store)?;
    if let Some(existing) =
        store::get_job(&conn, &request.job_id).map_err(ProviderHttpError::store)?
    {
        return idempotent_response(existing, &request_hash, version);
    }
    if store::has_active_job(&conn).map_err(ProviderHttpError::store)? {
        return Err(ProviderHttpError::new(
            StatusCode::CONFLICT,
            "PROVIDER_BUSY",
            "The provider is processing another job.",
            true,
        ));
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
    let import_path = receive_artifact(&state, &request.job_id, &input, artifact_field).await?;
    if multipart
        .next_field()
        .await
        .map_err(ProviderHttpError::multipart)?
        .is_some()
    {
        remove_file_quietly(&import_path).await;
        return Err(ProviderHttpError::bad_request(
            "MULTIPART_FIELDS_INVALID",
            "Unexpected multipart fields were provided.",
        ));
    }

    let import_path_text = match import_path.to_str() {
        Some(path) => path,
        None => {
            remove_file_quietly(&import_path).await;
            return Err(ProviderHttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "PROVIDER_STORAGE_INVALID",
                "Provider storage path is unavailable.",
                true,
            ));
        }
    };
    let (document, run) = match prepare_pdf_ingestion(import_path_text, Some(&input.display_name)) {
        Ok(prepared) => prepared,
        Err(error) => {
            remove_file_quietly(&import_path).await;
            return Err(ProviderHttpError::domain(error.code()));
        }
    };
    if document.byte_size != input.byte_size || document.content_hash != input.sha256 {
        remove_file_quietly(&import_path).await;
        return Err(ProviderHttpError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "ARTIFACT_IDENTITY_MISMATCH",
            "The promoted artifact no longer matches its declared identity.",
            false,
        ));
    }
    let provider_instance_id = match version {
        WireVersion::V1 => &state.instance_id_v1,
        WireVersion::V2 => &state.instance_id_v2,
    };
    let accepted = store::accept_job_with_ingestion(
        &mut conn,
        &request,
        &request_hash,
        import_path_text,
        provider_instance_id,
        &document,
        &run,
    );
    let accepted = match accepted {
        Ok((_, accepted)) => accepted,
        Err(error) => {
            if let Some(existing) =
                store::get_job(&conn, &request.job_id).map_err(ProviderHttpError::store)?
            {
                if !existing_job_owns_import_path(&existing.import_path, &import_path) {
                    remove_file_quietly(&import_path).await;
                }
                return idempotent_response(existing, &request_hash, version);
            }
            remove_file_quietly(&import_path).await;
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

    let worker_state = state.clone();
    let worker_job_id = request.job_id.clone();
    if let Err(error) = thread::Builder::new()
        .name(format!("connect-job-{}", &worker_job_id[..8]))
        .spawn(move || process_job(worker_state, worker_job_id))
    {
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

fn process_job(state: ProviderState, job_id: String) {
    let result = (|| -> Result<(), ProcessJobError> {
        let conn = db::init_db(&state.db_path)?;
        let job = store::mark_processing(&conn, &job_id)?;
        let runtime = (state.runtime_factory)()?;
        let mut pipeline_conn = db::init_db(&state.db_path)?;
        let parser = PdfExtractParser::new();
        let normalizer = CanonicalNormalizer::new();
        let interpreter = DeterministicStructureInterpreter::new();
        let chunker = DeterministicDocumentChunker::new();
        let summary = process_ingested_to_summary(
            &mut pipeline_conn,
            &job.pipeline_run_id,
            SummaryComponents {
                parser: &parser,
                normalizer: &normalizer,
                interpreter: &interpreter,
                chunker: &chunker,
                runtime: runtime.as_ref(),
            },
        )?;
        let result = JobResult::from_summary(&job.input, &summary.summary)?;
        store::mark_completed(&conn, &job_id, &result)?;
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

#[derive(Debug, Error)]
enum ProcessJobError {
    #[error(transparent)]
    PipelineStore(#[from] crate::pipeline::db::StoreError),
    #[error(transparent)]
    ConnectStore(#[from] ConnectStoreError),
    #[error("Model runtime failure: {0:?}")]
    Runtime(#[from] ModelRuntimeFailure),
    #[error(transparent)]
    Service(#[from] crate::pipeline::service::DocumentServiceError),
    #[error(transparent)]
    Contract(#[from] crate::connect::contracts::ContractBuildError),
}

impl ProcessJobError {
    fn public_error(&self) -> JobError {
        match self {
            Self::Runtime(failure) => job_error(
                &failure.code,
                "The configured local model runtime is unavailable.",
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
    )
}

async fn receive_artifact(
    state: &ProviderState,
    job_id: &str,
    input: &InputArtifact,
    mut field: axum::extract::multipart::Field<'_>,
) -> Result<PathBuf, ProviderHttpError> {
    let (staging, final_path) = allocate_import_paths(
        &state.imports_dir,
        job_id,
        &input.artifact_id,
        Uuid::new_v4(),
    );
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&staging)
        .await
        .map_err(ProviderHttpError::io)?;
    set_private_file_permissions(&staging)
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
    if let Err(error) = receive_result {
        remove_file_quietly(&staging).await;
        return Err(error);
    }

    promote_staged_artifact(&staging, &final_path, input, &state.imports_dir).await
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
        imports_dir.join(format!(".{stem}.part")),
        imports_dir.join(format!("{stem}.pdf")),
    )
}

async fn promote_staged_artifact(
    staging: &Path,
    final_path: &Path,
    input: &InputArtifact,
    imports_dir: &Path,
) -> Result<PathBuf, ProviderHttpError> {
    match tokio::fs::hard_link(staging, final_path).await {
        Ok(()) => {
            remove_file_quietly(staging).await;
            sync_directory(imports_dir)
                .await
                .map_err(ProviderHttpError::io)?;
            Ok(final_path.to_path_buf())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let existing = hash_file(final_path).await;
            remove_file_quietly(staging).await;
            let (existing_size, existing_hash) = existing?;
            if existing_size != input.byte_size || existing_hash != input.sha256 {
                return Err(ProviderHttpError::new(
                    StatusCode::CONFLICT,
                    "ARTIFACT_STORAGE_CONFLICT",
                    "Provider storage already contains different bytes for this artifact.",
                    false,
                ));
            }
            sync_directory(imports_dir)
                .await
                .map_err(ProviderHttpError::io)?;
            Ok(final_path.to_path_buf())
        }
        Err(error) => {
            remove_file_quietly(staging).await;
            Err(ProviderHttpError::io(error))
        }
    }
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

    fn with_protocol_version(mut self, protocol_version: u32) -> Self {
        self.protocol_version = protocol_version;
        self
    }

    fn multipart(error: axum::extract::multipart::MultipartError) -> Self {
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

fn write_registration<T: Serialize>(
    registration_path: &Path,
    registration: &T,
) -> Result<(), ProviderStartError> {
    let parent = registration_path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "registration has no parent"))?;
    let temporary = parent.join(format!(".{}.tmp", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(registration)?;
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

fn load_or_create_v2_instance_id(app_data_dir: &Path) -> Result<String, ProviderStartError> {
    let path = app_data_dir.join(V2_INSTANCE_ID_FILE);
    match fs::read_to_string(&path) {
        Ok(value) => return parse_v2_instance_id(&value),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let instance_id = Uuid::new_v4().to_string();
    let temporary = app_data_dir.join(format!(".{V2_INSTANCE_ID_FILE}.{}.tmp", Uuid::new_v4()));
    let write_result = (|| -> Result<(), io::Error> {
        let mut file = private_create_new(&temporary)?;
        file.write_all(instance_id.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
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

async fn set_private_file_permissions(path: &Path) -> Result<(), io::Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(())
}

async fn sync_directory(path: &Path) -> Result<(), io::Error> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || File::open(path)?.sync_all())
        .await
        .map_err(|error| io::Error::other(error.to_string()))?
}

async fn hash_file(path: &Path) -> Result<(u64, String), ProviderHttpError> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(ProviderHttpError::io)?;
    let mut buffer = [0u8; 8192];
    let mut size = 0u64;
    let mut hasher = Sha256::new();
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(ProviderHttpError::io)?;
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
    use crate::pipeline::contracts::{ModelRequest, ModelResponse};
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use reqwest::blocking::{multipart, Client};
    use std::collections::BTreeMap;
    use std::time::{Duration, Instant};

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
            Some("part")
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
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let error = runtime
            .block_on(promote_staged_artifact(
                &staging,
                &final_path,
                &input,
                &imports,
            ))
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
        let promoted = match runtime.block_on(promote_staged_artifact(
            &matching_staging,
            &final_path,
            &matching_input,
            &imports,
        )) {
            Ok(path) => path,
            Err(_) => panic!("matching promotion should reuse the existing import"),
        };

        assert_eq!(promoted, final_path);
        assert_eq!(fs::read(&promoted).unwrap(), winner);
        assert!(!matching_staging.exists());
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!("{label}-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("test directory should be created");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
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
            "connect-fixture-runtime"
        }

        fn model_id(&self) -> &str {
            "connect-fixture-model"
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

    #[test]
    fn restarted_v2_provider_reuses_identity_and_exposes_interrupted_failure() {
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
        drop(conn);
        drop(first);

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
        assert_eq!(terminal_v2.status, JobState::Completed);
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
        drop(provider);
        assert!(!registration_path.exists());
        assert!(!registration_path_v2.exists());
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
