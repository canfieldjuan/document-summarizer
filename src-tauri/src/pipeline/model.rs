use crate::pipeline::contracts::{ModelRequest, ModelResponse, ModelRuntime, ModelRuntimeFailure};
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::Url;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:1234/v1/";
const DEFAULT_MODEL: &str = "qwen3.5-4b";
const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const MAX_TOKEN_FILE_BYTES: u64 = 16_384;
const MAX_MODEL_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

pub struct OpenAiCompatibleRuntime {
    client: Client,
    base_url: Url,
    model_id: String,
    api_token: Option<String>,
}

impl OpenAiCompatibleRuntime {
    pub fn from_environment() -> Result<Self, ModelRuntimeFailure> {
        let base_url = std::env::var("DOC_SUM_MODEL_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let model_id =
            std::env::var("DOC_SUM_MODEL_NAME").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let timeout = match std::env::var("DOC_SUM_MODEL_TIMEOUT_SECONDS") {
            Ok(value) => value
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    runtime_failure(
                        "MODEL_CONFIG_INVALID",
                        "DOC_SUM_MODEL_TIMEOUT_SECONDS must be a positive integer",
                        false,
                    )
                })?,
            Err(_) => DEFAULT_TIMEOUT_SECONDS,
        };
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
}

impl ModelRuntime for OpenAiCompatibleRuntime {
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
        };
        let response = self
            .authorize(
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
            })?;
        if !response.status().is_success() {
            return Err(runtime_failure(
                "MODEL_RUNTIME_REJECTED",
                format!("Local model returned HTTP {}", response.status().as_u16()),
                response.status().is_server_error(),
            ));
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
            .authorize(self.client.get(self.endpoint("models")?))
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
        "openai-compatible-loopback"
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
    serde_json::from_slice(&body).map_err(|_| {
        runtime_failure(
            "MODEL_RESPONSE_INVALID",
            "Local model returned an invalid response",
            true,
        )
    })
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

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    temperature: f32,
    max_tokens: u32,
    stream: bool,
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
            "https://127.0.0.1:1234/v1",
            "http://example.com/v1",
            "http://localhost:1234/v1",
            "http://127.0.0.2:1234/v1",
            "http://[::2]:1234/v1",
            "http://user:pass@127.0.0.1:1234/v1",
            "http://127.0.0.1:1234/v1?token=secret",
        ] {
            let error =
                OpenAiCompatibleRuntime::new(invalid, "model", Duration::from_secs(1), None)
                    .err()
                    .expect("unsafe endpoint must fail");
            assert_eq!(error.code, "MODEL_CONFIG_INVALID");
        }
        assert!(OpenAiCompatibleRuntime::new(
            "http://127.0.0.1:1234/v1",
            "",
            Duration::from_secs(1),
            None,
        )
        .is_err());
        assert!(OpenAiCompatibleRuntime::new(
            "http://127.0.0.1:1234/v1",
            "model",
            Duration::ZERO,
            None,
        )
        .is_err());
    }

    #[test]
    fn runtime_accepts_exact_ipv4_and_ipv6_loopback() {
        assert!(OpenAiCompatibleRuntime::new(
            "http://127.0.0.1:1234/v1",
            "model",
            Duration::from_secs(1),
            None,
        )
        .is_ok());
        assert!(OpenAiCompatibleRuntime::new(
            "http://[::1]:1234/v1",
            "model",
            Duration::from_secs(1),
            None,
        )
        .is_ok());
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
}
