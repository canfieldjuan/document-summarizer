//! Frozen public development inputs; no prompt replacement or alternate planner.
use super::*;
use std::path::{Path, PathBuf};

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

fn request_value(r: &ModelRequest) -> Value {
    let ModelOutputFormat::JsonSchema { name, schema } = &r.output_format else {
        panic!("schema required")
    };
    json!({"stage":r.stage,"ordinal":r.ordinal,"seed":r.seed,"max_output_tokens":r.max_output_tokens,
        "system_prompt":r.system_prompt,"user_prompt":r.user_prompt,"schema_name":name,"schema":schema,
        "decoder_schema_json":serde_json::to_string(&crate::pipeline::model::response_format(&r.output_format).unwrap().unwrap()).unwrap()})
}

fn save(path: &Path, value: &Value) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
}

fn cost_report(runtime: &dyn ModelRuntime, output: Option<&Path>) -> Value {
    let f = data();
    let mut documents = Vec::new();
    for doc in f["documents"].as_array().unwrap() {
        let alias = doc["alias"].as_str().unwrap();
        let batches = match planned(doc, runtime) {
            Ok(b) => b,
            Err(e) => {
                documents.push(
                    json!({"document":alias,"admitted":false,"error":e.code,"message":e.message}),
                );
                continue;
            }
        };
        let association = validate_plan(&batches).unwrap();
        let seed = generation_seed_for_run(doc["run_id"].as_str().unwrap());
        let mut units = Vec::new();
        let mut offset = 0;
        for (parent_index, parent) in association.iter().enumerate() {
            let unit = format!("{alias}{}", parent_index + 1);
            let label = f["units"]
                .as_array()
                .unwrap()
                .iter()
                .find(|u| u["unit"] == unit)
                .unwrap();
            let mut sentences = Vec::new();
            let mut claim_catalog = BTreeSet::new();
            for (sentence_index, &(start, end)) in parent.ranges.iter().enumerate() {
                let mut dimensions = Vec::new();
                for dimension in DIMENSIONS {
                    let b = &batches[offset];
                    let request = verification_request(b, offset as u32 + 1, seed);
                    assert_eq!(
                        request.system_prompt,
                        label["baseline_system_prompts"][dimension]
                            .as_str()
                            .unwrap(),
                        "original system instruction {unit}/{dimension}"
                    );
                    let wire: Value = serde_json::from_str(&request.user_prompt).unwrap();
                    assert_eq!(wire["parent_claim_context"], parent.claim.text);
                    assert_eq!(wire["claims"][0]["text"], parent.claim.text[start..end]);
                    assert_eq!(wire["source_segments"], label["baseline_source_segments"]);
                    claim_catalog
                        .extend(b.comparison.as_ref().unwrap().claim_spans.iter().cloned());
                    let mut context = wire.clone();
                    context["claims"][0]["text"] = json!(parent.claim.text);
                    for key in ["parent_claim_context", "dimension", "claim_segments"] {
                        context.as_object_mut().unwrap().remove(key);
                    }
                    assert_eq!(
                        context, label["baseline_context_projection"],
                        "full original evidence/context {unit}"
                    );
                    let admission = runtime.preflight_request(&request);
                    if let Some(out) = output {
                        save(
                            &out.join(format!("{unit}-sentence-{sentence_index}-{dimension}.json")),
                            &request_value(&request),
                        );
                    }
                    dimensions.push(json!({"dimension":dimension,"admitted":admission.is_ok(),"error":admission.err().map(|e|json!({"code":e.code,"message":e.message})),"prompt_characters":request.system_prompt.chars().count()+request.user_prompt.chars().count(),"prompt_bytes":request.system_prompt.len()+request.user_prompt.len(),"schema_bytes":serde_json::to_vec(&b.comparison.as_ref().unwrap().schema).unwrap().len()}));
                    offset += 1;
                }
                sentences.push(json!({"ordinal":sentence_index,"byte_start":start,"byte_end":end,"text":&parent.claim.text[start..end],"dimensions":dimensions}));
            }
            assert_eq!(
                json!(claim_catalog),
                label["baseline_claim_segments"],
                "unchanged original claim pieces {unit}"
            );
            // The original full parent and unique sources remain the admission unit.
            let first = &batches[offset - parent.ranges.len() * DIMENSIONS.len()];
            let wire: Value = serde_json::from_str(&first.user_prompt).unwrap();
            let sources: BTreeSet<_> = wire["clause_contexts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["full_clause"].as_str().unwrap())
                .collect();
            units.push(json!({"unit":unit,"parent_id":parent.claim.claim_id,"parent_text":parent.claim.text,"label":label["independent_label"],"seed":seed,"claim_and_unique_source_characters":parent.claim.text.chars().count()+sources.iter().map(|s|s.chars().count()).sum::<usize>(),"old_calls":4,"new_calls":parent.ranges.len()*4,"sentence_count":parent.ranges.len(),"sentences":sentences}));
        }
        let admitted = units.iter().all(|u| {
            u["sentences"].as_array().unwrap().iter().all(|s| {
                s["dimensions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|d| d["admitted"] == true)
            })
        });
        documents.push(json!({"document":alias,"admitted":admitted,"old_calls":association.len()*4,"new_calls":batches.len(),"sentence_count":batches.len()/4,"units":units}));
    }
    let coverage = static_catalog_coverage(fixture()["records"].as_array().unwrap());
    let controls:Vec<_>=fixture()["records"].as_array().unwrap().iter().filter(|r|r["operator_expected_admit"].is_boolean()).map(|r|{
        let (prompt,claims)=input(r);
        match plan(runtime,&prompt,&claims,8) {
            Ok(batches)=>{
                let rows:Vec<_>=batches.iter().enumerate().map(|(n,b)|{
                    let request=verification_request(b,n as u32,7);
                    if let Some(out)=output { save(&out.join(format!("control-{}-{n}.json",r["case"].as_str().unwrap())),&request_value(&request)); }
                    let admission=runtime.preflight_request(&request);
                    json!({"admitted":admission.is_ok(),"error":admission.err().map(|e|e.code),"prompt_characters":request.system_prompt.chars().count()+request.user_prompt.chars().count(),"schema_bytes":serde_json::to_vec(&b.comparison.as_ref().unwrap().schema).unwrap().len()})
                }).collect();
                json!({"case":r["case"],"admitted":rows.iter().all(|d|d["admitted"]==true),"calls_per_repetition":batches.len(),"repetitions":3,"requests":rows})
            }
            Err(e)=>json!({"case":r["case"],"admitted":false,"error":e.code})
        }
    }).collect();
    json!({"runtime":runtime.runtime_id(),"model":runtime.model_id(),"comparison_call_limit":MAX_COMPARISON_CALLS,"inference_calls":0,"documents":documents,"controls":controls,"static_coverage":coverage,"gate_passed":documents.iter().all(|d|d["admitted"]==true)&&controls.iter().all(|d|d["admitted"]==true)&&coverage["gate_passed"]==true})
}

#[test]
fn public_sentence_projection_preserves_frozen_instructions() {
    let report = cost_report(&Runtime::default(), None);
    println!("C9_SENTENCE_STATIC {}", report);
    assert_eq!(report["gate_passed"], true);
}

#[test]
#[ignore = "explicit durable output and frozen gateway profile required; no inference"]
fn gateway_sentence_cost_preflight() {
    use crate::pipeline::{gateway_client::GatewayClientConfig, gateway_runtime::GatewayRuntime};
    let out = PathBuf::from(std::env::var("DOC_SUM_C9_SENTENCE_OUTPUT").unwrap());
    let settings: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("DOC_SUM_C9_GATEWAY_SETTINGS").unwrap()).unwrap(),
    )
    .unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let runtime = GatewayRuntime::from_snapshot(
        scratch.path().join("unused.sqlite"),
        GatewayClientConfig {
            base_url: "https://127.0.0.1:9".into(),
            token_file: scratch.path().join("no-token"),
            ca_file: settings["gateway"]["caFile"].as_str().unwrap().into(),
            timeout: std::time::Duration::from_secs(900),
            request_lifetime: std::time::Duration::from_secs(900),
        },
        &serde_json::from_value(data()["documents"][0]["profile"].clone()).unwrap(),
    )
    .unwrap();
    let report = cost_report(&runtime, Some(&out));
    save(&out.join("cost-report.json"), &report);
    let boundaries = measure_admission_boundaries(&runtime);
    save(&out.join("boundaries.json"), &boundaries);
    println!("C9_SENTENCE_ADMISSION {}", report["gate_passed"]);
    // Admission failures are a recorded experiment stop, not an invitation to tune.
}

