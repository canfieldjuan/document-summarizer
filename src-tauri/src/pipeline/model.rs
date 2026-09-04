use crate::pipeline::contracts::{
    ModelOutputFormat, ModelRequest, ModelRequestAttemptDiagnostic, ModelResponse, ModelRuntime,
    ModelRuntimeFailure, ModelTokenUsage, ModelTransportAttempt,
};
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::{StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434/v1/";
const DEFAULT_MODEL: &str = "qwen3-30b-a3b:latest";
const DEFAULT_TIMEOUT_SECONDS: u64 = 900;
const DEFAULT_CONNECT_TIMEOUT_SECONDS: u64 = 3;
const HEALTH_TIMEOUT_SECONDS: u64 = 5;
const MAX_TOKEN_FILE_BYTES: u64 = 16_384;
const MAX_MODEL_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_RESPONSE_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_SCHEMA_NAME_BYTES: usize = 64;

pub struct OllamaRuntime {
    client: Client,
    base_url: Url,
    model_id: String,
    api_token: Option<String>,
    format_vocabulary_unavailable: AtomicBool,
}

impl OllamaRuntime {
    pub fn from_environment() -> Result<Self, ModelRuntimeFailure> {
        let base_url = std::env::var("DOC_SUM_MODEL_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let model_id =
            std::env::var("DOC_SUM_MODEL_NAME").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let timeout = model_timeout_seconds(
            std::env::var("DOC_SUM_MODEL_TIMEOUT_SECONDS")
                .ok()
                .as_deref(),
        )?;
        let token = std::env::var_os("DOC_SUM_MODEL_API_TOKEN_FILE")
            .map(|value| read_token(Path::new(&value)))
            .transpose()?;
        Self::new(&base_url, &model_id, Duration::from_secs(timeout), token)
    }

    pub fn new(
        base_url: &str,
        model_id: &str,
        timeout: Duration,
        api_token: Option<String>,
    ) -> Result<Self, ModelRuntimeFailure> {
        let mut url = Url::parse(base_url).map_err(|_| {
            runtime_failure(
                "MODEL_CONFIG_INVALID",
                "Model base URL must be a valid URL",
                false,
            )
        })?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "::1" | "[::1]"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(runtime_failure(
                "MODEL_CONFIG_INVALID",
                "Model base URL must be an unauthenticated exact loopback HTTP URL",
                false,
            ));
        }
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        if model_id.trim().is_empty() || timeout.is_zero() {
            return Err(runtime_failure(
                "MODEL_CONFIG_INVALID",
                "Model name and timeout must be present",
                false,
            ));
        }
        let client = Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECONDS))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| {
                runtime_failure(
                    "MODEL_CLIENT_UNAVAILABLE",
                    "Local model client could not initialize",
                    true,
                )
            })?;
        Ok(Self {
            client,
            base_url: url,
            model_id: model_id.trim().to_string(),
            api_token: api_token.filter(|token| !token.trim().is_empty()),
            format_vocabulary_unavailable: AtomicBool::new(false),
        })
    }

    fn endpoint(&self, relative: &str) -> Result<Url, ModelRuntimeFailure> {
        self.base_url.join(relative).map_err(|_| {
            runtime_failure(
                "MODEL_CONFIG_INVALID",
                "Model endpoint could not be constructed",
                false,
            )
        })
    }

    fn authorize(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.api_token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    fn send_chat(
        &self,
        request: &ModelRequest,
        response_format: Option<serde_json::Value>,
    ) -> Result<Response, ModelRuntimeFailure> {
        let payload = ChatRequest {
            model: &self.model_id,
            messages: [
                ChatMessage {
                    role: "system",
                    content: &request.system_prompt,
                },
                ChatMessage {
                    role: "user",
                    content: &request.user_prompt,
                },
            ],
            temperature: 0.0,
            seed: request.seed,
            max_tokens: request.max_output_tokens,
            stream: false,
            reasoning_effort: "none",
            response_format,
        };
        self.authorize(
            self.client
                .post(self.endpoint("chat/completions")?)
                .json(&payload),
        )
        .send()
        .map_err(|_| {
            runtime_failure(
                "MODEL_RUNTIME_UNAVAILABLE",
                "Local model request failed",
                true,
            )
        })
    }

    fn send_chat_attempt(
        &self,
        request: &ModelRequest,
        response_format: Option<serde_json::Value>,
    ) -> Result<(StatusCode, Vec<u8>, Duration), (ModelRuntimeFailure, Duration)> {
        let started = Instant::now();
        let result = self
            .send_chat(request, response_format)
            .and_then(|response| {
                let status = response.status();
                read_bounded_body(response).map(|body| (status, body))
            });
        let elapsed = started.elapsed();
        result
            .map(|(status, body)| (status, body, elapsed))
            .map_err(|failure| (failure, elapsed))
    }
}

