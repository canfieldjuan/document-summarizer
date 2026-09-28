use super::*;
use crate::connect::ocr_consumer::tests::{
    completed_status, image_only_pdf, live_provider, snapshot,
};
use crate::pipeline::contracts::SummaryProfile;
use crate::pipeline::ingest::prepare_pdf_ingestion;
use crate::pipeline::parser::{parse_started_document, PdfExtractParser};
use std::net::TcpListener;
use tempfile::TempDir;

fn fixture() -> (
    TempDir,
    Connection,
    LiveOcrProvider,
    OcrHandoff,
    Status,
    Vec<u8>,
    Vec<u8>,
) {
    fixture_at_version(false)
}

fn fixture_at_version(
    legacy: bool,
) -> (
    TempDir,
    Connection,
    LiveOcrProvider,
    OcrHandoff,
    Status,
    Vec<u8>,
    Vec<u8>,
) {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("scan.pdf");
    fs::write(&path, image_only_pdf()).unwrap();
    let database = directory.path().join("summarizer.db");
    let mut conn = if legacy {
        let conn = Connection::open(&database).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        db::create_v21_fixture(&conn);
        conn
    } else {
        db::init_db(database).unwrap()
    };
    let (document, received) = prepare_pdf_ingestion(path.to_str().unwrap(), None).unwrap();
    let ingested = db::persist_ingestion_with_profiles(
        &mut conn,
        &document,
        &received,
        Some(&snapshot()),
        SummaryProfile::General,
    )
    .unwrap();
    let (parsing, document) =
        db::start_parsing(&mut conn, &ingested.run_id, ingested.state_version).unwrap();
    parse_started_document(
        &mut conn,
        &PdfExtractParser::new(),
        &parsing.run_id,
        parsing.state_version,
        &document,
    )
    .unwrap();
    let mut provider = live_provider();
    provider.protocol_version = 3;
    let handoff = prepare_handoff(&mut conn, &parsing.run_id, directory.path(), &provider).unwrap();
    let inline = completed_status(&handoff);
    let mut value = serde_json::to_value(&inline).unwrap();
    value["protocol_version"] = 3.into();
    value["capability"]["version"] = "1.1".into();
    let mut bodies = Vec::new();
    for output in value["result"]["outputs"].as_array_mut().unwrap() {
        let mut bytes = BASE64
            .decode(output["payload_base64"].as_str().unwrap())
            .unwrap();
        if output["media_type"] == OCR_INPUT_MEDIA_TYPE {
            let mut document = lopdf::Document::load_mem(&bytes).unwrap();
            document.add_object(lopdf::Stream::new(
                lopdf::Dictionary::new(),
                vec![b'x'; MAX_PDF_BYTES],
            ));
            bytes.clear();
            document.save_to(&mut bytes).unwrap();
            assert!(bytes.len() > MAX_PDF_BYTES);
        }
        output.as_object_mut().unwrap().remove("payload_base64");
        output["byte_size"] = (bytes.len() as u64).into();
        output["sha256"] = sha256_hex(&bytes).into();
        bodies.push(bytes);
    }
    let status = decode_metadata(&serde_json::to_vec(&value).unwrap(), MAX_METADATA).unwrap();
    let text = bodies.pop().unwrap();
    let pdf = bodies.pop().unwrap();
    (directory, conn, provider, handoff, status, pdf, text)
}

