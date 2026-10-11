//! Shared qualification runtime and zero-generation public-document inventory.
use super::*;
use crate::pipeline::contracts::ModelProfileSnapshot;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::pipeline::live_runtime;

const NATIVE_9B_DIGEST: &str = "cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13";

fn native_experiment_snapshot(runtime: &dyn ModelRuntime) -> ModelProfileSnapshot {
    use crate::pipeline::contracts::ModelRuntimeKind;
    let snapshot = runtime
        .profile_snapshot()
        .expect("selected factory snapshot required");
    assert_eq!(snapshot.version, 2);
    assert_eq!(snapshot.preset_id, "full-qwen35-9b-q4km-v1");
    for (stage, selected) in [
        (PipelineStage::Analyze, &snapshot.analysis),
        (PipelineStage::Synthesize, &snapshot.analysis),
        (PipelineStage::Verify, &snapshot.verification),
    ] {
        assert_eq!(selected.runtime_kind, ModelRuntimeKind::LlamaCppGguf);
        assert_eq!(selected.model_digest, NATIVE_9B_DIGEST);
        assert_eq!(selected.profile_id, "qwen35-9b-q4km-v1");
        assert_eq!(selected.context_tokens, 32768);
        assert_eq!(
            selected.tokenizer_version,
            crate::pipeline::qwen_tokenizer::QWEN35_TOKENIZER_VERSION
        );
        assert_eq!(runtime.context_tokens(stage.clone()), 32768);
        assert_eq!(runtime.model_id_for_stage(stage), NATIVE_9B_DIGEST);
    }
    snapshot
}

pub(super) fn data() -> Value {
    serde_json::from_str(include_str!("fixtures/c9-sentence-public.json")).unwrap()
}

fn planned(
    document: &Value,
    runtime: &dyn ModelRuntime,
) -> Result<Vec<VerificationBatch>, PipelineFailure> {
    coherent::summary_verification_batches(
        SummaryProfile::General,
        runtime,
        &serde_json::from_value(document["synthesized"].clone()).unwrap(),
        &serde_json::from_value(document["normalized"].clone()).unwrap(),
    )
}

pub(super) fn request_value(r: &ModelRequest) -> Value {
    let ModelOutputFormat::JsonSchema { name, schema } = &r.output_format else {
        panic!("schema required")
    };
    json!({"stage":r.stage,"ordinal":r.ordinal,"seed":r.seed,"max_output_tokens":r.max_output_tokens,
        "system_prompt":r.system_prompt,"user_prompt":r.user_prompt,"schema_name":name,"schema":schema,
        "decoder_schema_json":serde_json::to_string(&crate::pipeline::model::response_format(&r.output_format).unwrap().unwrap()).unwrap()})
}

pub(super) fn save(path: &Path, value: &Value) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
}

pub(super) fn admitted_runtime(
    out: &Path,
    route: &str,
) -> (Arc<dyn ModelRuntime>, Option<tempfile::TempDir>) {
    if route == "gateway" {
        use crate::pipeline::{
            db, gateway_client::GatewayClientConfig, gateway_runtime::GatewayRuntime,
            service::admit_pdf_for_background,
        };
        let settings: Value = serde_json::from_slice(
            &std::fs::read(std::env::var("DOC_SUM_C9_GATEWAY_SETTINGS").unwrap()).unwrap(),
        )
        .unwrap();
        let g = &settings["gateway"];
        let database = out.join("gateway-ledger.sqlite");
        let mut runtime = GatewayRuntime::new(
            database.clone(),
            GatewayClientConfig {
                base_url: g["baseUrl"].as_str().unwrap().into(),
                token_file: g["tokenFile"].as_str().unwrap().into(),
                ca_file: g["caFile"].as_str().unwrap().into(),
                timeout: std::time::Duration::from_secs(900),
                request_lifetime: std::time::Duration::from_secs(900),
            },
        )
        .unwrap();
        let snapshot = runtime.profile_snapshot().unwrap();
        assert_eq!(
            serde_json::to_value(&snapshot).unwrap(),
            data()["documents"][0]["profile"]
        );
        let mut conn = db::init_db(&database).unwrap();
        let pdf =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured_report.pdf");
        let (_, run) = admit_pdf_for_background(
            &mut conn,
            pdf.to_str().unwrap(),
            Some(&snapshot),
            SummaryProfile::General,
            None,
        )
        .unwrap();
        runtime.bind_request_owner(&run.run_id);
        save(
            &out.join("runtime.json"),
            &json!({"snapshot":snapshot,"fresh_owner":run.run_id,"route":route}),
        );
        (Arc::new(runtime), None)
    } else {
        assert_eq!(route, "native");
        let (runtime, directory) =
            live_runtime::configured_live_runtime(&out.join("native-owner.sqlite"));
        assert!(
            directory.is_some(),
            "fresh selected GGUF directory required; refuse alternate setup"
        );
        let snapshot = native_experiment_snapshot(runtime.as_ref());
        save(
            &out.join("runtime.json"),
            &json!({"route":route,"snapshot":snapshot}),
        );
        // Caller retains the selected factory's private directory for the whole run.
        (Arc::from(runtime), directory)
    }
}

