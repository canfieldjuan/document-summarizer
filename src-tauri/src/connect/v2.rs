use crate::connect::contracts::{
    self as v1, AcceptedMediaType, AppDescription, ArtifactProvenance, AuthRegistration,
    CapabilityRef, InputArtifact, JobError, JobState, ProviderRef, TransportRegistration, APP_ID,
    APP_NAME, APP_VERSION, CAPABILITY_ID, CAPABILITY_VERSION, INPUT_MEDIA_TYPE, OUTPUT_MEDIA_TYPE,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

pub const PROTOCOL_VERSION: u32 = 2;
pub const TRANSPORT_KIND: &str = "http-loopback-v2";
pub const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

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
pub struct CapabilityDeclaration {
    pub id: String,
    pub version: String,
    pub action: ActionDescription,
    pub accepts: Vec<AcceptedMediaType>,
    pub produces: Vec<String>,
    pub parameters: Vec<ParameterDeclaration>,
    pub effects: CapabilityEffects,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionDescription {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterDeclaration {
    pub name: String,
    pub value_type: ParameterType,
    pub required: bool,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterType {
    String,
    Integer,
    Boolean,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityEffects {
    pub external: bool,
    pub confirmation_required: bool,
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
#[serde(untagged)]
pub enum ParameterValue {
    String(String),
    Integer(i64),
    Boolean(bool),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobRequest {
    pub protocol_version: u32,
    pub job_id: String,
    pub capability: CapabilityRef,
    pub inputs: Vec<InputArtifact>,
    pub parameters: BTreeMap<String, ParameterValue>,
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
    pub outputs: Vec<OutputArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputArtifact {
    pub artifact_id: String,
    pub media_type: String,
    pub display_name: String,
    pub byte_size: u64,
    pub sha256: String,
    pub payload_base64: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorEnvelope {
    pub protocol_version: u32,
    pub error: JobError,
}

#[derive(Debug, Error)]
pub enum ContractBuildError {
    #[error("Connect v2 output serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Connect v2 output failed its persisted integrity fields")]
    OutputIntegrity,
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
                action: ActionDescription {
                    label: "Summarize".to_string(),
                    description: "Create a local plain-text summary of this document.".to_string(),
                },
                accepts: vec![AcceptedMediaType {
                    media_type: INPUT_MEDIA_TYPE.to_string(),
                    max_bytes: max_input_bytes,
                }],
                produces: vec![OUTPUT_MEDIA_TYPE.to_string()],
                parameters: vec![],
                effects: CapabilityEffects {
                    external: false,
                    confirmation_required: false,
                },
            }],
        }
    }
}

impl JobRequest {
    pub fn validate(&self, max_input_bytes: u64) -> Result<(), JobError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(v1::job_error(
                "PROTOCOL_VERSION_UNSUPPORTED",
                "The requested Connect protocol version is unsupported.",
                false,
            ));
        }
        if !self.parameters.is_empty() {
            return Err(v1::job_error(
                "PARAMETERS_INVALID",
                "This capability does not accept invocation parameters.",
                false,
            ));
        }
        self.as_internal()
            .validate_allowing_empty_input(max_input_bytes)
    }

    pub fn canonical_hash(&self) -> Result<String, serde_json::Error> {
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }

    pub fn as_internal(&self) -> v1::JobRequest {
        v1::JobRequest {
            protocol_version: v1::PROTOCOL_VERSION,
            job_id: self.job_id.clone(),
            capability: self.capability.clone(),
            inputs: self.inputs.clone(),
        }
    }
}

impl JobStatus {
    pub fn from_v1(status: v1::JobStatus) -> Result<Self, ContractBuildError> {
        let result = status.result.map(JobResult::from_v1).transpose()?;
        Ok(Self {
            protocol_version: PROTOCOL_VERSION,
            job_id: status.job_id,
            capability: status.capability,
            provider: status.provider,
            status: status.status,
            created_at: status.created_at,
            updated_at: status.updated_at,
            input_artifacts: status.input_artifacts,
            result,
            error: status.error,
        })
    }
}

impl JobResult {
    fn from_v1(result: v1::JobResult) -> Result<Self, ContractBuildError> {
        let mut outputs = Vec::with_capacity(result.outputs.len());
        for output in result.outputs {
            let bytes = serde_json::to_vec(&output.content)?;
            if bytes.is_empty()
                || bytes.len() > MAX_OUTPUT_BYTES
                || output.byte_size != bytes.len() as u64
                || output.sha256 != format!("{:x}", Sha256::digest(&bytes))
            {
                return Err(ContractBuildError::OutputIntegrity);
            }
            outputs.push(OutputArtifact {
                artifact_id: output.artifact_id,
                media_type: output.media_type,
                display_name: "summary.json".to_string(),
                byte_size: output.byte_size,
                sha256: output.sha256,
                payload_base64: BASE64.encode(bytes),
            });
        }
        Ok(Self { outputs })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contracts::{PipelineWarning, SummaryArtifact};

    fn request() -> JobRequest {
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
            parameters: BTreeMap::new(),
        }
    }

    #[test]
    fn manifest_declares_safe_generic_presentation_metadata() {
        let manifest = AppManifest::new(
            "11111111-1111-4111-8111-111111111111",
            v1::DEFAULT_MAX_INPUT_BYTES,
        );
        let capability = &manifest.capabilities[0];
        assert_eq!(manifest.protocol_version, PROTOCOL_VERSION);
        assert_eq!(capability.action.label, "Summarize");
        assert!(capability.parameters.is_empty());
        assert!(!capability.effects.external);
        assert!(!capability.effects.confirmation_required);
    }

    #[test]
    fn summary_request_rejects_protocol_parameters_and_v1_input_violations() {
        assert!(request().validate(v1::DEFAULT_MAX_INPUT_BYTES).is_ok());

        let mut protocol = request();
        protocol.protocol_version = 1;
        assert_eq!(
            protocol
                .validate(v1::DEFAULT_MAX_INPUT_BYTES)
                .unwrap_err()
                .code,
            "PROTOCOL_VERSION_UNSUPPORTED"
        );

        let mut parameters = request();
        parameters.parameters.insert(
            "target-language".to_string(),
            ParameterValue::String("Spanish".to_string()),
        );
        assert_eq!(
            parameters
                .validate(v1::DEFAULT_MAX_INPUT_BYTES)
                .unwrap_err()
                .code,
            "PARAMETERS_INVALID"
        );

        let mut path = request();
        path.inputs[0].display_name = "../report.pdf".to_string();
        assert!(path.validate(v1::DEFAULT_MAX_INPUT_BYTES).is_err());
    }

    #[test]
    fn v2_accepts_zero_byte_wire_artifacts_without_changing_v1() {
        let mut empty = request();
        empty.inputs[0].byte_size = 0;
        empty.inputs[0].sha256 =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string();

        assert!(empty.validate(v1::DEFAULT_MAX_INPUT_BYTES).is_ok());
        assert!(empty
            .as_internal()
            .validate(v1::DEFAULT_MAX_INPUT_BYTES)
            .is_err());
    }

    #[test]
    fn v1_summary_converts_to_integrity_checked_generic_bytes() {
        let request = request();
        let result = v1::JobResult::from_summary(
            &request.inputs[0],
            &SummaryArtifact {
                document_id: "44444444-4444-4444-8444-444444444444".to_string(),
                summary_version: "1.0.0".to_string(),
                text: "Invoice due Friday.".to_string(),
                warnings: vec![PipelineWarning {
                    code: "TEST_WARNING".to_string(),
                    message: "Fixture warning.".to_string(),
                    stage: None,
                }],
                created_at: Utc::now(),
                integrity_hash: "unused-by-wire-contract".to_string(),
            },
        )
        .unwrap();
        let converted = JobResult::from_v1(result).unwrap();
        let output = &converted.outputs[0];
        let bytes = BASE64.decode(&output.payload_base64).unwrap();
        assert_eq!(bytes.len() as u64, output.byte_size);
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), output.sha256);
    }
}
