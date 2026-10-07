use crate::pipeline::contracts::ModelOutputFormat;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
fn pinned_9b_runtime() -> (tempfile::TempDir, Arc<LlamaCppRuntime>) {
    use crate::pipeline::model_settings::{
        JACK_LLAMA_CPP_LIBRARIES, QUALIFIED_LLAMA_SERVER_DIGEST,
    };
    let model = PathBuf::from(std::env::var("DOC_SUM_QUALIFICATION_GGUF").unwrap());
    let (model_path, size, digest, identity) = inspect_regular_file(&model).unwrap();
    assert_eq!(
        digest,
        "cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13"
    );
    let scratch = tempfile::tempdir().unwrap();
    let runtime = LlamaCppRuntime::shared(GgufRuntimeConfig {
        model_path,
        runtime_parent: scratch.path().into(),
        model_digest: digest,
        expected_size_bytes: size,
        expected_file_identity: identity,
        expected_server_digest: QUALIFIED_LLAMA_SERVER_DIGEST.into(),
        expected_runtime_libraries: JACK_LLAMA_CPP_LIBRARIES,
        context_tokens: 32768,
        disable_thinking: true,
    })
    .unwrap();
    (scratch, runtime)
}

use super::*;
use serde_json::{json, Value};

// Runtime links belong to the owned child's descriptor namespace.
#[cfg(target_os = "linux")]
fn captured_library_bytes(path: &Path, child_pid: u32) -> std::io::Result<Vec<u8>> {
    let target = fs::read_link(path)?;
    let descriptor = target
        .to_str()
        .and_then(|s| s.strip_prefix("/proc/self/fd/"))
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unexpected runtime library descriptor link",
            )
        })?;
    fs::read(format!("/proc/{child_pid}/fd/{descriptor}"))
}

#[cfg(target_os = "linux")]
#[test]
fn native_library_capture_reads_owned_child_descriptor() {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::process::CommandExt;
    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let library = directory.path().join("public-library");
    fs::write(&library, b"public sealed library fixture").unwrap();
    let original = File::open(&library).unwrap();
    let descriptor = unsafe { libc::fcntl(original.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 500) };
    assert!(descriptor >= 500);
    let inherited = unsafe { File::from_raw_fd(descriptor) };
    let link = directory.path().join("library-link");
    std::os::unix::fs::symlink(format!("/proc/self/fd/{descriptor}"), &link).unwrap();
    let mut command = Command::new("/bin/sleep");
    command.arg("30");
    unsafe {
        command.pre_exec(move || {
            if libc::fcntl(descriptor, libc::F_SETFD, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = OwnedChild(command.spawn().unwrap());
    drop(inherited);
    assert!(
        fs::read(&link).is_err(),
        "parent must not own the inherited descriptor"
    );
    assert_eq!(
        captured_library_bytes(&link, child.0.id())
            .expect("capture must use owned child namespace"),
        b"public sealed library fixture"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn native_library_capture_rejects_non_descriptor_links() {
    let directory = tempfile::tempdir().unwrap();
    for (i, target) in [
        "/proc/self/fd/",
        "/proc/self/fd/3/../4",
        "/proc/1/fd/3",
        "/etc/passwd",
    ]
    .iter()
    .enumerate()
    {
        let link = directory.path().join(format!("link-{i}"));
        std::os::unix::fs::symlink(target, &link).unwrap();
        assert_eq!(
            captured_library_bytes(&link, std::process::id())
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
    }
}

const CLOSED: &str = "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n";

// Exercise the framing loader and the token assembly that completion consumes.
// The echo tokenizer makes omissions, ordering and parse_special observable.
fn assert_native_profile_framing(disable_thinking: bool) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut bodies = Vec::new();
        for _ in 0..5 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let (offset, length) = loop {
                let mut b = [0; 4096];
                let n = stream.read(&mut b).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&b[..n]);
                if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]);
                    assert!(header.starts_with("POST /tokenize "));
                    let length = header
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|s| s.parse::<usize>().unwrap())
                        })
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while bytes.len() < offset + length {
                let mut b = [0; 4096];
                let n = stream.read(&mut b).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&b[..n]);
            }
            let body: Value = serde_json::from_slice(&bytes[offset..offset + length]).unwrap();
            assert_eq!(body["add_special"], false);
            let tokens: Vec<_> = body["content"]
                .as_str()
                .unwrap()
                .bytes()
                .map(u32::from)
                .collect();
            let response = json!({"tokens":tokens}).to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
            bodies.push(body);
        }
        bodies
    });
    let mut runtime = LlamaCppRuntime::for_test(url, "fixture".into());
    runtime.prompt_framing = PromptFraming::load(
        &runtime.client,
        &runtime.base_url,
        &runtime.api_token,
        disable_thinking,
    )
    .unwrap();
    let system = "system <|im_end|>";
    let user = "user <think>do not interpret me</think>";
    let tokens = runtime
        .prompt_tokens(system, user, &UNCONTROLLED_EXECUTION)
        .unwrap();
    let bodies = server.join().unwrap();
    assert_eq!(
        bodies
            .iter()
            .map(|b| b["parse_special"].as_bool().unwrap())
            .collect::<Vec<_>>(),
        [true, true, true, false, false]
    );
    let close = if disable_thinking {
        CLOSED
    } else {
        "<|im_end|>\n<|im_start|>assistant\n"
    };
    let expected =
        format!("<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\n{user}{close}");
    assert_eq!(
        tokens,
        expected.bytes().map(u32::from).collect::<Vec<_>>(),
        "native framing does not match the profile policy"
    );
}

