#![allow(
    dead_code,
    reason = "the authenticated client lands immediately before its run-bound ModelRuntime adapter"
)]

use crate::pipeline::contracts::{ModelOutputFormat, ModelRequest};
use crate::pipeline::gateway_store::{
    load_request, mark_acknowledged, mark_submitted, persist_completion,
    reserve_request_after_lock, GatewayCompletion, GatewayRequestKey, GatewayRequestRecord,
    GatewayRequestState, GatewayStoreError,
};
use chrono::{DateTime, Duration as ChronoDuration, Timelike, Utc};
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_TYPE};
use reqwest::redirect::Policy;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

const PROTOCOL_VERSION: u32 = 1;
const TASK_ID: &str = "document.summary.step";
const TASK_VERSION: u32 = 1;
const MAX_REQUEST_BYTES: usize = 1_000_000;
const MAX_RESPONSE_BYTES: usize = 1_000_000;
const MAX_MESSAGE_CHARS: usize = 500_000;
const MAX_SCHEMA_BYTES: usize = 250_000;
const MAX_OUTPUT_TOKENS: u32 = 4_096;
const MIN_TOKEN_BYTES: usize = 32;
const MAX_TOKEN_BYTES: usize = 512;
const MAX_CA_BYTES: usize = 1_000_000;
const MAX_RETRY_AFTER_SECONDS: u64 = 3_600;

#[derive(Debug, Clone)]
pub(crate) struct GatewayClientConfig {
    pub base_url: String,
    pub token_file: PathBuf,
    pub ca_file: PathBuf,
    pub timeout: Duration,
    pub request_lifetime: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayResult {
    pub content: String,
    pub deployment_id: String,
    pub task_policy_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayHealth {
    pub available: bool,
    pub status: String,
}

#[derive(Debug, Error)]
pub(crate) enum GatewayClientError {
    #[error(transparent)]
    Store(#[from] GatewayStoreError),
    #[error("Inference gateway configuration is invalid: {0}")]
    Configuration(&'static str),
    #[error("Inference gateway credential is unavailable or invalid")]
    Credential,
    #[error("Inference gateway transport failed")]
    Transport,
    #[error("Inference gateway request expired before its outcome could be reconciled")]
    Expired,
    #[error("Inference gateway response violated the protocol: {0}")]
    Protocol(&'static str),
    #[error("Inference gateway rejected the request: {code}")]
    Rejected {
        code: String,
        retryable: bool,
        retry_after_seconds: Option<u64>,
    },
}

impl GatewayClientError {
    pub(crate) fn recoverable(&self) -> bool {
        match self {
            Self::Transport => true,
            Self::Rejected { retryable, .. } => *retryable,
            Self::Store(_)
            | Self::Configuration(_)
            | Self::Credential
            | Self::Expired
            | Self::Protocol(_) => false,
        }
    }
}

#[derive(Debug)]
struct RawResponse {
    status: u16,
    content_type: Option<String>,
    content_encoding: Option<String>,
    body: Vec<u8>,
}

trait GatewayTransport: Send + Sync {
    fn request(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<RawResponse, GatewayClientError>;
}

struct ReqwestGatewayTransport {
    base_url: String,
    client: Client,
}

impl GatewayTransport for ReqwestGatewayTransport {
    fn request(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<RawResponse, GatewayClientError> {
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| GatewayClientError::Configuration("HTTP method is invalid"))?;
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path))
            .timeout(timeout)
            .header(ACCEPT_ENCODING, "identity")
            .bearer_auth(token);
        if let Some(body) = body {
            request = request
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_vec());
        }
        let mut response = request.send().map_err(|_| GatewayClientError::Transport)?;
        let status = response.status().as_u16();
        let content_type = bounded_header(response.headers().get(CONTENT_TYPE))?;
        let content_encoding = bounded_header(response.headers().get(CONTENT_ENCODING))?;
        let mut response_body = Vec::new();
        response
            .by_ref()
            .take((MAX_RESPONSE_BYTES + 1) as u64)
            .read_to_end(&mut response_body)
            .map_err(|_| GatewayClientError::Transport)?;
        if response_body.len() > MAX_RESPONSE_BYTES {
            return Err(GatewayClientError::Protocol("response exceeds byte limit"));
        }
        Ok(RawResponse {
            status,
            content_type,
            content_encoding,
            body: response_body,
        })
    }
}

pub(crate) struct GatewayClient {
    token_file: PathBuf,
    timeout: Duration,
    request_lifetime: ChronoDuration,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    transport: Arc<dyn GatewayTransport>,
}

enum TokenOrTerminal {
    Token(String),
    Terminal(GatewayResult),
}

impl GatewayClient {
    pub(crate) fn new(config: GatewayClientConfig) -> Result<Self, GatewayClientError> {
        let base_url = validate_https_origin(&config.base_url)?;
        let timeout = validate_duration(config.timeout, "timeout is outside its bounds")?;
        let request_lifetime = validate_duration(
            config.request_lifetime,
            "request lifetime is outside its bounds",
        )?;
        let ca_bytes = read_bounded_regular(&config.ca_file, MAX_CA_BYTES, FilePolicy::TrustRoot)?;
        let certificates = reqwest::Certificate::from_pem_bundle(&ca_bytes)
            .map_err(|_| GatewayClientError::Configuration("CA bundle is invalid"))?;
        if certificates.is_empty() {
            return Err(GatewayClientError::Configuration("CA bundle is empty"));
        }
        let mut builder = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .timeout(timeout);
        for certificate in certificates {
            builder = builder.add_root_certificate(certificate);
        }
        let client = builder
            .build()
            .map_err(|_| GatewayClientError::Configuration("TLS client could not be built"))?;
        Ok(Self {
            token_file: config.token_file,
            timeout,
            request_lifetime: ChronoDuration::from_std(request_lifetime).map_err(|_| {
                GatewayClientError::Configuration("request lifetime is outside its bounds")
            })?,
            clock: Arc::new(Utc::now),
            transport: Arc::new(ReqwestGatewayTransport { base_url, client }),
        })
    }

    #[cfg(test)]
    fn with_transport(
        token_file: PathBuf,
        timeout: Duration,
        request_lifetime: Duration,
        transport: Arc<dyn GatewayTransport>,
        clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    ) -> Self {
        Self {
            token_file,
            timeout,
            request_lifetime: ChronoDuration::from_std(request_lifetime).unwrap(),
            clock,
            transport,
        }
    }

    pub(crate) fn health(&self) -> Result<GatewayHealth, GatewayClientError> {
        let response = self.send("GET", "/v1/health", None, Duration::from_secs(5))?;
        require_success_status(&response)?;
        let health: HealthEnvelope = decode_response(&response)?;
        if health.protocol_version != PROTOCOL_VERSION {
            return Err(GatewayClientError::Protocol(
                "health protocol version is unsupported",
            ));
        }
        let task = health
            .tasks
            .into_iter()
            .find(|task| task.id == TASK_ID && task.version == TASK_VERSION)
            .ok_or(GatewayClientError::Protocol(
                "document task is not advertised",
            ))?;
        if !matches!(
            task.status.as_str(),
            "available" | "degraded" | "unavailable"
        ) {
            return Err(GatewayClientError::Protocol(
                "document task status is invalid",
            ));
        }
        Ok(GatewayHealth {
            available: matches!(task.status.as_str(), "available" | "degraded"),
            status: task.status,
        })
    }

    pub(crate) fn preflight(&self, request: &ModelRequest) -> Result<(), GatewayClientError> {
        let core = request_core(request)?;
        preflight_request_size(&core)
    }

    pub(crate) fn execute(
        &self,
        conn: &mut Connection,
        key: &GatewayRequestKey,
        request: &ModelRequest,
        now: DateTime<Utc>,
    ) -> Result<GatewayResult, GatewayClientError> {
        if key.stage != request.stage || key.ordinal != request.ordinal {
            return Err(GatewayClientError::Protocol(
                "request identity does not match ledger key",
            ));
        }
        let core = request_core(request)?;
        let semantic_hash = sha256_hex(&encode_json(&core)?);
        preflight_request_size(&core)?;
        let record = match reserve_request_after_lock(conn, key, &semantic_hash, || {
            let reserved_at = (self.clock)();
            let request_expires_at = request_expiry(reserved_at, self.request_lifetime)
                .map_err(|_| GatewayStoreError::InvalidInput("request expiry overflowed"))?;
            Ok((request_expires_at, reserved_at))
        }) {
            Ok(record) => record,
            Err(error) => matching_terminal_after_store_error(conn, key, &semantic_hash, error)?,
        };
        self.execute_reserved(conn, key, record, core, now)
    }

    fn execute_reserved(
        &self,
        conn: &mut Connection,
        key: &GatewayRequestKey,
        record: GatewayRequestRecord,
        core: RequestCore<'_>,
        now: DateTime<Utc>,
    ) -> Result<GatewayResult, GatewayClientError> {
        if let Some(result) = self.resolve_local_record(conn, key, &record, now)? {
            return Ok(result);
        }

        let payload = InferenceEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: &record.request_id,
            request_expires_at: &record.request_expires_at,
            core,
        };
        let request_body = encode_json(&payload)?;
        if request_body.len() > MAX_REQUEST_BYTES {
            return Err(GatewayClientError::Protocol("request exceeds byte limit"));
        }
        let request_hash = sha256_hex(&request_body);
        let submitted = match mark_submitted(conn, key, &request_hash, (self.clock)()) {
            Ok(submitted) => submitted,
            Err(error) => matching_terminal_after_store_error(
                conn,
                key,
                &record.semantic_request_hash,
                error,
            )?,
        };
        if let Some(result) = self.resolve_local_record(conn, key, &submitted, (self.clock)())? {
            return Ok(result);
        }
        if !matches!(submitted.state, GatewayRequestState::Submitted) {
            return Err(GatewayClientError::Protocol(
                "local request state is invalid before submission",
            ));
        }
        let token = match self.read_token_or_terminal(conn, key)? {
            TokenOrTerminal::Token(token) => token,
            TokenOrTerminal::Terminal(result) => return Ok(result),
        };
        let current = load_request(conn, key)?.ok_or(GatewayStoreError::RequestNotFound)?;
        if let Some(result) = self.resolve_local_record(conn, key, &current, (self.clock)())? {
            return Ok(result);
        }
        if !matches!(current.state, GatewayRequestState::Submitted) {
            return Err(GatewayClientError::Protocol(
                "local request state is invalid before transport",
            ));
        }
        let response = match self.transport.request(
            "POST",
            "/v1/inference",
            &token,
            Some(&request_body),
            self.timeout,
        ) {
            Ok(response) => response,
            Err(error) => return self.reconcile_or_propagate(conn, key, error),
        };
        let completion = match parse_inference_response(&response, &record.request_id) {
            Ok(completion) => completion,
            Err(error) => return self.reconcile_or_propagate(conn, key, error),
        };
        let persisted =
            match persist_completion(conn, key, &request_hash, &completion, (self.clock)()) {
                Ok(persisted) => persisted,
                Err(error) => {
                    let current =
                        load_request(conn, key)?.ok_or(GatewayStoreError::RequestNotFound)?;
                    if current.completion.as_ref() != Some(&completion) {
                        return Err(error.into());
                    }
                    current
                }
            };
        self.resolve_local_record(conn, key, &persisted, (self.clock)())?
            .ok_or(GatewayClientError::Protocol(
                "persisted completion state is invalid",
            ))
    }

    fn resolve_local_record(
        &self,
        conn: &mut Connection,
        key: &GatewayRequestKey,
        record: &GatewayRequestRecord,
        now: DateTime<Utc>,
    ) -> Result<Option<GatewayResult>, GatewayClientError> {
        match record.state {
            GatewayRequestState::Acknowledged => result_from_record(record).map(Some),
            GatewayRequestState::Completed => {
                let result = result_from_record(record)?;
                if request_expired(record, (self.clock)())? {
                    return Ok(Some(result));
                }
                let token = match self.read_token_or_terminal(conn, key)? {
                    TokenOrTerminal::Token(token) => token,
                    TokenOrTerminal::Terminal(result) => return Ok(Some(result)),
                };
                if request_expired(record, (self.clock)())? {
                    return Ok(Some(result));
                }
                match self.acknowledge(&record.request_id, &token) {
                    Ok(()) => {
                        // The completion is already durable and the gateway has accepted the ACK.
                        // A transient local write failure must not turn that success into a failed
                        // pipeline; leaving the row Completed keeps later reconciliation idempotent.
                        let _ = mark_acknowledged(conn, key, (self.clock)());
                        Ok(Some(result))
                    }
                    Err(error) => {
                        let current =
                            load_request(conn, key)?.ok_or(GatewayStoreError::RequestNotFound)?;
                        match current.state {
                            GatewayRequestState::Acknowledged => {
                                result_from_record(&current).map(Some)
                            }
                            GatewayRequestState::Completed
                                if request_expired(&current, (self.clock)())? =>
                            {
                                result_from_record(&current).map(Some)
                            }
                            _ => Err(error),
                        }
                    }
                }
            }
            GatewayRequestState::Submitted if request_expired(record, now)? => {
                Err(GatewayClientError::Expired)
            }
            GatewayRequestState::Reserved | GatewayRequestState::Submitted => Ok(None),
        }
    }

    fn acknowledge(&self, request_id: &str, token: &str) -> Result<(), GatewayClientError> {
        let body = encode_json(&AcknowledgementEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            disposition: "persisted",
        })?;
        let response = self.transport.request(
            "POST",
            &format!("/v1/inference/{request_id}/ack"),
            token,
            Some(&body),
            self.timeout,
        )?;
        let acknowledgement = parse_acknowledgement(&response)?;
        if acknowledgement.protocol_version != PROTOCOL_VERSION
            || acknowledgement.request_id != request_id
            || acknowledgement.status != "acknowledged"
            || acknowledgement.disposition != "persisted"
        {
            return Err(GatewayClientError::Protocol(
                "acknowledgement envelope is invalid",
            ));
        }
        Ok(())
    }

    fn reconcile_or_propagate(
        &self,
        conn: &mut Connection,
        key: &GatewayRequestKey,
        error: GatewayClientError,
    ) -> Result<GatewayResult, GatewayClientError> {
        let current = load_request(conn, key)?.ok_or(GatewayStoreError::RequestNotFound)?;
        match self.resolve_local_record(conn, key, &current, (self.clock)())? {
            Some(result) => Ok(result),
            None => Err(error),
        }
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<RawResponse, GatewayClientError> {
        let token = self.read_token()?;
        self.transport.request(method, path, &token, body, timeout)
    }

    fn read_token(&self) -> Result<String, GatewayClientError> {
        let token_bytes =
            read_bounded_regular(&self.token_file, MAX_TOKEN_BYTES, FilePolicy::Token)
                .map_err(|_| GatewayClientError::Credential)?;
        let token = std::str::from_utf8(&token_bytes)
            .map_err(|_| GatewayClientError::Credential)?
            .trim();
        if token.is_empty()
            || token.len() < MIN_TOKEN_BYTES
            || token.len() > MAX_TOKEN_BYTES
            || !token
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_whitespace())
        {
            return Err(GatewayClientError::Credential);
        }
        Ok(token.to_string())
    }

    fn read_token_or_terminal(
        &self,
        conn: &Connection,
        key: &GatewayRequestKey,
    ) -> Result<TokenOrTerminal, GatewayClientError> {
        match self.read_token() {
            Ok(token) => Ok(TokenOrTerminal::Token(token)),
            Err(error) => {
                let current = load_request(conn, key)?.ok_or(GatewayStoreError::RequestNotFound)?;
                match current.state {
                    GatewayRequestState::Acknowledged => {
                        result_from_record(&current).map(TokenOrTerminal::Terminal)
                    }
                    GatewayRequestState::Completed
                        if request_expired(&current, (self.clock)())? =>
                    {
                        result_from_record(&current).map(TokenOrTerminal::Terminal)
                    }
                    _ => Err(error),
                }
            }
        }
    }
}

fn matching_terminal_after_store_error(
    conn: &Connection,
    key: &GatewayRequestKey,
    semantic_hash: &str,
    error: GatewayStoreError,
) -> Result<GatewayRequestRecord, GatewayClientError> {
    let current = load_request(conn, key)?.ok_or(GatewayStoreError::RequestNotFound)?;
    if current.semantic_request_hash != semantic_hash
        || !matches!(
            current.state,
            GatewayRequestState::Completed | GatewayRequestState::Acknowledged
        )
    {
        return Err(error.into());
    }
    Ok(current)
}

#[derive(Serialize)]
struct TaskReference<'a> {
    id: &'a str,
    version: u32,
}

