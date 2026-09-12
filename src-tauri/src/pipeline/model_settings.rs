use crate::pipeline::contracts::{
    ModelProfileSnapshot, ModelRequest, ModelResponse, ModelRuntime, ModelRuntimeFailure,
    ModelRuntimeKind, ModelStageProfileSnapshot, PipelineStage,
};
use crate::pipeline::control::ExecutionControl;
use crate::pipeline::gateway_client::GatewayClientConfig;
use crate::pipeline::gateway_runtime::GatewayRuntime;
use crate::pipeline::llama_cpp::{
    current_regular_file_identity, inspect_regular_file, prepare_for_ollama_runtime,
    prune_idle_managed_runtimes, qualified_runtime_available, FileIdentity, GgufRuntimeConfig,
    LlamaCppRuntime, QualifiedRuntimeFile,
};
use crate::pipeline::model::{InstalledModelDescriptor, OllamaRuntime, QwenTokenizerFamily};
use crate::pipeline::qwen_tokenizer::{
    tokenizer_version, QWEN35_TOKENIZER_VERSION, QWEN3_TOKENIZER_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;
use tempfile::Builder;
#[cfg(test)]
use uuid::Uuid;

const SETTINGS_VERSION: u32 = 3;
pub const SETTINGS_FILE_NAME: &str = "model-settings-v1.json";
const DEFAULT_PRESET_ID: &str = "full-qwen3-30b-a3b-q4ks-v1";
const GATEWAY_TIMEOUT: Duration = Duration::from_secs(900);
const GATEWAY_REQUEST_LIFETIME: Duration = Duration::from_secs(900);
const MAX_REGISTERED_GGUFS: usize = 32;
const MAX_REGISTERED_PATH_BYTES: usize = 4_096;
const MAX_REGISTERED_LABEL_BYTES: usize = 512;
const MAX_GATEWAY_URL_BYTES: usize = 2_048;
const JACK_GGUF_DIGEST: &str = "e7fecb29086afb4f6ca054b0f1469f2704a24e56db27c5980827f5f32d26f041";
const QUALIFIED_LLAMA_SERVER_DIGEST: &str =
    "0ca399edd758decd825a71823b04ba7ddbc8b2e10d2309d8bf623ee3c2283099";
static SETTINGS_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static ACTIVE_PROFILE_LEASE: OnceLock<Mutex<Option<(RuntimeProfileKey, usize)>>> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
struct QualifiedProfile {
    profile_id: &'static str,
    digest: &'static str,
    tokenizer_family: QwenTokenizerFamily,
    tokenizer_version: &'static str,
    safe_context_tokens: u32,
    analysis: bool,
    verifier_rank: Option<u16>,
    full: bool,
    hybrid_analysis: bool,
    runtime_kind: ModelRuntimeKind,
    runtime_binary_digest: Option<&'static str>,
    runtime_libraries: &'static [QualifiedRuntimeFile],
    label: &'static str,
}

// Qualifications are exact-digest evidence, not name-based capability guesses.
// Candidate entries are added only after the unchanged corpus gates pass.
const QUALIFIED_PROFILES: &[QualifiedProfile] = &[
    QualifiedProfile {
        profile_id: "qwen3-30b-a3b-q4ks-v1",
        digest: "1eda56426671cdf365913097543c2253a73c57e35b12741306689968d7f70292",
        tokenizer_family: QwenTokenizerFamily::Qwen3,
        tokenizer_version: QWEN3_TOKENIZER_VERSION,
        safe_context_tokens: 8_192,
        analysis: true,
        verifier_rank: Some(100),
        full: true,
        hybrid_analysis: true,
        runtime_kind: ModelRuntimeKind::OllamaNative,
        runtime_binary_digest: None,
        runtime_libraries: &[],
        label: "Qwen 3 30B-A3B",
    },
    QualifiedProfile {
        profile_id: "jack-qwen38-27b-iq2m-v1",
        digest: JACK_GGUF_DIGEST,
        tokenizer_family: QwenTokenizerFamily::Qwen35,
        tokenizer_version: QWEN35_TOKENIZER_VERSION,
        safe_context_tokens: 8_192,
        analysis: true,
        verifier_rank: Some(90),
        full: true,
        hybrid_analysis: false,
        runtime_kind: ModelRuntimeKind::LlamaCppGguf,
        runtime_binary_digest: Some(QUALIFIED_LLAMA_SERVER_DIGEST),
        runtime_libraries: JACK_LLAMA_CPP_LIBRARIES,
        label: "Jack Qwen 3.8 27B Coder (12 GB)",
    },
];

const JACK_LLAMA_CPP_LIBRARIES: &[QualifiedRuntimeFile] = &[
    QualifiedRuntimeFile {
        file_name: "libllama-server-impl.so",
        digest: "bd3e91a31fb3c61152043083f1e5008f9b7aef7bf5c168d6b3eca0019f634008",
    },
    QualifiedRuntimeFile {
        file_name: "libllama-common.so.0",
        digest: "9cf26696816c83fb6e148b3142c1a59bb374de9dc6a92a2966d9abe99939d2a1",
    },
    QualifiedRuntimeFile {
        file_name: "libmtmd.so.0",
        digest: "ec117a22c9acd24e9eeea4418c93d3d13512bcb607fee97a9674f82b93cb35c7",
    },
    QualifiedRuntimeFile {
        file_name: "libllama.so.0",
        digest: "c600923b1e548798b80d58b029505dbea4ccd4d2844de38416936e184906f645",
    },
    QualifiedRuntimeFile {
        file_name: "libggml.so.0",
        digest: "b80a4252c981712564828488b1962e80feb49675ca23da4a8479b2ed7361f86f",
    },
    QualifiedRuntimeFile {
        file_name: "libggml-base.so.0",
        digest: "c08e63a459d5d0ae4e982d1181fac3c4a0fcb40fcc49d7489ad9e16e4d39ca63",
    },
    QualifiedRuntimeFile {
        file_name: "libggml-cpu.so.0",
        digest: "d0b746ea2d7e8188236023d4c3a1ba8900a88600e8fdd0f1e6b5043ddf76fcdd",
    },
    QualifiedRuntimeFile {
        file_name: "libggml-cuda.so.0",
        digest: "4095bde67d003066a6a622573d165595ad8648470d55317cc94ed4d47acf353f",
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisteredGguf {
    pub canonical_path: PathBuf,
    pub file_name: String,
    pub digest: String,
    pub size_bytes: u64,
    pub file_identity: FileIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelSettings {
    version: u32,
    #[serde(default)]
    pub selected_source: InferenceSource,
    pub selected_preset_id: String,
    #[serde(default)]
    pub registered_ggufs: Vec<RegisteredGguf>,
    #[serde(default)]
    gateway: Option<GatewayConnectionSettings>,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            selected_source: InferenceSource::Direct,
            selected_preset_id: DEFAULT_PRESET_ID.to_string(),
            registered_ggufs: Vec::new(),
            gateway: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceSource {
    #[default]
    Direct,
    Gateway,
}

impl InferenceSource {
    pub fn provider_name(self) -> &'static str {
        match self {
            Self::Direct => "Local Qwen",
            Self::Gateway => "Local Inference Gateway",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GatewayConnectionSettings {
    base_url: String,
    token_file: PathBuf,
    ca_file: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayConnectionView {
    pub platform_supported: bool,
    pub configured: bool,
    pub base_url: Option<String>,
    pub token_file_name: Option<String>,
    pub ca_file_name: Option<String>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelPresetMode {
    Full,
    Hybrid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    #[serde(flatten)]
    pub descriptor: InstalledModelDescriptor,
    pub profile_id: Option<String>,
    pub qualified_context_tokens: Option<u32>,
    pub supports_analysis: bool,
    pub supports_verification: bool,
    pub supports_full: bool,
    pub runtime_kind: ModelRuntimeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPreset {
    pub preset_id: String,
    pub label: String,
    pub mode: ModelPresetMode,
    pub analysis_runtime_kind: ModelRuntimeKind,
    pub analysis_profile_id: String,
    pub analysis_model: String,
    pub analysis_digest: String,
    pub analysis_context_tokens: u32,
    pub analysis_tokenizer_version: String,
    pub verification_profile_id: String,
    pub verification_model: String,
    pub verification_digest: String,
    pub verification_context_tokens: u32,
    pub verification_tokenizer_version: String,
    pub verification_runtime_kind: ModelRuntimeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub selected_source: InferenceSource,
    pub selected_preset_id: String,
    pub selected_preset_available: bool,
    pub gateway: GatewayConnectionView,
    pub installed_models: Vec<ModelOption>,
    pub presets: Vec<ModelPreset>,
    pub discovery_warnings: Vec<String>,
}

pub fn settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(SETTINGS_FILE_NAME)
}

pub fn load_settings(path: &Path) -> Result<ModelSettings, ModelRuntimeFailure> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ModelSettings::default())
        }
        Err(_) => return Err(config_failure("Model settings could not be read")),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| config_failure("Model settings are malformed"))?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| config_failure("Model settings version is missing"))?;
    let mut settings: ModelSettings = if version == 1 {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct LegacySettings {
            version: u32,
            selected_preset_id: String,
        }
        let legacy: LegacySettings = serde_json::from_value(value)
            .map_err(|_| config_failure("Model settings are malformed"))?;
        let _ = legacy.version;
        ModelSettings {
            version: SETTINGS_VERSION,
            selected_source: InferenceSource::Direct,
            selected_preset_id: legacy.selected_preset_id,
            registered_ggufs: Vec::new(),
            gateway: None,
        }
    } else if version == 2 {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct LegacySettings {
            version: u32,
            selected_preset_id: String,
            #[serde(default)]
            registered_ggufs: Vec<RegisteredGguf>,
        }
        let legacy: LegacySettings = serde_json::from_value(value)
            .map_err(|_| config_failure("Model settings are malformed"))?;
        let _ = legacy.version;
        ModelSettings {
            version: SETTINGS_VERSION,
            selected_source: InferenceSource::Direct,
            selected_preset_id: legacy.selected_preset_id,
            registered_ggufs: legacy.registered_ggufs,
            gateway: None,
        }
    } else {
        serde_json::from_value(value).map_err(|_| config_failure("Model settings are malformed"))?
    };
    validate_settings(&settings)?;
    settings.version = SETTINGS_VERSION;
    if settings.version != SETTINGS_VERSION || settings.selected_preset_id.trim().is_empty() {
        return Err(config_failure(
            "Model settings version or preset is invalid",
        ));
    }
    Ok(settings)
}

pub fn save_selected_preset(
    path: &Path,
    selected_preset_id: &str,
) -> Result<ModelSettings, ModelRuntimeFailure> {
    if selected_preset_id.trim().is_empty() || selected_preset_id.len() > 128 {
        return Err(config_failure("Selected model preset is invalid"));
    }
    let _writer = settings_writer()?;
    let mut settings = load_settings(path)?;
    settings.selected_source = InferenceSource::Direct;
    settings.selected_preset_id = selected_preset_id.to_string();
    persist_settings(path, &settings)?;
    Ok(settings)
}

pub fn configure_gateway(
    path: &Path,
    db_path: &Path,
    base_url: &str,
    token_file: &Path,
    ca_file: &Path,
) -> Result<ModelSettings, ModelRuntimeFailure> {
    configure_gateway_with(path, base_url, token_file, ca_file, |gateway| {
        let runtime = GatewayRuntime::new(db_path.to_path_buf(), gateway.client_config())?;
        runtime.health()
    })
}

fn configure_gateway_with(
    path: &Path,
    base_url: &str,
    token_file: &Path,
    ca_file: &Path,
    validate: impl FnOnce(&GatewayConnectionSettings) -> Result<(), ModelRuntimeFailure>,
) -> Result<ModelSettings, ModelRuntimeFailure> {
    ensure_gateway_platform_supported()?;
    let gateway = GatewayConnectionSettings {
        base_url: base_url.to_string(),
        token_file: token_file.to_path_buf(),
        ca_file: ca_file.to_path_buf(),
    };
    validate_gateway_metadata(&gateway)?;
    validate(&gateway)?;
    prune_idle_managed_runtimes()?;
    let _writer = settings_writer()?;
    let mut settings = load_settings(path)?;
    settings.selected_source = InferenceSource::Gateway;
    settings.gateway = Some(gateway);
    persist_settings(path, &settings)?;
    Ok(settings)
}

pub fn select_gateway(path: &Path, db_path: &Path) -> Result<ModelSettings, ModelRuntimeFailure> {
    select_gateway_with(path, |gateway| {
        let runtime = GatewayRuntime::new(db_path.to_path_buf(), gateway.client_config())?;
        runtime.health()
    })
}

fn select_gateway_with(
    path: &Path,
    validate: impl FnOnce(&GatewayConnectionSettings) -> Result<(), ModelRuntimeFailure>,
) -> Result<ModelSettings, ModelRuntimeFailure> {
    ensure_gateway_platform_supported()?;
    let current = load_settings(path)?;
    let gateway = current
        .gateway
        .clone()
        .ok_or_else(|| config_failure("Inference gateway is not configured"))?;
    validate(&gateway)?;
    prune_idle_managed_runtimes()?;
    let _writer = settings_writer()?;
    let mut settings = load_settings(path)?;
    if settings.gateway.as_ref() != Some(&gateway) {
        return Err(config_failure(
            "Inference gateway configuration changed during selection",
        ));
    }
    settings.selected_source = InferenceSource::Gateway;
    persist_settings(path, &settings)?;
    Ok(settings)
}

pub fn register_gguf(
    path: &Path,
    selected_path: &Path,
) -> Result<ModelSettings, ModelRuntimeFailure> {
    validate_selected_gguf_path(selected_path)?;
    let (canonical_path, size_bytes, digest, file_identity) = inspect_regular_file(selected_path)?;
    let file_name = canonical_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| config_failure("Selected GGUF filename must be UTF-8"))?
        .to_string();
    let _writer = settings_writer()?;
    let mut settings = load_settings(path)?;
    if let Some(existing) = settings
        .registered_ggufs
        .iter_mut()
        .find(|existing| existing.digest == digest)
    {
        existing.canonical_path = canonical_path;
        existing.file_name = file_name;
        existing.size_bytes = size_bytes;
        existing.file_identity = file_identity;
    } else {
        if settings.registered_ggufs.len() >= MAX_REGISTERED_GGUFS {
            return Err(config_failure("Registered GGUF catalog is full"));
        }
        settings.registered_ggufs.push(RegisteredGguf {
            canonical_path,
            file_name,
            digest,
            size_bytes,
            file_identity,
        });
    }
    validate_settings(&settings)?;
    persist_settings(path, &settings)?;
    Ok(settings)
}

fn settings_writer() -> Result<MutexGuard<'static, ()>, ModelRuntimeFailure> {
    SETTINGS_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| config_failure("Model settings writer is unavailable"))
}

fn persist_settings(path: &Path, settings: &ModelSettings) -> Result<(), ModelRuntimeFailure> {
    validate_settings(settings)?;
    let parent = path
        .parent()
        .ok_or_else(|| config_failure("Model settings path is invalid"))?;
    fs::create_dir_all(parent)
        .map_err(|_| config_failure("Model settings directory could not be created"))?;
    let mut temporary = Builder::new()
        .prefix(".model-settings-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|_| config_failure("Temporary model settings could not be created"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| config_failure("Temporary model settings could not be created"))?;
    }
    serde_json::to_writer(temporary.as_file_mut(), &settings)
        .map_err(|_| config_failure("Model settings could not be serialized"))?;
    temporary
        .as_file_mut()
        .write_all(b"\n")
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| config_failure("Model settings could not be persisted"))?;
    temporary
        .persist(path)
        .map_err(|_| config_failure("Model settings could not be committed"))?;
    Ok(())
}

