use crate::pipeline::contracts::{
    ModelOutputFormat, ModelProfileSnapshot, ModelRequest, ModelRequestAttemptDiagnostic,
    ModelResponse, ModelRuntime, ModelRuntimeFailure, ModelRuntimeKind, ModelStageProfileSnapshot,
    ModelTokenUsage, ModelTransportAttempt, PipelineStage,
};
use crate::pipeline::control::ExecutionControl;
use crate::pipeline::db;
use crate::pipeline::gateway_client::{
    GatewayClient, GatewayClientConfig, GatewayClientError, GatewayHealth, GatewayResult,
    GatewayTaskProfile, MAX_OUTPUT_TOKENS,
};
use crate::pipeline::gateway_store::GatewayRequestKey;
use chrono::Utc;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

const SNAPSHOT_VERSION: u32 = 3;
const RUNTIME_ID: &str = "local-inference-gateway";
#[cfg(test)]
const TASK_ID: &str = "document.summary.step@1";
const TASK_PROFILE_ID: &str = "gateway-document-summary-step-v1";
const TASK_PROFILE_VERSION: &str = "gateway-task-profile-v1";
const TASK_PROFILE_CANONICAL: &str = concat!(
    "task=document.summary.step@1\n",
    "input=text\n",
    "output=application/json\n",
    "structured_output=true\n",
    "context_tokens=8192\n",
    "max_output_tokens=4096\n",
);

trait GatewayExecutor: Send + Sync {
    fn preflight(&self, request: &ModelRequest) -> Result<(), GatewayClientError>;
    fn health(&self) -> Result<GatewayHealth, GatewayClientError>;
    fn execute(
        &self,
        conn: &mut Connection,
        key: &GatewayRequestKey,
        request: &ModelRequest,
    ) -> Result<GatewayResult, GatewayClientError>;
}

impl GatewayExecutor for GatewayClient {
    fn preflight(&self, request: &ModelRequest) -> Result<(), GatewayClientError> {
        GatewayClient::preflight(self, request)
    }

    fn health(&self) -> Result<GatewayHealth, GatewayClientError> {
        GatewayClient::health(self)
    }

    fn execute(
        &self,
        conn: &mut Connection,
        key: &GatewayRequestKey,
        request: &ModelRequest,
    ) -> Result<GatewayResult, GatewayClientError> {
        GatewayClient::execute(self, conn, key, request, Utc::now())
    }
}

pub(crate) struct GatewayRuntime {
    profile: GatewayTaskProfile,
    db_path: PathBuf,
    executor: Arc<dyn GatewayExecutor>,
    bound_owner_id: Option<String>,
    snapshot: ModelProfileSnapshot,
}

impl GatewayRuntime {
    pub(crate) fn new(
        db_path: PathBuf,
        config: GatewayClientConfig,
    ) -> Result<Self, ModelRuntimeFailure> {
        let client = GatewayClient::new(config)
            .and_then(GatewayClient::negotiate_profile)
            .map_err(map_client_failure)?;
        let profile = client.profile();
        Ok(Self::with_profile_executor(
            db_path,
            Arc::new(client),
            profile,
        ))
    }

    pub(crate) fn from_snapshot(
        db_path: PathBuf,
        config: GatewayClientConfig,
        snapshot: &ModelProfileSnapshot,
    ) -> Result<Self, ModelRuntimeFailure> {
        let profile = validate_snapshot(snapshot)?;
        let client = GatewayClient::new(config)
            .map_err(map_client_failure)?
            .with_profile(profile);
        Ok(Self::with_profile_executor(
            db_path,
            Arc::new(client),
            profile,
        ))
    }

    #[cfg(test)]
    pub(super) fn with_client(db_path: PathBuf, client: GatewayClient) -> Self {
        let profile = client.profile();
        Self::with_profile_executor(db_path, Arc::new(client), profile)
    }

    #[cfg(test)]
    fn with_executor(db_path: PathBuf, executor: Arc<dyn GatewayExecutor>) -> Self {
        Self::with_profile_executor(db_path, executor, GatewayTaskProfile::LEGACY)
    }

    fn with_profile_executor(
        db_path: PathBuf,
        executor: Arc<dyn GatewayExecutor>,
        profile: GatewayTaskProfile,
    ) -> Self {
        Self {
            profile,
            db_path,
            executor,
            bound_owner_id: None,
            snapshot: canonical_snapshot(profile),
        }
    }
}

