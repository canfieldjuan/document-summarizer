#![allow(
    dead_code,
    reason = "the run-bound adapter lands before persisted gateway selection"
)]

use crate::pipeline::contracts::{
    ModelProfileSnapshot, ModelRequest, ModelRequestAttemptDiagnostic, ModelResponse, ModelRuntime,
    ModelRuntimeFailure, ModelRuntimeKind, ModelStageProfileSnapshot, ModelTokenUsage,
    ModelTransportAttempt, PipelineStage,
};
use crate::pipeline::db;
use crate::pipeline::gateway_client::{
    GatewayClient, GatewayClientConfig, GatewayClientError, GatewayHealth, GatewayResult,
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
const TASK_ID: &str = "document.summary.step@1";
const TASK_PROFILE_ID: &str = "gateway-document-summary-step-v1";
const TASK_PROFILE_VERSION: &str = "gateway-task-profile-v1";
const CONTEXT_TOKENS: u32 = 8_192;
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
    db_path: PathBuf,
    executor: Arc<dyn GatewayExecutor>,
    bound_run_id: Option<String>,
    snapshot: ModelProfileSnapshot,
}

impl GatewayRuntime {
    pub(crate) fn new(
        db_path: PathBuf,
        config: GatewayClientConfig,
    ) -> Result<Self, ModelRuntimeFailure> {
        let client = GatewayClient::new(config).map_err(map_client_failure)?;
        Ok(Self::with_executor(db_path, Arc::new(client)))
    }

    pub(crate) fn from_snapshot(
        db_path: PathBuf,
        config: GatewayClientConfig,
        snapshot: &ModelProfileSnapshot,
    ) -> Result<Self, ModelRuntimeFailure> {
        validate_snapshot(snapshot)?;
        Self::new(db_path, config)
    }

    fn with_executor(db_path: PathBuf, executor: Arc<dyn GatewayExecutor>) -> Self {
        Self {
            db_path,
            executor,
            bound_run_id: None,
            snapshot: canonical_snapshot(),
        }
    }
}

impl ModelRuntime for GatewayRuntime {
    fn bind_run(&mut self, run_id: &str) {
        self.bound_run_id = Some(run_id.to_string());
    }

    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        let run_id = self.bound_run_id.as_deref().ok_or_else(|| {
            failure(
                "MODEL_CONFIG_INVALID",
                "Inference gateway runtime is not bound to a durable pipeline run",
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
            run_id: run_id.to_string(),
            stage: request.stage.clone(),
            ordinal: request.ordinal,
        };
        match self.executor.execute(&mut conn, &key, request) {
            Ok(result) => Ok(ModelResponse {
                text: result.content,
                runtime_id: RUNTIME_ID.to_string(),
                model_id: TASK_ID.to_string(),
                request_attempts: vec![diagnostic(request, started, true)],
            }),
            Err(error) => {
                let mut mapped = map_client_failure(error);
                mapped.request_attempts = vec![diagnostic(request, started, false)];
                Err(mapped)
            }
        }
    }

    fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
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
        TASK_ID
    }

    fn context_tokens(&self, _stage: PipelineStage) -> u32 {
        CONTEXT_TOKENS
    }

    fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
        Some(self.snapshot.clone())
    }
}

fn canonical_snapshot() -> ModelProfileSnapshot {
    let stage = ModelStageProfileSnapshot {
        runtime_kind: ModelRuntimeKind::InferenceGateway,
        profile_id: TASK_PROFILE_ID.to_string(),
        model_name: TASK_ID.to_string(),
        model_digest: format!("{:x}", Sha256::digest(TASK_PROFILE_CANONICAL.as_bytes())),
        context_tokens: CONTEXT_TOKENS,
        tokenizer_version: TASK_PROFILE_VERSION.to_string(),
    };
    ModelProfileSnapshot {
        version: SNAPSHOT_VERSION,
        preset_id: TASK_PROFILE_ID.to_string(),
        analysis: stage.clone(),
        verification: stage,
    }
}

fn validate_snapshot(snapshot: &ModelProfileSnapshot) -> Result<(), ModelRuntimeFailure> {
    if snapshot == &canonical_snapshot() {
        Ok(())
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
        attempt_ordinal: 1,
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
        assert!(response.request_attempts[0].succeeded);
        assert_eq!(
            state.lock().unwrap().keys,
            vec![GatewayRequestKey {
                run_id: "durable-run".to_string(),
                stage: PipelineStage::Analyze,
                ordinal: 4,
            }]
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
