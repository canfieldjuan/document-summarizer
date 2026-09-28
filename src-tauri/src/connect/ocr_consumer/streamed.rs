//! ADR-0010 transport. Only fully verified pairs reach the existing handoff store.
use super::*;
use serde::de::{DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::fmt;
use std::io::{Seek, SeekFrom, Write};

pub(super) const MAX_PDF: usize = 36 * 1024 * 1024;
const MAX_METADATA: usize = 64 * 1024;
const CHUNK: usize = 64 * 1024;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(all(test, unix))]
const CONTRACT_REVISION: &str = "5e74cf650df07df22d1cff60d35f678601c7cfc1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Descriptor {
    artifact_id: String,
    media_type: String,
    display_name: String,
    byte_size: u64,
    sha256: String,
}

pub(super) type Status = crate::connect::v2::JobStatus<Descriptor>;

fn invalid(message: impl Into<String>) -> TransportError {
    TransportError::Invalid(message.into())
}

// Pre-decode depth is independent of serde's recursion limit. The visitor then
// preserves duplicate-member rejection even inside objects decoded as maps.
pub(super) fn decode_metadata<T: DeserializeOwned>(
    bytes: &[u8],
    limit: usize,
) -> Result<T, TransportError> {
    if bytes.len() > limit {
        return Err(invalid("metadata exceeds its byte limit"));
    }
    let (mut depth, mut quoted, mut escaped) = (0_i32, false, false);
    for byte in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > 16 {
                        return Err(invalid("metadata nesting exceeds 16"));
                    }
                }
                b'}' | b']' => depth -= 1,
                _ => {}
            }
        }
    }
    let value: UniqueValue = serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()))?;
    if value.0.get("status").is_some()
        && ["result", "error"]
            .iter()
            .any(|key| value.0.get(key).is_some_and(Value::is_null))
    {
        return Err(invalid("status contains a null terminal member"));
    }
    serde_json::from_value(value.0).map_err(|e| invalid(e.to_string()))
}

struct UniqueValue(Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("strict JSON")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, UniqueValue>()? {
                    if values.insert(key, value.0).is_some() {
                        return Err(serde::de::Error::custom("duplicate JSON member"));
                    }
                }
                Ok(UniqueValue(Value::Object(values)))
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<UniqueValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|n| UniqueValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

pub(super) fn answer(response: Response) -> Result<Status, TransportError> {
    let status = response.status();
    let bytes = read_bounded_response(response, MAX_METADATA as u64).map_err(|e| {
        if e.kind() == io::ErrorKind::InvalidData {
            invalid(e.to_string())
        } else {
            TransportError::Uncertain(e.to_string())
        }
    })?;
    if status == StatusCode::OK || status == StatusCode::ACCEPTED {
        return decode_metadata(&bytes, MAX_METADATA);
    }
    Err(error_response(status, &bytes)?)
}

fn error_response(status: StatusCode, bytes: &[u8]) -> Result<TransportError, TransportError> {
    let envelope: ErrorEnvelope = decode_metadata(bytes, MAX_METADATA)?;
    if envelope.protocol_version != 3 || !valid_error(&envelope.error) {
        return Err(invalid("invalid v3 HTTP error"));
    }
    let policy = match envelope.error.code.as_str() {
        "OUTPUT_BUSY" => Some((StatusCode::TOO_MANY_REQUESTS, true)),
        "OUTPUT_NOT_READY" => Some((StatusCode::CONFLICT, true)),
        "OUTPUT_NOT_FOUND" | "JOB_NOT_FOUND" => Some((StatusCode::NOT_FOUND, false)),
        "OUTPUT_UNAVAILABLE" => Some((StatusCode::INTERNAL_SERVER_ERROR, false)),
        "MALFORMED_REQUEST" => Some((StatusCode::BAD_REQUEST, false)),
        "PROVIDER_BUSY" => Some((StatusCode::TOO_MANY_REQUESTS, true)),
        _ => None,
    };
    if policy.is_some_and(|expected| expected != (status, envelope.error.retryable)) {
        return Err(invalid("invalid v3 HTTP error policy"));
    }
    if status == StatusCode::UNAUTHORIZED {
        return Ok(TransportError::Unauthorized);
    }
    if status == StatusCode::NOT_FOUND && envelope.error.code == "JOB_NOT_FOUND" {
        return Ok(TransportError::NotFound);
    }
    Ok(TransportError::Refused {
        message: envelope.error.message,
        retryable: envelope.error.retryable,
    })
}

pub(super) fn canonical_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| {
        id.get_version_num() == 4
            && id.get_variant() == uuid::Variant::RFC4122
            && id.to_string() == value
    })
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn pair(status: &Status) -> Result<(&Descriptor, &Descriptor), OcrConsumerError> {
    let bad = || OcrConsumerError::InvalidOutput("invalid v3 OCR descriptors".to_string());
    let outputs = &status.result.as_ref().ok_or_else(bad)?.outputs;
    if outputs.len() != 2 {
        return Err(bad());
    }
    let pdf = outputs
        .iter()
        .find(|o| o.media_type == OCR_INPUT_MEDIA_TYPE)
        .ok_or_else(bad)?;
    let text = outputs
        .iter()
        .find(|o| o.media_type == TEXT_MEDIA_TYPE)
        .ok_or_else(bad)?;
    for (output, limit) in [(pdf, MAX_PDF), (text, MAX_TEXT_BYTES)] {
        if !canonical_uuid(&output.artifact_id)
            || !digest(&output.sha256)
            || output.byte_size == 0
            || output.byte_size > limit as u64
            || !(1..=255).contains(&output.display_name.chars().count())
            || status
                .input_artifacts
                .iter()
                .any(|i| i.artifact_id == output.artifact_id)
        {
            return Err(bad());
        }
    }
    if pdf.artifact_id == text.artifact_id {
        return Err(bad());
    }
    Ok((pdf, text))
}

