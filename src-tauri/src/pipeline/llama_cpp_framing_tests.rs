// Synthetic transport helper and prompt inputs ported from e12ac4a's framing tests.
// Frozen payloads were captured before extraction, directly from the original wire.
use super::*;
use crate::pipeline::contracts::ModelOutputFormat;
use crate::pipeline::native_request::CompletionResponse;
use serde_json::{json, Value};

#[test]
fn raw_completion_seam_preserves_whitespace() {
    let body =
        br#"{"content":"  ok\n", "tokens_predicted":1, "tokens_evaluated":2, "stop_type":"word"}"#;
    let completed: CompletionResponse = serde_json::from_slice(body).unwrap();
    assert_eq!(completed.validated_content(2, 1).unwrap(), "  ok\n");
}
fn capture_native_framed_request(
    policy: FramingPolicy,
    stage: PipelineStage,
    output_format: ModelOutputFormat,
) -> (Value, Vec<Vec<u8>>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut completion = Value::Null;
        let mut wire_bytes = Vec::new();
        for index in 0..6 {
            let deadline = Instant::now() + Duration::from_secs(10);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(peer) => break peer,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("synthetic capture peer unavailable: {e}"),
                }
            };
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
            wire_bytes.push(bytes[offset..offset + length].to_vec());
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
        (completion, wire_bytes)
    });
    let mut runtime = LlamaCppRuntime::for_test(url, "fixture".into());
    runtime.prompt_framing = load_prompt_framing(
        &runtime.client,
        &runtime.base_url,
        &runtime.api_token,
        policy,
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
fn legacy_and_qualified_wires_match_pinned_originals() {
    let fixtures: Value =
        serde_json::from_str(include_str!("native_request_fixtures.json")).unwrap();
    let mut cases = 0;
    for (name, policy) in [
        ("legacy", FramingPolicy::Legacy),
        (
            "qualified_open",
            FramingPolicy::qualified_fixture([0x51; 32], false),
        ),
        (
            "qualified_closed",
            FramingPolicy::qualified_fixture([0x51; 32], true),
        ),
    ] {
        for (stage_name, stage) in [
            ("analyze", PipelineStage::Analyze),
            ("synthesize", PipelineStage::Synthesize),
            ("verify", PipelineStage::Verify),
        ] {
            for (schema_name, output) in [
                ("text", ModelOutputFormat::Text),
                (
                    "non_c9",
                    ModelOutputFormat::JsonSchema {
                        name: "u1b_synthetic_non_c9".into(),
                        schema: json!({"type":"object", "properties":{"z":{"type":"string"}, "a":{"type":"string"}}, "required":["z","a"], "additionalProperties":false}),
                    },
                ),
            ] {
                let (_, actual) =
                    capture_native_framed_request(policy.clone(), stage.clone(), output);
                let id = format!("{name}-{stage_name}-{schema_name}");
                let expected: Vec<_> = fixtures["cases"][&id]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| s.as_str().unwrap().as_bytes().to_vec())
                    .collect();
                assert_eq!(actual, expected, "wire bytes changed for {id}");
                for (index, bytes) in actual.iter().enumerate() {
                    save_capture(&format!("{id}/{index}.json.raw"), bytes);
                }
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 18);
}

#[test]
fn raw_content_and_legacy_final_trim_are_distinct() {
    for (raw, expected) in [("  ok\n", Some("ok")), ("", None), (" \n\t", None)] {
        let body =
            json!({"content":raw,"tokens_predicted":1,"tokens_evaluated":2,"stop_type":"word"})
                .to_string();
        let envelope: CompletionResponse = serde_json::from_str(&body).unwrap();
        assert_eq!(envelope.validated_content(2, 1).unwrap(), raw);
        let result = super::tests::generation_result_for_completion(&body);
        match expected {
            Some(expected) => assert_eq!(result.unwrap().text, expected),
            None => assert_eq!(result.unwrap_err().code, "MODEL_RESPONSE_EMPTY"),
        }
    }
}

#[test]
fn framing_rejects_each_empty_control_fragment_and_preserves_tokenizer_failure() {
    for policy in [
        FramingPolicy::Legacy,
        FramingPolicy::qualified_fixture([0x51; 32], false),
        FramingPolicy::qualified_fixture([0x51; 32], true),
    ] {
        for empty in 0..3 {
            let mut index = 0;
            let result = PromptFraming::load(policy.clone(), |_, special| {
                assert!(special);
                let tokens = if index == empty { vec![] } else { vec![index] };
                index += 1;
                Ok(tokens)
            });
            let error = result.err().unwrap();
            assert_eq!(
                (
                    error.code.as_str(),
                    error.message.as_str(),
                    error.recoverable
                ),
                (
                    "MODEL_EXECUTION_UNVERIFIED",
                    "Qualified Qwen prompt framing could not be tokenized",
                    false
                )
            );
        }
        let expected = failure("MODEL_RESPONSE_INVALID", "fixture tokenizer failure", true);
        let result = PromptFraming::load(policy, |_, _| Err(expected.clone()));
        assert_eq!(result.err().unwrap(), expected);
    }
}

#[test]
fn bounded_decoder_handles_limits_malformed_and_read_failures() {
    use std::io::{self, Cursor};
    let valid = br#"{"content":"ok","tokens_predicted":1,"tokens_evaluated":2,"stop_type":"eos"}"#;
    for cap in [valid.len() as u64, valid.len() as u64 + 1] {
        let body: CompletionResponse =
            native_request::decode_bounded_json(Cursor::new(valid), cap).unwrap();
        assert_eq!(body.validated_content(2, 1).unwrap(), "ok");
    }
    for cap in [0, valid.len() as u64 - 1] {
        let error =
            native_request::decode_bounded_json::<CompletionResponse>(Cursor::new(valid), cap)
                .err()
                .unwrap();
        assert_eq!(error.code, "MODEL_RESPONSE_TOO_LARGE");
        assert!(error.recoverable);
    }
    for bytes in [
        b"".as_slice(),
        b"{",
        &valid[..valid.len() - 1],
        b"{\"content\":false}",
    ] {
        let error = native_request::decode_bounded_json::<CompletionResponse>(
            Cursor::new(bytes),
            MAX_RESPONSE_BYTES,
        )
        .err()
        .unwrap();
        assert_eq!(error.code, "MODEL_RESPONSE_INVALID");
    }
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("fixture"))
        }
    }
    let error =
        native_request::decode_bounded_json::<CompletionResponse>(Broken, MAX_RESPONSE_BYTES)
            .err()
            .unwrap();
    assert_eq!(
        (error.code.as_str(), error.message.as_str()),
        ("MODEL_RESPONSE_INVALID", "GGUF response could not be read")
    );
}

