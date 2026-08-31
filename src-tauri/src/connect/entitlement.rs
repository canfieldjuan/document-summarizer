use crate::connect::contracts::valid_uuid_v4;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Take, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;

#[cfg(test)]
use uuid::Uuid;

#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};

pub const FEATURE_ID: &str = "connect.capability_exchange";
pub const ENTITLEMENT_FILE_NAME: &str = "entitlement-v1.json";
pub const ENTITLEMENT_LOCK_FILE_NAME: &str = ".entitlement-v1.lock";
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
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntitlementDecision {
    Active,
    AuthorityUnavailable,
    Missing,
    Invalid,
    NotYetValid,
    Expired,
    FeatureMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementStatus {
    pub state: EntitlementDecision,
    pub active: bool,
}

impl From<EntitlementDecision> for EntitlementStatus {
    fn from(state: EntitlementDecision) -> Self {
        Self {
            active: state.is_active(),
            state,
        }
    }
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EntitlementInstallError {
    #[error("this build has no trusted Connect entitlement authority")]
    AuthorityUnavailable,
    #[error("the selected Connect entitlement is not a safe, valid license file")]
    SourceInvalid,
    #[error("the selected Connect entitlement is not currently active")]
    NotActive,
    #[error("the private Connect entitlement directory is unavailable or unsafe")]
    StorageUnavailable,
    #[error("another Connect entitlement activation is already in progress")]
    ActivationBusy,
    #[error("the Connect entitlement could not be installed safely")]
    InstallFailed,
}

impl EntitlementInstallError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::AuthorityUnavailable => "CONNECT_ENTITLEMENT_AUTHORITY_UNAVAILABLE",
            Self::SourceInvalid => "CONNECT_ENTITLEMENT_SOURCE_INVALID",
            Self::NotActive => "CONNECT_ENTITLEMENT_NOT_ACTIVE",
            Self::StorageUnavailable => "CONNECT_ENTITLEMENT_STORAGE_UNAVAILABLE",
            Self::ActivationBusy => "CONNECT_ENTITLEMENT_ACTIVATION_BUSY",
            Self::InstallFailed => "CONNECT_ENTITLEMENT_INSTALL_FAILED",
        }
    }
}

#[derive(Clone)]
pub struct EntitlementGate {
    path: Option<PathBuf>,
    keys: Arc<BTreeMap<String, Vec<u8>>>,
    clock: Clock,
    #[cfg(test)]
    forced: Option<EntitlementDecision>,
    #[cfg(test)]
    fail_before_replace: bool,
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
            #[cfg(test)]
            fail_before_replace: false,
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

    pub fn status(&self) -> EntitlementStatus {
        self.decision().into()
    }

    pub fn install(&self, source: &Path) -> Result<EntitlementStatus, EntitlementInstallError> {
        if self.keys.is_empty() {
            return Err(EntitlementInstallError::AuthorityUnavailable);
        }
        let Some(destination) = &self.path else {
            return Err(EntitlementInstallError::StorageUnavailable);
        };

        #[cfg(unix)]
        {
            let candidate = read_candidate_entitlement(source)?;
            require_active_candidate(&candidate, &self.keys, (self.clock)())?;
            let parent = destination
                .parent()
                .ok_or(EntitlementInstallError::StorageUnavailable)?;
            ensure_private_directory(parent)?;
            let _lock = acquire_activation_lock(parent)?;
            validate_existing_destination(destination)?;
            require_active_candidate(&candidate, &self.keys, (self.clock)())?;
            install_candidate(self, destination, &candidate)?;
            let status = self.status();
            if !status.active {
                return Err(EntitlementInstallError::InstallFailed);
            }
            Ok(status)
        }

        #[cfg(not(unix))]
        {
            let _ = source;
            Err(EntitlementInstallError::StorageUnavailable)
        }
    }