fn validate_settings(settings: &ModelSettings) -> Result<(), ModelRuntimeFailure> {
    if settings.version != SETTINGS_VERSION
        || settings.selected_preset_id.trim().is_empty()
        || settings.selected_preset_id.len() > 128
        || settings.registered_ggufs.len() > MAX_REGISTERED_GGUFS
    {
        return Err(config_failure(
            "Model settings version or values are invalid",
        ));
    }
    let mut digests = HashSet::new();
    for registration in &settings.registered_ggufs {
        let path = registration
            .canonical_path
            .to_str()
            .ok_or_else(|| config_failure("Registered GGUF paths must use valid UTF-8"))?;
        if !registration.canonical_path.is_absolute()
            || path.len() > MAX_REGISTERED_PATH_BYTES
            || registration.file_name.is_empty()
            || registration.file_name.len() > MAX_REGISTERED_LABEL_BYTES
            || registration.size_bytes == 0
            || registration.file_identity.size_bytes != registration.size_bytes
            || registration.file_identity.device == 0
            || registration.file_identity.inode == 0
            || registration.digest.len() != 64
            || !registration
                .digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || !digests.insert(registration.digest.clone())
        {
            return Err(config_failure("Registered GGUF metadata is invalid"));
        }
    }
    if let Some(gateway) = &settings.gateway {
        validate_gateway_metadata(gateway)?;
    }
    if settings.selected_source == InferenceSource::Gateway && settings.gateway.is_none() {
        return Err(config_failure(
            "Selected inference gateway is not configured",
        ));
    }
    Ok(())
}

impl GatewayConnectionSettings {
    fn client_config(&self) -> GatewayClientConfig {
        GatewayClientConfig {
            base_url: self.base_url.clone(),
            token_file: self.token_file.clone(),
            ca_file: self.ca_file.clone(),
            timeout: GATEWAY_TIMEOUT,
            request_lifetime: GATEWAY_REQUEST_LIFETIME,
        }
    }
}

fn validate_gateway_metadata(
    gateway: &GatewayConnectionSettings,
) -> Result<(), ModelRuntimeFailure> {
    if gateway.base_url.is_empty()
        || gateway.base_url.len() > MAX_GATEWAY_URL_BYTES
        || !gateway.token_file.is_absolute()
        || !gateway.ca_file.is_absolute()
        || path_text_len(&gateway.token_file)? > MAX_REGISTERED_PATH_BYTES
        || path_text_len(&gateway.ca_file)? > MAX_REGISTERED_PATH_BYTES
    {
        return Err(config_failure(
            "Inference gateway connection metadata is invalid",
        ));
    }
    Ok(())
}

fn path_text_len(path: &Path) -> Result<usize, ModelRuntimeFailure> {
    path.to_str()
        .map(str::len)
        .ok_or_else(|| config_failure("Inference gateway paths must use valid UTF-8"))
}

fn ensure_gateway_platform_supported() -> Result<(), ModelRuntimeFailure> {
    if cfg!(unix) {
        Ok(())
    } else {
        Err(config_failure(
            "Inference gateway credential validation is not available on this platform",
        ))
    }
}

fn validate_selected_gguf_path(path: &Path) -> Result<(), ModelRuntimeFailure> {
    let raw = path
        .to_str()
        .ok_or_else(|| config_failure("Selected GGUF path must use valid UTF-8"))?;
    if raw.len() > MAX_REGISTERED_PATH_BYTES
        || !path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
    {
        return Err(config_failure(
            "Selected model must be one bounded .gguf file",
        ));
    }
    Ok(())
}

