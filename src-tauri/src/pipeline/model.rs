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
            max_tokens: request.max_output_tokens,
            stream: false,
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
        let mut output_format = response_format(&request.output_format)?;
        if self.format_vocabulary_unavailable.load(Ordering::Relaxed) {
            output_format = None;
        }
        let attempted_structured_output = output_format.is_some();
        let mut response = self.send_chat(request, output_format)?;
        if !response.status().is_success() && attempted_structured_output {
            let status = response.status();
            let body = read_bounded_body(response)?;
            if is_format_vocabulary_failure(status, &body) {
                self.format_vocabulary_unavailable
                    .store(true, Ordering::Relaxed);
                eprintln!(
                    "Ollama structured-output grammar is unavailable; retrying with strict application validation"
                );
                response = self.send_chat(request, None)?;
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

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    temperature: f32,
    max_tokens: u32,
    stream: bool,
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
    use std::io::Cursor;

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
