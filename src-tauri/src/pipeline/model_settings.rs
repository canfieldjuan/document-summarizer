use crate::pipeline::contracts::{
    ModelProfileSnapshot, ModelRequest, ModelResponse, ModelRuntime, ModelRuntimeFailure,
    ModelStageProfileSnapshot, PipelineStage,
};
use crate::pipeline::model::{InstalledModelDescriptor, OllamaRuntime, QwenTokenizerFamily};
use crate::pipeline::qwen_tokenizer::{tokenizer_version, QWEN3_TOKENIZER_VERSION};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::Builder;
#[cfg(test)]
use uuid::Uuid;

const SETTINGS_VERSION: u32 = 1;
pub const SETTINGS_FILE_NAME: &str = "model-settings-v1.json";
const DEFAULT_PRESET_ID: &str = "full-qwen3-30b-a3b-q4ks-v1";

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
    label: &'static str,
}

// Qualifications are exact-digest evidence, not name-based capability guesses.
// Candidate entries are added only after the unchanged corpus gates pass.
const QUALIFIED_PROFILES: &[QualifiedProfile] = &[QualifiedProfile {
    profile_id: "qwen3-30b-a3b-q4ks-v1",
    digest: "1eda56426671cdf365913097543c2253a73c57e35b12741306689968d7f70292",
    tokenizer_family: QwenTokenizerFamily::Qwen3,
    tokenizer_version: QWEN3_TOKENIZER_VERSION,
    safe_context_tokens: 8_192,
    analysis: true,
    verifier_rank: Some(100),
    full: true,
    label: "Qwen 3 30B-A3B",
}];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelSettings {
    version: u32,
    pub selected_preset_id: String,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            selected_preset_id: DEFAULT_PRESET_ID.to_string(),
        }
    }
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPreset {
    pub preset_id: String,
    pub label: String,
    pub mode: ModelPresetMode,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub selected_preset_id: String,
    pub selected_preset_available: bool,
    pub installed_models: Vec<ModelOption>,
    pub presets: Vec<ModelPreset>,
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
    let settings: ModelSettings = serde_json::from_slice(&bytes)
        .map_err(|_| config_failure("Model settings are malformed"))?;
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
    let settings = ModelSettings {
        version: SETTINGS_VERSION,
        selected_preset_id: selected_preset_id.to_string(),
    };
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
    Ok(settings)
}

pub fn catalog(path: &Path) -> Result<ModelCatalog, ModelRuntimeFailure> {
    let settings = load_settings(path)?;
    let discovery = OllamaRuntime::discovery_from_environment()?;
    catalog_from_descriptors(settings, discovery.installed_models()?, QUALIFIED_PROFILES)
}

fn catalog_from_descriptors(
    settings: ModelSettings,
    mut descriptors: Vec<InstalledModelDescriptor>,
    profiles: &'static [QualifiedProfile],
) -> Result<ModelCatalog, ModelRuntimeFailure> {
    descriptors.sort_by(|left, right| left.name.cmp(&right.name));
    let mut installed_models = Vec::with_capacity(descriptors.len());
    for mut descriptor in descriptors {
        let profile = profiles
            .iter()
            .find(|profile| profile.digest == descriptor.digest);
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
            descriptor,
        });
    }
    let verifier = strongest_verifier(&installed_models, profiles);
    let mut presets = Vec::new();
    for option in &installed_models {
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
        if profile.analysis {
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
    Ok(ModelCatalog {
        selected_preset_id: settings.selected_preset_id,
        selected_preset_available,
        installed_models,
        presets,
    })
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

fn strongest_verifier<'a>(
    options: &'a [ModelOption],
    profiles: &'static [QualifiedProfile],
) -> Option<&'a ModelOption> {
    options
        .iter()
        .filter(|option| option.descriptor.disabled_reason.is_none())
        .filter_map(|option| {
            profile_for_option(option, profiles)
                .and_then(|profile| profile.verifier_rank.map(|rank| (rank, option)))
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
    })
}