fn registered_descriptors(
    settings: &ModelSettings,
) -> Vec<(InstalledModelDescriptor, ModelRuntimeKind)> {
    settings
        .registered_ggufs
        .iter()
        .map(|registration| {
            let qualified = QUALIFIED_PROFILES.iter().find(|profile| {
                profile.runtime_kind == ModelRuntimeKind::LlamaCppGguf
                    && profile.digest == registration.digest
            });
            let mut disabled_reason = None;
            match current_regular_file_identity(&registration.canonical_path) {
                Ok(identity) if identity == registration.file_identity => {}
                _ => {
                    disabled_reason = Some(
                        "Registered GGUF file is missing or its identity has changed".to_string(),
                    );
                }
            }
            if disabled_reason.is_none() {
                if let Some(profile) = qualified {
                    if profile.runtime_binary_digest.is_some_and(|expected| {
                        qualified_runtime_available(expected, profile.runtime_libraries).is_err()
                    }) {
                        disabled_reason = Some(
                            "Qualified llama-server runtime is unavailable or changed".to_string(),
                        );
                    }
                }
            }
            if qualified.is_none() {
                disabled_reason = Some("This GGUF digest has not passed qualification".to_string());
            }
            (
                InstalledModelDescriptor {
                    name: registration.file_name.clone(),
                    digest: registration.digest.clone(),
                    size_bytes: registration.size_bytes,
                    architecture: qualified.map(|_| "qwen35".to_string()),
                    tokenizer_family: qualified.map(|profile| profile.tokenizer_family),
                    parameter_size: qualified.map(|_| "27.3B".to_string()),
                    quantization_level: qualified.map(|_| "IQ2_M".to_string()),
                    maximum_context_tokens: qualified.map(|_| 262_144),
                    disabled_reason,
                },
                ModelRuntimeKind::LlamaCppGguf,
            )
        })
        .collect()
}

pub fn catalog(path: &Path) -> Result<ModelCatalog, ModelRuntimeFailure> {
    let settings = load_settings(path)?;
    let mut warnings = Vec::new();
    let ollama = match OllamaRuntime::discovery_from_environment()
        .and_then(|runtime| runtime.installed_models())
    {
        Ok(descriptors) => descriptors,
        Err(_) => {
            warnings.push("Ollama discovery is unavailable".to_string());
            Vec::new()
        }
    };
    let mut sources = ollama
        .into_iter()
        .map(|descriptor| (descriptor, ModelRuntimeKind::OllamaNative))
        .collect::<Vec<_>>();
    sources.extend(registered_descriptors(&settings));
    catalog_from_descriptors(settings, sources, QUALIFIED_PROFILES, warnings)
}

fn catalog_from_descriptors(
    settings: ModelSettings,
    mut descriptors: Vec<(InstalledModelDescriptor, ModelRuntimeKind)>,
    profiles: &'static [QualifiedProfile],
    discovery_warnings: Vec<String>,
) -> Result<ModelCatalog, ModelRuntimeFailure> {
    descriptors.sort_by(|left, right| left.0.name.cmp(&right.0.name));
    let mut installed_models = Vec::with_capacity(descriptors.len());
    for (mut descriptor, runtime_kind) in descriptors {
        let profile = profiles.iter().find(|profile| {
            profile.digest == descriptor.digest && profile.runtime_kind == runtime_kind
        });
        if let Some(profile) = profile {
            let family_matches = descriptor.tokenizer_family == Some(profile.tokenizer_family);
            let context_matches = descriptor
                .maximum_context_tokens
                .is_some_and(|maximum| maximum >= profile.safe_context_tokens);
            if !family_matches || !context_matches {
                descriptor.disabled_reason = Some(
                    "Installed metadata no longer matches the qualified model profile".to_string(),
                );
            }
        }
        installed_models.push(ModelOption {
            profile_id: profile.map(|profile| profile.profile_id.to_string()),
            qualified_context_tokens: profile.map(|profile| profile.safe_context_tokens),
            supports_analysis: profile.is_some_and(|profile| profile.analysis),
            supports_verification: profile.is_some_and(|profile| profile.verifier_rank.is_some()),
            supports_full: profile.is_some_and(|profile| profile.full),
            runtime_kind,
            descriptor,
        });
    }
    let preset_options = canonical_preset_options(&installed_models, profiles);
    let verifier = strongest_verifier(&preset_options, profiles);
    let mut presets = Vec::new();
    for option in preset_options {
        let Some(profile) = profile_for_option(option, profiles) else {
            continue;
        };
        if option.descriptor.disabled_reason.is_some() {
            continue;
        }
        if profile.full {
            presets.push(preset(
                format!("full-{}", profile.profile_id),
                profile.label.to_string(),
                ModelPresetMode::Full,
                option,
                option,
                profiles,
            )?);
        }
        if profile.hybrid_analysis {
            if let Some(verifier) = verifier {
                if verifier.descriptor.digest != option.descriptor.digest {
                    presets.push(preset(
                        format!(
                            "hybrid-{}-{}",
                            profile.profile_id,
                            verifier.profile_id.as_deref().unwrap_or("unknown")
                        ),
                        format!("{} + strongest verifier", profile.label),
                        ModelPresetMode::Hybrid,
                        option,
                        verifier,
                        profiles,
                    )?);
                }
            }
        }
    }
    presets.sort_by(|left, right| left.preset_id.cmp(&right.preset_id));
    let selected_preset_available = presets
        .iter()
        .any(|preset| preset.preset_id == settings.selected_preset_id);
    let gateway = gateway_connection_view(settings.gateway.as_ref());
    Ok(ModelCatalog {
        selected_source: settings.selected_source,
        selected_preset_id: settings.selected_preset_id,
        selected_preset_available,
        gateway,
        installed_models,
        presets,
        discovery_warnings,
    })
}

fn gateway_connection_view(gateway: Option<&GatewayConnectionSettings>) -> GatewayConnectionView {
    let platform_supported = cfg!(unix);
    GatewayConnectionView {
        platform_supported,
        configured: gateway.is_some(),
        base_url: gateway.map(|value| value.base_url.clone()),
        token_file_name: gateway.and_then(|value| bounded_file_name(&value.token_file)),
        ca_file_name: gateway.and_then(|value| bounded_file_name(&value.ca_file)),
        unavailable_reason: (!platform_supported).then(|| {
            "Gateway credential ownership validation is not available on this platform".to_string()
        }),
    }
}

fn bounded_file_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && value.len() <= MAX_REGISTERED_LABEL_BYTES)
        .map(ToOwned::to_owned)
}

fn profile_for_option(
    option: &ModelOption,
    profiles: &'static [QualifiedProfile],
) -> Option<&'static QualifiedProfile> {
    option
        .profile_id
        .as_deref()
        .and_then(|id| profiles.iter().find(|profile| profile.profile_id == id))
}

fn canonical_preset_options<'a>(
    options: &'a [ModelOption],
    profiles: &'static [QualifiedProfile],
) -> Vec<&'a ModelOption> {
    let mut seen_identities = HashSet::new();
    options
        .iter()
        .filter(|option| option.descriptor.disabled_reason.is_none())
        .filter(|option| profile_for_option(option, profiles).is_some())
        .filter(|option| {
            seen_identities.insert((option.runtime_kind, option.descriptor.digest.clone()))
        })
        .collect()
}

fn strongest_verifier<'a>(
    options: &[&'a ModelOption],
    profiles: &'static [QualifiedProfile],
) -> Option<&'a ModelOption> {
    options
        .iter()
        .filter_map(|option| {
            profile_for_option(option, profiles)
                .and_then(|profile| profile.verifier_rank.map(|rank| (rank, *option)))
        })
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, option)| option)
}

fn preset(
    preset_id: String,
    label: String,
    mode: ModelPresetMode,
    analysis: &ModelOption,
    verification: &ModelOption,
    profiles: &'static [QualifiedProfile],
) -> Result<ModelPreset, ModelRuntimeFailure> {
    let analysis_profile = profile_for_option(analysis, profiles)
        .ok_or_else(|| config_failure("Analysis model has no qualified profile"))?;
    let verification_profile = profile_for_option(verification, profiles)
        .ok_or_else(|| config_failure("Verification model has no qualified profile"))?;
    Ok(ModelPreset {
        preset_id,
        label,
        mode,
        analysis_profile_id: analysis_profile.profile_id.to_string(),
        analysis_runtime_kind: analysis.runtime_kind,
        analysis_model: analysis.descriptor.name.clone(),
        analysis_digest: analysis.descriptor.digest.clone(),
        analysis_context_tokens: analysis
            .qualified_context_tokens
            .ok_or_else(|| config_failure("Analysis profile has no qualified context"))?,
        analysis_tokenizer_version: analysis_profile.tokenizer_version.to_string(),
        verification_profile_id: verification_profile.profile_id.to_string(),
        verification_model: verification.descriptor.name.clone(),
        verification_digest: verification.descriptor.digest.clone(),
        verification_context_tokens: verification
            .qualified_context_tokens
            .ok_or_else(|| config_failure("Verification profile has no qualified context"))?,
        verification_tokenizer_version: verification_profile.tokenizer_version.to_string(),
        verification_runtime_kind: verification.runtime_kind,
    })
}

pub fn runtime_from_settings(
    path: &Path,
    db_path: &Path,
) -> Result<Box<dyn ModelRuntime>, ModelRuntimeFailure> {
    let settings = load_settings(path)?;
    if settings.selected_source == InferenceSource::Gateway {
        ensure_gateway_platform_supported()?;
        let gateway = settings
            .gateway
            .as_ref()
            .ok_or_else(|| config_failure("Inference gateway is not configured"))?;
        return Ok(Box::new(GatewayRuntime::new(
            db_path.to_path_buf(),
            gateway.client_config(),
        )?));
    }
    if let Some(snapshot) = selected_direct_snapshot(&settings)? {
        return Ok(Box::new(QwenProfileRuntime::from_snapshot(
            &snapshot, path,
        )?));
    }
    let catalog = catalog(path)?;
    let preset = catalog
        .presets
        .iter()
        .find(|preset| preset.preset_id == catalog.selected_preset_id)
        .ok_or_else(|| config_failure("Selected model preset is unavailable"))?;
    Ok(Box::new(QwenProfileRuntime::new(preset, path)?))
}

