use crate::pipeline::contracts::{
    ModelOutputFormat, ModelRequest, ModelResponse, ModelRuntime, ModelRuntimeFailure,
};
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::{StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434/v1/";
const DEFAULT_MODEL: &str = "qwen3-30b-a3b:latest";
const DEFAULT_TIMEOUT_SECONDS: u64 = 300;
const DEFAULT_CONNECT_TIMEOUT_SECONDS: u64 = 3;
const HEALTH_TIMEOUT_SECONDS: u64 = 5;
const DETERMINISTIC_GENERATION_SEED: u64 = 42;
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
            seed: DETERMINISTIC_GENERATION_SEED,
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
        if request.system_prompt.trim().is_empty()
            || request.user_prompt.trim().is_empty()
            || request.max_output_tokens == 0
        {
            return Err(runtime_failure(
                "MODEL_REQUEST_INVALID",
                "Model request prompts and output limit must be present",
                false,
            ));
        }
        let schema_format = response_format(&request.output_format)?;
        let schema_unavailable = self.format_vocabulary_unavailable.load(Ordering::Relaxed);
        let attempted_schema = schema_format.is_some() && !schema_unavailable;
        let output_format = if schema_format.is_some() && schema_unavailable {
            Some(json_object_response_format())
        } else {
            schema_format
        };
        let mut response = self.send_chat(request, output_format)?;
        if !response.status().is_success() && attempted_schema {
            let status = response.status();
            let body = read_bounded_body(response)?;
            if is_format_vocabulary_failure(status, &body) {
                self.format_vocabulary_unavailable
                    .store(true, Ordering::Relaxed);
                eprintln!(
                    "Ollama structured-output grammar is unavailable; retrying in JSON mode with strict application validation"
                );
                response = self.send_chat(request, Some(json_object_response_format()))?;
            } else {
                return Err(rejected_response(status));
            }
        }
        if !response.status().is_success() {
            return Err(rejected_response(response.status()));
        }
        let output: ChatResponse = decode_bounded_json(response)?;
        let text = output
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content.trim().to_string())
            .filter(|text| !text.is_empty())
            .ok_or_else(|| {
                runtime_failure(
                    "MODEL_RESPONSE_EMPTY",
                    "Local model returned no summary text",
                    true,
                )
            })?;
        Ok(ModelResponse {
            text,
            runtime_id: self.runtime_id().to_string(),
            model_id: self.model_id.clone(),
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
    Ok(Some(serde_json::json!({
        "type": "json_schema",
        "json_schema": {
            "name": name,
            "strict": true,
            "schema": schema,
        }
    })))
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
                    write_json_response(
                        &mut stream,
                        "200 OK",
                        &serde_json::json!({
                            "choices": [{"message": {"content": "{\"status\":\"ok\"}"}}]
                        }),
                    );
                }
            }
            requests
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
            300
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
            seed: DETERMINISTIC_GENERATION_SEED,
            max_tokens: 8,
            stream: false,
            reasoning_effort: "none",
            response_format: Some(json_object_response_format()),
        };
        let serialized = serde_json::to_value(payload).expect("chat payload should serialize");
        assert_eq!(serialized["reasoning_effort"], "none");
        assert_eq!(serialized["response_format"]["type"], "json_object");
        assert_eq!(serialized["seed"], DETERMINISTIC_GENERATION_SEED);
        assert_eq!(serialized["max_tokens"], 8);
    }

    #[test]
    fn exact_vocabulary_failure_retries_and_caches_json_mode() {
        let (base_url, server) = schema_fallback_server();
        let runtime = OllamaRuntime::new(&base_url, "fixture-model", Duration::from_secs(5), None)
            .expect("loopback runtime should configure");
        let request = ModelRequest {
            system_prompt: "system".to_string(),
            user_prompt: "user".to_string(),
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

        for _ in 0..2 {
            let response = runtime
                .generate(&request)
                .expect("schema fallback request should succeed");
            assert_eq!(response.text, "{\"status\":\"ok\"}");
        }
        let requests = server.join().expect("loopback server should finish");
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0]["response_format"]["type"], "json_schema");
        assert_eq!(requests[1]["response_format"]["type"], "json_object");
        assert_eq!(requests[2]["response_format"]["type"], "json_object");
        assert!(requests
            .iter()
            .all(|request| request["reasoning_effort"] == "none"));
        assert!(requests
            .iter()
            .all(|request| request["seed"] == DETERMINISTIC_GENERATION_SEED));
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