    #[cfg(test)]
    pub(crate) fn always_active_for_test() -> Self {
        Self {
            path: None,
            keys: Arc::new(BTreeMap::new()),
            clock: Arc::new(Utc::now),
            forced: Some(EntitlementDecision::Active),
            fail_before_replace: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        path: PathBuf,
        keys: BTreeMap<String, Vec<u8>>,
        now: DateTime<Utc>,
    ) -> Self {
        Self::for_test_with_clock(path, keys, move || now)
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_clock<F>(
        path: PathBuf,
        keys: BTreeMap<String, Vec<u8>>,
        clock: F,
    ) -> Self
    where
        F: Fn() -> DateTime<Utc> + Send + Sync + 'static,
    {
        Self {
            path: Some(path),
            keys: Arc::new(keys),
            clock: Arc::new(clock),
            forced: None,
            fail_before_replace: false,
        }
    }
}

fn require_active_candidate(
    bytes: &[u8],
    keys: &BTreeMap<String, Vec<u8>>,
    now: DateTime<Utc>,
) -> Result<(), EntitlementInstallError> {
    match evaluate_entitlement(bytes, keys, now) {
        EntitlementDecision::Active => Ok(()),
        EntitlementDecision::NotYetValid
        | EntitlementDecision::Expired
        | EntitlementDecision::FeatureMissing => Err(EntitlementInstallError::NotActive),
        EntitlementDecision::Invalid | EntitlementDecision::Missing => {
            Err(EntitlementInstallError::SourceInvalid)
        }
        EntitlementDecision::AuthorityUnavailable => {
            Err(EntitlementInstallError::AuthorityUnavailable)
        }
    }
}

fn entitlement_path(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let root = match xdg_config_home.filter(|value| !value.is_empty()) {
        Some(value) => PathBuf::from(value),
        None => PathBuf::from(home.filter(|value| !value.is_empty())?).join(".config"),
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
    if claims.format_version != FORMAT_VERSION
        || !valid_uuid_v4(&claims.entitlement_id)
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
fn read_candidate_entitlement(path: &Path) -> Result<Vec<u8>, EntitlementInstallError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| EntitlementInstallError::SourceInvalid)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_ENTITLEMENT_BYTES
    {
        return Err(EntitlementInstallError::SourceInvalid);
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| EntitlementInstallError::SourceInvalid)?;
    let opened = file
        .metadata()
        .map_err(|_| EntitlementInstallError::SourceInvalid)?;
    if !opened.is_file()
        || opened.len() == 0
        || opened.len() > MAX_ENTITLEMENT_BYTES
        || opened.dev() != metadata.dev()
        || opened.ino() != metadata.ino()
    {
        return Err(EntitlementInstallError::SourceInvalid);
    }
    read_bounded(file.take(MAX_ENTITLEMENT_BYTES + 1))
        .filter(|bytes| !bytes.is_empty())
        .ok_or(EntitlementInstallError::SourceInvalid)
}

#[cfg(unix)]
fn ensure_private_directory(path: &Path) -> Result<(), EntitlementInstallError> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            builder
                .create(path)
                .map_err(|_| EntitlementInstallError::StorageUnavailable)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|_| EntitlementInstallError::StorageUnavailable)?;
        }
        Err(_) => return Err(EntitlementInstallError::StorageUnavailable),
    }

    let metadata =
        fs::symlink_metadata(path).map_err(|_| EntitlementInstallError::StorageUnavailable)?;
    let current_uid = unsafe { libc::geteuid() };
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != current_uid
        || metadata.mode() & 0o077 != 0
    {
        return Err(EntitlementInstallError::StorageUnavailable);
    }
    Ok(())
}

#[cfg(unix)]
fn validate_existing_destination(path: &Path) -> Result<(), EntitlementInstallError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(EntitlementInstallError::StorageUnavailable),
    };
    let current_uid = unsafe { libc::geteuid() };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != current_uid
        || metadata.mode() & 0o077 != 0
    {
        return Err(EntitlementInstallError::StorageUnavailable);
    }
    Ok(())
}

#[cfg(unix)]
struct ActivationLock {
    _file: File,
}