fn selected_direct_snapshot(
    settings: &ModelSettings,
) -> Result<Option<ModelProfileSnapshot>, ModelRuntimeFailure> {
    let Some(profile) = QUALIFIED_PROFILES.iter().find(|profile| {
        profile.runtime_kind == ModelRuntimeKind::LlamaCppGguf
            && settings.selected_preset_id == format!("full-{}", profile.profile_id)
    }) else {
        return Ok(None);
    };
    let registration = settings
        .registered_ggufs
        .iter()
        .find(|registration| registration.digest == profile.digest)
        .ok_or_else(|| config_failure("Selected GGUF registration is unavailable"))?;
    let stage = ModelStageProfileSnapshot {
        runtime_kind: profile.runtime_kind,
        profile_id: profile.profile_id.to_string(),
        model_name: registration.file_name.clone(),
        model_digest: profile.digest.to_string(),
        context_tokens: profile.safe_context_tokens,
        tokenizer_version: profile.tokenizer_version.to_string(),
    };
    Ok(Some(ModelProfileSnapshot {
        version: 2,
        preset_id: settings.selected_preset_id.clone(),
        analysis: stage.clone(),
        verification: stage,
    }))
}

pub fn runtime_from_snapshot(
    snapshot: &ModelProfileSnapshot,
    settings_path: &Path,
    db_path: &Path,
) -> Result<Box<dyn ModelRuntime>, ModelRuntimeFailure> {
    if snapshot.version == 3 {
        ensure_gateway_platform_supported()?;
        let settings = load_settings(settings_path)?;
        let gateway = settings
            .gateway
            .ok_or_else(|| config_failure("Inference gateway is not configured"))?;
        return Ok(Box::new(GatewayRuntime::from_snapshot(
            db_path.to_path_buf(),
            gateway.client_config(),
            snapshot,
        )?));
    }
    Ok(Box::new(QwenProfileRuntime::from_snapshot(
        snapshot,
        settings_path,
    )?))
}

pub struct QwenProfileRuntime {
    preset_id: String,
    snapshot: ModelProfileSnapshot,
    analysis: StageRuntime,
    verification: StageRuntime,
    _profile_lease: Option<RuntimeProfileLease>,
}

#[derive(Clone, PartialEq, Eq)]
struct RuntimeStageKey {
    runtime_kind: ModelRuntimeKind,
    profile_id: String,
    model_digest: String,
    context_tokens: u32,
}

#[derive(Clone, PartialEq, Eq)]
struct RuntimeProfileKey {
    analysis: RuntimeStageKey,
    verification: RuntimeStageKey,
}

struct RuntimeProfileLease {
    key: RuntimeProfileKey,
}

impl RuntimeProfileLease {
    fn acquire(snapshot: &ModelProfileSnapshot) -> Result<Self, ModelRuntimeFailure> {
        let key = RuntimeProfileKey::from(snapshot);
        let mut active = ACTIVE_PROFILE_LEASE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| config_failure("Active model profile registry is unavailable"))?;
        claim_profile_lease(&mut active, &key)?;
        Ok(Self { key })
    }
}

fn claim_profile_lease(
    active: &mut Option<(RuntimeProfileKey, usize)>,
    key: &RuntimeProfileKey,
) -> Result<(), ModelRuntimeFailure> {
    match active.as_mut() {
        Some((active_key, count)) if active_key == key => {
            *count = count
                .checked_add(1)
                .ok_or_else(|| config_failure("Active model profile lease count is invalid"))?;
        }
        Some(_) => {
            return Err(ModelRuntimeFailure {
                code: "MODEL_RUNTIME_BUSY".to_string(),
                message: "A different qualified model profile is still in use".to_string(),
                recoverable: true,
                request_attempts: Vec::new(),
            });
        }
        None => *active = Some((key.clone(), 1)),
    }
    Ok(())
}

fn release_profile_lease(active: &mut Option<(RuntimeProfileKey, usize)>, key: &RuntimeProfileKey) {
    match active.as_mut() {
        Some((active_key, count)) if active_key == key && *count > 1 => *count -= 1,
        Some((active_key, 1)) if active_key == key => *active = None,
        _ => {}
    }
}

impl From<&ModelStageProfileSnapshot> for RuntimeStageKey {
    fn from(stage: &ModelStageProfileSnapshot) -> Self {
        Self {
            runtime_kind: stage.runtime_kind,
            profile_id: stage.profile_id.clone(),
            model_digest: stage.model_digest.clone(),
            context_tokens: stage.context_tokens,
        }
    }
}

impl From<&ModelProfileSnapshot> for RuntimeProfileKey {
    fn from(snapshot: &ModelProfileSnapshot) -> Self {
        Self {
            analysis: RuntimeStageKey::from(&snapshot.analysis),
            verification: RuntimeStageKey::from(&snapshot.verification),
        }
    }
}

impl Drop for RuntimeProfileLease {
    fn drop(&mut self) {
        let Some(registry) = ACTIVE_PROFILE_LEASE.get() else {
            return;
        };
        let Ok(mut active) = registry.lock() else {
            return;
        };
        release_profile_lease(&mut active, &self.key);
    }
}

enum StageRuntime {
    Ollama(OllamaRuntime),
    LlamaCpp(Arc<LlamaCppRuntime>),
}

impl StageRuntime {
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        match self {
            Self::Ollama(runtime) => runtime.generate(request),
            Self::LlamaCpp(runtime) => runtime.generate(request),
        }
    }

    fn generate_with_control(
        &self,
        request: &ModelRequest,
        control: &dyn ExecutionControl,
    ) -> Result<ModelResponse, ModelRuntimeFailure> {
        match self {
            Self::Ollama(runtime) => runtime.generate_with_control(request, control),
            Self::LlamaCpp(runtime) => runtime.generate_with_control(request, control),
        }
    }

    fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        match self {
            Self::Ollama(runtime) => runtime.preflight_request(request),
            Self::LlamaCpp(runtime) => runtime.preflight_request(request),
        }
    }

    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        match self {
            Self::Ollama(runtime) => runtime.health(),
            Self::LlamaCpp(runtime) => runtime.health(),
        }
    }

    fn runtime_id(&self) -> &str {
        match self {
            Self::Ollama(runtime) => runtime.runtime_id(),
            Self::LlamaCpp(runtime) => runtime.runtime_id(),
        }
    }

    fn model_id(&self) -> &str {
        match self {
            Self::Ollama(runtime) => runtime.model_id(),
            Self::LlamaCpp(runtime) => runtime.model_id(),
        }
    }

    fn context_tokens(&self, stage: PipelineStage) -> u32 {
        match self {
            Self::Ollama(runtime) => runtime.context_tokens(stage),
            Self::LlamaCpp(runtime) => runtime.context_tokens(stage),
        }
    }
}

impl QwenProfileRuntime {
    fn new(preset: &ModelPreset, settings_path: &Path) -> Result<Self, ModelRuntimeFailure> {
        Self::from_snapshot(
            &ModelProfileSnapshot {
                version: 2,
                preset_id: preset.preset_id.clone(),
                analysis: ModelStageProfileSnapshot {
                    runtime_kind: preset.analysis_runtime_kind,
                    profile_id: preset.analysis_profile_id.clone(),
                    model_name: preset.analysis_model.clone(),
                    model_digest: preset.analysis_digest.clone(),
                    context_tokens: preset.analysis_context_tokens,
                    tokenizer_version: preset.analysis_tokenizer_version.clone(),
                },
                verification: ModelStageProfileSnapshot {
                    runtime_kind: preset.verification_runtime_kind,
                    profile_id: preset.verification_profile_id.clone(),
                    model_name: preset.verification_model.clone(),
                    model_digest: preset.verification_digest.clone(),
                    context_tokens: preset.verification_context_tokens,
                    tokenizer_version: preset.verification_tokenizer_version.clone(),
                },
            },
            settings_path,
        )
    }

    fn from_snapshot(
        snapshot: &ModelProfileSnapshot,
        settings_path: &Path,
    ) -> Result<Self, ModelRuntimeFailure> {
        if !matches!(snapshot.version, 1 | 2) || snapshot.preset_id.trim().is_empty() {
            return Err(config_failure("Run model profile snapshot is invalid"));
        }
        if snapshot.version == 1
            && (snapshot.analysis.runtime_kind != ModelRuntimeKind::OllamaNative
                || snapshot.verification.runtime_kind != ModelRuntimeKind::OllamaNative)
        {
            return Err(config_failure("Historical profile runtime kind is invalid"));
        }
        let analysis_profile = admitted_snapshot_profile(&snapshot.analysis, false)?;
        let verification_profile = admitted_snapshot_profile(&snapshot.verification, true)?;
        validate_admitted_snapshot_preset(snapshot, analysis_profile, verification_profile)?;
        let requires_gguf_registration = [
            snapshot.analysis.runtime_kind,
            snapshot.verification.runtime_kind,
        ]
        .into_iter()
        .any(|runtime_kind| runtime_kind == ModelRuntimeKind::LlamaCppGguf);
        let settings = if requires_gguf_registration {
            load_settings(settings_path)?
        } else {
            ModelSettings::default()
        };
        let profile_lease = RuntimeProfileLease::acquire(snapshot)?;
        Self::build_product(
            snapshot.clone(),
            analysis_profile,
            verification_profile,
            &settings,
            settings_path,
            profile_lease,
        )
    }

