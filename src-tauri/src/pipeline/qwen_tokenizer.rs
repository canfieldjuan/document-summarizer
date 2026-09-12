use crate::pipeline::model::QwenTokenizerFamily;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tokenizers::models::bpe::{Vocab, BPE};
use tokenizers::pre_tokenizers::byte_level::ByteLevel;
use tokenizers::pre_tokenizers::sequence::Sequence;
use tokenizers::pre_tokenizers::split::{Split, SplitPattern};
use tokenizers::{SplitDelimiterBehavior, Tokenizer};

pub const TOKENIZER_FRAMING_RESERVE_TOKENS: u32 = 512;
pub const QWEN3_TOKENIZER_VERSION: &str = "qwen3-qwen2-pre-f2ec4434-v2";
pub const QWEN35_TOKENIZER_VERSION: &str = "qwen35-pre-cc5fb918-v2";

const QWEN3_TOKENIZER_FINGERPRINT: &str =
    "3cedcd85881d197ef65742e4d74622256b8159faed015b24aa6d16982dbe5335";
const QWEN35_TOKENIZER_FINGERPRINT: &str =
    "d5ea96b68508288e2e6c70c4d80c47ba8471d020019e36a9d842464fcf1ec4b7";
const QWEN3_PRE_TOKENIZER: &str = "qwen2";
const QWEN35_PRE_TOKENIZER: &str = "qwen35";
const QWEN3_PATTERN: &str = r"(?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\r\n\p{L}\p{N}]?\p{L}+|\p{N}| ?[^\s\p{L}\p{N}]+[\r\n]*|\s*[\r\n]+|\s+(?!\S)|\s+";
const QWEN35_PATTERN: &str = r"(?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\r\n\p{L}\p{N}]?[\p{L}\p{M}]+|\p{N}| ?[^\s\p{L}\p{M}\p{N}]+[\r\n]*|\s*[\r\n]+|\s+(?!\S)|\s+";

pub struct QwenPromptTokenizer {
    tokenizer: Tokenizer,
}

impl QwenPromptTokenizer {
    #[cfg(test)]
    pub(crate) fn fixture_single_token() -> Self {
        let mut vocab = Vocab::new();
        vocab.insert("[UNK]".to_string(), 0);
        let model = BPE::builder()
            .vocab_and_merges(vocab, Vec::new())
            .unk_token("[UNK]".to_string())
            .build()
            .expect("fixture tokenizer should build");
        Self {
            tokenizer: Tokenizer::new(model),
        }
    }

    #[cfg(any(test, feature = "connect-proof-runtime"))]
    pub(crate) fn conservative_byte_counter() -> Result<Self, String> {
        let mut alphabet: Vec<_> = ByteLevel::alphabet().into_iter().collect();
        alphabet.sort_unstable();
        let vocab: Vocab = alphabet
            .into_iter()
            .enumerate()
            .map(|(index, token)| {
                Ok((
                    token.to_string(),
                    u32::try_from(index)
                        .map_err(|_| "Byte tokenizer vocabulary exceeds u32".to_string())?,
                ))
            })
            .collect::<Result<_, String>>()?;
        let model = BPE::builder()
            .vocab_and_merges(vocab, Vec::new())
            .build()
            .map_err(|_| "Conservative byte tokenizer model could not be built".to_string())?;
        Ok(Self {
            tokenizer: tokenizer_from_bpe(model, QWEN3_PATTERN)?,
        })
    }

    pub fn from_model_info(
        family: QwenTokenizerFamily,
        model_info: &Map<String, Value>,
    ) -> Result<Self, String> {
        let pre = model_info
            .get("tokenizer.ggml.pre")
            .and_then(Value::as_str)
            .ok_or_else(|| "Tokenizer pre-tokenizer metadata is missing".to_string())?;
        let tokens_value = model_info
            .get("tokenizer.ggml.tokens")
            .ok_or_else(|| "Tokenizer vocabulary metadata is missing".to_string())?;
        let merges_value = model_info
            .get("tokenizer.ggml.merges")
            .ok_or_else(|| "Tokenizer merge metadata is missing".to_string())?;
        let canonical =
            serde_json::to_vec(&(Value::String(pre.to_string()), tokens_value, merges_value))
                .map_err(|_| "Tokenizer metadata could not be fingerprinted".to_string())?;
        let fingerprint = format!("{:x}", Sha256::digest(canonical));
        let (expected_pre, expected_fingerprint, pattern) = match family {
            QwenTokenizerFamily::Qwen3 => (
                QWEN3_PRE_TOKENIZER,
                QWEN3_TOKENIZER_FINGERPRINT,
                QWEN3_PATTERN,
            ),
            QwenTokenizerFamily::Qwen35 => (
                QWEN35_PRE_TOKENIZER,
                QWEN35_TOKENIZER_FINGERPRINT,
                QWEN35_PATTERN,
            ),
        };
        if pre != expected_pre || fingerprint != expected_fingerprint {
            return Err("Tokenizer metadata does not match the pinned Qwen profile".to_string());
        }

        let tokens = tokens_value
            .as_array()
            .ok_or_else(|| "Tokenizer vocabulary metadata is malformed".to_string())?;
        let mut vocab = Vocab::with_capacity(tokens.len());
        for (index, token) in tokens.iter().enumerate() {
            let token = token
                .as_str()
                .ok_or_else(|| "Tokenizer vocabulary entry is malformed".to_string())?;
            let index = u32::try_from(index)
                .map_err(|_| "Tokenizer vocabulary is too large".to_string())?;
            if vocab.insert(token.to_string(), index).is_some() {
                return Err("Tokenizer vocabulary contains a duplicate token".to_string());
            }
        }
        let merges = merges_value
            .as_array()
            .ok_or_else(|| "Tokenizer merge metadata is malformed".to_string())?
            .iter()
            .map(|merge| {
                let merge = merge
                    .as_str()
                    .ok_or_else(|| "Tokenizer merge entry is malformed".to_string())?;
                let (left, right) = merge
                    .split_once(' ')
                    .ok_or_else(|| "Tokenizer merge entry has no pair".to_string())?;
                if left.is_empty() || right.is_empty() || right.contains(' ') {
                    return Err("Tokenizer merge entry has an invalid pair".to_string());
                }
                Ok((left.to_string(), right.to_string()))
            })
            .collect::<Result<Vec<_>, String>>()?;

        let model = BPE::builder()
            .vocab_and_merges(vocab, merges)
            .build()
            .map_err(|_| "Pinned Qwen tokenizer model could not be built".to_string())?;
        Ok(Self {
            tokenizer: tokenizer_from_bpe(model, pattern)?,
        })
    }