#[cfg(unix)]
fn acquire_activation_lock(parent: &Path) -> Result<ActivationLock, EntitlementInstallError> {
    let path = parent.join(ENTITLEMENT_LOCK_FILE_NAME);
    validate_lock_path(&path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|_| EntitlementInstallError::StorageUnavailable)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| EntitlementInstallError::StorageUnavailable)?;
    let metadata = file
        .metadata()
        .map_err(|_| EntitlementInstallError::StorageUnavailable)?;
    let current_uid = unsafe { libc::geteuid() };
    if !metadata.is_file() || metadata.uid() != current_uid || metadata.mode() & 0o077 != 0 {
        return Err(EntitlementInstallError::StorageUnavailable);
    }
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        let code = error.raw_os_error();
        return if code == Some(libc::EAGAIN) || code == Some(libc::EWOULDBLOCK) {
            Err(EntitlementInstallError::ActivationBusy)
        } else {
            Err(EntitlementInstallError::StorageUnavailable)
        };
    }
    Ok(ActivationLock { _file: file })
}

#[cfg(unix)]
fn validate_lock_path(path: &Path) -> Result<(), EntitlementInstallError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(EntitlementInstallError::StorageUnavailable),
    };
    let current_uid = unsafe { libc::geteuid() };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != current_uid
        || metadata.mode() & 0o077 != 0
    {
        return Err(EntitlementInstallError::StorageUnavailable);
    }
    Ok(())
}