    fn build_product(
        snapshot: ModelProfileSnapshot,
        analysis_profile: &'static QualifiedProfile,
        verification_profile: &'static QualifiedProfile,
        settings: &ModelSettings,
        settings_path: &Path,
        profile_lease: RuntimeProfileLease,
    ) -> Result<Self, ModelRuntimeFailure> {
        let runtime_parent = settings_path
            .parent()
            .ok_or_else(|| config_failure("Model settings path is invalid"))?;
        let analysis = stage_runtime(
            &snapshot.analysis,
            analysis_profile,
            settings,
            runtime_parent,
        )?;
        let verification = if snapshot.analysis == snapshot.verification {
            match &analysis {
                StageRuntime::LlamaCpp(runtime) => StageRuntime::LlamaCpp(Arc::clone(runtime)),
                StageRuntime::Ollama(_) => stage_runtime(
                    &snapshot.verification,
                    verification_profile,
                    settings,
                    runtime_parent,
                )?,
            }
        } else {
            stage_runtime(
                &snapshot.verification,
                verification_profile,
                settings,
                runtime_parent,
            )?
        };
        Ok(Self {
            preset_id: snapshot.preset_id.clone(),
            analysis,
            verification,
            snapshot,
            _profile_lease: Some(profile_lease),
        })
    }

    fn build_qualification(
        snapshot: ModelProfileSnapshot,
        analysis_family: QwenTokenizerFamily,
        verification_family: QwenTokenizerFamily,
    ) -> Result<Self, ModelRuntimeFailure> {
        Ok(Self {
            preset_id: snapshot.preset_id.clone(),
            analysis: StageRuntime::Ollama(OllamaRuntime::from_environment_profile(
                &snapshot.analysis.model_name,
                Some(&snapshot.analysis.model_digest),
                Some(analysis_family),
                snapshot.analysis.context_tokens,
            )?),
            verification: StageRuntime::Ollama(OllamaRuntime::from_environment_profile(
                &snapshot.verification.model_name,
                Some(&snapshot.verification.model_digest),
                Some(verification_family),
                snapshot.verification.context_tokens,
            )?),
            snapshot,
            _profile_lease: None,
        })
    }

    #[doc(hidden)]
    pub fn qualification_candidate(
        analysis_model: &str,
        verification_model: &str,
        context_tokens: u32,
    ) -> Result<Self, ModelRuntimeFailure> {
        let descriptors = OllamaRuntime::discovery_from_environment()?.installed_models()?;
        let (analysis, analysis_family) =
            qualification_stage_profile(&descriptors, analysis_model, context_tokens)?;
        let (verification, verification_family) =
            qualification_stage_profile(&descriptors, verification_model, context_tokens)?;
        Self::build_qualification(
            ModelProfileSnapshot {
                version: 1,
                preset_id: format!(
                    "qualification-{}-{}",
                    analysis.model_digest, verification.model_digest
                ),
                analysis,
                verification,
            },
            analysis_family,
            verification_family,
        )
    }

    fn runtime_for(&self, stage: &PipelineStage) -> &StageRuntime {
        if *stage == PipelineStage::Verify {
            &self.verification
        } else {
            &self.analysis
        }
    }
}

fn stage_runtime(
    snapshot: &ModelStageProfileSnapshot,
    profile: &'static QualifiedProfile,
    settings: &ModelSettings,
    runtime_parent: &Path,
) -> Result<StageRuntime, ModelRuntimeFailure> {
    match profile.runtime_kind {
        ModelRuntimeKind::OllamaNative => {
            prepare_for_ollama_runtime()?;
            Ok(StageRuntime::Ollama(
                OllamaRuntime::from_environment_profile(
                    &snapshot.model_name,
                    Some(&snapshot.model_digest),
                    Some(profile.tokenizer_family),
                    snapshot.context_tokens,
                )?,
            ))
        }
        ModelRuntimeKind::LlamaCppGguf => {
            let ollama = OllamaRuntime::discovery_from_environment()?;
            let admitted_ollama_digests = QUALIFIED_PROFILES
                .iter()
                .filter(|profile| profile.runtime_kind == ModelRuntimeKind::OllamaNative)
                .map(|profile| profile.digest)
                .collect::<Vec<_>>();
            ollama.ensure_models_not_resident_by_digest(&admitted_ollama_digests)?;
            let registration = settings
                .registered_ggufs
                .iter()
                .find(|registration| registration.digest == snapshot.model_digest)
                .ok_or_else(|| config_failure("Run GGUF registration is unavailable"))?;
            let server_digest = profile
                .runtime_binary_digest
                .ok_or_else(|| config_failure("Qualified GGUF runtime identity is missing"))?;
            Ok(StageRuntime::LlamaCpp(LlamaCppRuntime::shared(
                GgufRuntimeConfig {
                    model_path: registration.canonical_path.clone(),
                    runtime_parent: runtime_parent.to_path_buf(),
                    model_digest: snapshot.model_digest.clone(),
                    expected_size_bytes: registration.size_bytes,
                    expected_file_identity: registration.file_identity.clone(),
                    expected_server_digest: server_digest.to_string(),
                    expected_runtime_libraries: profile.runtime_libraries,
                    context_tokens: snapshot.context_tokens,
                },
            )?))
        }
        ModelRuntimeKind::InferenceGateway => Err(config_failure(
            "Gateway task profiles cannot be constructed by the direct runtime factory",
        )),
    }
}

fn qualification_stage_profile(
    descriptors: &[InstalledModelDescriptor],
    model_name: &str,
    context_tokens: u32,
) -> Result<(ModelStageProfileSnapshot, QwenTokenizerFamily), ModelRuntimeFailure> {
    let descriptor = descriptors
        .iter()
        .find(|descriptor| descriptor.name == model_name)
        .ok_or_else(|| config_failure("Qualification model is not installed"))?;
    if let Some(reason) = &descriptor.disabled_reason {
        return Err(config_failure(format!(
            "Qualification model is unavailable: {reason}"
        )));
    }
    let family = descriptor
        .tokenizer_family
        .ok_or_else(|| config_failure("Qualification model has no admitted tokenizer family"))?;
    if !descriptor
        .maximum_context_tokens
        .is_some_and(|maximum| context_tokens <= maximum)
    {
        return Err(config_failure(
            "Qualification context exceeds the discovered model maximum",
        ));
    }
    Ok((
        ModelStageProfileSnapshot {
            runtime_kind: ModelRuntimeKind::OllamaNative,
            profile_id: format!("qualification-{}", descriptor.digest),
            model_name: descriptor.name.clone(),
            model_digest: descriptor.digest.clone(),
            context_tokens,
            tokenizer_version: tokenizer_version(family).to_string(),
        },
        family,
    ))
}

fn admitted_snapshot_profile(
    snapshot: &ModelStageProfileSnapshot,
    verification: bool,
) -> Result<&'static QualifiedProfile, ModelRuntimeFailure> {
    let profile = QUALIFIED_PROFILES
        .iter()
        .find(|profile| profile.profile_id == snapshot.profile_id)
        .ok_or_else(|| config_failure("Run model profile is no longer admitted"))?;
    let stage_is_admitted = if verification {
        profile.verifier_rank.is_some()
    } else {
        profile.analysis
    };
    if !stage_is_admitted
        || snapshot.runtime_kind != profile.runtime_kind
        || snapshot.model_name.trim().is_empty()
        || snapshot.model_digest != profile.digest
        || snapshot.context_tokens != profile.safe_context_tokens
        || snapshot.tokenizer_version != profile.tokenizer_version
    {
        return Err(config_failure(
            "Run model profile snapshot does not match its qualified profile",
        ));
    }
    Ok(profile)
}

fn validate_admitted_snapshot_preset(
    snapshot: &ModelProfileSnapshot,
    analysis_profile: &'static QualifiedProfile,
    verification_profile: &'static QualifiedProfile,
) -> Result<(), ModelRuntimeFailure> {
    let full_is_admitted = analysis_profile.full
        && analysis_profile.profile_id == verification_profile.profile_id
        && snapshot.analysis == snapshot.verification
        && snapshot.preset_id == format!("full-{}", analysis_profile.profile_id);
    let strongest_verifier_rank = QUALIFIED_PROFILES
        .iter()
        .filter_map(|profile| profile.verifier_rank)
        .max();
    let hybrid_is_admitted = analysis_profile.hybrid_analysis
        && analysis_profile.profile_id != verification_profile.profile_id
        && verification_profile.verifier_rank == strongest_verifier_rank
        && snapshot.preset_id
            == format!(
                "hybrid-{}-{}",
                analysis_profile.profile_id, verification_profile.profile_id
            );
    if full_is_admitted || hybrid_is_admitted {
        Ok(())
    } else {
        Err(config_failure(
            "Run model profile snapshot does not match an admitted preset",
        ))
    }
}

impl ModelRuntime for QwenProfileRuntime {
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        self.runtime_for(&request.stage).generate(request)
    }

    fn generate_with_control(
        &self,
        request: &ModelRequest,
        control: &dyn ExecutionControl,
    ) -> Result<ModelResponse, ModelRuntimeFailure> {
        self.runtime_for(&request.stage)
            .generate_with_control(request, control)
    }

    fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        self.runtime_for(&request.stage).preflight_request(request)
    }

    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        self.analysis.health()?;
        if self.analysis.model_id() != self.verification.model_id() {
            self.verification.health()?;
        }
        Ok(())
    }

    fn runtime_id(&self) -> &str {
        "qwen-qualified-profile"
    }

    fn model_id(&self) -> &str {
        &self.preset_id
    }

    fn runtime_id_for_stage(&self, stage: PipelineStage) -> &str {
        self.runtime_for(&stage).runtime_id()
    }

    fn model_id_for_stage(&self, stage: PipelineStage) -> &str {
        self.runtime_for(&stage).model_id()
    }

    fn context_tokens(&self, stage: PipelineStage) -> u32 {
        self.runtime_for(&stage).context_tokens(stage)
    }

    fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
        Some(self.snapshot.clone())
    }
}

