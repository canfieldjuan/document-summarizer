use crate::connect::contracts::{
    job_error, valid_uuid_v4, AppManifest, AuthRegistration, ErrorEnvelope, InputArtifact,
    JobError, JobRequest, JobResult, JobStatus, RuntimeRegistration, TransportRegistration, APP_ID,
    DEFAULT_MAX_INPUT_BYTES, MAX_REQUEST_JSON_BYTES, PROTOCOL_VERSION,
};
use crate::connect::store::{self, ConnectStoreError, StoredConnectJob};
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

#[derive(Clone)]
struct ProviderState {
    db_path: PathBuf,
    imports_dir: PathBuf,
    instance_id: String,
    token: String,
    manifest: AppManifest,
    max_input_bytes: u64,
    runtime_factory: RuntimeFactory,
}

#[derive(Debug, Error)]
pub enum ProviderStartError {
    #[error("Connect requires XDG_RUNTIME_DIR")]
    RuntimeDirectoryUnavailable,
    #[error("Invalid DOC_SUM_CONNECT_MAX_BYTES configuration")]
    InvalidMaxInputBytes,
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
    registration_path: PathBuf,
    base_url: String,
    instance_id: String,
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
        let imports_dir = app_data_dir.join("connect-imports");
        ensure_private_directory(&imports_dir)?;
        let providers_dir = runtime_root.join("local-connect/v1/providers");
        ensure_private_directory(&providers_dir)?;

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
        let instance_id = Uuid::new_v4().to_string();
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let manifest = AppManifest::new(&instance_id, max_input_bytes);
        let state = ProviderState {
            db_path,
            imports_dir,
            instance_id: instance_id.clone(),
            token: token.clone(),
            manifest,
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

        let registration = RuntimeRegistration {
            protocol_version: PROTOCOL_VERSION,
            instance_id: instance_id.clone(),
            app_id: APP_ID.to_string(),
            pid: std::process::id(),
            started_at: Utc::now(),
            transport: TransportRegistration {
                kind: "http-loopback-v1".to_string(),
                base_url: base_url.clone(),
            },
            auth: AuthRegistration {
                scheme: "bearer".to_string(),
                token,
            },
        };
        let registration_path = providers_dir.join(format!("{APP_ID}-{instance_id}.json"));
        if let Err(error) = write_registration(&registration_path, &registration) {
            let _ = shutdown_tx.send(());
            let _ = server_thread.join();
            return Err(error);
        }

        Ok(Self {
            registration_path,
            base_url,
            instance_id,
            shutdown: Some(shutdown_tx),
            server_thread: Some(server_thread),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub fn registration_path(&self) -> &Path {
        &self.registration_path
    }
}

impl Drop for ConnectProvider {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.registration_path) {
            if error.kind() != io::ErrorKind::NotFound {
                eprintln!("Connect registration cleanup failed: {error}");
            }
        }
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.server_thread.take();
    }
}

async fn get_manifest(
    State(state): State<ProviderState>,
    headers: HeaderMap,
) -> Result<Json<AppManifest>, ProviderHttpError> {
    authorize(&state, &headers)?;
    Ok(Json(state.manifest))
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

async fn create_job(
    State(state): State<ProviderState>,
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
    let request: JobRequest = serde_json::from_slice(&request_bytes).map_err(|_| {
        ProviderHttpError::bad_request("REQUEST_INVALID", "The request JSON is invalid.")
    })?;
    request
        .validate(state.max_input_bytes)
        .map_err(ProviderHttpError::from_job_error)?;
    let request_hash = request.canonical_hash().map_err(ProviderHttpError::json)?;

    let mut conn = db::init_db(&state.db_path).map_err(ProviderHttpError::store)?;
    if let Some(existing) =
        store::get_job(&conn, &request.job_id).map_err(ProviderHttpError::store)?
    {
        return idempotent_response(existing, &request_hash);
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
    let accepted = store::accept_job_with_ingestion(
        &mut conn,
        &request,
        &request_hash,
        import_path_text,
        &state.instance_id,
        &document,
        &run,
    );
    let accepted = match accepted {
        Ok((_, accepted)) => accepted,
        Err(error) => {
            if let Some(existing) =
                store::get_job(&conn, &request.job_id).map_err(ProviderHttpError::store)?
            {
                if existing.request_hash == request_hash {
                    return Ok((StatusCode::OK, Json(existing.status())).into_response());
                }
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
    Ok((StatusCode::ACCEPTED, Json(accepted.status())).into_response())
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
        let result = JobResult::from_summary(&job.input, &summary)?;
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
    let staging = state.imports_dir.join(format!(
        ".{job_id}-{}.{}.part",
        input.artifact_id,
        Uuid::new_v4()
    ));
    let final_path = state
        .imports_dir
        .join(format!("{job_id}-{}.pdf", input.artifact_id));
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

    if tokio::fs::try_exists(&final_path)
        .await
        .map_err(ProviderHttpError::io)?
    {
        let (existing_size, existing_hash) = hash_file(&final_path).await?;
        if existing_size == input.byte_size && existing_hash == input.sha256 {
            remove_file_quietly(&staging).await;
            return Ok(final_path);
        }
        remove_file_quietly(&staging).await;
        return Err(ProviderHttpError::new(
            StatusCode::CONFLICT,
            "ARTIFACT_STORAGE_CONFLICT",
            "Provider storage already contains different bytes for this artifact.",
            false,
        ));
    }
    tokio::fs::rename(&staging, &final_path)
        .await
        .map_err(ProviderHttpError::io)?;
    sync_directory(&state.imports_dir)
        .await
        .map_err(ProviderHttpError::io)?;
    Ok(final_path)
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
) -> Result<Response, ProviderHttpError> {
    if existing.request_hash == request_hash {
        Ok((StatusCode::OK, Json(existing.status())).into_response())
    } else {
        Err(ProviderHttpError::new(
            StatusCode::CONFLICT,
            "JOB_ID_CONFLICT",
            "The job identifier was already used for different input.",
            false,
        ))
    }
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
        Self { status, error }
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
                protocol_version: PROTOCOL_VERSION,
                error: self.error,
            }),
        )
            .into_response()
    }
}

fn write_registration(
    registration_path: &Path,
    registration: &RuntimeRegistration,
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
    use reqwest::blocking::{multipart, Client};
    use std::time::{Duration, Instant};

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
            let text = if request.system_prompt.contains("synthesize chunk notes") {
                "Connect returned a grounded summary from the realistic PDF fixture."
            } else {
                "Grounded source-chunk notes for Connect."
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
        assert_eq!(registration.instance_id, provider.instance_id());
        assert_eq!(registration.transport.base_url, provider.base_url());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(provider.registration_path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
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
            .contains("grounded summary"));

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

        let conn = db::init_db(&db_path).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM connect_jobs", [], |row| {
                row.get::<_, u32>(0)
            })
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM documents", [], |row| {
                row.get::<_, u32>(0)
            })
            .unwrap(),
            1
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
        drop(provider);
        assert!(!registration_path.exists());
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
