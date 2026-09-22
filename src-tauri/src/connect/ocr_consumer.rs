use crate::connect::contracts::{CapabilityRef, InputArtifact, JobState, APP_ID};
#[cfg(any(unix, test))]
use crate::connect::v2::{AppManifest, RuntimeRegistration, TRANSPORT_KIND};
use crate::connect::v2::{
    ErrorEnvelope, JobRequest, JobStatus, OutputArtifact, OCR_INPUT_MEDIA_TYPE, PROTOCOL_VERSION,
};
use crate::pipeline::contracts::{IngestedDocument, ParsedDocument, PipelineRun, SourceType};
use crate::pipeline::db::{self, NewOcrHandoff, OcrHandoff, OcrOutput, StoreError};
use crate::pipeline::ingest::prepare_received_run;
use crate::pipeline::parser::canonical_tagged_ocr_text;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::Utc;
use reqwest::blocking::{multipart, Client, Response};
use reqwest::{StatusCode, Url};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::io::Write;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use uuid::Uuid;
#[cfg(unix)]
use uuid::{Variant, Version};

#[cfg(any(unix, test))]
const OCR_APP_ID: &str = "document-ocr";
const OCR_CAPABILITY_ID: &str = "document.ocr";
const OCR_CAPABILITY_VERSION: &str = "1.0";
const PDF_MEDIA_TYPE: &str = "application/pdf";
const TEXT_MEDIA_TYPE: &str = "text/plain";
const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
const MAX_PDF_BYTES: usize = 2 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 256 * 1024;
#[cfg(unix)]
const MAX_REGISTRATION_BYTES: u64 = 16 * 1024;
#[cfg(unix)]
const MAX_MANIFEST_BYTES: u64 = 128 * 1024;
const MAX_STATUS_BYTES: u64 = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveOcrProvider {
    pub app_id: String,
    pub instance_id: String,
    pub base_url: String,
    token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OcrRecoveryReport {
    pub child_run_ids: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub(crate) enum OcrConsumerError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("OCR provider discovery failed: {0}")]
    Discovery(String),
    #[error("OCR provider request failed: {0}")]
    Transport(String),
    #[error("OCR provider returned invalid data: {0}")]
    InvalidOutput(String),
    #[error("OCR provider failed: {0}")]
    ProviderFailed(String),
    #[error("OCR recovery is pinned to unavailable provider instance {0}")]
    ProviderUnavailable(String),
    #[error("OCR processing exceeded its local deadline")]
    Deadline,
    #[error("OCR snapshot I/O failed: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
enum TransportError {
    #[error("job not found")]
    NotFound,
    #[error("provider refused the request: {message}")]
    Refused { message: String, retryable: bool },
    #[error("provider response is unavailable: {0}")]
    Uncertain(String),
    #[error("provider response is invalid: {0}")]
    Invalid(String),
}

trait OcrTransport {
    fn submit(
        &self,
        provider: &LiveOcrProvider,
        request: &JobRequest,
        source: &[u8],
        deadline: Instant,
    ) -> Result<JobStatus, TransportError>;

    fn status(
        &self,
        provider: &LiveOcrProvider,
        job_id: &str,
        deadline: Instant,
    ) -> Result<JobStatus, TransportError>;
}

struct HttpOcrTransport;

struct ValidatedOutputs {
    pdf_artifact_id: String,
    pdf_sha256: String,
    pdf_bytes: Vec<u8>,
    text_artifact_id: String,
    text_sha256: String,
    text_bytes: Vec<u8>,
}

impl HttpOcrTransport {
    fn client(deadline: Instant) -> Result<Client, TransportError> {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| TransportError::Uncertain("the phase deadline expired".to_string()))?;
        Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(DISCOVERY_TIMEOUT.min(remaining))
            .timeout(remaining)
            .build()
            .map_err(|error| TransportError::Uncertain(error.to_string()))
    }

    fn answer(response: Response, limit: u64) -> Result<JobStatus, TransportError> {
        let status = response.status();
        let bytes = read_bounded_response(response, limit)
            .map_err(|error| TransportError::Invalid(error.to_string()))?;
        if status.is_success() {
            return serde_json::from_slice(&bytes)
                .map_err(|error| TransportError::Invalid(error.to_string()));
        }
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes)
            .map_err(|error| TransportError::Invalid(error.to_string()))?;
        if status == StatusCode::NOT_FOUND && envelope.error.code == "JOB_NOT_FOUND" {
            return Err(TransportError::NotFound);
        }
        Err(TransportError::Refused {
            message: envelope.error.message,
            retryable: envelope.error.retryable,
        })
    }
}

impl OcrTransport for HttpOcrTransport {
    fn submit(
        &self,
        provider: &LiveOcrProvider,
        request: &JobRequest,
        source: &[u8],
        deadline: Instant,
    ) -> Result<JobStatus, TransportError> {
        let client = Self::client(deadline)?;
        let form = multipart::Form::new()
            .part(
                "request",
                multipart::Part::text(
                    serde_json::to_string(request)
                        .map_err(|error| TransportError::Invalid(error.to_string()))?,
                )
                .mime_str("application/json")
                .map_err(|error| TransportError::Invalid(error.to_string()))?,
            )
            .part(
                "artifact",
                multipart::Part::bytes(source.to_vec())
                    .file_name(request.inputs[0].display_name.clone())
                    .mime_str(PDF_MEDIA_TYPE)
                    .map_err(|error| TransportError::Invalid(error.to_string()))?,
            );
        let response = client
            .post(endpoint(&provider.base_url, "v2/jobs")?)
            .bearer_auth(&provider.token)
            .multipart(form)
            .send()
            .map_err(|error| TransportError::Uncertain(error.to_string()))?;
        Self::answer(response, MAX_STATUS_BYTES)
    }

    fn status(
        &self,
        provider: &LiveOcrProvider,
        job_id: &str,
        deadline: Instant,
    ) -> Result<JobStatus, TransportError> {
        let client = Self::client(deadline)?;
        let response = client
            .get(endpoint(&provider.base_url, &format!("v2/jobs/{job_id}"))?)
            .bearer_auth(&provider.token)
            .send()
            .map_err(|error| TransportError::Uncertain(error.to_string()))?;
        Self::answer(response, MAX_STATUS_BYTES)
    }
}

fn endpoint(base_url: &str, path: &str) -> Result<Url, TransportError> {
    let mut url =
        Url::parse(base_url).map_err(|error| TransportError::Invalid(error.to_string()))?;
    url.set_path(path);
    Ok(url)
}

