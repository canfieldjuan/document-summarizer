use crate::pipeline::contracts::{PipelineWarning, SummaryArtifact};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::{Uuid, Version};

pub const PROTOCOL_VERSION: u32 = 1;
pub const APP_ID: &str = "document-summarizer";
pub const APP_NAME: &str = "Document Summarizer";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const CAPABILITY_ID: &str = "document.summarize";
pub const CAPABILITY_VERSION: &str = "1.0";
pub const INPUT_MEDIA_TYPE: &str = "application/pdf";
pub const OUTPUT_MEDIA_TYPE: &str = "application/vnd.local-connect.document-summary+json";
pub const DEFAULT_MAX_INPUT_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_REQUEST_JSON_BYTES: u64 = 64 * 1024;
pub const MAX_SUMMARY_TEXT_BYTES: usize = 1024 * 1024;
pub const MAX_SUMMARY_ARTIFACT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SUMMARY_WARNINGS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppManifest {
    pub protocol_version: u32,
    pub instance_id: String,
    pub app: AppDescription,
    pub capabilities: Vec<CapabilityDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppDescription {
    pub id: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDeclaration {
    pub id: String,
    pub version: String,
    pub accepts: Vec<AcceptedMediaType>,
    pub produces: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedMediaType {
    pub media_type: String,
    pub max_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRegistration {
    pub protocol_version: u32,
    pub instance_id: String,
    pub app_id: String,
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    pub transport: TransportRegistration,
    pub auth: AuthRegistration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportRegistration {
    pub kind: String,
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthRegistration {
    pub scheme: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobRequest {
    pub protocol_version: u32,
    pub job_id: String,
    pub capability: CapabilityRef,
    pub inputs: Vec<InputArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRef {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputArtifact {
    pub artifact_id: String,
    pub media_type: String,
    pub byte_size: u64,
    pub sha256: String,
    pub display_name: String,
    pub source_app_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRef {
    pub app_id: String,
    pub instance_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Accepted,
    Processing,
    Completed,
    Failed,
}

impl JobState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactProvenance {
    pub artifact_id: String,
    pub media_type: String,
    pub byte_size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobStatus {
    pub protocol_version: u32,
    pub job_id: String,
    pub capability: CapabilityRef,
    pub provider: ProviderRef,
    pub status: JobState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub input_artifacts: Vec<ArtifactProvenance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<JobResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JobError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobResult {
    pub outputs: Vec<SummaryOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryOutput {
    pub artifact_id: String,
    pub media_type: String,
    pub byte_size: u64,
    pub sha256: String,
    pub content: SummaryContent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummaryContent {
    pub summary_version: String,
    pub text: String,
    pub warnings: Vec<ConnectWarning>,
    pub input_artifact: ArtifactProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorEnvelope {
    pub protocol_version: u32,
    pub error: JobError,
}

#[derive(Debug, Error)]
pub enum ContractBuildError {
    #[error("Connect output serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Connect summary output violates the v1 contract: {0}")]
    InvalidSummary(String),
}

impl AppManifest {
    pub fn new(instance_id: &str, max_input_bytes: u64) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            instance_id: instance_id.to_string(),
            app: AppDescription {
                id: APP_ID.to_string(),
                name: APP_NAME.to_string(),
                version: APP_VERSION.to_string(),
            },
            capabilities: vec![CapabilityDeclaration {
                id: CAPABILITY_ID.to_string(),
                version: CAPABILITY_VERSION.to_string(),
                accepts: vec![AcceptedMediaType {
                    media_type: INPUT_MEDIA_TYPE.to_string(),
                    max_bytes: max_input_bytes,
                }],
                produces: vec![OUTPUT_MEDIA_TYPE.to_string()],
            }],
        }
    }
}

impl JobRequest {
    pub fn validate(&self, max_input_bytes: u64) -> Result<(), JobError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(job_error(
                "PROTOCOL_VERSION_UNSUPPORTED",
                "The requested Connect protocol version is unsupported.",
                false,
            ));
        }
        if !valid_uuid_v4(&self.job_id) {
            return Err(job_error(
                "JOB_ID_INVALID",
                "The job identifier must be a lowercase UUID v4.",
                false,
            ));
        }
        if self.capability.id != CAPABILITY_ID || self.capability.version != CAPABILITY_VERSION {
            return Err(job_error(
                "CAPABILITY_UNSUPPORTED",
                "The requested capability or version is unsupported.",
                false,
            ));
        }
        if self.inputs.len() != 1 {
            return Err(job_error(
                "INPUT_COUNT_INVALID",
                "This capability requires exactly one input artifact.",
                false,
            ));
        }
        let input = &self.inputs[0];
        if !valid_uuid_v4(&input.artifact_id)
            || input.media_type != INPUT_MEDIA_TYPE
            || input.byte_size == 0
            || input.byte_size > max_input_bytes
            || !valid_sha256(&input.sha256)
            || !valid_display_name(&input.display_name)
            || !valid_identifier(&input.source_app_id)
        {
            return Err(job_error(
                "INPUT_ARTIFACT_INVALID",
                "The input artifact descriptor is invalid or exceeds provider limits.",
                false,
            ));
        }
        Ok(())
    }

    pub fn canonical_hash(&self) -> Result<String, serde_json::Error> {
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
}

impl ArtifactProvenance {
    pub fn from_input(input: &InputArtifact) -> Self {
        Self {
            artifact_id: input.artifact_id.clone(),
            media_type: input.media_type.clone(),
            byte_size: input.byte_size,
            sha256: input.sha256.clone(),
        }
    }
}

impl JobResult {
    pub fn from_summary(
        input: &InputArtifact,
        summary: &SummaryArtifact,
    ) -> Result<Self, ContractBuildError> {
        if summary.text.is_empty() || summary.text.len() > MAX_SUMMARY_TEXT_BYTES {
            return Err(ContractBuildError::InvalidSummary(
                "summary text is empty or exceeds the byte limit".to_string(),
            ));
        }
        let input_artifact = ArtifactProvenance::from_input(input);
        let warnings = summary
            .warnings
            .iter()
            .map(wire_warning)
            .collect::<Vec<_>>();
        if warnings.len() > MAX_SUMMARY_WARNINGS || warnings.iter().any(invalid_wire_warning) {
            return Err(ContractBuildError::InvalidSummary(
                "summary warnings exceed count or field limits".to_string(),
            ));
        }
        let content = SummaryContent {
            summary_version: capability_summary_version(&summary.summary_version),
            text: summary.text.clone(),
            warnings,
            input_artifact,
        };
        let bytes = serde_json::to_vec(&content)?;
        if bytes.is_empty() || bytes.len() > MAX_SUMMARY_ARTIFACT_BYTES {
            return Err(ContractBuildError::InvalidSummary(
                "summary artifact exceeds the byte limit".to_string(),
            ));
        }
        Ok(Self {
            outputs: vec![SummaryOutput {
                artifact_id: Uuid::new_v4().to_string(),
                media_type: OUTPUT_MEDIA_TYPE.to_string(),
                byte_size: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
                content,
            }],
        })
    }
}

pub fn job_error(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> JobError {
    JobError {
        code: code.into(),
        message: message.into(),
        retryable,
    }
}

fn capability_summary_version(internal: &str) -> String {
    internal
        .split_once('.')
        .map(|(major, rest)| {
            let minor = rest.split('.').next().unwrap_or("0");
            format!("{major}.{minor}")
        })
        .unwrap_or_else(|| "1.0".to_string())
}

fn wire_warning(warning: &PipelineWarning) -> ConnectWarning {
    ConnectWarning {
        code: warning.code.clone(),
        message: warning.message.clone(),
    }
}

fn invalid_wire_warning(warning: &ConnectWarning) -> bool {
    warning.code.is_empty()
        || warning.code.len() > 100
        || !warning
            .code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        || warning.message.is_empty()
        || warning.message.len() > 1000
}

pub fn valid_uuid_v4(value: &str) -> bool {
    Uuid::parse_str(value)
        .ok()
        .filter(|uuid| uuid.get_version() == Some(Version::Random))
        .is_some_and(|uuid| uuid.to_string() == value)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_display_name(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 255
        && trimmed != "."
        && trimmed != ".."
        && !trimmed.contains(['/', '\\', '\0'])
        && trimmed
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("pdf"))
        && !trimmed.chars().any(char::is_control)
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'0'..=b'9' => true,
            b'.' | b'-' => index > 0 && index + 1 < value.len(),
            _ => false,
        })
        && !value.contains("..")
        && !value.contains("--")
        && !value.contains(".-")
        && !value.contains("-.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn valid_request() -> JobRequest {
        JobRequest {
            protocol_version: PROTOCOL_VERSION,
            job_id: "22222222-2222-4222-8222-222222222222".to_string(),
            capability: CapabilityRef {
                id: CAPABILITY_ID.to_string(),
                version: CAPABILITY_VERSION.to_string(),
            },
            inputs: vec![InputArtifact {
                artifact_id: "33333333-3333-4333-8333-333333333333".to_string(),
                media_type: INPUT_MEDIA_TYPE.to_string(),
                byte_size: 100,
                sha256: "a".repeat(64),
                display_name: "report.pdf".to_string(),
                source_app_id: "email-watcher".to_string(),
            }],
        }
    }

    #[test]
    fn job_request_boundary_rejects_paths_versions_and_size_overflow() {
        assert!(valid_request().validate(DEFAULT_MAX_INPUT_BYTES).is_ok());

        let mut path = valid_request();
        path.inputs[0].display_name = "../report.pdf".to_string();
        assert!(path.validate(DEFAULT_MAX_INPUT_BYTES).is_err());

        let mut too_large = valid_request();
        too_large.inputs[0].byte_size = DEFAULT_MAX_INPUT_BYTES + 1;
        assert!(too_large.validate(DEFAULT_MAX_INPUT_BYTES).is_err());

        let mut mismatch = valid_request();
        mismatch.capability.version = "2.0".to_string();
        assert!(mismatch.validate(DEFAULT_MAX_INPUT_BYTES).is_err());
    }

    #[test]
    fn canonical_job_hash_is_stable_and_sensitive_to_input_identity() {
        let first = valid_request();
        let mut second = first.clone();
        assert_eq!(
            first.canonical_hash().unwrap(),
            second.canonical_hash().unwrap()
        );
        second.inputs[0].sha256 = "b".repeat(64);
        assert_ne!(
            first.canonical_hash().unwrap(),
            second.canonical_hash().unwrap()
        );
    }

    #[test]
    fn summary_output_enforces_both_valid_and_oversized_boundaries() {
        let request = valid_request();
        let mut summary = SummaryArtifact {
            document_id: Uuid::new_v4().to_string(),
            summary_version: "1.0.0".to_string(),
            text: "Grounded summary.".to_string(),
            warnings: vec![PipelineWarning {
                code: "SEMANTIC_VERIFICATION_DEFERRED".to_string(),
                message: "Semantic verification is deferred.".to_string(),
                stage: None,
            }],
            created_at: Utc::now(),
            integrity_hash: "unused-by-wire-contract".to_string(),
        };
        let output = JobResult::from_summary(&request.inputs[0], &summary)
            .expect("bounded summary should become a wire artifact");
        assert_eq!(output.outputs[0].content.text, "Grounded summary.");
        assert_eq!(output.outputs[0].content.summary_version, "1.0");

        summary.text = "x".repeat(MAX_SUMMARY_TEXT_BYTES + 1);
        assert!(matches!(
            JobResult::from_summary(&request.inputs[0], &summary),
            Err(ContractBuildError::InvalidSummary(_))
        ));
    }
}