#[test]
#[ignore = "zero generation public A/B comparison after corrected parity"]
fn public_document_admission_inventory() {
    let out = PathBuf::from(std::env::var("DOC_SUM_C9_PARITY_OUTPUT").unwrap());
    let (runtime, _directory) = admitted_runtime(&out, "gateway");
    let mut rows = Vec::new();
    for doc in data()["documents"].as_array().unwrap() {
        let result = planned(doc, runtime.as_ref());
        let row = match result {
            Ok(batches) => {
                let requests: Vec<_>=batches.iter().enumerate().map(|(i,b)| {
                    let r=verification_request(b,i as u32,7);
                    let admission=runtime.preflight_request(&r);
                    json!({"request":request_value(&r),"admitted":admission.is_ok(),"error":admission.err().map(|e|e.code)})
                }).collect();
                json!({"document":doc["alias"],"planned":true,"requests":requests})
            }
            Err(e) => {
                json!({"document":doc["alias"],"planned":false,"error":e.code,"message":e.message})
            }
        };
        rows.push(row);
    }
    save(
        &out.join("public-admission.json"),
        &json!({"generation_calls":0,"documents":rows,"unqualified":true}),
    );
    println!("C9_PUBLIC_ADMISSION generation_calls=0");
}

#[test]
fn native_experiment_pins_reject_drift_before_generation() {
    use crate::pipeline::contracts::{ModelRuntimeKind, ModelStageProfileSnapshot};
    struct Metadata(Option<ModelProfileSnapshot>);
    impl ModelRuntime for Metadata {
        fn generate(&self, _: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
            panic!("identity checks must not generate")
        }
        fn health(&self) -> Result<(), ModelRuntimeFailure> {
            Ok(())
        }
        fn runtime_id(&self) -> &str {
            "selected-runtime"
        }
        fn model_id(&self) -> &str {
            NATIVE_9B_DIGEST
        }
        fn model_id_for_stage(&self, stage: PipelineStage) -> &str {
            let s = self.0.as_ref().unwrap();
            if stage == PipelineStage::Verify {
                &s.verification.model_digest
            } else {
                &s.analysis.model_digest
            }
        }
        fn context_tokens(&self, stage: PipelineStage) -> u32 {
            let s = self.0.as_ref().unwrap();
            if stage == PipelineStage::Verify {
                s.verification.context_tokens
            } else {
                s.analysis.context_tokens
            }
        }
        fn profile_snapshot(&self) -> Option<ModelProfileSnapshot> {
            self.0.clone()
        }
    }
    let stage = ModelStageProfileSnapshot {
        runtime_kind: ModelRuntimeKind::LlamaCppGguf,
        profile_id: "qwen35-9b-q4km-v1".into(),
        model_name: "owned-9b.gguf".into(),
        model_digest: NATIVE_9B_DIGEST.into(),
        context_tokens: 32768,
        tokenizer_version: crate::pipeline::qwen_tokenizer::QWEN35_TOKENIZER_VERSION.into(),
    };
    let valid = ModelProfileSnapshot {
        version: 2,
        preset_id: "full-qwen35-9b-q4km-v1".into(),
        analysis: stage.clone(),
        verification: stage,
    };
    assert_eq!(
        native_experiment_snapshot(&Metadata(Some(valid.clone()))),
        valid
    );
    assert!(std::panic::catch_unwind(|| native_experiment_snapshot(&Metadata(None))).is_err());
    for field in [
        "version",
        "preset",
        "kind",
        "profile",
        "digest",
        "tokenizer",
        "low",
        "high",
        "verifier",
    ] {
        let mut bad = valid.clone();
        match field {
            "version" => bad.version = 1,
            "preset" => bad.preset_id = "foreign".into(),
            "kind" => bad.analysis.runtime_kind = ModelRuntimeKind::OllamaNative,
            "profile" => bad.analysis.profile_id = "foreign".into(),
            "digest" => bad.analysis.model_digest = "foreign".into(),
            "tokenizer" => bad.analysis.tokenizer_version = "foreign".into(),
            "low" => bad.analysis.context_tokens = 32767,
            "high" => bad.analysis.context_tokens = 32769,
            _ => bad.verification.model_digest = "foreign".into(),
        }
        assert!(
            std::panic::catch_unwind(|| native_experiment_snapshot(&Metadata(Some(bad)))).is_err(),
            "{field}"
        );
    }
}
