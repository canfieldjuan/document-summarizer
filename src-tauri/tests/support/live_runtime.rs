use crate::pipeline::contracts::ModelRuntime;
use crate::pipeline::model_settings::{
    register_gguf, runtime_from_settings, settings_path, QwenProfileRuntime,
};
use std::{env, path::Path};

pub fn configured_live_runtime(
    db_path: &Path,
) -> (Box<dyn ModelRuntime>, Option<tempfile::TempDir>) {
    if let Some(settings_path) = env::var_os("DOC_SUM_MODEL_SETTINGS_PATH") {
        return (
            runtime_from_settings(Path::new(&settings_path), db_path)
                .expect("selected product model settings should configure"),
            None,
        );
    }
    if let Ok(analysis_model) = env::var("DOC_SUM_QUALIFICATION_ANALYSIS_MODEL") {
        let verification_model = env::var("DOC_SUM_QUALIFICATION_VERIFICATION_MODEL")
            .unwrap_or_else(|_| analysis_model.clone());
        let context_tokens = env::var("DOC_SUM_QUALIFICATION_CONTEXT_TOKENS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(8_192);
        return (
            Box::new(
                QwenProfileRuntime::qualification_candidate(
                    &analysis_model,
                    &verification_model,
                    context_tokens,
                )
                .expect("qualification stage models should configure"),
            ),
            None,
        );
    }
    let model = env::var_os("DOC_SUM_QUALIFICATION_GGUF").expect(
        "default live acceptance requires DOC_SUM_QUALIFICATION_GGUF or explicit model settings",
    );
    let directory = isolated_runtime_directory();
    let path = settings_path(directory.path());
    register_gguf(&path, Path::new(&model)).expect("default GGUF should register");
    let runtime =
        runtime_from_settings(&path, db_path).expect("default model profile should configure");
    // Keep the private socket directory alive until the acceptance run ends.
    (runtime, Some(directory))
}

pub(crate) fn isolated_runtime_directory() -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    builder.prefix("docsum-accept-");
    #[cfg(unix)]
    {
        use std::{fs, os::unix::fs::PermissionsExt};
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    builder
        .tempdir()
        .expect("isolated model settings directory")
}

#[test]
#[cfg(unix)]
fn isolated_runtime_directory_is_owner_only() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let directory = isolated_runtime_directory();
    assert_eq!(
        fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

// Shared reservation owner for the accepted native-stage qualification only.
// This module is included by test binaries, never by the shipped application.
use crate::pipeline::contracts::{ModelRequest, ModelResponse, ModelRuntimeFailure, PipelineStage};
use crate::pipeline::control::{ExecutionControl, UNCONTROLLED_EXECUTION};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct QualificationState {
    completions: Vec<serde_json::Value>,
    preflights: Vec<serde_json::Value>,
    stopped: Option<String>,
}

pub struct QualificationBudget {
    completion_limit: usize,
    preflight_limit: usize,
    state: Mutex<QualificationState>,
}

impl QualificationBudget {
    pub fn native_stages() -> Arc<Self> {
        Arc::new(Self::with_limits(41, 128))
    }
    fn with_limits(completion_limit: usize, preflight_limit: usize) -> Self {
        Self {
            completion_limit,
            preflight_limit,
            state: Mutex::new(QualificationState::default()),
        }
    }
    pub fn wrap(self: &Arc<Self>, inner: Arc<dyn ModelRuntime>) -> QualificationRuntime {
        QualificationRuntime {
            inner,
            budget: self.clone(),
        }
    }
    pub fn stop(&self, reason: &str) {
        self.state
            .lock()
            .unwrap()
            .stopped
            .get_or_insert_with(|| reason.into());
    }
    pub fn observation(&self) -> serde_json::Value {
        let state = self.state.lock().unwrap();
        serde_json::json!({"completion_limit":self.completion_limit,"preflight_limit":self.preflight_limit,
            "reserved_completions":state.completions.len(),"delegated_preflights":state.preflights.len(),
            "completions":state.completions,"preflights":state.preflights,"stopped":state.stopped})
    }
}

fn qualification_failure(code: &str) -> ModelRuntimeFailure {
    ModelRuntimeFailure {
        code: code.into(),
        message: "qualification reservation stopped; see run receipt".into(),
        recoverable: false,
        request_attempts: vec![],
    }
}

pub struct QualificationRuntime {
    inner: Arc<dyn ModelRuntime>,
    budget: Arc<QualificationBudget>,
}

impl ModelRuntime for QualificationRuntime {
    fn generate(&self, r: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        self.generate_with_control(r, &UNCONTROLLED_EXECUTION)
    }
    fn generate_with_control(
        &self,
        r: &ModelRequest,
        c: &dyn ExecutionControl,
    ) -> Result<ModelResponse, ModelRuntimeFailure> {
        // Hold the shared state through delegation: concurrent callers cannot cross a stop.
        let mut state = self.budget.state.lock().unwrap();
        if state.stopped.is_some() {
            return Err(qualification_failure("QUALIFICATION_STOPPED"));
        }
        // Initial page requests retain the owning run seed. The page owner derives
        // a different seed for a repair. Reject that departure before delegation;
        // no generated text or prompt is re-parsed here.
        if r.stage == PipelineStage::Analyze
            && state
                .completions
                .iter()
                .find(|row| row["stage"] == serde_json::json!(PipelineStage::Analyze))
                .is_some_and(|first| first["seed"] != serde_json::json!(r.seed))
        {
            state.stopped = Some("QUALIFICATION_ANALYSIS_SEED_CHANGED".into());
            return Err(qualification_failure("QUALIFICATION_ANALYSIS_SEED_CHANGED"));
        }
        let stage_limit = match r.stage {
            PipelineStage::Verify => 30,
            PipelineStage::Analyze => 10,
            PipelineStage::Synthesize => 1,
            _ => 0,
        };
        let same_stage = state
            .completions
            .iter()
            .filter(|row| row["stage"] == serde_json::json!(r.stage))
            .count();
        if state.completions.len() >= self.budget.completion_limit || same_stage >= stage_limit {
            state.stopped = Some("QUALIFICATION_COMPLETION_CEILING".into());
            return Err(qualification_failure("QUALIFICATION_COMPLETION_CEILING"));
        }
        let i = state.completions.len();
        state
            .completions
            .push(serde_json::json!({"stage":r.stage,"ordinal":r.ordinal,"seed":r.seed,"decision":"pending"}));
        let result = self.inner.generate_with_control(r, c);
        state.completions[i]["decision"] = serde_json::json!(match &result {
            Ok(_) => "completed",
            Err(e) => &e.code,
        });
        if let Err(error) = &result {
            state.stopped = Some(error.code.clone());
        }
        result
    }
    fn preflight_request(&self, r: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        let mut state = self.budget.state.lock().unwrap();
        if state.stopped.is_some() {
            return Err(qualification_failure("QUALIFICATION_STOPPED"));
        }
        if state.preflights.len() >= self.budget.preflight_limit {
            state.stopped = Some("QUALIFICATION_PREFLIGHT_CEILING".into());
            return Err(qualification_failure("QUALIFICATION_PREFLIGHT_CEILING"));
        }
        let i = state.preflights.len();
        state
            .preflights
            .push(serde_json::json!({"stage":r.stage,"ordinal":r.ordinal,"decision":"pending"}));
        let result = self.inner.preflight_request(r);
        state.preflights[i]["decision"] = serde_json::json!(match &result {
            Ok(_) => "accepted",
            Err(e) => &e.code,
        });
        if let Err(e) = &result {
            // Failed capacity probes are part of the existing owning budget search.
            if !matches!(
                e.code.as_str(),
                "MODEL_CONTEXT_EXCEEDED" | "MODEL_OUTPUT_BUDGET_EXCEEDED"
            ) {
                state.stopped = Some(e.code.clone());
            }
        }
        result
    }
    fn request_owner_contract(&self, contract: &str) -> String {
        self.inner.request_owner_contract(contract)
    }
    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        self.inner.health()
    }
    fn runtime_id(&self) -> &str {
        self.inner.runtime_id()
    }
    fn model_id(&self) -> &str {
        self.inner.model_id()
    }
    fn runtime_id_for_stage(&self, s: PipelineStage) -> &str {
        self.inner.runtime_id_for_stage(s)
    }
    fn model_id_for_stage(&self, s: PipelineStage) -> &str {
        self.inner.model_id_for_stage(s)
    }
    fn profile_snapshot(&self) -> Option<crate::pipeline::contracts::ModelProfileSnapshot> {
        self.inner.profile_snapshot()
    }
    fn context_tokens(&self, s: PipelineStage) -> u32 {
        self.inner.context_tokens(s)
    }
    fn supports_response_schema(&self, n: &str) -> bool {
        self.inner.supports_response_schema(n)
    }
    fn response_schema_byte_limit(&self, s: PipelineStage, n: &str) -> usize {
        self.inner.response_schema_byte_limit(s, n)
    }
}

#[cfg(test)]
mod qualification_budget_tests {
    use super::*;
    use crate::pipeline::contracts::ModelOutputFormat;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Probe {
        calls: AtomicUsize,
        admissions: AtomicUsize,
    }
    impl Probe {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                admissions: AtomicUsize::new(0),
            })
        }
    }
    impl ModelRuntime for Probe {
        fn generate(&self, _: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ModelResponse {
                text: "{}".into(),
                runtime_id: "probe".into(),
                model_id: "probe".into(),
                request_attempts: vec![],
            })
        }
        fn preflight_request(&self, _: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
            self.admissions.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }
        fn runtime_id(&self) -> &str {
            "probe"
        }
        fn model_id(&self) -> &str {
            "probe"
        }
    }
    fn request(stage: PipelineStage, ordinal: u32) -> ModelRequest {
        ModelRequest {
            stage,
            ordinal,
            system_prompt: "system".into(),
            user_prompt: "owned input".into(),
            seed: 7,
            max_output_tokens: 8,
            output_format: ModelOutputFormat::Text,
        }
    }
    #[test]
    fn qualification_stages_share_the_exact_total_ceiling() {
        let probe = Probe::new();
        let budget = QualificationBudget::native_stages();
        let first = budget.wrap(probe.clone());
        let second = budget.wrap(probe.clone());
        for i in 0..30 {
            first.generate(&request(PipelineStage::Verify, i)).unwrap();
        }
        assert_eq!(budget.observation()["reserved_completions"], 30);
        for i in 0..10 {
            second
                .generate(&request(PipelineStage::Analyze, i))
                .unwrap();
        }
        assert_eq!(budget.observation()["reserved_completions"], 40);
        second
            .generate(&request(PipelineStage::Synthesize, 0))
            .unwrap();
        assert_eq!(budget.observation()["reserved_completions"], 41);
        assert!(second
            .generate(&request(PipelineStage::Synthesize, 1))
            .is_err());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 41);
        assert!(first.generate(&request(PipelineStage::Verify, 30)).is_err());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 41);
        assert!(first
            .preflight_request(&request(PipelineStage::Analyze, 11))
            .is_err());
    }
    #[test]
    fn qualification_preflights_reserve_before_concurrent_delegation() {
        let probe = Probe::new();
        let budget = QualificationBudget::native_stages();
        let runtime = budget.wrap(probe.clone());
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for i in 0..17 {
                        let _ = runtime.preflight_request(&request(PipelineStage::Verify, i));
                    }
                });
            }
        });
        assert_eq!(probe.admissions.load(Ordering::SeqCst), 128);
        assert_eq!(budget.observation()["delegated_preflights"], 128);
        assert!(runtime
            .generate(&request(PipelineStage::Verify, 0))
            .is_err());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn qualification_explicit_stop_refuses_all_later_operations() {
        let probe = Probe::new();
        let budget = QualificationBudget::native_stages();
        let runtime = budget.wrap(probe.clone());
        budget.stop("FAILED_CASE_CHECK");
        assert!(runtime
            .generate(&request(PipelineStage::Verify, 0))
            .is_err());
        assert!(runtime
            .preflight_request(&request(PipelineStage::Verify, 0))
            .is_err());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
        assert_eq!(probe.admissions.load(Ordering::SeqCst), 0);
        assert_eq!(budget.observation()["stopped"], "FAILED_CASE_CHECK");
    }
    #[test]
    fn qualification_stage_and_request_error_stop_without_retry() {
        struct Failing;
        impl ModelRuntime for Failing {
            fn generate(&self, _: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
                Err(qualification_failure("MODEL_OUTPUT_LIMIT_REACHED"))
            }
            fn health(&self) -> Result<(), ModelRuntimeFailure> {
                Ok(())
            }
            fn runtime_id(&self) -> &str {
                "probe"
            }
            fn model_id(&self) -> &str {
                "probe"
            }
        }
        let budget = QualificationBudget::native_stages();
        let runtime = budget.wrap(Arc::new(Failing));
        assert_eq!(
            runtime
                .generate(&request(PipelineStage::Analyze, 0))
                .unwrap_err()
                .code,
            "MODEL_OUTPUT_LIMIT_REACHED"
        );
        assert_eq!(budget.observation()["reserved_completions"], 1);
        assert_eq!(
            runtime
                .generate(&request(PipelineStage::Analyze, 1))
                .unwrap_err()
                .code,
            "QUALIFICATION_STOPPED"
        );
        assert_eq!(budget.observation()["reserved_completions"], 1);
        let probe = Probe::new();
        let budget = QualificationBudget::native_stages();
        let runtime = budget.wrap(probe.clone());
        runtime
            .generate(&request(PipelineStage::Synthesize, 0))
            .unwrap();
        assert!(runtime
            .generate(&request(PipelineStage::Synthesize, 1))
            .is_err());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn qualification_zero_budget_never_delegates() {
        let probe = Probe::new();
        let budget = Arc::new(QualificationBudget::with_limits(0, 0));
        let runtime = budget.wrap(probe.clone());
        assert!(
            runtime
                .generate(&request(PipelineStage::Verify, 0))
                .is_err(),
            "generation delegated past zero-call ceiling"
        );
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
        assert_eq!(probe.admissions.load(Ordering::SeqCst), 0);
        let budget = Arc::new(QualificationBudget::with_limits(1, 0));
        let runtime = budget.wrap(probe.clone());
        assert!(runtime
            .preflight_request(&request(PipelineStage::Verify, 0))
            .is_err());
        assert_eq!(probe.admissions.load(Ordering::SeqCst), 0);
        assert_eq!(budget.observation()["delegated_preflights"], 0);
    }

    #[test]
    fn qualification_cancelled_reservation_is_spent_and_terminal() {
        let probe = Probe::new();
        let budget = QualificationBudget::native_stages();
        let runtime = budget.wrap(probe.clone());
        let token = crate::pipeline::control::CancellationToken::new();
        token.request();
        assert_eq!(
            runtime
                .generate_with_control(&request(PipelineStage::Verify, 0), &token)
                .unwrap_err()
                .code,
            "MODEL_REQUEST_CANCELLED"
        );
        assert_eq!(budget.observation()["reserved_completions"], 1);
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
        assert!(runtime
            .generate(&request(PipelineStage::Verify, 1))
            .is_err());
        assert_eq!(budget.observation()["reserved_completions"], 1);
    }

    #[test]
    fn qualification_failed_admission_is_spent_and_capacity_search_is_bounded() {
        struct Rejected(&'static str);
        impl ModelRuntime for Rejected {
            fn generate(&self, _: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
                panic!("admission probe must not generate")
            }
            fn preflight_request(&self, _: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
                Err(qualification_failure(self.0))
            }
            fn health(&self) -> Result<(), ModelRuntimeFailure> {
                Ok(())
            }
            fn runtime_id(&self) -> &str {
                "probe"
            }
            fn model_id(&self) -> &str {
                "probe"
            }
        }
        for code in ["MODEL_CONTEXT_EXCEEDED", "MODEL_OUTPUT_BUDGET_EXCEEDED"] {
            let budget = QualificationBudget::native_stages();
            let runtime = budget.wrap(Arc::new(Rejected(code)));
            for i in 0..127 {
                assert_eq!(
                    runtime
                        .preflight_request(&request(PipelineStage::Analyze, i))
                        .unwrap_err()
                        .code,
                    code
                );
            }
            assert_eq!(budget.observation()["delegated_preflights"], 127);
            assert!(budget.observation()["stopped"].is_null());
            assert_eq!(
                runtime
                    .preflight_request(&request(PipelineStage::Analyze, 127))
                    .unwrap_err()
                    .code,
                code
            );
            assert_eq!(budget.observation()["delegated_preflights"], 128);
            assert_eq!(
                runtime
                    .preflight_request(&request(PipelineStage::Analyze, 128))
                    .unwrap_err()
                    .code,
                "QUALIFICATION_PREFLIGHT_CEILING"
            );
            assert_eq!(budget.observation()["delegated_preflights"], 128);
        }
        let budget = QualificationBudget::native_stages();
        let runtime = budget.wrap(Arc::new(Rejected("MODEL_CONFIG_INVALID")));
        assert_eq!(
            runtime
                .preflight_request(&request(PipelineStage::Analyze, 0))
                .unwrap_err()
                .code,
            "MODEL_CONFIG_INVALID"
        );
        assert_eq!(
            runtime
                .preflight_request(&request(PipelineStage::Analyze, 1))
                .unwrap_err()
                .code,
            "QUALIFICATION_STOPPED"
        );
        assert_eq!(budget.observation()["delegated_preflights"], 1);
    }

    #[test]
    fn qualification_concurrent_generations_cannot_cross_stage_ceiling() {
        let probe = Probe::new();
        let budget = QualificationBudget::native_stages();
        let runtime = budget.wrap(probe.clone());
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for i in 0..5 {
                        let _ = runtime.generate(&request(PipelineStage::Verify, i));
                    }
                });
            }
        });
        assert_eq!(probe.calls.load(Ordering::SeqCst), 30);
        assert_eq!(budget.observation()["reserved_completions"], 30);
    }
}
