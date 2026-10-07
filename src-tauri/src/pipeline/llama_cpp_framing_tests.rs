use super::*;
use serde_json::{json, Value};

const CLOSED: &str = "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n";

// Exercise the framing loader and the token assembly that completion consumes.
// The echo tokenizer makes omissions, ordering and parse_special observable.
#[test]
fn native_prompt_closes_thinking_without_interpreting_content_as_control() {
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
    runtime.prompt_framing =
        PromptFraming::load(&runtime.client, &runtime.base_url, &runtime.api_token).unwrap();
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
    let expected =
        format!("<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\n{user}{CLOSED}");
    assert_eq!(
        tokens,
        expected.bytes().map(u32::from).collect::<Vec<_>>(),
        "native framing omits validated closed-thinking boundary"
    );
}

fn save(root: &Path, name: &str, value: &Value) {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))
        .unwrap();
    f.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
}

#[test]
#[ignore = "zero generation; requires frozen C9 packet, qualified GGUF and exclusive inference lock"]
fn original_c9_native_framing_parity() {
    use crate::pipeline::model_settings::{
        JACK_LLAMA_CPP_LIBRARIES, QUALIFIED_LLAMA_SERVER_DIGEST,
    };
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
    let model = PathBuf::from(std::env::var("DOC_SUM_QUALIFICATION_GGUF").unwrap());
    let (model_path, size, digest, identity) = inspect_regular_file(&model).unwrap();
    assert_eq!(
        digest,
        "cd76ec205963b3b33350093e6904d9de16c4e666fd104e1f632d25c7f15f2a13"
    );
    let scratch = tempfile::tempdir().unwrap();
    let _shutdown = Shutdown;
    let runtime = LlamaCppRuntime::shared(GgufRuntimeConfig {
        model_path,
        runtime_parent: scratch.path().into(),
        model_digest: digest,
        expected_size_bytes: size,
        expected_file_identity: identity,
        expected_server_digest: QUALIFIED_LLAMA_SERVER_DIGEST.into(),
        expected_runtime_libraries: JACK_LLAMA_CPP_LIBRARIES,
        context_tokens: 32768,
    })
    .unwrap();
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