#[test]
fn native_prompt_closes_thinking_without_interpreting_content_as_control() {
    assert_native_profile_framing(true);
}

#[test]
fn native_prompt_unflagged_keeps_qualified_open_framing() {
    assert_native_profile_framing(false);
}

fn private_capture_file(path: &Path) -> File {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path).unwrap()
}
fn save(root: &Path, name: &str, value: &Value) {
    private_capture_file(&root.join(name))
        .write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
}

#[test]
#[ignore = "zero generation; requires frozen C9 packet, qualified GGUF and exclusive inference lock"]
fn original_c9_native_framing_parity() {
    struct Shutdown;
    impl Drop for Shutdown {
        fn drop(&mut self) {
            shutdown_managed_runtimes();
        }
    }
    let input = PathBuf::from(std::env::var("DOC_SUM_C9_ORIGINAL").unwrap());
    let output = PathBuf::from(std::env::var("DOC_SUM_C9_PARITY_OUTPUT").unwrap());
    let read = |name: &str| -> Value {
        serde_json::from_slice(&fs::read(input.join(name)).unwrap()).unwrap()
    };
    let frozen = read("frozen/requests.json");
    let order = read("9b/execution-order.json");
    assert_eq!(order.as_array().unwrap().len(), 30);
    let _shutdown = Shutdown;
    let (_scratch, runtime) = pinned_9b_runtime();
    let call = |path: &str, body: Value| -> Value {
        runtime
            .authorize(runtime.client.post(runtime.endpoint(path)))
            .json(&body)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap()
    };
    let detokenize = |tokens: &[u32]| -> String {
        call("/detokenize", json!({"tokens":tokens}))["content"]
            .as_str()
            .unwrap()
            .into()
    };
    let framing = &runtime.prompt_framing;
    let open = detokenize(&framing.system_open);
    let middle = detokenize(&framing.system_close_user_open);
    let close = detokenize(&framing.user_close_assistant_open);
    let hash = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    let mut rows = Vec::new();
    for (i, alias) in order.as_array().unwrap().iter().enumerate() {
        let request = frozen
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["case_ids"][0] == *alias)
            .unwrap();
        let system = request["system_prompt"].as_str().unwrap();
        let user = request["user_prompt"].as_str().unwrap();
        let original_render = fs::read_to_string(input.join(format!("9b/prompt-{i}.txt"))).unwrap();
        let template_render = call("/apply-template", json!({"messages":[{"role":"system","content":system},{"role":"user","content":user}],"add_generation_prompt":true,"chat_template_kwargs":{"enable_thinking":false}}))["prompt"].as_str().unwrap().to_string();
        assert_eq!(
            template_render, original_render,
            "historical template changed"
        );
        let mut original_tokens = Vec::new();
        for (content, special) in [
            ("<|im_start|>system\n", true),
            (system, false),
            ("<|im_end|>\n<|im_start|>user\n", true),
            (user, false),
            (CLOSED, true),
        ] {
            original_tokens.extend(
                tokenize_text(
                    &runtime.client,
                    &runtime.base_url,
                    &runtime.api_token,
                    content,
                    special,
                    HEALTH_TIMEOUT,
                )
                .unwrap(),
            );
        }
        assert_eq!(
            json!(original_tokens),
            read(&format!("9b/request-{i}.json"))["prompt"],
            "historical tokens changed"
        );
        let current_tokens = runtime
            .prompt_tokens(system, user, &UNCONTROLLED_EXECUTION)
            .unwrap();
        let current_render = format!("{open}{system}{middle}{user}{close}");
        let row = json!({"case":alias,"original_render":original_render,"current_render":current_render,
            "original_render_sha256":hash(original_render.as_bytes()),"current_render_sha256":hash(current_render.as_bytes()),
            "original_tokens":original_tokens,"current_tokens":current_tokens,
            "original_tokens_sha256":hash(&serde_json::to_vec(&original_tokens).unwrap()),"current_tokens_sha256":hash(&serde_json::to_vec(&current_tokens).unwrap()),
            "equal":original_tokens==current_tokens && original_render==current_render});
        save(&output, &format!("framing-{i}.json"), &row);
        rows.push(json!({"case":alias,"equal":row["equal"],"original_tokens_sha256":row["original_tokens_sha256"],"current_tokens_sha256":row["current_tokens_sha256"]}));
    }
    let passed = rows.iter().all(|r| r["equal"] == true);
    let report = json!({"generation_calls":0,"cases":rows,"gate_passed":passed});
    save(&output, "native-framing.json", &report);
    println!(
        "C9_NATIVE_FRAMING cases={} parity={} generation_calls=0",
        rows.len(),
        passed
    );
    assert!(
        passed,
        "native framing omits validated closed-thinking boundary"
    );
}

