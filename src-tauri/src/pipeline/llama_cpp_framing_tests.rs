use crate::pipeline::contracts::ModelOutputFormat;
use crate::pipeline::control::CancellationToken;
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
    save(
        &output,
        "message-delimiters.json",
        &serde_json::to_value(PromptFraming::MESSAGE_DELIMITERS).unwrap(),
    );
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
pub(crate) struct CompletionCapture;
#[cfg(target_os = "linux")]
impl Drop for CompletionCapture {
    fn drop(&mut self) {
        COMPLETION_CAPTURE.with(|c| *c.borrow_mut() = None);
    }
}
// Reuse the framing lane's actual serializer/decoder hooks for stage qualification.
// The scope must stay on the calling thread, just like the existing parity lane.
#[cfg(target_os = "linux")]
pub(crate) fn capture_native_completion(root: &Path) -> CompletionCapture {
    COMPLETION_CAPTURE.with(|c| {
        assert!(c.borrow().is_none(), "completion capture already active");
        *c.borrow_mut() = Some((root.to_path_buf(), false));
    });
    CompletionCapture
}

struct TokenizerObservation {
    root: PathBuf,
    state: Mutex<TokenizerObservationState>,
}
struct TokenizerObservationState {
    closed: bool,
    phase: String,
    attempts: Vec<Value>,
    decoded: usize,
}
struct TokenizerRegistry {
    enabled: AtomicBool,
    scopes: Mutex<HashMap<String, Arc<TokenizerObservation>>>,
}
impl TokenizerRegistry {
    fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            scopes: Mutex::new(HashMap::new()),
        }
    }
    fn send(&self, endpoint: &str, parse_special: bool) {
        // Disabled observation does not touch a mutex or thread-local state.
        if !self.enabled.load(Ordering::Acquire) {
            return;
        }
        let scope = self.scopes.lock().unwrap().get(endpoint).cloned();
        TOKENIZER_PENDING.with(|p| *p.borrow_mut() = None);
        if let Some(scope) = scope {
            let mut state = scope.state.lock().unwrap();
            if state.closed {
                return;
            }
            let ordinal = state.attempts.len();
            let event = json!({"send_ordinal":ordinal,"endpoint":endpoint,"phase":state.phase,"parse_special":parse_special});
            save(
                &scope.root,
                &format!("tokenizer-attempt-{ordinal}.json"),
                &event,
            );
            state.attempts.push(event);
            drop(state);
            TOKENIZER_PENDING.with(|p| *p.borrow_mut() = Some(scope));
        }
    }
}
static TOKENIZER_REGISTRY: OnceLock<TokenizerRegistry> = OnceLock::new();
thread_local! {
    static TOKENIZER_PENDING: std::cell::RefCell<Option<Arc<TokenizerObservation>>> = const { std::cell::RefCell::new(None) };
}
pub(crate) struct NativeTokenizerCapture {
    endpoints: Vec<String>,
    scope: Arc<TokenizerObservation>,
}
impl Drop for NativeTokenizerCapture {
    fn drop(&mut self) {
        let registry = TOKENIZER_REGISTRY.get().unwrap();
        {
            let mut scopes = registry.scopes.lock().unwrap();
            for endpoint in &self.endpoints {
                scopes.remove(endpoint);
            }
            registry
                .enabled
                .store(!scopes.is_empty(), Ordering::Release);
        }
        let mut state = self.scope.state.lock().unwrap();
        state.closed = true;
        save(
            &self.scope.root,
            "tokenizer-observation.json",
            &json!({
                "tokenizer_http_send_attempts":state.attempts.len(),
                "decoded_tokenizer_http_responses":state.decoded,
                "attempts":state.attempts,
                "metric":"attempted sends, including unsuccessful attempts; not proof of remote receipt",
            }),
        );
    }
}
fn capture_tokenizer_endpoints(root: &Path, endpoints: &[String]) -> NativeTokenizerCapture {
    assert!(!endpoints.is_empty());
    let registry = TOKENIZER_REGISTRY.get_or_init(TokenizerRegistry::new);
    let scope = Arc::new(TokenizerObservation {
        root: root.into(),
        state: Mutex::new(TokenizerObservationState {
            closed: false,
            phase: "startup".into(),
            attempts: vec![],
            decoded: 0,
        }),
    });
    let mut scopes = registry.scopes.lock().unwrap();
    for endpoint in endpoints {
        assert!(
            !scopes.contains_key(endpoint),
            "tokenizer endpoint already observed"
        );
    }
    for endpoint in endpoints {
        scopes.insert(endpoint.clone(), scope.clone());
    }
    registry.enabled.store(true, Ordering::Release);
    NativeTokenizerCapture {
        endpoints: endpoints.to_vec(),
        scope,
    }
}
pub(crate) fn capture_native_tokenizer(root: &Path) -> NativeTokenizerCapture {
    // The selected native owner uses one private Unix socket and this HTTP authority.
    // The qualification binary runs one test/selected profile, never another app.
    capture_tokenizer_endpoints(root, &["http://localhost".into()])
}
impl NativeTokenizerCapture {
    pub(crate) fn set_phase(&self, phase: &str) {
        self.scope.state.lock().unwrap().phase = phase.into();
    }
}
pub(super) fn capture_tokenizer_send(endpoint: &str, parse_special: bool) {
    if let Some(registry) = TOKENIZER_REGISTRY.get() {
        registry.send(endpoint, parse_special);
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
    TOKENIZER_PENDING.with(|p| {
        if let Some(scope) = p.borrow_mut().take() {
            let mut state = scope.state.lock().unwrap();
            if !state.closed && serde_json::from_slice::<TokenizeResponse>(bytes).is_ok() {
                let mut f = private_capture_file(
                    &scope
                        .root
                        .join(format!("tokenizer-response-{}.json", state.decoded)),
                );
                f.write_all(bytes).unwrap();
                state.decoded += 1;
            }
        }
    });
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
    // The original reservation remains immutable. A separately accepted repair
    // has its own fixed, source/contract/history-bound reservation.
    let repair_sha = std::env::var("DOC_SUM_SUMMARY_REPAIR_CONTRACT_SHA").ok();
    let repair_reservation = repair_sha.as_ref().map(|sha| {
        let value: Value = serde_json::from_slice(
            &fs::read(
                root.parent()
                    .unwrap()
                    .join("native-message-boundary-repair-generation-reservation.json"),
            )
            .unwrap(),
        )
        .unwrap();
        validate_summary_repair_reservation(&value, sha).unwrap();
        let current = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        assert!(current.status.success());
        assert_eq!(
            value["source"]["head"],
            String::from_utf8(current.stdout).unwrap().trim()
        );
        value
    });
    if repair_sha.is_none() {
        save(
            root.parent().unwrap(),
            "native-summary-parity-generation-reservation.json",
            &json!({"maximum_calls":2,"manifest":m,"output":out}),
        );
    }
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
        if let (Some(sha), Some(reservation)) = (&repair_sha, &repair_reservation) {
            assert_summary_gpu_idle();
            let tokens = runtime
                .prompt_tokens(
                    &request.system_prompt,
                    &request.user_prompt,
                    &UNCONTROLLED_EXECUTION,
                )
                .unwrap();
            let projected =
                serde_json::to_value(runtime.completion_request(request, &tokens, None)).unwrap();
            let expected: Value = serde_json::from_slice(
                &fs::read(
                    root.parent()
                        .unwrap()
                        .join("native-message-boundary-repair-preflight")
                        .join(format!("case-{}-candidate-wire.json", i + 1)),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(
                projected, expected,
                "native request changed before generation"
            );
            reserve_summary_repair_call(root.parent().unwrap(), reservation, sha, i + 1).unwrap();
        }
        let result = runtime.generate(request);
        if repair_sha.is_some() {
            assert_summary_gpu_idle();
        }
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
        let wire: Value =
            serde_json::from_slice(&fs::read(case.join("wire-request.json")).unwrap()).unwrap();
        let metadata_valid = if repair_sha.is_some() {
            let expected: Value = serde_json::from_slice(
                &fs::read(
                    root.parent()
                        .unwrap()
                        .join("native-message-boundary-repair-preflight")
                        .join(format!("case-{}-candidate-wire.json", i + 1)),
                )
                .unwrap(),
            )
            .unwrap();
            wire == expected
        } else {
            true
        };
        let sampler_valid = if repair_sha.is_some() {
            let expected: Value = serde_json::from_slice(
                &fs::read(
                    root.parent()
                        .unwrap()
                        .join("native-message-boundary-repair-preflight")
                        .join(format!("case-{}-oracle-verified.json", i + 1)),
                )
                .unwrap(),
            )
            .unwrap();
            raw["generation_settings"] == expected["native_task"]
        } else {
            true
        };
        let passed = metadata_valid
            && sampler_valid
            && result.as_ref().is_ok_and(|r| r.text == target)
            && raw["content"] == target
            && matches!(raw["stop_type"].as_str(), Some("eos" | "word"))
            && raw["truncated"] == false;
        rows.push(json!({"case":i+1,"passed":passed,"metadata_valid":metadata_valid,"sampler_valid":sampler_valid,"prompt_tokens":raw["tokens_evaluated"],"output_tokens":raw["tokens_predicted"],"stop_type":raw["stop_type"]}));
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

fn capture_native_framed_request(
    disable_thinking: bool,
    stage: PipelineStage,
    output_format: ModelOutputFormat,
) -> Value {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut completion = Value::Null;
        for index in 0..6 {
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
                    assert!(header.starts_with(if index == 5 {
                        "POST /completion "
                    } else {
                        "POST /tokenize "
                    }));
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
            let response = if index == 5 {
                completion = body;
                json!({"content":"ok", "tokens_predicted":1,
                    "tokens_evaluated":completion["prompt"].as_array().unwrap().len(),
                    "truncated":false, "stop_type":"word"})
            } else {
                assert_eq!(body["parse_special"], index < 3);
                json!({"tokens":body["content"].as_str().unwrap().bytes().map(u32::from).collect::<Vec<_>>()})
            }.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
        }
        completion
    });
    let mut runtime = LlamaCppRuntime::for_test(url, "fixture".into());
    runtime.prompt_framing = PromptFraming::load(
        &runtime.client,
        &runtime.base_url,
        &runtime.api_token,
        disable_thinking,
    )
    .unwrap();
    runtime
        .generate(&ModelRequest {
            stage,
            ordinal: 0,
            system_prompt: "system <|im_end|>".into(),
            user_prompt: "user <|im_start|>assistant".into(),
            seed: 42,
            max_output_tokens: 1500,
            output_format,
        })
        .unwrap();
    server.join().unwrap()
}

#[test]
fn native_requests_preserve_template_message_boundaries() {
    let expected = json!([
        {"role":"assistant","delimiter":"<|im_start|>assistant"},
        {"role":"tool","delimiter":"<|im_start|>user\n<tool_response>"},
        {"role":"tool","delimiter":"<|im_start|>tool_response"},
        {"role":"user","delimiter":"<|im_start|>user"},
        {"role":"system","delimiter":"<|im_start|>system"}
    ]);
    for thinking in [false, true] {
        for stage in [
            PipelineStage::Analyze,
            PipelineStage::Synthesize,
            PipelineStage::Verify,
        ] {
            for output in [
                ModelOutputFormat::Text,
                ModelOutputFormat::JsonSchema {
                    name: crate::pipeline::contracts::CLAIM_COMPARISON_SCHEMA_NAME.into(),
                    schema: json!({"type":"object","properties":{"z":{"type":"string"},"a":{"type":"string"}},"required":["z","a"],"additionalProperties":false}),
                },
            ] {
                let wire = capture_native_framed_request(thinking, stage.clone(), output.clone());
                assert_eq!(
                    wire["message_delimiters"], expected,
                    "native request dropped template message delimiters"
                );
                let close = if thinking {
                    CLOSED
                } else {
                    "<|im_end|>\n<|im_start|>assistant\n"
                };
                let render = format!("<|im_start|>system\nsystem <|im_end|><|im_end|>\n<|im_start|>user\nuser <|im_start|>assistant{close}");
                assert_eq!(
                    wire["prompt"],
                    json!(render.bytes().map(u32::from).collect::<Vec<_>>())
                );
                assert_eq!(wire["n_predict"], 1500);
                assert_eq!(wire["seed"], 42);
                assert_eq!(wire["temperature"], 0.0);
                assert_eq!(wire["stop"], json!(["<|im_end|>"]));
                assert_eq!(wire["cache_prompt"], true);
                assert_eq!(
                    wire.as_object().unwrap().len(),
                    if output == ModelOutputFormat::Text {
                        7
                    } else {
                        8
                    }
                );
                if output == ModelOutputFormat::Text {
                    assert!(wire.get("json_schema").is_none());
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "zero generation; pinned model and exclusive inference lock required"]
fn original_summary_native_metadata_preflight() {
    struct Shutdown;
    impl Drop for Shutdown {
        fn drop(&mut self) {
            shutdown_managed_runtimes();
        }
    }
    let _shutdown = Shutdown;
    let root = PathBuf::from(std::env::var("DOC_SUM_SUMMARY_ORIGINAL").unwrap());
    let out = PathBuf::from(std::env::var("DOC_SUM_SUMMARY_PARITY_OUTPUT").unwrap());
    let requests = summary_packet(&root, &summary_manifest()).unwrap();
    let (_scratch, runtime) = pinned_9b_runtime();
    for (i, request) in requests.iter().enumerate() {
        let tokens = runtime
            .prompt_tokens(
                &request.system_prompt,
                &request.user_prompt,
                &UNCONTROLLED_EXECUTION,
            )
            .unwrap();
        let frozen: Value = serde_json::from_slice(
            &fs::read(root.join(format!("case-{}-tokens.json", i + 1))).unwrap(),
        )
        .unwrap();
        assert_eq!(json!(tokens), frozen["tokens"]);
        let wire = runtime.completion_request(request, &tokens, None);
        save(
            &out,
            &format!("case-{}-candidate-wire.json", i + 1),
            &serde_json::to_value(wire).unwrap(),
        );
    }
    save(
        &out,
        "result.json",
        &json!({"cases":requests.len(),"generation_calls":0,"tokens_equal":true}),
    );
    println!(
        "SUMMARY_METADATA_PREFLIGHT cases={} generation_calls=0 tokens_equal=true",
        requests.len()
    );
}

const REPAIR_CONTRACT: &str = include_str!("../../../docs/PR-NATIVE-MESSAGE-BOUNDARY-REPAIR.md");

fn validate_summary_repair_reservation(
    reservation: &Value,
    contract_sha: &str,
) -> Result<(), &'static str> {
    if contract_sha != format!("{:x}", Sha256::digest(REPAIR_CONTRACT.as_bytes()))
        || reservation["accepted_contract_sha256"] != contract_sha
        || reservation["maximum_calls"].as_u64() != Some(2)
        || reservation["source"]["head"]
            .as_str()
            .is_none_or(|s| s.len() != 40)
        || reservation["approval_sha256"]
            .as_str()
            .is_none_or(|s| s.len() != 64)
        || reservation["prior_reservation_sha256"]
            .as_str()
            .is_none_or(|s| s.len() != 64)
    {
        return Err("invalid repair reservation");
    }
    Ok(())
}

fn reserve_summary_repair_call(
    root: &Path,
    reservation: &Value,
    contract_sha: &str,
    index: usize,
) -> Result<(), String> {
    validate_summary_repair_reservation(reservation, contract_sha).map_err(str::to_string)?;
    if !(1..=2).contains(&index) {
        return Err("repair call ceiling exceeded".into());
    }
    if index == 2
        && !root
            .join("native-message-boundary-repair-call-1.json")
            .exists()
    {
        return Err("out-of-order repair call".into());
    }
    let path = root.join(format!("native-message-boundary-repair-call-{index}.json"));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(serde_json::to_string(&json!({"index":index,"status":"reserved before generation","contract_sha256":contract_sha})).unwrap().as_bytes()).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

#[test]
fn summary_repair_reservation_rejects_missing_stale_and_extra_calls() {
    let sha = format!("{:x}", Sha256::digest(REPAIR_CONTRACT.as_bytes()));
    let good = json!({"accepted_contract_sha256":sha,"maximum_calls":2,"source":{"head":"a".repeat(40)},"approval_sha256":"a".repeat(64),"prior_reservation_sha256":"b".repeat(64)});
    assert!(validate_summary_repair_reservation(&good, &sha).is_ok());
    assert!(validate_summary_repair_reservation(&good, "stale").is_err());
    for bad in [
        json!(0),
        json!(false),
        json!(""),
        json!(2.0),
        json!(3),
        Value::Null,
    ] {
        let mut altered = good.clone();
        altered["maximum_calls"] = bad;
        assert!(validate_summary_repair_reservation(&altered, &sha).is_err());
    }
    for key in [
        "accepted_contract_sha256",
        "source",
        "approval_sha256",
        "prior_reservation_sha256",
    ] {
        let mut altered = good.clone();
        altered.as_object_mut().unwrap().remove(key);
        assert!(validate_summary_repair_reservation(&altered, &sha).is_err());
    }
    let root = tempfile::tempdir().unwrap();
    assert!(reserve_summary_repair_call(root.path(), &good, &sha, 0).is_err());
    assert!(reserve_summary_repair_call(root.path(), &good, &sha, 2).is_err());
    reserve_summary_repair_call(root.path(), &good, &sha, 1).unwrap();
    assert!(reserve_summary_repair_call(root.path(), &good, &sha, 1).is_err());
    reserve_summary_repair_call(root.path(), &good, &sha, 2).unwrap();
    assert!(reserve_summary_repair_call(root.path(), &good, &sha, 3).is_err());
}

#[cfg(target_os = "linux")]
fn assert_summary_gpu_idle() {
    let client = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let queue: Value = client
        .get("http://127.0.0.1:8189/queue")
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert!(
        queue["queue_running"].as_array().unwrap().is_empty()
            && queue["queue_pending"].as_array().unwrap().is_empty(),
        "ComfyUI is busy"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn qualification_capture_reuses_actual_wire_and_scopes_tokenizer_observations() {
    let output = tempfile::tempdir().unwrap();
    {
        let _tokens = capture_tokenizer_endpoints(output.path(), &["wire-unit-fixture".into()]);
        capture_tokenizer_send("wire-unit-fixture", false);
        capture_completion_response(br#"{"tokens":[1,2]}"#);
        capture_completion_response(br#"{"data":[]}"#);
        let _completion = capture_native_completion(output.path());
        capture_tokenizer_send("wire-unit-fixture", false);
        capture_completion_response(br#"{"tokens":[3]}"#);
        assert!(!output.path().join("response.raw.json").exists());
        capture_completion_request(&json!({"prompt":[1,2],"n_predict":10}));
        capture_completion_response(br#"{"content":"ok","stop_type":"word"}"#);
    }
    capture_completion_response(br#"{"tokens":[4]}"#);
    let observation: Value = serde_json::from_slice(
        &fs::read(output.path().join("tokenizer-observation.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(observation["decoded_tokenizer_http_responses"], 2);
    assert!(!output.path().join("tokenizer-response-2.json").exists());
    assert_eq!(
        fs::read(output.path().join("response.raw.json")).unwrap(),
        br#"{"content":"ok","stop_type":"word"}"#
    );
    let wire: Value =
        serde_json::from_slice(&fs::read(output.path().join("wire-request.json")).unwrap())
            .unwrap();
    assert_eq!(wire["prompt"], json!([1, 2]));
}

fn tokenizer_observation(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(root.join("tokenizer-observation.json")).unwrap()).unwrap()
}

fn tokenizer_http_fixture(
    body: Vec<u8>,
    status: &str,
    calls: usize,
) -> (String, thread::JoinHandle<usize>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let status = status.to_string();
    let server = thread::spawn(move || {
        for _ in 0..calls {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = [0; 4096];
            let count = stream.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..count]).starts_with("POST /tokenize "));
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            // The decoder may stop at its hard bound before the server finishes.
            let _ = stream.write_all(&body);
        }
        calls
    });
    (endpoint, server)
}

#[test]
fn qualification_tokenizer_capture_counts_rejected_http_attempt() {
    let output = tempfile::tempdir().unwrap();
    let (endpoint, server) = tokenizer_http_fixture(vec![], "503 Service Unavailable", 1);
    {
        let _capture = capture_tokenizer_endpoints(output.path(), std::slice::from_ref(&endpoint));
        let runtime = LlamaCppRuntime::for_test(endpoint, "public-fixture-token".into());
        assert_eq!(
            runtime
                .prompt_tokens("system", "user", &UNCONTROLLED_EXECUTION)
                .unwrap_err()
                .code,
            "MODEL_RUNTIME_REJECTED"
        );
    }
    let actual = server.join().unwrap();
    let observed = tokenizer_observation(output.path());
    println!("TOKENIZER_ATTEMPT_PROBE actual_http_attempts={actual} observed_send_attempts={} observed_decoded_responses={}", observed["tokenizer_http_send_attempts"], observed["decoded_tokenizer_http_responses"]);
    assert_eq!(observed["tokenizer_http_send_attempts"], json!(actual));
    assert_eq!(observed["decoded_tokenizer_http_responses"], 0);
}

#[test]
fn qualification_tokenizer_capture_success_and_malformed_oversized_boundaries() {
    for (body, calls, expected_error) in [
        (br#"{"tokens":[1]}"#.to_vec(), 2, None),
        (b"malformed".to_vec(), 1, Some("MODEL_RESPONSE_INVALID")),
        (
            br#"{"tokens":"wrong-type"}"#.to_vec(),
            1,
            Some("MODEL_RESPONSE_INVALID"),
        ),
        (
            vec![b'x'; MAX_TOKENIZE_RESPONSE_BYTES as usize + 1],
            1,
            Some("MODEL_RESPONSE_TOO_LARGE"),
        ),
    ] {
        let output = tempfile::tempdir().unwrap();
        let (endpoint, server) = tokenizer_http_fixture(body, "200 OK", calls);
        {
            let _capture =
                capture_tokenizer_endpoints(output.path(), std::slice::from_ref(&endpoint));
            let runtime = LlamaCppRuntime::for_test(endpoint, "public-fixture-token".into());
            let result = runtime.prompt_tokens("system", "user", &UNCONTROLLED_EXECUTION);
            if let Some(code) = expected_error {
                assert_eq!(result.unwrap_err().code, code);
            } else {
                assert!(result.is_ok());
            }
        }
        assert_eq!(server.join().unwrap(), calls);
        let observed = tokenizer_observation(output.path());
        assert_eq!(observed["tokenizer_http_send_attempts"], calls);
        assert_eq!(
            observed["decoded_tokenizer_http_responses"],
            if expected_error.is_some() { 0 } else { calls }
        );
    }
}

#[test]
fn qualification_tokenizer_capture_transport_failure_and_cancellation() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut bytes = [0; 4096];
        let count = stream.read(&mut bytes).unwrap();
        assert!(count > 0);
    });
    let output = tempfile::tempdir().unwrap();
    {
        let _capture = capture_tokenizer_endpoints(output.path(), std::slice::from_ref(&endpoint));
        let runtime = LlamaCppRuntime::for_test(endpoint, "public-fixture-token".into());
        assert_eq!(
            runtime
                .prompt_tokens("system", "user", &UNCONTROLLED_EXECUTION)
                .unwrap_err()
                .code,
            "MODEL_RUNTIME_UNAVAILABLE"
        );
    }
    server.join().unwrap();
    assert_eq!(
        tokenizer_observation(output.path())["tokenizer_http_send_attempts"],
        1
    );
    assert_eq!(
        tokenizer_observation(output.path())["decoded_tokenizer_http_responses"],
        0
    );
    let output = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    {
        let _capture = capture_tokenizer_endpoints(output.path(), std::slice::from_ref(&endpoint));
        let control = CancellationToken::new();
        control.request();
        let runtime = LlamaCppRuntime::for_test(endpoint, "public-fixture-token".into());
        assert_eq!(
            runtime
                .prompt_tokens("system", "user", &control)
                .unwrap_err()
                .code,
            "MODEL_REQUEST_CANCELLED"
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
    assert_eq!(
        tokenizer_observation(output.path())["tokenizer_http_send_attempts"],
        0
    );
}

#[test]
fn qualification_tokenizer_capture_disabled_does_not_acquire_lock() {
    let registry = Arc::new(TokenizerRegistry::new());
    let lock = registry.scopes.lock().unwrap();
    let cloned = registry.clone();
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        cloned.send("unobserved", false);
        tx.send(()).unwrap();
    });
    // Holding exactly the mutex send would take proves the disabled early return.
    assert!(rx.recv_timeout(Duration::from_secs(1)).is_ok());
    drop(lock);
    worker.join().unwrap();
    assert!(registry.scopes.lock().unwrap().is_empty());
}

#[test]
fn qualification_tokenizer_capture_cross_thread_and_endpoint_isolation() {
    let output = tempfile::tempdir().unwrap();
    let native = capture_native_tokenizer(output.path());
    native.set_phase("worker-fixture");
    let workers: Vec<_> = (0..8)
        .map(|_| {
            thread::spawn(|| {
                capture_tokenizer_send("http://localhost", true);
                capture_completion_response(br#"{"tokens":[1]}"#);
                capture_tokenizer_send("unrelated-endpoint", false);
                capture_completion_response(br#"{"tokens":[2]}"#);
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    drop(native);
    let observed = tokenizer_observation(output.path());
    assert_eq!(observed["tokenizer_http_send_attempts"], 8);
    assert_eq!(observed["decoded_tokenizer_http_responses"], 8);
    assert!(observed["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["endpoint"] == "http://localhost" && v["phase"] == "worker-fixture"));
    capture_tokenizer_send("http://localhost", false);
    assert_eq!(tokenizer_observation(output.path()), observed);
}

// Test-only transport for the real native framing and preflight owners. Content
// tokens are counts, never a completion-ready token stream. Generation is denied.
pub(crate) struct OfflineNativeCountRuntime {
    runtime: LlamaCppRuntime,
    allowed: Arc<Mutex<std::collections::HashSet<String>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
    validated_records: usize,
    preflights: std::sync::atomic::AtomicUsize,
}
impl Drop for OfflineNativeCountRuntime {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(server) = self.server.take() {
            server.join().unwrap();
        }
    }
}
impl OfflineNativeCountRuntime {
    pub(crate) fn from_frozen_inventory(path: &Path, expected_sha256: &str) -> Self {
        use crate::pipeline::qwen_tokenizer::QwenPromptTokenizer;
        use std::io::{Read, Write};
        let bytes = fs::read(path).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            expected_sha256,
            "count inventory changed"
        );
        let packet: Value = serde_json::from_slice(&bytes).unwrap();
        for receipt in packet["receipts"].as_array().unwrap() {
            let bytes = fs::read(receipt["path"].as_str().unwrap()).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(&bytes)),
                receipt["sha256"].as_str().unwrap(),
                "native token receipt changed"
            );
        }
        let (context, family, disable_thinking) =
            crate::pipeline::model_settings::offline_native_count_profile("qwen35-9b-q4km-v1")
                .unwrap();
        let metadata = fs::read(packet["metadata_path"].as_str().unwrap()).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&metadata)),
            packet["metadata_sha256"].as_str().unwrap(),
            "tokenizer metadata changed"
        );
        let metadata: Value = serde_json::from_slice(&metadata).unwrap();
        let tokenizer =
            QwenPromptTokenizer::from_model_info(family, metadata.as_object().unwrap()).unwrap();
        let fixed = packet["fixed_framing"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["content"].as_str().unwrap().to_owned(),
                    f["tokens"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|n| u32::try_from(n.as_u64().unwrap()).unwrap())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<HashMap<_, _>>();
        assert_eq!(
            fixed.len(),
            3,
            "all fixed native frame receipts are required"
        );
        let records = packet["records"].as_array().unwrap();
        assert!(
            !records.is_empty(),
            "recorded count inventory must not be empty"
        );
        let allowed = Arc::new(Mutex::new(
            records
                .iter()
                .flat_map(|r| {
                    [
                        r["system"].as_str().unwrap().to_owned(),
                        r["user"].as_str().unwrap().to_owned(),
                    ]
                })
                .collect::<std::collections::HashSet<_>>(),
        ));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server_stop = stop.clone();
        let server_allowed = allowed.clone();
        let server = thread::spawn(move || {
            // This cache belongs only to the immutable offline test tokenizer.
            // Repeated budget probes change output allowance, not source bytes.
            let mut counted = HashMap::<String, u32>::new();
            while !server_stop.load(std::sync::atomic::Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(s) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("offline tokenizer fixture accept failed: {e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let (offset, length) = loop {
                    let mut buffer = [0; 8192];
                    let n = stream.read(&mut buffer).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        assert!(
                            headers.starts_with("POST /tokenize "),
                            "only token counting is authorized"
                        );
                        let length = headers
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(|s| s.parse::<usize>().unwrap())
                            })
                            .unwrap();
                        assert!(length < 4 * 1024 * 1024, "unbounded offline token request");
                        break (end + 4, length);
                    }
                };
                while bytes.len() < offset + length {
                    let mut buffer = [0; 8192];
                    let n = stream.read(&mut buffer).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                }
                let body: Value = serde_json::from_slice(&bytes[offset..offset + length]).unwrap();
                let content = body["content"].as_str();
                let tokens = match (
                    content,
                    body["parse_special"].as_bool(),
                    body["add_special"].as_bool(),
                ) {
                    (Some(content), Some(true), Some(false)) => fixed.get(content).cloned(),
                    (Some(content), Some(false), Some(false))
                        if server_allowed.lock().unwrap().contains(content) =>
                    {
                        let count = *counted
                            .entry(content.to_owned())
                            .or_insert_with(|| tokenizer.count(content).unwrap());
                        Some(vec![0; usize::try_from(count).unwrap()])
                    }
                    _ => None,
                };
                let (status, response) = match tokens {
                    Some(tokens) => ("200 OK", json!({"tokens":tokens}).to_string()),
                    None => (
                        "400 Bad Request",
                        json!({"error":"unattributed offline token request"}).to_string(),
                    ),
                };
                write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
            }
        });
        let mut runtime = LlamaCppRuntime::for_test(url, "offline-count-fixture".into());
        runtime.context_tokens = context;
        let mut result = Self {
            runtime,
            allowed,
            stop,
            server: Some(server),
            validated_records: 0,
            preflights: std::sync::atomic::AtomicUsize::new(0),
        };
        result.runtime.prompt_framing = PromptFraming::load(
            &result.runtime.client,
            &result.runtime.base_url,
            &result.runtime.api_token,
            disable_thinking,
        )
        .expect("every trusted native frame must have an exact recorded response");
        for record in records {
            let actual = result
                .runtime
                .prompt_tokens(
                    record["system"].as_str().unwrap(),
                    record["user"].as_str().unwrap(),
                    &UNCONTROLLED_EXECUTION,
                )
                .unwrap()
                .len();
            assert_eq!(
                actual,
                usize::try_from(record["expected_tokens"].as_u64().unwrap()).unwrap(),
                "recorded native token count mismatch for {}",
                record["alias"].as_str().unwrap()
            );
            result.validated_records += 1;
        }
        result
    }

    pub(crate) fn input_tokens(&self, request: &ModelRequest) -> usize {
        self.attribute(request);
        self.runtime
            .prompt_tokens(
                &request.system_prompt,
                &request.user_prompt,
                &UNCONTROLLED_EXECUTION,
            )
            .expect("attributed offline input must tokenize")
            .len()
    }

    pub(crate) fn preflight_count(&self) -> usize {
        self.preflights.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn attribute(&self, request: &ModelRequest) {
        assert!(
            self.validated_records > 0,
            "native count parity must precede capacity evidence"
        );
        let mut allowed = self.allowed.lock().unwrap();
        allowed.insert(request.system_prompt.clone());
        allowed.insert(request.user_prompt.clone());
    }
}
impl ModelRuntime for OfflineNativeCountRuntime {
    fn generate(&self, _: &ModelRequest) -> Result<ModelResponse, ModelRuntimeFailure> {
        panic!("OFFLINE_NATIVE_COUNT_GENERATION_FORBIDDEN");
    }
    fn health(&self) -> Result<(), ModelRuntimeFailure> {
        panic!("OFFLINE_NATIVE_COUNT_STARTUP_FORBIDDEN");
    }
    fn runtime_id(&self) -> &str {
        "offline-native-capacity-only"
    }
    fn model_id(&self) -> &str {
        "offline-native-capacity-only"
    }
    fn context_tokens(&self, stage: PipelineStage) -> u32 {
        self.runtime.context_tokens(stage)
    }
    fn preflight_request(&self, request: &ModelRequest) -> Result<(), ModelRuntimeFailure> {
        self.attribute(request);
        self.preflights
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.runtime.preflight_request(request)
    }
}

#[test]
#[ignore = "offline only; requires frozen complete native count inventory and pinned tokenizer metadata"]
fn offline_native_counter_matches_recorded_counts() {
    let path = PathBuf::from(
        std::env::var_os("DOC_SUM_OFFLINE_COUNT_INVENTORY").expect("explicit count inventory"),
    );
    let hash =
        std::env::var("DOC_SUM_OFFLINE_COUNT_INVENTORY_SHA256").expect("frozen inventory hash");
    let counter = OfflineNativeCountRuntime::from_frozen_inventory(&path, &hash);
    for body in [
        json!({"content":"unattributed-offline-count","parse_special":false,"add_special":false}),
        json!({"content":"unrecorded trusted frame","parse_special":true,"add_special":false}),
        json!({"content":"unattributed-offline-count","parse_special":false,"add_special":true}),
        json!({"content":false,"parse_special":false,"add_special":false}),
    ] {
        let response = counter
            .runtime
            .client
            .post(counter.runtime.endpoint("/tokenize"))
            .json(&body)
            .send()
            .unwrap();
        assert_eq!(
            response.status(),
            reqwest::StatusCode::BAD_REQUEST,
            "unattributed token counts must never be guessed"
        );
    }
    println!(
        "OFFLINE_NATIVE_COUNT_PARITY records={} context={} generation_calls=0 runtime_startups=0",
        counter.validated_records,
        counter.context_tokens(PipelineStage::Synthesize)
    );
}