struct Reply {
    path: String,
    headers: String,
    body: Vec<u8>,
}
fn artifact(path: String, bytes: &[u8]) -> Reply {
    Reply { path, headers: format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",bytes.len()), body:bytes.to_vec() }
}

fn server(replies: Vec<Reply>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let worker = thread::spawn(move || {
        let mut paths = Vec::new();
        for reply in replies {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("missing request: {error}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                let mut one = [0];
                socket.read_exact(&mut one).unwrap();
                bytes.push(one[0]);
                assert!(bytes.len() < 65536);
            }
            let request = String::from_utf8(bytes).unwrap();
            let path = request
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            assert_eq!(path, reply.path);
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-token"));
            paths.push(path.to_string());
            let _ = socket.write_all(reply.headers.as_bytes());
            let _ = socket.write_all(&reply.body);
        }
        paths
    });
    (format!("http://{address}/"), worker)
}

#[test]
fn streamed_large_pair_reopens_and_admits_only_one_child() {
    let (directory, conn, mut provider, handoff, status, pdf, text) = fixture();
    save_status(&conn, &handoff, &status).unwrap();
    drop(conn);
    let mut conn = db::init_db(directory.path().join("summarizer.db")).unwrap();
    let (pdf_desc, text_desc) = pair(&status).unwrap();
    let metadata = serde_json::to_vec(&status).unwrap();
    let (url, worker) = server(vec![
        Reply {
            path: format!("/v3/jobs/{}", handoff.provider_job_id),
            headers: format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                metadata.len()
            ),
            body: metadata,
        },
        artifact(
            format!(
                "/v3/jobs/{}/outputs/{}",
                handoff.provider_job_id, pdf_desc.artifact_id
            ),
            &pdf,
        ),
        artifact(
            format!(
                "/v3/jobs/{}/outputs/{}",
                handoff.provider_job_id, text_desc.artifact_id
            ),
            &text,
        ),
    ]);
    provider.base_url = url;
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    assert!(saved.ocr_pdf_bytes.is_none());
    let child = run_handoff(
        &mut conn,
        saved,
        directory.path(),
        &provider,
        &HttpOcrTransport,
        &UNCONTROLLED_EXECUTION,
    )
    .unwrap();
    assert_eq!(child, handoff.child_run_id);
    assert_eq!(worker.join().unwrap().len(), 3);
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    assert_eq!(saved.ocr_pdf_bytes.as_ref().unwrap(), &pdf);
    assert_eq!(saved.text_bytes.as_ref().unwrap(), &text);
    assert_eq!(
        run_handoff(
            &mut conn,
            saved,
            directory.path(),
            &provider,
            &HttpOcrTransport,
            &UNCONTROLLED_EXECUTION
        )
        .unwrap(),
        child
    );
    let count: usize = conn
        .query_row("SELECT COUNT(*) FROM ocr_lineage", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn metadata_and_descriptor_boundaries_reject_before_download() {
    let (_directory, conn, _provider, handoff, status, _pdf, _text) = fixture();
    for size in [MAX_PDF - 1, MAX_PDF] {
        let mut status = status.clone();
        status.result.as_mut().unwrap().outputs[0].byte_size = size as u64;
        assert!(validate(&handoff, &status).is_ok());
    }
    for size in [0, MAX_PDF + 1] {
        let mut status = status.clone();
        status.result.as_mut().unwrap().outputs[0].byte_size = size as u64;
        assert!(validate(&handoff, &status).is_err());
    }
    for bytes in [br#"{"a":1,"a":2}"#.as_slice(), br#"{"a":NaN}"#.as_slice()] {
        assert!(decode_metadata::<Value>(bytes, MAX_METADATA).is_err());
    }
    for depth in [16, 17] {
        let bytes = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        assert_eq!(
            decode_metadata::<Value>(bytes.as_bytes(), MAX_METADATA).is_ok(),
            depth == 16
        );
    }
    let mut value = serde_json::to_value(&status).unwrap();
    value["result"]["outputs"][0]["payload_base64"] = "injected".into();
    assert!(decode_metadata::<Status>(&serde_json::to_vec(&value).unwrap(), MAX_METADATA).is_err());
    let mut wrong = status.clone();
    wrong.result.as_mut().unwrap().outputs[1].artifact_id = wrong.result.as_ref().unwrap().outputs
        [0]
    .artifact_id
    .clone();
    assert!(save_status(&conn, &handoff, &wrong).is_err());
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    assert_eq!(saved.phase, "failed");
    assert!(saved.ocr_pdf_bytes.is_none());
}

#[test]
fn input_aliased_output_is_rejected_before_output_get() {
    let (directory, mut conn, mut provider, handoff, mut status, _, _) = fixture();
    let mut processing = status.clone();
    processing.status = JobState::Processing;
    processing.result = None;
    save_status(&conn, &handoff, &processing).unwrap();
    status.result.as_mut().unwrap().outputs[0].artifact_id =
        status.input_artifacts[0].artifact_id.clone();
    let (url, worker) = server(vec![status_reply(&handoff, &status)]);
    provider.base_url = url;
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    let result = run_handoff(
        &mut conn,
        saved,
        directory.path(),
        &provider,
        &HttpOcrTransport,
        &UNCONTROLLED_EXECUTION,
    );
    assert!(
        matches!(result, Err(OcrConsumerError::InvalidOutput(_))),
        "{result:?}"
    );
    assert_eq!(
        worker.join().unwrap(),
        vec![format!("/v3/jobs/{}", handoff.provider_job_id)]
    );
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    assert_eq!(saved.phase, "failed");
    assert!(saved.ocr_pdf_bytes.is_none());
    assert!(saved.text_bytes.is_none());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn changed_completed_descriptors_never_replace_saved_pair() {
    let (_directory, conn, _provider, handoff, status, _pdf, _text) = fixture();
    save_status(&conn, &handoff, &status).unwrap();
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    let mut wrong = status;
    wrong.result.as_mut().unwrap().outputs[0].display_name = "changed.pdf".to_string();
    assert!(save_status(&conn, &saved, &wrong).is_err());
    assert!(reload(&conn, &handoff.handoff_id)
        .unwrap()
        .ocr_pdf_bytes
        .is_none());
}

#[test]
fn output_http_framing_refuses_duplicate_content_length() {
    let (_directory, _conn, mut provider, handoff, status, pdf, _text) = fixture();
    let (descriptor, _) = pair(&status).unwrap();
    let path = format!(
        "/v3/jobs/{}/outputs/{}",
        handoff.provider_job_id, descriptor.artifact_id
    );
    let mut reply = artifact(path, &pdf);
    reply.headers = reply.headers.replace(
        "Connection: close",
        &format!("Content-Length: {}\r\nConnection: close", pdf.len()),
    );
    let (url, worker) = server(vec![reply]);
    provider.base_url = url;
    let result = download(
        &HttpOcrTransport,
        &provider,
        &handoff.provider_job_id,
        descriptor,
        Instant::now() + Duration::from_secs(3),
        &UNCONTROLLED_EXECUTION,
    );
    worker.join().unwrap();
    assert!(
        matches!(result, Err(TransportError::Invalid(_))),
        "duplicate Content-Length admitted"
    );
}

#[test]
fn download_notices_cancellation_while_body_stalls() {
    let (_directory, _conn, mut provider, handoff, status, _pdf, _text) = fixture();
    let (descriptor, _) = pair(&status).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    provider.base_url = format!("http://{}/", listener.local_addr().unwrap());
    let token = crate::pipeline::control::CancellationToken::new();
    let request_cancel = token.clone();
    let size = descriptor.byte_size;
    let worker = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut one = [0];
            socket.read_exact(&mut one).unwrap();
            request.push(one[0]);
        }
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {size}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n").unwrap();
        thread::sleep(Duration::from_millis(100));
        request_cancel.request();
        let _ = socket.read(&mut [0]);
    });
    let start = Instant::now();
    let result = download(
        &HttpOcrTransport,
        &provider,
        &handoff.provider_job_id,
        descriptor,
        start + Duration::from_secs(3),
        &token,
    );
    let elapsed = start.elapsed();
    worker.join().unwrap();
    assert!(result.is_err());
    assert!(
        elapsed < Duration::from_secs(1),
        "cancel waited for the transfer deadline: {elapsed:?}"
    );
}

#[test]
#[ignore = "reads canonical fixtures from CONNECT_CONTRACTS_DIR at the pinned revision"]
fn pinned_streamed_contract_fixtures() {
    let root = std::env::var("CONNECT_CONTRACTS_DIR").unwrap();
    let read = |path: &str| {
        let output = std::process::Command::new("git")
            .args(["show", &format!("{CONTRACT_REVISION}:fixtures/v3/{path}")])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(output.status.success(), "{path}");
        output.stdout
    };
    let (_dir, _conn, _provider, mut handoff, _, _, _) = fixture();
    let reference: Status =
        decode_metadata(&read("valid/job-completed-ocr-large.json"), MAX_METADATA).unwrap();
    handoff.provider_job_id = reference.job_id.clone();
    handoff.provider_instance_id = reference.provider.instance_id.clone();
    handoff.source_artifact_id = reference.input_artifacts[0].artifact_id.clone();
    handoff.source_sha256 = reference.input_artifacts[0].sha256.clone();
    handoff.source_byte_size = reference.input_artifacts[0].byte_size;
    let cases: Value = serde_json::from_slice(&read("index.json")).unwrap();
    let mut count = 0;
    for case in cases.as_array().unwrap() {
        let path = case["fixture"].as_str().unwrap();
        let expected = case["valid"].as_bool().unwrap();
        let bytes = read(path);
        let actual = match case["schema"].as_str().unwrap() {
            "job-status.schema.json" if case["provider_manifest"] == "valid/manifest-ocr.json" => {
                decode_metadata::<Status>(&bytes, MAX_METADATA)
                    .map_err(map_transport)
                    .and_then(|status| validate(&handoff, &status))
                    .is_ok()
            }
            "error.schema.json" => {
                let value: Value = serde_json::from_slice(&bytes).unwrap();
                let status = match value["error"]["code"].as_str().unwrap() {
                    "OUTPUT_BUSY" => StatusCode::TOO_MANY_REQUESTS,
                    "OUTPUT_NOT_READY" => StatusCode::CONFLICT,
                    "OUTPUT_NOT_FOUND" | "JOB_NOT_FOUND" => StatusCode::NOT_FOUND,
                    "MALFORMED_REQUEST" => StatusCode::BAD_REQUEST,
                    "OUTPUT_UNAVAILABLE" => StatusCode::INTERNAL_SERVER_ERROR,
                    other => panic!("unexpected fixture code: {other}"),
                };
                error_response(status, &bytes).is_ok()
            }
            _ => continue,
        };
        assert_eq!(actual, expected, "{path}");
        count += 1;
    }
    assert!(count > 30, "canonical fixture coverage changed: {count}");
    println!("canonical v3 OCR status/error fixtures: {count}");
}