fn read_bounded_response(response: Response, limit: u64) -> io::Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "provider response exceeded its byte limit",
        ));
    }
    let mut bytes = Vec::new();
    response.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "provider response exceeded its byte limit",
        ));
    }
    Ok(bytes)
}

pub(crate) fn parsed_document_requires_ocr(parsed: &ParsedDocument) -> bool {
    parsed.source_type == SourceType::NativeText
        && parsed
            .warnings
            .iter()
            .any(|warning| warning.code == "NO_NATIVE_TEXT_IN_DOCUMENT")
}

pub(crate) fn process_scanned_document(
    conn: &mut Connection,
    root_run_id: &str,
    app_data_dir: &Path,
) -> Result<String, OcrConsumerError> {
    let provider = match db::get_ocr_handoff_for_root(conn, root_run_id)? {
        Some(handoff) => discover_selected_provider(&handoff.provider_instance_id)?,
        None => discover_one_provider()?,
    };
    process_scanned_document_with(
        conn,
        root_run_id,
        app_data_dir,
        &provider,
        &HttpOcrTransport,
    )
}

fn process_scanned_document_with(
    conn: &mut Connection,
    root_run_id: &str,
    app_data_dir: &Path,
    provider: &LiveOcrProvider,
    transport: &dyn OcrTransport,
) -> Result<String, OcrConsumerError> {
    let handoff = match db::get_ocr_handoff_for_root(conn, root_run_id)? {
        Some(existing) => existing,
        None => prepare_handoff(conn, root_run_id, app_data_dir, provider)?,
    };
    if handoff.provider_app_id != provider.app_id
        || handoff.provider_instance_id != provider.instance_id
    {
        return Err(OcrConsumerError::ProviderUnavailable(
            handoff.provider_instance_id,
        ));
    }
    run_handoff(conn, handoff, app_data_dir, provider, transport)
}

fn prepare_handoff(
    conn: &mut Connection,
    root_run_id: &str,
    app_data_dir: &Path,
    provider: &LiveOcrProvider,
) -> Result<OcrHandoff, OcrConsumerError> {
    let root = db::get_pipeline_run(conn, root_run_id)?
        .ok_or_else(|| StoreError::RunNotFound(root_run_id.to_string()))?;
    let document = db::get_document(conn, &root.document_id)?
        .ok_or_else(|| StoreError::DocumentNotFound(root.document_id.clone()))?;
    let source = fs::read(&document.local_source_path)?;
    if source.is_empty()
        || source.len() > MAX_INPUT_BYTES
        || source.len() as u64 != document.byte_size
        || sha256_hex(&source) != document.content_hash
    {
        return Err(OcrConsumerError::InvalidOutput(
            "the admitted source no longer matches its durable identity".to_string(),
        ));
    }
    let page_count = pdf_page_count(&source)?;
    if !(1..=100).contains(&page_count) {
        return Err(OcrConsumerError::InvalidOutput(
            "the scanned PDF is outside the OCR page boundary".to_string(),
        ));
    }
    let handoff_id = Uuid::new_v4().to_string();
    let source_artifact_id = Uuid::new_v4().to_string();
    let provider_job_id = Uuid::new_v4().to_string();
    let child_document_id = Uuid::new_v4().to_string();
    let child_run_id = Uuid::new_v4().to_string();
    let request = JobRequest {
        protocol_version: PROTOCOL_VERSION,
        job_id: provider_job_id.clone(),
        capability: CapabilityRef {
            id: OCR_CAPABILITY_ID.to_string(),
            version: OCR_CAPABILITY_VERSION.to_string(),
        },
        inputs: vec![InputArtifact {
            artifact_id: source_artifact_id.clone(),
            media_type: PDF_MEDIA_TYPE.to_string(),
            byte_size: source.len() as u64,
            sha256: document.content_hash.clone(),
            display_name: document.original_filename.clone(),
            source_app_id: APP_ID.to_string(),
        }],
        parameters: BTreeMap::new(),
    };
    let request_json = serde_json::to_string(&request)
        .map_err(|error| OcrConsumerError::InvalidOutput(error.to_string()))?;
    let derived_path = expected_derived_path(app_data_dir, &handoff_id);
    let derived_path = derived_path.to_str().ok_or_else(|| {
        OcrConsumerError::InvalidOutput("the OCR snapshot path is unavailable".to_string())
    })?;
    db::prepare_ocr_handoff(
        conn,
        &NewOcrHandoff {
            handoff_id: &handoff_id,
            root_run_id,
            root_document_id: &document.document_id,
            source_artifact_id: &source_artifact_id,
            source_bytes: &source,
            source_sha256: &document.content_hash,
            source_display_name: &document.original_filename,
            provider_app_id: &provider.app_id,
            provider_instance_id: &provider.instance_id,
            provider_job_id: &provider_job_id,
            provider_request_json: &request_json,
            child_document_id: &child_document_id,
            child_run_id: &child_run_id,
            derived_path,
        },
    )
    .map_err(OcrConsumerError::from)
}