impl ModelRuntime for GatewayRuntime {
    fn request_owner_contract(&self, operation_contract: &str) -> String {
        if self.profile == GatewayTaskProfile::LEGACY {
            operation_contract.to_string()
        } else {
            format!(
                "{operation_contract}.{}",
                self.snapshot.analysis.model_digest
            )
        }
    }

    fn bind_request_owner(&mut self, owner_id: &str) {
        self.bound_owner_id = Some(owner_id.to_string());
    }

    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        let owner_id = self.bound_owner_id.as_deref().ok_or_else(|| {
            failure(
                "MODEL_CONFIG_INVALID",
                "Inference gateway runtime is not bound to a durable request owner",
                false,
                Vec::new(),
            )
        })?;
        let started = Instant::now();
        let mut conn = db::init_db(&self.db_path).map_err(|_| {
            failure(
                "MODEL_GATEWAY_STORE_FAILED",
                "Inference gateway request state is unavailable",
                false,
                vec![diagnostic(request, started, false)],
            )
        })?;
        let key = GatewayRequestKey {
            owner_id: owner_id.to_string(),
            stage: request.stage.clone(),
            ordinal: request.ordinal,
        };
        match self.executor.execute(&mut conn, &key, request) {
            Ok(result) => Ok(ModelResponse {
                text: result.content,
                runtime_id: RUNTIME_ID.to_string(),
                model_id: self.profile.model_id().to_string(),
                request_attempts: vec![diagnostic(request, started, true)],
            }),
            Err(error) => {
                let mut mapped = map_client_failure(error);
                mapped.request_attempts = vec![diagnostic(request, started, false)];
                Err(mapped)
            }
        }
    }

    fn generate_with_control(
        &self,
        request: &ModelRequest,
        control: &dyn ExecutionControl,
    ) -> Result<ModelResponse, ModelRuntimeFailure> {
        if control.cancellation_requested() {
            return Err(failure(
                "MODEL_REQUEST_CANCELLED",
                "Inference gateway request was cancelled before transport",
                true,
                Vec::new(),
            ));
        }
        self.generate(request)
    }

    fn supports_response_schema(&self, name: &str) -> bool {
        self.profile.supports_response_schema(name)
    }

    fn response_schema_byte_limit(&self, _stage: PipelineStage, name: &str) -> usize {
        crate::pipeline::contracts::response_schema_byte_limit(name)
            .min(crate::pipeline::gateway_client::MAX_SCHEMA_BYTES)
    }

    fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        if matches!(&request.output_format, ModelOutputFormat::JsonSchema { name, .. } if !self.supports_response_schema(name))
        {
            return Err(map_client_failure(GatewayClientError::UnsupportedSchema));
        }
        if request.max_output_tokens > MAX_OUTPUT_TOKENS {
            return Err(failure(
                "MODEL_OUTPUT_BUDGET_EXCEEDED",
                "The requested output allowance exceeds the gateway task capacity",
                false,
                Vec::new(),
            ));
        }
        self.executor.preflight(request).map_err(map_client_failure)
    }

    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        let health = self.executor.health().map_err(map_client_failure)?;
        if health.available {
            Ok(())
        } else {
            Err(failure(
                "MODEL_GATEWAY_UNAVAILABLE",
                "Inference gateway task is unavailable",
                true,
                Vec::new(),
            ))
        }
    }

    fn runtime_id(&self) -> &str {
        RUNTIME_ID
    }

    fn model_id(&self) -> &str {
        self.profile.model_id()
    }

    fn context_tokens(&self, _stage: PipelineStage) -> u32 {
        self.profile.context_tokens()
    }

    fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
        Some(self.snapshot.clone())
    }
}