#[test]
fn populated_v21_migration_preserves_legacy_pair_and_lineage() {
    let (directory, mut conn, mut provider, handoff, _status, _pdf, _text) =
        fixture_at_version(true);
    provider.protocol_version = 2;
    let mut request: JobRequest = serde_json::from_str(&handoff.provider_request_json).unwrap();
    request.protocol_version = 2;
    request.capability.version = "1.0".to_string();
    let encoded = serde_json::to_string(&request).unwrap();
    conn.execute("UPDATE ocr_handoffs SET provider_request_json=?1, provider_request_sha256=?2 WHERE handoff_id=?3", rusqlite::params![encoded, sha256_hex(encoded.as_bytes()), handoff.handoff_id]).unwrap();
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    apply_status(&conn, &saved, completed_status(&saved)).unwrap();
    let ready = reload(&conn, &handoff.handoff_id).unwrap();
    run_handoff(
        &mut conn,
        ready,
        directory.path(),
        &provider,
        &HttpOcrTransport,
        &UNCONTROLLED_EXECUTION,
    )
    .unwrap();
    let before = reload(&conn, &handoff.handoff_id).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        21
    );
    drop(conn);
    let conn = db::init_db(directory.path().join("summarizer.db")).unwrap();
    let after = reload(&conn, &handoff.handoff_id).unwrap();
    assert_eq!(after.provider_request_json, before.provider_request_json);
    assert_eq!(after.provider_status_json, before.provider_status_json);
    assert_eq!(after.source_bytes, before.source_bytes);
    assert_eq!(after.ocr_pdf_bytes, before.ocr_pdf_bytes);
    assert_eq!(after.text_bytes, before.text_bytes);
    assert_eq!(after.child_run_id, before.child_run_id);
    assert_eq!(after.phase, before.phase);
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        22
    );
    assert!(conn
        .pragma_query_value(None, "foreign_keys", |r| r.get::<_, bool>(0))
        .unwrap());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert!(conn.execute("DELETE FROM ocr_lineage", []).is_err());
    for size in [MAX_PDF_BYTES - 1, MAX_PDF_BYTES, MAX_PDF_BYTES + 1] {
        let result=conn.execute("UPDATE ocr_handoffs SET ocr_pdf_byte_size=?1, ocr_pdf_bytes=zeroblob(?1) WHERE handoff_id=?2",rusqlite::params![size,handoff.handoff_id]);
        assert_eq!(result.is_ok(), size <= MAX_PDF_BYTES);
    }
    drop(conn);
    let conn = db::init_db(directory.path().join("summarizer.db")).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        22
    );
}

fn status_reply(handoff: &OcrHandoff, status: &Status) -> Reply {
    let bytes = serde_json::to_vec(status).unwrap();
    Reply {
        path: format!("/v3/jobs/{}", handoff.provider_job_id),
        headers: format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        ),
        body: bytes,
    }
}
fn output_path(handoff: &OcrHandoff, descriptor: &Descriptor) -> String {
    format!(
        "/v3/jobs/{}/outputs/{}",
        handoff.provider_job_id, descriptor.artifact_id
    )
}
fn busy_reply(path: String) -> Reply {
    let bytes=br#"{"protocol_version":3,"error":{"code":"OUTPUT_BUSY","message":"Busy","retryable":true}}"#.to_vec();
    Reply {path, headers: format!("HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()),body:bytes}
}

#[test]
fn output_framing_and_integrity_fail_closed() {
    let (_dir, _conn, mut provider, handoff, status, _, text) = fixture();
    let (_, descriptor) = pair(&status).unwrap();
    for case in [
        "valid",
        "wrong-content-type",
        "missing-length",
        "conflicting-length",
        "length-mismatch",
        "excess-body",
        "chunked",
        "encoded",
        "filename",
        "cache",
        "redirect",
        "corrupt",
        "short",
    ] {
        let mut reply = artifact(output_path(&handoff, descriptor), &text);
        match case {
            "wrong-content-type" => {
                reply.headers = reply
                    .headers
                    .replace("application/octet-stream", "application/json");
            }
            "missing-length" => {
                reply.headers = reply
                    .headers
                    .replace(&format!("Content-Length: {}\r\n", text.len()), "")
            }
            "conflicting-length" => {
                reply.headers = reply.headers.replace(
                    "Connection: close",
                    "Content-Length: 1\r\nConnection: close",
                )
            }
            "length-mismatch" => {
                reply.headers = reply.headers.replace(
                    &format!("Content-Length: {}", text.len()),
                    &format!("Content-Length: {}", text.len() + 1),
                )
            }
            "excess-body" => {
                let mut excess = text.clone();
                excess.push(b'x');
                reply = artifact(output_path(&handoff, descriptor), &excess);
            }
            "chunked" => {
                reply.headers = reply.headers.replace(
                    "Connection: close",
                    "Transfer-Encoding: chunked\r\nConnection: close",
                )
            }
            "encoded" => {
                reply.headers = reply.headers.replace(
                    "Connection: close",
                    "Content-Encoding: gzip\r\nConnection: close",
                )
            }
            "filename" => {
                reply.headers = reply.headers.replace(
                    "Connection: close",
                    "Content-Disposition: attachment; filename=x\r\nConnection: close",
                )
            }
            "cache" => reply.headers = reply.headers.replace("no-store", "public"),
            "redirect" => {
                reply.headers = reply.headers.replace("200 OK", "302 Found").replace(
                    "Connection: close",
                    "Location: http://127.0.0.1:1/forbidden\r\nConnection: close",
                )
            }
            "corrupt" => reply.body[0] ^= 1,
            "short" => {
                reply.body.pop();
            }
            _ => {}
        }
        let (url, worker) = server(vec![reply]);
        provider.base_url = url;
        let result = download(
            &HttpOcrTransport,
            &provider,
            &handoff.provider_job_id,
            descriptor,
            Instant::now() + Duration::from_secs(2),
            &UNCONTROLLED_EXECUTION,
        );
        worker.join().unwrap();
        assert_eq!(result.is_ok(), case == "valid", "{case}: {result:?}");
        if case == "conflicting-length" {
            assert!(
                matches!(result, Err(TransportError::Invalid(_))),
                "bad framing was classified as transient: {result:?}"
            );
        }
    }
}

#[test]
fn busy_is_bounded_and_recovery_reuses_job_without_partial_pair() {
    let (dir, mut conn, mut provider, handoff, status, pdf, text) = fixture();
    let (pdf_desc, text_desc) = pair(&status).unwrap();
    save_status(&conn, &handoff, &status).unwrap();
    let (url, worker) = server(vec![
        status_reply(&handoff, &status),
        busy_reply(output_path(&handoff, pdf_desc)),
        busy_reply(output_path(&handoff, pdf_desc)),
        busy_reply(output_path(&handoff, pdf_desc)),
    ]);
    provider.base_url = url;
    let start = Instant::now();
    let result = run_handoff_until(
        &mut conn,
        reload_for_test(&dir),
        dir.path(),
        &provider,
        &HttpOcrTransport,
        &UNCONTROLLED_EXECUTION,
        start + Duration::from_secs(10),
    );
    assert!(matches!(result, Err(OcrConsumerError::Transport(_))));
    assert!(start.elapsed() >= Duration::from_secs(2));
    assert_eq!(worker.join().unwrap().len(), 4);
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    assert_eq!(saved.phase, "running");
    assert!(saved.ocr_pdf_bytes.is_none());
    assert!(saved.text_bytes.is_none());
    let (url, worker) = server(vec![
        status_reply(&handoff, &status),
        artifact(output_path(&handoff, pdf_desc), &pdf),
        artifact(output_path(&handoff, text_desc), &text),
    ]);
    provider.base_url = url;
    run_handoff(
        &mut conn,
        saved,
        dir.path(),
        &provider,
        &HttpOcrTransport,
        &UNCONTROLLED_EXECUTION,
    )
    .unwrap();
    assert_eq!(worker.join().unwrap().len(), 3);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        1
    );
}
fn reload_for_test(dir: &TempDir) -> OcrHandoff {
    let conn = db::init_db(dir.path().join("summarizer.db")).unwrap();
    db::list_recoverable_ocr_handoffs(&conn).unwrap().remove(0)
}

#[test]
fn corrupt_second_sibling_stores_neither_artifact_nor_child() {
    let (dir, mut conn, mut provider, handoff, status, pdf, mut text) = fixture();
    let (pdf_desc, text_desc) = pair(&status).unwrap();
    save_status(&conn, &handoff, &status).unwrap();
    text[0] ^= 1;
    let (url, worker) = server(vec![
        status_reply(&handoff, &status),
        artifact(output_path(&handoff, pdf_desc), &pdf),
        artifact(output_path(&handoff, text_desc), &text),
    ]);
    provider.base_url = url;
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    assert!(matches!(
        run_handoff(
            &mut conn,
            saved,
            dir.path(),
            &provider,
            &HttpOcrTransport,
            &UNCONTROLLED_EXECUTION
        ),
        Err(OcrConsumerError::InvalidOutput(_))
    ));
    assert_eq!(worker.join().unwrap().len(), 3);
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    assert_eq!(saved.phase, "failed");
    assert!(saved.ocr_pdf_bytes.is_none());
    assert!(saved.text_bytes.is_none());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        0
    );
}