fn run_handoff(
    conn: &mut Connection,
    mut handoff: OcrHandoff,
    app_data_dir: &Path,
    provider: &LiveOcrProvider,
    transport: &dyn OcrTransport,
) -> Result<String, OcrConsumerError> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        if Instant::now() > deadline {
            return Err(OcrConsumerError::Deadline);
        }
        match handoff.phase.as_str() {
            "prepared" => {
                let request = match validate_saved_request(&handoff) {
                    Ok(request) => request,
                    Err(error) => {
                        db::fail_ocr_handoff(
                            conn,
                            &handoff.handoff_id,
                            "prepared",
                            "OCR_REQUEST_INVALID",
                            &error.to_string(),
                            false,
                        )?;
                        return Err(error);
                    }
                };
                if !db::transition_ocr_handoff(
                    conn,
                    &handoff.handoff_id,
                    "prepared",
                    "submission_uncertain",
                    None,
                )? {
                    handoff = reload(conn, &handoff.handoff_id)?;
                    continue;
                }
                handoff = reload(conn, &handoff.handoff_id)?;
                match transport.submit(provider, &request, &handoff.source_bytes, deadline) {
                    Ok(status) => apply_status(conn, &handoff, status)?,
                    Err(TransportError::Refused { message, retryable }) => {
                        db::fail_ocr_handoff(
                            conn,
                            &handoff.handoff_id,
                            "submission_uncertain",
                            "OCR_PROVIDER_FAILED",
                            &message,
                            retryable,
                        )?;
                        return Err(OcrConsumerError::ProviderFailed(message));
                    }
                    Err(error) => return Err(map_transport(error)),
                }
            }
            "submission_uncertain" => {
                match transport.status(provider, &handoff.provider_job_id, deadline) {
                    Ok(status) => apply_status(conn, &handoff, status)?,
                    Err(TransportError::NotFound) => {
                        db::transition_ocr_handoff(
                            conn,
                            &handoff.handoff_id,
                            "submission_uncertain",
                            "prepared",
                            None,
                        )?;
                    }
                    Err(error) => return Err(map_transport(error)),
                }
            }
            "running" => {
                let status = transport
                    .status(provider, &handoff.provider_job_id, deadline)
                    .map_err(map_transport)?;
                apply_status(conn, &handoff, status)?;
                thread::sleep(Duration::from_millis(100));
            }
            "output_ready" => {
                let child = admit_child(conn, &handoff)?;
                materialize_derived(app_data_dir, &reload(conn, &handoff.handoff_id)?)?;
                return Ok(child.run_id);
            }
            "child_admitted" | "completed" => {
                materialize_derived(app_data_dir, &handoff)?;
                return Ok(handoff.child_run_id);
            }
            "failed" => {
                return Err(OcrConsumerError::ProviderFailed(
                    handoff
                        .error_message
                        .clone()
                        .unwrap_or_else(|| "OCR work failed".to_string()),
                ));
            }
            phase => {
                return Err(OcrConsumerError::InvalidOutput(format!(
                    "unsupported persisted OCR phase {phase}"
                )));
            }
        }
        handoff = reload(conn, &handoff.handoff_id)?;
    }
}

fn validate_saved_request(handoff: &OcrHandoff) -> Result<JobRequest, OcrConsumerError> {
    if handoff.source_bytes.is_empty()
        || handoff.source_bytes.len() > MAX_INPUT_BYTES
        || handoff.source_bytes.len() as u64 != handoff.source_byte_size
        || sha256_hex(&handoff.source_bytes) != handoff.source_sha256
        || sha256_hex(handoff.provider_request_json.as_bytes()) != handoff.provider_request_sha256
    {
        return Err(OcrConsumerError::InvalidOutput(
            "the persisted OCR recovery request failed integrity validation".to_string(),
        ));
    }
    let request: JobRequest = serde_json::from_str(&handoff.provider_request_json)
        .map_err(|error| OcrConsumerError::InvalidOutput(error.to_string()))?;
    if request.protocol_version != PROTOCOL_VERSION
        || request.job_id != handoff.provider_job_id
        || request.capability.id != OCR_CAPABILITY_ID
        || request.capability.version != OCR_CAPABILITY_VERSION
        || !request.parameters.is_empty()
        || request.inputs.len() != 1
        || request.inputs[0].artifact_id != handoff.source_artifact_id
        || request.inputs[0].media_type != PDF_MEDIA_TYPE
        || request.inputs[0].byte_size != handoff.source_byte_size
        || request.inputs[0].sha256 != handoff.source_sha256
        || request.inputs[0].display_name != handoff.source_display_name
        || request.inputs[0].source_app_id != APP_ID
    {
        return Err(OcrConsumerError::InvalidOutput(
            "the persisted OCR request does not match its durable owner".to_string(),
        ));
    }
    Ok(request)
}

fn apply_status(
    conn: &Connection,
    handoff: &OcrHandoff,
    status: JobStatus,
) -> Result<(), OcrConsumerError> {
    if let Err(error) = validate_status_identity(handoff, &status) {
        db::fail_ocr_handoff(
            conn,
            &handoff.handoff_id,
            &handoff.phase,
            "OCR_OUTPUT_INVALID",
            &error.to_string(),
            false,
        )?;
        return Err(error);
    }
    let encoded = serde_json::to_string(&status)
        .map_err(|error| OcrConsumerError::InvalidOutput(error.to_string()))?;
    match status.status {
        JobState::Accepted | JobState::Processing => {
            db::transition_ocr_handoff(
                conn,
                &handoff.handoff_id,
                &handoff.phase,
                "running",
                Some(&encoded),
            )?;
        }
        JobState::Failed => {
            let error = status.error.ok_or_else(|| {
                OcrConsumerError::InvalidOutput("failed OCR status omitted its error".to_string())
            })?;
            db::fail_ocr_handoff(
                conn,
                &handoff.handoff_id,
                &handoff.phase,
                &error.code,
                &error.message,
                error.retryable,
            )?;
            return Err(OcrConsumerError::ProviderFailed(error.message));
        }
        JobState::Completed => {
            let output = match validate_completed_outputs(handoff, &status) {
                Ok(output) => output,
                Err(error) => {
                    db::fail_ocr_handoff(
                        conn,
                        &handoff.handoff_id,
                        &handoff.phase,
                        "OCR_OUTPUT_INVALID",
                        &error.to_string(),
                        false,
                    )?;
                    return Err(error);
                }
            };
            db::store_ocr_outputs(
                conn,
                &handoff.handoff_id,
                &handoff.phase,
                &OcrOutput {
                    provider_status_json: &encoded,
                    pdf_artifact_id: &output.pdf_artifact_id,
                    pdf_sha256: &output.pdf_sha256,
                    pdf_bytes: &output.pdf_bytes,
                    text_artifact_id: &output.text_artifact_id,
                    text_sha256: &output.text_sha256,
                    text_bytes: &output.text_bytes,
                },
            )?;
        }
    }
    Ok(())
}

fn validate_status_identity(
    handoff: &OcrHandoff,
    status: &JobStatus,
) -> Result<(), OcrConsumerError> {
    if status.protocol_version != PROTOCOL_VERSION
        || status.job_id != handoff.provider_job_id
        || status.capability.id != OCR_CAPABILITY_ID
        || status.capability.version != OCR_CAPABILITY_VERSION
        || status.provider.app_id != handoff.provider_app_id
        || status.provider.instance_id != handoff.provider_instance_id
        || status.input_artifacts.len() != 1
        || status.input_artifacts[0].artifact_id != handoff.source_artifact_id
        || status.input_artifacts[0].media_type != PDF_MEDIA_TYPE
        || status.input_artifacts[0].byte_size != handoff.source_byte_size
        || status.input_artifacts[0].sha256 != handoff.source_sha256
    {
        return Err(OcrConsumerError::InvalidOutput(
            "provider status does not match the admitted OCR request".to_string(),
        ));
    }
    let terminal_shape = match status.status {
        JobState::Accepted | JobState::Processing => {
            status.result.is_none() && status.error.is_none()
        }
        JobState::Completed => status.result.is_some() && status.error.is_none(),
        JobState::Failed => status.result.is_none() && status.error.is_some(),
    };
    if !terminal_shape {
        return Err(OcrConsumerError::InvalidOutput(
            "provider status has an invalid terminal shape".to_string(),
        ));
    }
    Ok(())
}