fn canonical_snapshot(profile: GatewayTaskProfile) -> ModelProfileSnapshot {
    let (profile_id, canonical) = if profile == GatewayTaskProfile::LEGACY {
        (TASK_PROFILE_ID, TASK_PROFILE_CANONICAL.to_string())
    } else {
        ("gateway-document-summary-step-v2", format!(
            "task={}\nprofile_version=1\ninput=text\noutput=application/json\nstructured_output=true\ncontext_tokens={}\nmax_output_tokens={}\nmax_schema_bytes={}\nschema_features=bounded_source_passages_v1\n",
            profile.model_id(), profile.context_tokens(), MAX_OUTPUT_TOKENS, crate::pipeline::gateway_client::MAX_SCHEMA_BYTES
        ))
    };
    let stage = ModelStageProfileSnapshot {
        runtime_kind: ModelRuntimeKind::InferenceGateway,
        profile_id: profile_id.to_string(),
        model_name: profile.model_id().to_string(),
        model_digest: format!("{:x}", Sha256::digest(canonical.as_bytes())),
        context_tokens: profile.context_tokens(),
        tokenizer_version: TASK_PROFILE_VERSION.to_string(),
    };
    ModelProfileSnapshot {
        version: SNAPSHOT_VERSION,
        preset_id: profile_id.to_string(),
        analysis: stage.clone(),
        verification: stage,
    }
}

fn validate_snapshot(
    snapshot: &ModelProfileSnapshot,
) -> Result<GatewayTaskProfile, ModelRuntimeFailure> {
    let profile = if snapshot.preset_id == "gateway-document-summary-step-v2" {
        GatewayTaskProfile::passages(snapshot.analysis.context_tokens)
            .map_err(map_client_failure)?
    } else {
        GatewayTaskProfile::LEGACY
    };
    if snapshot == &canonical_snapshot(profile) {
        Ok(profile)
    } else {
        Err(failure(
            "MODEL_CONFIG_INVALID",
            "Run gateway task profile does not match the supported application contract",
            false,
            Vec::new(),
        ))
    }
}

fn map_client_failure(error: GatewayClientError) -> ModelRuntimeFailure {
    let recoverable = error.recoverable();
    let (code, message) = match error {
        GatewayClientError::Store(_) => (
            "MODEL_GATEWAY_STORE_FAILED",
            "Inference gateway request state is unavailable",
        ),
        GatewayClientError::Configuration(_) => (
            "MODEL_CONFIG_INVALID",
            "Inference gateway configuration is invalid",
        ),
        GatewayClientError::Credential => (
            "MODEL_GATEWAY_CREDENTIAL_UNAVAILABLE",
            "Inference gateway credential is unavailable",
        ),
        GatewayClientError::Transport => (
            "MODEL_GATEWAY_UNAVAILABLE",
            "Inference gateway transport is unavailable",
        ),
        GatewayClientError::Expired => (
            "MODEL_GATEWAY_REQUEST_EXPIRED",
            "Inference gateway request expired before reconciliation",
        ),
        GatewayClientError::Protocol(_) => (
            "MODEL_GATEWAY_PROTOCOL_INVALID",
            "Inference gateway response violated the application contract",
        ),
        GatewayClientError::UnsupportedSchema => (
            "MODEL_SCHEMA_UNSUPPORTED",
            "The gateway task does not support this response protocol",
        ),
        GatewayClientError::Rejected { .. } => (
            "MODEL_GATEWAY_REJECTED",
            "Inference gateway rejected the request",
        ),
    };
    failure(code, message, recoverable, Vec::new())
}

fn diagnostic(
    request: &ModelRequest,
    started: Instant,
    succeeded: bool,
) -> ModelRequestAttemptDiagnostic {
    ModelRequestAttemptDiagnostic {
        stage: request.stage.clone(),
        request_ordinal: request.ordinal,
        attempt_ordinal: 0,
        transport_attempt: ModelTransportAttempt::Primary,
        elapsed_milliseconds: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        configured_output_tokens: request.max_output_tokens,
        provider_usage: ModelTokenUsage::default(),
        succeeded,
    }
}

