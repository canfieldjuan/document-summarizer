//! Native Qwen framing and wire representation shared by native transports.
//! No process, credentials, socket, HTTP client or lifecycle owner lives here.
use super::contracts::{ModelRequest, ModelRuntimeFailure, ModelTokenUsage};
use super::model::response_format;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;

pub(super) const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
pub(super) const MAX_TOKENIZE_RESPONSE_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FramingPolicy {
    Legacy,
    // The fixed shared encoder uses this internally. Route admission owns transport access.
    Qualified(QualifiedFraming),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QualifiedFraming {
    profile_identity: [u8; 32],
    thinking: ThinkingPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Open thinking remains covered by the historical qualified fixtures.
enum ThinkingPolicy {
    Enabled,
    Disabled,
}

impl FramingPolicy {
    pub(super) fn update_cache_key(&self, hasher: &mut Sha256) {
        if let Self::Qualified(binding) = self {
            hasher.update(b"\0qualified-native-framing-v1\0");
            hasher.update(binding.profile_identity);
            hasher.update([match binding.thinking {
                ThinkingPolicy::Enabled => 0,
                ThinkingPolicy::Disabled => 1,
            }]);
        }
    }

    #[cfg(test)]
    pub(super) fn qualified_fixture(profile_identity: [u8; 32], disable_thinking: bool) -> Self {
        Self::Qualified(QualifiedFraming {
            profile_identity,
            thinking: if disable_thinking {
                ThinkingPolicy::Disabled
            } else {
                ThinkingPolicy::Enabled
            },
        })
    }
}

#[derive(Serialize)]
struct MessageDelimiter {
    role: &'static str,
    delimiter: &'static str,
}

pub(super) struct PromptFraming {
    policy: FramingPolicy,
    system_open: Vec<u32>,
    system_close_user_open: Vec<u32>,
    user_close_assistant_open: Vec<u32>,
}

impl PromptFraming {
    const MESSAGE_END: &'static str = "<|im_end|>";
    // Ordered qualified metadata from the pinned reference owner. Content is never scanned.
    const MESSAGE_DELIMITERS: [MessageDelimiter; 5] = [
        MessageDelimiter {
            role: "assistant",
            delimiter: "<|im_start|>assistant",
        },
        MessageDelimiter {
            role: "tool",
            delimiter: "<|im_start|>user\n<tool_response>",
        },
        MessageDelimiter {
            role: "tool",
            delimiter: "<|im_start|>tool_response",
        },
        MessageDelimiter {
            role: "user",
            delimiter: "<|im_start|>user",
        },
        MessageDelimiter {
            role: "system",
            delimiter: "<|im_start|>system",
        },
    ];

    pub(super) fn load(
        policy: FramingPolicy,
        mut tokenize: impl FnMut(&str, bool) -> Result<Vec<u32>, ModelRuntimeFailure>,
    ) -> Result<Self, ModelRuntimeFailure> {
        let system_prefix = format!("{}\n", Self::MESSAGE_DELIMITERS[4].delimiter);
        let user_prefix = format!(
            "{}\n{}\n",
            Self::MESSAGE_END,
            Self::MESSAGE_DELIMITERS[3].delimiter
        );
        let mut assistant_prefix = format!(
            "{}\n{}\n",
            Self::MESSAGE_END,
            Self::MESSAGE_DELIMITERS[0].delimiter
        );
        if matches!(&policy, FramingPolicy::Qualified(binding) if binding.thinking == ThinkingPolicy::Disabled)
        {
            assistant_prefix.push_str("<think>\n\n</think>\n\n");
        }
        let system_open = tokenize(&system_prefix, true)?;
        let system_close_user_open = tokenize(&user_prefix, true)?;
        let user_close_assistant_open = tokenize(&assistant_prefix, true)?;
        if system_open.is_empty()
            || system_close_user_open.is_empty()
            || user_close_assistant_open.is_empty()
        {
            return Err(failure(
                "MODEL_EXECUTION_UNVERIFIED",
                "Qualified Qwen prompt framing could not be tokenized",
                false,
            ));
        }
        Ok(Self {
            policy,
            system_open,
            system_close_user_open,
            user_close_assistant_open,
        })
    }

    #[cfg(test)]
    pub(super) fn empty_fixture() -> Self {
        Self {
            policy: FramingPolicy::Legacy,
            system_open: Vec::new(),
            system_close_user_open: Vec::new(),
            user_close_assistant_open: Vec::new(),
        }
    }

    pub(super) fn compose(
        &self,
        system: Vec<u32>,
        user: Vec<u32>,
    ) -> Result<Vec<u32>, ModelRuntimeFailure> {
        let capacity = checked_prompt_capacity([
            self.system_open.len(),
            system.len(),
            self.system_close_user_open.len(),
            user.len(),
            self.user_close_assistant_open.len(),
        ])?;
        let mut tokens = Vec::with_capacity(capacity);
        tokens.extend_from_slice(&self.system_open);
        tokens.extend(system);
        tokens.extend_from_slice(&self.system_close_user_open);
        tokens.extend(user);
        tokens.extend_from_slice(&self.user_close_assistant_open);
        Ok(tokens)
    }

    pub(super) fn completion_request<'a>(
        &self,
        request: &ModelRequest,
        prompt: &'a [u32],
        schema: Option<serde_json::Value>,
    ) -> CompletionRequest<'a> {
        self.wire_request(
            prompt,
            request.max_output_tokens,
            request.seed,
            schema,
            CompletionRoute::Standalone,
        )
    }

    fn wire_request<'a, S>(
        &self,
        prompt: &'a [u32],
        n_predict: u32,
        seed: u64,
        json_schema: Option<S>,
        route: CompletionRoute,
    ) -> CompletionRequest<'a, S> {
        CompletionRequest {
            prompt,
            n_predict,
            temperature: 0.0,
            seed,
            stop: [Self::MESSAGE_END],
            cache_prompt: matches!(route, CompletionRoute::Standalone),
            message_delimiters: match self.policy {
                FramingPolicy::Legacy => None,
                FramingPolicy::Qualified(_) => Some(&Self::MESSAGE_DELIMITERS),
            },
            json_schema,
        }
    }
}