fn model_timeout_seconds(value: Option<&str>) -> Result<u64, ModelRuntimeFailure> {
    match value {
        Some(value) => value
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                runtime_failure(
                    "MODEL_CONFIG_INVALID",
                    "DOC_SUM_MODEL_TIMEOUT_SECONDS must be a positive integer",
                    false,
                )
            }),
        None => Ok(DEFAULT_TIMEOUT_SECONDS),
    }
}

impl ModelRuntime for OllamaRuntime {
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        let request_started = Instant::now();
        if request.system_prompt.trim().is_empty()
            || request.user_prompt.trim().is_empty()
            || request.max_output_tokens == 0
            || request.seed > i64::MAX as u64
        {
            return Err(with_request_attempts(
                runtime_failure(
                    "MODEL_REQUEST_INVALID",
                    "Model request prompts, output limit, and signed-range seed must be valid",
                    false,
                ),
                vec![request_attempt_diagnostic(
                    request,
                    0,
                    ModelTransportAttempt::Primary,
                    request_started.elapsed(),
                    ModelTokenUsage::default(),
                    false,
                )],
            ));
        }
        let schema_format = response_format(&request.output_format).map_err(|failure| {
            with_request_attempts(
                failure,
                vec![request_attempt_diagnostic(
                    request,
                    0,
                    ModelTransportAttempt::Primary,
                    request_started.elapsed(),
                    ModelTokenUsage::default(),
                    false,
                )],
            )
        })?;
        let schema_unavailable = self.format_vocabulary_unavailable.load(Ordering::Relaxed);
        let attempted_schema = schema_format.is_some() && !schema_unavailable;
        let output_format = if schema_format.is_some() && schema_unavailable {
            Some(json_object_response_format())
        } else {
            schema_format
        };
        let mut attempts = Vec::with_capacity(if attempted_schema { 2 } else { 1 });
        let initial_transport_attempt = if schema_unavailable && output_format.is_some() {
            ModelTransportAttempt::CachedSchemaFallback
        } else {
            ModelTransportAttempt::Primary
        };
        let (mut status, mut body, mut elapsed) = self
            .send_chat_attempt(request, output_format)
            .map_err(|(failure, elapsed)| {
                with_request_attempts(
                    failure,
                    vec![request_attempt_diagnostic(
                        request,
                        0,
                        initial_transport_attempt.clone(),
                        elapsed,
                        ModelTokenUsage::default(),
                        false,
                    )],
                )
            })?;
        if !status.is_success() && attempted_schema {
            let usage = provider_usage(&body);
            if is_format_vocabulary_failure(status, &body) {
                attempts.push(request_attempt_diagnostic(
                    request,
                    0,
                    initial_transport_attempt.clone(),
                    elapsed,
                    usage,
                    false,
                ));
                self.format_vocabulary_unavailable
                    .store(true, Ordering::Relaxed);
                eprintln!(
                    "Ollama structured-output grammar is unavailable; retrying in JSON mode with strict application validation"
                );
                (status, body, elapsed) = self
                    .send_chat_attempt(request, Some(json_object_response_format()))
                    .map_err(|(failure, elapsed)| {
                        let mut failed_attempts = attempts.clone();
                        failed_attempts.push(request_attempt_diagnostic(
                            request,
                            1,
                            ModelTransportAttempt::SchemaFallbackRetry,
                            elapsed,
                            ModelTokenUsage::default(),
                            false,
                        ));
                        with_request_attempts(failure, failed_attempts)
                    })?;
            } else {
                attempts.push(request_attempt_diagnostic(
                    request,
                    0,
                    initial_transport_attempt.clone(),
                    elapsed,
                    usage,
                    false,
                ));
                return Err(with_request_attempts(rejected_response(status), attempts));
            }
        }
        let attempt_ordinal = if attempts.is_empty() { 0 } else { 1 };
        let transport_attempt = if attempt_ordinal == 0 {
            initial_transport_attempt
        } else {
            ModelTransportAttempt::SchemaFallbackRetry
        };
        let usage = provider_usage(&body);
        if !status.is_success() {
            attempts.push(request_attempt_diagnostic(
                request,
                attempt_ordinal,
                transport_attempt,
                elapsed,
                usage,
                false,
            ));
            return Err(with_request_attempts(rejected_response(status), attempts));
        }
        let output: ChatResponse = serde_json::from_slice(&body).map_err(|_| {
            attempts.push(request_attempt_diagnostic(
                request,
                attempt_ordinal,
                transport_attempt.clone(),
                elapsed,
                usage.clone(),
                false,
            ));
            with_request_attempts(
                runtime_failure(
                    "MODEL_RESPONSE_INVALID",
                    "Local model returned an invalid response",
                    true,
                ),
                attempts.clone(),
            )
        })?;
        let text = output
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content.trim().to_string())
            .filter(|text| !text.is_empty())
            .ok_or_else(|| {
                attempts.push(request_attempt_diagnostic(
                    request,
                    attempt_ordinal,
                    transport_attempt.clone(),
                    elapsed,
                    usage.clone(),
                    false,
                ));
                with_request_attempts(
                    runtime_failure(
                        "MODEL_RESPONSE_EMPTY",
                        "Local model returned no summary text",
                        true,
                    ),
                    attempts.clone(),
                )
            })?;
        attempts.push(request_attempt_diagnostic(
            request,
            attempt_ordinal,
            transport_attempt,
            elapsed,
            usage,
            true,
        ));
        Ok(ModelResponse {
            text,
            runtime_id: self.runtime_id().to_string(),
            model_id: self.model_id.clone(),
            request_attempts: attempts,
        })
    }

    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        let response = self
            .authorize(
                self.client
                    .get(self.endpoint("models")?)
                    .timeout(Duration::from_secs(HEALTH_TIMEOUT_SECONDS)),
            )
            .send()
            .map_err(|_| {
                runtime_failure(
                    "MODEL_RUNTIME_UNAVAILABLE",
                    "Local model service is unavailable",
                    true,
                )
            })?;
        if !response.status().is_success() {
            return Err(runtime_failure(
                "MODEL_RUNTIME_UNAVAILABLE",
                format!(
                    "Local model health returned HTTP {}",
                    response.status().as_u16()
                ),
                true,
            ));
        }
        let models: ModelsResponse = decode_bounded_json(response)?;
        if !models.data.iter().any(|model| model.id == self.model_id) {
            return Err(runtime_failure(
                "MODEL_NOT_AVAILABLE",
                "Configured local model is not currently available",
                true,
            ));
        }
        Ok(())
    }

    fn runtime_id(&self) -> &str {
        "ollama-openai-loopback"
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}

