use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Take};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

pub const FEATURE_ID: &str = "connect.capability_exchange";
pub const ENTITLEMENT_FILE_NAME: &str = "entitlement-v1.json";
const FORMAT_VERSION: u32 = 1;
const MAX_ENTITLEMENT_BYTES: u64 = 16 * 1024;
const MAX_PAYLOAD_BASE64URL_CHARS: usize = 8192;
const MAX_FEATURES: usize = 32;
const MAX_KEYS: usize = 16;
const MAX_KEY_ID_CHARS: usize = 100;
const MAX_SUBJECT_CHARS: usize = 200;
const PUBLIC_KEY_BYTES: usize = 32;
const SIGNATURE_BYTES: usize = 64;
const COMPILED_KEYRING: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/connect-entitlement-keyring.json"
));

type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitlementDecision {
    Active,
    AuthorityUnavailable,
    Missing,
    Invalid,
    NotYetValid,
    Expired,
    FeatureMissing,
}

impl EntitlementDecision {
    pub fn is_active(self) -> bool {
        self == Self::Active
    }
}

#[derive(Debug, Error)]
pub enum EntitlementConfigurationError {
    #[error("the embedded Connect entitlement key ring is invalid")]
    InvalidKeyring,
}

#[derive(Clone)]
pub struct EntitlementGate {
    path: Option<PathBuf>,
    keys: Arc<BTreeMap<String, Vec<u8>>>,
    clock: Clock,
    #[cfg(test)]
    forced: Option<EntitlementDecision>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Keyring {
    keys: Vec<TrustedKey>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedKey {
    key_id: String,
    algorithm: String,
    public_key_base64url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntitlementEnvelope {
    format_version: u32,
    key_id: String,
    payload_base64url: String,
    signature_base64url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntitlementClaims {
    format_version: u32,
    entitlement_id: String,
    subject: String,
    features: Vec<String>,
    issued_at: String,
    not_before: String,
    expires_at: String,
}

impl EntitlementGate {
    pub fn from_installation() -> Result<Self, EntitlementConfigurationError> {
        let keys = parse_keyring(COMPILED_KEYRING)?;
        Ok(Self {
            path: entitlement_path(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME")),
            keys: Arc::new(keys),
            clock: Arc::new(Utc::now),
            #[cfg(test)]
            forced: None,
        })
    }

    pub fn decision(&self) -> EntitlementDecision {
        #[cfg(test)]
        if let Some(decision) = self.forced {
            return decision;
        }
        if self.keys.is_empty() {
            return EntitlementDecision::AuthorityUnavailable;
        }
        let Some(path) = &self.path else {
            return EntitlementDecision::Missing;
        };
        let Some(bytes) = read_private_entitlement(path) else {
            return EntitlementDecision::Missing;
        };
        evaluate_entitlement(&bytes, &self.keys, (self.clock)())
    }

    #[cfg(test)]
    pub(crate) fn always_active_for_test() -> Self {
        Self {
            path: None,
            keys: Arc::new(BTreeMap::new()),
            clock: Arc::new(Utc::now),
            forced: Some(EntitlementDecision::Active),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        path: PathBuf,
        keys: BTreeMap<String, Vec<u8>>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            path: Some(path),
            keys: Arc::new(keys),
            clock: Arc::new(move || now),
            forced: None,
        }
    }
}

fn entitlement_path(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let root = match xdg_config_home {
        Some(value) => PathBuf::from(value),
        None => PathBuf::from(home?).join(".config"),
    };
    root.is_absolute()
        .then(|| root.join("local-connect").join(ENTITLEMENT_FILE_NAME))
}

fn parse_keyring(value: &str) -> Result<BTreeMap<String, Vec<u8>>, EntitlementConfigurationError> {
    let parsed: Keyring =
        serde_json::from_str(value).map_err(|_| EntitlementConfigurationError::InvalidKeyring)?;
    if parsed.keys.len() > MAX_KEYS {
        return Err(EntitlementConfigurationError::InvalidKeyring);
    }
    let mut keys = BTreeMap::new();
    for key in parsed.keys {
        if key.algorithm != "Ed25519" || !valid_key_id(&key.key_id) {
            return Err(EntitlementConfigurationError::InvalidKeyring);
        }
        let public_key = decode_base64url(&key.public_key_base64url, PUBLIC_KEY_BYTES)
            .filter(|bytes| bytes.len() == PUBLIC_KEY_BYTES)
            .ok_or(EntitlementConfigurationError::InvalidKeyring)?;
        if keys.insert(key.key_id, public_key).is_some() {
            return Err(EntitlementConfigurationError::InvalidKeyring);
        }
    }
    Ok(keys)
}

fn evaluate_entitlement(
    bytes: &[u8],
    keys: &BTreeMap<String, Vec<u8>>,
    now: DateTime<Utc>,
) -> EntitlementDecision {
    if bytes.is_empty() || bytes.len() as u64 > MAX_ENTITLEMENT_BYTES {
        return EntitlementDecision::Invalid;
    }
    let Ok(envelope) = serde_json::from_slice::<EntitlementEnvelope>(bytes) else {
        return EntitlementDecision::Invalid;
    };
    if envelope.format_version != FORMAT_VERSION
        || !valid_key_id(&envelope.key_id)
        || envelope.payload_base64url.len() > MAX_PAYLOAD_BASE64URL_CHARS
    {
        return EntitlementDecision::Invalid;
    }
    let Some(public_key) = keys.get(&envelope.key_id) else {
        return EntitlementDecision::Invalid;
    };
    let Some(payload) = decode_base64url(
        &envelope.payload_base64url,
        MAX_PAYLOAD_BASE64URL_CHARS * 3 / 4,
    ) else {
        return EntitlementDecision::Invalid;
    };
    let Some(signature) = decode_base64url(&envelope.signature_base64url, SIGNATURE_BYTES)
        .filter(|value| value.len() == SIGNATURE_BYTES)
    else {
        return EntitlementDecision::Invalid;
    };
    if UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&payload, &signature)
        .is_err()
    {
        return EntitlementDecision::Invalid;
    }
    let Ok(claims) = serde_json::from_slice::<EntitlementClaims>(&payload) else {
        return EntitlementDecision::Invalid;
    };
    let valid_entitlement_id = Uuid::parse_str(&claims.entitlement_id)
        .ok()
        .filter(|value| value.get_version_num() == 4 && value.to_string() == claims.entitlement_id)
        .is_some();
    if claims.format_version != FORMAT_VERSION
        || !valid_entitlement_id
        || claims.subject.is_empty()
        || claims.subject.chars().count() > MAX_SUBJECT_CHARS
        || claims.features.is_empty()
        || claims.features.len() > MAX_FEATURES
        || claims
            .features
            .iter()
            .any(|feature| !valid_feature_id(feature))
        || claims.features.iter().collect::<BTreeSet<_>>().len() != claims.features.len()
    {
        return EntitlementDecision::Invalid;
    }
    let Some(issued_at) = parse_utc(&claims.issued_at) else {
        return EntitlementDecision::Invalid;
    };
    let Some(not_before) = parse_utc(&claims.not_before) else {
        return EntitlementDecision::Invalid;
    };
    let Some(expires_at) = parse_utc(&claims.expires_at) else {
        return EntitlementDecision::Invalid;
    };
    if issued_at > not_before || not_before >= expires_at {
        return EntitlementDecision::Invalid;
    }
    if now < not_before {
        return EntitlementDecision::NotYetValid;
    }
    if now >= expires_at {
        return EntitlementDecision::Expired;
    }
    if !claims.features.iter().any(|feature| feature == FEATURE_ID) {
        return EntitlementDecision::FeatureMissing;
    }
    EntitlementDecision::Active
}

fn parse_utc(value: &str) -> Option<DateTime<Utc>> {
    if !value.ends_with('Z') {
        return None;
    }
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn valid_key_id(value: &str) -> bool {
    valid_identifier(value, MAX_KEY_ID_CHARS, false)
}

fn valid_feature_id(value: &str) -> bool {
    valid_identifier(value, MAX_KEY_ID_CHARS, true)
}

fn valid_identifier(value: &str, max_chars: usize, allow_underscore: bool) -> bool {
    if value.is_empty() || value.len() > max_chars || !value.is_ascii() {
        return false;
    }
    let mut segment_has_character = false;
    for byte in value.bytes() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            segment_has_character = true;
        } else if (matches!(byte, b'.' | b'-') || (allow_underscore && byte == b'_'))
            && segment_has_character
        {
            segment_has_character = false;
        } else {
            return false;
        }
    }
    segment_has_character
}

fn decode_base64url(value: &str, max_decoded_bytes: usize) -> Option<Vec<u8>> {
    if value.is_empty() || value.contains('=') {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(value).ok()?;
    if decoded.len() > max_decoded_bytes || URL_SAFE_NO_PAD.encode(&decoded) != value {
        return None;
    }
    Some(decoded)
}

#[cfg(unix)]
fn read_private_entitlement(path: &Path) -> Option<Vec<u8>> {
    let parent = path.parent()?;
    let directory = fs::symlink_metadata(parent).ok()?;
    let current_uid = unsafe { libc::geteuid() };
    if !directory.is_dir()
        || directory.file_type().is_symlink()
        || directory.uid() != current_uid
        || directory.mode() & 0o077 != 0
    {
        return None;
    }
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != current_uid
        || metadata.mode() & 0o077 != 0
        || metadata.len() == 0
        || metadata.len() > MAX_ENTITLEMENT_BYTES
    {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .ok()?;
    let opened = file.metadata().ok()?;
    if !opened.is_file()
        || opened.uid() != current_uid
        || opened.mode() & 0o077 != 0
        || opened.len() == 0
        || opened.len() > MAX_ENTITLEMENT_BYTES
    {
        return None;
    }
    read_bounded(file.take(MAX_ENTITLEMENT_BYTES + 1))
}

#[cfg(not(unix))]
fn read_private_entitlement(_path: &Path) -> Option<Vec<u8>> {
    None
}

fn read_bounded(mut reader: Take<File>) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_ENTITLEMENT_BYTES).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use serde_json::{json, Value};
    use std::process::Command;

    #[cfg(unix)]
    use std::os::unix::fs::{symlink, PermissionsExt};

    const CONTRACTS_REVISION: &str = "3851b4c55901ef18470c63b92a99a8348e2f1459";

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = env::temp_dir().join(format!("doc-sum-entitlement-{}", Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            #[cfg(unix)]
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn signing_key() -> Ed25519KeyPair {
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap()
    }

    fn claims(not_before: &str, expires_at: &str, features: Vec<&str>) -> Value {
        json!({
            "format_version": 1,
            "entitlement_id": Uuid::new_v4().to_string(),
            "subject": "test-customer",
            "features": features,
            "issued_at": not_before,
            "not_before": not_before,
            "expires_at": expires_at,
        })
    }

    fn signed_entitlement(key: &Ed25519KeyPair, key_id: &str, claims: &Value) -> Vec<u8> {
        let payload = serde_json::to_vec(claims).unwrap();
        signed_payload(key, key_id, &payload)
    }

    fn signed_payload(key: &Ed25519KeyPair, key_id: &str, payload: &[u8]) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "format_version": 1,
            "key_id": key_id,
            "payload_base64url": URL_SAFE_NO_PAD.encode(payload),
            "signature_base64url": URL_SAFE_NO_PAD.encode(key.sign(payload).as_ref()),
        }))
        .unwrap()
    }

    fn gate(root: &TestDirectory, key: &Ed25519KeyPair, now: &str) -> EntitlementGate {
        let mut keys = BTreeMap::new();
        keys.insert("test-key".to_string(), key.public_key().as_ref().to_vec());
        EntitlementGate::for_test(
            root.0.join(ENTITLEMENT_FILE_NAME),
            keys,
            DateTime::parse_from_rfc3339(now)
                .unwrap()
                .with_timezone(&Utc),
        )
    }

    fn write_private(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn signed_entitlement_enforces_signature_feature_and_exact_time_boundaries() {
        let root = TestDirectory::new();
        let key = signing_key();
        let license = signed_entitlement(
            &key,
            "test-key",
            &claims(
                "2026-01-01T00:00:00Z",
                "2027-01-01T00:00:00Z",
                vec![FEATURE_ID],
            ),
        );
        write_private(&root.0.join(ENTITLEMENT_FILE_NAME), &license);
        assert_eq!(
            gate(&root, &key, "2026-01-01T00:00:00Z").decision(),
            EntitlementDecision::Active
        );
        assert_eq!(
            gate(&root, &key, "2027-01-01T00:00:00Z").decision(),
            EntitlementDecision::Expired
        );
        assert_eq!(
            gate(&root, &key, "2025-12-31T23:59:59Z").decision(),
            EntitlementDecision::NotYetValid
        );

        let missing_feature = signed_entitlement(
            &key,
            "test-key",
            &claims(
                "2026-01-01T00:00:00Z",
                "2027-01-01T00:00:00Z",
                vec!["document.local_processing"],
            ),
        );
        write_private(&root.0.join(ENTITLEMENT_FILE_NAME), &missing_feature);
        assert_eq!(
            gate(&root, &key, "2026-08-31T00:00:00Z").decision(),
            EntitlementDecision::FeatureMissing
        );

        let mut uppercase_id = claims(
            "2026-01-01T00:00:00Z",
            "2027-01-01T00:00:00Z",
            vec![FEATURE_ID],
        );
        uppercase_id["entitlement_id"] = Value::String(
            uppercase_id["entitlement_id"]
                .as_str()
                .unwrap()
                .to_ascii_uppercase(),
        );
        write_private(
            &root.0.join(ENTITLEMENT_FILE_NAME),
            &signed_entitlement(&key, "test-key", &uppercase_id),
        );
        assert_eq!(
            gate(&root, &key, "2026-08-31T00:00:00Z").decision(),
            EntitlementDecision::Invalid
        );

        let mut tampered: Value = serde_json::from_slice(&license).unwrap();
        tampered["payload_base64url"] = Value::String(URL_SAFE_NO_PAD.encode(b"{}"));
        write_private(
            &root.0.join(ENTITLEMENT_FILE_NAME),
            &serde_json::to_vec(&tampered).unwrap(),
        );
        assert_eq!(
            gate(&root, &key, "2026-08-31T00:00:00Z").decision(),
            EntitlementDecision::Invalid
        );
    }

    #[test]
    fn entitlement_file_must_be_owner_private_regular_and_nonsymlinked() {
        let root = TestDirectory::new();
        let key = signing_key();
        let path = root.0.join(ENTITLEMENT_FILE_NAME);
        assert_eq!(
            gate(&root, &key, "2026-08-31T00:00:00Z").decision(),
            EntitlementDecision::Missing
        );
        let license = signed_entitlement(
            &key,
            "test-key",
            &claims(
                "2026-01-01T00:00:00Z",
                "2027-01-01T00:00:00Z",
                vec![FEATURE_ID],
            ),
        );
        write_private(&path, &license);
        #[cfg(unix)]
        {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
            assert_eq!(
                gate(&root, &key, "2026-08-31T00:00:00Z").decision(),
                EntitlementDecision::Missing
            );
            fs::remove_file(&path).unwrap();
            let target = root.0.join("target.json");
            write_private(&target, &license);
            symlink(&target, &path).unwrap();
            assert_eq!(
                gate(&root, &key, "2026-08-31T00:00:00Z").decision(),
                EntitlementDecision::Missing
            );
        }
    }

    #[test]
    fn duplicate_claim_members_are_rejected_before_authorization() {
        let root = TestDirectory::new();
        let key = signing_key();
        let payload = format!(
            r#"{{"format_version":1,"entitlement_id":"{}","subject":"test-customer","features":["document.local_processing"],"features":["{}"],"issued_at":"2026-01-01T00:00:00Z","not_before":"2026-01-01T00:00:00Z","expires_at":"2027-01-01T00:00:00Z"}}"#,
            Uuid::new_v4(),
            FEATURE_ID
        );
        write_private(
            &root.0.join(ENTITLEMENT_FILE_NAME),
            &signed_payload(&key, "test-key", payload.as_bytes()),
        );

        assert_eq!(
            gate(&root, &key, "2026-08-31T00:00:00Z").decision(),
            EntitlementDecision::Invalid
        );
    }

    #[test]
    fn keyring_and_path_boundaries_fail_closed() {
        assert!(parse_keyring(r#"{"keys":[]}"#).unwrap().is_empty());
        assert!(parse_keyring(
            r#"{"keys":[{"key_id":"duplicate","algorithm":"Ed25519","public_key_base64url":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"},{"key_id":"duplicate","algorithm":"Ed25519","public_key_base64url":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}]}"#
        )
        .is_err());
        assert!(parse_keyring(
            r#"{"keys":[{"key_id":"bad_key","algorithm":"Ed25519","public_key_base64url":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}]}"#
        )
        .is_err());
        assert!(parse_keyring(
            r#"{"keys":[{"key_id":"good-key.v1","algorithm":"Ed25519","public_key_base64url":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}]}"#
        )
        .is_ok());
        assert!(valid_feature_id("document.local_processing"));
        assert_eq!(
            entitlement_path(Some(OsString::from("relative")), None),
            None
        );
        assert_eq!(
            entitlement_path(None, Some(OsString::from("relative"))),
            None
        );
        assert_eq!(
            entitlement_path(Some(OsString::from("/config")), None),
            Some(PathBuf::from("/config/local-connect/entitlement-v1.json"))
        );
    }

    fn canonical_fixture(relative_path: &str) -> Value {
        let repository = PathBuf::from(
            env::var_os("CONNECT_CONTRACTS_DIR")
                .expect("CONNECT_CONTRACTS_DIR must name a connect-contracts Git checkout"),
        );
        let revision_path = format!("{CONTRACTS_REVISION}:entitlements/v1/{relative_path}");
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .arg("show")
            .arg(&revision_path)
            .output()
            .expect("git must be available for canonical entitlement conformance");
        assert!(
            output.status.success(),
            "canonical entitlement fixture unavailable at {revision_path}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        serde_json::from_slice(&output.stdout).expect("canonical entitlement fixture must be JSON")
    }

    #[test]
    #[ignore = "requires CONNECT_CONTRACTS_DIR"]
    fn canonical_entitlement_v1_fixtures() {
        let keyring = parse_keyring(&canonical_fixture("fixtures/test-keyring.json").to_string())
            .expect("canonical test key ring must be valid");
        let index = canonical_fixture("fixtures/index.json");
        let now = parse_utc(index["evaluated_at"].as_str().unwrap()).unwrap();
        for case in index["cases"].as_array().unwrap() {
            let fixture =
                canonical_fixture(&format!("fixtures/{}", case["fixture"].as_str().unwrap()));
            let decision =
                evaluate_entitlement(&serde_json::to_vec(&fixture).unwrap(), &keyring, now);
            assert_eq!(
                decision.is_active(),
                case["entitled"].as_bool().unwrap(),
                "entitlement decision diverged for {}",
                case["fixture"].as_str().unwrap()
            );
        }
    }
}