fn valid_error(error: &crate::connect::contracts::JobError) -> bool {
    (1..=1000).contains(&error.message.chars().count())
        && (1..=100).contains(&error.code.len())
        && error
            .code
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}

fn validate(handoff: &OcrHandoff, status: &Status) -> Result<(), OcrConsumerError> {
    validate_status_identity(handoff, status)?;
    if status.protocol_version != 3
        || !canonical_uuid(&status.job_id)
        || !canonical_uuid(&status.provider.instance_id)
        || status
            .input_artifacts
            .iter()
            .any(|i| !canonical_uuid(&i.artifact_id) || !digest(&i.sha256))
    {
        return Err(OcrConsumerError::InvalidOutput(
            "invalid v3 OCR identity".to_string(),
        ));
    }
    if status.status == JobState::Completed {
        pair(status)?;
    }
    if let Some(error) = &status.error {
        let policy = match error.code.as_str() {
            "DOCUMENT_INVALID" => Some("The PDF is not a readable document with at least one page."),
            "INPUT_PAGE_LIMIT_EXCEEDED" => Some("The PDF has more than 100 pages."),
            "OUTPUT_TEXT_LIMIT_EXCEEDED" => Some("The recognized text is larger than this provider can return."),
            "OUTPUT_PDF_LIMIT_EXCEEDED" => Some("The reconstructed PDF is larger than this provider can return."),
            "NO_OCR_TEXT" => Some("No text was detected in the document."),
            "OUTPUT_PDF_BUDGET_EXCEEDED" => Some("The retained source PDF leaves insufficient room for the OCR layer in the output budget."),
            _ => None,
        };
        if !valid_error(error)
            || policy.is_some_and(|message| error.retryable || error.message != message)
            || [
                "PROVIDER_BUSY",
                "JOB_NOT_FOUND",
                "MALFORMED_REQUEST",
                "OUTPUT_BUSY",
                "OUTPUT_NOT_READY",
                "OUTPUT_NOT_FOUND",
                "OUTPUT_UNAVAILABLE",
            ]
            .contains(&error.code.as_str())
        {
            return Err(OcrConsumerError::InvalidOutput(
                "invalid OCR terminal error".to_string(),
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_retained_pair(handoff: &OcrHandoff) -> Result<(), OcrConsumerError> {
    validate_saved_request(handoff)?;
    let encoded = handoff.provider_status_json.as_ref().ok_or_else(|| {
        OcrConsumerError::InvalidOutput("saved OCR completion is missing".to_string())
    })?;
    let status: Status =
        decode_metadata(encoded.as_bytes(), MAX_METADATA).map_err(map_transport)?;
    validate(handoff, &status)?;
    let (pdf, text) = pair(&status)?;
    for (descriptor, id, content) in [
        (pdf, &handoff.ocr_pdf_artifact_id, &handoff.ocr_pdf_bytes),
        (text, &handoff.text_artifact_id, &handoff.text_bytes),
    ] {
        if id.as_deref() != Some(descriptor.artifact_id.as_str())
            || !content.as_ref().is_some_and(|bytes| {
                bytes.len() as u64 == descriptor.byte_size && sha256_hex(bytes) == descriptor.sha256
            })
        {
            return Err(OcrConsumerError::InvalidOutput(
                "retained OCR pair failed descriptor validation".to_string(),
            ));
        }
    }
    Ok(())
}

pub(super) fn has_saved_completion(handoff: &OcrHandoff) -> Result<bool, OcrConsumerError> {
    if saved_protocol(handoff)? != 3 {
        return Ok(false);
    }
    let Some(encoded) = &handoff.provider_status_json else {
        return Ok(false);
    };
    let status: Status =
        decode_metadata(encoded.as_bytes(), MAX_METADATA).map_err(map_transport)?;
    validate(handoff, &status)?;
    Ok(status.status == JobState::Completed)
}

pub(super) fn save_status(
    conn: &Connection,
    handoff: &OcrHandoff,
    status: &Status,
) -> Result<(), OcrConsumerError> {
    // Phase alone is not a compare-and-set for running -> running. Reload
    // and compare the durable completion under the same write lock as its save.
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
        .map_err(StoreError::from)?;
    let current = reload(&tx, &handoff.handoff_id)?;
    if current.phase != handoff.phase {
        return Ok(());
    }
    let handoff = &current;
    let checked = validate(handoff, status).and_then(|()| {
        if has_saved_completion(handoff)? {
            // Overlapping polls can deliver an older nonterminal reply after
            // completion. It cannot supersede the durable completed result.
            if matches!(status.status, JobState::Accepted | JobState::Processing) {
                return Ok(false);
            }
            let previous: Status = decode_metadata(
                handoff.provider_status_json.as_ref().unwrap().as_bytes(),
                MAX_METADATA,
            )
            .map_err(map_transport)?;
            if previous.result != status.result {
                return Err(OcrConsumerError::InvalidOutput(
                    "completed OCR descriptors changed".to_string(),
                ));
            }
        }
        Ok(true)
    });
    match checked {
        Ok(false) => return Ok(()),
        Ok(true) => {}
        Err(error) => {
            db::fail_ocr_handoff(
                &tx,
                &handoff.handoff_id,
                &handoff.phase,
                "OCR_OUTPUT_INVALID",
                &error.to_string(),
                false,
            )?;
            tx.commit().map_err(StoreError::from)?;
            return Err(error);
        }
    }
    if let Some(error) = &status.error {
        db::fail_ocr_handoff(
            &tx,
            &handoff.handoff_id,
            &handoff.phase,
            &error.code,
            &error.message,
            error.retryable,
        )?;
        tx.commit().map_err(StoreError::from)?;
        return Err(OcrConsumerError::ProviderFailed(error.message.clone()));
    }
    let encoded = serde_json::to_string(status)
        .map_err(|e| OcrConsumerError::InvalidOutput(e.to_string()))?;
    db::transition_ocr_handoff(
        &tx,
        &handoff.handoff_id,
        &handoff.phase,
        "running",
        Some(&encoded),
    )?;
    tx.commit().map_err(StoreError::from)?;
    Ok(())
}

fn refresh_provider(
    provider: &LiveOcrProvider,
    transport: &dyn OcrTransport,
) -> Result<LiveOcrProvider, OcrConsumerError> {
    let refreshed = transport.rediscover(provider)?;
    if refreshed.app_id != provider.app_id
        || refreshed.instance_id != provider.instance_id
        || refreshed.protocol_version != 3
        || validate_base_url(&refreshed.base_url).is_none()
    {
        return Err(OcrConsumerError::InvalidOutput(
            "OCR rediscovery changed provider identity".to_string(),
        ));
    }
    Ok(refreshed)
}

fn reconcile(
    handoff: &OcrHandoff,
    provider: &mut LiveOcrProvider,
    transport: &dyn OcrTransport,
    deadline: Instant,
) -> Result<Status, OcrConsumerError> {
    let mut response = transport.status(provider, &handoff.provider_job_id, deadline);
    if matches!(response, Err(TransportError::Unauthorized)) {
        *provider = refresh_provider(provider, transport)?;
        response = transport.status(provider, &handoff.provider_job_id, deadline);
    }
    let status = match response.map_err(map_transport)? {
        ReceivedStatus::Streamed(status) => status,
        _ => {
            return Err(OcrConsumerError::InvalidOutput(
                "OCR recovery changed protocol".to_string(),
            ))
        }
    };
    validate(handoff, &status)?;
    let saved: Status = decode_metadata(
        handoff.provider_status_json.as_ref().unwrap().as_bytes(),
        MAX_METADATA,
    )
    .map_err(map_transport)?;
    if status.status != JobState::Completed || status.result != saved.result {
        return Err(OcrConsumerError::InvalidOutput(
            "completed OCR descriptors changed".to_string(),
        ));
    }
    Ok(status)
}

pub(super) fn retrieve(
    conn: &Connection,
    handoff: &OcrHandoff,
    provider: &LiveOcrProvider,
    transport: &dyn OcrTransport,
    control: &dyn ExecutionControl,
    deadline: Instant,
) -> Result<(), OcrConsumerError> {
    let result = validate_saved_request(handoff)
        .and_then(|_| retrieve_verified(conn, handoff, provider, transport, control, deadline));
    cancellation_checkpoint(conn, handoff, control)?;
    if let Err(OcrConsumerError::InvalidOutput(message)) = &result {
        db::fail_ocr_handoff(
            conn,
            &handoff.handoff_id,
            &handoff.phase,
            "OCR_OUTPUT_INVALID",
            message,
            false,
        )?;
    }
    result
}

fn retrieve_verified(
    conn: &Connection,
    handoff: &OcrHandoff,
    provider: &LiveOcrProvider,
    transport: &dyn OcrTransport,
    control: &dyn ExecutionControl,
    deadline: Instant,
) -> Result<(), OcrConsumerError> {
    let mut provider = provider.clone();
    let status = reconcile(handoff, &mut provider, transport, deadline)?;
    let (pdf, text) = pair(&status)?;
    let mut verified = Vec::new();
    for descriptor in [pdf, text] {
        for attempt in 0..3 {
            cancellation_checkpoint(conn, handoff, control)?;
            let result = download(
                transport,
                &provider,
                &handoff.provider_job_id,
                descriptor,
                deadline,
                control,
            );
            match result {
                Ok(file) => {
                    verified.push(file);
                    break;
                }
                Err(TransportError::Unauthorized) if attempt < 2 => {
                    provider = refresh_provider(&provider, transport)?;
                    reconcile(handoff, &mut provider, transport, deadline)?;
                }
                Err(TransportError::Uncertain(_)) if attempt < 2 => {}
                Err(error) => return Err(map_transport(error)),
            }
            let until = (Instant::now() + Duration::from_secs(1)).min(deadline);
            while Instant::now() < until {
                cancellation_checkpoint(conn, handoff, control)?;
                thread::sleep(
                    Duration::from_millis(50).min(until.saturating_duration_since(Instant::now())),
                );
            }
        }
    }
    cancellation_checkpoint(conn, handoff, control)?;
    let mut bytes = Vec::new();
    for (mut file, descriptor) in verified.into_iter().zip([pdf, text]) {
        file.seek(SeekFrom::Start(0))?;
        let mut content = Vec::with_capacity(descriptor.byte_size as usize);
        file.take(descriptor.byte_size + 1)
            .read_to_end(&mut content)?;
        if content.len() as u64 != descriptor.byte_size || sha256_hex(&content) != descriptor.sha256
        {
            return Err(OcrConsumerError::InvalidOutput(
                "verified OCR file changed".to_string(),
            ));
        }
        bytes.push(content);
    }
    validate_pair_bytes(handoff, &bytes[0], &bytes[1])?;
    cancellation_checkpoint(conn, handoff, control)?;
    db::store_ocr_outputs(
        conn,
        &handoff.handoff_id,
        &handoff.phase,
        &OcrOutput {
            provider_status_json: handoff.provider_status_json.as_ref().unwrap(),
            pdf_artifact_id: &pdf.artifact_id,
            pdf_sha256: &pdf.sha256,
            pdf_bytes: &bytes[0],
            text_artifact_id: &text.artifact_id,
            text_sha256: &text.sha256,
            text_bytes: &bytes[1],
        },
    )?;
    Ok(())
}

fn http_failure(error: reqwest::Error) -> TransportError {
    use std::error::Error;
    let mut cause = error.source();
    while let Some(current) = cause {
        if current
            .downcast_ref::<hyper::Error>()
            .is_some_and(hyper::Error::is_parse)
        {
            return invalid("invalid OCR output HTTP framing");
        }
        cause = current.source();
    }
    TransportError::Uncertain(error.to_string())
}

fn download(
    transport: &dyn OcrTransport,
    provider: &LiveOcrProvider,
    job: &str,
    descriptor: &Descriptor,
    parent_deadline: Instant,
    control: &dyn ExecutionControl,
) -> Result<std::fs::File, TransportError> {
    let deadline = (Instant::now() + TRANSFER_TIMEOUT).min(parent_deadline);
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| TransportError::Uncertain("retrieval deadline expired".to_string()))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(download_async(
        transport, provider, job, descriptor, control, deadline, remaining,
    ))
}

// Retain the in-flight future across short checks. Cancellation drops the
// future and closes the body rather than waiting for the transfer deadline.
async fn cancellable<F: std::future::Future>(
    future: F,
    deadline: Instant,
    control: &dyn ExecutionControl,
) -> Result<F::Output, TransportError> {
    tokio::pin!(future);
    loop {
        if control.cancellation_requested() {
            return Err(TransportError::Uncertain(
                "OCR retrieval cancelled".to_string(),
            ));
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| TransportError::Uncertain("retrieval deadline expired".to_string()))?;
        if let Ok(result) =
            tokio::time::timeout(remaining.min(Duration::from_millis(50)), &mut future).await
        {
            return Ok(result);
        }
    }
}

async fn download_async(
    transport: &dyn OcrTransport,
    provider: &LiveOcrProvider,
    job: &str,
    descriptor: &Descriptor,
    control: &dyn ExecutionControl,
    deadline: Instant,
    remaining: Duration,
) -> Result<std::fs::File, TransportError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .connect_timeout(DISCOVERY_TIMEOUT.min(remaining))
        .timeout(remaining)
        .build()
        .map_err(|e| TransportError::Uncertain(e.to_string()))?;
    let mut response = cancellable(
        client
            .get(endpoint(
                &provider.base_url,
                &format!("v3/jobs/{job}/outputs/{}", descriptor.artifact_id),
            )?)
            .header("Accept-Encoding", "identity")
            .bearer_auth(&provider.token)
            .send(),
        deadline,
        control,
    )
    .await?
    .map_err(http_failure)?;
    if response.status() != StatusCode::OK {
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            == Some("1");
        let mut bytes = Vec::new();
        while let Some(chunk) = cancellable(response.chunk(), deadline, control)
            .await?
            .map_err(http_failure)?
        {
            if chunk.len() > MAX_METADATA - bytes.len() {
                return Err(invalid("output error exceeds metadata limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let error: ErrorEnvelope = decode_metadata(&bytes, MAX_METADATA)?;
        let refusal = error_response(status, &bytes)?;
        if error.error.code == "OUTPUT_BUSY" && !retry_after {
            return Err(invalid("OUTPUT_BUSY requires Retry-After: 1"));
        }
        return Err(match refusal {
            TransportError::Unauthorized => TransportError::Unauthorized,
            TransportError::Refused {
                message,
                retryable: true,
            } => TransportError::Uncertain(message),
            _ => invalid(format!(
                "OCR output retrieval refused: {}",
                error.error.code
            )),
        });
    }
    let headers = response.headers();
    let exactly = |key: &str, expected: &str| {
        let values: Vec<_> = headers.get_all(key).iter().collect();
        values.len() == 1 && values[0].to_str().ok() == Some(expected)
    };
    let lengths: Vec<_> = headers.get_all("content-length").iter().collect();
    let length = lengths.first().and_then(|v| v.to_str().ok());
    if lengths.len() != 1
        || !length.is_some_and(|s| {
            !s.is_empty()
                && s.bytes().all(|b| b.is_ascii_digit())
                && s.parse::<u64>().ok() == Some(descriptor.byte_size)
        })
        || !exactly("content-type", "application/octet-stream")
        || !exactly("cache-control", "no-store")
        || [
            "content-encoding",
            "transfer-encoding",
            "location",
            "content-disposition",
        ]
        .iter()
        .any(|key| headers.contains_key(*key))
    {
        return Err(invalid("invalid OCR output HTTP framing"));
    }
    let mut file = transport.temporary_output_file()?;
    let (mut count, mut hash) = (0_u64, Sha256::new());
    while let Some(bytes) = cancellable(response.chunk(), deadline, control)
        .await?
        .map_err(http_failure)?
    {
        for chunk in bytes.chunks(CHUNK) {
            count += chunk.len() as u64;
            if count > descriptor.byte_size {
                return Err(invalid("OCR output exceeded declared length"));
            }
            hash.update(chunk);
            file.write_all(chunk)?;
        }
    }
    if count != descriptor.byte_size || format!("{:x}", hash.finalize()) != descriptor.sha256 {
        return Err(invalid("OCR output length or digest mismatch"));
    }
    Ok(file)
}

#[cfg(all(test, unix))]
mod tests;