#[test]
fn cancellation_after_content_tokenization_sends_no_completion() {
    use crate::pipeline::control::CancellationToken;
    for cancel_after in [1, 2] {
        let control = CancellationToken::new();
        let server_control = control.clone();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let runtime = LlamaCppRuntime::for_test(
            format!("http://{}", listener.local_addr().unwrap()),
            "fixture".into(),
        );
        let server = thread::spawn(move || {
            for index in 1..=cancel_after {
                let deadline = Instant::now() + Duration::from_secs(5);
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(peer) => break peer,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(5))
                        }
                        Err(e) => panic!("synthetic peer missing: {e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = [0; 4096];
                let n = stream.read(&mut bytes).unwrap();
                assert!(bytes[..n].starts_with(b"POST /tokenize "));
                if index == cancel_after {
                    server_control.request();
                }
                let response = r#"{"tokens":[1]}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                )
                .unwrap();
            }
            listener
        });
        let result = runtime.generate_with_control(
            &ModelRequest {
                stage: PipelineStage::Analyze,
                ordinal: 0,
                system_prompt: "system".into(),
                user_prompt: "user".into(),
                seed: 42,
                max_output_tokens: 1,
                output_format: ModelOutputFormat::Text,
            },
            &control,
        );
        assert_eq!(result.unwrap_err().code, "MODEL_REQUEST_CANCELLED");
        let listener = server.join().unwrap();
        assert!(matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock));
    }
}