enum CompletionRoute {
    Standalone,
    Shared,
}

// Same bytes as runtime/v1/profile.json at connect-contracts c35f607 and the
// host's vendor/connect-contracts/profile.json at 73a0758. Used for framing identity.
pub(super) const SHARED_PROFILE: &[u8] =
    include_bytes!("../../vendor/connect-contracts/profile.json");

/// Builds V1 shared completion bytes before handing them to the admitted transport.
pub struct SharedRequestEncoder(PromptFraming);

impl SharedRequestEncoder {
    /// Tokenize the control fragments once using the shared profile's closed thinking frame.
    pub fn load(
        tokenize_control: impl FnMut(&str, bool) -> Result<Vec<u32>, ModelRuntimeFailure>,
    ) -> Result<Self, ModelRuntimeFailure> {
        PromptFraming::load(
            FramingPolicy::Qualified(QualifiedFraming {
                profile_identity: Sha256::digest(SHARED_PROFILE).into(),
                thinking: ThinkingPolicy::Disabled,
            }),
            tokenize_control,
        )
        .map(Self)
    }

    /// Serialize the app's projected decoder schema directly, retaining its field order.
    pub fn completion_bytes(
        &self,
        request: &ModelRequest,
        tokenize: impl FnMut(&str, bool) -> Result<Vec<u32>, ModelRuntimeFailure>,
    ) -> Result<Vec<u8>, ModelRuntimeFailure> {
        self.encode(
            &request.system_prompt,
            &request.user_prompt,
            request.max_output_tokens,
            request.seed,
            response_format(&request.output_format)?,
            tokenize,
        )
    }

    /// Build DS-HOST-CANARY-1 with its own fixed inputs and no answer-forcing schema.
    pub fn canary_bytes(
        &self,
        tokenize: impl FnMut(&str, bool) -> Result<Vec<u32>, ModelRuntimeFailure>,
    ) -> Result<Vec<u8>, ModelRuntimeFailure> {
        self.encode(
            "Answer only from the supplied record. Return exactly the requested marker, with no explanation, added punctuation, or surrounding whitespace.",
            "Record:\nThe approved route uses marker CEDAR-17.\nThe rejected route uses marker MAPLE-42.\n\nQuestion: What marker does the approved route use?",
            1500,
            u64::from(u32::MAX),
            None::<()>,
            tokenize,
        )
    }

    fn encode<S: Serialize>(
        &self,
        system: &str,
        user: &str,
        n_predict: u32,
        seed: u64,
        schema: Option<S>,
        mut tokenize: impl FnMut(&str, bool) -> Result<Vec<u32>, ModelRuntimeFailure>,
    ) -> Result<Vec<u8>, ModelRuntimeFailure> {
        let prompt = self
            .0
            .compose(tokenize(system, false)?, tokenize(user, false)?)?;
        let request =
            self.0
                .wire_request(&prompt, n_predict, seed, schema, CompletionRoute::Shared);
        serde_json::to_vec(&request).map_err(|_| {
            failure(
                "MODEL_REQUEST_INVALID",
                "Cannot encode shared completion",
                false,
            )
        })
    }
}

impl<'a> TokenizeRequest<'a> {
    pub(super) fn new(content: &'a str, parse_special: bool) -> Self {
        Self {
            content,
            add_special: false,
            parse_special,
            with_pieces: false,
        }
    }
}

impl CompletionResponse {
    pub(super) fn usage(&self) -> ModelTokenUsage {
        ModelTokenUsage {
            prompt_tokens: self.tokens_evaluated,
            completion_tokens: self.tokens_predicted,
            total_tokens: self
                .tokens_evaluated
                .zip(self.tokens_predicted)
                .map(|(prompt, completion)| prompt.saturating_add(completion)),
        }
    }