#[test]
fn token_rotation_reconciles_same_completed_job_and_rejects_changed_descriptors() {
    struct Rotating {
        status: Status,
        provider: LiveOcrProvider,
        calls: std::cell::Cell<usize>,
    }
    impl OcrTransport for Rotating {
        fn submit(
            &self,
            _: &LiveOcrProvider,
            _: &JobRequest,
            _: &[u8],
            _: Instant,
        ) -> Result<ReceivedStatus, TransportError> {
            panic!("must never resubmit completed OCR")
        }
        fn status(
            &self,
            provider: &LiveOcrProvider,
            job: &str,
            _: Instant,
        ) -> Result<ReceivedStatus, TransportError> {
            assert_eq!(job, self.status.job_id);
            self.calls.set(self.calls.get() + 1);
            if provider.token == "expired" {
                Err(TransportError::Unauthorized)
            } else {
                Ok(ReceivedStatus::Streamed(self.status.clone()))
            }
        }
        fn rediscover(&self, _: &LiveOcrProvider) -> Result<LiveOcrProvider, OcrConsumerError> {
            Ok(self.provider.clone())
        }
    }
    let (_dir, conn, provider, handoff, status, _, _) = fixture();
    save_status(&conn, &handoff, &status).unwrap();
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    let mut expired = provider.clone();
    expired.token = "expired".into();
    let mut transport = Rotating {
        status,
        provider,
        calls: std::cell::Cell::new(0),
    };
    assert!(reconcile(
        &saved,
        &mut expired,
        &transport,
        Instant::now() + Duration::from_secs(2)
    )
    .is_ok());
    assert_eq!(transport.calls.get(), 2);
    assert_eq!(expired.token, "fixture-token");
    transport.status.result.as_mut().unwrap().outputs[0].display_name = "changed".into();
    assert!(matches!(
        reconcile(
            &saved,
            &mut expired,
            &transport,
            Instant::now() + Duration::from_secs(2)
        ),
        Err(OcrConsumerError::InvalidOutput(_))
    ));
    transport.provider.instance_id = Uuid::new_v4().to_string();
    assert!(matches!(
        refresh_provider(&expired, &transport),
        Err(OcrConsumerError::InvalidOutput(_))
    ));
}

#[test]
fn new_jobs_prefer_v3_but_saved_v2_never_upgrades() {
    let (_dir, conn, mut provider, handoff, status, _, _) = fixture();
    let mut legacy = provider.clone();
    legacy.protocol_version = 2;
    assert_eq!(
        choose_provider(vec![legacy.clone(), provider.clone()])
            .unwrap()
            .protocol_version,
        3
    );
    assert_eq!(choose_provider(vec![legacy]).unwrap().protocol_version, 2);
    let mut request: JobRequest = serde_json::from_str(&handoff.provider_request_json).unwrap();
    request.protocol_version = 2;
    request.capability.version = "1.0".into();
    let mut legacy_handoff = handoff.clone();
    legacy_handoff.provider_request_json = serde_json::to_string(&request).unwrap();
    assert!(validate(&legacy_handoff, &status).is_err());
    provider.instance_id = Uuid::new_v4().to_string();
    assert!(choose_provider(vec![provider, live_provider()]).is_err());
    assert!(conn.is_autocommit());
}

#[test]
fn v3_storage_pdf_cap_is_exact_and_text_cap_unchanged() {
    let (_dir, conn, _provider, handoff, status, _, _) = fixture();
    // The SQL checks independently prevent either profile from enlarging its cap.
    let (pdf, text) = pair(&status).unwrap();
    for size in [MAX_PDF - 1, MAX_PDF, MAX_PDF + 1] {
        let result=conn.execute("UPDATE ocr_handoffs SET ocr_pdf_artifact_id=?1,ocr_pdf_sha256=?2,ocr_pdf_byte_size=?3,ocr_pdf_bytes=zeroblob(?3),text_artifact_id=?4,text_sha256=?5,text_byte_size=1,text_bytes=x'78' WHERE handoff_id=?6",rusqlite::params![pdf.artifact_id,pdf.sha256,size,text.artifact_id,text.sha256,handoff.handoff_id]);
        assert_eq!(result.is_ok(), size <= MAX_PDF, "PDF size {size}");
    }
    for size in [MAX_TEXT_BYTES - 1, MAX_TEXT_BYTES, MAX_TEXT_BYTES + 1] {
        let result = conn.execute(
            "UPDATE ocr_handoffs SET text_byte_size=?1,text_bytes=zeroblob(?1) WHERE handoff_id=?2",
            rusqlite::params![size, handoff.handoff_id],
        );
        assert_eq!(result.is_ok(), size <= MAX_TEXT_BYTES, "text size {size}");
    }
}

