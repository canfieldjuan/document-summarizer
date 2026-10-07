//! Shared qualification runtime and zero-generation public-document inventory.
use super::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn data() -> Value {
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
    runtime_parent: &Path,
) -> Arc<dyn ModelRuntime> {
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
        Arc::new(runtime)
    } else {
        assert_eq!(route, "native");
        use crate::pipeline::llama_cpp::{self, GgufRuntimeConfig, LlamaCppRuntime};
        use crate::pipeline::model_settings::{
            JACK_LLAMA_CPP_LIBRARIES, QUALIFIED_LLAMA_SERVER_DIGEST,
        };
        let (model_path, size, digest, identity) = llama_cpp::inspect_regular_file(Path::new(
            &std::env::var("DOC_SUM_QUALIFICATION_GGUF").unwrap(),
        ))
        .unwrap();
        assert_eq!(
            digest,
            "cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13"
        );
        save(
            &out.join("runtime.json"),
            &json!({"route":route,"model_digest":digest,"server_digest":QUALIFIED_LLAMA_SERVER_DIGEST,"context_tokens":32768,"thinking":false}),
        );
        LlamaCppRuntime::shared(GgufRuntimeConfig {
            model_path,
            runtime_parent: runtime_parent.into(),
            model_digest: digest,
            expected_size_bytes: size,
            expected_file_identity: identity,
            expected_server_digest: QUALIFIED_LLAMA_SERVER_DIGEST.into(),
            expected_runtime_libraries: JACK_LLAMA_CPP_LIBRARIES,
            context_tokens: 32768,
            disable_thinking: true,
        })
        .unwrap()
    }
}

#[test]
#[ignore = "zero generation public A/B comparison after corrected parity"]
fn public_document_admission_inventory() {
    let out = PathBuf::from(std::env::var("DOC_SUM_C9_PARITY_OUTPUT").unwrap());
    let scratch = tempfile::tempdir().unwrap();
    let runtime = admitted_runtime(&out, "gateway", scratch.path());
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