fn label_passes(label: &str, verdict: &ClaimVerdict) -> bool {
    match label {
        "faithful" => *verdict == ClaimVerdict::Supported,
        "withhold" | "needs more context" => *verdict != ClaimVerdict::Supported,
        _ => false,
    }
}

#[derive(Clone)]
struct ParentCheck {
    unit: String,
    label: String,
    start: usize,
    end: usize,
    prepared: Vec<Prepared>,
    control: Option<Value>,
}

fn elapsed_span(responses: &[Value]) -> Option<u64> {
    responses.last()?["end_elapsed_ms"]
        .as_u64()?
        .checked_sub(responses.first()?["start_elapsed_ms"].as_u64()?)
}

fn parent_observation(check: &ParentCheck, responses: &[Value]) -> Value {
    let mut row = json!({"unit":check.unit,"label":check.label,"complete":false,"passed":false,"sentences":[],"parent_verdict":null});
    if responses.len() != check.prepared.len() {
        return row;
    }
    let mut sentence_rows = Vec::new();
    let mut verdicts = Vec::new();
    for (prepared, responses) in check
        .prepared
        .as_chunks::<4>()
        .0
        .iter()
        .zip(responses.as_chunks::<4>().0.iter())
    {
        let parsed: Result<Vec<_>, _> = prepared
            .iter()
            .zip(responses)
            .map(|(p, r)| p.parse(r["text"].as_str().unwrap_or("")))
            .collect();
        let Ok(parsed) = parsed else {
            return row;
        };
        let verdict = aggregate(&parsed.try_into().unwrap());
        let owned = prepared[0].sentence.as_ref().unwrap();
        let parent = &owned.document[owned.parent];
        let (start, end) = parent.ranges[owned.ordinal];
        sentence_rows.push(json!({"ordinal":owned.ordinal,"byte_start":start,"byte_end":end,"text":&parent.claim.text[start..end],"verdict":verdict,"elapsed_ms":elapsed_span(responses),"relations":responses.iter().map(|r|serde_json::from_str::<Value>(r["text"].as_str().unwrap()).unwrap()["relation"].clone()).collect::<Vec<_>>(),"milliseconds":responses.iter().map(|r|r["milliseconds"].as_u64().unwrap()).sum::<u64>()}));
        verdicts.push(verdict);
    }
    let verdict = aggregate_parent(&verdicts).unwrap();
    let mut passed = label_passes(&check.label, &verdict);
    if let Some(record) = &check.control {
        // Frozen controls each contain exactly one sentence; no synthetic
        // sentence labels or rewritten reference spans are introduced.
        assert_eq!(check.prepared.len(), 4);
        let scored = scoring_wrapper(responses).unwrap();
        let accuracy = passage_accuracy(
            &check.prepared[0],
            scored["text"].as_str().unwrap(),
            record["raw_response"].as_str().unwrap(),
        )
        .unwrap();
        passed &= serde_json::to_value(&verdict).unwrap() == record["recorded_verdict"]
            && accuracy["coverage_complete"] == true;
        row["passage_accuracy"] = accuracy;
        row["exact_verdict_parity"] =
            json!(serde_json::to_value(&verdict).unwrap() == record["recorded_verdict"]);
    }
    row["sentences"] = json!(sentence_rows);
    row["elapsed_ms"] = json!(elapsed_span(responses));
    row["complete"] = json!(true);
    row["passed"] = json!(passed);
    row["parent_verdict"] = json!(verdict);
    row["wrong_approval"] = json!(check.label != "faithful" && verdict == ClaimVerdict::Supported);
    row["milliseconds"] = json!(responses
        .iter()
        .map(|r| r["milliseconds"].as_u64().unwrap())
        .sum::<u64>());
    row
}