// Capture is enabled only by the opted-in parity test, on its calling thread.
thread_local! {
    static COMPLETION_CAPTURE: std::cell::RefCell<Option<(PathBuf, bool)>> = const { std::cell::RefCell::new(None) };
}
#[cfg(target_os = "linux")]
struct CompletionCapture;
#[cfg(target_os = "linux")]
impl Drop for CompletionCapture {
    fn drop(&mut self) {
        COMPLETION_CAPTURE.with(|c| *c.borrow_mut() = None);
    }
}
pub(super) fn capture_completion_request(payload: &impl Serialize) {
    COMPLETION_CAPTURE.with(|c| {
        if let Some((root, pending)) = c.borrow_mut().as_mut() {
            save(
                root,
                "wire-request.json",
                &serde_json::to_value(payload).unwrap(),
            );
            *pending = true;
        }
    });
}
pub(super) fn capture_completion_response(bytes: &[u8]) {
    COMPLETION_CAPTURE.with(|c| {
        if let Some((root, pending)) = c.borrow_mut().as_mut() {
            if *pending {
                let mut f = private_capture_file(&root.join("response.raw.json"));
                f.write_all(bytes).unwrap();
                *pending = false;
            }
        }
    });
}
const SUMMARY_MANIFEST: &str =
    include_str!("summary/comparisons/fixtures/native-summary-parity-manifest.json");
const SUMMARY_MANIFEST_SHA: &str =
    "5ed3e4c777eb6dd6da0f2c8cf03d3b084f11969f04e7f65ae99af11ebe8fd4cc";