fn validate_completed_outputs(
    handoff: &OcrHandoff,
    status: &JobStatus,
) -> Result<ValidatedOutputs, OcrConsumerError> {
    let outputs = &status
        .result
        .as_ref()
        .expect("validated completed shape")
        .outputs;
    if outputs.len() != 2 {
        return Err(OcrConsumerError::InvalidOutput(
            "completed OCR status must contain exactly two outputs".to_string(),
        ));
    }
    let pdf = one_output(outputs, OCR_INPUT_MEDIA_TYPE)?;
    let text = one_output(outputs, TEXT_MEDIA_TYPE)?;
    if pdf.artifact_id == text.artifact_id
        || pdf.artifact_id == handoff.source_artifact_id
        || text.artifact_id == handoff.source_artifact_id
    {
        return Err(OcrConsumerError::InvalidOutput(
            "OCR output artifact identities are not distinct".to_string(),
        ));
    }
    let pdf_bytes = decode_output(pdf, MAX_PDF_BYTES)?;
    let text_bytes = decode_output(text, MAX_TEXT_BYTES)?;
    if text_bytes.starts_with(&[0xef, 0xbb, 0xbf])
        || std::str::from_utf8(&text_bytes)
            .map_err(|_| OcrConsumerError::InvalidOutput("OCR text is not UTF-8".to_string()))?
            .trim()
            .is_empty()
    {
        return Err(OcrConsumerError::InvalidOutput(
            "OCR text is empty or begins with a byte-order mark".to_string(),
        ));
    }
    let page_count = pdf_page_count(&handoff.source_bytes)?;
    if text_bytes.split(|byte| *byte == 0x0c).count() != page_count {
        return Err(OcrConsumerError::InvalidOutput(
            "OCR text page segmentation does not match the source".to_string(),
        ));
    }
    let canonical = canonical_tagged_ocr_text(&pdf_bytes).map_err(|error| {
        OcrConsumerError::InvalidOutput(format!("tagged OCR PDF is invalid: {}", error.message))
    })?;
    if canonical != text_bytes {
        return Err(OcrConsumerError::InvalidOutput(
            "OCR PDF canonical text does not match the paired text output".to_string(),
        ));
    }
    Ok(ValidatedOutputs {
        pdf_artifact_id: pdf.artifact_id.clone(),
        pdf_sha256: pdf.sha256.clone(),
        pdf_bytes,
        text_artifact_id: text.artifact_id.clone(),
        text_sha256: text.sha256.clone(),
        text_bytes,
    })
}

fn one_output<'a>(
    outputs: &'a [OutputArtifact],
    media_type: &str,
) -> Result<&'a OutputArtifact, OcrConsumerError> {
    let mut matches = outputs
        .iter()
        .filter(|output| output.media_type == media_type);
    let output = matches.next().ok_or_else(|| {
        OcrConsumerError::InvalidOutput(format!("OCR output {media_type} is missing"))
    })?;
    if matches.next().is_some() {
        return Err(OcrConsumerError::InvalidOutput(format!(
            "OCR output {media_type} is duplicated"
        )));
    }
    Ok(output)
}

fn decode_output(output: &OutputArtifact, limit: usize) -> Result<Vec<u8>, OcrConsumerError> {
    let bytes = BASE64.decode(&output.payload_base64).map_err(|_| {
        OcrConsumerError::InvalidOutput("OCR output is not canonical base64".to_string())
    })?;
    if BASE64.encode(&bytes) != output.payload_base64
        || bytes.is_empty()
        || bytes.len() > limit
        || bytes.len() as u64 != output.byte_size
        || sha256_hex(&bytes) != output.sha256
    {
        return Err(OcrConsumerError::InvalidOutput(
            "OCR output integrity does not match its descriptor".to_string(),
        ));
    }
    Ok(bytes)
}

fn admit_child(
    conn: &mut Connection,
    handoff: &OcrHandoff,
) -> Result<PipelineRun, OcrConsumerError> {
    let pdf = handoff.ocr_pdf_bytes.as_ref().ok_or_else(|| {
        OcrConsumerError::InvalidOutput("retained OCR PDF is missing".to_string())
    })?;
    let now = Utc::now();
    let document = IngestedDocument {
        document_id: handoff.child_document_id.clone(),
        original_filename: handoff.source_display_name.clone(),
        file_type: "pdf".to_string(),
        source_type: SourceType::OcrText,
        byte_size: pdf.len() as u64,
        content_hash: sha256_hex(pdf),
        local_source_path: handoff.derived_path.clone(),
        created_at: now,
    };
    let mut run = prepare_received_run(document.document_id.clone(), now);
    run.run_id = handoff.child_run_id.clone();
    db::admit_ocr_child(conn, handoff, &document, &run).map_err(OcrConsumerError::from)
}

pub(crate) fn recover_ocr_handoffs(
    conn: &mut Connection,
    app_data_dir: &Path,
) -> Result<OcrRecoveryReport, StoreError> {
    let providers = discover_providers().unwrap_or_default();
    recover_ocr_handoffs_with(conn, app_data_dir, &providers, &HttpOcrTransport)
}

fn recover_ocr_handoffs_with(
    conn: &mut Connection,
    app_data_dir: &Path,
    providers: &[LiveOcrProvider],
    transport: &dyn OcrTransport,
) -> Result<OcrRecoveryReport, StoreError> {
    let handoffs = db::list_recoverable_ocr_handoffs(conn)?;
    let mut child_run_ids = Vec::new();
    let mut warnings = Vec::new();
    for handoff in handoffs {
        let result = if matches!(handoff.phase.as_str(), "child_admitted" | "completed") {
            materialize_derived(app_data_dir, &handoff).map(|_| handoff.child_run_id.clone())
        } else if let Some(provider) = providers.iter().find(|provider| {
            provider.app_id == handoff.provider_app_id
                && provider.instance_id == handoff.provider_instance_id
        }) {
            run_handoff(conn, handoff.clone(), app_data_dir, provider, transport)
        } else {
            Err(OcrConsumerError::ProviderUnavailable(
                handoff.provider_instance_id.clone(),
            ))
        };
        match result {
            Ok(child_run_id) => child_run_ids.push(child_run_id),
            Err(error) => warnings.push(format!("{}: {error}", handoff.handoff_id)),
        }
    }
    child_run_ids.sort();
    child_run_ids.dedup();
    Ok(OcrRecoveryReport {
        child_run_ids,
        warnings,
    })
}