#[test]
#[ignore = "isolated production provider and private frozen corpus; no inference"]
fn production_streamed_corpus_reaches_child_processing() {
    use crate::pipeline::{chunk, normalize, structure};
    let manifest = PathBuf::from(std::env::var("DOCSUM_OCR_CORPUS_MANIFEST").unwrap());
    let destination = PathBuf::from(std::env::var("DOCSUM_OCR_PROOF_DIR").unwrap());
    let cases: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    let mut rows = Vec::new();
    for case in cases.as_array().unwrap() {
        let id = case["case"].as_str().unwrap();
        assert!(id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
        let directory = destination.join(id);
        fs::create_dir(&directory).unwrap();
        let mut conn = db::init_db(directory.join("summarizer.db")).unwrap();
        let source = fs::read(case["path"].as_str().unwrap()).unwrap();
        assert_eq!(sha256_hex(&source), case["sha256"].as_str().unwrap());
        let path = directory.join("scan.pdf");
        fs::write(&path, &source).unwrap();
        let (document, received) = prepare_pdf_ingestion(path.to_str().unwrap(), None).unwrap();
        let root = db::persist_ingestion_with_profiles(
            &mut conn,
            &document,
            &received,
            Some(&snapshot()),
            SummaryProfile::General,
        )
        .unwrap();
        let parsed = crate::pipeline::parser::parse_document(
            &mut conn,
            &PdfExtractParser::new(),
            &root.run_id,
        )
        .unwrap();
        let eligible = parsed_document_requires_ocr(&parsed);
        let outcome =
            process_scanned_document(&mut conn, &root.run_id, &directory, &UNCONTROLLED_EXECUTION);
        let handoff = db::get_ocr_handoff_for_root(&conn, &root.run_id)
            .unwrap()
            .unwrap();
        let mut row = serde_json::json!({"case":id,"input_sha256":sha256_hex(&source),"native_requires_ocr":eligible,"phase":handoff.phase,"job_id":handoff.provider_job_id,"failure_code":handoff.error_code});
        fs::write(
            directory.join("request.json"),
            &handoff.provider_request_json,
        )
        .unwrap();
        if let Some(status) = &handoff.provider_status_json {
            fs::write(directory.join("status.json"), status).unwrap();
        }
        match outcome {
            Ok(child) => {
                assert_eq!(case["expected_status"], "completed");
                let pdf = handoff.ocr_pdf_bytes.as_ref().unwrap();
                let text = handoff.text_bytes.as_ref().unwrap();
                assert_eq!(sha256_hex(pdf), case["pdf_sha256"]);
                assert_eq!(sha256_hex(text), case["text_sha256"]);
                let parsed = crate::pipeline::parser::parse_document(
                    &mut conn,
                    &PdfExtractParser::new(),
                    &child,
                )
                .unwrap();
                assert_eq!(parsed.source_type, SourceType::OcrText);
                normalize::normalize_document(
                    &mut conn,
                    &normalize::CanonicalNormalizer::new(),
                    &child,
                )
                .unwrap();
                structure::structure_document(
                    &mut conn,
                    &structure::DeterministicStructureInterpreter::new(),
                    &child,
                )
                .unwrap();
                let chunks = chunk::chunk_document(
                    &mut conn,
                    &chunk::DeterministicDocumentChunker::new(),
                    &child,
                )
                .unwrap();
                row["pdf_bytes"] = pdf.len().into();
                row["pdf_sha256"] = sha256_hex(pdf).into();
                row["text_sha256"] = sha256_hex(text).into();
                row["pages"] = parsed.pages.len().into();
                row["chunks"] = chunks.chunks.len().into();
                row["warnings"] = serde_json::to_value(&chunks.warnings).unwrap();
                row["child_processed"] = true.into();
                // Recovery must reuse retained bytes even after the original file is gone.
                fs::rename(&path, directory.join("archived-original.pdf")).unwrap();
                drop(conn);
                let mut conn = db::init_db(directory.join("summarizer.db")).unwrap();
                let report = recover_ocr_handoffs(&mut conn, &directory).unwrap();
                assert_eq!(report.child_run_ids, vec![child]);
                assert!(report.warnings.is_empty());
                assert_eq!(
                    conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
                        .get::<_, u32>(0))
                        .unwrap(),
                    1
                );
            }
            Err(error) => {
                assert_eq!(case["expected_status"], "failed", "{id}: {error}");
                assert!(handoff.ocr_pdf_bytes.is_none());
                assert!(handoff.text_bytes.is_none());
                assert_eq!(
                    conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
                        .get::<_, u32>(0))
                        .unwrap(),
                    0
                );
                row["error"] = error.to_string().into();
                row["child_processed"] = false.into();
            }
        }
        rows.push(row);
        fs::write(
            destination.join("results.json"),
            serde_json::to_vec_pretty(&rows).unwrap(),
        )
        .unwrap();
    }
    let completed = rows.iter().filter(|r| r["child_processed"] == true).count();
    println!("frozen OCR corpus: {} cases; {} child pipelines processed; {} preserved failures; {} native OCR routes; no new OCR or summary inference", rows.len(),completed,rows.len()-completed,rows.iter().filter(|r|r["native_requires_ocr"]==true).count());
}

#[test]
fn lost_transfer_restarts_from_zero_and_never_resubmits_ocr() {
    let (dir, mut conn, mut provider, handoff, status, pdf, text) = fixture();
    save_status(&conn, &handoff, &status).unwrap();
    let (pdf_desc, text_desc) = pair(&status).unwrap();
    let mut interrupted = artifact(output_path(&handoff, pdf_desc), &pdf);
    interrupted.body.truncate(pdf.len() / 2);
    let (url, worker) = server(vec![
        status_reply(&handoff, &status),
        interrupted,
        artifact(output_path(&handoff, pdf_desc), &pdf),
        artifact(output_path(&handoff, text_desc), &text),
    ]);
    provider.base_url = url;
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    run_handoff(
        &mut conn,
        saved,
        dir.path(),
        &provider,
        &HttpOcrTransport,
        &UNCONTROLLED_EXECUTION,
    )
    .unwrap();
    let paths = worker.join().unwrap();
    assert_eq!(paths.len(), 4);
    assert_eq!(paths[1], paths[2]);
    assert_eq!(
        reload(&conn, &handoff.handoff_id)
            .unwrap()
            .ocr_pdf_bytes
            .unwrap(),
        pdf
    );
}