fn summary_manifest() -> Value {
    assert_eq!(
        format!("{:x}", Sha256::digest(SUMMARY_MANIFEST.as_bytes())),
        SUMMARY_MANIFEST_SHA
    );
    serde_json::from_str(SUMMARY_MANIFEST).unwrap()
}
fn summary_packet(root: &Path, manifest: &Value) -> Result<Vec<ModelRequest>, String> {
    let check = |ok: bool, message: &str| if ok { Ok(()) } else { Err(message.to_string()) };
    check(
        manifest["task"] == "native-summary-parity" && manifest["call_ceiling"] == 2,
        "task/budget changed",
    )?;
    check(
        manifest["context"] == 32768
            && manifest["seed"] == 4294967295_u64
            && manifest["max_output_tokens"] == 1500,
        "effective settings changed",
    )?;
    check(
        manifest["model_sha256"]
            == "cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13"
            && manifest["server_sha256"]
                == crate::pipeline::model_settings::QUALIFIED_LLAMA_SERVER_DIGEST,
        "runtime pin changed",
    )?;
    let hash = |b: &[u8]| format!("{:x}", Sha256::digest(b));
    let execution = fs::read(root.join("execution.json")).map_err(|e| e.to_string())?;
    check(
        hash(&execution) == manifest["execution_sha256"],
        "execution freeze changed",
    )?;
    let cases = manifest["cases"].as_array().ok_or("missing cases")?;
    check(cases.len() == 2, "missing/extra cases")?;
    let mut requests = Vec::new();
    for (i, case) in cases.iter().enumerate() {
        check(
            case["case"] == i + 1 && case["alias"] == format!("synthesis-{}", i + 1),
            "duplicate/reordered case",
        )?;
        let files = case["files"].as_object().ok_or("missing files")?;
        check(files.len() == 6, "missing file freeze")?;
        for kind in [
            "request",
            "wire-request",
            "response.raw",
            "template",
            "tokens",
            "props",
        ] {
            let name = format!("case-{}-{kind}.json", i + 1);
            let bytes = fs::read(root.join(&name)).map_err(|e| e.to_string())?;
            check(
                hash(&bytes) == *files.get(&name).ok_or("missing file hash")?,
                "artifact hash changed",
            )?;
        }
        let read = |kind: &str| -> Result<Value, String> {
            serde_json::from_slice(
                &fs::read(root.join(format!("case-{}-{kind}.json", i + 1)))
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())
        };
        let request = read("request")?;
        check(
            request["temperature"] == 0.0
                && request["max_tokens"] == 1500
                && request.get("seed").is_none()
                && request["stream"] == false,
            "baseline settings changed",
        )?;
        let messages = request["messages"].as_array().ok_or("missing messages")?;
        check(
            messages.len() == 2 && messages[0]["role"] == "system" && messages[1]["role"] == "user",
            "message projection changed",
        )?;
        let response = read("response.raw")?;
        let text = response["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("missing output target")?;
        check(
            hash(text.as_bytes()) == case["output_sha256"]
                && response["choices"][0]["finish_reason"] == "stop",
            "output target changed",
        )?;
        requests.push(ModelRequest {
            stage: PipelineStage::Synthesize,
            ordinal: i as u32,
            system_prompt: messages[0]["content"]
                .as_str()
                .ok_or("missing system")?
                .into(),
            user_prompt: messages[1]["content"]
                .as_str()
                .ok_or("missing user")?
                .into(),
            seed: 4294967295,
            max_output_tokens: 1500,
            output_format: ModelOutputFormat::Text,
        });
    }
    Ok(requests)
}
#[test]
fn native_summary_registry_rejects_budget_settings_and_case_drift() {
    let root = tempfile::tempdir().unwrap();
    let mut m = summary_manifest();
    let hash = |b: &[u8]| format!("{:x}", Sha256::digest(b));
    fs::write(root.path().join("execution.json"), b"{}").unwrap();
    m["execution_sha256"] = json!(hash(b"{}"));
    for i in 0..2 {
        let request = json!({"temperature":0.0,"max_tokens":1500,"stream":false,"messages":[{"role":"system","content":"system"},{"role":"user","content":"user"}]});
        let response = json!({"choices":[{"finish_reason":"stop","message":{"content":"target"}}]});
        m["cases"][i]["output_sha256"] = json!(hash(b"target"));
        for kind in [
            "request",
            "wire-request",
            "response.raw",
            "template",
            "tokens",
            "props",
        ] {
            let data = if kind == "request" {
                request.clone()
            } else if kind == "response.raw" {
                response.clone()
            } else {
                json!({})
            };
            let bytes = serde_json::to_vec(&data).unwrap();
            let name = format!("case-{}-{kind}.json", i + 1);
            fs::write(root.path().join(&name), &bytes).unwrap();
            m["cases"][i]["files"][&name] = json!(hash(&bytes));
        }
    }
    assert_eq!(summary_packet(root.path(), &m).unwrap().len(), 2);
    for (key, bad) in [
        ("call_ceiling", json!(0)),
        ("call_ceiling", json!(3)),
        ("context", json!(8192)),
        ("seed", json!(7)),
        ("max_output_tokens", json!(2048)),
        ("model_sha256", json!("wrong")),
        ("server_sha256", json!("wrong")),
    ] {
        let mut altered = m.clone();
        altered[key] = bad;
        assert!(
            summary_packet(root.path(), &altered).is_err(),
            "accepted changed {key}"
        );
    }
    for cases in [
        json!([]),
        json!([m["cases"][0].clone()]),
        json!([m["cases"][0].clone(), m["cases"][0].clone()]),
        json!([
            m["cases"][0].clone(),
            m["cases"][1].clone(),
            m["cases"][1].clone()
        ]),
    ] {
        let mut altered = m.clone();
        altered["cases"] = cases;
        assert!(summary_packet(root.path(), &altered).is_err());
    }
    for key in ["output_sha256", "files"] {
        let mut altered = m.clone();
        altered["cases"][0][key] = json!("wrong");
        assert!(summary_packet(root.path(), &altered).is_err());
    }
    for kind in [
        "request",
        "wire-request",
        "response.raw",
        "template",
        "tokens",
        "props",
    ] {
        let name = format!("case-1-{kind}.json");
        let bytes = fs::read(root.path().join(&name)).unwrap();
        fs::write(root.path().join(&name), b"changed").unwrap();
        assert!(summary_packet(root.path(), &m).is_err());
        fs::write(root.path().join(&name), bytes).unwrap();
    }
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "accepted two-call summary parity; frozen packet, exclusive inference lock and pinned GGUF required"]
fn original_summary_native_adapter_parity() {
    struct Shutdown;
    impl Drop for Shutdown {
        fn drop(&mut self) {
            shutdown_managed_runtimes();
        }
    }
    let _shutdown = Shutdown;
    let root = PathBuf::from(std::env::var("DOC_SUM_SUMMARY_ORIGINAL").unwrap());
    let out = PathBuf::from(std::env::var("DOC_SUM_SUMMARY_PARITY_OUTPUT").unwrap());
    let m = summary_manifest();
    let requests = summary_packet(&root, &m).unwrap();
    let (_scratch, runtime) = pinned_9b_runtime();
    let call = |path: &str, body: Option<Value>| -> Value {
        let request = if let Some(body) = body {
            runtime.client.post(runtime.endpoint(path)).json(&body)
        } else {
            runtime.client.get(runtime.endpoint(path))
        };
        runtime
            .authorize(request)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap()
    };
    let props = call("/props", None);
    save(&out, "props.json", &props);
    let read = |case: usize, kind: &str| -> Value {
        serde_json::from_slice(&fs::read(root.join(format!("case-{case}-{kind}.json"))).unwrap())
            .unwrap()
    };
    let mut mismatches = Vec::new();
    for key in [
        "seed",
        "dynatemp_range",
        "dynatemp_exponent",
        "top_k",
        "top_p",
        "min_p",
        "top_n_sigma",
        "xtc_probability",
        "xtc_threshold",
        "typical_p",
        "repeat_last_n",
        "repeat_penalty",
        "presence_penalty",
        "frequency_penalty",
        "dry_multiplier",
        "dry_base",
        "dry_allowed_length",
        "dry_penalty_last_n",
        "mirostat",
        "mirostat_tau",
        "mirostat_eta",
        "adaptive_target",
        "adaptive_decay",
        "ignore_eos",
        "n_keep",
        "n_discard",
        "n_probs",
        "min_keep",
        "samplers",
        "speculative.types",
        "backend_sampling",
        "lora",
    ] {
        let old = read(1, "props")["default_generation_settings"]["params"][key].clone();
        let current = props["default_generation_settings"]["params"][key].clone();
        if old.is_null() || current.is_null() || old != current {
            mismatches.push(json!({"key":key,"baseline":old,"current":current}));
        }
    }
    let mut framed = Vec::new();
    let detokenize = |tokens: &[u32]| {
        call("/detokenize", Some(json!({"tokens":tokens})))["content"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let open = detokenize(&runtime.prompt_framing.system_open);
    let middle = detokenize(&runtime.prompt_framing.system_close_user_open);
    let close = detokenize(&runtime.prompt_framing.user_close_assistant_open);
    for (i, request) in requests.iter().enumerate() {
        let tokens = runtime
            .prompt_tokens(
                &request.system_prompt,
                &request.user_prompt,
                &UNCONTROLLED_EXECUTION,
            )
            .unwrap();
        let text = format!(
            "{open}{}{middle}{}{close}",
            request.system_prompt, request.user_prompt
        );
        let template = call(
            "/apply-template",
            Some(
                json!({"messages":[{"role":"system","content":request.system_prompt},{"role":"user","content":request.user_prompt}],"add_generation_prompt":true,"chat_template_kwargs":{"enable_thinking":false}}),
            ),
        );
        let same = text == read(i + 1, "template")["prompt"]
            && template == read(i + 1, "template")
            && json!(tokens) == read(i + 1, "tokens")["tokens"];
        let admitted = runtime.preflight_request(request).is_ok();
        save(
            &out,
            &format!("framing-{}.json", i + 1),
            &json!({"case":i+1,"render":text,"tokens":tokens,"same":same,"admitted":admitted}),
        );
        framed.push(json!({"case":i+1,"same":same,"admitted":admitted}));
    }
    let ready = mismatches.is_empty()
        && framed
            .iter()
            .all(|f| f["same"] == true && f["admitted"] == true);
    save(
        &out,
        "preflight.json",
        &json!({"ready":ready,"framing":framed,"sampler_mismatches":mismatches,"generation_calls":0}),
    );
    if !ready {
        println!("SUMMARY_NATIVE_PARITY stopped_before_generation=true calls=0");
        return;
    }
    let owner = runtime.owner.as_ref().unwrap();
    let pid = owner.child.lock().unwrap().id();
    let exe = fs::read(format!("/proc/{pid}/exe")).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&exe)), m["server_sha256"]);
    let maps = fs::read_to_string(format!("/proc/{pid}/maps")).unwrap();
    let mut libraries = serde_json::Map::new();
    for library in crate::pipeline::model_settings::JACK_LLAMA_CPP_LIBRARIES {
        let path = owner
            ._runtime_library_directory
            .path()
            .join(library.file_name);
        let bytes = captured_library_bytes(&path, pid).unwrap();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(digest, library.digest);
        libraries.insert(
            library.file_name.into(),
            json!({"sha256":digest,"mapped":maps.contains(library.file_name)}),
        );
    }
    save(&out, "runtime-libraries.json", &Value::Object(libraries));
    save(
        &out,
        "process.json",
        &json!({"pid":pid,"maps":maps,"server_sha256":m["server_sha256"],"model_sha256":m["model_sha256"],"context":32768,"disable_thinking":true}),
    );
    // The fixed reservation lives beside all outputs. Renaming the run cannot reset it.
    save(
        root.parent().unwrap(),
        "native-summary-parity-generation-reservation.json",
        &json!({"maximum_calls":2,"manifest":m,"output":out}),
    );
    let mut rows = Vec::new();
    for (i, request) in requests.iter().enumerate() {
        let case = out.join(format!("case-{}", i + 1));
        fs::create_dir(&case).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&case, fs::Permissions::from_mode(0o700)).unwrap();
        }
        COMPLETION_CAPTURE.with(|c| *c.borrow_mut() = Some((case.clone(), false)));
        let capture = CompletionCapture;
        let result = runtime.generate(request);
        drop(capture);
        let exposed = match &result {
            Ok(r) => {
                json!({"text":r.text,"runtime_id":r.runtime_id,"model_id":r.model_id,"request_attempts":r.request_attempts})
            }
            Err(e) => {
                json!({"code":e.code,"message":e.message,"recoverable":e.recoverable,"request_attempts":e.request_attempts})
            }
        };
        save(&case, "adapter-result.json", &exposed);
        let raw: Value = fs::read(case.join("response.raw.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);
        let target = read(i + 1, "response.raw")["choices"][0]["message"]["content"]
            .as_str()
            .unwrap()
            .to_string();
        let passed = result.as_ref().is_ok_and(|r| r.text == target)
            && raw["content"] == target
            && matches!(raw["stop_type"].as_str(), Some("eos" | "word"))
            && raw["truncated"] == false;
        rows.push(json!({"case":i+1,"passed":passed,"prompt_tokens":raw["tokens_evaluated"],"output_tokens":raw["tokens_predicted"],"stop_type":raw["stop_type"]}));
        save(
            &out,
            &format!("status-{}.json", i + 1),
            &json!({"cases":rows,"calls":rows.len(),"passed":passed}),
        );
        if !passed {
            break;
        }
    }
    let passed = rows.len() == 2 && rows.iter().all(|r| r["passed"] == true);
    save(
        &out,
        "results.json",
        &json!({"cases":rows,"calls":rows.len(),"passed":passed}),
    );
    println!("SUMMARY_NATIVE_PARITY passed={passed} calls={}", rows.len());
}