    /// Validated envelope with exact raw content; the application retains its own final trim.
    pub(super) fn validated_content(
        &self,
        prompt_tokens: u32,
        max_output_tokens: u32,
    ) -> Result<&str, ModelRuntimeFailure> {
        if self.truncated {
            return Err(failure(
                "MODEL_EXECUTION_UNVERIFIED",
                "Direct GGUF response exceeded the qualified context",
                true,
            ));
        }
        if matches!(self.stop_type, CompletionStopType::Limit) {
            return Err(failure(
                "MODEL_OUTPUT_LIMIT_REACHED",
                "Direct GGUF generation exhausted its output allowance before completion",
                true,
            ));
        }
        if !matches!(
            self.stop_type,
            CompletionStopType::Eos | CompletionStopType::Word
        ) {
            return Err(failure(
                "MODEL_EXECUTION_UNVERIFIED",
                "Direct GGUF generation stopped before a qualified completion boundary",
                true,
            ));
        }
        let reported_prompt_tokens = self.tokens_evaluated.ok_or_else(|| {
            failure(
                "MODEL_RESPONSE_INVALID",
                "Direct GGUF response omitted prompt-token accounting",
                true,
            )
        })?;
        let completion_tokens = self.tokens_predicted.ok_or_else(|| {
            failure(
                "MODEL_RESPONSE_INVALID",
                "Direct GGUF response omitted completion-token accounting",
                true,
            )
        })?;
        if reported_prompt_tokens != u64::from(prompt_tokens)
            || completion_tokens > u64::from(max_output_tokens)
        {
            return Err(failure(
                "MODEL_EXECUTION_UNVERIFIED",
                "Direct GGUF token accounting did not match the admitted request",
                true,
            ));
        }
        Ok(&self.content)
    }
}

pub(super) fn decode_bounded_json<T: for<'de> Deserialize<'de>>(
    reader: impl Read,
    limit: u64,
) -> Result<T, ModelRuntimeFailure> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            failure(
                "MODEL_RESPONSE_INVALID",
                "GGUF response could not be read",
                true,
            )
        })?;
    if bytes.len() as u64 > limit {
        return Err(failure(
            "MODEL_RESPONSE_TOO_LARGE",
            "GGUF response exceeded its byte limit",
            true,
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        failure(
            "MODEL_RESPONSE_INVALID",
            "GGUF response was malformed",
            true,
        )
    })
}

pub(super) fn failure(
    code: &str,
    message: impl Into<String>,
    recoverable: bool,
) -> ModelRuntimeFailure {
    ModelRuntimeFailure {
        code: code.to_string(),
        message: message.into(),
        recoverable,
        request_attempts: Vec::new(),
    }
}
#[derive(Serialize)]
pub(super) struct TokenizeRequest<'a> {
    content: &'a str,
    add_special: bool,
    parse_special: bool,
    with_pieces: bool,
}

#[derive(Deserialize)]
pub(super) struct TokenizeResponse {
    pub(super) tokens: Vec<u32>,
}

#[derive(Serialize)]
pub(super) struct CompletionRequest<'a, S = serde_json::Value> {
    prompt: &'a [u32],
    n_predict: u32,
    temperature: f32,
    seed: u64,
    stop: [&'a str; 1],
    cache_prompt: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_delimiters: Option<&'static [MessageDelimiter]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_schema: Option<S>,
}

#[derive(Deserialize)]
pub(super) struct CompletionResponse {
    content: String,
    tokens_predicted: Option<u64>,
    tokens_evaluated: Option<u64>,
    #[serde(default)]
    truncated: bool,
    stop_type: CompletionStopType,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CompletionStopType {
    Eos,
    Word,
    Limit,
    None,
}

fn checked_prompt_capacity(lengths: [usize; 5]) -> Result<usize, ModelRuntimeFailure> {
    let overflow = || {
        failure(
            "MODEL_CONTEXT_EXCEEDED",
            "GGUF prompt token count exceeds the supported range",
            false,
        )
    };
    let capacity = lengths
        .into_iter()
        .try_fold(0usize, |total, length| total.checked_add(length))
        .ok_or_else(overflow)?;
    u32::try_from(capacity).map_err(|_| overflow())?;
    Ok(capacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_token_capacity_preserves_exact_boundaries_without_allocating() {
        for (lengths, expected) in [
            ([0, 0, 0, 0, 0], 0),
            ([0, 1, 0, 0, 0], 1),
            ([0, u32::MAX as usize, 0, 0, 0], u32::MAX as usize),
        ] {
            assert_eq!(checked_prompt_capacity(lengths).unwrap(), expected);
        }
        for lengths in [[usize::MAX, 1, 0, 0, 0], [0, u32::MAX as usize, 0, 1, 0]] {
            let e = checked_prompt_capacity(lengths).unwrap_err();
            assert_eq!(
                (e.code.as_str(), e.message.as_str(), e.recoverable),
                (
                    "MODEL_CONTEXT_EXCEEDED",
                    "GGUF prompt token count exceeds the supported range",
                    false
                )
            );
        }
    }
}