#[test]
fn progressing_body_cannot_extend_the_parent_deadline() {
    let (_dir, _conn, mut provider, handoff, status, _, _) = fixture();
    let (descriptor, _) = pair(&status).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    provider.base_url = format!("http://{}/", listener.local_addr().unwrap());
    let size = descriptor.byte_size;
    let worker = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut one = [0];
            socket.read_exact(&mut one).unwrap();
            request.push(one[0]);
        }
        write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {size}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n").unwrap();
        let mut writes = 0;
        for _ in 0..100 {
            if socket.write_all(b"x").is_err() {
                break;
            }
            writes += 1;
            thread::sleep(Duration::from_millis(20));
        }
        writes
    });
    let start = Instant::now();
    assert!(download(
        &HttpOcrTransport,
        &provider,
        &handoff.provider_job_id,
        descriptor,
        start + Duration::from_millis(300),
        &UNCONTROLLED_EXECUTION
    )
    .is_err());
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(worker.join().unwrap() > 2, "probe did not make progress");
}

#[test]
fn malformed_status_terminalizes_consumer_without_admitting_outputs() {
    let (dir, mut conn, mut provider, handoff, _, _, _) = fixture();
    db::transition_ocr_handoff(&conn, &handoff.handoff_id, "prepared", "running", None).unwrap();
    let (url, worker) = server(vec![Reply {
        path: format!("/v3/jobs/{}", handoff.provider_job_id),
        headers: "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n".into(),
        body: b"{}".to_vec(),
    }]);
    provider.base_url = url;
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    assert!(matches!(
        run_handoff(
            &mut conn,
            saved,
            dir.path(),
            &provider,
            &HttpOcrTransport,
            &UNCONTROLLED_EXECUTION
        ),
        Err(OcrConsumerError::InvalidOutput(_))
    ));
    assert_eq!(worker.join().unwrap().len(), 1);
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    assert_eq!(
        saved.phase, "failed",
        "malformed v3 status left the consumer running"
    );
    assert!(saved.ocr_pdf_bytes.is_none());
    assert!(saved.text_bytes.is_none());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
}

#[test]
fn concurrent_retrievals_share_one_child_without_holding_a_write_transaction() {
    use std::sync::{Arc, Barrier};
    struct ConcurrentStatus {
        status: Status,
        barrier: Arc<Barrier>,
    }
    impl OcrTransport for ConcurrentStatus {
        fn submit(
            &self,
            _: &LiveOcrProvider,
            _: &JobRequest,
            _: &[u8],
            _: Instant,
        ) -> Result<ReceivedStatus, TransportError> {
            panic!("completed job resubmitted")
        }
        fn status(
            &self,
            _: &LiveOcrProvider,
            _: &str,
            _: Instant,
        ) -> Result<ReceivedStatus, TransportError> {
            self.barrier.wait();
            Ok(ReceivedStatus::Streamed(self.status.clone()))
        }
    }
    let (dir, conn, mut provider, handoff, status, pdf, text) = fixture();
    save_status(&conn, &handoff, &status).unwrap();
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    let (pdf_desc, text_desc) = pair(&status).unwrap();
    let outputs = BTreeMap::from([
        (output_path(&handoff, pdf_desc), pdf),
        (output_path(&handoff, text_desc), text),
    ]);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    provider.base_url = format!("http://{}/", listener.local_addr().unwrap());
    let database = dir.path().join("summarizer.db");
    let network_database = database.clone();
    let server = thread::spawn(move || {
        for _ in 0..4 {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e)
                        if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("missing download: {e}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                let mut one = [0];
                socket.read_exact(&mut one).unwrap();
                bytes.push(one[0]);
            }
            let request = String::from_utf8(bytes).unwrap();
            let path = request
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            let body = outputs.get(path).unwrap();
            // No consumer can be holding a write transaction while awaiting this response.
            let mut probe = Connection::open(&network_database).unwrap();
            probe.busy_timeout(Duration::from_millis(100)).unwrap();
            probe
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .unwrap()
                .rollback()
                .unwrap();
            let reply = artifact(path.into(), body);
            socket.write_all(reply.headers.as_bytes()).unwrap();
            socket.write_all(&reply.body).unwrap();
        }
    });
    let transport = ConcurrentStatus {
        status,
        barrier: Arc::new(Barrier::new(2)),
    };
    thread::scope(|scope| {
        let first = scope.spawn(|| {
            let mut conn = db::init_db(&database).unwrap();
            run_handoff(
                &mut conn,
                saved.clone(),
                dir.path(),
                &provider,
                &transport,
                &UNCONTROLLED_EXECUTION,
            )
            .unwrap()
        });
        let second = scope.spawn(|| {
            let mut conn = db::init_db(&database).unwrap();
            run_handoff(
                &mut conn,
                saved.clone(),
                dir.path(),
                &provider,
                &transport,
                &UNCONTROLLED_EXECUTION,
            )
            .unwrap()
        });
        assert_eq!(first.join().unwrap(), second.join().unwrap());
    });
    server.join().unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM pipeline_runs", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        2
    );
}