fn stopped() -> ModelRuntimeFailure {
    ModelRuntimeFailure {
        code: "C9_DEVELOPMENT_STOP".into(),
        message: "Frozen sentence experiment stopped at its first failed gate".into(),
        recoverable: false,
        request_attempts: vec![],
    }
}

struct RecordingRuntime {
    inner: Arc<dyn ModelRuntime>,
    allowed: Vec<ModelRequest>,
    checks: Vec<ParentCheck>,
    output: PathBuf,
    responses: std::sync::Mutex<Vec<Value>>,
    started: std::time::Instant,
}

impl ModelRuntime for RecordingRuntime {
    fn generate(&self, request: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        self.generate_with_control(request, &UNCONTROLLED_EXECUTION)
    }
    fn generate_with_control(
        &self,
        request: &ModelRequest,
        control: &dyn ExecutionControl,
    ) -> Result<ModelResponse, ModelRuntimeFailure> {
        let mut responses = self.responses.lock().unwrap();
        let index = responses.len();
        if self.allowed.get(index) != Some(request) {
            return Err(stopped());
        }
        save(
            &self.output.join(format!("request-{index}.json")),
            &request_value(request),
        );
        let start = std::time::Instant::now();
        let start_elapsed_ms = self.started.elapsed().as_millis();
        let result = self.inner.generate_with_control(request, control);
        let mut row = match &result {
            Ok(r) => {
                json!({"text":r.text,"runtime_id":r.runtime_id,"model_id":r.model_id,"request_attempts":r.request_attempts,"output_characters":r.text.chars().count(),"output_bytes":r.text.len()})
            }
            Err(e) => {
                json!({"error":e.code,"message":e.message,"request_attempts":e.request_attempts})
            }
        };
        row["milliseconds"] = json!(start.elapsed().as_millis());
        row["start_elapsed_ms"] = json!(start_elapsed_ms);
        row["end_elapsed_ms"] = json!(self.started.elapsed().as_millis());
        row["ordinal"] = json!(request.ordinal);
        row["output_tokens"] = Value::Null;
        row["finish_reason"] = Value::Null;
        save(&self.output.join(format!("response-{index}.json")), &row);
        responses.push(row);
        if let Some(check) = self.checks.iter().find(|c| c.end == responses.len()) {
            let observation = parent_observation(check, &responses[check.start..check.end]);
            save(
                &self.output.join(format!("parent-{}.json", check.unit)),
                &observation,
            );
            println!(
                "C9_SENTENCE_PARENT {}",
                json!({"unit":check.unit,"verdict":observation["parent_verdict"],"passed":observation["passed"],"milliseconds":observation["milliseconds"]})
            );
            if observation["passed"] != true {
                return Err(stopped());
            }
        }
        result
    }
    fn preflight_request(&self, r: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        self.inner.preflight_request(r)
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

fn admitted_runtime(out: &Path, route: &str, runtime_parent: &Path) -> Arc<dyn ModelRuntime> {
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
        })
        .unwrap()
    }
}

