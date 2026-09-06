use crate::pipeline::contracts::{
    ModelOutputFormat, ModelRequest, ModelRequestAttemptDiagnostic, ModelResponse, ModelRuntime,
    ModelRuntimeFailure, ModelTokenUsage, ModelTransportAttempt,
};
use crate::pipeline::qwen_tokenizer::{request_fits_context, QwenPromptTokenizer};
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::{StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434/";
const DEFAULT_MODEL: &str = "qwen3-30b-a3b:latest";
const LEGACY_DEFAULT_CONTEXT_TOKENS: u32 = 8_192;
const DEFAULT_TIMEOUT_SECONDS: u64 = 900;
const DEFAULT_CONNECT_TIMEOUT_SECONDS: u64 = 3;
const HEALTH_TIMEOUT_SECONDS: u64 = 5;
const MAX_INSTALLED_MODEL_RECORDS: usize = 256;
const MAX_TOKEN_FILE_BYTES: u64 = 16_384;
const MAX_MODEL_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MODEL_METADATA_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESPONSE_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_SCHEMA_NAME_BYTES: usize = 64;
const MIN_SUPPORTED_CONTEXT_TOKENS: u32 = 4_096;
const MAX_SUPPORTED_CONTEXT_TOKENS: u32 = 1_048_576;
const CHAT_RUNNER_KEEP_ALIVE: &str = "30s";
pub(super) const MAX_DECODER_STRING_LENGTH: u64 = 1_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QwenTokenizerFamily {
    Qwen3,
    Qwen35,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModelDescriptor {
    pub name: String,
    pub digest: String,
    pub size_bytes: u64,
    pub architecture: Option<String>,
    pub tokenizer_family: Option<QwenTokenizerFamily>,
    pub parameter_size: Option<String>,
    pub quantization_level: Option<String>,
    pub maximum_context_tokens: Option<u32>,
    pub disabled_reason: Option<String>,
}

pub struct OllamaRuntime {
    client: Client,
    base_url: Url,
    model_id: String,
    expected_digest: Option<String>,
    expected_tokenizer_family: Option<QwenTokenizerFamily>,
    context_tokens: u32,
    api_token: Option<String>,
    format_vocabulary_unavailable: AtomicBool,
    require_token_admission: bool,
    tokenizer: Mutex<Option<Arc<QwenPromptTokenizer>>>,
}

impl OllamaRuntime {
    pub fn from_environment() -> Result<Self, ModelRuntimeFailure> {
        let model_id =
            std::env::var("DOC_SUM_MODEL_NAME").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        Self::from_environment_profile(&model_id, None, None, LEGACY_DEFAULT_CONTEXT_TOKENS)
    }

    pub fn from_environment_profile(
        model_id: &str,
        expected_digest: Option<&str>,
        expected_tokenizer_family: Option<QwenTokenizerFamily>,
        context_tokens: u32,
    ) -> Result<Self, ModelRuntimeFailure> {
        let base_url = std::env::var("DOC_SUM_MODEL_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let timeout = model_timeout_seconds(
            std::env::var("DOC_SUM_MODEL_TIMEOUT_SECONDS")
                .ok()
                .as_deref(),
        )?;
        let token = std::env::var_os("DOC_SUM_MODEL_API_TOKEN_FILE")
            .map(|value| read_token(Path::new(&value)))
            .transpose()?;
        Self::new_with_profile(
            &base_url,
            model_id,
            expected_digest,
            expected_tokenizer_family,
            context_tokens,
            Duration::from_secs(timeout),
            token,
        )
    }

    pub fn discovery_from_environment() -> Result<Self, ModelRuntimeFailure> {
        let base_url = std::env::var("DOC_SUM_MODEL_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let token = std::env::var_os("DOC_SUM_MODEL_API_TOKEN_FILE")
            .map(|value| read_token(Path::new(&value)))
            .transpose()?;
        Self::new_internal(
            &base_url,
            DEFAULT_MODEL,
            None,
            None,
            LEGACY_DEFAULT_CONTEXT_TOKENS,
            Duration::from_secs(HEALTH_TIMEOUT_SECONDS),
            token,
            false,
        )
    }

    pub fn new(
        base_url: &str,
        model_id: &str,
        timeout: Duration,
        api_token: Option<String>,
    ) -> Result<Self, ModelRuntimeFailure> {
        Self::new_with_context(
            base_url,
            model_id,
            LEGACY_DEFAULT_CONTEXT_TOKENS,
            timeout,
            api_token,
        )
    }

    pub fn new_with_context(
        base_url: &str,
        model_id: &str,
        context_tokens: u32,
        timeout: Duration,
        api_token: Option<String>,
    ) -> Result<Self, ModelRuntimeFailure> {
        Self::new_internal(
            base_url,
            model_id,
            None,
            None,
            context_tokens,
            timeout,
            api_token,
            false,
        )
    }

    pub fn new_with_profile(
        base_url: &str,
        model_id: &str,
        expected_digest: Option<&str>,
        expected_tokenizer_family: Option<QwenTokenizerFamily>,
        context_tokens: u32,
        timeout: Duration,
        api_token: Option<String>,
    ) -> Result<Self, ModelRuntimeFailure> {
        Self::new_internal(
            base_url,
            model_id,
            expected_digest,
            expected_tokenizer_family,
            context_tokens,
            timeout,
            api_token,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_internal(
        base_url: &str,
        model_id: &str,
        expected_digest: Option<&str>,
        expected_tokenizer_family: Option<QwenTokenizerFamily>,
        context_tokens: u32,
        timeout: Duration,
        api_token: Option<String>,
        require_token_admission: bool,
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
        if url.path() == "/v1/" {
            url.set_path("/");
        }
        if model_id.trim().is_empty() || context_tokens == 0 || timeout.is_zero() {
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
            expected_digest: expected_digest
                .map(str::trim)
                .filter(|digest| !digest.is_empty())
                .map(str::to_string),
            expected_tokenizer_family,
            context_tokens,
            api_token: api_token.filter(|token| !token.trim().is_empty()),
            format_vocabulary_unavailable: AtomicBool::new(false),
            require_token_admission,
            tokenizer: Mutex::new(None),
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

    fn installed_model_records(&self) -> Result<Vec<ModelRecord>, ModelRuntimeFailure> {
        self.installed_model_records_with_timeout(Duration::from_secs(HEALTH_TIMEOUT_SECONDS))
    }

    fn installed_model_records_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<Vec<ModelRecord>, ModelRuntimeFailure> {
        let response = self
            .authorize(self.client.get(self.endpoint("api/tags")?).timeout(timeout))
            .send()
            .map_err(|_| {
                runtime_failure(
                    "MODEL_RUNTIME_UNAVAILABLE",
                    "Local model service is unavailable",
                    true,
                )
            })?;
        if !response.status().is_success() {
            return Err(rejected_response(response.status()));
        }
        Ok(decode_bounded_json::<ModelsResponse>(response)?.models)
    }

    pub fn installed_models(&self) -> Result<Vec<InstalledModelDescriptor>, ModelRuntimeFailure> {
        self.installed_models_with_deadline(Duration::from_secs(HEALTH_TIMEOUT_SECONDS))
    }

    fn installed_models_with_deadline(
        &self,
        timeout: Duration,
    ) -> Result<Vec<InstalledModelDescriptor>, ModelRuntimeFailure> {
        let deadline = Instant::now() + timeout;
        let records = self.installed_model_records_with_timeout(discovery_remaining(deadline)?)?;
        validate_model_record_count(records.len())?;
        let mut descriptors = Vec::with_capacity(records.len());
        for model in records {
            let unavailable = unavailable_model_descriptor(model.clone());
            match self.describe_model(model, discovery_remaining(deadline)?) {
                Ok(descriptor) => descriptors.push(descriptor),
                Err(error) if metadata_failure_is_catalog_wide(&error) => return Err(error),
                Err(_) => descriptors.push(unavailable),
            }
        }
        discovery_remaining(deadline)?;
        Ok(descriptors)
    }

    fn describe_model(
        &self,
        model: ModelRecord,
        timeout: Duration,
    ) -> Result<InstalledModelDescriptor, ModelRuntimeFailure> {
        let response = self
            .authorize(
                self.client
                    .post(self.endpoint("api/show")?)
                    .timeout(timeout)
                    .json(&ShowRequest {
                        model: &model.name,
                        verbose: false,
                    }),
            )
            .send()
            .map_err(|_| {
                runtime_failure(
                    "MODEL_RUNTIME_UNAVAILABLE",
                    "Installed model metadata could not be read",
                    true,
                )
            })?;
        if !response.status().is_success() {
            return Err(rejected_response(response.status()));
        }
        let shown: ShowResponse = decode_bounded_json(response)?;
        Ok(model_descriptor(model, shown))
    }

    fn pinned_tokenizer(&self) -> Result<Arc<QwenPromptTokenizer>, ModelRuntimeFailure> {
        let mut cached = self.tokenizer.lock().map_err(|_| {
            runtime_failure(
                "MODEL_TOKENIZER_UNAVAILABLE",
                "Pinned tokenizer cache is unavailable",
                false,
            )
        })?;
        if let Some(tokenizer) = cached.as_ref() {
            return Ok(Arc::clone(tokenizer));
        }
        let response = self
            .authorize(
                self.client
                    .post(self.endpoint("api/show")?)
                    .timeout(Duration::from_secs(HEALTH_TIMEOUT_SECONDS))
                    .json(&ShowRequest {
                        model: &self.model_id,
                        verbose: true,
                    }),
            )
            .send()
            .map_err(|_| {
                runtime_failure(
                    "MODEL_TOKENIZER_UNAVAILABLE",
                    "Pinned tokenizer metadata could not be read",
                    true,
                )
            })?;
        if !response.status().is_success() {
            return Err(rejected_response(response.status()));
        }
        let shown: ShowResponse = decode_bounded_json_with_limit(
            response,
            MAX_MODEL_METADATA_RESPONSE_BYTES,
            "Local model metadata exceeds the supported size limit",
        )?;
        let architecture = shown
            .model_info
            .get("general.architecture")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                runtime_failure(
                    "MODEL_TOKENIZER_INVALID",
                    "Tokenizer architecture metadata is missing",
                    false,
                )
            })?;
        let family = qwen_tokenizer_family_for_architecture(architecture).ok_or_else(|| {
            runtime_failure(
                "MODEL_TOKENIZER_INVALID",
                "Tokenizer architecture is not an admitted Qwen family",
                false,
            )
        })?;
        if self
            .expected_tokenizer_family
            .is_some_and(|expected| expected != family)
        {
            return Err(runtime_failure(
                "MODEL_TOKENIZER_INVALID",
                "Tokenizer family no longer matches the qualified profile",
                false,
            ));
        }
        let tokenizer = Arc::new(
            QwenPromptTokenizer::from_model_info(family, &shown.model_info)
                .map_err(|message| runtime_failure("MODEL_TOKENIZER_INVALID", message, false))?,
        );
        *cached = Some(Arc::clone(&tokenizer));
        Ok(tokenizer)
    }

    fn admit_request(
        &self,
        request: &ModelRequest,
        format: Option<serde_json::Value>,
    ) -> Result<(), ModelRuntimeFailure> {
        if !self.require_token_admission {
            return Ok(());
        }
        let payload = serde_json::to_string(&self.chat_payload(request, format)).map_err(|_| {
            runtime_failure(
                "MODEL_REQUEST_INVALID",
                "Native model request could not be serialized for token admission",
                false,
            )
        })?;
        let tokenizer = self.pinned_tokenizer()?;
        let input_tokens = tokenizer
            .count(&payload)
            .map_err(|message| runtime_failure("MODEL_TOKENIZER_INVALID", message, false))?;
        if !request_fits_context(input_tokens, request.max_output_tokens, self.context_tokens) {
            return Err(runtime_failure(
                "MODEL_CONTEXT_EXCEEDED",
                format!(
                    "Native request needs {input_tokens} input tokens plus {} output tokens and framing reserve, exceeding the qualified {}-token context",
                    request.max_output_tokens, self.context_tokens
                ),
                false,
            ));
        }
        Ok(())
    }

    fn chat_payload<'a>(
        &'a self,
        request: &'a ModelRequest,
        format: Option<serde_json::Value>,
    ) -> ChatRequest<'a> {
        ChatRequest {
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
            stream: false,
            think: false,
            keep_alive: CHAT_RUNNER_KEEP_ALIVE,
            format,
            options: ChatOptions {
                num_ctx: self.context_tokens,
                num_predict: request.max_output_tokens,
                temperature: 0.0,
                seed: request.seed,
            },
        }
    }

    fn send_chat(
        &self,
        request: &ModelRequest,
        format: Option<serde_json::Value>,
    ) -> Result<Response, ModelRuntimeFailure> {
        let payload = self.chat_payload(request, format);
        self.authorize(self.client.post(self.endpoint("api/chat")?).json(&payload))
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
        format: Option<serde_json::Value>,
    ) -> Result<(StatusCode, Vec<u8>, Duration), (ModelRuntimeFailure, Duration)> {
        let started = Instant::now();
        let result = self.send_chat(request, format).and_then(|response| {
            let status = response.status();
            read_bounded_body(response).map(|body| (status, body))
        });
        let elapsed = started.elapsed();
        result
            .map(|(status, body)| (status, body, elapsed))
            .map_err(|failure| (failure, elapsed))
    }

    fn verify_execution_digest(&self) -> Result<(), ModelRuntimeFailure> {
        let Some(expected) = self.expected_digest.as_ref() else {
            return Ok(());
        };
        let response = self
            .authorize(
                self.client
                    .get(self.endpoint("api/ps")?)
                    .timeout(Duration::from_secs(HEALTH_TIMEOUT_SECONDS)),
            )
            .send()
            .map_err(|_| {
                runtime_failure(
                    "MODEL_RUNTIME_UNAVAILABLE",
                    "Local model execution provenance is unavailable",
                    true,
                )
            })?;
        if !response.status().is_success() {
            return Err(rejected_response(response.status()));
        }
        let matching: Vec<_> = decode_bounded_json::<ModelsResponse>(response)?
            .models
            .into_iter()
            .filter(|model| model.name == self.model_id)
            .collect();
        if matching.len() != 1 {
            return Err(runtime_failure(
                "MODEL_EXECUTION_UNVERIFIED",
                "Local model execution could not be bound to one running model record",
                true,
            ));
        }
        if &matching[0].digest != expected {
            return Err(runtime_failure(
                "MODEL_PROFILE_STALE",
                "Local model execution used a digest outside the qualified profile",
                true,
            ));
        }
        Ok(())
    }
}

fn discovery_remaining(deadline: Instant) -> Result<Duration, ModelRuntimeFailure> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| {
            runtime_failure(
                "MODEL_DISCOVERY_TIMEOUT",
                "Installed model discovery exceeded its aggregate deadline",
                true,
            )
        })
}

fn validate_model_record_count(count: usize) -> Result<(), ModelRuntimeFailure> {
    if count > MAX_INSTALLED_MODEL_RECORDS {
        return Err(runtime_failure(
            "MODEL_CATALOG_TOO_LARGE",
            "Installed model catalog exceeds the supported record limit",
            false,
        ));
    }
    Ok(())
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
        self.admit_request(request, output_format.clone())
            .map_err(|failure| {
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
        if let Err(failure) = self.verify_execution_digest() {
            attempts.push(request_attempt_diagnostic(
                request,
                attempt_ordinal,
                transport_attempt.clone(),
                elapsed,
                usage.clone(),
                false,
            ));
            return Err(with_request_attempts(failure, attempts));
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
        let text = Some(output.message.content.trim().to_string())
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
        let descriptor = self
            .installed_model_records()?
            .into_iter()
            .find(|model| model.name == self.model_id)
            .ok_or_else(|| {
                runtime_failure(
                    "MODEL_NOT_AVAILABLE",
                    "Configured local model is not currently available",
                    true,
                )
            })?;
        let descriptor =
            self.describe_model(descriptor, Duration::from_secs(HEALTH_TIMEOUT_SECONDS))?;
        if self
            .expected_digest
            .as_ref()
            .is_some_and(|expected| expected != &descriptor.digest)
        {
            return Err(runtime_failure(
                "MODEL_PROFILE_STALE",
                "Configured local model digest no longer matches its qualified profile",
                false,
            ));
        }
        let maximum_context = descriptor.maximum_context_tokens.ok_or_else(|| {
            runtime_failure(
                "MODEL_CONTEXT_UNAVAILABLE",
                "Configured local model does not report a supported context",
                false,
            )
        })?;
        if descriptor.tokenizer_family.is_none() || self.context_tokens > maximum_context {
            return Err(runtime_failure(
                "MODEL_CONTEXT_INVALID",
                "Configured context exceeds this supported Qwen model profile",
                false,
            ));
        }
        if self.require_token_admission {
            self.pinned_tokenizer()?;
        }
        Ok(())
    }

    fn runtime_id(&self) -> &str {
        "ollama-native-loopback"
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn context_tokens(&self, _stage: crate::pipeline::contracts::PipelineStage) -> u32 {
        self.context_tokens
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
    decode_bounded_json_with_limit(
        reader,
        MAX_MODEL_RESPONSE_BYTES,
        "Local model response exceeds the supported size limit",
    )
}

fn decode_bounded_json_with_limit<T: DeserializeOwned>(
    reader: impl Read,
    limit: u64,
    too_large_message: &'static str,
) -> Result<T, ModelRuntimeFailure> {
    let body = read_bounded_body_with_limit(reader, limit, too_large_message)?;
    serde_json::from_slice(&body).map_err(|_| {
        runtime_failure(
            "MODEL_RESPONSE_INVALID",
            "Local model returned an invalid response",
            true,
        )
    })
}

fn read_bounded_body(reader: impl Read) -> Result<Vec<u8>, ModelRuntimeFailure> {
    read_bounded_body_with_limit(
        reader,
        MAX_MODEL_RESPONSE_BYTES,
        "Local model response exceeds the supported size limit",
    )
}

fn read_bounded_body_with_limit(
    reader: impl Read,
    limit: u64,
    too_large_message: &'static str,
) -> Result<Vec<u8>, ModelRuntimeFailure> {
    let mut body = Vec::new();
    reader.take(limit + 1).read_to_end(&mut body).map_err(|_| {
        runtime_failure(
            "MODEL_RESPONSE_INVALID",
            "Local model response could not be read",
            true,
        )
    })?;
    if body.len() as u64 > limit {
        return Err(runtime_failure(
            "MODEL_RESPONSE_TOO_LARGE",
            too_large_message,
            true,
        ));
    }
    Ok(body)
}

fn is_format_vocabulary_failure(status: StatusCode, body: &[u8]) -> bool {
    status == StatusCode::INTERNAL_SERVER_ERROR
        && serde_json::from_slice::<serde_json::Value>(body)
            .ok()
            .and_then(|value| {
                value["error"]
                    .as_str()
                    .or_else(|| value["error"]["message"].as_str())
                    .map(str::to_string)
            })
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
    let response = serde_json::from_slice::<serde_json::Value>(body).ok();
    let prompt_tokens = response
        .as_ref()
        .and_then(|value| value.get("prompt_eval_count"))
        .and_then(serde_json::Value::as_u64);
    let completion_tokens = response
        .as_ref()
        .and_then(|value| value.get("eval_count"))
        .and_then(serde_json::Value::as_u64);
    ModelTokenUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens
            .zip(completion_tokens)
            .map(|(prompt, completion)| prompt.saturating_add(completion)),
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
    Ok(Some(decoder_compatible_schema(schema)))
}

fn decoder_compatible_schema(schema: &serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(fields) = schema else {
        return schema.clone();
    };

    let mut projected = fields.clone();
    // Neither the production Ollama/llama.cpp grammar converter nor vLLM
    // enforces `uniqueItems`; it also compares objects, not quote-ID fields.
    // Single-item page analysis enforces uniqueness structurally in both paths.
    // Retain bounded decoder headroom, not the paraphrase acceptance limit:
    // a grammar ceiling can close a string before its sentence is complete.
    // Larger Ollama grammar repetitions can fail compilation.
    // Stage parsers remain authoritative even when the decoder has a bound.
    projected.remove("uniqueItems");
    if fields
        .get("maxLength")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|maximum| maximum > MAX_DECODER_STRING_LENGTH)
    {
        projected.remove("maxLength");
    }

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
    serde_json::json!("json")
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    stream: bool,
    think: bool,
    keep_alive: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<serde_json::Value>,
    options: ChatOptions,
}

#[derive(Serialize)]
struct ChatOptions {
    num_ctx: u32,
    num_predict: u32,
    temperature: f32,
    seed: u64,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatOutputMessage,
}

#[derive(Deserialize)]
struct ChatOutputMessage {
    content: String,
}

#[derive(Deserialize)]
struct ModelsResponse {
    models: Vec<ModelRecord>,
}

#[derive(Clone, Deserialize)]
struct ModelRecord {
    name: String,
    #[serde(default)]
    digest: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    details: ModelDetails,
}

#[derive(Clone, Default, Deserialize)]
struct ModelDetails {
    #[serde(default)]
    family: String,
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

#[derive(Serialize)]
struct ShowRequest<'a> {
    model: &'a str,
    verbose: bool,
}

#[derive(Deserialize)]
struct ShowResponse {
    #[serde(default)]
    model_info: serde_json::Map<String, serde_json::Value>,
}

fn model_descriptor(model: ModelRecord, shown: ShowResponse) -> InstalledModelDescriptor {
    let architecture = shown
        .model_info
        .get("general.architecture")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| (!model.details.family.is_empty()).then(|| model.details.family.clone()));
    let tokenizer_family = architecture
        .as_deref()
        .and_then(qwen_tokenizer_family_for_architecture);
    let maximum_context_tokens = architecture
        .as_deref()
        .and_then(|architecture| {
            shown
                .model_info
                .get(&format!("{architecture}.context_length"))
        })
        .and_then(serde_json::Value::as_u64)
        .and_then(|context| u32::try_from(context).ok())
        .filter(|context| {
            (MIN_SUPPORTED_CONTEXT_TOKENS..=MAX_SUPPORTED_CONTEXT_TOKENS).contains(context)
        });
    let disabled_reason = if tokenizer_family.is_none() {
        Some("Installed model is not a supported Qwen architecture".to_string())
    } else if maximum_context_tokens.is_none() {
        Some("Installed model does not report a valid maximum context".to_string())
    } else if model.digest.is_empty() {
        Some("Installed model does not report an immutable digest".to_string())
    } else {
        None
    };
    InstalledModelDescriptor {
        name: model.name,
        digest: model.digest,
        size_bytes: model.size,
        architecture,
        tokenizer_family,
        parameter_size: model.details.parameter_size,
        quantization_level: model.details.quantization_level,
        maximum_context_tokens,
        disabled_reason,
    }
}

fn unavailable_model_descriptor(model: ModelRecord) -> InstalledModelDescriptor {
    let architecture = (!model.details.family.is_empty()).then(|| model.details.family.clone());
    let tokenizer_family = architecture
        .as_deref()
        .and_then(qwen_tokenizer_family_for_architecture);
    InstalledModelDescriptor {
        name: model.name,
        digest: model.digest,
        size_bytes: model.size,
        architecture,
        tokenizer_family,
        parameter_size: model.details.parameter_size,
        quantization_level: model.details.quantization_level,
        maximum_context_tokens: None,
        disabled_reason: Some("Installed model metadata could not be read".to_string()),
    }
}

fn metadata_failure_is_catalog_wide(error: &ModelRuntimeFailure) -> bool {
    error.code == "MODEL_RUNTIME_UNAVAILABLE"
        || (error.code == "MODEL_RUNTIME_REJECTED"
            && (error.message.ends_with("HTTP 401") || error.message.ends_with("HTTP 403")))
}

fn qwen_tokenizer_family_for_architecture(architecture: &str) -> Option<QwenTokenizerFamily> {
    match architecture {
        "qwen3" | "qwen3moe" => Some(QwenTokenizerFamily::Qwen3),
        "qwen35" | "qwen35moe" => Some(QwenTokenizerFamily::Qwen35),
        _ => None,
    }
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
                            "error": "failed to load model vocabulary required for format"
                        }),
                    );
                } else {
                    let usage = (index == 1).then(|| {
                        serde_json::json!({
                            "prompt_eval_count": 11,
                            "eval_count": 3
                        })
                    });
                    write_json_response(
                        &mut stream,
                        "200 OK",
                        &serde_json::json!({
                            "message": {"role": "assistant", "content": "{\"status\":\"ok\"}"},
                            "prompt_eval_count": usage.as_ref().and_then(|value| value["prompt_eval_count"].as_u64()),
                            "eval_count": usage.as_ref().and_then(|value| value["eval_count"].as_u64())
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
                    "error": "fixture rejection",
                    "prompt_eval_count": 5,
                    "eval_count": 0
                }),
            );
            request
        });
        (format!("http://{address}/v1/"), handle)
    }

    fn execution_digest_server(
        running_digests: &'static [&'static str],
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server should bind");
        let address = listener
            .local_addr()
            .expect("loopback address should resolve");
        let handle = thread::spawn(move || {
            let (mut chat, _) = listener.accept().expect("chat request should arrive");
            let chat_request = read_json_request(&mut chat);
            assert_eq!(chat_request["keep_alive"], CHAT_RUNNER_KEEP_ALIVE);
            write_json_response(
                &mut chat,
                "200 OK",
                &serde_json::json!({
                    "message": {"role": "assistant", "content": "bounded response"},
                    "prompt_eval_count": 10,
                    "eval_count": 2
                }),
            );
            let (mut running, _) = listener.accept().expect("digest check should arrive");
            let headers = read_headers(&mut running);
            assert!(headers.starts_with("GET /api/ps HTTP/1.1"));
            let models: Vec<_> = running_digests
                .iter()
                .map(|digest| {
                    serde_json::json!({
                        "name": "fixture-model",
                        "digest": digest,
                        "size": 12345,
                        "details": {"family": "qwen3"}
                    })
                })
                .collect();
            write_json_response(
                &mut running,
                "200 OK",
                &serde_json::json!({"models": models}),
            );
        });
        (format!("http://{address}/"), handle)
    }

    fn read_headers(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let count = stream
                .read(&mut buffer)
                .expect("loopback headers should be readable");
            assert!(count > 0, "loopback request ended before headers");
            request.extend_from_slice(&buffer[..count]);
            if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                return std::str::from_utf8(&request[..position + 4])
                    .expect("loopback headers should be UTF-8")
                    .to_string();
            }
        }
    }

    fn runtime_with_fixture_tokenizer(base_url: &str) -> OllamaRuntime {
        let runtime = OllamaRuntime::new_internal(
            base_url,
            "fixture-model",
            None,
            Some(QwenTokenizerFamily::Qwen3),
            8_192,
            Duration::from_secs(5),
            None,
            true,
        )
        .expect("fixture runtime should configure");
        *runtime
            .tokenizer
            .lock()
            .expect("fixture tokenizer cache should lock") =
            Some(Arc::new(QwenPromptTokenizer::fixture_single_token()));
        runtime
    }

    fn digest_guard_runtime(base_url: &str) -> OllamaRuntime {
        OllamaRuntime::new_internal(
            base_url,
            "fixture-model",
            Some("qualified-digest"),
            Some(QwenTokenizerFamily::Qwen3),
            8_192,
            Duration::from_secs(5),
            None,
            false,
        )
        .expect("digest guard runtime should configure")
    }

    fn digest_guard_request() -> ModelRequest {
        ModelRequest {
            stage: crate::pipeline::contracts::PipelineStage::Analyze,
            ordinal: 0,
            system_prompt: "system".to_string(),
            user_prompt: "user".to_string(),
            seed: 42,
            max_output_tokens: 64,
            output_format: ModelOutputFormat::Text,
        }
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
    fn request_output_is_accepted_only_when_the_execution_digest_is_proven() {
        let (matching_url, matching_server) = execution_digest_server(&["qualified-digest"]);
        let matching = digest_guard_runtime(&matching_url)
            .generate(&digest_guard_request())
            .expect("an unchanged digest should admit the response");
        matching_server
            .join()
            .expect("matching digest server should finish");
        assert_eq!(matching.text, "bounded response");
        assert!(matching.request_attempts[0].succeeded);

        let (changed_url, changed_server) = execution_digest_server(&["repointed-digest"]);
        let changed = digest_guard_runtime(&changed_url)
            .generate(&digest_guard_request())
            .expect_err("a repointed tag must reject the generated response");
        changed_server
            .join()
            .expect("changed digest server should finish");
        assert_eq!(changed.code, "MODEL_PROFILE_STALE");
        assert!(changed.recoverable);
        assert_eq!(changed.request_attempts.len(), 1);
        assert!(!changed.request_attempts[0].succeeded);

        for running_digests in [&[][..], &["qualified-digest", "qualified-digest"][..]] {
            let (unproven_url, unproven_server) = execution_digest_server(running_digests);
            let unproven = digest_guard_runtime(&unproven_url)
                .generate(&digest_guard_request())
                .expect_err("missing or ambiguous execution records must reject the response");
            unproven_server
                .join()
                .expect("unproven digest server should finish");
            assert_eq!(unproven.code, "MODEL_EXECUTION_UNVERIFIED");
            assert!(unproven.recoverable);
            assert!(!unproven.request_attempts[0].succeeded);
        }
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
    fn model_discovery_uses_the_configured_authorization_boundary() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server should bind");
        let address = listener
            .local_addr()
            .expect("loopback address should resolve");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("discovery request should arrive");
            let headers = read_headers(&mut stream);
            write_json_response(&mut stream, "200 OK", &serde_json::json!({"models": []}));
            headers
        });
        let runtime = OllamaRuntime::new(
            &format!("http://{address}/"),
            "fixture-model",
            Duration::from_secs(5),
            Some("private-discovery-token".to_string()),
        )
        .expect("discovery runtime should configure");
        assert!(runtime
            .installed_model_records()
            .expect("authenticated discovery should succeed")
            .is_empty());
        let headers = server.join().expect("discovery server should finish");
        assert!(
            headers
                .lines()
                .any(|line| line
                    .eq_ignore_ascii_case("authorization: Bearer private-discovery-token"))
        );
    }

    #[test]
    fn unreadable_model_metadata_stays_visible_but_unqualified() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server should bind");
        let address = listener
            .local_addr()
            .expect("loopback address should resolve");
        let server = thread::spawn(move || {
            let (mut tags, _) = listener.accept().expect("tags request should arrive");
            let _ = read_headers(&mut tags);
            write_json_response(
                &mut tags,
                "200 OK",
                &serde_json::json!({
                    "models": [{
                        "name": "broken-qwen:latest",
                        "digest": "immutable-digest",
                        "size": 12345,
                        "details": {
                            "family": "qwen35",
                            "parameter_size": "9B",
                            "quantization_level": "Q4_K_M"
                        }
                    }]
                }),
            );
            let (mut show, _) = listener.accept().expect("show request should arrive");
            let _ = read_json_request(&mut show);
            write_json_response(
                &mut show,
                "500 Internal Server Error",
                &serde_json::json!({"error": "fixture metadata failure"}),
            );
        });
        let runtime = OllamaRuntime::new(
            &format!("http://{address}/"),
            "fixture-model",
            Duration::from_secs(5),
            None,
        )
        .expect("discovery runtime should configure");
        let models = runtime
            .installed_models()
            .expect("one bad model must not hide the catalog");
        server.join().expect("discovery server should finish");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "broken-qwen:latest");
        assert_eq!(
            models[0].tokenizer_family,
            Some(QwenTokenizerFamily::Qwen35)
        );
        assert_eq!(models[0].maximum_context_tokens, None);
        assert_eq!(
            models[0].disabled_reason.as_deref(),
            Some("Installed model metadata could not be read")
        );
    }

    #[test]
    fn discovery_authentication_failure_is_not_downgraded_to_disabled_metadata() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server should bind");
        let address = listener
            .local_addr()
            .expect("loopback address should resolve");
        let server = thread::spawn(move || {
            let (mut tags, _) = listener.accept().expect("tags request should arrive");
            let _ = read_headers(&mut tags);
            write_json_response(
                &mut tags,
                "200 OK",
                &serde_json::json!({
                    "models": [{
                        "name": "protected-qwen:latest",
                        "digest": "immutable-digest",
                        "size": 12345,
                        "details": {"family": "qwen35"}
                    }]
                }),
            );
            let (mut show, _) = listener.accept().expect("show request should arrive");
            let _ = read_json_request(&mut show);
            write_json_response(
                &mut show,
                "401 Unauthorized",
                &serde_json::json!({"error": "fixture authentication failure"}),
            );
        });
        let runtime = OllamaRuntime::new(
            &format!("http://{address}/"),
            "fixture-model",
            Duration::from_secs(5),
            None,
        )
        .expect("discovery runtime should configure");
        let error = runtime
            .installed_models()
            .expect_err("authentication failures must remain fatal");
        server.join().expect("discovery server should finish");
        assert_eq!(error.code, "MODEL_RUNTIME_REJECTED");
    }

    #[test]
    fn discovery_record_limit_rejects_max_plus_one_before_metadata_probes() {
        assert!(validate_model_record_count(MAX_INSTALLED_MODEL_RECORDS).is_ok());
        assert_eq!(
            validate_model_record_count(MAX_INSTALLED_MODEL_RECORDS + 1)
                .expect_err("max plus one records must fail")
                .code,
            "MODEL_CATALOG_TOO_LARGE"
        );

        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server should bind");
        let address = listener
            .local_addr()
            .expect("loopback address should resolve");
        let server = thread::spawn(move || {
            let (mut tags, _) = listener.accept().expect("tags request should arrive");
            let _ = read_headers(&mut tags);
            let models: Vec<_> = (0..=MAX_INSTALLED_MODEL_RECORDS)
                .map(|ordinal| {
                    serde_json::json!({
                        "name": format!("fixture-{ordinal}"),
                        "digest": format!("digest-{ordinal}"),
                        "size": 1,
                        "details": {"family": "qwen3"}
                    })
                })
                .collect();
            write_json_response(&mut tags, "200 OK", &serde_json::json!({"models": models}));
            listener
                .set_nonblocking(true)
                .expect("listener should become nonblocking");
            thread::sleep(Duration::from_millis(50));
            let error = listener
                .accept()
                .expect_err("record overflow must fail before any show request");
            assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        });
        let runtime = OllamaRuntime::new(
            &format!("http://{address}/"),
            "fixture-model",
            Duration::from_secs(5),
            None,
        )
        .expect("discovery runtime should configure");
        let error = runtime
            .installed_models()
            .expect_err("oversized record catalog must fail");
        server.join().expect("record limit server should finish");
        assert_eq!(error.code, "MODEL_CATALOG_TOO_LARGE");
    }

    #[test]
    fn discovery_metadata_probes_share_one_aggregate_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback server should bind");
        let address = listener
            .local_addr()
            .expect("loopback address should resolve");
        let server = thread::spawn(move || {
            let (mut tags, _) = listener.accept().expect("tags request should arrive");
            let _ = read_headers(&mut tags);
            write_json_response(
                &mut tags,
                "200 OK",
                &serde_json::json!({
                    "models": [{
                        "name": "slow-qwen:latest",
                        "digest": "immutable-digest",
                        "size": 12345,
                        "details": {"family": "qwen3"}
                    }]
                }),
            );
            let (_show, _) = listener.accept().expect("show request should arrive");
            thread::sleep(Duration::from_millis(100));
        });
        let runtime = OllamaRuntime::new(
            &format!("http://{address}/"),
            "fixture-model",
            Duration::from_secs(5),
            None,
        )
        .expect("discovery runtime should configure");
        let started = Instant::now();
        let error = runtime
            .installed_models_with_deadline(Duration::from_millis(20))
            .expect_err("one slow probe must exhaust the aggregate deadline");
        assert!(started.elapsed() < Duration::from_secs(1));
        server.join().expect("deadline server should finish");
        assert_eq!(error.code, "MODEL_RUNTIME_UNAVAILABLE");
        assert!(error.recoverable);
    }

    #[test]
    fn runtime_defaults_to_the_selected_ollama_deployment() {
        assert_eq!(DEFAULT_BASE_URL, "http://127.0.0.1:11434/");
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
            decode_bounded_json(Cursor::new(br#"{"models":[{"name":"fixture-model"}]}"#))
                .expect("bounded valid JSON should decode");
        assert_eq!(decoded.models[0].name, "fixture-model");

        let oversized = vec![b' '; (MAX_MODEL_RESPONSE_BYTES + 1) as usize];
        let error = match decode_bounded_json::<ModelsResponse>(Cursor::new(oversized)) {
            Ok(_) => panic!("oversized response must fail"),
            Err(error) => error,
        };
        assert_eq!(error.code, "MODEL_RESPONSE_TOO_LARGE");
    }

    #[test]
    fn discovery_requires_qwen_architecture_digest_and_valid_architecture_context() {
        let qwen = ModelRecord {
            name: "qualified:latest".to_string(),
            digest: "immutable-digest".to_string(),
            size: 12_345,
            details: ModelDetails {
                family: "qwen3moe".to_string(),
                parameter_size: Some("30B-A3B".to_string()),
                quantization_level: Some("Q4_K_S".to_string()),
            },
        };
        let shown = ShowResponse {
            model_info: serde_json::json!({
                "general.architecture": "qwen3moe",
                "qwen3moe.context_length": 262_144
            })
            .as_object()
            .expect("fixture metadata should be an object")
            .clone(),
        };
        let descriptor = model_descriptor(qwen, shown);
        assert_eq!(
            descriptor.tokenizer_family,
            Some(QwenTokenizerFamily::Qwen3)
        );
        assert_eq!(descriptor.maximum_context_tokens, Some(262_144));
        assert!(descriptor.disabled_reason.is_none());

        for (architecture, context, digest) in [
            ("llama", 32_768_u64, "digest"),
            ("qwen35", 0, "digest"),
            ("qwen35", 3_999, "digest"),
            ("qwen35", 1_048_577, "digest"),
            ("qwen35", 32_768, ""),
        ] {
            let descriptor = model_descriptor(
                ModelRecord {
                    name: "invalid:latest".to_string(),
                    digest: digest.to_string(),
                    size: 1,
                    details: ModelDetails::default(),
                },
                ShowResponse {
                    model_info: serde_json::json!({
                        "general.architecture": architecture,
                        format!("{architecture}.context_length"): context
                    })
                    .as_object()
                    .expect("fixture metadata should be an object")
                    .clone(),
                },
            );
            assert!(descriptor.disabled_reason.is_some());
        }
    }

    #[test]
    fn exact_token_admission_rejects_before_transport_and_uses_the_admitted_limits() {
        let blocked_listener =
            TcpListener::bind("127.0.0.1:0").expect("blocked listener should bind");
        blocked_listener
            .set_nonblocking(true)
            .expect("blocked listener should become nonblocking");
        let blocked_url = format!(
            "http://{}/",
            blocked_listener
                .local_addr()
                .expect("blocked address should resolve")
        );
        let blocked = runtime_with_fixture_tokenizer(&blocked_url);
        let mut oversized = ModelRequest {
            stage: crate::pipeline::contracts::PipelineStage::Analyze,
            ordinal: 0,
            system_prompt: "system".to_string(),
            user_prompt: "user".to_string(),
            seed: 42,
            max_output_tokens: 7_000,
            output_format: ModelOutputFormat::Text,
        };
        let counted_payload = serde_json::to_string(&blocked.chat_payload(&oversized, None))
            .expect("counted payload should serialize");
        let input_tokens = blocked
            .tokenizer
            .lock()
            .expect("fixture tokenizer cache should lock")
            .as_ref()
            .expect("fixture tokenizer should be cached")
            .count(&counted_payload)
            .expect("fixture payload should tokenize");
        let maximum_output = 8_192
            - crate::pipeline::qwen_tokenizer::TOKENIZER_FRAMING_RESERVE_TOKENS
            - input_tokens;
        oversized.max_output_tokens = maximum_output + 1;
        let failure = blocked
            .generate(&oversized)
            .expect_err("one token above context must fail before transport");
        assert_eq!(failure.code, "MODEL_CONTEXT_EXCEEDED");
        assert!(matches!(
            blocked_listener.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ));

        let admitted_listener =
            TcpListener::bind("127.0.0.1:0").expect("admitted listener should bind");
        let admitted_url = format!(
            "http://{}/",
            admitted_listener
                .local_addr()
                .expect("admitted address should resolve")
        );
        let server = thread::spawn(move || {
            let (mut stream, _) = admitted_listener
                .accept()
                .expect("admitted request should arrive");
            let request = read_json_request(&mut stream);
            write_json_response(
                &mut stream,
                "200 OK",
                &serde_json::json!({"message":{"role":"assistant","content":"ok"}}),
            );
            request
        });
        let admitted = runtime_with_fixture_tokenizer(&admitted_url);
        let mut fitting = oversized;
        fitting.max_output_tokens = maximum_output;
        admitted
            .generate(&fitting)
            .expect("adjacent fitting request should reach transport");
        let sent = server.join().expect("admitted server should finish");
        assert_eq!(sent["options"]["num_ctx"], 8_192);
        assert_eq!(sent["options"]["num_predict"], maximum_output);
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
        assert_eq!(actual, schema);

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
        let projected = &actual;

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
        assert_eq!(
            projected["properties"]["choice"]["anyOf"][0]["maxLength"],
            8
        );
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
    fn decoder_projection_preserves_small_string_bounds_and_strips_large_ones() {
        for maximum in [
            0, 191, 192, 193, 767, 768, 769, 1_535, 1_536, 1_537, 2_000, 4_000,
        ] {
            let canonical = serde_json::json!({
                "type": "object",
                "properties": {
                    "evidence": {
                        "type": "array", "minItems": 1, "maxItems": 9,
                        "uniqueItems": true,
                        "items": {
                            "type": "object",
                            "properties": {
                                "claim_text": {"type": "string", "maxLength": maximum},
                                "quote_id": {"type": "string", "enum": ["q1", "q2"]}
                            },
                            "required": ["quote_id", "claim_text"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["evidence"], "additionalProperties": false
            });
            let original = canonical.clone();
            let format = response_format(&ModelOutputFormat::JsonSchema {
                name: "analysis_bound_probe".to_string(),
                schema: canonical.clone(),
            })
            .expect("bounded schema should project")
            .expect("schema transport should remain enabled");
            let projected = &format;
            let actual = &projected["properties"]["evidence"]["items"]["properties"]["claim_text"];
            if maximum <= MAX_DECODER_STRING_LENGTH {
                assert_eq!(actual["maxLength"], maximum, "small bound must survive");
            } else {
                assert!(
                    actual.get("maxLength").is_none(),
                    "large bound must be omitted"
                );
            }
            assert_eq!(projected["properties"]["evidence"]["maxItems"], 9);
            assert!(projected["properties"]["evidence"]
                .get("uniqueItems")
                .is_none());
            assert_eq!(canonical, original, "projection must not mutate its input");
        }
    }

    #[test]
    fn chat_request_disables_thinking_and_sets_native_context_and_output_options() {
        assert_eq!(json_object_response_format(), serde_json::json!("json"));
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
            stream: false,
            think: false,
            keep_alive: CHAT_RUNNER_KEEP_ALIVE,
            format: Some(json_object_response_format()),
            options: ChatOptions {
                num_ctx: 16_384,
                num_predict: 8,
                temperature: 0.0,
                seed: 7_654_321,
            },
        };
        let serialized = serde_json::to_value(payload).expect("chat payload should serialize");
        assert_eq!(serialized["think"], false);
        assert_eq!(serialized["keep_alive"], CHAT_RUNNER_KEEP_ALIVE);
        assert_eq!(serialized["format"], "json");
        assert_eq!(serialized["options"]["num_ctx"], 16_384);
        assert_eq!(serialized["options"]["seed"], 7_654_321);
        assert_eq!(serialized["options"]["num_predict"], 8);
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
        assert!(requests[0]["format"].is_object());
        assert_eq!(requests[1]["format"], "json");
        assert_eq!(requests[2]["format"], "json");
        assert!(requests.iter().all(|request| request["think"] == false));
        assert!(requests
            .iter()
            .all(|request| request["options"]["seed"] == 8_675_309));
        assert!(requests
            .iter()
            .all(|request| request["options"]["num_ctx"] == 8_192));
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
        assert_eq!(sent_request["options"]["num_predict"], 321);
        assert_eq!(sent_request["options"]["num_ctx"], 8_192);
        assert_eq!(sent_request["options"]["seed"], i64::MAX);
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