#[test]
fn stale_retrieval_failure_preserves_verified_pair_and_one_child() {
    struct WinningPeer {
        database: std::path::PathBuf,
        handoff: OcrHandoff,
        status: Status,
    }
    impl OcrTransport for WinningPeer {
        fn submit(
            &self,
            _: &LiveOcrProvider,
            _: &JobRequest,
            _: &[u8],
            _: Instant,
        ) -> Result<ReceivedStatus, TransportError> {
            panic!("completed job resubmitted")
        }
        fn status(
            &self,
            provider: &LiveOcrProvider,
            _: &str,
            deadline: Instant,
        ) -> Result<ReceivedStatus, TransportError> {
            // The other coordinator completes while this caller still owns
            // its running snapshot. Exercise the real retrieval/storage path.
            let conn = db::init_db(&self.database).unwrap();
            retrieve(
                &conn,
                &self.handoff,
                provider,
                &HttpOcrTransport,
                &UNCONTROLLED_EXECUTION,
                deadline,
            )
            .unwrap();
            assert_eq!(
                reload(&conn, &self.handoff.handoff_id).unwrap().phase,
                "output_ready"
            );
            Ok(ReceivedStatus::Streamed(self.status.clone()))
        }
    }
    let (dir, mut conn, mut provider, handoff, status, pdf, text) = fixture();
    save_status(&conn, &handoff, &status).unwrap();
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    let (pdf_desc, text_desc) = pair(&status).unwrap();
    let body = br#"{"protocol_version":3,"error":{"code":"OUTPUT_UNAVAILABLE","message":"output unavailable","retryable":false}}"#;
    let (url, worker) = server(vec![
        status_reply(&handoff, &status),
        artifact(output_path(&handoff, pdf_desc), &pdf),
        artifact(output_path(&handoff, text_desc), &text),
        Reply {
            path: output_path(&handoff, pdf_desc),
            headers: format!("HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()),
            body: body.to_vec(),
        },
    ]);
    provider.base_url = url;
    let peer = WinningPeer {
        database: dir.path().join("summarizer.db"),
        handoff: saved.clone(),
        status: status.clone(),
    };
    let result = run_handoff(
        &mut conn,
        saved,
        dir.path(),
        &provider,
        &peer,
        &UNCONTROLLED_EXECUTION,
    );
    assert_eq!(worker.join().unwrap().len(), 4);
    assert!(
        matches!(&result, Err(OcrConsumerError::InvalidOutput(message)) if message.contains("OUTPUT_UNAVAILABLE")),
        "{result:?}"
    );
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    assert_eq!(saved.phase, "output_ready");
    assert_eq!(saved.ocr_pdf_bytes.as_deref(), Some(pdf.as_slice()));
    assert_eq!(saved.text_bytes.as_deref(), Some(text.as_slice()));
    assert!(saved.error_code.is_none());
    // The HTTP server is gone. Both resumes must reuse the saved pair.
    for _ in 0..2 {
        let saved = reload(&conn, &handoff.handoff_id).unwrap();
        assert_eq!(
            run_handoff(
                &mut conn,
                saved,
                dir.path(),
                &provider,
                &HttpOcrTransport,
                &UNCONTROLLED_EXECUTION
            )
            .unwrap(),
            handoff.child_run_id
        );
    }
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
}

#[test]
fn exact_pdf_and_text_caps_transfer_over_http() {
    let mut provider = live_provider();
    provider.protocol_version = 3;
    let job = Uuid::new_v4().to_string();
    for (media_type, size) in [
        (OCR_INPUT_MEDIA_TYPE, MAX_PDF),
        (TEXT_MEDIA_TYPE, MAX_TEXT_BYTES),
    ] {
        let bytes = vec![b'x'; size];
        let descriptor = Descriptor {
            artifact_id: Uuid::new_v4().to_string(),
            media_type: media_type.into(),
            display_name: "boundary".into(),
            byte_size: size as u64,
            sha256: sha256_hex(&bytes),
        };
        let path = format!("/v3/jobs/{job}/outputs/{}", descriptor.artifact_id);
        let (url, worker) = server(vec![artifact(path, &bytes)]);
        provider.base_url = url;
        let file = download(
            &HttpOcrTransport,
            &provider,
            &job,
            &descriptor,
            Instant::now() + Duration::from_secs(10),
            &UNCONTROLLED_EXECUTION,
        )
        .unwrap();
        assert_eq!(file.metadata().unwrap().len(), size as u64);
        assert_eq!(worker.join().unwrap().len(), 1);
    }
}

#[test]
fn review_concurrent_completion_saves_preserve_first_descriptors() {
    use std::sync::{mpsc, Arc, Barrier};
    for different in [false, true] {
        let (dir, conn, _, handoff, status, _, _) = fixture();
        let mut processing = status.clone();
        processing.status = JobState::Processing;
        processing.result = None;
        save_status(&conn, &handoff, &processing).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let (sent, received) = mpsc::channel();
        let mut receiver = Some(received);
        let mut workers = Vec::new();
        for (index, changed) in [false, different].into_iter().enumerate() {
            let database = dir.path().join("summarizer.db");
            let id = handoff.handoff_id.clone();
            let barrier = barrier.clone();
            let mut answer = status.clone();
            if changed {
                answer.result.as_mut().unwrap().outputs[0].display_name = "changed.pdf".into();
            }
            // Both connections capture a pre-completion snapshot. The second
            // poll deliberately finishes after the first completion is durable.
            let sender = if index == 0 { Some(sent.clone()) } else { None };
            let wait = if index == 1 { receiver.take() } else { None };
            workers.push(thread::spawn(move || {
                let conn = db::init_db(database).unwrap();
                let snapshot = reload(&conn, &id).unwrap();
                barrier.wait();
                if let Some(wait) = wait {
                    wait.recv_timeout(Duration::from_secs(5)).unwrap();
                }
                let result = save_status(&conn, &snapshot, &answer);
                if let Some(sender) = sender {
                    sender.send(()).unwrap();
                }
                result
            }));
        }
        let first = workers.remove(0).join().unwrap();
        let second = workers.remove(0).join().unwrap();
        assert!(first.is_ok());
        assert_eq!(second.is_err(), different, "stale save: {second:?}");
        let saved = reload(&conn, &handoff.handoff_id).unwrap();
        assert_eq!(
            saved.provider_status_json,
            Some(serde_json::to_string(&status).unwrap())
        );
        assert_eq!(saved.phase, if different { "failed" } else { "running" });
        assert!(saved.ocr_pdf_bytes.is_none() && saved.text_bytes.is_none());
    }
}

struct FailingLocalFile {
    read_only_path: Option<std::path::PathBuf>,
}
impl OcrTransport for FailingLocalFile {
    fn temporary_output_file(&self) -> io::Result<fs::File> {
        match &self.read_only_path {
            Some(path) => fs::File::open(path),
            None => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "test temp directory unavailable",
            )),
        }
    }
    fn submit(
        &self,
        _: &LiveOcrProvider,
        _: &JobRequest,
        _: &[u8],
        _: Instant,
    ) -> Result<ReceivedStatus, TransportError> {
        panic!("must not rerun completed OCR")
    }
    fn status(
        &self,
        provider: &LiveOcrProvider,
        job: &str,
        deadline: Instant,
    ) -> Result<ReceivedStatus, TransportError> {
        HttpOcrTransport.status(provider, job, deadline)
    }
}