    pub fn count(&self, payload: &str) -> Result<u32, String> {
        let encoding = self
            .tokenizer
            .encode(payload, false)
            .map_err(|_| "Pinned Qwen tokenizer could not encode the request".to_string())?;
        u32::try_from(encoding.len()).map_err(|_| "Token count exceeds supported range".to_string())
    }
}

fn tokenizer_from_bpe(model: BPE, pattern: &str) -> Result<Tokenizer, String> {
    let split = Split::new(
        SplitPattern::Regex(pattern.to_string()),
        SplitDelimiterBehavior::Isolated,
        false,
    )
    .map_err(|_| "Pinned Qwen pre-tokenizer could not be built".to_string())?;
    let byte_level = ByteLevel::default()
        .add_prefix_space(false)
        .trim_offsets(false)
        .use_regex(false);
    let mut tokenizer = Tokenizer::new(model);
    tokenizer.with_pre_tokenizer(Some(Sequence::new(vec![split.into(), byte_level.into()])));
    Ok(tokenizer)
}

pub fn tokenizer_version(family: QwenTokenizerFamily) -> &'static str {
    match family {
        QwenTokenizerFamily::Qwen3 => QWEN3_TOKENIZER_VERSION,
        QwenTokenizerFamily::Qwen35 => QWEN35_TOKENIZER_VERSION,
    }
}

pub fn request_fits_context(input_tokens: u32, output_tokens: u32, context_tokens: u32) -> bool {
    input_tokens
        .checked_add(output_tokens)
        .and_then(|tokens| tokens.checked_add(TOKENIZER_FRAMING_RESERVE_TOKENS))
        .is_some_and(|required| required <= context_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn byte_complete_prompt_tokenizer() -> QwenPromptTokenizer {
        QwenPromptTokenizer::conservative_byte_counter()
            .expect("byte-complete fixture tokenizer should build")
    }

    #[test]
    fn token_admission_checks_both_sides_of_the_context_boundary() {
        assert!(request_fits_context(3_584, 4_096, 8_192));
        assert!(!request_fits_context(3_585, 4_096, 8_192));
        assert!(!request_fits_context(u32::MAX, 1, u32::MAX));
    }

    #[test]
    fn token_count_preserves_decomposed_unicode_from_the_transmitted_payload() {
        let tokenizer = byte_complete_prompt_tokenizer();
        let composed = tokenizer
            .count("\u{00e9}")
            .expect("composed input should tokenize");
        let decomposed = tokenizer
            .count("e\u{0301}")
            .expect("decomposed input should tokenize");

        assert_eq!(composed, 2);
        assert_eq!(decomposed, 3);
        assert!(decomposed > composed);
        assert_eq!(
            tokenizer_version(QwenTokenizerFamily::Qwen3),
            QWEN3_TOKENIZER_VERSION
        );
        assert_eq!(
            tokenizer_version(QwenTokenizerFamily::Qwen35),
            QWEN35_TOKENIZER_VERSION
        );
    }

    #[test]
    fn unknown_or_incomplete_tokenizer_metadata_fails_closed() {
        let mut model_info = Map::new();
        model_info.insert(
            "tokenizer.ggml.pre".to_string(),
            Value::String(QWEN3_PRE_TOKENIZER.to_string()),
        );
        assert!(
            QwenPromptTokenizer::from_model_info(QwenTokenizerFamily::Qwen3, &model_info).is_err()
        );
        model_info.insert(
            "tokenizer.ggml.tokens".to_string(),
            Value::Array(Vec::new()),
        );
        model_info.insert(
            "tokenizer.ggml.merges".to_string(),
            Value::Array(Vec::new()),
        );
        assert!(
            QwenPromptTokenizer::from_model_info(QwenTokenizerFamily::Qwen3, &model_info).is_err()
        );
    }
}