#[test]
fn response_results_match_original_typed_errors_and_usage() {
    let fixtures: Value =
        serde_json::from_str(include_str!("native_response_fixtures.json")).unwrap();
    for row in fixtures["responses"].as_array().unwrap() {
        let (actual, mut attempts) = match super::tests::generation_result_for_completion(
            row["input"].as_str().unwrap(),
        ) {
            Ok(response) => (
                json!({"text":response.text,"runtime_id":response.runtime_id,"model_id":response.model_id}),
                response.request_attempts,
            ),
            Err(error) => (
                json!({"code":error.code,"message":error.message,"recoverable":error.recoverable}),
                error.request_attempts,
            ),
        };
        for attempt in &mut attempts {
            attempt.elapsed_milliseconds = 0;
        }
        assert_eq!(actual, row["result"], "result changed for {}", row["input"]);
        assert_eq!(
            serde_json::to_value(attempts).unwrap(),
            row["attempts"],
            "usage changed for {}",
            row["input"]
        );
    }
}

#[test]
fn cache_identity_preserves_legacy_and_separates_qualified_bindings() {
    static LIBRARIES: &[QualifiedRuntimeFile] = &[QualifiedRuntimeFile {
        file_name: "libfixture.so.1",
        digest: "b718f1354f7247312eca086d9a024afe5fa717ddea5adeddd6f12bcf945b2e8c",
    }];
    let config = GgufRuntimeConfig {
        model_path: PathBuf::from("/model.gguf"),
        runtime_parent: PathBuf::from("/runtime"),
        model_digest: "a".repeat(64),
        expected_size_bytes: 1,
        expected_file_identity: FileIdentity {
            device: 1,
            inode: 1,
            size_bytes: 1,
            modified_seconds: 1,
            modified_nanoseconds: 0,
            changed_seconds: 1,
            changed_nanoseconds: 0,
        },
        expected_server_digest: "b".repeat(64),
        expected_runtime_libraries: LIBRARIES,
        context_tokens: 8_192,
        framing_policy: FramingPolicy::Legacy,
    };

    let fixtures: Value =
        serde_json::from_str(include_str!("native_response_fixtures.json")).unwrap();
    let legacy = runtime_cache_key(&config).unwrap();
    assert_eq!(legacy, fixtures["cache_key"].as_str().unwrap());
    let mut keys = std::collections::HashSet::from([legacy]);
    for profile in [[0x51; 32], [0x52; 32]] {
        for disabled in [false, true] {
            let config = GgufRuntimeConfig {
                framing_policy: FramingPolicy::qualified_fixture(profile, disabled),
                ..config.clone()
            };
            let key = runtime_cache_key(&config).unwrap();
            assert_eq!(runtime_cache_key(&config.clone()).unwrap(), key);
            assert!(
                keys.insert(key),
                "different policy/profile reused a native runtime cache key"
            );
        }
    }
}

#[test]
fn response_byte_limits_accept_both_real_caps_and_reject_overread() {
    use std::io::Cursor;
    for cap in [MAX_TOKENIZE_RESPONSE_BYTES, MAX_RESPONSE_BYTES] {
        for length in [cap - 1, cap, cap + 1] {
            let mut bytes = b"{}".to_vec();
            bytes.resize(length as usize, b' ');
            let result = native_request::decode_bounded_json::<Value>(Cursor::new(bytes), cap);
            if length <= cap {
                assert_eq!(result.unwrap(), json!({}));
            } else {
                assert_eq!(result.unwrap_err().code, "MODEL_RESPONSE_TOO_LARGE");
            }
        }
    }
}

// Optional durable qualification receipt; no production path reads this variable.
fn save_capture(name: &str, bytes: &[u8]) {
    let Some(root) = std::env::var_os("DOC_SUM_U1B_WIRE_CAPTURE") else {
        return;
    };
    let path = PathBuf::from(root).join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}