fn local_file_failure_is_recoverable(write_failure: bool) {
    let (dir, mut conn, mut provider, handoff, status, pdf, text) = fixture();
    save_status(&conn, &handoff, &status).unwrap();
    let (pdf_desc, text_desc) = pair(&status).unwrap();
    let read_only_path = dir.path().join("read-only-output");
    fs::write(&read_only_path, []).unwrap();
    let transport = FailingLocalFile {
        read_only_path: write_failure.then_some(read_only_path),
    };
    let (url, worker) = server(vec![
        status_reply(&handoff, &status),
        artifact(output_path(&handoff, pdf_desc), &pdf),
    ]);
    provider.base_url = url;
    let result = run_handoff(
        &mut conn,
        reload_for_test(&dir),
        dir.path(),
        &provider,
        &transport,
        &UNCONTROLLED_EXECUTION,
    );
    assert_eq!(worker.join().unwrap().len(), 2);
    assert!(matches!(result, Err(OcrConsumerError::Io(_))), "{result:?}");
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    assert_eq!(saved.phase, "running");
    assert!(saved.ocr_pdf_bytes.is_none() && saved.text_bytes.is_none());
    assert_eq!(
        saved.provider_status_json,
        Some(serde_json::to_string(&status).unwrap())
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
    let (url, worker) = server(vec![
        status_reply(&handoff, &status),
        artifact(output_path(&handoff, pdf_desc), &pdf),
        artifact(output_path(&handoff, text_desc), &text),
    ]);
    provider.base_url = url;
    assert_eq!(
        run_handoff(
            &mut conn,
            saved,
            dir.path(),
            &provider,
            &HttpOcrTransport,
            &UNCONTROLLED_EXECUTION
        )
        .unwrap(),
        handoff.child_run_id
    );
    assert_eq!(worker.join().unwrap().len(), 3);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
}

#[test]
fn review_temp_creation_failure_is_recoverable() {
    local_file_failure_is_recoverable(false);
}
#[test]
fn review_temp_write_failure_is_recoverable() {
    local_file_failure_is_recoverable(true);
}

#[test]
fn late_nonterminal_reply_keeps_completion_and_retrieves_one_child() {
    for state in [JobState::Accepted, JobState::Processing, JobState::Failed] {
        let (dir, mut conn, mut provider, handoff, status, pdf, text) = fixture();
        let mut processing = status.clone();
        processing.status = JobState::Processing;
        processing.result = None;
        save_status(&conn, &handoff, &processing).unwrap();
        let stale = reload(&conn, &handoff.handoff_id).unwrap();
        save_status(&conn, &stale, &status).unwrap();
        processing.status = state;
        let failure = processing.status == JobState::Failed;
        if failure {
            processing.error = Some(
                serde_json::from_value(serde_json::json!({
                    "code":"OCR_ENGINE_FAILED", "message":"OCR engine failed", "retryable":false
                }))
                .unwrap(),
            );
        }
        let result = save_status(&conn, &stale, &processing);
        assert_eq!(result.is_err(), failure, "late poll: {result:?}");
        let saved = reload(&conn, &handoff.handoff_id).unwrap();
        assert_eq!(saved.phase, if failure { "failed" } else { "running" });
        assert_eq!(
            saved.provider_status_json,
            Some(serde_json::to_string(&status).unwrap())
        );
        if failure {
            continue;
        }
        let (pdf_desc, text_desc) = pair(&status).unwrap();
        let (url, worker) = server(vec![
            status_reply(&handoff, &status),
            artifact(output_path(&handoff, pdf_desc), &pdf),
            artifact(output_path(&handoff, text_desc), &text),
        ]);
        provider.base_url = url;
        assert_eq!(
            run_handoff(
                &mut conn,
                saved,
                dir.path(),
                &provider,
                &HttpOcrTransport,
                &UNCONTROLLED_EXECUTION
            )
            .unwrap(),
            handoff.child_run_id
        );
        assert_eq!(worker.join().unwrap().len(), 3);
        let advanced = reload(&conn, &handoff.handoff_id).unwrap();
        assert!(save_status(&conn, &stale, &processing).is_ok());
        assert_eq!(
            reload(&conn, &handoff.handoff_id).unwrap().phase,
            advanced.phase
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
                .get::<_, u32>(0))
                .unwrap(),
            1
        );
    }
}

fn output_refusal(path: String, code: &str, http: u16, retryable: bool) -> Reply {
    let body = serde_json::to_vec(&serde_json::json!({"protocol_version":3,
        "error":{"code":code,"message":"test refusal","retryable":retryable}}))
    .unwrap();
    Reply {
        path,
        headers: format!(
            "HTTP/1.1 {http} Refused\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        ),
        body,
    }
}

#[test]
fn not_ready_exhaustion_is_recoverable_and_later_retry_completes_same_job() {
    let (dir, mut conn, mut provider, handoff, status, pdf, text) = fixture();
    save_status(&conn, &handoff, &status).unwrap();
    let (pdf_desc, text_desc) = pair(&status).unwrap();
    let (url, worker) = server(vec![
        status_reply(&handoff, &status),
        output_refusal(
            output_path(&handoff, pdf_desc),
            "OUTPUT_NOT_READY",
            409,
            true,
        ),
        output_refusal(
            output_path(&handoff, pdf_desc),
            "OUTPUT_NOT_READY",
            409,
            true,
        ),
        output_refusal(
            output_path(&handoff, pdf_desc),
            "OUTPUT_NOT_READY",
            409,
            true,
        ),
    ]);
    provider.base_url = url;
    let result = run_handoff(
        &mut conn,
        reload_for_test(&dir),
        dir.path(),
        &provider,
        &HttpOcrTransport,
        &UNCONTROLLED_EXECUTION,
    );
    assert!(
        matches!(result, Err(OcrConsumerError::Transport(_))),
        "{result:?}"
    );
    assert_eq!(worker.join().unwrap().len(), 4);
    let saved = reload(&conn, &handoff.handoff_id).unwrap();
    assert_eq!(saved.phase, "running");
    assert!(saved.ocr_pdf_bytes.is_none() && saved.text_bytes.is_none());
    assert_eq!(
        saved.provider_status_json,
        Some(serde_json::to_string(&status).unwrap())
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
    let (url, worker) = server(vec![
        status_reply(&handoff, &status),
        output_refusal(
            output_path(&handoff, pdf_desc),
            "OUTPUT_NOT_READY",
            409,
            true,
        ),
        artifact(output_path(&handoff, pdf_desc), &pdf),
        artifact(output_path(&handoff, text_desc), &text),
    ]);
    provider.base_url = url;
    assert_eq!(
        run_handoff(
            &mut conn,
            saved,
            dir.path(),
            &provider,
            &HttpOcrTransport,
            &UNCONTROLLED_EXECUTION
        )
        .unwrap(),
        handoff.child_run_id
    );
    assert_eq!(worker.join().unwrap().len(), 4);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM ocr_lineage", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        1
    );
}

#[test]
fn output_refusal_policy_rejects_invalid_and_nonretryable_errors() {
    for (code, http, retryable) in [
        ("OUTPUT_NOT_READY", 409, false),
        ("OUTPUT_NOT_READY", 500, true),
        ("OUTPUT_UNAVAILABLE", 500, false),
        ("OUTPUT_NOT_FOUND", 404, false),
        ("OUTPUT_BUSY", 429, true), // Missing required Retry-After remains invalid.
    ] {
        let (dir, mut conn, mut provider, handoff, status, _, _) = fixture();
        save_status(&conn, &handoff, &status).unwrap();
        let (pdf, _) = pair(&status).unwrap();
        let (url, worker) = server(vec![
            status_reply(&handoff, &status),
            output_refusal(output_path(&handoff, pdf), code, http, retryable),
        ]);
        provider.base_url = url;
        let result = run_handoff(
            &mut conn,
            reload_for_test(&dir),
            dir.path(),
            &provider,
            &HttpOcrTransport,
            &UNCONTROLLED_EXECUTION,
        );
        assert!(
            matches!(result, Err(OcrConsumerError::InvalidOutput(_))),
            "{code}: {result:?}"
        );
        assert_eq!(worker.join().unwrap().len(), 2);
        let saved = reload(&conn, &handoff.handoff_id).unwrap();
        assert_eq!(saved.phase, "failed");
        assert!(saved.ocr_pdf_bytes.is_none() && saved.text_bytes.is_none());
    }
}