#[cfg(unix)]
fn install_candidate(
    _gate: &EntitlementGate,
    destination: &Path,
    candidate: &[u8],
) -> Result<(), EntitlementInstallError> {
    let parent = destination
        .parent()
        .ok_or(EntitlementInstallError::StorageUnavailable)?;
    let (mut temporary, temporary_path) = create_temporary_entitlement(parent)?;
    let mut replaced = false;
    let result = (|| {
        temporary
            .write_all(candidate)
            .map_err(|_| EntitlementInstallError::InstallFailed)?;
        temporary
            .sync_all()
            .map_err(|_| EntitlementInstallError::InstallFailed)?;
        #[cfg(test)]
        if _gate.fail_before_replace {
            return Err(EntitlementInstallError::InstallFailed);
        }
        fs::rename(&temporary_path, destination)
            .map_err(|_| EntitlementInstallError::InstallFailed)?;
        replaced = true;
        sync_directory(parent)?;
        Ok(())
    })();
    drop(temporary);
    if !replaced {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

#[cfg(unix)]
fn create_temporary_entitlement(parent: &Path) -> Result<(File, PathBuf), EntitlementInstallError> {
    for _ in 0..64 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".{ENTITLEMENT_FILE_NAME}.tmp.{}.{}",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&path)
        {
            Ok(file) => {
                file.set_permissions(fs::Permissions::from_mode(0o600))
                    .map_err(|_| EntitlementInstallError::InstallFailed)?;
                let metadata = file
                    .metadata()
                    .map_err(|_| EntitlementInstallError::InstallFailed)?;
                if !metadata.is_file()
                    || metadata.uid() != unsafe { libc::geteuid() }
                    || metadata.mode() & 0o777 != 0o600
                {
                    let _ = fs::remove_file(&path);
                    return Err(EntitlementInstallError::InstallFailed);
                }
                return Ok((file, path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(EntitlementInstallError::InstallFailed),
        }
    }
    Err(EntitlementInstallError::InstallFailed)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), EntitlementInstallError> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(path)
        .map_err(|_| EntitlementInstallError::StorageUnavailable)?;
    directory
        .sync_all()
        .map_err(|_| EntitlementInstallError::StorageUnavailable)
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

    const CONTRACTS_REVISION: &str = "c5405935bd1354cf6a4c8539425a53dfd7f52949";

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

    fn active_license(key: &Ed25519KeyPair) -> Vec<u8> {
        signed_entitlement(
            key,
            "test-key",
            &claims(
                "2026-01-01T00:00:00Z",
                "2027-01-01T00:00:00Z",
                vec![FEATURE_ID],
            ),
        )
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

        let mut invalid_variant = claims(
            "2026-01-01T00:00:00Z",
            "2027-01-01T00:00:00Z",
            vec![FEATURE_ID],
        );
        invalid_variant["entitlement_id"] =
            Value::String("00000000-0000-4000-0000-000000000000".to_string());
        write_private(
            &root.0.join(ENTITLEMENT_FILE_NAME),
            &signed_entitlement(&key, "test-key", &invalid_variant),
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
        assert_eq!(
            entitlement_path(
                Some(OsString::new()),
                Some(OsString::from("/home/test-user")),
            ),
            Some(PathBuf::from(
                "/home/test-user/.config/local-connect/entitlement-v1.json"
            ))
        );
    }

    #[cfg(unix)]
    #[test]
    fn active_entitlement_install_is_exact_private_atomic_and_visible_to_a_new_gate() {
        let root = TestDirectory::new();
        let key = signing_key();
        let source = root.0.join("purchased-license.json");
        let destination = root.0.join(ENTITLEMENT_FILE_NAME);
        let license = active_license(&key);
        write_private(&source, &license);
        let source_before = fs::read(&source).unwrap();
        let entitlement_gate = gate(&root, &key, "2026-08-31T00:00:00Z");

        assert_eq!(
            entitlement_gate.status().state,
            EntitlementDecision::Missing
        );
        assert_eq!(
            entitlement_gate.install(&source).unwrap(),
            EntitlementStatus {
                state: EntitlementDecision::Active,
                active: true,
            }
        );
        assert_eq!(fs::read(&source).unwrap(), source_before);
        assert_eq!(fs::read(&destination).unwrap(), license);
        assert_eq!(
            fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(root.0.join(ENTITLEMENT_LOCK_FILE_NAME))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(fs::read_dir(&root.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(&format!(".{ENTITLEMENT_FILE_NAME}.tmp."))
        }));

        let reopened = gate(&root, &key, "2026-08-31T00:00:00Z");
        assert_eq!(reopened.status().state, EntitlementDecision::Active);
    }

    #[cfg(unix)]
    #[test]
    fn installer_sets_exact_private_modes_under_restrictive_umask() {
        const CHILD_ENV: &str = "DOC_SUM_RESTRICTIVE_UMASK_CHILD";
        if env::var_os(CHILD_ENV).is_none() {
            let status = Command::new(env::current_exe().unwrap())
                .arg("--exact")
                .arg(
                    "connect::entitlement::tests::installer_sets_exact_private_modes_under_restrictive_umask",
                )
                .arg("--nocapture")
                .env(CHILD_ENV, "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }

        unsafe {
            libc::umask(0o777);
        }
        let root = TestDirectory::new();
        let key = signing_key();
        let source = root.0.join("candidate.json");
        write_private(&source, &active_license(&key));
        gate(&root, &key, "2026-08-31T00:00:00Z")
            .install(&source)
            .unwrap();

        for path in [
            root.0.join(ENTITLEMENT_FILE_NAME),
            root.0.join(ENTITLEMENT_LOCK_FILE_NAME),
        ] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn invalid_and_inactive_sources_preserve_the_existing_entitlement() {
        let root = TestDirectory::new();
        let key = signing_key();
        let destination = root.0.join(ENTITLEMENT_FILE_NAME);
        let existing = active_license(&key);
        write_private(&destination, &existing);
        let gate = gate(&root, &key, "2026-08-31T00:00:00Z");

        let invalid = root.0.join("invalid.json");
        write_private(&invalid, b"{}");
        assert_eq!(
            gate.install(&invalid),
            Err(EntitlementInstallError::SourceInvalid)
        );
        assert_eq!(fs::read(&destination).unwrap(), existing);

        let inactive_cases = [
            claims(
                "2025-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z",
                vec![FEATURE_ID],
            ),
            claims(
                "2026-09-01T00:00:00Z",
                "2027-01-01T00:00:00Z",
                vec![FEATURE_ID],
            ),
            claims(
                "2026-01-01T00:00:00Z",
                "2027-01-01T00:00:00Z",
                vec!["document.local_processing"],
            ),
        ];
        for (index, inactive_claims) in inactive_cases.iter().enumerate() {
            let source = root.0.join(format!("inactive-{index}.json"));
            write_private(
                &source,
                &signed_entitlement(&key, "test-key", inactive_claims),
            );
            assert_eq!(
                gate.install(&source),
                Err(EntitlementInstallError::NotActive)
            );
            assert_eq!(fs::read(&destination).unwrap(), existing);
        }
    }

    #[cfg(unix)]
    #[test]
    fn source_and_destination_safety_boundaries_fail_closed() {
        let root = TestDirectory::new();
        let key = signing_key();
        let destination = root.0.join(ENTITLEMENT_FILE_NAME);
        let candidate = root.0.join("candidate.json");
        let license = active_license(&key);
        write_private(&candidate, &license);
        let gate = gate(&root, &key, "2026-08-31T00:00:00Z");

        let linked_source = root.0.join("linked-source.json");
        symlink(&candidate, &linked_source).unwrap();
        assert_eq!(
            gate.install(&linked_source),
            Err(EntitlementInstallError::SourceInvalid)
        );
        assert!(!destination.exists());

        write_private(&destination, b"existing-entitlement");
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o640)).unwrap();
        assert_eq!(
            gate.install(&candidate),
            Err(EntitlementInstallError::StorageUnavailable)
        );
        assert_eq!(fs::read(&destination).unwrap(), b"existing-entitlement");
    }

    #[cfg(unix)]
    #[test]
    fn held_lock_blocks_installer_and_a_child_process() {
        const CHILD_ENV: &str = "DOC_SUM_ACTIVATION_LOCK_CHILD";
        if let Some(parent) = env::var_os(CHILD_ENV) {
            assert!(matches!(
                acquire_activation_lock(Path::new(&parent)),
                Err(EntitlementInstallError::ActivationBusy)
            ));
            return;
        }

        let root = TestDirectory::new();
        let key = signing_key();
        let source = root.0.join("candidate.json");
        let destination = root.0.join(ENTITLEMENT_FILE_NAME);
        write_private(&source, &active_license(&key));
        let lock = acquire_activation_lock(&root.0).unwrap();
        let gate = gate(&root, &key, "2026-08-31T00:00:00Z");

        assert_eq!(
            gate.install(&source),
            Err(EntitlementInstallError::ActivationBusy)
        );
        assert!(!destination.exists());

        let child = Command::new(env::current_exe().unwrap())
            .arg("--exact")
            .arg("connect::entitlement::tests::held_lock_blocks_installer_and_a_child_process")
            .arg("--nocapture")
            .env(CHILD_ENV, &root.0)
            .status()
            .unwrap();
        assert!(child.success());

        drop(lock);
        assert!(gate.install(&source).unwrap().active);
    }

    #[cfg(unix)]
    #[test]
    fn injected_pre_replace_failure_preserves_existing_bytes_and_cleans_temporary_file() {
        let root = TestDirectory::new();
        let key = signing_key();
        let destination = root.0.join(ENTITLEMENT_FILE_NAME);
        let existing = active_license(&key);
        write_private(&destination, &existing);
        let source = root.0.join("replacement.json");
        write_private(&source, &active_license(&key));
        let mut failing = gate(&root, &key, "2026-08-31T00:00:00Z");
        failing.fail_before_replace = true;

        assert_eq!(
            failing.install(&source),
            Err(EntitlementInstallError::InstallFailed)
        );
        assert_eq!(fs::read(&destination).unwrap(), existing);
        assert!(fs::read_dir(&root.0).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(&format!(".{ENTITLEMENT_FILE_NAME}.tmp."))
        }));
    }

    #[test]
    fn public_status_and_error_codes_are_stable_and_claim_free() {
        let status: EntitlementStatus = EntitlementDecision::Expired.into();
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            json!({"state": "expired", "active": false})
        );
        assert_eq!(
            EntitlementInstallError::AuthorityUnavailable.code(),
            "CONNECT_ENTITLEMENT_AUTHORITY_UNAVAILABLE"
        );
        assert_eq!(
            EntitlementInstallError::SourceInvalid.code(),
            "CONNECT_ENTITLEMENT_SOURCE_INVALID"
        );
        assert_eq!(
            EntitlementInstallError::NotActive.code(),
            "CONNECT_ENTITLEMENT_NOT_ACTIVE"
        );
        assert_eq!(
            EntitlementInstallError::StorageUnavailable.code(),
            "CONNECT_ENTITLEMENT_STORAGE_UNAVAILABLE"
        );
        assert_eq!(
            EntitlementInstallError::ActivationBusy.code(),
            "CONNECT_ENTITLEMENT_ACTIVATION_BUSY"
        );
        assert_eq!(
            EntitlementInstallError::InstallFailed.code(),
            "CONNECT_ENTITLEMENT_INSTALL_FAILED"
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