pub fn runtime_from_settings(path: &Path) -> Result<QwenProfileRuntime, ModelRuntimeFailure> {
    let catalog = catalog(path)?;
    let preset = catalog
        .presets
        .iter()
        .find(|preset| preset.preset_id == catalog.selected_preset_id)
        .ok_or_else(|| config_failure("Selected model preset is unavailable"))?;
    QwenProfileRuntime::new(preset)
}

pub fn runtime_from_snapshot(
    snapshot: &ModelProfileSnapshot,
) -> Result<QwenProfileRuntime, ModelRuntimeFailure> {
    QwenProfileRuntime::from_snapshot(snapshot)
}

pub struct QwenProfileRuntime {
    preset_id: String,
    snapshot: ModelProfileSnapshot,
    analysis: OllamaRuntime,
    verification: OllamaRuntime,
}

impl QwenProfileRuntime {
    fn new(preset: &ModelPreset) -> Result<Self, ModelRuntimeFailure> {
        Self::from_snapshot(&ModelProfileSnapshot {
            version: 1,
            preset_id: preset.preset_id.clone(),
            analysis: ModelStageProfileSnapshot {
                profile_id: preset.analysis_profile_id.clone(),
                model_name: preset.analysis_model.clone(),
                model_digest: preset.analysis_digest.clone(),
                context_tokens: preset.analysis_context_tokens,
                tokenizer_version: preset.analysis_tokenizer_version.clone(),
            },
            verification: ModelStageProfileSnapshot {
                profile_id: preset.verification_profile_id.clone(),
                model_name: preset.verification_model.clone(),
                model_digest: preset.verification_digest.clone(),
                context_tokens: preset.verification_context_tokens,
                tokenizer_version: preset.verification_tokenizer_version.clone(),
            },
        })
    }

    fn from_snapshot(snapshot: &ModelProfileSnapshot) -> Result<Self, ModelRuntimeFailure> {
        if snapshot.version != 1 || snapshot.preset_id.trim().is_empty() {
            return Err(config_failure("Run model profile snapshot is invalid"));
        }
        let analysis_profile = admitted_snapshot_profile(&snapshot.analysis, false)?;
        let verification_profile = admitted_snapshot_profile(&snapshot.verification, true)?;
        Self::build(
            snapshot.clone(),
            analysis_profile.tokenizer_family,
            verification_profile.tokenizer_family,
        )
    }