fn reload(conn: &Connection, handoff_id: &str) -> Result<OcrHandoff, OcrConsumerError> {
    db::get_ocr_handoff(conn, handoff_id)?
        .ok_or_else(|| StoreError::InvalidOcrHandoff("the OCR handoff disappeared".to_string()))
        .map_err(OcrConsumerError::from)
}

fn map_transport(error: TransportError) -> OcrConsumerError {
    match error {
        TransportError::Refused { message, .. } => OcrConsumerError::ProviderFailed(message),
        TransportError::NotFound => {
            OcrConsumerError::Transport("the provider job is unavailable".to_string())
        }
        TransportError::Uncertain(message) => OcrConsumerError::Transport(message),
        TransportError::Invalid(message) => OcrConsumerError::InvalidOutput(message),
    }
}

fn pdf_page_count(source: &[u8]) -> Result<usize, OcrConsumerError> {
    let pdf = lopdf::Document::load_mem(source).map_err(|_| {
        OcrConsumerError::InvalidOutput("the source is not a readable PDF".to_string())
    })?;
    Ok(pdf.get_pages().len())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn expected_derived_path(app_data_dir: &Path, handoff_id: &str) -> PathBuf {
    app_data_dir
        .join("ocr-derived")
        .join(format!("{handoff_id}.pdf"))
}

fn validate_retained_derived<'a>(
    app_data_dir: &Path,
    handoff: &'a OcrHandoff,
) -> Result<(PathBuf, &'a [u8]), OcrConsumerError> {
    let expected = expected_derived_path(app_data_dir, &handoff.handoff_id);
    if Path::new(&handoff.derived_path) != expected {
        return Err(OcrConsumerError::InvalidOutput(
            "the persisted OCR snapshot path is outside its owner".to_string(),
        ));
    }
    let bytes = handoff.ocr_pdf_bytes.as_ref().ok_or_else(|| {
        OcrConsumerError::InvalidOutput("retained OCR PDF is missing".to_string())
    })?;
    if sha256_hex(bytes) != handoff.ocr_pdf_sha256.as_deref().unwrap_or_default() {
        return Err(OcrConsumerError::InvalidOutput(
            "retained OCR PDF integrity is invalid".to_string(),
        ));
    }
    Ok((expected, bytes))
}