fn config_failure(message: impl Into<String>) -> ModelRuntimeFailure {
    ModelRuntimeFailure {
        code: "MODEL_CONFIG_INVALID".to_string(),
        message: message.into(),
        recoverable: false,
        request_attempts: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::control::CancellationToken;

    fn qualified_snapshot() -> ModelProfileSnapshot {
        let profile = QUALIFIED_PROFILES[0];
        let stage = ModelStageProfileSnapshot {
            runtime_kind: profile.runtime_kind,
            profile_id: profile.profile_id.to_string(),
            model_name: "renamed-baseline:latest".to_string(),
            model_digest: profile.digest.to_string(),
            context_tokens: profile.safe_context_tokens,
            tokenizer_version: profile.tokenizer_version.to_string(),
        };
        ModelProfileSnapshot {
            version: 1,
            preset_id: DEFAULT_PRESET_ID.to_string(),
            analysis: stage.clone(),
            verification: stage,
        }
    }

    #[test]
    fn snapshot_admission_requires_one_exact_product_preset() {
        let ollama = qualified_snapshot();
        let ollama_profile = admitted_snapshot_profile(&ollama.analysis, false).unwrap();
        let ollama_verifier = admitted_snapshot_profile(&ollama.verification, true).unwrap();
        validate_admitted_snapshot_preset(&ollama, ollama_profile, ollama_verifier).unwrap();

        let direct_profile = QUALIFIED_PROFILES
            .iter()
            .find(|profile| profile.runtime_kind == ModelRuntimeKind::LlamaCppGguf)
            .unwrap();
        let direct_stage = ModelStageProfileSnapshot {
            runtime_kind: direct_profile.runtime_kind,
            profile_id: direct_profile.profile_id.to_string(),
            model_name: "jack.gguf".to_string(),
            model_digest: direct_profile.digest.to_string(),
            context_tokens: direct_profile.safe_context_tokens,
            tokenizer_version: direct_profile.tokenizer_version.to_string(),
        };
        let direct = ModelProfileSnapshot {
            version: 2,
            preset_id: format!("full-{}", direct_profile.profile_id),
            analysis: direct_stage.clone(),
            verification: direct_stage,
        };
        validate_admitted_snapshot_preset(&direct, direct_profile, direct_profile).unwrap();

        let mut cross_runtime = direct;
        cross_runtime.verification = ollama.verification.clone();
        assert_eq!(
            validate_admitted_snapshot_preset(&cross_runtime, direct_profile, ollama_verifier)
                .unwrap_err()
                .code,
            "MODEL_CONFIG_INVALID"
        );

        let mut wrong_preset = ollama;
        wrong_preset.preset_id = "full-not-the-qualified-profile".to_string();
        assert_eq!(
            validate_admitted_snapshot_preset(&wrong_preset, ollama_profile, ollama_verifier)
                .unwrap_err()
                .code,
            "MODEL_CONFIG_INVALID"
        );
    }

    fn descriptor(
        name: &str,
        digest: &str,
        family: Option<QwenTokenizerFamily>,
        context: Option<u32>,
    ) -> InstalledModelDescriptor {
        InstalledModelDescriptor {
            name: name.to_string(),
            digest: digest.to_string(),
            size_bytes: 1,
            architecture: Some("qwen3moe".to_string()),
            tokenizer_family: family,
            parameter_size: Some("30B-A3B".to_string()),
            quantization_level: Some("Q4_K_S".to_string()),
            maximum_context_tokens: context,
            disabled_reason: None,
        }
    }

    fn catalog_for(
        settings: ModelSettings,
        descriptors: Vec<InstalledModelDescriptor>,
        profiles: &'static [QualifiedProfile],
    ) -> Result<ModelCatalog, ModelRuntimeFailure> {
        catalog_from_descriptors(
            settings,
            descriptors
                .into_iter()
                .map(|descriptor| (descriptor, ModelRuntimeKind::OllamaNative))
                .collect(),
            profiles,
            Vec::new(),
        )
    }

    #[test]
    fn exact_digest_and_context_admit_only_the_qualified_preset() {
        let settings = ModelSettings::default();
        let qualified = descriptor(
            "renamed-baseline:latest",
            QUALIFIED_PROFILES[0].digest,
            Some(QwenTokenizerFamily::Qwen3),
            Some(262_144),
        );
        let catalog = catalog_for(settings, vec![qualified], QUALIFIED_PROFILES).unwrap();
        assert_eq!(catalog.presets.len(), 1);
        assert_eq!(catalog.presets[0].analysis_model, "renamed-baseline:latest");
        assert_eq!(catalog.presets[0].analysis_context_tokens, 8_192);

        for bad in [
            descriptor(
                "same-name:latest",
                "wrong-digest",
                Some(QwenTokenizerFamily::Qwen3),
                Some(262_144),
            ),
            descriptor(
                "wrong-family:latest",
                QUALIFIED_PROFILES[0].digest,
                Some(QwenTokenizerFamily::Qwen35),
                Some(262_144),
            ),
            descriptor(
                "short-context:latest",
                QUALIFIED_PROFILES[0].digest,
                Some(QwenTokenizerFamily::Qwen3),
                Some(4_096),
            ),
        ] {
            let catalog =
                catalog_for(ModelSettings::default(), vec![bad], QUALIFIED_PROFILES).unwrap();
            assert!(!catalog.selected_preset_available);
            assert!(catalog.presets.is_empty());
        }
    }

    #[test]
    fn qualified_digest_aliases_share_one_canonical_preset_identity() {
        let later_alias = descriptor(
            "z-baseline:latest",
            QUALIFIED_PROFILES[0].digest,
            Some(QwenTokenizerFamily::Qwen3),
            Some(262_144),
        );
        let canonical_alias = descriptor(
            "a-baseline:latest",
            QUALIFIED_PROFILES[0].digest,
            Some(QwenTokenizerFamily::Qwen3),
            Some(262_144),
        );
        let catalog = catalog_for(
            ModelSettings::default(),
            vec![later_alias, canonical_alias],
            QUALIFIED_PROFILES,
        )
        .expect("qualified aliases should build a catalog");

        assert_eq!(catalog.installed_models.len(), 2);
        assert_eq!(catalog.presets.len(), 1);
        assert_eq!(catalog.presets[0].preset_id, DEFAULT_PRESET_ID);
        assert_eq!(catalog.presets[0].analysis_model, "a-baseline:latest");
        assert_eq!(catalog.presets[0].verification_model, "a-baseline:latest");
        assert!(catalog.selected_preset_available);
    }

    #[test]
    fn stale_selection_keeps_qualified_recovery_presets_visible() {
        let settings = ModelSettings {
            version: SETTINGS_VERSION,
            selected_preset_id: "removed-profile".to_string(),
            ..ModelSettings::default()
        };
        let qualified = descriptor(
            "baseline:latest",
            QUALIFIED_PROFILES[0].digest,
            Some(QwenTokenizerFamily::Qwen3),
            Some(262_144),
        );
        let catalog = catalog_for(settings, vec![qualified], QUALIFIED_PROFILES).unwrap();
        assert!(!catalog.selected_preset_available);
        assert_eq!(catalog.presets.len(), 1);
        assert_eq!(catalog.selected_preset_id, "removed-profile");
    }

    #[test]
    fn settings_round_trip_atomically_and_malformed_values_fail_closed() {
        let directory =
            std::env::temp_dir().join(format!("doc-sum-model-settings-{}", Uuid::new_v4()));
        let path = settings_path(&directory);
        assert_eq!(load_settings(&path).unwrap(), ModelSettings::default());
        let saved = save_selected_preset(&path, DEFAULT_PRESET_ID).unwrap();
        assert_eq!(load_settings(&path).unwrap(), saved);
        let replaced = save_selected_preset(&path, "replacement-qualified-preset").unwrap();
        assert_eq!(load_settings(&path).unwrap(), replaced);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::write(&path, b"{\"version\":1,\"selectedPresetId\":false}").unwrap();
        assert_eq!(
            load_settings(&path).unwrap_err().code,
            "MODEL_CONFIG_INVALID"
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn version_one_settings_migrate_in_memory_and_upgrade_on_write() {
        let directory = tempfile::tempdir().unwrap();
        let path = settings_path(directory.path());
        fs::write(
            &path,
            format!("{{\"version\":1,\"selectedPresetId\":\"{DEFAULT_PRESET_ID}\"}}"),
        )
        .unwrap();

        let loaded = load_settings(&path).unwrap();
        assert_eq!(loaded.version, SETTINGS_VERSION);
        assert!(loaded.registered_ggufs.is_empty());
        let saved = save_selected_preset(&path, DEFAULT_PRESET_ID).unwrap();
        assert_eq!(saved, loaded);
        let persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted["version"], SETTINGS_VERSION);
        assert_eq!(persisted["registeredGgufs"], serde_json::json!([]));
    }

    #[test]
    fn version_two_settings_preserve_direct_selection_on_upgrade() {
        let directory = tempfile::tempdir().unwrap();
        let path = settings_path(directory.path());
        fs::write(
            &path,
            format!(
                "{{\"version\":2,\"selectedPresetId\":\"{DEFAULT_PRESET_ID}\",\"registeredGgufs\":[]}}"
            ),
        )
        .unwrap();

        let loaded = load_settings(&path).unwrap();
        assert_eq!(loaded.version, SETTINGS_VERSION);
        assert_eq!(loaded.selected_source, InferenceSource::Direct);
        assert_eq!(loaded.selected_preset_id, DEFAULT_PRESET_ID);
        assert!(loaded.gateway.is_none());

        save_selected_preset(&path, DEFAULT_PRESET_ID).unwrap();
        let persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted["version"], SETTINGS_VERSION);
        assert_eq!(persisted["selectedSource"], "direct");
        assert_eq!(persisted["gateway"], serde_json::Value::Null);
    }

    #[cfg(unix)]
    #[test]
    fn gateway_selection_validates_before_atomic_source_switch() {
        let directory = tempfile::tempdir().unwrap();
        let path = settings_path(directory.path());
        let token = directory.path().join("document-summarizer.token");
        let ca = directory.path().join("office-ca.pem");

        let rejected =
            configure_gateway_with(&path, "https://inference.office:8787", &token, &ca, |_| {
                Err(config_failure("fixture gateway unavailable"))
            })
            .unwrap_err();
        assert_eq!(rejected.code, "MODEL_CONFIG_INVALID");
        assert!(!path.exists());

        let configured =
            configure_gateway_with(&path, "https://inference.office:8787", &token, &ca, |_| {
                Ok(())
            })
            .unwrap();
        assert_eq!(configured.selected_source, InferenceSource::Gateway);
        assert_eq!(
            configured.gateway.as_ref().unwrap().base_url,
            "https://inference.office:8787"
        );

        let direct = save_selected_preset(&path, DEFAULT_PRESET_ID).unwrap();
        assert_eq!(direct.selected_source, InferenceSource::Direct);
        assert!(direct.gateway.is_some());

        let restored = select_gateway_with(&path, |_| Ok(())).unwrap();
        assert_eq!(restored.selected_source, InferenceSource::Gateway);
        assert_eq!(load_settings(&path).unwrap(), restored);
    }

    #[test]
    fn gateway_metadata_and_view_fail_closed_without_disclosing_paths() {
        let mut settings = ModelSettings {
            selected_source: InferenceSource::Gateway,
            ..ModelSettings::default()
        };
        assert_eq!(
            validate_settings(&settings).unwrap_err().code,
            "MODEL_CONFIG_INVALID"
        );

        settings.gateway = Some(GatewayConnectionSettings {
            base_url: "https://inference.office:8787".to_string(),
            token_file: PathBuf::from("relative.token"),
            ca_file: PathBuf::from("relative-ca.pem"),
        });
        assert_eq!(
            validate_settings(&settings).unwrap_err().code,
            "MODEL_CONFIG_INVALID"
        );

        let root = std::env::temp_dir().join("private-gateway-parent");
        settings.gateway = Some(GatewayConnectionSettings {
            base_url: "https://inference.office:8787".to_string(),
            token_file: root.join("document-summarizer.token"),
            ca_file: root.join("office-ca.pem"),
        });
        validate_settings(&settings).unwrap();
        let view = gateway_connection_view(settings.gateway.as_ref());
        assert!(view.configured);
        assert_eq!(
            view.base_url.as_deref(),
            Some("https://inference.office:8787")
        );
        assert_eq!(
            view.token_file_name.as_deref(),
            Some("document-summarizer.token")
        );
        assert_eq!(view.ca_file_name.as_deref(), Some("office-ca.pem"));
        assert!(!serde_json::to_string(&view)
            .unwrap()
            .contains("private-gateway-parent"));
    }

    #[test]
    fn selected_direct_profile_reconstructs_without_ollama_discovery() {
        let profile = QUALIFIED_PROFILES[1];
        let registration = RegisteredGguf {
            canonical_path: PathBuf::from("/qualified/jack.gguf"),
            file_name: "jack.gguf".to_string(),
            digest: profile.digest.to_string(),
            size_bytes: 1,
            file_identity: FileIdentity {
                device: 1,
                inode: 1,
                size_bytes: 1,
                modified_seconds: 1,
                modified_nanoseconds: 0,
                changed_seconds: 1,
                changed_nanoseconds: 0,
            },
        };
        let settings = ModelSettings {
            version: SETTINGS_VERSION,
            selected_preset_id: format!("full-{}", profile.profile_id),
            registered_ggufs: vec![registration],
            ..ModelSettings::default()
        };
        let snapshot = selected_direct_snapshot(&settings)
            .unwrap()
            .expect("direct selection should reconstruct from its registration");
        assert_eq!(snapshot.version, 2);
        assert_eq!(
            snapshot.analysis.runtime_kind,
            ModelRuntimeKind::LlamaCppGguf
        );
        assert_eq!(snapshot.analysis.model_name, "jack.gguf");
        assert_eq!(snapshot.analysis.model_digest, profile.digest);
        assert_eq!(snapshot.analysis, snapshot.verification);

        let missing = ModelSettings {
            registered_ggufs: Vec::new(),
            ..settings
        };
        assert_eq!(
            selected_direct_snapshot(&missing).unwrap_err().code,
            "MODEL_CONFIG_INVALID"
        );
        assert!(selected_direct_snapshot(&ModelSettings::default())
            .unwrap()
            .is_none());
    }

    #[test]
    fn registration_is_regular_file_only_and_deduplicates_exact_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let settings = settings_path(directory.path());
        let first = directory.path().join("first.gguf");
        let second = directory.path().join("second.gguf");
        fs::write(&first, b"qualified bytes").unwrap();
        fs::write(&second, b"qualified bytes").unwrap();

        let first_registration = register_gguf(&settings, &first).unwrap();
        assert_eq!(first_registration.registered_ggufs.len(), 1);
        assert_eq!(
            first_registration.registered_ggufs[0].canonical_path,
            fs::canonicalize(&first).unwrap()
        );
        let second_registration = register_gguf(&settings, &second).unwrap();
        assert_eq!(second_registration.registered_ggufs.len(), 1);
        assert_eq!(
            second_registration.registered_ggufs[0].canonical_path,
            fs::canonicalize(&second).unwrap()
        );

        let wrong_extension = directory.path().join("model.bin");
        fs::write(&wrong_extension, b"qualified bytes").unwrap();
        assert_eq!(
            register_gguf(&settings, &wrong_extension).unwrap_err().code,
            "MODEL_CONFIG_INVALID"
        );
        let directory_model = directory.path().join("directory.gguf");
        fs::create_dir(&directory_model).unwrap();
        assert_eq!(
            register_gguf(&settings, &directory_model).unwrap_err().code,
            "MODEL_CONFIG_INVALID"
        );
        #[cfg(unix)]
        {
            let symlink = directory.path().join("link.gguf");
            std::os::unix::fs::symlink(&first, &symlink).unwrap();
            assert_eq!(
                register_gguf(&settings, &symlink).unwrap_err().code,
                "MODEL_NOT_AVAILABLE"
            );
        }
    }

    #[test]
    fn concurrent_registrations_preserve_every_distinct_model() {
        let directory = Arc::new(tempfile::tempdir().unwrap());
        let settings = settings_path(directory.path());
        let mut workers = Vec::new();
        for index in 0..8_u8 {
            let directory = Arc::clone(&directory);
            let settings = settings.clone();
            workers.push(std::thread::spawn(move || {
                let model = directory.path().join(format!("model-{index}.gguf"));
                fs::write(&model, [index + 1]).unwrap();
                register_gguf(&settings, &model).unwrap();
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        let registrations = load_settings(&settings).unwrap().registered_ggufs;
        assert_eq!(registrations.len(), 8);
        assert_eq!(
            registrations
                .iter()
                .map(|registration| registration.digest.as_str())
                .collect::<HashSet<_>>()
                .len(),
            8
        );
    }

    #[test]
    fn preset_deduplication_keeps_equal_digests_from_distinct_runtimes() {
        const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        const OLLAMA: QualifiedProfile = QualifiedProfile {
            profile_id: "same-digest-ollama",
            digest: DIGEST,
            tokenizer_family: QwenTokenizerFamily::Qwen3,
            tokenizer_version: QWEN3_TOKENIZER_VERSION,
            safe_context_tokens: 8_192,
            analysis: true,
            verifier_rank: Some(1),
            full: true,
            hybrid_analysis: false,
            runtime_kind: ModelRuntimeKind::OllamaNative,
            runtime_binary_digest: None,
            runtime_libraries: &[],
            label: "Ollama fixture",
        };
        const DIRECT: QualifiedProfile = QualifiedProfile {
            profile_id: "same-digest-direct",
            runtime_kind: ModelRuntimeKind::LlamaCppGguf,
            label: "Direct fixture",
            ..OLLAMA
        };
        const PROFILES: &[QualifiedProfile] = &[OLLAMA, DIRECT];
        let settings = ModelSettings::default();
        let sources = vec![
            (
                descriptor(
                    "ollama:latest",
                    DIGEST,
                    Some(QwenTokenizerFamily::Qwen3),
                    Some(8_192),
                ),
                ModelRuntimeKind::OllamaNative,
            ),
            (
                descriptor(
                    "direct.gguf",
                    DIGEST,
                    Some(QwenTokenizerFamily::Qwen3),
                    Some(8_192),
                ),
                ModelRuntimeKind::LlamaCppGguf,
            ),
        ];
        let catalog = catalog_from_descriptors(settings, sources, PROFILES, Vec::new()).unwrap();
        assert_eq!(catalog.presets.len(), 2);
        assert_ne!(
            catalog.presets[0].analysis_runtime_kind,
            catalog.presets[1].analysis_runtime_kind
        );
    }

    #[test]
    fn registration_bounds_hold_on_both_sides() {
        let maximum_path = format!("/{}.gguf", "a".repeat(MAX_REGISTERED_PATH_BYTES - 6));
        assert_eq!(maximum_path.len(), MAX_REGISTERED_PATH_BYTES);
        assert!(validate_selected_gguf_path(Path::new(&maximum_path)).is_ok());
        let oversized_path = format!("/{}.gguf", "a".repeat(MAX_REGISTERED_PATH_BYTES - 5));
        assert_eq!(oversized_path.len(), MAX_REGISTERED_PATH_BYTES + 1);
        assert_eq!(
            validate_selected_gguf_path(Path::new(&oversized_path))
                .unwrap_err()
                .code,
            "MODEL_CONFIG_INVALID"
        );

        let registration = |index: usize, label_length: usize| RegisteredGguf {
            canonical_path: PathBuf::from(format!("/model-{index}.gguf")),
            file_name: "m".repeat(label_length),
            digest: format!("{index:064x}"),
            size_bytes: 1,
            file_identity: FileIdentity {
                device: 1,
                inode: u64::try_from(index + 1).unwrap(),
                size_bytes: 1,
                modified_seconds: 1,
                modified_nanoseconds: 0,
                changed_seconds: 1,
                changed_nanoseconds: 0,
            },
        };
        let maximum = ModelSettings {
            version: SETTINGS_VERSION,
            selected_preset_id: DEFAULT_PRESET_ID.to_string(),
            registered_ggufs: (0..MAX_REGISTERED_GGUFS)
                .map(|index| registration(index, MAX_REGISTERED_LABEL_BYTES))
                .collect(),
            ..ModelSettings::default()
        };
        assert!(validate_settings(&maximum).is_ok());
        let mut too_many = maximum.clone();
        too_many.registered_ggufs.push(registration(
            MAX_REGISTERED_GGUFS,
            MAX_REGISTERED_LABEL_BYTES,
        ));
        assert_eq!(
            validate_settings(&too_many).unwrap_err().code,
            "MODEL_CONFIG_INVALID"
        );
        let mut oversized_label = maximum;
        oversized_label.registered_ggufs[0].file_name = "m".repeat(MAX_REGISTERED_LABEL_BYTES + 1);
        assert_eq!(
            validate_settings(&oversized_label).unwrap_err().code,
            "MODEL_CONFIG_INVALID"
        );
    }

    #[test]
    fn analysis_only_profile_routes_verification_to_the_strongest_qualified_verifier() {
        const SMALL: QualifiedProfile = QualifiedProfile {
            profile_id: "qwen35-small-analysis-v1",
            digest: "small-analysis-digest",
            tokenizer_family: QwenTokenizerFamily::Qwen35,
            tokenizer_version: crate::pipeline::qwen_tokenizer::QWEN35_TOKENIZER_VERSION,
            safe_context_tokens: 8_192,
            analysis: true,
            verifier_rank: None,
            full: false,
            hybrid_analysis: true,
            runtime_kind: ModelRuntimeKind::OllamaNative,
            runtime_binary_digest: None,
            runtime_libraries: &[],
            label: "Small analysis candidate",
        };
        const PROFILES: &[QualifiedProfile] = &[QUALIFIED_PROFILES[0], SMALL];
        let baseline = descriptor(
            "baseline:latest",
            QUALIFIED_PROFILES[0].digest,
            Some(QwenTokenizerFamily::Qwen3),
            Some(262_144),
        );
        let small = descriptor(
            "small:latest",
            SMALL.digest,
            Some(QwenTokenizerFamily::Qwen35),
            Some(32_768),
        );
        let catalog =
            catalog_for(ModelSettings::default(), vec![small, baseline], PROFILES).unwrap();
        let hybrid = catalog
            .presets
            .iter()
            .find(|preset| preset.mode == ModelPresetMode::Hybrid)
            .expect("analysis-only model should produce a hybrid preset");
        assert_eq!(hybrid.analysis_digest, SMALL.digest);
        assert_eq!(hybrid.verification_digest, QUALIFIED_PROFILES[0].digest);
        assert!(catalog
            .presets
            .iter()
            .all(|preset| preset.verification_digest == QUALIFIED_PROFILES[0].digest));
    }

    #[test]
    fn persisted_snapshot_rebuilds_its_exact_runtime_and_rejects_profile_drift() {
        let snapshot = qualified_snapshot();
        let settings = std::env::temp_dir().join(format!("missing-settings-{}", Uuid::new_v4()));
        let runtime = runtime_from_snapshot(&snapshot, &settings, &settings.with_extension("db"))
            .expect("an admitted immutable snapshot should rebuild its runtime");
        assert_eq!(runtime.profile_snapshot(), Some(snapshot.clone()));
        assert_eq!(
            runtime.model_id_for_stage(PipelineStage::Analyze),
            "renamed-baseline:latest"
        );

        for changed in [
            ModelProfileSnapshot {
                version: 3,
                ..snapshot.clone()
            },
            ModelProfileSnapshot {
                analysis: ModelStageProfileSnapshot {
                    context_tokens: 4_096,
                    ..snapshot.analysis.clone()
                },
                ..snapshot.clone()
            },
            ModelProfileSnapshot {
                verification: ModelStageProfileSnapshot {
                    model_digest: "changed-digest".to_string(),
                    ..snapshot.verification.clone()
                },
                ..snapshot.clone()
            },
        ] {
            let error = runtime_from_snapshot(&changed, &settings, &settings.with_extension("db"))
                .err()
                .expect("profile drift must fail before inference");
            assert_eq!(error.code, "MODEL_CONFIG_INVALID");
        }
    }

    #[test]
    fn ollama_snapshot_reconstruction_ignores_malformed_gguf_settings() {
        let directory = tempfile::tempdir().unwrap();
        let settings = settings_path(directory.path());
        fs::write(&settings, b"not-json").unwrap();
        let snapshot = qualified_snapshot();

        let runtime = runtime_from_snapshot(&snapshot, &settings, &settings.with_extension("db"))
            .expect("Ollama-only snapshots must not read GGUF settings");
        assert_eq!(runtime.profile_snapshot(), Some(snapshot));
    }

    #[test]
    fn qualification_runtime_stays_separate_from_product_profile_admission() {
        let snapshot = ModelProfileSnapshot {
            version: 1,
            preset_id: "qualification-candidate".to_string(),
            analysis: ModelStageProfileSnapshot {
                runtime_kind: ModelRuntimeKind::OllamaNative,
                profile_id: "qualification-analysis".to_string(),
                model_name: "candidate:latest".to_string(),
                model_digest: "candidate-digest".to_string(),
                context_tokens: 8_192,
                tokenizer_version: crate::pipeline::qwen_tokenizer::QWEN35_TOKENIZER_VERSION
                    .to_string(),
            },
            verification: qualified_snapshot().verification,
        };
        let settings = std::env::temp_dir().join(format!("missing-settings-{}", Uuid::new_v4()));
        assert!(
            runtime_from_snapshot(&snapshot, &settings, &settings.with_extension("db")).is_err()
        );
        let runtime = QwenProfileRuntime::build_qualification(
            snapshot,
            QwenTokenizerFamily::Qwen35,
            QwenTokenizerFamily::Qwen3,
        )
        .expect("the hidden qualification harness must accept an explicit candidate");
        assert_eq!(
            runtime.model_id_for_stage(PipelineStage::Analyze),
            "candidate:latest"
        );
        assert_eq!(
            runtime.model_id_for_stage(PipelineStage::Verify),
            "renamed-baseline:latest"
        );
    }

    #[test]
    fn runtime_routes_by_typed_stage_only() {
        let runtime = QwenProfileRuntime {
            preset_id: "routing-test".to_string(),
            snapshot: ModelProfileSnapshot {
                version: 1,
                preset_id: "routing-test".to_string(),
                analysis: ModelStageProfileSnapshot {
                    runtime_kind: ModelRuntimeKind::OllamaNative,
                    profile_id: "analysis".to_string(),
                    model_name: "analysis-model".to_string(),
                    model_digest: "analysis-digest".to_string(),
                    context_tokens: 8_192,
                    tokenizer_version: QWEN3_TOKENIZER_VERSION.to_string(),
                },
                verification: ModelStageProfileSnapshot {
                    runtime_kind: ModelRuntimeKind::OllamaNative,
                    profile_id: "verification".to_string(),
                    model_name: "verification-model".to_string(),
                    model_digest: "verification-digest".to_string(),
                    context_tokens: 16_384,
                    tokenizer_version: QWEN3_TOKENIZER_VERSION.to_string(),
                },
            },
            analysis: StageRuntime::Ollama(
                OllamaRuntime::new_with_context(
                    "http://127.0.0.1:11434/",
                    "analysis-model",
                    8_192,
                    std::time::Duration::from_secs(1),
                    None,
                )
                .unwrap(),
            ),
            verification: StageRuntime::Ollama(
                OllamaRuntime::new_with_context(
                    "http://127.0.0.1:11434/",
                    "verification-model",
                    16_384,
                    std::time::Duration::from_secs(1),
                    None,
                )
                .unwrap(),
            ),
            _profile_lease: None,
        };
        assert_eq!(
            runtime.model_id_for_stage(PipelineStage::Analyze),
            "analysis-model"
        );
        assert_eq!(
            runtime.model_id_for_stage(PipelineStage::Synthesize),
            "analysis-model"
        );
        assert_eq!(
            runtime.model_id_for_stage(PipelineStage::Verify),
            "verification-model"
        );
        assert_eq!(runtime.context_tokens(PipelineStage::Analyze), 8_192);
        assert_eq!(runtime.context_tokens(PipelineStage::Verify), 16_384);

        let control = CancellationToken::new();
        control.request();
        for stage in [
            PipelineStage::Analyze,
            PipelineStage::Synthesize,
            PipelineStage::Verify,
        ] {
            let failure = runtime
                .generate_with_control(
                    &ModelRequest {
                        stage,
                        ordinal: 0,
                        system_prompt: "system".to_string(),
                        user_prompt: "user".to_string(),
                        seed: 42,
                        max_output_tokens: 1,
                        output_format: Default::default(),
                    },
                    &control,
                )
                .unwrap_err();
            assert_eq!(failure.code, "MODEL_REQUEST_CANCELLED");
        }
    }

    #[test]
    fn active_profile_lease_allows_same_profile_and_blocks_a_different_one() {
        let first = RuntimeProfileKey::from(&qualified_snapshot());
        let mut changed_snapshot = qualified_snapshot();
        changed_snapshot.verification.context_tokens += 1;
        let different = RuntimeProfileKey::from(&changed_snapshot);
        let mut active = None;

        claim_profile_lease(&mut active, &first).unwrap();
        claim_profile_lease(&mut active, &first).unwrap();
        assert_eq!(
            claim_profile_lease(&mut active, &different)
                .unwrap_err()
                .code,
            "MODEL_RUNTIME_BUSY"
        );
        release_profile_lease(&mut active, &first);
        assert!(claim_profile_lease(&mut active, &different).is_err());
        release_profile_lease(&mut active, &first);
        claim_profile_lease(&mut active, &different).unwrap();
        release_profile_lease(&mut active, &different);
        assert!(active.is_none());
    }
}