#[derive(Serialize)]
struct Requirements {
    input_modalities: [&'static str; 1],
    output_media_type: &'static str,
    structured_output: bool,
    max_output_tokens: u32,
}

#[derive(Serialize)]
struct GenerationMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct Generation<'a> {
    messages: [GenerationMessage<'a>; 2],
    temperature: f64,
    seed: u64,
    response_schema: &'a Value,
}

#[derive(Serialize)]
struct RequestCore<'a> {
    task: TaskReference<'a>,
    requirements: Requirements,
    generation: Generation<'a>,
}

#[derive(Serialize)]
struct InferenceEnvelope<'a> {
    protocol_version: u32,
    request_id: &'a str,
    request_expires_at: &'a str,
    #[serde(flatten)]
    core: RequestCore<'a>,
}

#[derive(Serialize)]
struct AcknowledgementEnvelope<'a> {
    protocol_version: u32,
    request_id: &'a str,
    disposition: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InferenceResponse {
    protocol_version: u32,
    request_id: String,
    status: String,
    output: OutputEnvelope,
    provenance: ProvenanceEnvelope,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputEnvelope {
    media_type: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceEnvelope {
    task_policy_version: u32,
    deployment_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FailureEnvelope {
    protocol_version: u32,
    request_id: Option<String>,
    status: String,
    error: FailureDetail,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FailureDetail {
    code: String,
    retryable: bool,
    retry_after_seconds: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcknowledgementResponse {
    protocol_version: u32,
    request_id: String,
    status: String,
    disposition: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HealthEnvelope {
    protocol_version: u32,
    tasks: Vec<HealthTask>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HealthTask {
    id: String,
    version: u32,
    status: String,
    diagnostic_code: String,
}

fn request_core(request: &ModelRequest) -> Result<RequestCore<'_>, GatewayClientError> {
    if request.system_prompt.is_empty()
        || request.user_prompt.is_empty()
        || request.system_prompt.chars().count() > MAX_MESSAGE_CHARS
        || request.user_prompt.chars().count() > MAX_MESSAGE_CHARS
        || request.seed > i64::MAX as u64
        || request.max_output_tokens == 0
        || request.max_output_tokens > MAX_OUTPUT_TOKENS
    {
        return Err(GatewayClientError::Protocol(
            "model request is outside task bounds",
        ));
    }
    let schema = match &request.output_format {
        ModelOutputFormat::JsonSchema { schema, .. } => schema,
        ModelOutputFormat::Text => {
            return Err(GatewayClientError::Protocol(
                "document gateway task requires structured output",
            ))
        }
    };
    if encode_json(schema)?.len() > MAX_SCHEMA_BYTES || !schema.is_object() {
        return Err(GatewayClientError::Protocol(
            "response schema is outside task bounds",
        ));
    }
    Ok(RequestCore {
        task: TaskReference {
            id: TASK_ID,
            version: TASK_VERSION,
        },
        requirements: Requirements {
            input_modalities: ["text"],
            output_media_type: "application/json",
            structured_output: true,
            max_output_tokens: request.max_output_tokens,
        },
        generation: Generation {
            messages: [
                GenerationMessage {
                    role: "system",
                    content: &request.system_prompt,
                },
                GenerationMessage {
                    role: "user",
                    content: &request.user_prompt,
                },
            ],
            temperature: 0.0,
            seed: request.seed,
            response_schema: schema,
        },
    })
}

fn preflight_request_size(core: &RequestCore<'_>) -> Result<(), GatewayClientError> {
    let template = InferenceEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: "00000000-0000-4000-8000-000000000000",
        request_expires_at: "2000-01-01T00:00:00Z",
        core: RequestCore {
            task: TaskReference {
                id: core.task.id,
                version: core.task.version,
            },
            requirements: Requirements {
                input_modalities: core.requirements.input_modalities,
                output_media_type: core.requirements.output_media_type,
                structured_output: core.requirements.structured_output,
                max_output_tokens: core.requirements.max_output_tokens,
            },
            generation: Generation {
                messages: [
                    GenerationMessage {
                        role: core.generation.messages[0].role,
                        content: core.generation.messages[0].content,
                    },
                    GenerationMessage {
                        role: core.generation.messages[1].role,
                        content: core.generation.messages[1].content,
                    },
                ],
                temperature: core.generation.temperature,
                seed: core.generation.seed,
                response_schema: core.generation.response_schema,
            },
        },
    };
    if encode_json(&template)?.len() > MAX_REQUEST_BYTES {
        return Err(GatewayClientError::Protocol("request exceeds byte limit"));
    }
    Ok(())
}

fn parse_inference_response(
    response: &RawResponse,
    expected_request_id: &str,
) -> Result<GatewayCompletion, GatewayClientError> {
    validate_response_headers(response)?;
    if response.status != 200 {
        return Err(parse_failure(response, expected_request_id)?);
    }
    let success: InferenceResponse = decode_response(response)?;
    if success.protocol_version != PROTOCOL_VERSION
        || success.request_id != expected_request_id
        || success.status != "completed"
        || success.output.media_type != "application/json"
    {
        return Err(GatewayClientError::Protocol("success envelope is invalid"));
    }
    let output: Value = serde_json::from_str(&success.output.content)
        .map_err(|_| GatewayClientError::Protocol("gateway output is not valid JSON"))?;
    if !output.is_object() {
        return Err(GatewayClientError::Protocol(
            "gateway output is not a JSON object",
        ));
    }
    GatewayCompletion::new(
        &success.output.media_type,
        &success.output.content,
        &success.provenance.deployment_id,
        success.provenance.task_policy_version,
    )
    .map_err(|_| GatewayClientError::Protocol("success provenance is invalid"))
}

fn require_success_status(response: &RawResponse) -> Result<(), GatewayClientError> {
    validate_response_headers(response)?;
    if response.status != 200 {
        return Err(GatewayClientError::Protocol("request was rejected"));
    }
    Ok(())
}

fn parse_acknowledgement(
    response: &RawResponse,
) -> Result<AcknowledgementResponse, GatewayClientError> {
    require_success_status(response)?;
    decode_response(response)
}

fn parse_failure(
    response: &RawResponse,
    expected_request_id: &str,
) -> Result<GatewayClientError, GatewayClientError> {
    let failure: FailureEnvelope = decode_response(response)?;
    let ownerless_auth_failure =
        failure.request_id.is_none() && failure.error.code == "unauthenticated";
    if failure.protocol_version != PROTOCOL_VERSION
        || failure.status != "failed"
        || failure.request_id.as_deref() != Some(expected_request_id) && !ownerless_auth_failure
        || !valid_failure_status(
            response.status,
            &failure.error.code,
            failure.error.retryable,
        )
        || failure.error.code.is_empty()
        || failure.error.code.len() > 64
        || !failure
            .error
            .code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        || failure.error.retry_after_seconds.is_some_and(|seconds| {
            !failure.error.retryable || seconds == 0 || seconds > MAX_RETRY_AFTER_SECONDS
        })
    {
        return Err(GatewayClientError::Protocol("error envelope is invalid"));
    }
    Ok(GatewayClientError::Rejected {
        code: failure.error.code,
        retryable: failure.error.retryable,
        retry_after_seconds: failure.error.retry_after_seconds,
    })
}

fn valid_failure_status(status: u16, code: &str, retryable: bool) -> bool {
    matches!(
        (status, code, retryable),
        (401, "unauthenticated", false)
            | (403, "forbidden", false)
            | (409, "request_expired" | "invalid_request", false)
            | (410, "unknown_request", false)
            | (422, "invalid_request" | "unsupported_task", false)
            | (429, "capacity_limited", true)
            | (500 | 502, "invalid_worker_output", false)
            | (503, "worker_unavailable", true)
            | (504, "inference_timeout", true)
    )
}

fn result_from_record(
    record: &crate::pipeline::gateway_store::GatewayRequestRecord,
) -> Result<GatewayResult, GatewayClientError> {
    let completion = record
        .completion
        .as_ref()
        .ok_or(GatewayClientError::Protocol("local completion is missing"))?;
    Ok(GatewayResult {
        content: completion.content.clone(),
        deployment_id: completion.deployment_id.clone(),
        task_policy_version: completion.task_policy_version,
    })
}

fn request_expired(
    record: &GatewayRequestRecord,
    now: DateTime<Utc>,
) -> Result<bool, GatewayClientError> {
    let expires_at = DateTime::parse_from_rfc3339(&record.request_expires_at)
        .map_err(|_| GatewayStoreError::InvalidRecord("expiry is invalid"))?
        .with_timezone(&Utc);
    Ok(expires_at <= now)
}

fn request_expiry(
    now: DateTime<Utc>,
    request_lifetime: ChronoDuration,
) -> Result<DateTime<Utc>, GatewayClientError> {
    let expires_at =
        now.checked_add_signed(request_lifetime)
            .ok_or(GatewayClientError::Configuration(
                "request expiry overflowed",
            ))?;
    if expires_at.nanosecond() == 0 {
        return Ok(expires_at);
    }
    expires_at
        .with_nanosecond(0)
        .and_then(|whole_second| whole_second.checked_add_signed(ChronoDuration::seconds(1)))
        .ok_or(GatewayClientError::Configuration(
            "request expiry overflowed",
        ))
}

fn decode_response<T: for<'de> Deserialize<'de>>(
    response: &RawResponse,
) -> Result<T, GatewayClientError> {
    validate_response_headers(response)?;
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(GatewayClientError::Protocol("response exceeds byte limit"));
    }
    serde_json::from_slice(&response.body)
        .map_err(|_| GatewayClientError::Protocol("response JSON is invalid"))
}

fn validate_response_headers(response: &RawResponse) -> Result<(), GatewayClientError> {
    let content_type = response
        .content_type
        .as_deref()
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if content_type != Some("application/json") {
        return Err(GatewayClientError::Protocol(
            "response content type is invalid",
        ));
    }
    if response
        .content_encoding
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty() && !value.eq_ignore_ascii_case("identity"))
    {
        return Err(GatewayClientError::Protocol(
            "response content encoding is unsupported",
        ));
    }
    Ok(())
}

fn encode_json<T: Serialize>(value: &T) -> Result<Vec<u8>, GatewayClientError> {
    serde_json::to_vec(value).map_err(|_| GatewayClientError::Protocol("request JSON is invalid"))
}

fn sha256_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn validate_https_origin(value: &str) -> Result<String, GatewayClientError> {
    if value.is_empty()
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(GatewayClientError::Configuration("gateway URL is invalid"));
    }
    let parsed = reqwest::Url::parse(value)
        .map_err(|_| GatewayClientError::Configuration("gateway URL is invalid"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return Err(GatewayClientError::Configuration(
            "gateway URL must be an HTTPS origin",
        ));
    }
    Ok(value.trim_end_matches('/').to_string())
}

fn validate_duration(
    value: Duration,
    message: &'static str,
) -> Result<Duration, GatewayClientError> {
    if value < Duration::from_secs(1) || value > Duration::from_secs(900) {
        return Err(GatewayClientError::Configuration(message));
    }
    Ok(value)
}

fn bounded_header(
    value: Option<&reqwest::header::HeaderValue>,
) -> Result<Option<String>, GatewayClientError> {
    value
        .map(|value| {
            let text = value
                .to_str()
                .map_err(|_| GatewayClientError::Protocol("response header is invalid"))?;
            if text.len() > 256 {
                return Err(GatewayClientError::Protocol("response header is too large"));
            }
            Ok(text.to_string())
        })
        .transpose()
}

#[derive(Clone, Copy)]
enum FilePolicy {
    Token,
    TrustRoot,
}

fn read_bounded_regular(
    path: &Path,
    max_bytes: usize,
    policy: FilePolicy,
) -> Result<Vec<u8>, GatewayClientError> {
    let link_metadata = fs::symlink_metadata(path)
        .map_err(|_| GatewayClientError::Configuration("private file is unavailable"))?;
    if link_metadata.file_type().is_symlink() {
        return Err(GatewayClientError::Configuration("private file is unsafe"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| GatewayClientError::Configuration("private file is unavailable"))?;
    validate_file_metadata(&file, max_bytes, policy)?;
    let mut bytes = Vec::new();
    file.take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| GatewayClientError::Configuration("private file could not be read"))?;
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err(GatewayClientError::Configuration(
            "private file size is invalid",
        ));
    }
    Ok(bytes)
}

fn validate_file_metadata(
    file: &File,
    max_bytes: usize,
    policy: FilePolicy,
) -> Result<(), GatewayClientError> {
    let metadata = file
        .metadata()
        .map_err(|_| GatewayClientError::Configuration("private file is unavailable"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > max_bytes as u64 {
        return Err(GatewayClientError::Configuration(
            "private file size is invalid",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let forbidden_mode = match policy {
            FilePolicy::Token => 0o077,
            FilePolicy::TrustRoot => 0o022,
        };
        let owner = metadata.uid();
        if metadata.mode() & forbidden_mode != 0
            || owner != 0 && owner != unsafe { libc::geteuid() }
        {
            return Err(GatewayClientError::Configuration(
                "private file permissions are invalid",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contracts::PipelineStage;
    use crate::pipeline::db;
    use crate::pipeline::gateway_store::reserve_request;
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::{Barrier, Mutex};
    use std::thread;
    use uuid::Uuid;

    #[derive(Debug, Clone)]
    struct Call {
        method: String,
        path: String,
        token: String,
        body: Option<Vec<u8>>,
    }

    #[derive(Default)]
    struct FakeTransport {
        responses: Mutex<VecDeque<Result<RawResponse, GatewayClientError>>>,
        calls: Mutex<Vec<Call>>,
        inspect_database: Option<PathBuf>,
        observed_submission: Mutex<Option<(String, String)>>,
        before_response: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        before_ack_response: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl GatewayTransport for FakeTransport {
        fn request(
            &self,
            method: &str,
            path: &str,
            token: &str,
            body: Option<&[u8]>,
            _timeout: Duration,
        ) -> Result<RawResponse, GatewayClientError> {
            if path == "/v1/inference" {
                if let Some(database) = &self.inspect_database {
                    let conn = Connection::open(database).unwrap();
                    let observed = conn
                        .query_row(
                            "SELECT state, gateway_request_hash FROM model_gateway_requests",
                            [],
                            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                        )
                        .unwrap();
                    *self.observed_submission.lock().unwrap() = Some(observed);
                }
                if let Some(before_response) = self.before_response.lock().unwrap().take() {
                    before_response();
                }
            }
            if path.ends_with("/ack") {
                if let Some(before_ack_response) = self.before_ack_response.lock().unwrap().take() {
                    before_ack_response();
                }
            }
            self.calls.lock().unwrap().push(Call {
                method: method.to_string(),
                path: path.to_string(),
                token: token.to_string(),
                body: body.map(ToOwned::to_owned),
            });
            self.responses.lock().unwrap().pop_front().unwrap()
        }
    }

    struct TestContext {
        root: PathBuf,
        database: PathBuf,
        token: PathBuf,
    }

    impl TestContext {
        fn new() -> (Self, Connection) {
            let root =
                std::env::temp_dir().join(format!("doc-sum-gateway-client-{}", Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            let database = root.join("app.sqlite3");
            let token = root.join("gateway.token");
            fs::write(&token, format!("{}\n", "t".repeat(MIN_TOKEN_BYTES))).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
            }
            let conn = db::init_db(&database).unwrap();
            conn.execute_batch(
                r#"
                INSERT INTO documents VALUES (
                    'document-1', 'document.pdf', 'pdf', 1, 'hash', '/document.pdf',
                    '2026-09-11T20:00:00Z'
                );
                INSERT INTO pipeline_runs VALUES (
                    'run-1', 'document-1', '"Chunked"', 1, '1.0',
                    '2026-09-11T20:00:00Z', NULL, '2026-09-11T20:00:00Z', NULL, '"Chunk"',
                    '{"total_units":0,"completed_units":0,"failed_units":0}',
                    '[]', NULL, 0, 1
                );
                INSERT INTO pipeline_run_summary_profiles VALUES (
                    'run-1', '"general"', '2026-09-11T20:00:00Z'
                );
                "#,
            )
            .unwrap();
            (
                Self {
                    root,
                    database,
                    token,
                },
                conn,
            )
        }
    }

    impl Drop for TestContext {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn key(ordinal: u32) -> GatewayRequestKey {
        GatewayRequestKey {
            owner_id: "run-1".to_string(),
            stage: PipelineStage::Analyze,
            ordinal,
        }
    }

    fn request() -> ModelRequest {
        ModelRequest {
            stage: PipelineStage::Analyze,
            ordinal: 0,
            system_prompt: "Summarize evidence.".to_string(),
            user_prompt: "Private document text.".to_string(),
            seed: 7,
            max_output_tokens: 500,
            output_format: ModelOutputFormat::JsonSchema {
                name: "summary".to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {"summary": {"type": "string"}},
                    "required": ["summary"],
                    "additionalProperties": false
                }),
            },
        }
    }

    fn raw_response(status: u16, body: Value) -> RawResponse {
        RawResponse {
            status,
            content_type: Some("application/json".to_string()),
            content_encoding: None,
            body: serde_json::to_vec(&body).unwrap(),
        }
    }

    fn response(status: u16, body: Value) -> Result<RawResponse, GatewayClientError> {
        Ok(raw_response(status, body))
    }

    fn completed(request_id: &str) -> Value {
        serde_json::json!({
            "protocol_version": 1,
            "request_id": request_id,
            "status": "completed",
            "output": {"media_type": "application/json", "content": "{\"summary\":\"done\"}"},
            "provenance": {"task_policy_version": 1, "deployment_id": "office-gateway"}
        })
    }

    fn acknowledged(request_id: &str) -> Value {
        serde_json::json!({
            "protocol_version": 1,
            "request_id": request_id,
            "status": "acknowledged",
            "disposition": "persisted"
        })
    }

    fn client(context: &TestContext, transport: Arc<dyn GatewayTransport>) -> GatewayClient {
        client_at(context, transport, Utc::now())
    }

    fn client_at(
        context: &TestContext,
        transport: Arc<dyn GatewayTransport>,
        now: DateTime<Utc>,
    ) -> GatewayClient {
        GatewayClient::with_transport(
            context.token.clone(),
            Duration::from_secs(30),
            Duration::from_secs(600),
            transport,
            Arc::new(move || now),
        )
    }

    #[test]
    fn submission_is_durable_before_transport_and_completion_is_acknowledged() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let transport = Arc::new(FakeTransport {
            inspect_database: Some(context.database.clone()),
            ..Default::default()
        });
        let reserved = reserve_request(
            &mut conn,
            &key(0),
            &sha256_hex(&encode_json(&request_core(&request()).unwrap()).unwrap()),
            now + ChronoDuration::minutes(10),
            now,
        )
        .unwrap();
        transport.responses.lock().unwrap().extend([
            response(200, completed(&reserved.request_id)),
            response(200, acknowledged(&reserved.request_id)),
        ]);

        let result = client_at(&context, transport.clone(), now)
            .execute(&mut conn, &key(0), &request(), now)
            .unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        let calls = transport.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            (calls[0].method.as_str(), calls[0].path.as_str()),
            ("POST", "/v1/inference")
        );
        assert_eq!(calls[0].token, "t".repeat(MIN_TOKEN_BYTES));
        let submitted: Value = serde_json::from_slice(calls[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(submitted["generation"]["seed"], 7);
        let observed = transport
            .observed_submission
            .lock()
            .unwrap()
            .clone()
            .unwrap();
        assert_eq!(observed.0, "submitted");
        assert_eq!(observed.1, sha256_hex(calls[0].body.as_ref().unwrap()));
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Acknowledged
        );
    }

    #[test]
    fn request_identity_must_match_ledger_key_before_reservation() {
        let (context, mut conn) = TestContext::new();
        let transport = Arc::new(FakeTransport::default());
        let client = client(&context, transport.clone());
        let mut wrong_stage = request();
        wrong_stage.stage = PipelineStage::Verify;
        let mut wrong_ordinal = request();
        wrong_ordinal.ordinal = 1;

        for (request_key, model_request) in [
            (key(0), &wrong_stage),
            (key(0), &wrong_ordinal),
            (key(1), &request()),
        ] {
            assert!(matches!(
                client.execute(&mut conn, &request_key, model_request, Utc::now()),
                Err(GatewayClientError::Protocol(
                    "request identity does not match ledger key"
                ))
            ));
            assert!(load_request(&conn, &request_key).unwrap().is_none());
        }
        assert!(transport.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn retry_reuses_body_and_lost_ack_never_repeats_inference() {
        let (context, mut conn) = TestContext::new();
        let transport = Arc::new(FakeTransport::default());
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(Err(GatewayClientError::Transport));
        let client = client(&context, transport.clone());
        assert!(matches!(
            client.execute(&mut conn, &key(0), &request(), Utc::now()),
            Err(GatewayClientError::Transport)
        ));
        let record = load_request(&conn, &key(0)).unwrap().unwrap();
        transport.responses.lock().unwrap().extend([
            response(200, completed(&record.request_id)),
            Err(GatewayClientError::Transport),
            response(200, acknowledged(&record.request_id)),
        ]);
        assert!(matches!(
            client.execute(&mut conn, &key(0), &request(), Utc::now()),
            Err(GatewayClientError::Transport)
        ));
        let result = client
            .execute(&mut conn, &key(0), &request(), Utc::now())
            .unwrap();
        assert_eq!(result.deployment_id, "office-gateway");
        let call_count = transport.calls.lock().unwrap().len();
        let replay = client
            .execute(&mut conn, &key(0), &request(), Utc::now())
            .unwrap();
        assert_eq!(replay, result);
        let calls = transport.calls.lock().unwrap();
        assert_eq!(calls.len(), call_count);
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.path == "/v1/inference")
                .count(),
            2
        );
        assert_eq!(calls[0].body, calls[1].body);
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.path.ends_with("/ack"))
                .count(),
            2
        );
    }

    #[test]
    fn expired_local_completion_returns_without_remote_acknowledgement() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let transport = Arc::new(FakeTransport::default());
        let client = client_at(&context, transport.clone(), now);
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(Err(GatewayClientError::Transport));
        assert!(matches!(
            client.execute(&mut conn, &key(0), &request(), now),
            Err(GatewayClientError::Transport)
        ));
        let record = load_request(&conn, &key(0)).unwrap().unwrap();
        transport.responses.lock().unwrap().extend([
            response(200, completed(&record.request_id)),
            Err(GatewayClientError::Transport),
        ]);
        assert!(matches!(
            client.execute(&mut conn, &key(0), &request(), now),
            Err(GatewayClientError::Transport)
        ));

        let result = client_at(
            &context,
            transport.clone(),
            now + ChronoDuration::minutes(11),
        )
        .execute(
            &mut conn,
            &key(0),
            &request(),
            now + ChronoDuration::minutes(11),
        )
        .unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        assert_eq!(transport.calls.lock().unwrap().len(), 3);
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Completed
        );
    }

    #[test]
    fn submitted_request_stops_at_expiry_without_another_transport_call() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let transport = Arc::new(FakeTransport::default());
        let client = client_at(&context, transport.clone(), now);
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(Err(GatewayClientError::Transport));
        assert!(matches!(
            client.execute(&mut conn, &key(0), &request(), now),
            Err(GatewayClientError::Transport)
        ));
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(Err(GatewayClientError::Transport));
        assert!(matches!(
            client.execute(
                &mut conn,
                &key(0),
                &request(),
                now + ChronoDuration::minutes(10) - ChronoDuration::seconds(1),
            ),
            Err(GatewayClientError::Transport)
        ));

        assert!(matches!(
            client.execute(
                &mut conn,
                &key(0),
                &request(),
                now + ChronoDuration::minutes(10),
            ),
            Err(GatewayClientError::Expired)
        ));
        assert_eq!(transport.calls.lock().unwrap().len(), 2);
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Submitted
        );
    }

    #[test]
    fn reservation_lifetime_starts_after_the_sqlite_write_lock_is_acquired() {
        let (context, conn) = TestContext::new();
        let initial = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let acquired = initial + ChronoDuration::seconds(5);
        let current_time = Arc::new(Mutex::new(initial));
        let clock_time = current_time.clone();
        let transport = Arc::new(FakeTransport::default());
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(Err(GatewayClientError::Transport));
        let client = GatewayClient::with_transport(
            context.token.clone(),
            Duration::from_secs(30),
            Duration::from_secs(1),
            transport,
            Arc::new(move || *clock_time.lock().unwrap()),
        );
        let lock = Connection::open(&context.database).unwrap();
        lock.execute_batch("BEGIN IMMEDIATE").unwrap();
        let ready = Arc::new(Barrier::new(2));
        let worker_ready = ready.clone();
        let database = context.database.clone();
        let handle = thread::spawn(move || {
            let mut worker = db::init_db(database).unwrap();
            worker_ready.wait();
            client.execute(&mut worker, &key(0), &request(), initial)
        });

        ready.wait();
        *current_time.lock().unwrap() = acquired;
        drop(lock);
        assert!(matches!(
            handle.join().unwrap(),
            Err(GatewayClientError::Transport)
        ));

        let record = load_request(&conn, &key(0)).unwrap().unwrap();
        assert_eq!(
            record.request_expires_at,
            (acquired + ChronoDuration::seconds(1))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        );
        assert_eq!(record.state, GatewayRequestState::Submitted);
    }

    #[test]
    fn state_advance_between_reserve_and_mark_uses_local_completion() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let model_request = request();
        let core = request_core(&model_request).unwrap();
        let semantic_hash = sha256_hex(&encode_json(&core).unwrap());
        let stale = reserve_request(
            &mut conn,
            &key(0),
            &semantic_hash,
            now + ChronoDuration::minutes(10),
            now,
        )
        .unwrap();
        let request_body = encode_json(&InferenceEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: &stale.request_id,
            request_expires_at: &stale.request_expires_at,
            core,
        })
        .unwrap();
        let request_hash = sha256_hex(&request_body);
        let mut concurrent = Connection::open(&context.database).unwrap();
        mark_submitted(&mut concurrent, &key(0), &request_hash, now).unwrap();
        let completion = GatewayCompletion::new(
            "application/json",
            r#"{"summary":"done"}"#,
            "office-gateway",
            1,
        )
        .unwrap();
        persist_completion(&mut concurrent, &key(0), &request_hash, &completion, now).unwrap();
        mark_acknowledged(&mut concurrent, &key(0), now).unwrap();

        let transport = Arc::new(FakeTransport::default());
        let result = client(&context, transport.clone())
            .execute_reserved(
                &mut conn,
                &key(0),
                stale,
                request_core(&model_request).unwrap(),
                now,
            )
            .unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        assert!(transport.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn expiry_crossed_after_submission_stops_before_transport() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00.250Z")
            .unwrap()
            .with_timezone(&Utc);
        let expiry = request_expiry(now, ChronoDuration::seconds(1)).unwrap();
        let times = Arc::new(Mutex::new(VecDeque::from([
            now,
            expiry - ChronoDuration::milliseconds(1),
            expiry - ChronoDuration::milliseconds(1),
            expiry,
        ])));
        let clock_times = times.clone();
        let transport = Arc::new(FakeTransport::default());
        let client = GatewayClient::with_transport(
            context.token.clone(),
            Duration::from_secs(30),
            Duration::from_secs(1),
            transport.clone(),
            Arc::new(move || clock_times.lock().unwrap().pop_front().unwrap()),
        );

        assert!(matches!(
            client.execute(&mut conn, &key(0), &request(), now),
            Err(GatewayClientError::Expired)
        ));
        assert!(transport.calls.lock().unwrap().is_empty());
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Submitted
        );
    }

    #[test]
    fn replay_failure_returns_completion_won_by_concurrent_caller() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let transport = Arc::new(FakeTransport::default());
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(Err(GatewayClientError::Transport));
        let client = client_at(&context, transport.clone(), now);
        assert!(matches!(
            client.execute(&mut conn, &key(0), &request(), now),
            Err(GatewayClientError::Transport)
        ));
        let record = load_request(&conn, &key(0)).unwrap().unwrap();
        let database = context.database.clone();
        let request_hash = record.gateway_request_hash.clone().unwrap();
        transport
            .before_response
            .lock()
            .unwrap()
            .replace(Box::new(move || {
                let mut concurrent = Connection::open(database).unwrap();
                let completion = GatewayCompletion::new(
                    "application/json",
                    r#"{"summary":"done"}"#,
                    "office-gateway",
                    1,
                )
                .unwrap();
                persist_completion(&mut concurrent, &key(0), &request_hash, &completion, now)
                    .unwrap();
                mark_acknowledged(&mut concurrent, &key(0), now).unwrap();
            }));
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(Err(GatewayClientError::Transport));

        let result = client.execute(&mut conn, &key(0), &request(), now).unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        assert_eq!(transport.calls.lock().unwrap().len(), 2);
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Acknowledged
        );
    }

    #[test]
    fn completion_persisted_at_expiry_returns_without_impossible_ack() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00.250Z")
            .unwrap()
            .with_timezone(&Utc);
        let expiry = request_expiry(now, ChronoDuration::seconds(1)).unwrap();
        let current_time = Arc::new(Mutex::new(expiry - ChronoDuration::milliseconds(1)));
        let response_time = current_time.clone();
        let transport = Arc::new(FakeTransport::default());
        let reserved = reserve_request(
            &mut conn,
            &key(0),
            &sha256_hex(&encode_json(&request_core(&request()).unwrap()).unwrap()),
            expiry,
            now,
        )
        .unwrap();
        transport
            .before_response
            .lock()
            .unwrap()
            .replace(Box::new(move || *response_time.lock().unwrap() = expiry));
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(response(200, completed(&reserved.request_id)));
        let clock_time = current_time.clone();
        let client = GatewayClient::with_transport(
            context.token.clone(),
            Duration::from_secs(30),
            Duration::from_secs(1),
            transport.clone(),
            Arc::new(move || *clock_time.lock().unwrap()),
        );

        let result = client.execute(&mut conn, &key(0), &request(), now).unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        assert_eq!(transport.calls.lock().unwrap().len(), 1);
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Completed
        );
    }

    #[test]
    fn concurrent_acknowledgement_wins_over_ack_transport_failure() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let transport = Arc::new(FakeTransport::default());
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(Err(GatewayClientError::Transport));
        let client = client_at(&context, transport.clone(), now);
        assert!(matches!(
            client.execute(&mut conn, &key(0), &request(), now),
            Err(GatewayClientError::Transport)
        ));
        let record = load_request(&conn, &key(0)).unwrap().unwrap();
        let database = context.database.clone();
        transport
            .before_ack_response
            .lock()
            .unwrap()
            .replace(Box::new(move || {
                let mut concurrent = Connection::open(database).unwrap();
                mark_acknowledged(&mut concurrent, &key(0), now).unwrap();
            }));
        transport.responses.lock().unwrap().extend([
            response(200, completed(&record.request_id)),
            Err(GatewayClientError::Transport),
        ]);

        let result = client.execute(&mut conn, &key(0), &request(), now).unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Acknowledged
        );
        assert_eq!(transport.calls.lock().unwrap().len(), 3);
    }

    #[test]
    fn acknowledgement_failure_crossing_expiry_returns_durable_completion() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let expiry = now + ChronoDuration::seconds(1);
        let current_time = Arc::new(Mutex::new(now));
        let transport = Arc::new(FakeTransport::default());
        let reserved = reserve_request(
            &mut conn,
            &key(0),
            &sha256_hex(&encode_json(&request_core(&request()).unwrap()).unwrap()),
            expiry,
            now,
        )
        .unwrap();
        let response_time = current_time.clone();
        transport
            .before_ack_response
            .lock()
            .unwrap()
            .replace(Box::new(move || *response_time.lock().unwrap() = expiry));
        transport.responses.lock().unwrap().extend([
            response(200, completed(&reserved.request_id)),
            Err(GatewayClientError::Transport),
        ]);
        let clock_time = current_time.clone();
        let client = GatewayClient::with_transport(
            context.token.clone(),
            Duration::from_secs(30),
            Duration::from_secs(1),
            transport.clone(),
            Arc::new(move || *clock_time.lock().unwrap()),
        );

        let result = client.execute(&mut conn, &key(0), &request(), now).unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Completed
        );
        assert_eq!(transport.calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn stale_entry_time_does_not_ack_an_expired_concurrent_completion() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let expiry = now + ChronoDuration::seconds(1);
        let semantic_hash = sha256_hex(&encode_json(&request_core(&request()).unwrap()).unwrap());
        reserve_request(&mut conn, &key(0), &semantic_hash, expiry, now).unwrap();
        let request_hash = "a".repeat(64);
        mark_submitted(&mut conn, &key(0), &request_hash, now).unwrap();
        let completion = GatewayCompletion::new(
            "application/json",
            r#"{"summary":"done"}"#,
            "office-gateway",
            1,
        )
        .unwrap();
        persist_completion(
            &mut conn,
            &key(0),
            &request_hash,
            &completion,
            now + ChronoDuration::milliseconds(500),
        )
        .unwrap();
        let transport = Arc::new(FakeTransport::default());
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(response(200, acknowledged("unused")));
        let client = client_at(&context, transport.clone(), expiry);

        let result = client.execute(&mut conn, &key(0), &request(), now).unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        assert!(transport.calls.lock().unwrap().is_empty());
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Completed
        );
    }

    #[test]
    fn token_loading_precedes_the_final_acknowledgement_expiry_check() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let expiry = now + ChronoDuration::seconds(1);
        let semantic_hash = sha256_hex(&encode_json(&request_core(&request()).unwrap()).unwrap());
        let reserved = reserve_request(&mut conn, &key(0), &semantic_hash, expiry, now).unwrap();
        let request_hash = "a".repeat(64);
        mark_submitted(&mut conn, &key(0), &request_hash, now).unwrap();
        let completion = GatewayCompletion::new(
            "application/json",
            r#"{"summary":"done"}"#,
            "office-gateway",
            1,
        )
        .unwrap();
        let completed =
            persist_completion(&mut conn, &key(0), &request_hash, &completion, now).unwrap();
        let times = Arc::new(Mutex::new(VecDeque::from([now, expiry])));
        let clock_times = times.clone();
        let transport = Arc::new(FakeTransport::default());
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(response(200, acknowledged(&reserved.request_id)));
        let client = GatewayClient::with_transport(
            context.token.clone(),
            Duration::from_secs(30),
            Duration::from_secs(1),
            transport.clone(),
            Arc::new(move || clock_times.lock().unwrap().pop_front().unwrap()),
        );

        let result = client
            .resolve_local_record(&mut conn, &key(0), &completed, now)
            .unwrap()
            .unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        assert!(transport.calls.lock().unwrap().is_empty());
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Completed
        );
    }

    #[test]
    fn credential_failure_after_ack_expiry_returns_durable_completion() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let expiry = now + ChronoDuration::seconds(1);
        let semantic_hash = sha256_hex(&encode_json(&request_core(&request()).unwrap()).unwrap());
        reserve_request(&mut conn, &key(0), &semantic_hash, expiry, now).unwrap();
        let request_hash = "a".repeat(64);
        mark_submitted(&mut conn, &key(0), &request_hash, now).unwrap();
        let completion = GatewayCompletion::new(
            "application/json",
            r#"{"summary":"done"}"#,
            "office-gateway",
            1,
        )
        .unwrap();
        let completed =
            persist_completion(&mut conn, &key(0), &request_hash, &completion, now).unwrap();
        let times = Arc::new(Mutex::new(VecDeque::from([now, expiry])));
        let clock_times = times.clone();
        let transport = Arc::new(FakeTransport::default());
        let client = GatewayClient::with_transport(
            context.root.join("missing-token"),
            Duration::from_secs(30),
            Duration::from_secs(1),
            transport.clone(),
            Arc::new(move || clock_times.lock().unwrap().pop_front().unwrap()),
        );

        let result = client
            .resolve_local_record(&mut conn, &key(0), &completed, now)
            .unwrap()
            .unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        assert!(transport.calls.lock().unwrap().is_empty());
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Completed
        );
    }

    #[test]
    fn credential_failure_reconciles_acknowledged_but_not_nonterminal_state() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let expiry = now + ChronoDuration::seconds(1);
        let semantic_hash = sha256_hex(&encode_json(&request_core(&request()).unwrap()).unwrap());
        reserve_request(&mut conn, &key(0), &semantic_hash, expiry, now).unwrap();
        let client = GatewayClient::with_transport(
            context.root.join("missing-token"),
            Duration::from_secs(30),
            Duration::from_secs(1),
            Arc::new(FakeTransport::default()),
            Arc::new(move || now),
        );

        assert!(matches!(
            client.read_token_or_terminal(&conn, &key(0)),
            Err(GatewayClientError::Credential)
        ));

        let request_hash = "a".repeat(64);
        mark_submitted(&mut conn, &key(0), &request_hash, now).unwrap();
        let completion = GatewayCompletion::new(
            "application/json",
            r#"{"summary":"done"}"#,
            "office-gateway",
            1,
        )
        .unwrap();
        persist_completion(&mut conn, &key(0), &request_hash, &completion, now).unwrap();
        mark_acknowledged(&mut conn, &key(0), now).unwrap();

        let result = match client.read_token_or_terminal(&conn, &key(0)).unwrap() {
            TokenOrTerminal::Terminal(result) => result,
            TokenOrTerminal::Token(_) => panic!("missing token unexpectedly loaded"),
        };
        assert_eq!(result.content, r#"{"summary":"done"}"#);
    }

    #[test]
    fn successful_acknowledgement_uses_post_response_time() {
        let (context, mut conn) = TestContext::new();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let acknowledged_at = now + ChronoDuration::seconds(1);
        let current_time = Arc::new(Mutex::new(now));
        let transport = Arc::new(FakeTransport::default());
        let reserved = reserve_request(
            &mut conn,
            &key(0),
            &sha256_hex(&encode_json(&request_core(&request()).unwrap()).unwrap()),
            now + ChronoDuration::seconds(2),
            now,
        )
        .unwrap();
        let response_time = current_time.clone();
        transport
            .before_ack_response
            .lock()
            .unwrap()
            .replace(Box::new(move || {
                *response_time.lock().unwrap() = acknowledged_at
            }));
        transport.responses.lock().unwrap().extend([
            response(200, completed(&reserved.request_id)),
            response(200, acknowledged(&reserved.request_id)),
        ]);
        let clock_time = current_time.clone();
        let client = GatewayClient::with_transport(
            context.token.clone(),
            Duration::from_secs(30),
            Duration::from_secs(2),
            transport,
            Arc::new(move || *clock_time.lock().unwrap()),
        );

        client.execute(&mut conn, &key(0), &request(), now).unwrap();

        let (completed, acknowledged): (String, String) = conn
            .query_row(
                "SELECT completed_at, acknowledged_at FROM model_gateway_requests
                 WHERE owner_id = ?1 AND stage = ?2 AND request_ordinal = ?3",
                rusqlite::params!["run-1", "Analyze", 0],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            acknowledged,
            acknowledged_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        );
        assert!(acknowledged >= completed);
    }

    #[test]
    fn accepted_ack_with_local_write_lock_returns_and_reconciles_later() {
        let (context, mut conn) = TestContext::new();
        conn.busy_timeout(Duration::ZERO).unwrap();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let transport = Arc::new(FakeTransport::default());
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(Err(GatewayClientError::Transport));
        let client = client_at(&context, transport.clone(), now);
        assert!(matches!(
            client.execute(&mut conn, &key(0), &request(), now),
            Err(GatewayClientError::Transport)
        ));
        let record = load_request(&conn, &key(0)).unwrap().unwrap();
        let held_lock = Arc::new(Mutex::new(None));
        let callback_lock = held_lock.clone();
        let database = context.database.clone();
        transport
            .before_ack_response
            .lock()
            .unwrap()
            .replace(Box::new(move || {
                let lock = Connection::open(database).unwrap();
                lock.execute_batch("BEGIN IMMEDIATE").unwrap();
                callback_lock.lock().unwrap().replace(lock);
            }));
        transport.responses.lock().unwrap().extend([
            response(200, completed(&record.request_id)),
            response(200, acknowledged(&record.request_id)),
        ]);

        let result = client.execute(&mut conn, &key(0), &request(), now).unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        held_lock.lock().unwrap().take();
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Completed
        );
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(response(200, acknowledged(&record.request_id)));
        assert_eq!(
            client.execute(&mut conn, &key(0), &request(), now).unwrap(),
            result
        );
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Acknowledged
        );
    }

    #[test]
    fn concurrent_completion_wins_over_local_persistence_lock_failure() {
        let (context, mut conn) = TestContext::new();
        conn.busy_timeout(Duration::ZERO).unwrap();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let transport = Arc::new(FakeTransport::default());
        transport
            .responses
            .lock()
            .unwrap()
            .push_back(Err(GatewayClientError::Transport));
        let client = client_at(&context, transport.clone(), now);
        assert!(matches!(
            client.execute(&mut conn, &key(0), &request(), now),
            Err(GatewayClientError::Transport)
        ));
        let record = load_request(&conn, &key(0)).unwrap().unwrap();
        let database = context.database.clone();
        let request_hash = record.gateway_request_hash.clone().unwrap();
        let held_lock = Arc::new(Mutex::new(None));
        let callback_lock = held_lock.clone();
        transport
            .before_response
            .lock()
            .unwrap()
            .replace(Box::new(move || {
                let mut concurrent = Connection::open(database).unwrap();
                let completion = GatewayCompletion::new(
                    "application/json",
                    r#"{"summary":"done"}"#,
                    "office-gateway",
                    1,
                )
                .unwrap();
                persist_completion(&mut concurrent, &key(0), &request_hash, &completion, now)
                    .unwrap();
                concurrent.execute_batch("BEGIN IMMEDIATE").unwrap();
                callback_lock.lock().unwrap().replace(concurrent);
            }));
        transport.responses.lock().unwrap().extend([
            response(200, completed(&record.request_id)),
            response(200, acknowledged(&record.request_id)),
        ]);

        let result = client.execute(&mut conn, &key(0), &request(), now).unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        held_lock.lock().unwrap().take();
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Completed
        );
        assert_eq!(transport.calls.lock().unwrap().len(), 3);
    }

    #[test]
    fn terminal_rows_remain_readable_when_reservation_is_write_blocked() {
        for acknowledged_state in [false, true] {
            let (context, mut conn) = TestContext::new();
            conn.busy_timeout(Duration::ZERO).unwrap();
            let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
                .unwrap()
                .with_timezone(&Utc);
            let expiry = now + ChronoDuration::seconds(1);
            let semantic_hash =
                sha256_hex(&encode_json(&request_core(&request()).unwrap()).unwrap());
            reserve_request(&mut conn, &key(0), &semantic_hash, expiry, now).unwrap();
            let request_hash = "a".repeat(64);
            mark_submitted(&mut conn, &key(0), &request_hash, now).unwrap();
            let completion = GatewayCompletion::new(
                "application/json",
                r#"{"summary":"done"}"#,
                "office-gateway",
                1,
            )
            .unwrap();
            persist_completion(&mut conn, &key(0), &request_hash, &completion, now).unwrap();
            if acknowledged_state {
                mark_acknowledged(&mut conn, &key(0), now).unwrap();
            }
            let lock = Connection::open(&context.database).unwrap();
            lock.execute_batch("BEGIN IMMEDIATE").unwrap();
            let transport = Arc::new(FakeTransport::default());
            let client = client_at(&context, transport.clone(), expiry);

            let result = client.execute(&mut conn, &key(0), &request(), now).unwrap();

            assert_eq!(result.content, r#"{"summary":"done"}"#);
            assert!(transport.calls.lock().unwrap().is_empty());
            assert_eq!(
                load_request(&conn, &key(0)).unwrap().unwrap().state,
                if acknowledged_state {
                    GatewayRequestState::Acknowledged
                } else {
                    GatewayRequestState::Completed
                }
            );
            drop(lock);
        }
    }

    #[test]
    fn terminal_row_wins_when_submission_transition_is_write_blocked() {
        let (context, mut conn) = TestContext::new();
        conn.busy_timeout(Duration::ZERO).unwrap();
        let now = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let expiry = now + ChronoDuration::seconds(1);
        let model_request = request();
        let core = request_core(&model_request).unwrap();
        let semantic_hash = sha256_hex(&encode_json(&core).unwrap());
        let stale = reserve_request(&mut conn, &key(0), &semantic_hash, expiry, now).unwrap();
        let payload = InferenceEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: &stale.request_id,
            request_expires_at: &stale.request_expires_at,
            core,
        };
        let request_hash = sha256_hex(&encode_json(&payload).unwrap());
        mark_submitted(&mut conn, &key(0), &request_hash, now).unwrap();
        let completion = GatewayCompletion::new(
            "application/json",
            r#"{"summary":"done"}"#,
            "office-gateway",
            1,
        )
        .unwrap();
        persist_completion(&mut conn, &key(0), &request_hash, &completion, now).unwrap();
        let lock = Connection::open(&context.database).unwrap();
        lock.execute_batch("BEGIN IMMEDIATE").unwrap();
        let transport = Arc::new(FakeTransport::default());
        let client = client_at(&context, transport.clone(), expiry);

        let result = client
            .execute_reserved(
                &mut conn,
                &key(0),
                stale,
                request_core(&model_request).unwrap(),
                now,
            )
            .unwrap();

        assert_eq!(result.content, r#"{"summary":"done"}"#);
        assert!(transport.calls.lock().unwrap().is_empty());
        assert_eq!(
            load_request(&conn, &key(0)).unwrap().unwrap().state,
            GatewayRequestState::Completed
        );
        drop(lock);
    }

    #[test]
    fn health_and_failure_envelopes_are_strict_and_bounded() {
        let (context, mut conn) = TestContext::new();
        let transport = Arc::new(FakeTransport::default());
        transport.responses.lock().unwrap().extend([
            response(200, serde_json::json!({
                "protocol_version": 1,
                "tasks": [{"id": TASK_ID, "version": 1, "status": "available", "diagnostic_code": "ready"}]
            })),
            response(429, serde_json::json!({
                "protocol_version": 1,
                "request_id": "placeholder",
                "status": "failed",
                "error": {"code": "capacity_limited", "retryable": true, "retry_after_seconds": 30}
            })),
        ]);
        let client = client(&context, transport.clone());
        assert_eq!(
            client.health().unwrap(),
            GatewayHealth {
                available: true,
                status: "available".to_string()
            }
        );
        let error = client
            .execute(&mut conn, &key(0), &request(), Utc::now())
            .unwrap_err();
        assert!(matches!(
            error,
            GatewayClientError::Protocol("error envelope is invalid")
        ));
        assert!(!error.recoverable());

        transport
            .responses
            .lock()
            .unwrap()
            .push_back(Ok(RawResponse {
                status: 200,
                content_type: Some("application/json".to_string()),
                content_encoding: None,
                body: vec![b' '; MAX_RESPONSE_BYTES + 1],
            }));
        let mut second = request();
        second.ordinal = 1;
        assert!(matches!(
            client.execute(&mut conn, &key(1), &second, Utc::now()),
            Err(GatewayClientError::Protocol("response exceeds byte limit"))
        ));
    }

    #[test]
    fn url_duration_request_and_private_file_boundaries_fail_closed() {
        assert_eq!(
            validate_https_origin("https://gateway.office.internal:8787/").unwrap(),
            "https://gateway.office.internal:8787"
        );
        for value in [
            "http://127.0.0.1:8787",
            "https://user@gateway.office.internal",
            "https://gateway.office.internal/path",
            "https://gateway.office.internal?secret=1",
        ] {
            assert!(validate_https_origin(value).is_err());
        }
        assert!(validate_duration(Duration::ZERO, "invalid").is_err());
        assert!(validate_duration(Duration::from_secs(1), "invalid").is_ok());
        assert!(validate_duration(Duration::from_secs(900), "invalid").is_ok());
        assert!(validate_duration(Duration::from_secs(901), "invalid").is_err());
        let fractional = DateTime::parse_from_rfc3339("2026-09-11T20:00:00.999Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            request_expiry(fractional, ChronoDuration::seconds(1)).unwrap(),
            DateTime::parse_from_rfc3339("2026-09-11T20:00:02Z")
                .unwrap()
                .with_timezone(&Utc)
        );
        let exact = DateTime::parse_from_rfc3339("2026-09-11T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            request_expiry(exact, ChronoDuration::seconds(1)).unwrap(),
            exact + ChronoDuration::seconds(1)
        );

        let (context, mut conn) = TestContext::new();
        fs::write(&context.token, b"private token\n").unwrap();
        let transport = Arc::new(FakeTransport::default());
        let error = client(&context, transport)
            .execute(&mut conn, &key(0), &request(), Utc::now())
            .unwrap_err();
        assert!(matches!(error, GatewayClientError::Credential));
        assert!(!error.to_string().contains("private token"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_broad_token_permissions_are_rejected() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let (context, mut conn) = TestContext::new();
        fs::set_permissions(&context.token, fs::Permissions::from_mode(0o644)).unwrap();
        let transport = Arc::new(FakeTransport::default());
        assert!(matches!(
            client(&context, transport.clone()).execute(&mut conn, &key(0), &request(), Utc::now()),
            Err(GatewayClientError::Credential)
        ));
        fs::set_permissions(&context.token, fs::Permissions::from_mode(0o600)).unwrap();
        let link = context.root.join("token-link");
        symlink(&context.token, &link).unwrap();
        let mut second_request = request();
        second_request.ordinal = 1;
        assert!(matches!(
            GatewayClient::with_transport(
                link,
                Duration::from_secs(30),
                Duration::from_secs(600),
                transport,
                Arc::new(Utc::now)
            )
            .execute(&mut conn, &key(1), &second_request, Utc::now()),
            Err(GatewayClientError::Credential)
        ));
    }

    #[test]
    fn model_request_bounds_reject_text_empty_and_oversized_shapes() {
        let mut invalid = request();
        invalid.output_format = ModelOutputFormat::Text;
        assert!(request_core(&invalid).is_err());
        invalid = request();
        invalid.system_prompt = "\0".repeat(MAX_MESSAGE_CHARS);
        invalid.user_prompt = "\0".repeat(MAX_MESSAGE_CHARS);
        assert!(preflight_request_size(&request_core(&invalid).unwrap()).is_err());
        invalid = request();
        invalid.system_prompt.clear();
        assert!(request_core(&invalid).is_err());
        invalid = request();
        invalid.seed = i64::MAX as u64;
        assert_eq!(
            request_core(&invalid).unwrap().generation.seed,
            i64::MAX as u64
        );
        let maximum_hash = sha256_hex(&encode_json(&request_core(&invalid).unwrap()).unwrap());
        invalid.seed = 0;
        assert_ne!(
            maximum_hash,
            sha256_hex(&encode_json(&request_core(&invalid).unwrap()).unwrap())
        );
        invalid.seed = (i64::MAX as u64) + 1;
        assert!(request_core(&invalid).is_err());
        invalid = request();
        invalid.max_output_tokens = MAX_OUTPUT_TOKENS;
        assert!(request_core(&invalid).is_ok());
        invalid.max_output_tokens = MAX_OUTPUT_TOKENS + 1;
        assert!(request_core(&invalid).is_err());
        invalid = request();
        invalid.user_prompt = "x".repeat(MAX_MESSAGE_CHARS + 1);
        assert!(request_core(&invalid).is_err());
        let mut schema = serde_json::Map::new();
        schema.insert(
            "padding".to_string(),
            Value::String("x".repeat(MAX_SCHEMA_BYTES)),
        );
        invalid = request();
        invalid.output_format = ModelOutputFormat::JsonSchema {
            name: "oversized".to_string(),
            schema: Value::Object(schema),
        };
        assert!(request_core(&invalid).is_err());
    }

    #[test]
    fn response_contract_rejects_both_sides_of_every_owned_boundary() {
        let request_id = "11111111-1111-4111-8111-111111111111";
        let wrong_id = "22222222-2222-4222-8222-222222222222";
        let invalid_successes = [
            serde_json::json!({
                "protocol_version": 2, "request_id": request_id, "status": "completed",
                "output": {"media_type": "application/json", "content": "{}"},
                "provenance": {"task_policy_version": 1, "deployment_id": "office-gateway"}
            }),
            serde_json::json!({
                "protocol_version": 1, "request_id": wrong_id, "status": "completed",
                "output": {"media_type": "application/json", "content": "{}"},
                "provenance": {"task_policy_version": 1, "deployment_id": "office-gateway"}
            }),
            serde_json::json!({
                "protocol_version": 1, "request_id": request_id, "status": "queued",
                "output": {"media_type": "application/json", "content": "{}"},
                "provenance": {"task_policy_version": 1, "deployment_id": "office-gateway"}
            }),
            serde_json::json!({
                "protocol_version": 1, "request_id": request_id, "status": "completed",
                "output": {"media_type": "text/plain", "content": "{}"},
                "provenance": {"task_policy_version": 1, "deployment_id": "office-gateway"}
            }),
            serde_json::json!({
                "protocol_version": 1, "request_id": request_id, "status": "completed",
                "output": {"media_type": "application/json", "content": "[]"},
                "provenance": {"task_policy_version": 1, "deployment_id": "office-gateway"}
            }),
            serde_json::json!({
                "protocol_version": 1, "request_id": request_id, "status": "completed",
                "output": {"media_type": "application/json", "content": "{}"},
                "provenance": {"task_policy_version": 0, "deployment_id": ""}
            }),
        ];
        for body in invalid_successes {
            let error = parse_inference_response(&raw_response(200, body), request_id).unwrap_err();
            assert!(matches!(error, GatewayClientError::Protocol(_)));
            assert!(!error.recoverable());
        }
        assert!(matches!(
            parse_inference_response(&raw_response(201, completed(request_id)), request_id),
            Err(GatewayClientError::Protocol(_))
        ));

        let accepted = parse_inference_response(
            &raw_response(429, serde_json::json!({
                "protocol_version": 1, "request_id": request_id, "status": "failed",
                "error": {"code": "capacity_limited", "retryable": true, "retry_after_seconds": 30}
            })),
            request_id,
        )
        .unwrap_err();
        assert!(matches!(
            accepted,
            GatewayClientError::Rejected {
                retryable: true,
                retry_after_seconds: Some(30),
                ..
            }
        ));
        for (status, code, retryable) in [
            (201, "capacity_limited", true),
            (429, "capacity_limited", false),
            (503, "capacity_limited", true),
            (503, "worker_unavailable", false),
        ] {
            let error = parse_inference_response(
                &raw_response(
                    status,
                    serde_json::json!({
                        "protocol_version": 1, "request_id": request_id, "status": "failed",
                        "error": {"code": code, "retryable": retryable}
                    }),
                ),
                request_id,
            )
            .unwrap_err();
            assert!(matches!(
                error,
                GatewayClientError::Protocol("error envelope is invalid")
            ));
            assert!(!error.recoverable());
        }

        for error in [
            serde_json::json!({
                "protocol_version": 1, "status": "failed",
                "error": {"code": "capacity_limited", "retryable": true}
            }),
            serde_json::json!({
                "protocol_version": 1, "request_id": request_id, "status": "failed",
                "error": {"code": "capacity_limited", "retryable": false, "retry_after_seconds": 1}
            }),
            serde_json::json!({
                "protocol_version": 1, "request_id": request_id, "status": "failed",
                "error": {"code": "capacity_limited", "retryable": true, "retry_after_seconds": 0}
            }),
            serde_json::json!({
                "protocol_version": 1, "request_id": request_id, "status": "failed",
                "error": {"code": "capacity_limited", "retryable": true, "retry_after_seconds": 3601}
            }),
            serde_json::json!({
                "protocol_version": 1, "request_id": request_id, "status": "failed",
                "error": {"code": "Invalid", "retryable": false}
            }),
        ] {
            assert!(matches!(
                parse_inference_response(&raw_response(429, error), request_id),
                Err(GatewayClientError::Protocol("error envelope is invalid"))
            ));
        }

        let unauthenticated = parse_inference_response(
            &raw_response(
                401,
                serde_json::json!({
                    "protocol_version": 1, "status": "failed",
                    "error": {"code": "unauthenticated", "retryable": false}
                }),
            ),
            request_id,
        )
        .unwrap_err();
        assert!(matches!(
            unauthenticated,
            GatewayClientError::Rejected {
                retryable: false,
                retry_after_seconds: None,
                ..
            }
        ));
        let retryable_unauthenticated = parse_inference_response(
            &raw_response(
                401,
                serde_json::json!({
                    "protocol_version": 1, "status": "failed",
                    "error": {"code": "unauthenticated", "retryable": true}
                }),
            ),
            request_id,
        )
        .unwrap_err();
        assert!(matches!(
            retryable_unauthenticated,
            GatewayClientError::Protocol("error envelope is invalid")
        ));
        assert!(!retryable_unauthenticated.recoverable());

        assert!(matches!(
            parse_acknowledgement(&raw_response(201, acknowledged(request_id))),
            Err(GatewayClientError::Protocol("request was rejected"))
        ));
        let mut compressed = raw_response(200, acknowledged(request_id));
        compressed.content_encoding = Some("gzip".to_string());
        assert!(matches!(
            parse_acknowledgement(&compressed),
            Err(GatewayClientError::Protocol(
                "response content encoding is unsupported"
            ))
        ));
        let mut wrong_type = raw_response(200, acknowledged(request_id));
        wrong_type.content_type = Some("text/html".to_string());
        assert!(matches!(
            parse_acknowledgement(&wrong_type),
            Err(GatewayClientError::Protocol(
                "response content type is invalid"
            ))
        ));
    }
}