#[cfg(unix)]
fn materialize_derived(app_data_dir: &Path, handoff: &OcrHandoff) -> Result<(), OcrConsumerError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

    let (expected, bytes) = validate_retained_derived(app_data_dir, handoff)?;
    let directory = expected.parent().expect("derived path has a parent");
    match fs::create_dir(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let directory_metadata = fs::symlink_metadata(directory)?;
    if !directory_metadata.file_type().is_dir()
        || directory_metadata.uid() != unsafe { libc::geteuid() }
    {
        return Err(OcrConsumerError::InvalidOutput(
            "the OCR snapshot directory is not owned by the current user".to_string(),
        ));
    }
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    match fs::symlink_metadata(&expected) {
        Ok(metadata) => {
            if !metadata.file_type().is_file()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err(OcrConsumerError::InvalidOutput(
                    "the OCR snapshot is not an owner-private regular file".to_string(),
                ));
            }
            if fs::read(&expected)? == bytes {
                return Ok(());
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let temporary = directory.join(format!(".{}.{}.tmp", handoff.handoff_id, Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, &expected)?;
    File::open(directory)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn materialize_derived(app_data_dir: &Path, handoff: &OcrHandoff) -> Result<(), OcrConsumerError> {
    validate_retained_derived(app_data_dir, handoff)?;
    Err(OcrConsumerError::Discovery(
        "direct OCR recovery is not enabled on this platform".to_string(),
    ))
}

fn discover_one_provider() -> Result<LiveOcrProvider, OcrConsumerError> {
    let providers = discover_providers()?;
    match providers.as_slice() {
        [provider] => Ok(provider.clone()),
        [] => Err(OcrConsumerError::Discovery(
            "no conforming document.ocr provider is live".to_string(),
        )),
        _ => Err(OcrConsumerError::Discovery(
            "multiple document.ocr providers are live; selection is ambiguous".to_string(),
        )),
    }
}

fn discover_selected_provider(instance_id: &str) -> Result<LiveOcrProvider, OcrConsumerError> {
    discover_providers()?
        .into_iter()
        .find(|provider| provider.instance_id == instance_id)
        .ok_or_else(|| OcrConsumerError::ProviderUnavailable(instance_id.to_string()))
}

#[cfg(not(unix))]
fn discover_providers() -> Result<Vec<LiveOcrProvider>, OcrConsumerError> {
    Err(OcrConsumerError::Discovery(
        "direct OCR discovery is not enabled on this platform".to_string(),
    ))
}

#[cfg(unix)]
fn discover_providers() -> Result<Vec<LiveOcrProvider>, OcrConsumerError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| OcrConsumerError::Discovery("XDG_RUNTIME_DIR is unavailable".to_string()))?;
    let runtime_metadata = fs::symlink_metadata(&runtime).map_err(|error| {
        OcrConsumerError::Discovery(format!("runtime directory is unavailable: {error}"))
    })?;
    if !runtime_metadata.file_type().is_dir() || runtime_metadata.permissions().mode() & 0o077 != 0
    {
        return Err(OcrConsumerError::Discovery(
            "runtime directory is not owner-private".to_string(),
        ));
    }
    let owner_uid = unsafe { libc::geteuid() };
    if runtime_metadata.uid() != owner_uid {
        return Err(OcrConsumerError::Discovery(
            "runtime directory is not owned by the current user".to_string(),
        ));
    }
    let directory = runtime.join("local-connect/v2/providers");
    let metadata = fs::symlink_metadata(&directory).map_err(|error| {
        OcrConsumerError::Discovery(format!("provider directory is unavailable: {error}"))
    })?;
    if !metadata.file_type().is_dir()
        || metadata.uid() != owner_uid
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(OcrConsumerError::Discovery(
            "provider directory is not owner-private".to_string(),
        ));
    }
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(DISCOVERY_TIMEOUT)
        .timeout(DISCOVERY_TIMEOUT)
        .build()
        .map_err(|error| OcrConsumerError::Discovery(error.to_string()))?;
    let mut providers = Vec::new();
    for entry in
        fs::read_dir(&directory).map_err(|error| OcrConsumerError::Discovery(error.to_string()))?
    {
        let path = match entry {
            Ok(entry) => entry.path(),
            Err(_) => continue,
        };
        let Some(registration) = read_registration(&path, owner_uid) else {
            continue;
        };
        if registration.app_id != OCR_APP_ID
            || registration.protocol_version != PROTOCOL_VERSION
            || registration.transport.kind != TRANSPORT_KIND
            || registration.auth.scheme != "bearer"
            || registration.auth.token.is_empty()
            || !valid_uuid_v4(&registration.instance_id)
            || validate_base_url(&registration.transport.base_url).is_none()
        {
            continue;
        }
        let response = match client
            .get(endpoint(&registration.transport.base_url, "v2/manifest").map_err(map_transport)?)
            .bearer_auth(&registration.auth.token)
            .send()
        {
            Ok(response) if response.status() == StatusCode::OK => response,
            _ => continue,
        };
        let bytes = match read_bounded_response(response, MAX_MANIFEST_BYTES) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let manifest: AppManifest = match serde_json::from_slice(&bytes) {
            Ok(manifest) => manifest,
            Err(_) => continue,
        };
        if valid_ocr_manifest(&manifest, &registration) {
            providers.push(LiveOcrProvider {
                app_id: registration.app_id,
                instance_id: registration.instance_id,
                base_url: registration.transport.base_url,
                token: registration.auth.token,
            });
        }
    }
    providers.sort_by(|left, right| left.instance_id.cmp(&right.instance_id));
    providers.dedup_by(|left, right| left.instance_id == right.instance_id);
    Ok(providers)
}

#[cfg(unix)]
fn read_registration(path: &Path, owner_uid: u32) -> Option<RuntimeRegistration> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file()
        || metadata.uid() != owner_uid
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() == 0
        || metadata.len() > MAX_REGISTRATION_BYTES
    {
        return None;
    }
    let file = File::open(path).ok()?;
    let opened = file.metadata().ok()?;
    if !opened.is_file()
        || opened.uid() != metadata.uid()
        || opened.ino() != metadata.ino()
        || opened.len() != metadata.len()
    {
        return None;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_REGISTRATION_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_REGISTRATION_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

#[cfg(any(unix, test))]
fn valid_ocr_manifest(manifest: &AppManifest, registration: &RuntimeRegistration) -> bool {
    if manifest.protocol_version != PROTOCOL_VERSION
        || manifest.instance_id != registration.instance_id
        || manifest.app.id != OCR_APP_ID
        || manifest.capabilities.len() != 1
    {
        return false;
    }
    let capability = &manifest.capabilities[0];
    capability.id == OCR_CAPABILITY_ID
        && capability.version == OCR_CAPABILITY_VERSION
        && capability.accepts.len() == 1
        && capability.accepts[0].media_type == PDF_MEDIA_TYPE
        && capability.accepts[0].max_bytes == MAX_INPUT_BYTES as u64
        && capability.produces.len() == 2
        && capability
            .produces
            .iter()
            .filter(|media| media.as_str() == OCR_INPUT_MEDIA_TYPE)
            .count()
            == 1
        && capability
            .produces
            .iter()
            .filter(|media| media.as_str() == TEXT_MEDIA_TYPE)
            .count()
            == 1
        && capability.parameters.is_empty()
        && !capability.effects.external
        && !capability.effects.confirmation_required
}

#[cfg(unix)]
fn validate_base_url(value: &str) -> Option<Url> {
    let url = Url::parse(value).ok()?;
    (url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none())
    .then_some(url)
}

#[cfg(unix)]
fn valid_uuid_v4(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|uuid| {
        uuid.get_version() == Some(Version::Random) && uuid.get_variant() == Variant::RFC4122
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::contracts::AppDescription;
    #[cfg(unix)]
    use crate::connect::contracts::{ArtifactProvenance, ProviderRef};
    #[cfg(unix)]
    use crate::connect::v2::JobResult;
    use crate::pipeline::contracts::{
        ModelProfileSnapshot, ModelStageProfileSnapshot, ParsedPage, PipelineWarning,
        SummaryProfile,
    };
    use crate::pipeline::ingest::prepare_pdf_ingestion;
    use crate::pipeline::parser::{parse_started_document, PdfExtractParser};
    use lopdf::{dictionary, Dictionary, Document, Object, Stream};
    use std::sync::Mutex;
    use tempfile::TempDir;

    fn image_only_pdf() -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let image_id = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 1,
                "Height" => 1,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
            },
            vec![0, 0, 0],
        ));
        let content = lopdf::content::Content {
            operations: vec![
                lopdf::content::Operation::new("q", vec![]),
                lopdf::content::Operation::new(
                    "cm",
                    vec![
                        72.into(),
                        0.into(),
                        0.into(),
                        72.into(),
                        72.into(),
                        72.into(),
                    ],
                ),
                lopdf::content::Operation::new("Do", vec!["Im1".into()]),
                lopdf::content::Operation::new("Q", vec![]),
            ],
        }
        .encode()
        .unwrap();
        let content_id = document.add_object(Stream::new(Dictionary::new(), content));
        let first_page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => dictionary! { "XObject" => dictionary! { "Im1" => image_id } },
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        let second_page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => dictionary! { "XObject" => dictionary! { "Im1" => image_id } },
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(first_page_id), Object::Reference(second_page_id)],
                "Count" => 2,
            }),
        );
        let catalog_id =
            document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    fn snapshot() -> ModelProfileSnapshot {
        let stage = ModelStageProfileSnapshot {
            runtime_kind: Default::default(),
            profile_id: "ocr-test-profile".to_string(),
            model_name: "ocr-test-model".to_string(),
            model_digest: "ocr-test-digest".to_string(),
            context_tokens: 8_192,
            tokenizer_version: "ocr-test-tokenizer".to_string(),
        };
        ModelProfileSnapshot {
            version: 1,
            preset_id: "ocr-test-preset".to_string(),
            analysis: stage.clone(),
            verification: stage,
        }
    }

    fn live_provider() -> LiveOcrProvider {
        LiveOcrProvider {
            app_id: OCR_APP_ID.to_string(),
            instance_id: "11111111-1111-4111-8111-111111111111".to_string(),
            base_url: "http://127.0.0.1:4545/".to_string(),
            token: "fixture-token".to_string(),
        }
    }

    #[cfg(unix)]
    fn completed_status(handoff: &OcrHandoff) -> JobStatus {
        let pdf = include_bytes!("../../tests/fixtures/ocr_tagged.pdf").to_vec();
        let text = canonical_tagged_ocr_text(&pdf).unwrap();
        let output = |artifact_id: &str, media_type: &str, bytes: Vec<u8>| OutputArtifact {
            artifact_id: artifact_id.to_string(),
            media_type: media_type.to_string(),
            display_name: if media_type == TEXT_MEDIA_TYPE {
                "recognized.txt".to_string()
            } else {
                "recognized.pdf".to_string()
            },
            byte_size: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
            payload_base64: BASE64.encode(bytes),
        };
        JobStatus {
            protocol_version: PROTOCOL_VERSION,
            job_id: handoff.provider_job_id.clone(),
            capability: CapabilityRef {
                id: OCR_CAPABILITY_ID.to_string(),
                version: OCR_CAPABILITY_VERSION.to_string(),
            },
            provider: ProviderRef {
                app_id: handoff.provider_app_id.clone(),
                instance_id: handoff.provider_instance_id.clone(),
            },
            status: JobState::Completed,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            input_artifacts: vec![ArtifactProvenance {
                artifact_id: handoff.source_artifact_id.clone(),
                media_type: PDF_MEDIA_TYPE.to_string(),
                byte_size: handoff.source_byte_size,
                sha256: handoff.source_sha256.clone(),
            }],
            result: Some(JobResult {
                outputs: vec![
                    output(
                        "22222222-2222-4222-8222-222222222222",
                        OCR_INPUT_MEDIA_TYPE,
                        pdf,
                    ),
                    output(
                        "33333333-3333-4333-8333-333333333333",
                        TEXT_MEDIA_TYPE,
                        text,
                    ),
                ],
            }),
            error: None,
        }
    }

    #[derive(Default)]
    struct LostAckTransport {
        calls: Mutex<Vec<&'static str>>,
        completed: Mutex<Option<JobStatus>>,
    }

    impl OcrTransport for LostAckTransport {
        fn submit(
            &self,
            _provider: &LiveOcrProvider,
            _request: &JobRequest,
            _source: &[u8],
            _deadline: Instant,
        ) -> Result<JobStatus, TransportError> {
            self.calls.lock().unwrap().push("submit");
            Err(TransportError::Uncertain(
                "lost acknowledgement".to_string(),
            ))
        }

        fn status(
            &self,
            _provider: &LiveOcrProvider,
            _job_id: &str,
            _deadline: Instant,
        ) -> Result<JobStatus, TransportError> {
            self.calls.lock().unwrap().push("status");
            self.completed
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| TransportError::Uncertain("status unavailable".to_string()))
        }
    }

    #[test]
    fn routing_requires_the_whole_document_no_text_signal() {
        let page = ParsedPage {
            page_number: 1,
            text: String::new(),
            warnings: vec![PipelineWarning {
                code: "NO_NATIVE_TEXT".to_string(),
                message: "NO_NATIVE_TEXT".to_string(),
                stage: None,
            }],
            requires_visual_processing: true,
        };
        let mut parsed = ParsedDocument {
            document_id: "document".to_string(),
            parser_id: "pdf-extract".to_string(),
            parser_version: "0.12.0".to_string(),
            source_type: SourceType::NativeText,
            pages: vec![page],
            warnings: Vec::new(),
        };
        assert!(!parsed_document_requires_ocr(&parsed));
        parsed.warnings.push(PipelineWarning {
            code: "NO_NATIVE_TEXT_IN_DOCUMENT".to_string(),
            message: "NO_NATIVE_TEXT_IN_DOCUMENT".to_string(),
            stage: None,
        });
        assert!(parsed_document_requires_ocr(&parsed));
        parsed.source_type = SourceType::OcrText;
        assert!(!parsed_document_requires_ocr(&parsed));
    }

    #[test]
    fn manifest_profile_checks_both_sides_of_the_exact_boundary() {
        let registration = RuntimeRegistration {
            protocol_version: PROTOCOL_VERSION,
            instance_id: live_provider().instance_id,
            app_id: OCR_APP_ID.to_string(),
            pid: 7,
            started_at: Utc::now(),
            transport: crate::connect::contracts::TransportRegistration {
                kind: TRANSPORT_KIND.to_string(),
                base_url: "http://127.0.0.1:4545/".to_string(),
            },
            auth: crate::connect::contracts::AuthRegistration {
                scheme: "bearer".to_string(),
                token: "token".to_string(),
            },
        };
        let mut manifest = AppManifest {
            protocol_version: PROTOCOL_VERSION,
            instance_id: registration.instance_id.clone(),
            app: AppDescription {
                id: OCR_APP_ID.to_string(),
                name: "OCR".to_string(),
                version: "1.0".to_string(),
            },
            capabilities: vec![crate::connect::v2::CapabilityDeclaration {
                id: OCR_CAPABILITY_ID.to_string(),
                version: OCR_CAPABILITY_VERSION.to_string(),
                action: crate::connect::v2::ActionDescription {
                    label: "Recognize".to_string(),
                    description: "OCR".to_string(),
                },
                accepts: vec![crate::connect::contracts::AcceptedMediaType {
                    media_type: PDF_MEDIA_TYPE.to_string(),
                    max_bytes: MAX_INPUT_BYTES as u64,
                }],
                produces: vec![
                    OCR_INPUT_MEDIA_TYPE.to_string(),
                    TEXT_MEDIA_TYPE.to_string(),
                ],
                parameters: Vec::new(),
                effects: crate::connect::v2::CapabilityEffects {
                    external: false,
                    confirmation_required: false,
                },
            }],
        };
        assert!(valid_ocr_manifest(&manifest, &registration));
        manifest.capabilities[0].produces[1] = OCR_INPUT_MEDIA_TYPE.to_string();
        assert!(!valid_ocr_manifest(&manifest, &registration));
        manifest.capabilities[0].produces[1] = TEXT_MEDIA_TYPE.to_string();
        manifest.capabilities[0].effects.external = true;
        assert!(!valid_ocr_manifest(&manifest, &registration));
    }

    #[test]
    fn tampered_saved_request_fails_before_transport() {
        let directory = TempDir::new().unwrap();
        let source_path = directory.path().join("scan.pdf");
        fs::write(&source_path, image_only_pdf()).unwrap();
        let mut conn = db::init_db(directory.path().join("summarizer.db")).unwrap();
        let (document, received) =
            prepare_pdf_ingestion(source_path.to_str().unwrap(), None).unwrap();
        let ingested = db::persist_ingestion_with_profiles(
            &mut conn,
            &document,
            &received,
            Some(&snapshot()),
            SummaryProfile::General,
        )
        .unwrap();
        let (parsing, persisted) =
            db::start_parsing(&mut conn, &ingested.run_id, ingested.state_version).unwrap();
        parse_started_document(
            &mut conn,
            &PdfExtractParser::new(),
            &parsing.run_id,
            parsing.state_version,
            &persisted,
        )
        .unwrap();

        let provider = live_provider();
        let handoff =
            prepare_handoff(&mut conn, &parsing.run_id, directory.path(), &provider).unwrap();
        conn.execute(
            "UPDATE ocr_handoffs SET provider_request_json = '{}' WHERE handoff_id = ?1",
            [&handoff.handoff_id],
        )
        .unwrap();
        let transport = LostAckTransport::default();
        let tampered = db::get_ocr_handoff(&conn, &handoff.handoff_id)
            .unwrap()
            .unwrap();
        assert!(matches!(
            run_handoff(&mut conn, tampered, directory.path(), &provider, &transport,),
            Err(OcrConsumerError::InvalidOutput(_))
        ));
        assert!(transport.calls.lock().unwrap().is_empty());
        assert_eq!(
            db::get_ocr_handoff(&conn, &handoff.handoff_id)
                .unwrap()
                .unwrap()
                .phase,
            "failed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn lost_acknowledgement_reconciles_before_one_child_admission_after_reopen() {
        let directory = TempDir::new().unwrap();
        let source_path = directory.path().join("scan.pdf");
        fs::write(&source_path, image_only_pdf()).unwrap();
        let database = directory.path().join("summarizer.db");
        let mut conn = db::init_db(&database).unwrap();
        let (document, received) =
            prepare_pdf_ingestion(source_path.to_str().unwrap(), None).unwrap();
        let ingested = db::persist_ingestion_with_profiles(
            &mut conn,
            &document,
            &received,
            Some(&snapshot()),
            SummaryProfile::General,
        )
        .unwrap();
        let (parsing, persisted) =
            db::start_parsing(&mut conn, &ingested.run_id, ingested.state_version).unwrap();
        let parsed = parse_started_document(
            &mut conn,
            &PdfExtractParser::new(),
            &parsing.run_id,
            parsing.state_version,
            &persisted,
        )
        .unwrap();
        assert!(parsed_document_requires_ocr(&parsed));

        let provider = live_provider();
        let transport = LostAckTransport::default();
        let first = process_scanned_document_with(
            &mut conn,
            &parsing.run_id,
            directory.path(),
            &provider,
            &transport,
        );
        assert!(matches!(first, Err(OcrConsumerError::Transport(_))));
        let handoff = db::get_ocr_handoff_for_root(&conn, &parsing.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(handoff.phase, "submission_uncertain");
        let mut replacement = provider.clone();
        replacement.instance_id = "44444444-4444-4444-8444-444444444444".to_string();
        assert!(matches!(
            process_scanned_document_with(
                &mut conn,
                &parsing.run_id,
                directory.path(),
                &replacement,
                &transport,
            ),
            Err(OcrConsumerError::ProviderUnavailable(_))
        ));
        assert_eq!(transport.calls.lock().unwrap().as_slice(), ["submit"]);

        let mut invalid_status = completed_status(&handoff);
        let invalid_text = b"wrong page one\x0cwrong page two".to_vec();
        let text_output = invalid_status
            .result
            .as_mut()
            .unwrap()
            .outputs
            .iter_mut()
            .find(|output| output.media_type == TEXT_MEDIA_TYPE)
            .unwrap();
        text_output.byte_size = invalid_text.len() as u64;
        text_output.sha256 = sha256_hex(&invalid_text);
        text_output.payload_base64 = BASE64.encode(&invalid_text);
        assert!(matches!(
            validate_completed_outputs(&handoff, &invalid_status),
            Err(OcrConsumerError::InvalidOutput(_))
        ));
        *transport.completed.lock().unwrap() = Some(completed_status(&handoff));

        drop(conn);
        let mut reopened = db::init_db(&database).unwrap();
        let recovery = recover_ocr_handoffs_with(
            &mut reopened,
            directory.path(),
            std::slice::from_ref(&provider),
            &transport,
        )
        .unwrap();
        assert!(recovery.warnings.is_empty());
        assert_eq!(recovery.child_run_ids.len(), 1);
        let child_run_id = recovery.child_run_ids[0].clone();
        assert_eq!(
            transport.calls.lock().unwrap().as_slice(),
            ["submit", "status"]
        );
        let same_child = process_scanned_document_with(
            &mut reopened,
            &parsing.run_id,
            directory.path(),
            &provider,
            &transport,
        )
        .unwrap();
        assert_eq!(same_child, child_run_id);
        assert_eq!(
            transport.calls.lock().unwrap().as_slice(),
            ["submit", "status"]
        );
        let child = db::get_pipeline_run(&reopened, &child_run_id)
            .unwrap()
            .unwrap();
        let child_document = db::get_document(&reopened, &child.document_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            child.state,
            crate::pipeline::contracts::PipelineState::Ingested
        );
        assert_eq!(child_document.source_type, SourceType::OcrText);
        assert_eq!(
            reopened
                .query_row("SELECT COUNT(*) FROM ocr_lineage", [], |row| row
                    .get::<_, u32>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            reopened
                .query_row("SELECT COUNT(*) FROM ocr_handoffs", [], |row| row
                    .get::<_, u32>(0))
                .unwrap(),
            1
        );
        assert!(Path::new(&child_document.local_source_path).is_file());
        let projected = crate::pipeline::workspace::get_run(&reopened, &parsing.run_id).unwrap();
        assert_eq!(projected.run_id, child_run_id);
        let recent = crate::pipeline::workspace::list_recent_runs(&reopened).unwrap();
        assert!(recent.iter().any(|run| run.run_id == child_run_id));
        assert!(recent.iter().all(|run| run.run_id != parsing.run_id));
    }
}