    fn build(
        snapshot: ModelProfileSnapshot,
        analysis_family: QwenTokenizerFamily,
        verification_family: QwenTokenizerFamily,
    ) -> Result<Self, ModelRuntimeFailure> {
        Ok(Self {
            preset_id: snapshot.preset_id.clone(),
            analysis: OllamaRuntime::from_environment_profile(
                &snapshot.analysis.model_name,
                Some(&snapshot.analysis.model_digest),
                Some(analysis_family),
                snapshot.analysis.context_tokens,
            )?,
            verification: OllamaRuntime::from_environment_profile(
                &snapshot.verification.model_name,
                Some(&snapshot.verification.model_digest),
                Some(verification_family),
                snapshot.verification.context_tokens,
            )?,
            snapshot,
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
        Self::build(
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

    fn runtime_for(&self, stage: &PipelineStage) -> &OllamaRuntime {
        if *stage == PipelineStage::Verify {
            &self.verification
        } else {
            &self.analysis
        }
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

impl ModelRuntime for QwenProfileRuntime {
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        self.runtime_for(&request.stage).generate(request)
    }

    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        self.analysis.health()?;
        if self.analysis.model_id() != self.verification.model_id() {
            self.verification.health()?;
        }
        Ok(())
    }

    fn runtime_id(&self) -> &str {
        "ollama-native-qwen-profile"
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

    fn qualified_snapshot() -> ModelProfileSnapshot {
        let profile = QUALIFIED_PROFILES[0];
        let stage = ModelStageProfileSnapshot {
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

    #[test]
    fn exact_digest_and_context_admit_only_the_qualified_preset() {
        let settings = ModelSettings::default();
        let qualified = descriptor(
            "renamed-baseline:latest",
            QUALIFIED_PROFILES[0].digest,
            Some(QwenTokenizerFamily::Qwen3),
            Some(262_144),
        );
        let catalog =
            catalog_from_descriptors(settings, vec![qualified], QUALIFIED_PROFILES).unwrap();
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
                catalog_from_descriptors(ModelSettings::default(), vec![bad], QUALIFIED_PROFILES)
                    .unwrap();
            assert!(!catalog.selected_preset_available);
            assert!(catalog.presets.is_empty());
        }
    }

    #[test]
    fn stale_selection_keeps_qualified_recovery_presets_visible() {
        let settings = ModelSettings {
            version: SETTINGS_VERSION,
            selected_preset_id: "removed-profile".to_string(),
        };
        let qualified = descriptor(
            "baseline:latest",
            QUALIFIED_PROFILES[0].digest,
            Some(QwenTokenizerFamily::Qwen3),
            Some(262_144),
        );
        let catalog =
            catalog_from_descriptors(settings, vec![qualified], QUALIFIED_PROFILES).unwrap();
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
            catalog_from_descriptors(ModelSettings::default(), vec![small, baseline], PROFILES)
                .unwrap();
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
        let runtime = runtime_from_snapshot(&snapshot)
            .expect("an admitted immutable snapshot should rebuild its runtime");
        assert_eq!(runtime.profile_snapshot(), Some(snapshot.clone()));
        assert_eq!(
            runtime.model_id_for_stage(PipelineStage::Analyze),
            "renamed-baseline:latest"
        );

        for changed in [
            ModelProfileSnapshot {
                version: 2,
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
            let error = runtime_from_snapshot(&changed)
                .err()
                .expect("profile drift must fail before inference");
            assert_eq!(error.code, "MODEL_CONFIG_INVALID");
        }
    }

    #[test]
    fn qualification_runtime_stays_separate_from_product_profile_admission() {
        let snapshot = ModelProfileSnapshot {
            version: 1,
            preset_id: "qualification-candidate".to_string(),
            analysis: ModelStageProfileSnapshot {
                profile_id: "qualification-analysis".to_string(),
                model_name: "candidate:latest".to_string(),
                model_digest: "candidate-digest".to_string(),
                context_tokens: 8_192,
                tokenizer_version: crate::pipeline::qwen_tokenizer::QWEN35_TOKENIZER_VERSION
                    .to_string(),
            },
            verification: qualified_snapshot().verification,
        };
        assert!(runtime_from_snapshot(&snapshot).is_err());
        let runtime = QwenProfileRuntime::build(
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
                    profile_id: "analysis".to_string(),
                    model_name: "analysis-model".to_string(),
                    model_digest: "analysis-digest".to_string(),
                    context_tokens: 8_192,
                    tokenizer_version: QWEN3_TOKENIZER_VERSION.to_string(),
                },
                verification: ModelStageProfileSnapshot {
                    profile_id: "verification".to_string(),
                    model_name: "verification-model".to_string(),
                    model_digest: "verification-digest".to_string(),
                    context_tokens: 16_384,
                    tokenizer_version: QWEN3_TOKENIZER_VERSION.to_string(),
                },
            },
            analysis: OllamaRuntime::new_with_context(
                "http://127.0.0.1:11434/",
                "analysis-model",
                8_192,
                std::time::Duration::from_secs(1),
                None,
            )
            .unwrap(),
            verification: OllamaRuntime::new_with_context(
                "http://127.0.0.1:11434/",
                "verification-model",
                16_384,
                std::time::Duration::from_secs(1),
                None,
            )
            .unwrap(),
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
    }
}