fn failure(
    code: &str,
    message: &str,
    recoverable: bool,
    request_attempts: Vec<ModelRequestAttemptDiagnostic>,
) -> ModelRuntimeFailure {
    ModelRuntimeFailure {
        code: code.to_string(),
        message: message.to_string(),
        recoverable,
        request_attempts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contracts::{ModelOutputFormat, SummaryProfile};
    use crate::pipeline::service::admit_pdf_for_background;
    use serde_json::json;
    use std::fs;
    use std::sync::Mutex;
    use uuid::Uuid;

    #[derive(Default)]
    struct ExecutorState {
        keys: Vec<GatewayRequestKey>,
        preflight_calls: usize,
        health_calls: usize,
    }

    struct FixtureExecutor {
        state: Arc<Mutex<ExecutorState>>,
        rejection: bool,
        available: bool,
    }

    impl GatewayExecutor for FixtureExecutor {
        fn preflight(&self, _request: &ModelRequest) -> Result<(), GatewayClientError> {
            self.state.lock().unwrap().preflight_calls += 1;
            Ok(())
        }

        fn health(&self) -> Result<GatewayHealth, GatewayClientError> {
            self.state.lock().unwrap().health_calls += 1;
            Ok(GatewayHealth {
                available: self.available,
                status: if self.available {
                    "available".to_string()
                } else {
                    "unavailable".to_string()
                },
            })
        }

        fn execute(
            &self,
            _conn: &mut Connection,
            key: &GatewayRequestKey,
            _request: &ModelRequest,
        ) -> Result<GatewayResult, GatewayClientError> {
            self.state.lock().unwrap().keys.push(key.clone());
            if self.rejection {
                Err(GatewayClientError::Rejected {
                    code: "PRIVATE_PROVIDER_DETAIL".to_string(),
                    retryable: true,
                    retry_after_seconds: Some(1),
                })
            } else {
                Ok(GatewayResult {
                    content: "{\"summary\":\"fixture\"}".to_string(),
                    deployment_id: "private-deployment".to_string(),
                    task_policy_version: 7,
                })
            }
        }
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("gateway-runtime-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn request() -> ModelRequest {
        ModelRequest {
            stage: PipelineStage::Analyze,
            ordinal: 4,
            system_prompt: "system".to_string(),
            user_prompt: "user".to_string(),
            seed: 9,
            max_output_tokens: 128,
            output_format: ModelOutputFormat::JsonSchema {
                name: "summary".to_string(),
                schema: json!({"type": "object"}),
            },
        }
    }

    fn runtime(
        root: &TestDirectory,
        state: Arc<Mutex<ExecutorState>>,
        rejection: bool,
        available: bool,
    ) -> GatewayRuntime {
        GatewayRuntime::with_executor(
            root.0.join("summarizer.db"),
            Arc::new(FixtureExecutor {
                state,
                rejection,
                available,
            }),
        )
    }

    #[test]
    fn negotiated_gateway_snapshot_preserves_budget_capability_and_task() {
        let root = TestDirectory::new();
        let state = Arc::new(Mutex::new(ExecutorState::default()));
        let profile = GatewayTaskProfile::passages(32768).unwrap();
        let runtime = GatewayRuntime::with_profile_executor(
            root.0.join("summarizer.db"),
            Arc::new(FixtureExecutor {
                state: state.clone(),
                rejection: false,
                available: true,
            }),
            profile,
        );
        for stage in [
            PipelineStage::Analyze,
            PipelineStage::Verify,
            PipelineStage::Synthesize,
        ] {
            assert_eq!(runtime.context_tokens(stage), 32768);
        }
        assert_eq!(runtime.model_id(), "document.summary.step@2");
        let mut request = request();
        request.output_format = ModelOutputFormat::JsonSchema {
            name: crate::pipeline::contracts::CLAIM_COMPARISON_SCHEMA_NAME.into(),
            schema: json!({"type":"object"}),
        };
        assert!(runtime
            .supports_response_schema(crate::pipeline::contracts::CLAIM_COMPARISON_SCHEMA_NAME));
        runtime.preflight_request(&request).unwrap();
        assert_eq!(state.lock().unwrap().preflight_calls, 1);
        let snapshot = runtime.profile_snapshot().unwrap();
        let mut conn = db::init_db(root.0.join("summarizer.db")).unwrap();
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let (_, run) = admit_pdf_for_background(
            &mut conn,
            fixture.to_str().unwrap(),
            Some(&snapshot),
            SummaryProfile::General,
            None,
        )
        .unwrap();
        let restored = db::get_run_model_profile(&conn, &run.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(restored, snapshot);
        assert_eq!(validate_snapshot(&restored).unwrap(), profile);
        assert_eq!(
            canonical_snapshot(validate_snapshot(&restored).unwrap()),
            snapshot
        );
        let legacy = canonical_snapshot(GatewayTaskProfile::LEGACY);
        assert_eq!(
            validate_snapshot(&legacy).unwrap(),
            GatewayTaskProfile::LEGACY
        );
        assert_eq!(legacy.analysis.context_tokens, 8192);
        assert_ne!(legacy.analysis.model_digest, snapshot.analysis.model_digest);
        for field in ["version", "presetId", "analysis", "verification"] {
            let mut serialized = serde_json::to_value(&snapshot).unwrap();
            // Mutate real serialized fields rather than adding ignored data.
            if field == "version" {
                serialized[field] = json!(4);
            } else if field == "presetId" {
                serialized[field] = json!("unknown");
            } else {
                serialized[field]["contextTokens"] = json!(16384);
            }
            let changed: ModelProfileSnapshot = serde_json::from_value(serialized).unwrap();
            assert_ne!(changed, snapshot);
            assert!(validate_snapshot(&changed).is_err());
        }
    }

    #[test]
    fn c9_schema_admission_preserves_the_gateway_task_limit() {
        let root = TestDirectory::new();
        let state = Arc::new(Mutex::new(ExecutorState::default()));
        let runtime = runtime(&root, state, false, true);
        assert_eq!(
            runtime.response_schema_byte_limit(
                PipelineStage::Verify,
                crate::pipeline::contracts::CLAIM_COMPARISON_SCHEMA_NAME
            ),
            crate::pipeline::gateway_client::MAX_SCHEMA_BYTES
        );
        assert_eq!(
            runtime.response_schema_byte_limit(PipelineStage::Verify, "document_claim_verdicts_v1"),
            crate::pipeline::contracts::MAX_RESPONSE_SCHEMA_BYTES
        );
    }

    #[test]
    fn c9_gateway_admission_rejects_unsupported_protocol() {
        let root = TestDirectory::new();
        let state = Arc::new(Mutex::new(ExecutorState::default()));
        let runtime = runtime(&root, Arc::clone(&state), false, true);
        let mut request = request();
        request.output_format = ModelOutputFormat::JsonSchema {
            name: crate::pipeline::contracts::CLAIM_COMPARISON_SCHEMA_NAME.into(),
            schema: json!({"type":"object", "$defs":{"passage":{"type":"string","enum":["a"]}}}),
        };
        let result = runtime.preflight_request(&request);
        println!("c9_gateway_admission={result:?}");
        assert!(matches!(result, Err(ref e) if e.code == "MODEL_SCHEMA_UNSUPPORTED"));
        assert_eq!(state.lock().unwrap().preflight_calls, 0);
        assert!(state.lock().unwrap().keys.is_empty());
    }

    #[test]
    fn output_capacity_rejection_is_distinct_from_protocol_failure() {
        let root = TestDirectory::new();
        let state = Arc::new(Mutex::new(ExecutorState::default()));
        let runtime = runtime(&root, Arc::clone(&state), false, true);
        let mut request = request();
        request.max_output_tokens = MAX_OUTPUT_TOKENS;
        runtime.preflight_request(&request).unwrap();
        request.max_output_tokens += 1;
        assert_eq!(
            runtime.preflight_request(&request).unwrap_err().code,
            "MODEL_OUTPUT_BUDGET_EXCEEDED"
        );
        assert!(state.lock().unwrap().keys.is_empty());
    }

    #[test]
    fn unbound_generation_refuses_without_store_or_executor_effect() {
        let root = TestDirectory::new();
        let state = Arc::new(Mutex::new(ExecutorState::default()));
        let runtime = runtime(&root, Arc::clone(&state), false, true);

        let error = runtime.generate(&request()).unwrap_err();

        assert_eq!(error.code, "MODEL_CONFIG_INVALID");
        assert!(error.request_attempts.is_empty());
        assert!(state.lock().unwrap().keys.is_empty());
        assert!(!root.0.join("summarizer.db").exists());
    }

    #[test]
    fn bound_generation_projects_exact_key_and_stable_task_identity() {
        let root = TestDirectory::new();
        let state = Arc::new(Mutex::new(ExecutorState::default()));
        let mut runtime = runtime(&root, Arc::clone(&state), false, true);
        runtime.bind_run("durable-run");

        let response = runtime.generate(&request()).unwrap();

        assert_eq!(response.runtime_id, RUNTIME_ID);
        assert_eq!(response.model_id, TASK_ID);
        assert_eq!(response.request_attempts.len(), 1);
        assert_eq!(response.request_attempts[0].attempt_ordinal, 0);
        assert!(response.request_attempts[0].succeeded);
        assert_eq!(
            state.lock().unwrap().keys,
            vec![GatewayRequestKey {
                owner_id: "durable-run".to_string(),
                stage: PipelineStage::Analyze,
                ordinal: 4,
            }]
        );
    }

    #[test]
    fn profile_suggestion_attempts_share_the_bound_request_owner() {
        let root = TestDirectory::new();
        let state = Arc::new(Mutex::new(ExecutorState::default()));
        let mut runtime = runtime(&root, Arc::clone(&state), false, true);
        runtime.bind_request_owner("profile-suggestion-owner");
        let mut primary = request();
        primary.ordinal = 0;
        let mut expanded = request();
        expanded.ordinal = 1;

        runtime.generate(&primary).unwrap();
        runtime.generate(&expanded).unwrap();

        assert_eq!(
            state.lock().unwrap().keys,
            vec![
                GatewayRequestKey {
                    owner_id: "profile-suggestion-owner".to_string(),
                    stage: PipelineStage::Analyze,
                    ordinal: 0,
                },
                GatewayRequestKey {
                    owner_id: "profile-suggestion-owner".to_string(),
                    stage: PipelineStage::Analyze,
                    ordinal: 1,
                },
            ]
        );
    }

    #[test]
    fn gateway_failures_preserve_recoverability_without_private_detail() {
        let root = TestDirectory::new();
        let state = Arc::new(Mutex::new(ExecutorState::default()));
        let mut runtime = runtime(&root, state, true, true);
        runtime.bind_run("durable-run");

        let error = runtime.generate(&request()).unwrap_err();

        assert_eq!(error.code, "MODEL_GATEWAY_REJECTED");
        assert!(error.recoverable);
        assert!(!error.message.contains("PRIVATE_PROVIDER_DETAIL"));
        assert_eq!(error.request_attempts.len(), 1);
        assert_eq!(error.request_attempts[0].attempt_ordinal, 0);
        assert!(!error.request_attempts[0].succeeded);
    }

    #[test]
    fn preflight_and_health_delegate_without_generation() {
        let root = TestDirectory::new();
        let state = Arc::new(Mutex::new(ExecutorState::default()));
        let runtime = runtime(&root, Arc::clone(&state), false, false);

        runtime.preflight_request(&request()).unwrap();
        let error = runtime.health().unwrap_err();

        assert_eq!(error.code, "MODEL_GATEWAY_UNAVAILABLE");
        assert!(error.recoverable);
        let observed = state.lock().unwrap();
        assert_eq!(observed.preflight_calls, 1);
        assert_eq!(observed.health_calls, 1);
        assert!(observed.keys.is_empty());
    }

    #[test]
    fn exact_gateway_snapshot_round_trips_through_sqlite_and_mutation_is_rejected() {
        let root = TestDirectory::new();
        let state = Arc::new(Mutex::new(ExecutorState::default()));
        let runtime = runtime(&root, Arc::clone(&state), false, true);
        let snapshot = runtime.profile_snapshot().unwrap();
        assert_eq!(snapshot.version, SNAPSHOT_VERSION);
        assert_eq!(
            snapshot.analysis.runtime_kind,
            ModelRuntimeKind::InferenceGateway
        );
        assert_eq!(snapshot.analysis.model_name, TASK_ID);

        let mut conn = db::init_db(root.0.join("summarizer.db")).unwrap();
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let (_, run) = admit_pdf_for_background(
            &mut conn,
            fixture.to_str().unwrap(),
            Some(&snapshot),
            SummaryProfile::General,
            None,
        )
        .unwrap();
        assert_eq!(
            db::get_run_model_profile(&conn, &run.run_id).unwrap(),
            Some(snapshot.clone())
        );
        validate_snapshot(&snapshot).unwrap();

        let mut mutated = snapshot;
        mutated.analysis.context_tokens += 1;
        let error = validate_snapshot(&mutated).unwrap_err();
        assert_eq!(error.code, "MODEL_CONFIG_INVALID");
    }
}