#[test]
fn sentence_scorer_rejects_missing_relations_and_unsafe_approval() {
    let (prompt, claims) = sentence_document(&[2]);
    let batches = plan(&Runtime::default(), &prompt, &claims, 8).unwrap();
    let check = ParentCheck {
        unit: "fixture".into(),
        label: "faithful".into(),
        start: 0,
        end: 8,
        prepared: batches
            .iter()
            .map(|b| b.comparison.clone().unwrap())
            .collect(),
        control: None,
    };
    let responses:Vec<_>=batches.iter().map(|b|json!({"text":fixture_response(&verification_request(b,0,7),false),"milliseconds":1})).collect();
    assert_eq!(parent_observation(&check, &responses)["passed"], true);
    for n in [0, 1, 4, 7] {
        assert_eq!(parent_observation(&check, &responses[..n])["passed"], false);
    }
    let mut check = check;
    for label in ["withhold", "needs more context", "unknown"] {
        check.label = label.into();
        assert_eq!(parent_observation(&check, &responses)["passed"], false);
    }
    check.label = "faithful".into();
    for i in 0..responses.len() {
        let mut bad = responses.clone();
        bad[i]["text"] = json!("{}");
        assert_eq!(parent_observation(&check, &bad)["passed"], false);
    }
}

#[test]
#[ignore = "accepted sentence contract, frozen source, deployment receipt and exclusive inference lock required"]
fn sentence_candidate_admission_or_live() {
    struct Shutdown;
    impl Drop for Shutdown {
        fn drop(&mut self) {
            crate::pipeline::llama_cpp::shutdown_managed_runtimes();
        }
    }
    let _shutdown = Shutdown;
    let out = PathBuf::from(std::env::var("DOC_SUM_C9_SENTENCE_OUTPUT").unwrap());
    let route = std::env::var("DOC_SUM_C9_RUNTIME").unwrap();
    let admission_only = std::env::var("DOC_SUM_C9_BOUNDARIES_ONLY").as_deref() == Ok("1");
    let scratch = tempfile::Builder::new()
        .prefix("c9-sent-")
        .tempdir()
        .unwrap();
    let inner = admitted_runtime(&out, &route, scratch.path());
    let cost = cost_report(inner.as_ref(), Some(&out));
    save(&out.join("cost-report.json"), &cost);
    if cost["gate_passed"] != true {
        save(
            &out.join("results.json"),
            &json!({"gate_passed":false,"admission_failed":true,"actual_calls":0}),
        );
        return;
    }
    if admission_only {
        let boundaries = measure_admission_boundaries(inner.as_ref());
        save(&out.join("boundaries.json"), &boundaries);
        save(
            &out.join("results.json"),
            &json!({"gate_passed":true,"admission_only":true,"actual_calls":0}),
        );
        return;
    }
    assert_eq!(route, "gateway");
    let f = data();
    let mut plans = Vec::new();
    let mut seeds = Vec::new();
    let mut allowed = Vec::new();
    let mut checks = Vec::new();
    for doc in f["documents"].as_array().unwrap() {
        let batches = planned(doc, inner.as_ref()).unwrap();
        let association = validate_plan(&batches).unwrap();
        let seed = generation_seed_for_run(doc["run_id"].as_str().unwrap());
        let mut offset = 0;
        for (n, parent) in association.iter().enumerate() {
            let unit = format!("{}{}", doc["alias"].as_str().unwrap(), n + 1);
            let label = f["units"]
                .as_array()
                .unwrap()
                .iter()
                .find(|u| u["unit"] == unit)
                .unwrap()["independent_label"]
                .as_str()
                .unwrap()
                .to_string();
            let count = parent.ranges.len() * 4;
            checks.push(ParentCheck {
                unit,
                label,
                start: allowed.len() + offset,
                end: allowed.len() + offset + count,
                prepared: batches[offset..offset + count]
                    .iter()
                    .map(|b| b.comparison.clone().unwrap())
                    .collect(),
                control: None,
            });
            offset += count;
        }
        for b in &batches {
            allowed.push(verification_request(b, allowed.len() as u32, seed));
        }
        plans.push(batches);
        seeds.push(seed);
    }
    for record in fixture()["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["operator_expected_admit"].is_boolean())
    {
        for repetition in 1..=3 {
            let (prompt, claims) = input(record);
            let batches = plan(inner.as_ref(), &prompt, &claims, 8).unwrap();
            assert_eq!(batches.len(), 4);
            checks.push(ParentCheck {
                unit: format!("{}-{repetition}", record["case"].as_str().unwrap()),
                label: if record["operator_expected_admit"] == true {
                    "faithful"
                } else {
                    "withhold"
                }
                .into(),
                start: allowed.len(),
                end: allowed.len() + batches.len(),
                prepared: batches
                    .iter()
                    .map(|b| b.comparison.clone().unwrap())
                    .collect(),
                control: Some(record.clone()),
            });
            for b in &batches {
                allowed.push(verification_request(b, allowed.len() as u32, 7));
            }
            plans.push(batches);
            seeds.push(7);
        }
    }
    // Preflight all required full documents and controls before the first call.
    for r in &allowed {
        inner.preflight_request(r).unwrap();
    }
    save(
        &out.join("execution-plan.json"),
        &json!({"maximum_semantic_calls":allowed.len(),"parents":checks.iter().map(|c|json!({"unit":c.unit,"start":c.start,"end":c.end,"label":c.label})).collect::<Vec<_>>(),"seeds":seeds,"stop_on_first_failure":true}),
    );
    let runtime = RecordingRuntime {
        inner,
        allowed,
        checks,
        output: out.clone(),
        responses: std::sync::Mutex::new(vec![]),
        started: std::time::Instant::now(),
    };
    let mut ordinal = 0;
    let mut failure = Value::Null;
    let mut plan_times = Vec::new();
    for (plan_index, (batches, seed)) in plans.into_iter().zip(seeds).enumerate() {
        let started = std::time::Instant::now();
        let result = classify(
            &runtime,
            batches,
            seed,
            &mut ordinal,
            &UNCONTROLLED_EXECUTION,
        );
        plan_times.push(json!({"plan_index":plan_index,"milliseconds":started.elapsed().as_millis(),"complete":result.is_ok()}));
        if let Err(e) = result {
            failure = json!({"code":e.code,"message":e.message});
            break;
        }
    }
    let responses = runtime.responses.lock().unwrap();
    let rows: Vec<_> = runtime
        .checks
        .iter()
        .map(|check| {
            let available = responses.get(check.start..check.end).unwrap_or(&[]);
            parent_observation(check, available)
        })
        .collect();
    let passed = failure.is_null()
        && rows.iter().all(|r| r["passed"] == true)
        && responses.len() == runtime.allowed.len();
    save(
        &out.join("results.json"),
        &json!({"development_only":true,"fidelity_qualified":false,"gate_passed":passed,"stopped_on_first_failure":!passed,"actual_calls":responses.len(),"maximum_semantic_calls":runtime.allowed.len(),"rows":rows,"plan_times":plan_times,"failure":failure,"output_tokens_and_finish_reason":"not exposed by ModelResponse"}),
    );
    println!(
        "C9_SENTENCE_RESULT {}",
        json!({"gate_passed":passed,"actual_calls":responses.len(),"failure":failure})
    );
}