fn read_token(path: &Path) -> Result<String, ModelRuntimeFailure> {
    let metadata = path.metadata().map_err(|_| {
        runtime_failure(
            "MODEL_TOKEN_UNAVAILABLE",
            "Configured local model token file is unavailable",
            false,
        )
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_TOKEN_FILE_BYTES {
        return Err(runtime_failure(
            "MODEL_TOKEN_INVALID",
            "Configured local model token file has an invalid size",
            false,
        ));
    }
    let mut token = String::new();
    File::open(path)
        .and_then(|file| {
            file.take(MAX_TOKEN_FILE_BYTES + 1)
                .read_to_string(&mut token)
        })
        .map_err(|_| {
            runtime_failure(
                "MODEL_TOKEN_UNAVAILABLE",
                "Configured local model token file could not be read",
                false,
            )
        })?;
    let token = token.trim().to_string();
    if token.is_empty() || token.len() as u64 > MAX_TOKEN_FILE_BYTES {
        return Err(runtime_failure(
            "MODEL_TOKEN_INVALID",
            "Configured local model token is invalid",
            false,
        ));
    }
    Ok(token)
}

fn decode_bounded_json<T: DeserializeOwned>(reader: impl Read) -> Result<T, ModelRuntimeFailure> {
    let body = read_bounded_body(reader)?;
    serde_json::from_slice(&body).map_err(|_| {
        runtime_failure(
            "MODEL_RESPONSE_INVALID",
            "Local model returned an invalid response",
            true,
        )
    })
}

fn read_bounded_body(reader: impl Read) -> Result<Vec<u8>, ModelRuntimeFailure> {
    let mut body = Vec::new();
    reader
        .take(MAX_MODEL_RESPONSE_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|_| {
            runtime_failure(
                "MODEL_RESPONSE_INVALID",
                "Local model response could not be read",
                true,
            )
        })?;
    if body.len() as u64 > MAX_MODEL_RESPONSE_BYTES {
        return Err(runtime_failure(
            "MODEL_RESPONSE_TOO_LARGE",
            "Local model response exceeds the supported size limit",
            true,
        ));
    }
    Ok(body)
}

fn is_format_vocabulary_failure(status: StatusCode, body: &[u8]) -> bool {
    status == StatusCode::INTERNAL_SERVER_ERROR
        && serde_json::from_slice::<serde_json::Value>(body)
            .ok()
            .and_then(|value| value["error"]["message"].as_str().map(str::to_string))
            .is_some_and(|message| message == "failed to load model vocabulary required for format")
}

fn rejected_response(status: StatusCode) -> ModelRuntimeFailure {
    runtime_failure(
        "MODEL_RUNTIME_REJECTED",
        format!("Local model returned HTTP {}", status.as_u16()),
        status.is_server_error(),
    )
}

fn runtime_failure(
    code: impl Into<String>,
    message: impl Into<String>,
    recoverable: bool,
) -> ModelRuntimeFailure {
    ModelRuntimeFailure {
        code: code.into(),
        message: message.into(),
        recoverable,
        request_attempts: Vec::new(),
    }
}

fn with_request_attempts(
    mut failure: ModelRuntimeFailure,
    request_attempts: Vec<ModelRequestAttemptDiagnostic>,
) -> ModelRuntimeFailure {
    failure.request_attempts = request_attempts;
    failure
}

fn request_attempt_diagnostic(
    request: &ModelRequest,
    attempt_ordinal: u32,
    transport_attempt: ModelTransportAttempt,
    elapsed: Duration,
    provider_usage: ModelTokenUsage,
    succeeded: bool,
) -> ModelRequestAttemptDiagnostic {
    ModelRequestAttemptDiagnostic {
        stage: request.stage.clone(),
        request_ordinal: request.ordinal,
        attempt_ordinal,
        transport_attempt,
        elapsed_milliseconds: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        configured_output_tokens: request.max_output_tokens,
        provider_usage,
        succeeded,
    }
}

fn provider_usage(body: &[u8]) -> ModelTokenUsage {
    let usage = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("usage").cloned());
    ModelTokenUsage {
        prompt_tokens: usage
            .as_ref()
            .and_then(|usage| usage.get("prompt_tokens"))
            .and_then(serde_json::Value::as_u64),
        completion_tokens: usage
            .as_ref()
            .and_then(|usage| usage.get("completion_tokens"))
            .and_then(serde_json::Value::as_u64),
        total_tokens: usage
            .as_ref()
            .and_then(|usage| usage.get("total_tokens"))
            .and_then(serde_json::Value::as_u64),
    }
}

fn response_format(
    output_format: &ModelOutputFormat,
) -> Result<Option<serde_json::Value>, ModelRuntimeFailure> {
    let ModelOutputFormat::JsonSchema { name, schema } = output_format else {
        return Ok(None);
    };
    if name.is_empty()
        || name.len() > MAX_RESPONSE_SCHEMA_NAME_BYTES
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        || !schema.is_object()
        || serde_json::to_vec(schema)
            .map_err(|_| {
                runtime_failure(
                    "MODEL_CONFIG_INVALID",
                    "Structured-output schema could not be serialized",
                    false,
                )
            })?
            .len()
            > MAX_RESPONSE_SCHEMA_BYTES
    {
        return Err(runtime_failure(
            "MODEL_CONFIG_INVALID",
            "Structured-output schema name or body is invalid",
            false,
        ));
    }
    let decoder_schema = decoder_compatible_schema(schema);
    Ok(Some(serde_json::json!({
        "type": "json_schema",
        "json_schema": {
            "name": name,
            "strict": true,
            "schema": decoder_schema,
        }
    })))
}

fn decoder_compatible_schema(schema: &serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(fields) = schema else {
        return schema.clone();
    };

    let mut projected = fields.clone();
    // vLLM does not implement `uniqueItems`, while Ollama expands large
    // `maxLength` values into grammar repetitions that it refuses to compile.
    // Stage parsers remain authoritative for uniqueness and string bounds.
    projected.remove("uniqueItems");
    projected.remove("maxLength");

    for keyword in ["properties", "patternProperties", "$defs", "definitions"] {
        if let Some(serde_json::Value::Object(named_schemas)) = fields.get(keyword) {
            projected.insert(
                keyword.to_string(),
                serde_json::Value::Object(
                    named_schemas
                        .iter()
                        .map(|(name, child)| (name.clone(), decoder_compatible_schema(child)))
                        .collect(),
                ),
            );
        }
    }
    for keyword in [
        "items",
        "contains",
        "additionalProperties",
        "propertyNames",
        "not",
        "if",
        "then",
        "else",
    ] {
        if let Some(child) = fields.get(keyword) {
            projected.insert(keyword.to_string(), decoder_compatible_schema(child));
        }
    }
    for keyword in ["prefixItems", "allOf", "anyOf", "oneOf"] {
        if let Some(serde_json::Value::Array(children)) = fields.get(keyword) {
            projected.insert(
                keyword.to_string(),
                serde_json::Value::Array(children.iter().map(decoder_compatible_schema).collect()),
            );
        }
    }

    serde_json::Value::Object(projected)
}

fn json_object_response_format() -> serde_json::Value {
    serde_json::json!({"type": "json_object"})
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    temperature: f32,
    seed: u64,
    max_tokens: u32,
    stream: bool,
    reasoning_effort: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatOutputMessage,
}

#[derive(Deserialize)]
struct ChatOutputMessage {
    content: String,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelRecord>,
}

#[derive(Deserialize)]
struct ModelRecord {
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn read_json_request(stream: &mut TcpStream) -> serde_json::Value {
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        let header_end = loop {
            let count = stream
                .read(&mut buffer)
                .expect("loopback request should be readable");
            assert!(count > 0, "loopback request ended before headers");
            request.extend_from_slice(&buffer[..count]);
            if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = std::str::from_utf8(&request[..header_end])
            .expect("loopback request headers should be UTF-8");
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.split_once(':').and_then(|(name, value)| {
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
            })
            .expect("loopback request should include Content-Length");
        while request.len() < header_end + content_length {
            let count = stream
                .read(&mut buffer)
                .expect("loopback request body should be readable");
            assert!(count > 0, "loopback request ended before its body");
            request.extend_from_slice(&buffer[..count]);
        }
        serde_json::from_slice(&request[header_end..header_end + content_length])
            .expect("loopback request body should be JSON")
    }

    fn write_json_response(stream: &mut TcpStream, status: &str, body: &serde_json::Value) {
        let body = serde_json::to_vec(body).expect("loopback response should serialize");
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .expect("loopback response headers should write");
        stream
            .write_all(&body)
            .expect("loopback response body should write");
    }

    fn schema_fallback_server() -> (String, thread::JoinHandle<Vec<serde_json::Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server should bind");
        let address = listener
            .local_addr()
            .expect("loopback address should resolve");
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for index in 0..3 {
                let (mut stream, _) = listener.accept().expect("loopback request should arrive");
                requests.push(read_json_request(&mut stream));
                if index == 0 {
                    write_json_response(
                        &mut stream,
                        "500 Internal Server Error",
                        &serde_json::json!({
                            "error": {
                                "message": "failed to load model vocabulary required for format"
                            }
                        }),
                    );
                } else {
                    let usage = (index == 1).then(|| {
                        serde_json::json!({
                            "prompt_tokens": 11,
                            "completion_tokens": 3,
                            "total_tokens": 14
                        })
                    });
                    write_json_response(
                        &mut stream,
                        "200 OK",
                        &serde_json::json!({
                            "choices": [{"message": {"content": "{\"status\":\"ok\"}"}}],
                            "usage": usage
                        }),
                    );
                }
            }
            requests
        });
        (format!("http://{address}/v1/"), handle)
    }

    fn rejected_server() -> (String, thread::JoinHandle<serde_json::Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server should bind");
        let address = listener
            .local_addr()
            .expect("loopback address should resolve");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("loopback request should arrive");
            let request = read_json_request(&mut stream);
            write_json_response(
                &mut stream,
                "503 Service Unavailable",
                &serde_json::json!({
                    "error": {"message": "fixture rejection"},
                    "usage": {
                        "prompt_tokens": 5,
                        "completion_tokens": 0,
                        "total_tokens": 5
                    }
                }),
            );
            request
        });
        (format!("http://{address}/v1/"), handle)
    }

    #[test]
    fn runtime_rejects_remote_credentials_and_invalid_limits() {
        for invalid in [
            "https://127.0.0.1:11434/v1",
            "http://example.com/v1",
            "http://localhost:11434/v1",
            "http://127.0.0.2:11434/v1",
            "http://[::2]:11434/v1",
            "http://user:pass@127.0.0.1:11434/v1",
            "http://127.0.0.1:11434/v1?token=secret",
        ] {
            let error = OllamaRuntime::new(invalid, "model", Duration::from_secs(1), None)
                .err()
                .expect("unsafe endpoint must fail");
            assert_eq!(error.code, "MODEL_CONFIG_INVALID");
        }
        assert!(OllamaRuntime::new(
            "http://127.0.0.1:11434/v1",
            "",
            Duration::from_secs(1),
            None,
        )
        .is_err());
        assert!(
            OllamaRuntime::new("http://127.0.0.1:11434/v1", "model", Duration::ZERO, None,)
                .is_err()
        );
    }

    #[test]
    fn runtime_accepts_exact_ipv4_and_ipv6_loopback() {
        assert!(OllamaRuntime::new(
            "http://127.0.0.1:11434/v1",
            "model",
            Duration::from_secs(1),
            None,
        )
        .is_ok());
        assert!(OllamaRuntime::new(
            "http://[::1]:11434/v1",
            "model",
            Duration::from_secs(1),
            None,
        )
        .is_ok());
    }

    #[test]
    fn runtime_defaults_to_the_selected_ollama_deployment() {
        assert_eq!(DEFAULT_BASE_URL, "http://127.0.0.1:11434/v1/");
        assert_eq!(DEFAULT_MODEL, "qwen3-30b-a3b:latest");
        assert_eq!(
            model_timeout_seconds(None).expect("default timeout should configure"),
            900
        );
    }

    #[test]
    fn runtime_timeout_override_accepts_positive_values_and_rejects_invalid_boundaries() {
        assert_eq!(
            model_timeout_seconds(Some("1")).expect("minimum positive timeout should configure"),
            1
        );
        let maximum = model_timeout_seconds(Some(&u64::MAX.to_string()))
            .expect("maximum timeout should parse");
        assert_eq!(maximum, u64::MAX);
        assert!(OllamaRuntime::new(
            "http://127.0.0.1:11434/v1",
            "model",
            Duration::from_secs(maximum),
            None,
        )
        .is_ok());
        for invalid in ["", "0", "-1", "1.5", "not-a-number", "18446744073709551616"] {
            let error = model_timeout_seconds(Some(invalid))
                .expect_err("non-positive or malformed timeouts must fail");
            assert_eq!(error.code, "MODEL_CONFIG_INVALID");
        }
    }

    #[test]
    fn response_decoder_accepts_valid_json_and_rejects_oversized_input() {
        let decoded: ModelsResponse =
            decode_bounded_json(Cursor::new(br#"{"data":[{"id":"fixture-model"}]}"#))
                .expect("bounded valid JSON should decode");
        assert_eq!(decoded.data[0].id, "fixture-model");

        let oversized = vec![b' '; (MAX_MODEL_RESPONSE_BYTES + 1) as usize];
        let error = match decode_bounded_json::<ModelsResponse>(Cursor::new(oversized)) {
            Ok(_) => panic!("oversized response must fail"),
            Err(error) => error,
        };
        assert_eq!(error.code, "MODEL_RESPONSE_TOO_LARGE");
    }

    #[test]
    fn structured_response_format_accepts_bounded_schema_and_rejects_invalid_boundaries() {
        assert!(response_format(&ModelOutputFormat::Text)
            .expect("plain text should be supported")
            .is_none());

        let schema = serde_json::json!({
            "type": "object",
            "properties": {"status": {"type": "string"}},
            "required": ["status"],
            "additionalProperties": false
        });
        let actual = response_format(&ModelOutputFormat::JsonSchema {
            name: "readiness_v1".to_string(),
            schema: schema.clone(),
        })
        .expect("bounded schema should be accepted")
        .expect("structured format should be present");
        assert_eq!(actual["type"], "json_schema");
        assert_eq!(actual["json_schema"]["name"], "readiness_v1");
        assert_eq!(actual["json_schema"]["strict"], true);
        assert_eq!(actual["json_schema"]["schema"], schema);

        for (name, schema) in [
            (
                "bad name".to_string(),
                serde_json::json!({"type": "object"}),
            ),
            (
                "x".repeat(MAX_RESPONSE_SCHEMA_NAME_BYTES + 1),
                serde_json::json!({"type": "object"}),
            ),
            ("valid_name".to_string(), serde_json::json!("not an object")),
        ] {
            let error = response_format(&ModelOutputFormat::JsonSchema { name, schema })
                .expect_err("invalid schema boundary must fail");
            assert_eq!(error.code, "MODEL_CONFIG_INVALID");
        }

        let oversized_schema = serde_json::json!({
            "type": "object",
            "description": "x".repeat(MAX_RESPONSE_SCHEMA_BYTES),
        });
        let error = response_format(&ModelOutputFormat::JsonSchema {
            name: "oversized_schema".to_string(),
            schema: oversized_schema,
        })
        .expect_err("an oversized schema must fail before an HTTP request");
        assert_eq!(error.code, "MODEL_CONFIG_INVALID");
    }

    #[test]
    fn structured_response_format_projects_only_unsupported_decoder_keywords() {
        let contract_schema = serde_json::json!({
            "type": "object",
            "description": "maxLength and uniqueItems remain ordinary description text",
            "properties": {
                "maxLength": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 4_000
                },
                "ids": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 5,
                    "uniqueItems": true,
                    "items": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 2_000
                    }
                },
                "choice": {
                    "anyOf": [
                        {"type": "string", "maxLength": 8, "enum": ["q1", "q2"]},
                        {"type": "null"}
                    ]
                }
            },
            "required": ["maxLength", "ids"],
            "additionalProperties": false
        });

        let actual = response_format(&ModelOutputFormat::JsonSchema {
            name: "decoder_projection_v1".to_string(),
            schema: contract_schema.clone(),
        })
        .expect("canonical schema should be accepted")
        .expect("structured format should be present");
        let projected = &actual["json_schema"]["schema"];

        assert_eq!(
            projected["description"],
            "maxLength and uniqueItems remain ordinary description text"
        );
        assert!(projected["properties"].get("maxLength").is_some());
        assert_eq!(projected["properties"]["maxLength"]["minLength"], 1);
        assert!(projected["properties"]["maxLength"]
            .get("maxLength")
            .is_none());
        assert_eq!(projected["properties"]["ids"]["minItems"], 1);
        assert_eq!(projected["properties"]["ids"]["maxItems"], 5);
        assert!(projected["properties"]["ids"].get("uniqueItems").is_none());
        assert_eq!(projected["properties"]["ids"]["items"]["minLength"], 1);
        assert!(projected["properties"]["ids"]["items"]
            .get("maxLength")
            .is_none());
        assert!(projected["properties"]["choice"]["anyOf"][0]
            .get("maxLength")
            .is_none());
        assert_eq!(
            projected["properties"]["choice"]["anyOf"][0]["enum"],
            serde_json::json!(["q1", "q2"])
        );
        assert_eq!(projected["required"], contract_schema["required"]);
        assert_eq!(projected["additionalProperties"], false);

        assert_eq!(contract_schema["properties"]["ids"]["uniqueItems"], true);
        assert_eq!(
            contract_schema["properties"]["ids"]["items"]["maxLength"],
            2_000
        );
    }

    #[test]
    fn chat_request_disables_reasoning_and_json_fallback_remains_structured() {
        assert_eq!(
            json_object_response_format(),
            serde_json::json!({"type": "json_object"})
        );
        let payload = ChatRequest {
            model: "fixture-model",
            messages: [
                ChatMessage {
                    role: "system",
                    content: "system",
                },
                ChatMessage {
                    role: "user",
                    content: "user",
                },
            ],
            temperature: 0.0,
            seed: 7_654_321,
            max_tokens: 8,
            stream: false,
            reasoning_effort: "none",
            response_format: Some(json_object_response_format()),
        };
        let serialized = serde_json::to_value(payload).expect("chat payload should serialize");
        assert_eq!(serialized["reasoning_effort"], "none");
        assert_eq!(serialized["response_format"]["type"], "json_object");
        assert_eq!(serialized["seed"], 7_654_321);
        assert_eq!(serialized["max_tokens"], 8);
    }

    #[test]
    fn runtime_rejects_a_seed_above_the_ollama_signed_integer_boundary() {
        let runtime = OllamaRuntime::new(
            "http://127.0.0.1:11434/v1/",
            "fixture-model",
            Duration::from_secs(1),
            None,
        )
        .expect("loopback runtime should configure");
        let failure = runtime
            .generate(&ModelRequest {
                stage: crate::pipeline::contracts::PipelineStage::Analyze,
                ordinal: 0,
                system_prompt: "system".to_string(),
                user_prompt: "user".to_string(),
                seed: (i64::MAX as u64) + 1,
                max_output_tokens: 1,
                output_format: ModelOutputFormat::Text,
            })
            .expect_err("a seed above Ollama's signed integer range must fail before transport");

        assert_eq!(failure.code, "MODEL_REQUEST_INVALID");
        assert_eq!(failure.request_attempts.len(), 1);
        assert!(!failure.request_attempts[0].succeeded);
    }

    #[test]
    fn exact_vocabulary_failure_retries_and_caches_json_mode() {
        let (base_url, server) = schema_fallback_server();
        let runtime = OllamaRuntime::new(&base_url, "fixture-model", Duration::from_secs(5), None)
            .expect("loopback runtime should configure");
        let request = ModelRequest {
            stage: crate::pipeline::contracts::PipelineStage::Analyze,
            ordinal: 7,
            system_prompt: "system".to_string(),
            user_prompt: "user".to_string(),
            seed: 8_675_309,
            max_output_tokens: 8,
            output_format: ModelOutputFormat::JsonSchema {
                name: "fixture_v1".to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "properties": {"status": {"type": "string"}},
                    "required": ["status"],
                    "additionalProperties": false
                }),
            },
        };

        let fallback_response = runtime
            .generate(&request)
            .expect("schema fallback request should succeed");
        assert_eq!(fallback_response.text, "{\"status\":\"ok\"}");
        assert_eq!(fallback_response.request_attempts.len(), 2);
        assert_eq!(fallback_response.request_attempts[0].stage, request.stage);
        assert_eq!(fallback_response.request_attempts[0].request_ordinal, 7);
        assert_eq!(fallback_response.request_attempts[0].attempt_ordinal, 0);
        assert_eq!(
            fallback_response.request_attempts[0].transport_attempt,
            ModelTransportAttempt::Primary
        );
        assert!(!fallback_response.request_attempts[0].succeeded);
        assert_eq!(
            fallback_response.request_attempts[0].provider_usage,
            ModelTokenUsage::default()
        );
        assert_eq!(fallback_response.request_attempts[1].attempt_ordinal, 1);
        assert_eq!(
            fallback_response.request_attempts[1].transport_attempt,
            ModelTransportAttempt::SchemaFallbackRetry
        );
        assert!(fallback_response.request_attempts[1].succeeded);
        assert_eq!(
            fallback_response.request_attempts[1].provider_usage,
            ModelTokenUsage {
                prompt_tokens: Some(11),
                completion_tokens: Some(3),
                total_tokens: Some(14),
            }
        );
        assert!(fallback_response
            .request_attempts
            .iter()
            .all(|attempt| attempt.configured_output_tokens == 8));

        let cached_response = runtime
            .generate(&request)
            .expect("cached JSON mode request should succeed");
        assert_eq!(cached_response.text, "{\"status\":\"ok\"}");
        assert_eq!(cached_response.request_attempts.len(), 1);
        assert_eq!(
            cached_response.request_attempts[0].transport_attempt,
            ModelTransportAttempt::CachedSchemaFallback
        );
        assert!(cached_response.request_attempts[0].succeeded);
        assert_eq!(
            cached_response.request_attempts[0].provider_usage,
            ModelTokenUsage::default()
        );
        let cached_diagnostic = serde_json::to_value(&cached_response.request_attempts[0])
            .expect("cached request diagnostic should serialize");
        assert!(cached_diagnostic["elapsed_milliseconds"].is_number());
        assert!(cached_diagnostic["provider_usage"]["prompt_tokens"].is_null());
        assert!(cached_diagnostic["provider_usage"]["completion_tokens"].is_null());
        assert!(cached_diagnostic["provider_usage"]["total_tokens"].is_null());
        let requests = server.join().expect("loopback server should finish");
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0]["response_format"]["type"], "json_schema");
        assert_eq!(requests[1]["response_format"]["type"], "json_object");
        assert_eq!(requests[2]["response_format"]["type"], "json_object");
        assert!(requests
            .iter()
            .all(|request| request["reasoning_effort"] == "none"));
        assert!(requests.iter().all(|request| request["seed"] == 8_675_309));
    }

    #[test]
    fn rejected_request_reports_elapsed_time_and_provider_usage_without_content() {
        let (base_url, server) = rejected_server();
        let runtime = OllamaRuntime::new(&base_url, "fixture-model", Duration::from_secs(5), None)
            .expect("loopback runtime should configure");
        let request = ModelRequest {
            stage: crate::pipeline::contracts::PipelineStage::Verify,
            ordinal: 4,
            system_prompt: "PRIVATE_SYSTEM_SENTINEL".to_string(),
            user_prompt: "PRIVATE_SOURCE_SENTINEL".to_string(),
            seed: i64::MAX as u64,
            max_output_tokens: 321,
            output_format: ModelOutputFormat::Text,
        };

        let failure = runtime
            .generate(&request)
            .expect_err("rejected request should fail");
        let attempt = failure
            .request_attempts
            .first()
            .expect("failed request should retain one attempt diagnostic");
        assert_eq!(attempt.stage, request.stage);
        assert_eq!(attempt.request_ordinal, 4);
        assert_eq!(attempt.attempt_ordinal, 0);
        assert_eq!(attempt.transport_attempt, ModelTransportAttempt::Primary);
        assert_eq!(attempt.configured_output_tokens, 321);
        assert_eq!(attempt.provider_usage.prompt_tokens, Some(5));
        assert_eq!(attempt.provider_usage.completion_tokens, Some(0));
        assert_eq!(attempt.provider_usage.total_tokens, Some(5));
        assert!(!attempt.succeeded);

        let diagnostic_json =
            serde_json::to_value(attempt).expect("request attempt diagnostic should serialize");
        assert_eq!(
            diagnostic_json,
            serde_json::json!({
                "stage": "Verify",
                "request_ordinal": 4,
                "attempt_ordinal": 0,
                "transport_attempt": "primary",
                "elapsed_milliseconds": attempt.elapsed_milliseconds,
                "configured_output_tokens": 321,
                "provider_usage": {
                    "prompt_tokens": 5,
                    "completion_tokens": 0,
                    "total_tokens": 5
                },
                "succeeded": false
            })
        );
        let encoded = diagnostic_json.to_string();
        for private_content in [
            "PRIVATE_SYSTEM_SENTINEL",
            "PRIVATE_SOURCE_SENTINEL",
            "PRIVATE_OUTPUT_SENTINEL",
            "PRIVATE_TOKEN_SENTINEL",
            "/home/private/document.pdf",
        ] {
            assert!(!encoded.contains(private_content));
        }

        let sent_request = server.join().expect("loopback server should finish");
        assert_eq!(sent_request["max_tokens"], 321);
        assert_eq!(sent_request["seed"], i64::MAX);
    }

    #[test]
    fn format_vocabulary_fallback_matches_only_the_exact_ollama_server_failure() {
        let exact =
            br#"{"error":{"message":"failed to load model vocabulary required for format"}}"#;
        assert!(is_format_vocabulary_failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            exact
        ));
        assert!(!is_format_vocabulary_failure(
            StatusCode::BAD_REQUEST,
            exact
        ));
        assert!(!is_format_vocabulary_failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            br#"{"error":{"message":"another server failure"}}"#,
        ));
        assert!(!is_format_vocabulary_failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            b"not-json",
        ));
    }
}
