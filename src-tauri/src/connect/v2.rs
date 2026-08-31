use crate::connect::contracts::{
    self as v1, AcceptedMediaType, AppDescription, ArtifactProvenance, AuthRegistration,
    CapabilityRef, InputArtifact, JobError, JobState, ProviderRef, TransportRegistration, APP_ID,
    APP_NAME, APP_VERSION, CAPABILITY_ID, CAPABILITY_VERSION, INPUT_MEDIA_TYPE, OUTPUT_MEDIA_TYPE,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
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
            .validate_v2_input_descriptor(max_input_bytes)
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
        let mut artifact_ids = status
            .input_artifacts
            .iter()
            .map(|artifact| artifact.artifact_id.clone())
            .collect::<BTreeSet<_>>();
        if let Some(result) = &result {
            for output in &result.outputs {
                if !artifact_ids.insert(output.artifact_id.clone()) {
                    return Err(ContractBuildError::OutputIntegrity);
                }
            }
        }
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
    use serde_json::Value;
    use std::env;
    use std::path::PathBuf;
    use std::process::Command;

    const CONTRACTS_REVISION: &str = "4d46af25ef5112f76daf841c7622987f05d25142";

    fn canonical_fixture(relative_path: &str) -> Value {
        let repository = PathBuf::from(
            env::var_os("CONNECT_CONTRACTS_DIR")
                .expect("CONNECT_CONTRACTS_DIR must name a connect-contracts Git checkout"),
        );
        assert!(
            repository.is_dir(),
            "CONNECT_CONTRACTS_DIR is not a directory"
        );
        let revision_path = format!("{CONTRACTS_REVISION}:fixtures/v2/{relative_path}");
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .arg("show")
            .arg(&revision_path)
            .output()
            .expect("git must be available for canonical Connect conformance");
        assert!(
            output.status.success(),
            "canonical Connect fixture unavailable at {revision_path}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        serde_json::from_slice(&output.stdout).expect("canonical Connect fixture must be JSON")
    }

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
    fn v2_accepts_empty_and_extensionless_artifacts_without_changing_v1() {
        let mut empty = request();
        empty.inputs[0].byte_size = 0;
        empty.inputs[0].sha256 =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string();

        assert!(empty.validate(v1::DEFAULT_MAX_INPUT_BYTES).is_ok());
        assert!(empty
            .as_internal()
            .validate(v1::DEFAULT_MAX_INPUT_BYTES)
            .is_err());

        let mut extensionless = request();
        extensionless.inputs[0].display_name = "scan".to_string();
        assert!(extensionless.validate(v1::DEFAULT_MAX_INPUT_BYTES).is_ok());
        assert!(extensionless
            .as_internal()
            .validate(v1::DEFAULT_MAX_INPUT_BYTES)
            .is_err());

        extensionless.inputs[0].display_name = "../scan".to_string();
        assert!(extensionless.validate(v1::DEFAULT_MAX_INPUT_BYTES).is_err());
    }

    #[test]
    fn v2_input_count_and_size_boundaries_are_exact() {
        let mut at_limit = request();
        at_limit.inputs[0].byte_size = v1::DEFAULT_MAX_INPUT_BYTES;
        assert!(at_limit.validate(v1::DEFAULT_MAX_INPUT_BYTES).is_ok());

        let mut over_limit = at_limit.clone();
        over_limit.inputs[0].byte_size += 1;
        assert_eq!(
            over_limit
                .validate(v1::DEFAULT_MAX_INPUT_BYTES)
                .unwrap_err()
                .code,
            "INPUT_ARTIFACT_INVALID"
        );

        let mut missing = request();
        missing.inputs.clear();
        assert_eq!(
            missing
                .validate(v1::DEFAULT_MAX_INPUT_BYTES)
                .unwrap_err()
                .code,
            "INPUT_COUNT_INVALID"
        );

        let mut multiple = request();
        multiple.inputs.push(multiple.inputs[0].clone());
        assert_eq!(
            multiple
                .validate(v1::DEFAULT_MAX_INPUT_BYTES)
                .unwrap_err()
                .code,
            "INPUT_COUNT_INVALID"
        );
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

    #[test]
    fn v2_output_identity_cannot_alias_its_input() {
        let request = request();
        let mut result = v1::JobResult::from_summary(
            &request.inputs[0],
            &SummaryArtifact {
                document_id: "44444444-4444-4444-8444-444444444444".to_string(),
                summary_version: "1.0.0".to_string(),
                text: "Invoice due Friday.".to_string(),
                warnings: vec![],
                created_at: Utc::now(),
                integrity_hash: "unused-by-wire-contract".to_string(),
            },
        )
        .unwrap();
        result.outputs[0].artifact_id = request.inputs[0].artifact_id.clone();
        let timestamp = Utc::now();
        let status = v1::JobStatus {
            protocol_version: v1::PROTOCOL_VERSION,
            job_id: request.job_id,
            capability: request.capability,
            provider: ProviderRef {
                app_id: APP_ID.to_string(),
                instance_id: "11111111-1111-4111-8111-111111111111".to_string(),
            },
            status: JobState::Completed,
            created_at: timestamp,
            updated_at: timestamp,
            input_artifacts: vec![ArtifactProvenance::from_input(&request.inputs[0])],
            result: Some(result),
            error: None,
        };

        assert!(matches!(
            JobStatus::from_v1(status),
            Err(ContractBuildError::OutputIntegrity)
        ));
    }

    #[test]
    #[ignore = "requires CONNECT_CONTRACTS_DIR"]
    fn canonical_v2_provider_contract_fixtures() {
        let cases = canonical_fixture("index.json");
        let cases = cases
            .as_array()
            .expect("canonical Connect fixture index must be an array");
        let mut output_schemas = BTreeSet::new();
        let mut admitted_provider_request = false;
        let mut rejected_request = false;

        for case in cases {
            let schema = case["schema"]
                .as_str()
                .expect("canonical fixture schema must be a string");
            let fixture_path = case["fixture"]
                .as_str()
                .expect("canonical fixture path must be a string");
            let valid = case["valid"]
                .as_bool()
                .expect("canonical fixture validity must be boolean");
            let fixture = canonical_fixture(fixture_path);

            match schema {
                "job-request.schema.json" => {
                    let targets_document_summarizer = case["provider_manifest"]
                        .as_str()
                        .is_some_and(|path| path == "valid/manifest.json");
                    let admitted = serde_json::from_value::<JobRequest>(fixture)
                        .ok()
                        .is_some_and(|request| {
                            request.validate(v1::DEFAULT_MAX_INPUT_BYTES).is_ok()
                        });
                    assert_eq!(
                        admitted,
                        valid && targets_document_summarizer,
                        "provider request admission diverged for {fixture_path}"
                    );
                    admitted_provider_request |= admitted;
                    rejected_request |= !admitted;
                }
                "manifest.schema.json" if valid => {
                    let parsed: AppManifest = serde_json::from_value(fixture.clone())
                        .expect("valid canonical manifest must deserialize");
                    assert_eq!(serde_json::to_value(parsed).unwrap(), fixture);
                    output_schemas.insert(schema);
                }
                "registration.schema.json" if valid => {
                    let parsed: RuntimeRegistration = serde_json::from_value(fixture.clone())
                        .expect("valid canonical registration must deserialize");
                    assert_eq!(serde_json::to_value(parsed).unwrap(), fixture);
                    output_schemas.insert(schema);
                }
                "job-status.schema.json" if valid => {
                    let parsed: JobStatus = serde_json::from_value(fixture.clone())
                        .expect("valid canonical job status must deserialize");
                    assert_eq!(serde_json::to_value(parsed).unwrap(), fixture);
                    output_schemas.insert(schema);
                }
                "error.schema.json" if valid => {
                    let parsed: ErrorEnvelope = serde_json::from_value(fixture.clone())
                        .expect("valid canonical error must deserialize");
                    assert_eq!(serde_json::to_value(parsed).unwrap(), fixture);
                    output_schemas.insert(schema);
                }
                _ => {}
            }
        }

        assert!(admitted_provider_request);
        assert!(rejected_request);
        assert_eq!(
            output_schemas,
            BTreeSet::from([
                "error.schema.json",
                "job-status.schema.json",
                "manifest.schema.json",
                "registration.schema.json",
            ])
        );
        assert_eq!(
            serde_json::to_value(AppManifest::new(
                "11111111-1111-4111-8111-111111111111",
                v1::DEFAULT_MAX_INPUT_BYTES,
            ))
            .unwrap(),
            canonical_fixture("valid/manifest.json")
        );
    }
}
