use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::ffi::{CStr, CString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use uuid::Uuid;

const FORMAT_VERSION: u32 = 2;
const PACKAGE_ID: &str = "document-summarizer";
const PACKAGE_ROOT: &str = "/var/lib/document-summarizer";
const RECORD_FILE: &str = "package-operation-v1.json";
const LOCK_FILE: &str = ".package-operation-v1.lock";
const CONTROLLER_LOCK_FILE: &str = ".package-controller-v1.lock";
const QUIESCE_LOCK_FILE: &str = ".package-quiesce-v1.lock";
const QUIESCE_FILE: &str = "package-quiesce-v1.record";
const PARTICIPANTS_DIRECTORY: &str = "participants-v1";
const REMOVAL_RECEIPT_FILE: &str = "package-removal-receipt-v1.json";
const INSTALL_RECEIPT_FILE: &str = "package-install-receipt-v1.json";
const MAX_RECORD_BYTES: u64 = 256 * 1024;
const QUIESCE_FORMAT_VERSION: u32 = 1;
const BOOTSTRAP_QUIESCE_FORMAT_VERSION: u32 = 2;

#[derive(Debug, Error)]
pub(crate) enum PackageControlError {
    #[error("package lifecycle control requires root")]
    RootRequired,
    #[error("package lifecycle state is unavailable or unsafe")]
    Storage,
    #[error("package lifecycle record is invalid or conflicts with this operation")]
    Conflict,
    #[error("a per-user provider could not be stopped or restored")]
    Manager,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PackageKind {
    Upgrade,
    Remove,
    Reinstall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PackagePhase {
    IntentRecorded,
    PublishersStopped,
    Settling,
    Finalizing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ParticipantSettlement {
    Pending,
    Disabled,
    DeferredEnabled,
    SuccessorReady,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Participant {
    uid: u32,
    user: String,
    runtime_root: String,
    runtime_device: Option<u64>,
    runtime_inode: Option<u64>,
    app_data_root: String,
    app_data_device: u64,
    app_data_inode: u64,
    control_root: String,
    control_device: u64,
    control_inode: u64,
    manager_link: String,
    manager_parent_device: u64,
    manager_parent_inode: u64,
    acknowledgement_generation: String,
    enabled: bool,
    settlement: ParticipantSettlement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageRecord {
    format_version: u32,
    package_id: String,
    generation: String,
    kind: PackageKind,
    phase: PackagePhase,
    source_version: String,
    target_version: String,
    predecessor_removal_generation: Option<String>,
    participants: Vec<Participant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParticipantAcknowledgement {
    format_version: u32,
    package_id: String,
    uid: u32,
    user: String,
    generation: String,
    runtime_root: String,
    runtime_device: u64,
    runtime_inode: u64,
    app_data_root: String,
    app_data_device: u64,
    app_data_inode: u64,
    control_root: String,
    control_device: u64,
    control_inode: u64,
    manager_link: String,
    manager_parent_device: u64,
    manager_parent_inode: u64,
    enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemovalReceipt {
    format_version: u32,
    package_id: String,
    producer_generation: String,
    removed_version: String,
    participants: Vec<Participant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallReceipt {
    format_version: u32,
    package_id: String,
    install_generation: String,
    predecessor_removal_generation: String,
    installed_version: String,
    participants: Vec<Participant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QuiesceIntent {
    generation: String,
    kind: PackageKind,
    source_version: String,
    target_version: String,
    bootstrap: Option<BootstrapQuiesce>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BootstrapPhase {
    Quarantining,
    Quiesced,
    Adopted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BootstrapQuiesce {
    phase: BootstrapPhase,
    legacy_device: u64,
    legacy_inode: u64,
    legacy_sha256: String,
}

pub(crate) enum PackageAction<'a> {
    Initialize,
    PrepareUpgrade { source: &'a str, target: &'a str },
    FinishUpgrade { target: &'a str },
    PrepareRemove { target: &'a str },
    FinishRemove { target: &'a str },
    RecoverInstall { target: &'a str },
    PrepareReinstall { target: &'a str },
    AdoptBootstrap,
}

trait PackageEffects {
    fn discover(&mut self) -> Result<Vec<Participant>, PackageControlError>;
    fn stop(&mut self, participant: &Participant) -> Result<(), PackageControlError>;
    fn suppress(&mut self, participant: &Participant) -> Result<(), PackageControlError>;
    fn restore(
        &mut self,
        participant: &Participant,
        package_generation: &str,
    ) -> Result<ParticipantSettlement, PackageControlError>;
    fn validate_restored(
        &mut self,
        participant: &Participant,
        package_generation: &str,
        settlement: ParticipantSettlement,
    ) -> Result<(), PackageControlError>;
    fn cleanup(&mut self, participant: &Participant) -> Result<(), PackageControlError>;
    fn refresh_runtime_identity(
        &mut self,
        participant: &Participant,
    ) -> Result<Option<(u64, u64)>, PackageControlError>;
    fn prepare_controller(&mut self, record: &PackageRecord) -> Result<(), PackageControlError>;
    fn retire_controller(&mut self, record: &PackageRecord) -> Result<(), PackageControlError>;
}

struct PackageStore {
    root: PathBuf,
}

impl PackageStore {
    fn production() -> Self {
        Self {
            root: PathBuf::from(PACKAGE_ROOT),
        }
    }

    #[cfg(test)]
    fn for_test(root: PathBuf) -> Self {
        Self { root }
    }

    fn prepare_root(&self) -> Result<(), PackageControlError> {
        match fs::symlink_metadata(&self.root) {
            Ok(metadata)
                if metadata.file_type().is_dir()
                    && metadata.uid() == unsafe { libc::geteuid() } => {}
            Ok(_) => return Err(PackageControlError::Storage),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir_all(&self.root).map_err(|_| PackageControlError::Storage)?;
            }
            Err(_) => return Err(PackageControlError::Storage),
        }
        fs::set_permissions(&self.root, fs::Permissions::from_mode(0o755))
            .map_err(|_| PackageControlError::Storage)?;
        let metadata =
            fs::symlink_metadata(&self.root).map_err(|_| PackageControlError::Storage)?;
        if !metadata.file_type().is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o755
        {
            return Err(PackageControlError::Storage);
        }
        let participants = self.root.join(PARTICIPANTS_DIRECTORY);
        match fs::symlink_metadata(&participants) {
            Ok(metadata)
                if metadata.file_type().is_dir()
                    && metadata.uid() == unsafe { libc::geteuid() } => {}
            Ok(_) => return Err(PackageControlError::Storage),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&participants).map_err(|_| PackageControlError::Storage)?;
            }
            Err(_) => return Err(PackageControlError::Storage),
        }
        fs::set_permissions(&participants, fs::Permissions::from_mode(0o1777))
            .map_err(|_| PackageControlError::Storage)?;
        let participant_metadata =
            fs::symlink_metadata(&participants).map_err(|_| PackageControlError::Storage)?;
        if !participant_metadata.file_type().is_dir()
            || participant_metadata.uid() != unsafe { libc::geteuid() }
            || participant_metadata.mode() & 0o7777 != 0o1777
        {
            return Err(PackageControlError::Storage);
        }
        Ok(())
    }

    fn record_path(&self) -> PathBuf {
        self.root.join(RECORD_FILE)
    }

    fn quiesce_path(&self) -> PathBuf {
        self.root.join(QUIESCE_FILE)
    }

    fn bootstrap_quarantine_path(&self, generation: &str) -> Result<PathBuf, PackageControlError> {
        Uuid::parse_str(generation).map_err(|_| PackageControlError::Conflict)?;
        Ok(self.root.join(format!("legacy-executable-{generation}")))
    }

    fn removal_receipt_path(&self) -> PathBuf {
        self.root.join(REMOVAL_RECEIPT_FILE)
    }

    fn install_receipt_path(&self) -> PathBuf {
        self.root.join(INSTALL_RECEIPT_FILE)
    }

    fn lock(&self) -> Result<File, PackageControlError> {
        self.prepare_root()?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let file = options
            .open(self.root.join(LOCK_FILE))
            .map_err(|_| PackageControlError::Storage)?;
        file.set_permissions(fs::Permissions::from_mode(0o644))
            .map_err(|_| PackageControlError::Storage)?;
        file.lock().map_err(|_| PackageControlError::Storage)?;
        Ok(file)
    }

    fn controller_lock(&self) -> Result<File, PackageControlError> {
        self.prepare_root()?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let file = options
            .open(self.root.join(CONTROLLER_LOCK_FILE))
            .map_err(|_| PackageControlError::Storage)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| PackageControlError::Storage)?;
        file.lock().map_err(|_| PackageControlError::Storage)?;
        Ok(file)
    }

    fn quiesce_lock(&self, exclusive: bool) -> Result<File, PackageControlError> {
        if exclusive {
            self.prepare_root()?;
        }
        let mut options = OpenOptions::new();
        options.read(true);
        if exclusive {
            options.write(true).create(true).mode(0o644);
        }
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let file = options
            .open(self.root.join(QUIESCE_LOCK_FILE))
            .map_err(|_| PackageControlError::Storage)?;
        if exclusive {
            file.set_permissions(fs::Permissions::from_mode(0o644))
                .map_err(|_| PackageControlError::Storage)?;
        }
        let metadata = file.metadata().map_err(|_| PackageControlError::Storage)?;
        let root = fs::symlink_metadata(&self.root).map_err(|_| PackageControlError::Storage)?;
        if !metadata.file_type().is_file()
            || metadata.uid() != root.uid()
            || metadata.nlink() != 1
            || metadata.mode() & 0o777 != 0o644
        {
            return Err(PackageControlError::Conflict);
        }
        if exclusive {
            file.lock().map_err(|_| PackageControlError::Storage)?;
        } else {
            file.lock_shared()
                .map_err(|_| PackageControlError::Storage)?;
        }
        Ok(file)
    }

    fn lock_shared(&self) -> Result<File, PackageControlError> {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let file = options
            .open(self.root.join(LOCK_FILE))
            .map_err(|_| PackageControlError::Storage)?;
        let root = fs::symlink_metadata(&self.root).map_err(|_| PackageControlError::Storage)?;
        let metadata = file.metadata().map_err(|_| PackageControlError::Storage)?;
        let expected_uid = root.uid();
        if !root.file_type().is_dir()
            || root.mode() & 0o777 != 0o755
            || (self.root == Path::new(PACKAGE_ROOT) && expected_uid != 0)
            || !metadata.file_type().is_file()
            || metadata.uid() != expected_uid
            || metadata.nlink() != 1
            || metadata.mode() & 0o777 != 0o644
        {
            return Err(PackageControlError::Conflict);
        }
        file.lock_shared()
            .map_err(|_| PackageControlError::Storage)?;
        Ok(file)
    }

    fn read(&self) -> Result<Option<PackageRecord>, PackageControlError> {
        read_record(&self.record_path())
    }

    fn read_quiesce(&self) -> Result<Option<QuiesceIntent>, PackageControlError> {
        read_quiesce_intent(&self.quiesce_path(), &self.root)
    }

    fn write_quiesce(&self, intent: &QuiesceIntent) -> Result<(), PackageControlError> {
        let bytes = encode_quiesce_intent(intent)?;
        let temporary = self
            .root
            .join(format!(".{QUIESCE_FILE}.{}.tmp", intent.generation));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&temporary)
                .map_err(|_| PackageControlError::Storage)?;
            file.write_all(&bytes)
                .map_err(|_| PackageControlError::Storage)?;
            file.sync_all().map_err(|_| PackageControlError::Storage)?;
            fs::rename(&temporary, self.quiesce_path())
                .map_err(|_| PackageControlError::Storage)?;
            sync_directory(&self.root)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn clear_quiesce(&self, generation: &str) -> Result<(), PackageControlError> {
        let current = self.read_quiesce()?.ok_or(PackageControlError::Conflict)?;
        if current.generation != generation {
            return Err(PackageControlError::Conflict);
        }
        fs::remove_file(self.quiesce_path()).map_err(|_| PackageControlError::Storage)?;
        sync_directory(&self.root)
    }

    fn write(&self, record: &PackageRecord) -> Result<(), PackageControlError> {
        self.write_value(RECORD_FILE, &record.generation, record)
    }

    fn write_value<T: Serialize>(
        &self,
        name: &str,
        generation: &str,
        value: &T,
    ) -> Result<(), PackageControlError> {
        let bytes = serde_json::to_vec(value).map_err(|_| PackageControlError::Storage)?;
        let temporary = self.root.join(format!(".{name}.{generation}.tmp"));
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let result = (|| {
            let mut file = options
                .open(&temporary)
                .map_err(|_| PackageControlError::Storage)?;
            file.write_all(&bytes)
                .map_err(|_| PackageControlError::Storage)?;
            file.sync_all().map_err(|_| PackageControlError::Storage)?;
            fs::rename(&temporary, self.root.join(name))
                .map_err(|_| PackageControlError::Storage)?;
            sync_directory(&self.root)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn read_removal_receipt(&self) -> Result<Option<RemovalReceipt>, PackageControlError> {
        read_json_record(&self.removal_receipt_path())
    }

    fn clear(&self, generation: &str) -> Result<(), PackageControlError> {
        let current = self.read()?.ok_or(PackageControlError::Conflict)?;
        if current.generation != generation {
            return Err(PackageControlError::Conflict);
        }
        fs::remove_file(self.record_path()).map_err(|_| PackageControlError::Storage)?;
        sync_directory(&self.root)
    }
}

pub(crate) fn run(action: PackageAction<'_>) -> Result<(), PackageControlError> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(PackageControlError::RootRequired);
    }
    let store = PackageStore::production();
    let _controller = store.controller_lock()?;
    let quiesce_authority = store.quiesce_lock(true)?;
    let mut effects = SystemEffects {
        package_root: store.root.clone(),
    };
    match action {
        PackageAction::PrepareUpgrade { source, target } => {
            let intent = ensure_quiesce_intent(&store, PackageKind::Upgrade, source, target, None)?;
            prepare_from_intent(&store, &intent, &mut effects)?;
            let _package_authority = store.lock()?;
            store.clear_quiesce(&intent.generation)
        }
        PackageAction::PrepareRemove { target } => {
            let intent = ensure_quiesce_intent(&store, PackageKind::Remove, target, target, None)?;
            prepare_from_intent(&store, &intent, &mut effects)?;
            let _package_authority = store.lock()?;
            store.clear_quiesce(&intent.generation)
        }
        PackageAction::AdoptBootstrap => {
            let intent = store.read_quiesce()?.ok_or(PackageControlError::Conflict)?;
            if intent.kind != PackageKind::Upgrade {
                return Err(PackageControlError::Conflict);
            }
            adopt_bootstrap(&store, &intent, &mut effects)
        }
        action => {
            let lock = store.lock()?;
            run_with(
                &store,
                action,
                &mut effects,
                Some(&quiesce_authority),
                Some(&lock),
            )
        }
    }
}

fn ensure_quiesce_intent(
    store: &PackageStore,
    kind: PackageKind,
    source: &str,
    target: &str,
    generation: Option<&str>,
) -> Result<QuiesceIntent, PackageControlError> {
    validate_version(source)?;
    validate_version(target)?;
    let expected_generation = generation
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let expected = QuiesceIntent {
        generation: expected_generation,
        kind,
        source_version: source.to_string(),
        target_version: target.to_string(),
        bootstrap: None,
    };
    if let Some(existing) = store.read_quiesce()? {
        return (existing == expected
            || generation.is_none()
                && existing.kind == kind
                && existing.source_version == source
                && existing.target_version == target)
            .then_some(existing)
            .ok_or(PackageControlError::Conflict);
    }
    store.write_quiesce(&expected)?;
    Ok(expected)
}

fn adopt_bootstrap(
    store: &PackageStore,
    intent: &QuiesceIntent,
    effects: &mut dyn PackageEffects,
) -> Result<(), PackageControlError> {
    let bootstrap = intent
        .bootstrap
        .as_ref()
        .ok_or(PackageControlError::Conflict)?;
    match bootstrap.phase {
        BootstrapPhase::Quarantining => return Err(PackageControlError::Conflict),
        BootstrapPhase::Quiesced => validate_bootstrap_quarantine(store, intent, false)?,
        BootstrapPhase::Adopted => validate_bootstrap_quarantine(store, intent, true)?,
    }
    prepare_from_intent(store, intent, effects)?;
    let _package_authority = store.lock()?;
    let current = store.read_quiesce()?.ok_or(PackageControlError::Conflict)?;
    if current != *intent {
        return Err(PackageControlError::Conflict);
    }
    let adopted = if bootstrap.phase == BootstrapPhase::Adopted {
        intent.clone()
    } else {
        let mut adopted = intent.clone();
        adopted
            .bootstrap
            .as_mut()
            .ok_or(PackageControlError::Conflict)?
            .phase = BootstrapPhase::Adopted;
        store.write_quiesce(&adopted)?;
        adopted
    };
    remove_bootstrap_quarantine(store, &adopted)?;
    store.clear_quiesce(&adopted.generation)
}

fn prepare_from_intent(
    store: &PackageStore,
    intent: &QuiesceIntent,
    effects: &mut dyn PackageEffects,
) -> Result<(), PackageControlError> {
    let (mut record, is_new) = match store.read()? {
        Some(record) => {
            validate(&record, intent.kind, &intent.target_version)?;
            if record.generation != intent.generation
                || record.source_version != intent.source_version
            {
                return Err(PackageControlError::Conflict);
            }
            (record, false)
        }
        None => (
            PackageRecord {
                format_version: FORMAT_VERSION,
                package_id: PACKAGE_ID.to_string(),
                generation: intent.generation.clone(),
                kind: intent.kind,
                phase: PackagePhase::IntentRecorded,
                source_version: intent.source_version.clone(),
                target_version: intent.target_version.clone(),
                predecessor_removal_generation: None,
                participants: effects.discover()?,
            },
            true,
        ),
    };
    let _participant_authorities =
        lock_participant_authorities(&record.participants, store.root == Path::new(PACKAGE_ROOT))?;
    if is_new {
        store.write(&record)?;
    }
    effects.prepare_controller(&record)?;
    if record.phase == PackagePhase::IntentRecorded {
        for index in 0..record.participants.len() {
            if record.participants[index].runtime_device.is_none() {
                if let Some((device, inode)) =
                    effects.refresh_runtime_identity(&record.participants[index])?
                {
                    record.participants[index].runtime_device = Some(device);
                    record.participants[index].runtime_inode = Some(inode);
                    store.write(&record)?;
                }
            }
            effects.stop(&record.participants[index])?;
            effects.suppress(&record.participants[index])?;
            effects.cleanup(&record.participants[index])?;
        }
        record.phase = PackagePhase::PublishersStopped;
        store.write(&record)?;
    }
    Ok(())
}

fn run_with(
    store: &PackageStore,
    action: PackageAction<'_>,
    effects: &mut dyn PackageEffects,
    quiesce_lock: Option<&File>,
    package_lock: Option<&File>,
) -> Result<(), PackageControlError> {
    match action {
        PackageAction::Initialize => Ok(()),
        PackageAction::PrepareUpgrade { source, target } => {
            prepare(store, PackageKind::Upgrade, source, target, effects)
        }
        PackageAction::PrepareRemove { target } => {
            prepare(store, PackageKind::Remove, target, target, effects)
        }
        PackageAction::FinishUpgrade { target } => finish(
            store,
            PackageKind::Upgrade,
            target,
            effects,
            quiesce_lock,
            package_lock,
        ),
        PackageAction::FinishRemove { target } => finish(
            store,
            PackageKind::Remove,
            target,
            effects,
            quiesce_lock,
            package_lock,
        ),
        PackageAction::RecoverInstall { target } => {
            let record = store.read()?.ok_or(PackageControlError::Conflict)?;
            match record.kind {
                PackageKind::Upgrade => finish(
                    store,
                    PackageKind::Upgrade,
                    target,
                    effects,
                    quiesce_lock,
                    package_lock,
                ),
                PackageKind::Reinstall => finish(
                    store,
                    PackageKind::Reinstall,
                    target,
                    effects,
                    quiesce_lock,
                    package_lock,
                ),
                PackageKind::Remove => Err(PackageControlError::Conflict),
            }
        }
        PackageAction::PrepareReinstall { target } => {
            if let Some(record) = store.read()? {
                match record.kind {
                    PackageKind::Remove => {
                        prepare(
                            store,
                            PackageKind::Remove,
                            &record.source_version,
                            &record.target_version,
                            effects,
                        )?;
                        finish(
                            store,
                            PackageKind::Remove,
                            &record.target_version,
                            effects,
                            quiesce_lock,
                            package_lock,
                        )?;
                    }
                    PackageKind::Reinstall => {}
                    PackageKind::Upgrade => return Err(PackageControlError::Conflict),
                }
            }
            prepare_reinstall(store, target)
        }
        PackageAction::AdoptBootstrap => Err(PackageControlError::Conflict),
    }
}

fn prepare(
    store: &PackageStore,
    kind: PackageKind,
    source: &str,
    target: &str,
    effects: &mut dyn PackageEffects,
) -> Result<(), PackageControlError> {
    let (mut record, is_new) = match store.read()? {
        Some(record) => {
            validate(&record, kind, target)?;
            (record, false)
        }
        None => {
            let record = PackageRecord {
                format_version: FORMAT_VERSION,
                package_id: PACKAGE_ID.to_string(),
                generation: Uuid::new_v4().to_string(),
                kind,
                phase: PackagePhase::IntentRecorded,
                source_version: source.to_string(),
                target_version: target.to_string(),
                predecessor_removal_generation: None,
                participants: effects.discover()?,
            };
            (record, true)
        }
    };
    let _participant_authorities =
        lock_participant_authorities(&record.participants, store.root == Path::new(PACKAGE_ROOT))?;
    if is_new {
        store.write(&record)?;
    }
    effects.prepare_controller(&record)?;
    if record.phase == PackagePhase::IntentRecorded {
        for participant in &record.participants {
            effects.stop(participant)?;
            effects.suppress(participant)?;
            effects.cleanup(participant)?;
        }
        record.phase = PackagePhase::PublishersStopped;
        store.write(&record)?;
    }
    Ok(())
}

fn finish(
    store: &PackageStore,
    kind: PackageKind,
    target: &str,
    effects: &mut dyn PackageEffects,
    quiesce_lock: Option<&File>,
    package_lock: Option<&File>,
) -> Result<(), PackageControlError> {
    let mut record = store.read()?.ok_or(PackageControlError::Conflict)?;
    validate(&record, kind, target)?;
    let mut participant_authorities =
        lock_participant_authorities(&record.participants, store.root == Path::new(PACKAGE_ROOT))?;
    if !matches!(
        record.phase,
        PackagePhase::PublishersStopped | PackagePhase::Settling | PackagePhase::Finalizing
    ) {
        return Err(PackageControlError::Conflict);
    }
    if record.phase == PackagePhase::PublishersStopped {
        record.phase = PackagePhase::Settling;
        store.write(&record)?;
    }
    if record.phase == PackagePhase::Settling {
        for index in 0..record.participants.len() {
            if record.participants[index].settlement != ParticipantSettlement::Pending {
                continue;
            }
            if record.participants[index].runtime_device.is_none() {
                if let Some((device, inode)) =
                    effects.refresh_runtime_identity(&record.participants[index])?
                {
                    record.participants[index].runtime_device = Some(device);
                    record.participants[index].runtime_inode = Some(inode);
                    store.write(&record)?;
                }
            }
            let participant = record.participants[index].clone();
            effects.cleanup(&participant)?;
            if matches!(kind, PackageKind::Upgrade | PackageKind::Reinstall) {
                drop(participant_authorities);
                if let Some(lock) = package_lock {
                    File::unlock(lock).map_err(|_| PackageControlError::Storage)?;
                }
                if let Some(lock) = quiesce_lock {
                    File::unlock(lock).map_err(|_| PackageControlError::Storage)?;
                }
                let restored = effects.restore(&participant, &record.generation);
                if let Some(lock) = quiesce_lock {
                    lock_exclusive_bounded(lock)?;
                }
                if let Some(lock) = package_lock {
                    lock_exclusive_bounded(lock)?;
                }
                participant_authorities = lock_participant_authorities(
                    &record.participants,
                    store.root == Path::new(PACKAGE_ROOT),
                )?;
                let current = store.read()?.ok_or(PackageControlError::Conflict)?;
                if current.generation != record.generation
                    || current.kind != record.kind
                    || current.phase != PackagePhase::Settling
                    || current.participants != record.participants
                {
                    return Err(PackageControlError::Conflict);
                }
                let settlement = restored?;
                effects.validate_restored(&participant, &record.generation, settlement)?;
                record.participants[index].settlement = settlement;
            } else {
                record.participants[index].settlement = ParticipantSettlement::Disabled;
            }
            store.write(&record)?;
        }
        if kind == PackageKind::Remove {
            write_removal_receipt(store, &record)?;
        } else if kind == PackageKind::Reinstall {
            write_install_receipt(store, &record)?;
        }
        record.phase = PackagePhase::Finalizing;
        store.write(&record)?;
    } else if record
        .participants
        .iter()
        .any(|participant| participant.settlement == ParticipantSettlement::Pending)
    {
        return Err(PackageControlError::Conflict);
    } else if kind == PackageKind::Remove {
        validate_exact_removal_receipt(store, &record)?;
    } else if kind == PackageKind::Reinstall {
        validate_exact_install_receipt(store, &record)?;
    }
    if kind != PackageKind::Remove {
        effects.retire_controller(&record)?;
    }
    if kind == PackageKind::Reinstall {
        remove_exact_removal_receipt(store, &record)?;
    }
    store.clear(&record.generation)?;
    drop(participant_authorities);
    Ok(())
}

fn prepare_reinstall(store: &PackageStore, target: &str) -> Result<(), PackageControlError> {
    let receipt = store
        .read_removal_receipt()?
        .ok_or(PackageControlError::Conflict)?;
    validate_removal_receipt(&receipt)?;
    let expected_participants = receipt
        .participants
        .iter()
        .cloned()
        .map(|mut participant| {
            participant.settlement = ParticipantSettlement::Pending;
            participant
        })
        .collect::<Vec<_>>();
    let (record, is_new) = match store.read()? {
        Some(record) => {
            validate(&record, PackageKind::Reinstall, target)?;
            if record.phase != PackagePhase::PublishersStopped
                || record.source_version != receipt.removed_version
                || record.predecessor_removal_generation.as_deref()
                    != Some(receipt.producer_generation.as_str())
                || record.participants != expected_participants
            {
                return Err(PackageControlError::Conflict);
            }
            (record, false)
        }
        None => (
            PackageRecord {
                format_version: FORMAT_VERSION,
                package_id: PACKAGE_ID.to_string(),
                generation: Uuid::new_v4().to_string(),
                kind: PackageKind::Reinstall,
                phase: PackagePhase::PublishersStopped,
                source_version: receipt.removed_version.clone(),
                target_version: target.to_string(),
                predecessor_removal_generation: Some(receipt.producer_generation.clone()),
                participants: expected_participants,
            },
            true,
        ),
    };
    let _participant_authorities =
        lock_participant_authorities(&record.participants, store.root == Path::new(PACKAGE_ROOT))?;
    if is_new {
        store.write(&record)?;
    }
    adopt_reinstall_controller(store, &record, &receipt)
}

fn adopt_reinstall_controller(
    store: &PackageStore,
    record: &PackageRecord,
    receipt: &RemovalReceipt,
) -> Result<(), PackageControlError> {
    let predecessor_controller = store.root.join(format!(
        "package-controller-{}",
        receipt.producer_generation
    ));
    let successor_controller = store
        .root
        .join(format!("package-controller-{}", record.generation));
    let root_metadata =
        fs::symlink_metadata(&store.root).map_err(|_| PackageControlError::Storage)?;
    let validate_controller = |path: &Path| {
        fs::symlink_metadata(path).is_ok_and(|metadata| {
            metadata.file_type().is_file()
                && metadata.uid() == root_metadata.uid()
                && metadata.nlink() == 1
                && metadata.mode() & 0o777 == 0o700
        })
    };
    if successor_controller.exists() {
        if predecessor_controller.exists() || !validate_controller(&successor_controller) {
            return Err(PackageControlError::Conflict);
        }
        return Ok(());
    }
    if !validate_controller(&predecessor_controller) {
        return Err(PackageControlError::Conflict);
    }
    fs::rename(&predecessor_controller, &successor_controller)
        .map_err(|_| PackageControlError::Storage)?;
    sync_directory(&store.root)
}

fn write_removal_receipt(
    store: &PackageStore,
    record: &PackageRecord,
) -> Result<(), PackageControlError> {
    let receipt = RemovalReceipt {
        format_version: FORMAT_VERSION,
        package_id: PACKAGE_ID.to_string(),
        producer_generation: record.generation.clone(),
        removed_version: record.target_version.clone(),
        participants: record.participants.clone(),
    };
    if let Some(existing) = store.read_removal_receipt()? {
        return (existing == receipt)
            .then_some(())
            .ok_or(PackageControlError::Conflict);
    }
    store.write_value(REMOVAL_RECEIPT_FILE, &record.generation, &receipt)
}

fn write_install_receipt(
    store: &PackageStore,
    record: &PackageRecord,
) -> Result<(), PackageControlError> {
    let predecessor = record
        .predecessor_removal_generation
        .clone()
        .ok_or(PackageControlError::Conflict)?;
    let receipt = InstallReceipt {
        format_version: FORMAT_VERSION,
        package_id: PACKAGE_ID.to_string(),
        install_generation: record.generation.clone(),
        predecessor_removal_generation: predecessor,
        installed_version: record.target_version.clone(),
        participants: record.participants.clone(),
    };
    if let Some(existing) = read_json_record::<InstallReceipt>(&store.install_receipt_path())? {
        validate_install_receipt(&existing)?;
        if existing.install_generation == record.generation {
            return (existing == receipt)
                .then_some(())
                .ok_or(PackageControlError::Conflict);
        }
        if existing.installed_version != record.source_version {
            return Err(PackageControlError::Conflict);
        }
    }
    store.write_value(INSTALL_RECEIPT_FILE, &record.generation, &receipt)
}

fn validate_exact_removal_receipt(
    store: &PackageStore,
    record: &PackageRecord,
) -> Result<(), PackageControlError> {
    let receipt = store
        .read_removal_receipt()?
        .ok_or(PackageControlError::Conflict)?;
    validate_removal_receipt(&receipt)?;
    (receipt.producer_generation == record.generation
        && receipt.removed_version == record.target_version
        && receipt.participants == record.participants)
        .then_some(())
        .ok_or(PackageControlError::Conflict)
}

fn validate_exact_install_receipt(
    store: &PackageStore,
    record: &PackageRecord,
) -> Result<(), PackageControlError> {
    let receipt: InstallReceipt =
        read_json_record(&store.install_receipt_path())?.ok_or(PackageControlError::Conflict)?;
    validate_install_receipt(&receipt)?;
    (receipt.install_generation == record.generation
        && Some(receipt.predecessor_removal_generation.as_str())
            == record.predecessor_removal_generation.as_deref()
        && receipt.installed_version == record.target_version
        && receipt.participants == record.participants)
        .then_some(())
        .ok_or(PackageControlError::Conflict)
}

fn validate_install_receipt(receipt: &InstallReceipt) -> Result<(), PackageControlError> {
    if receipt.format_version != FORMAT_VERSION
        || receipt.package_id != PACKAGE_ID
        || Uuid::parse_str(&receipt.install_generation).is_err()
        || Uuid::parse_str(&receipt.predecessor_removal_generation).is_err()
        || receipt
            .participants
            .windows(2)
            .any(|pair| pair[0].uid >= pair[1].uid)
        || receipt.participants.iter().any(invalid_runtime_identity)
    {
        return Err(PackageControlError::Conflict);
    }
    Ok(())
}

fn remove_exact_removal_receipt(
    store: &PackageStore,
    record: &PackageRecord,
) -> Result<(), PackageControlError> {
    let Some(receipt) = store.read_removal_receipt()? else {
        return validate_exact_install_receipt(store, record);
    };
    validate_removal_receipt(&receipt)?;
    if Some(receipt.producer_generation.as_str())
        != record.predecessor_removal_generation.as_deref()
    {
        return Err(PackageControlError::Conflict);
    }
    fs::remove_file(store.removal_receipt_path()).map_err(|_| PackageControlError::Storage)?;
    sync_directory(&store.root)
}

fn validate_removal_receipt(receipt: &RemovalReceipt) -> Result<(), PackageControlError> {
    if receipt.format_version != FORMAT_VERSION
        || receipt.package_id != PACKAGE_ID
        || Uuid::parse_str(&receipt.producer_generation).is_err()
        || receipt
            .participants
            .windows(2)
            .any(|pair| pair[0].uid >= pair[1].uid)
        || receipt.participants.iter().any(invalid_runtime_identity)
    {
        return Err(PackageControlError::Conflict);
    }
    Ok(())
}

fn lock_participant_authorities(
    participants: &[Participant],
    enforce: bool,
) -> Result<Vec<File>, PackageControlError> {
    if !enforce {
        return Ok(Vec::new());
    }
    let mut guards = Vec::with_capacity(participants.len() * 2);
    for participant in participants {
        for name in [
            ".background-control-v1.lock",
            ".background-admission-v1.lock",
        ] {
            let path = Path::new(&participant.control_root).join(name);
            let mut options = OpenOptions::new();
            options
                .read(true)
                .write(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
            let file = options
                .open(path)
                .map_err(|_| PackageControlError::Storage)?;
            let metadata = file.metadata().map_err(|_| PackageControlError::Storage)?;
            if !metadata.file_type().is_file()
                || metadata.uid() != participant.uid
                || metadata.nlink() != 1
                || metadata.mode() & 0o777 != 0o600
            {
                return Err(PackageControlError::Conflict);
            }
            file.lock().map_err(|_| PackageControlError::Storage)?;
            guards.push(file);
        }
    }
    Ok(guards)
}

fn validate(
    record: &PackageRecord,
    kind: PackageKind,
    target: &str,
) -> Result<(), PackageControlError> {
    if record.format_version != FORMAT_VERSION
        || record.package_id != PACKAGE_ID
        || record.kind != kind
        || record.target_version != target
        || Uuid::parse_str(&record.generation).is_err()
        || (record.kind == PackageKind::Reinstall)
            != record.predecessor_removal_generation.is_some()
        || record
            .participants
            .windows(2)
            .any(|pair| pair[0].uid >= pair[1].uid)
        || record.participants.iter().any(invalid_runtime_identity)
    {
        return Err(PackageControlError::Conflict);
    }
    Ok(())
}

fn invalid_runtime_identity(participant: &Participant) -> bool {
    participant.runtime_device.is_some() != participant.runtime_inode.is_some()
}

pub(crate) struct PackageAdmissionGuard {
    _quiesce: File,
    _lock: File,
}

pub(crate) fn enter_admission() -> Result<PackageAdmissionGuard, PackageControlError> {
    enter_admission_store(&PackageStore::production())
}

#[cfg(not(test))]
pub(crate) fn enter_startup_admission(
    app_data_root: &Path,
    runtime_root: &Path,
) -> Result<PackageAdmissionGuard, PackageControlError> {
    enter_startup_admission_store(
        &PackageStore::production(),
        unsafe { libc::geteuid() },
        app_data_root,
        runtime_root,
    )
}

#[cfg(test)]
pub(crate) fn enter_admission_at(
    root: &Path,
) -> Result<PackageAdmissionGuard, PackageControlError> {
    let store = PackageStore::for_test(root.to_path_buf());
    if !store.root.exists() {
        store.prepare_root()?;
        drop(store.quiesce_lock(true)?);
        drop(store.lock()?);
    }
    enter_admission_store(&store)
}

#[cfg(test)]
pub(crate) fn enter_startup_admission_at(
    root: &Path,
    app_data_root: &Path,
    runtime_root: &Path,
) -> Result<PackageAdmissionGuard, PackageControlError> {
    let store = PackageStore::for_test(root.to_path_buf());
    if !store.root.exists() {
        store.prepare_root()?;
        drop(store.quiesce_lock(true)?);
        drop(store.lock()?);
    }
    enter_startup_admission_store(
        &store,
        unsafe { libc::geteuid() },
        app_data_root,
        runtime_root,
    )
}

#[cfg(test)]
pub(crate) fn begin_quiesce_at(root: &Path) -> Result<(), PackageControlError> {
    let store = PackageStore::for_test(root.to_path_buf());
    if !store.root.exists() {
        store.prepare_root()?;
        drop(store.lock()?);
    }
    let _authority = store.quiesce_lock(true)?;
    let intent = ensure_quiesce_intent(&store, PackageKind::Upgrade, "0.0.9", "0.1.0", None)?;
    if store.read()?.is_none() {
        store.write(&PackageRecord {
            format_version: FORMAT_VERSION,
            package_id: PACKAGE_ID.to_string(),
            generation: intent.generation.clone(),
            kind: intent.kind,
            phase: PackagePhase::PublishersStopped,
            source_version: intent.source_version.clone(),
            target_version: intent.target_version.clone(),
            predecessor_removal_generation: None,
            participants: Vec::new(),
        })?;
    }
    let _package_authority = store.lock()?;
    store.clear_quiesce(&intent.generation)
}

fn enter_admission_store(
    store: &PackageStore,
) -> Result<PackageAdmissionGuard, PackageControlError> {
    let quiesce = store.quiesce_lock(false)?;
    if store.read_quiesce()?.is_some() {
        return Err(PackageControlError::Conflict);
    }
    let lock = store.lock_shared()?;
    if let Some(record) = store.read()? {
        validate(&record, record.kind, &record.target_version)?;
        return Err(PackageControlError::Conflict);
    }
    Ok(PackageAdmissionGuard {
        _quiesce: quiesce,
        _lock: lock,
    })
}

fn enter_startup_admission_store(
    store: &PackageStore,
    uid: u32,
    app_data_root: &Path,
    runtime_root: &Path,
) -> Result<PackageAdmissionGuard, PackageControlError> {
    let quiesce = store.quiesce_lock(false)?;
    if store.read_quiesce()?.is_some() {
        return Err(PackageControlError::Conflict);
    }
    let lock = store.lock_shared()?;
    let Some(record) = store.read()? else {
        return Ok(PackageAdmissionGuard {
            _quiesce: quiesce,
            _lock: lock,
        });
    };
    validate(&record, record.kind, &record.target_version)?;
    let app_data_root = absolute_path_text(app_data_root)?;
    let runtime_root = absolute_path_text(runtime_root)?;
    let admitted = matches!(record.kind, PackageKind::Upgrade | PackageKind::Reinstall)
        && record.phase == PackagePhase::Settling
        && record.participants.iter().any(|participant| {
            participant.uid == uid
                && participant.enabled
                && participant.settlement == ParticipantSettlement::Pending
                && participant.app_data_root == app_data_root
                && participant.runtime_root == runtime_root
        });
    if !admitted {
        return Err(PackageControlError::Conflict);
    }
    let participant = record
        .participants
        .iter()
        .find(|participant| {
            participant.uid == uid
                && participant.app_data_root == app_data_root
                && participant.runtime_root == runtime_root
        })
        .ok_or(PackageControlError::Conflict)?;
    validate_participant_directories(participant)?;
    let runtime = safe_directory_identity(Path::new(&participant.runtime_root), uid, true)?;
    if Some(runtime.dev()) != participant.runtime_device
        || Some(runtime.ino()) != participant.runtime_inode
    {
        return Err(PackageControlError::Conflict);
    }
    Ok(PackageAdmissionGuard {
        _quiesce: quiesce,
        _lock: lock,
    })
}

pub(crate) fn record_participant_acknowledgement(
    enabled: bool,
    runtime_root: &Path,
    app_data_root: &Path,
    control_root: &Path,
    manager_link: &Path,
) -> Result<(), PackageControlError> {
    let uid = unsafe { libc::geteuid() };
    let user = nss_user_name(uid).ok_or(PackageControlError::Conflict)?;
    let runtime_identity = safe_directory_identity(runtime_root, uid, true)?;
    let app_data_identity = safe_directory_identity(app_data_root, uid, true)?;
    let control_identity = safe_directory_identity(control_root, uid, true)?;
    let manager_parent = manager_link.parent().ok_or(PackageControlError::Conflict)?;
    let manager_identity = safe_directory_identity(manager_parent, uid, false)?;
    let acknowledgement = ParticipantAcknowledgement {
        format_version: FORMAT_VERSION,
        package_id: PACKAGE_ID.to_string(),
        uid,
        user,
        generation: Uuid::new_v4().to_string(),
        runtime_root: absolute_path_text(runtime_root)?,
        runtime_device: runtime_identity.dev(),
        runtime_inode: runtime_identity.ino(),
        app_data_root: absolute_path_text(app_data_root)?,
        app_data_device: app_data_identity.dev(),
        app_data_inode: app_data_identity.ino(),
        control_root: absolute_path_text(control_root)?,
        control_device: control_identity.dev(),
        control_inode: control_identity.ino(),
        manager_link: absolute_path_text(manager_link)?,
        manager_parent_device: manager_identity.dev(),
        manager_parent_inode: manager_identity.ino(),
        enabled,
    };
    let store = PackageStore::production();
    let directory = store.root.join(PARTICIPANTS_DIRECTORY);
    cleanup_participant_temporaries(&directory, uid)?;
    let path = directory.join(format!("{uid}.json"));
    let temporary = directory.join(format!(".{uid}.{}.tmp", acknowledgement.generation));
    let bytes = serde_json::to_vec(&acknowledgement).map_err(|_| PackageControlError::Storage)?;
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|_| PackageControlError::Storage)?;
        file.write_all(&bytes)
            .map_err(|_| PackageControlError::Storage)?;
        file.sync_all().map_err(|_| PackageControlError::Storage)?;
        fs::rename(&temporary, &path).map_err(|_| PackageControlError::Storage)?;
        sync_directory(&directory)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn cleanup_participant_temporaries(directory: &Path, uid: u32) -> Result<(), PackageControlError> {
    let prefix = format!(".{uid}.");
    for entry in fs::read_dir(directory).map_err(|_| PackageControlError::Storage)? {
        let entry = entry.map_err(|_| PackageControlError::Storage)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(&prefix) || !name.ends_with(".tmp") {
            continue;
        }
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(|_| PackageControlError::Storage)?;
        if !metadata.file_type().is_file()
            || metadata.uid() != uid
            || metadata.nlink() != 1
            || metadata.mode() & 0o777 != 0o600
        {
            return Err(PackageControlError::Conflict);
        }
        fs::remove_file(entry.path()).map_err(|_| PackageControlError::Storage)?;
    }
    sync_directory(directory)
}

fn absolute_path_text(path: &Path) -> Result<String, PackageControlError> {
    if !path.is_absolute() {
        return Err(PackageControlError::Conflict);
    }
    path.to_str()
        .map(str::to_string)
        .ok_or(PackageControlError::Conflict)
}

fn validate_version(value: &str) -> Result<(), PackageControlError> {
    (!value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b':' | b'~' | b'_' | b'-')
        }))
    .then_some(())
    .ok_or(PackageControlError::Conflict)
}

fn package_kind_text(kind: PackageKind) -> &'static str {
    match kind {
        PackageKind::Upgrade => "upgrade",
        PackageKind::Remove => "remove",
        PackageKind::Reinstall => "reinstall",
    }
}

fn open_bootstrap_quarantine(
    store: &PackageStore,
    intent: &QuiesceIntent,
) -> Result<Option<(File, fs::Metadata, PathBuf)>, PackageControlError> {
    let bootstrap = intent
        .bootstrap
        .as_ref()
        .ok_or(PackageControlError::Conflict)?;
    if bootstrap.legacy_device == 0
        || bootstrap.legacy_inode == 0
        || bootstrap.legacy_sha256.len() != 64
        || !bootstrap
            .legacy_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(PackageControlError::Conflict);
    }
    let path = store.bootstrap_quarantine_path(&intent.generation)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = match options.open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(PackageControlError::Storage),
    };
    let metadata = file.metadata().map_err(|_| PackageControlError::Storage)?;
    let root = fs::symlink_metadata(&store.root).map_err(|_| PackageControlError::Storage)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != root.uid()
        || metadata.nlink() != 1
        || metadata.mode() & 0o777 != 0o600
        || metadata.len() == 0
    {
        return Err(PackageControlError::Conflict);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| PackageControlError::Storage)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if format!("{:x}", hasher.finalize()) != bootstrap.legacy_sha256 {
        return Err(PackageControlError::Conflict);
    }
    let current = fs::symlink_metadata(&path).map_err(|_| PackageControlError::Storage)?;
    if current.dev() != metadata.dev() || current.ino() != metadata.ino() {
        return Err(PackageControlError::Conflict);
    }
    Ok(Some((file, metadata, path)))
}

fn validate_bootstrap_quarantine(
    store: &PackageStore,
    intent: &QuiesceIntent,
    allow_missing: bool,
) -> Result<(), PackageControlError> {
    match open_bootstrap_quarantine(store, intent)? {
        Some(_) => Ok(()),
        None if allow_missing => Ok(()),
        None => Err(PackageControlError::Conflict),
    }
}

fn remove_bootstrap_quarantine(
    store: &PackageStore,
    intent: &QuiesceIntent,
) -> Result<(), PackageControlError> {
    let bootstrap = intent
        .bootstrap
        .as_ref()
        .ok_or(PackageControlError::Conflict)?;
    if bootstrap.phase != BootstrapPhase::Adopted {
        return Err(PackageControlError::Conflict);
    }
    let Some((_file, metadata, path)) = open_bootstrap_quarantine(store, intent)? else {
        return Ok(());
    };
    let current = fs::symlink_metadata(&path).map_err(|_| PackageControlError::Storage)?;
    if current.dev() != metadata.dev() || current.ino() != metadata.ino() {
        return Err(PackageControlError::Conflict);
    }
    fs::remove_file(path).map_err(|_| PackageControlError::Storage)?;
    sync_directory(&store.root)
}

fn encode_quiesce_intent(intent: &QuiesceIntent) -> Result<Vec<u8>, PackageControlError> {
    if Uuid::parse_str(&intent.generation).is_err() || intent.kind == PackageKind::Reinstall {
        return Err(PackageControlError::Conflict);
    }
    validate_version(&intent.source_version)?;
    validate_version(&intent.target_version)?;
    let payload = if let Some(bootstrap) = &intent.bootstrap {
        if intent.kind != PackageKind::Upgrade
            || bootstrap.legacy_device == 0
            || bootstrap.legacy_inode == 0
            || bootstrap.legacy_sha256.len() != 64
            || !bootstrap
                .legacy_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(PackageControlError::Conflict);
        }
        let phase = match bootstrap.phase {
            BootstrapPhase::Quarantining => "quarantining",
            BootstrapPhase::Quiesced => "quiesced",
            BootstrapPhase::Adopted => "adopted",
        };
        format!(
            "format_version={BOOTSTRAP_QUIESCE_FORMAT_VERSION}\npackage_id={PACKAGE_ID}\nartifact_scope=shared-debian\ngeneration={}\nkind={}\nphase={phase}\nsource_version={}\ntarget_version={}\nlegacy_device={}\nlegacy_inode={}\nlegacy_sha256={}\n",
            intent.generation,
            package_kind_text(intent.kind),
            intent.source_version,
            intent.target_version,
            bootstrap.legacy_device,
            bootstrap.legacy_inode,
            bootstrap.legacy_sha256,
        )
    } else {
        format!(
            "format_version={QUIESCE_FORMAT_VERSION}\npackage_id={PACKAGE_ID}\nartifact_scope=shared-debian\ngeneration={}\nkind={}\nsource_version={}\ntarget_version={}\n",
            intent.generation,
            package_kind_text(intent.kind),
            intent.source_version,
            intent.target_version,
        )
    };
    let digest = format!("{:x}", Sha256::digest(payload.as_bytes()));
    Ok(format!("{payload}sha256={digest}\n").into_bytes())
}

fn read_quiesce_intent(
    path: &Path,
    root: &Path,
) -> Result<Option<QuiesceIntent>, PackageControlError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(PackageControlError::Storage),
    };
    let metadata = file.metadata().map_err(|_| PackageControlError::Storage)?;
    let root_metadata = fs::symlink_metadata(root).map_err(|_| PackageControlError::Storage)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != root_metadata.uid()
        || metadata.nlink() != 1
        || metadata.mode() & 0o777 != 0o600
        || metadata.len() == 0
        || metadata.len() > MAX_RECORD_BYTES
    {
        return Err(PackageControlError::Conflict);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PackageControlError::Storage)?;
    if bytes.len() as u64 != metadata.len() {
        return Err(PackageControlError::Conflict);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| PackageControlError::Conflict)?;
    let (payload, digest_line) = text
        .rsplit_once("sha256=")
        .ok_or(PackageControlError::Conflict)?;
    let digest = digest_line
        .strip_suffix('\n')
        .ok_or(PackageControlError::Conflict)?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || format!("{:x}", Sha256::digest(payload.as_bytes())) != digest
    {
        return Err(PackageControlError::Conflict);
    }
    let mut values = std::collections::BTreeMap::new();
    for line in payload.lines() {
        let (key, value) = line.split_once('=').ok_or(PackageControlError::Conflict)?;
        if values.insert(key, value).is_some() {
            return Err(PackageControlError::Conflict);
        }
    }
    if values.get("package_id") != Some(&PACKAGE_ID)
        || values.get("artifact_scope") != Some(&"shared-debian")
    {
        return Err(PackageControlError::Conflict);
    }
    let generation = values
        .get("generation")
        .filter(|value| Uuid::parse_str(value).is_ok())
        .ok_or(PackageControlError::Conflict)?;
    let kind = match values.get("kind") {
        Some(&"upgrade") => PackageKind::Upgrade,
        Some(&"remove") => PackageKind::Remove,
        _ => return Err(PackageControlError::Conflict),
    };
    let source_version = *values
        .get("source_version")
        .ok_or(PackageControlError::Conflict)?;
    let target_version = *values
        .get("target_version")
        .ok_or(PackageControlError::Conflict)?;
    validate_version(source_version)?;
    validate_version(target_version)?;
    let bootstrap = match values.get("format_version") {
        Some(&"1") if values.len() == 7 => None,
        Some(&"2") if values.len() == 11 && kind == PackageKind::Upgrade => {
            let phase = match values.get("phase") {
                Some(&"quarantining") => BootstrapPhase::Quarantining,
                Some(&"quiesced") => BootstrapPhase::Quiesced,
                Some(&"adopted") => BootstrapPhase::Adopted,
                _ => return Err(PackageControlError::Conflict),
            };
            let legacy_device = values
                .get("legacy_device")
                .ok_or(PackageControlError::Conflict)?
                .parse::<u64>()
                .map_err(|_| PackageControlError::Conflict)?;
            let legacy_inode = values
                .get("legacy_inode")
                .ok_or(PackageControlError::Conflict)?
                .parse::<u64>()
                .map_err(|_| PackageControlError::Conflict)?;
            let legacy_sha256 = values
                .get("legacy_sha256")
                .ok_or(PackageControlError::Conflict)?
                .to_string();
            Some(BootstrapQuiesce {
                phase,
                legacy_device,
                legacy_inode,
                legacy_sha256,
            })
        }
        _ => return Err(PackageControlError::Conflict),
    };
    let intent = QuiesceIntent {
        generation: (*generation).to_string(),
        kind,
        source_version: source_version.to_string(),
        target_version: target_version.to_string(),
        bootstrap,
    };
    if encode_quiesce_intent(&intent)? != bytes {
        return Err(PackageControlError::Conflict);
    }
    Ok(Some(intent))
}

fn safe_directory_identity(
    path: &Path,
    uid: u32,
    private: bool,
) -> Result<fs::Metadata, PackageControlError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| PackageControlError::Storage)?;
    let mode = metadata.mode() & 0o777;
    if !path.is_absolute()
        || !metadata.file_type().is_dir()
        || metadata.uid() != uid
        || (private && mode != 0o700)
        || (!private && mode & 0o022 != 0)
    {
        return Err(PackageControlError::Conflict);
    }
    Ok(metadata)
}

fn read_record(path: &Path) -> Result<Option<PackageRecord>, PackageControlError> {
    read_json_record(path)
}

fn read_json_record<T: for<'de> Deserialize<'de>>(
    path: &Path,
) -> Result<Option<T>, PackageControlError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(PackageControlError::Storage),
    };
    let metadata = file.metadata().map_err(|_| PackageControlError::Storage)?;
    let parent = path.parent().ok_or(PackageControlError::Storage)?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|_| PackageControlError::Storage)?;
    let expected_uid = parent_metadata.uid();
    if !parent_metadata.file_type().is_dir()
        || parent_metadata.mode() & 0o777 != 0o755
        || (parent == Path::new("/var/lib/document-summarizer") && expected_uid != 0)
    {
        return Err(PackageControlError::Conflict);
    }
    if !metadata.file_type().is_file()
        || metadata.uid() != expected_uid
        || metadata.nlink() != 1
        || metadata.mode() & 0o022 != 0
        || metadata.len() == 0
        || metadata.len() > MAX_RECORD_BYTES
    {
        return Err(PackageControlError::Conflict);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PackageControlError::Storage)?;
    if bytes.len() as u64 != metadata.len() {
        return Err(PackageControlError::Conflict);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| PackageControlError::Conflict)
}

fn sync_directory(path: &Path) -> Result<(), PackageControlError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| PackageControlError::Storage)
}

struct SystemEffects {
    package_root: PathBuf,
}

fn controller_path(record: &PackageRecord) -> PathBuf {
    PackageStore::production()
        .root
        .join(format!("package-controller-{}", record.generation))
}

impl PackageEffects for SystemEffects {
    fn discover(&mut self) -> Result<Vec<Participant>, PackageControlError> {
        let mut participants = Vec::new();
        let directory = self.package_root.join(PARTICIPANTS_DIRECTORY);
        let package_root =
            fs::symlink_metadata(&self.package_root).map_err(|_| PackageControlError::Storage)?;
        let directory_metadata =
            fs::symlink_metadata(&directory).map_err(|_| PackageControlError::Storage)?;
        if !directory_metadata.file_type().is_dir()
            || directory_metadata.uid() != package_root.uid()
            || directory_metadata.mode() & 0o7777 != 0o1777
        {
            return Err(PackageControlError::Conflict);
        }
        for entry in fs::read_dir(directory).map_err(|_| PackageControlError::Storage)? {
            let entry = entry.map_err(|_| PackageControlError::Storage)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(PackageControlError::Conflict)?;
            let uid = name
                .strip_suffix(".json")
                .and_then(|value| value.parse::<u32>().ok())
                .ok_or(PackageControlError::Conflict)?;
            let metadata =
                fs::symlink_metadata(entry.path()).map_err(|_| PackageControlError::Storage)?;
            if !metadata.file_type().is_file()
                || metadata.uid() != uid
                || metadata.nlink() != 1
                || metadata.mode() & 0o777 != 0o600
            {
                return Err(PackageControlError::Conflict);
            }
            let acknowledgement = read_participant_acknowledgement(&entry.path(), &metadata)?;
            validate_participant_acknowledgement(&acknowledgement, uid)?;
            validate_acknowledged_directories(&acknowledgement)?;
            let runtime_present = Path::new(&acknowledgement.runtime_root).is_dir();
            if Path::new(&acknowledgement.control_root)
                .join("background-mode-transition-v1.json")
                .exists()
            {
                return Err(PackageControlError::Conflict);
            }
            participants.push(Participant {
                uid,
                user: acknowledgement.user,
                runtime_root: acknowledgement.runtime_root,
                runtime_device: runtime_present.then_some(acknowledgement.runtime_device),
                runtime_inode: runtime_present.then_some(acknowledgement.runtime_inode),
                app_data_root: acknowledgement.app_data_root,
                app_data_device: acknowledgement.app_data_device,
                app_data_inode: acknowledgement.app_data_inode,
                control_root: acknowledgement.control_root,
                control_device: acknowledgement.control_device,
                control_inode: acknowledgement.control_inode,
                manager_link: acknowledgement.manager_link,
                manager_parent_device: acknowledgement.manager_parent_device,
                manager_parent_inode: acknowledgement.manager_parent_inode,
                acknowledgement_generation: acknowledgement.generation,
                enabled: acknowledgement.enabled,
                settlement: ParticipantSettlement::Pending,
            });
        }
        participants.sort_by_key(|participant| participant.uid);
        Ok(participants)
    }

    fn stop(&mut self, participant: &Participant) -> Result<(), PackageControlError> {
        validate_participant_directories(participant)?;
        if !Path::new(&participant.runtime_root).is_dir() {
            return Ok(());
        }
        manager_success(&participant.user, &["stop"])?;
        let executable = env::current_exe().map_err(|_| PackageControlError::Storage)?;
        bounded_command(
            Command::new("runuser")
                .arg("-u")
                .arg(&participant.user)
                .arg("--")
                .arg(executable)
                .arg("--connect-package-user-stop")
                .arg(&participant.app_data_root)
                .arg(&participant.runtime_root),
            Duration::from_secs(50),
        )
        .and_then(|status| {
            status
                .success()
                .then_some(())
                .ok_or(PackageControlError::Manager)
        })
    }

    fn restore(
        &mut self,
        participant: &Participant,
        _package_generation: &str,
    ) -> Result<ParticipantSettlement, PackageControlError> {
        validate_participant_directories(participant)?;
        if !Path::new(&participant.runtime_root).is_dir() {
            if participant.enabled {
                create_participant_enablement_link(participant)?;
                return Ok(ParticipantSettlement::DeferredEnabled);
            }
            remove_participant_enablement_link(participant)?;
            return Ok(ParticipantSettlement::Disabled);
        }
        if !participant.enabled {
            manager_success(&participant.user, &["disable", "--now"])?;
            return Ok(ParticipantSettlement::Disabled);
        }
        manager_success(&participant.user, &["enable", "--now"])?;
        crate::connect::provider::wait_for_registered_provider(
            Path::new(&participant.runtime_root),
            None,
            Instant::now() + Duration::from_secs(35),
        )
        .map_err(|_| PackageControlError::Manager)?;
        Ok(ParticipantSettlement::SuccessorReady)
    }

    fn validate_restored(
        &mut self,
        participant: &Participant,
        _package_generation: &str,
        settlement: ParticipantSettlement,
    ) -> Result<(), PackageControlError> {
        validate_participant_directories(participant)?;
        match (participant.enabled, settlement) {
            (true, ParticipantSettlement::SuccessorReady) => {
                let runtime = safe_directory_identity(
                    Path::new(&participant.runtime_root),
                    participant.uid,
                    true,
                )?;
                if Some(runtime.dev()) != participant.runtime_device
                    || Some(runtime.ino()) != participant.runtime_inode
                {
                    return Err(PackageControlError::Conflict);
                }
                crate::connect::provider::wait_for_registered_provider(
                    Path::new(&participant.runtime_root),
                    None,
                    Instant::now() + Duration::from_secs(2),
                )
                .map_err(|_| PackageControlError::Manager)
            }
            (true, ParticipantSettlement::DeferredEnabled) => {
                if Path::new(&participant.runtime_root).exists() {
                    return Err(PackageControlError::Conflict);
                }
                let directory = open_participant_manager_parent(participant)?;
                let name = participant_link_name(participant)?;
                (read_participant_enablement_target(&directory, &name)?
                    == Some(PathBuf::from(
                        "/usr/lib/systemd/user/document-summarizer-connect.service",
                    )))
                .then_some(())
                .ok_or(PackageControlError::Conflict)
            }
            (false, ParticipantSettlement::Disabled) => {
                if Path::new(&participant.runtime_root).is_dir() {
                    let status = systemctl_user(
                        &participant.user,
                        &[
                            "is-enabled",
                            "--quiet",
                            "document-summarizer-connect.service",
                        ],
                    )?;
                    (status.code() == Some(1))
                        .then_some(())
                        .ok_or(PackageControlError::Manager)
                } else {
                    let directory = open_participant_manager_parent(participant)?;
                    let name = participant_link_name(participant)?;
                    read_participant_enablement_target(&directory, &name)?
                        .is_none()
                        .then_some(())
                        .ok_or(PackageControlError::Conflict)
                }
            }
            _ => Err(PackageControlError::Conflict),
        }
    }

    fn suppress(&mut self, participant: &Participant) -> Result<(), PackageControlError> {
        validate_participant_directories(participant)?;
        if Path::new(&participant.runtime_root).is_dir() {
            return manager_success(&participant.user, &["disable", "--now"]);
        }
        remove_participant_enablement_link(participant)
    }

    fn cleanup(&mut self, participant: &Participant) -> Result<(), PackageControlError> {
        validate_participant_directories(participant)?;
        for version in ["v1", "v2"] {
            let directory = Path::new(&participant.runtime_root)
                .join("local-connect")
                .join(version)
                .join("providers");
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => return Err(PackageControlError::Storage),
            };
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if !name.starts_with("document-summarizer-") || !name.ends_with(".json") {
                    continue;
                }
                return Err(PackageControlError::Conflict);
            }
        }
        Ok(())
    }

    fn refresh_runtime_identity(
        &mut self,
        participant: &Participant,
    ) -> Result<Option<(u64, u64)>, PackageControlError> {
        let runtime_path = Path::new(&participant.runtime_root);
        if !runtime_path.exists() {
            return Ok(None);
        }
        if nss_user_name(participant.uid).as_deref() != Some(participant.user.as_str()) {
            return Err(PackageControlError::Conflict);
        }
        let runtime = safe_directory_identity(runtime_path, participant.uid, true)?;
        let loginctl = bounded_stdout(
            Command::new("loginctl")
                .arg("show-user")
                .arg(participant.uid.to_string())
                .arg("--property=RuntimePath")
                .arg("--value"),
            Duration::from_secs(10),
        )?;
        let manager = bounded_stdout(
            Command::new("systemctl")
                .arg("--user")
                .arg(format!("--machine={}@", participant.user))
                .arg("show-environment"),
            Duration::from_secs(10),
        )?;
        if !runtime_session_proof_matches(participant, &loginctl, &manager) {
            return Err(PackageControlError::Conflict);
        }
        Ok(Some((runtime.dev(), runtime.ino())))
    }

    fn prepare_controller(&mut self, record: &PackageRecord) -> Result<(), PackageControlError> {
        let source = env::current_exe().map_err(|_| PackageControlError::Storage)?;
        let destination = controller_path(record);
        if destination.exists() {
            let metadata =
                fs::symlink_metadata(&destination).map_err(|_| PackageControlError::Storage)?;
            return (metadata.file_type().is_file()
                && metadata.uid() == 0
                && metadata.nlink() == 1
                && metadata.mode() & 0o777 == 0o700)
                .then_some(())
                .ok_or(PackageControlError::Conflict);
        }
        let temporary = destination.with_extension("tmp");
        if temporary.exists() {
            let metadata =
                fs::symlink_metadata(&temporary).map_err(|_| PackageControlError::Storage)?;
            if !metadata.file_type().is_file()
                || metadata.uid() != 0
                || metadata.nlink() != 1
                || metadata.mode() & 0o777 != 0o700
            {
                return Err(PackageControlError::Conflict);
            }
            fs::remove_file(&temporary).map_err(|_| PackageControlError::Storage)?;
        }
        let mut source_file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(source)
            .map_err(|_| PackageControlError::Storage)?;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o700)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let mut destination_file = options
            .open(&temporary)
            .map_err(|_| PackageControlError::Storage)?;
        io::copy(&mut source_file, &mut destination_file)
            .map_err(|_| PackageControlError::Storage)?;
        destination_file
            .sync_all()
            .map_err(|_| PackageControlError::Storage)?;
        fs::rename(&temporary, &destination).map_err(|_| PackageControlError::Storage)?;
        sync_directory(destination.parent().ok_or(PackageControlError::Storage)?)
    }

    fn retire_controller(&mut self, record: &PackageRecord) -> Result<(), PackageControlError> {
        let path = controller_path(record);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && record.phase == PackagePhase::Finalizing =>
            {
                return Ok(())
            }
            Err(_) => return Err(PackageControlError::Storage),
        };
        if !metadata.file_type().is_file()
            || metadata.uid() != 0
            || metadata.nlink() != 1
            || metadata.mode() & 0o777 != 0o700
        {
            return Err(PackageControlError::Conflict);
        }
        fs::remove_file(&path).map_err(|_| PackageControlError::Storage)?;
        sync_directory(path.parent().ok_or(PackageControlError::Storage)?)
    }
}

fn runtime_session_proof_matches(
    participant: &Participant,
    loginctl: &[u8],
    manager: &[u8],
) -> bool {
    let Ok(runtime_text) = std::str::from_utf8(loginctl) else {
        return false;
    };
    let Ok(manager_text) = std::str::from_utf8(manager) else {
        return false;
    };
    let expected = format!("XDG_RUNTIME_DIR={}", participant.runtime_root);
    runtime_text.trim() == participant.runtime_root
        && manager_text.lines().any(|line| line == expected)
}

fn validate_participant_directories(participant: &Participant) -> Result<(), PackageControlError> {
    let matches = |path: &str, device: u64, inode: u64, private: bool| {
        safe_directory_identity(Path::new(path), participant.uid, private)
            .is_ok_and(|metadata| metadata.dev() == device && metadata.ino() == inode)
    };
    let manager_parent = Path::new(&participant.manager_link)
        .parent()
        .ok_or(PackageControlError::Conflict)?;
    let runtime_matches_or_is_absent = match fs::symlink_metadata(&participant.runtime_root) {
        Ok(_) => participant
            .runtime_device
            .zip(participant.runtime_inode)
            .is_some_and(|(device, inode)| matches(&participant.runtime_root, device, inode, true)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(_) => false,
    };
    if !runtime_matches_or_is_absent
        || !matches(
            &participant.app_data_root,
            participant.app_data_device,
            participant.app_data_inode,
            true,
        )
        || !matches(
            &participant.control_root,
            participant.control_device,
            participant.control_inode,
            true,
        )
        || !safe_directory_identity(manager_parent, participant.uid, false).is_ok_and(|metadata| {
            metadata.dev() == participant.manager_parent_device
                && metadata.ino() == participant.manager_parent_inode
        })
    {
        return Err(PackageControlError::Conflict);
    }
    Ok(())
}

#[cfg(test)]
fn exact_enablement_link(home: &Path) -> Result<Option<PathBuf>, PackageControlError> {
    let path = home
        .join(".config/systemd/user/default.target.wants")
        .join("document-summarizer-connect.service");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(PackageControlError::Storage),
    };
    if !metadata.file_type().is_symlink() {
        return Err(PackageControlError::Conflict);
    }
    let target = fs::read_link(&path).map_err(|_| PackageControlError::Conflict)?;
    if target != Path::new("/usr/lib/systemd/user/document-summarizer-connect.service") {
        return Err(PackageControlError::Conflict);
    }
    Ok(Some(path))
}

fn open_participant_manager_parent(participant: &Participant) -> Result<File, PackageControlError> {
    let parent = Path::new(&participant.manager_link)
        .parent()
        .ok_or(PackageControlError::Conflict)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let directory = options
        .open(parent)
        .map_err(|_| PackageControlError::Storage)?;
    let metadata = directory
        .metadata()
        .map_err(|_| PackageControlError::Storage)?;
    if !metadata.file_type().is_dir()
        || metadata.uid() != participant.uid
        || metadata.dev() != participant.manager_parent_device
        || metadata.ino() != participant.manager_parent_inode
        || metadata.mode() & 0o022 != 0
    {
        return Err(PackageControlError::Conflict);
    }
    Ok(directory)
}

fn participant_link_name(participant: &Participant) -> Result<CString, PackageControlError> {
    let name = Path::new(&participant.manager_link)
        .file_name()
        .ok_or(PackageControlError::Conflict)?;
    if name != "document-summarizer-connect.service" {
        return Err(PackageControlError::Conflict);
    }
    CString::new(name.as_bytes()).map_err(|_| PackageControlError::Conflict)
}

fn read_participant_enablement_target(
    directory: &File,
    name: &CStr,
) -> Result<Option<PathBuf>, PackageControlError> {
    let mut bytes = vec![0_u8; 4096];
    let length = unsafe {
        libc::readlinkat(
            directory.as_raw_fd(),
            name.as_ptr(),
            bytes.as_mut_ptr().cast(),
            bytes.len(),
        )
    };
    if length < 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(PackageControlError::Conflict)
        };
    }
    bytes.truncate(length as usize);
    Ok(Some(PathBuf::from(std::ffi::OsString::from_vec(bytes))))
}

fn remove_participant_enablement_link(
    participant: &Participant,
) -> Result<(), PackageControlError> {
    let directory = open_participant_manager_parent(participant)?;
    let name = participant_link_name(participant)?;
    let Some(target) = read_participant_enablement_target(&directory, &name)? else {
        return Ok(());
    };
    if target != Path::new("/usr/lib/systemd/user/document-summarizer-connect.service") {
        return Err(PackageControlError::Conflict);
    }
    if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(PackageControlError::Storage);
    }
    directory
        .sync_all()
        .map_err(|_| PackageControlError::Storage)
}

fn create_participant_enablement_link(
    participant: &Participant,
) -> Result<(), PackageControlError> {
    let directory = open_participant_manager_parent(participant)?;
    let name = participant_link_name(participant)?;
    let target = CString::new("/usr/lib/systemd/user/document-summarizer-connect.service")
        .map_err(|_| PackageControlError::Conflict)?;
    if unsafe { libc::symlinkat(target.as_ptr(), directory.as_raw_fd(), name.as_ptr()) } != 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists
            || read_participant_enablement_target(&directory, &name)?
                != Some(PathBuf::from(
                    target.to_str().map_err(|_| PackageControlError::Conflict)?,
                ))
        {
            return Err(PackageControlError::Conflict);
        }
    }
    directory
        .sync_all()
        .map_err(|_| PackageControlError::Storage)
}

fn validate_participant_acknowledgement(
    acknowledgement: &ParticipantAcknowledgement,
    owner_uid: u32,
) -> Result<(), PackageControlError> {
    let absolute = |value: &str| Path::new(value).is_absolute() && !value.contains('\n');
    if acknowledgement.format_version != FORMAT_VERSION
        || acknowledgement.package_id != PACKAGE_ID
        || acknowledgement.uid != owner_uid
        || acknowledgement.user.is_empty()
        || nss_user_name(owner_uid).as_deref() != Some(acknowledgement.user.as_str())
        || Uuid::parse_str(&acknowledgement.generation).is_err()
        || !absolute(&acknowledgement.runtime_root)
        || !absolute(&acknowledgement.app_data_root)
        || !absolute(&acknowledgement.control_root)
        || !absolute(&acknowledgement.manager_link)
    {
        return Err(PackageControlError::Conflict);
    }
    Ok(())
}

fn validate_acknowledged_directories(
    acknowledgement: &ParticipantAcknowledgement,
) -> Result<(), PackageControlError> {
    let matches = |path: &str, device: u64, inode: u64, private: bool| {
        safe_directory_identity(Path::new(path), acknowledgement.uid, private)
            .is_ok_and(|metadata| metadata.dev() == device && metadata.ino() == inode)
    };
    let runtime_matches_or_is_absent = match fs::symlink_metadata(&acknowledgement.runtime_root) {
        Ok(_) => matches(
            &acknowledgement.runtime_root,
            acknowledgement.runtime_device,
            acknowledgement.runtime_inode,
            true,
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(_) => false,
    };
    let manager_link = Path::new(&acknowledgement.manager_link);
    if manager_link.file_name().and_then(|name| name.to_str())
        != Some("document-summarizer-connect.service")
        || !runtime_matches_or_is_absent
        || !matches(
            &acknowledgement.app_data_root,
            acknowledgement.app_data_device,
            acknowledgement.app_data_inode,
            true,
        )
        || !matches(
            &acknowledgement.control_root,
            acknowledgement.control_device,
            acknowledgement.control_inode,
            true,
        )
        || !manager_link.parent().is_some_and(|parent| {
            safe_directory_identity(parent, acknowledgement.uid, false).is_ok_and(|metadata| {
                metadata.dev() == acknowledgement.manager_parent_device
                    && metadata.ino() == acknowledgement.manager_parent_inode
            })
        })
    {
        return Err(PackageControlError::Conflict);
    }
    Ok(())
}

fn read_participant_acknowledgement(
    path: &Path,
    expected: &fs::Metadata,
) -> Result<ParticipantAcknowledgement, PackageControlError> {
    let length = expected.len();
    if length == 0 || length > MAX_RECORD_BYTES {
        return Err(PackageControlError::Conflict);
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options
        .open(path)
        .map_err(|_| PackageControlError::Storage)?;
    let opened = file.metadata().map_err(|_| PackageControlError::Storage)?;
    if !opened.file_type().is_file()
        || opened.uid() != expected.uid()
        || opened.dev() != expected.dev()
        || opened.ino() != expected.ino()
        || opened.nlink() != 1
        || opened.mode() & 0o777 != 0o600
        || opened.len() != length
    {
        return Err(PackageControlError::Conflict);
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PackageControlError::Storage)?;
    if bytes.len() as u64 != length {
        return Err(PackageControlError::Conflict);
    }
    serde_json::from_slice(&bytes).map_err(|_| PackageControlError::Conflict)
}

fn nss_user_name(uid: u32) -> Option<String> {
    let mut record = unsafe { std::mem::zeroed::<libc::passwd>() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0_u8; 16 * 1024];
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            &mut record,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() || record.pw_name.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(record.pw_name) }
        .to_str()
        .ok()
        .map(str::to_string)
}

fn manager_success(user: &str, arguments: &[&str]) -> Result<(), PackageControlError> {
    let mut command = arguments.to_vec();
    command.push("document-summarizer-connect.service");
    systemctl_user(user, &command)?
        .success()
        .then_some(())
        .ok_or(PackageControlError::Manager)
}

fn systemctl_user(user: &str, arguments: &[&str]) -> Result<ExitStatus, PackageControlError> {
    bounded_command(
        Command::new("systemctl")
            .arg("--user")
            .arg(format!("--machine={user}@"))
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
        Duration::from_secs(45),
    )
}

fn bounded_command(
    command: &mut Command,
    timeout: Duration,
) -> Result<ExitStatus, PackageControlError> {
    let mut child = command.spawn().map_err(|_| PackageControlError::Manager)?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|_| PackageControlError::Manager)? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PackageControlError::Manager);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn lock_exclusive_bounded(file: &File) -> Result<(), PackageControlError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(std::fs::TryLockError::WouldBlock) => return Err(PackageControlError::Manager),
            Err(std::fs::TryLockError::Error(_)) => return Err(PackageControlError::Storage),
        }
    }
}

fn bounded_stdout(
    command: &mut Command,
    timeout: Duration,
) -> Result<Vec<u8>, PackageControlError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| PackageControlError::Manager)?;
    let mut stdout = child.stdout.take().ok_or(PackageControlError::Manager)?;
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let count = stdout.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            if output.len() <= 8192 {
                let remaining = 8193usize.saturating_sub(output.len());
                output.extend_from_slice(&buffer[..count.min(remaining)]);
            }
        }
        Ok::<_, io::Error>(output)
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_| PackageControlError::Manager)? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(PackageControlError::Manager);
        }
        thread::sleep(Duration::from_millis(25));
    };
    let output = reader
        .join()
        .map_err(|_| PackageControlError::Manager)?
        .map_err(|_| PackageControlError::Manager)?;
    if !status.success() || output.len() > 8192 {
        return Err(PackageControlError::Manager);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MockEffects {
        participants: Vec<Participant>,
        calls: Arc<Mutex<Vec<String>>>,
        defer_enabled: bool,
        fail_restore_once: bool,
        controller_root: Option<PathBuf>,
        refreshed_runtime: Option<(u64, u64)>,
        restore_flock_paths: Option<(PathBuf, PathBuf)>,
        restore_record_path: Option<PathBuf>,
    }

    impl PackageEffects for MockEffects {
        fn discover(&mut self) -> Result<Vec<Participant>, PackageControlError> {
            Ok(self.participants.clone())
        }
        fn stop(&mut self, participant: &Participant) -> Result<(), PackageControlError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("stop:{}", participant.uid));
            Ok(())
        }
        fn restore(
            &mut self,
            participant: &Participant,
            _package_generation: &str,
        ) -> Result<ParticipantSettlement, PackageControlError> {
            if self.fail_restore_once {
                self.fail_restore_once = false;
                return Err(PackageControlError::Manager);
            }
            if let Some((quiesce, package)) = &self.restore_flock_paths {
                let status = bounded_command(
                    Command::new("flock")
                        .arg("-s")
                        .arg(quiesce)
                        .arg("flock")
                        .arg("-s")
                        .arg(package)
                        .arg("true"),
                    Duration::from_millis(250),
                )?;
                if !status.success() {
                    return Err(PackageControlError::Manager);
                }
            }
            if let Some(path) = &self.restore_record_path {
                let mut record = read_record(path)?.ok_or(PackageControlError::Conflict)?;
                record.participants[0].enabled = !record.participants[0].enabled;
                fs::write(
                    path,
                    serde_json::to_vec(&record).map_err(|_| PackageControlError::Storage)?,
                )
                .map_err(|_| PackageControlError::Storage)?;
            }
            self.calls.lock().unwrap().push(format!(
                "restore:{}:{}",
                participant.uid, participant.enabled
            ));
            Ok(if participant.enabled && self.defer_enabled {
                ParticipantSettlement::DeferredEnabled
            } else if participant.enabled {
                ParticipantSettlement::SuccessorReady
            } else {
                ParticipantSettlement::Disabled
            })
        }
        fn validate_restored(
            &mut self,
            participant: &Participant,
            package_generation: &str,
            settlement: ParticipantSettlement,
        ) -> Result<(), PackageControlError> {
            self.calls.lock().unwrap().push(format!(
                "validate:{}:{package_generation}:{settlement:?}",
                participant.uid
            ));
            Ok(())
        }
        fn suppress(&mut self, participant: &Participant) -> Result<(), PackageControlError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("suppress:{}", participant.uid));
            Ok(())
        }
        fn cleanup(&mut self, participant: &Participant) -> Result<(), PackageControlError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("cleanup:{}", participant.uid));
            Ok(())
        }
        fn refresh_runtime_identity(
            &mut self,
            participant: &Participant,
        ) -> Result<Option<(u64, u64)>, PackageControlError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("refresh:{}", participant.uid));
            Ok(self.refreshed_runtime)
        }
        fn prepare_controller(
            &mut self,
            _record: &PackageRecord,
        ) -> Result<(), PackageControlError> {
            Ok(())
        }
        fn retire_controller(&mut self, record: &PackageRecord) -> Result<(), PackageControlError> {
            if let Some(root) = &self.controller_root {
                let path = root.join(format!("package-controller-{}", record.generation));
                match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(error)
                        if error.kind() == io::ErrorKind::NotFound
                            && record.phase == PackagePhase::Finalizing => {}
                    Err(_) => return Err(PackageControlError::Storage),
                }
            }
            Ok(())
        }
    }

    fn store(label: &str) -> (PackageStore, PathBuf) {
        let root = env::temp_dir().join(format!("{label}-{}", Uuid::new_v4()));
        let store = PackageStore::for_test(root.clone());
        store.prepare_root().unwrap();
        drop(store.quiesce_lock(true).unwrap());
        drop(store.lock().unwrap());
        (store, root)
    }

    fn participant(uid: u32, enabled: bool) -> Participant {
        Participant {
            uid,
            user: format!("user-{uid}"),
            runtime_root: format!("/run/user/{uid}"),
            runtime_device: Some(0),
            runtime_inode: Some(0),
            app_data_root: format!("/home/user-{uid}/.local/share/com.juan-canfield.docsum"),
            app_data_device: 0,
            app_data_inode: 0,
            control_root: format!("/home/user-{uid}/.config/com.juan-canfield.docsum"),
            control_device: 0,
            control_inode: 0,
            manager_link: format!("/home/user-{uid}/.config/systemd/user/default.target.wants/document-summarizer-connect.service"),
            manager_parent_device: 0,
            manager_parent_inode: 0,
            acknowledgement_generation: Uuid::new_v4().to_string(),
            enabled,
            settlement: ParticipantSettlement::Pending,
        }
    }

    #[test]
    fn upgrade_crash_resume_preserves_choices_and_exact_generation() {
        let (store, root) = store("doc-sum-package-upgrade");
        let mut effects = MockEffects {
            participants: vec![participant(1000, true), participant(1001, false)],
            ..Default::default()
        };
        prepare(&store, PackageKind::Upgrade, "0.0.9", "0.1.0", &mut effects).unwrap();
        let generation = store.read().unwrap().unwrap().generation;
        prepare(&store, PackageKind::Upgrade, "0.0.9", "0.1.0", &mut effects).unwrap();
        assert_eq!(store.read().unwrap().unwrap().generation, generation);
        finish(
            &store,
            PackageKind::Upgrade,
            "0.1.0",
            &mut effects,
            None,
            None,
        )
        .unwrap();
        assert!(store.read().unwrap().is_none());
        assert_eq!(
            effects.calls.lock().unwrap().clone(),
            vec![
                "stop:1000".to_string(),
                "suppress:1000".to_string(),
                "cleanup:1000".to_string(),
                "stop:1001".to_string(),
                "suppress:1001".to_string(),
                "cleanup:1001".to_string(),
                "cleanup:1000".to_string(),
                "restore:1000:true".to_string(),
                format!("validate:1000:{generation}:SuccessorReady"),
                "cleanup:1001".to_string(),
                "restore:1001:false".to_string(),
                format!("validate:1001:{generation}:Disabled"),
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn remove_stops_and_cleans_without_restoring_workers() {
        let (store, root) = store("doc-sum-package-remove");
        let mut effects = MockEffects {
            participants: vec![participant(1000, true)],
            ..Default::default()
        };
        prepare(&store, PackageKind::Remove, "0.1.0", "0.1.0", &mut effects).unwrap();
        finish(
            &store,
            PackageKind::Remove,
            "0.1.0",
            &mut effects,
            None,
            None,
        )
        .unwrap();
        assert!(store.read().unwrap().is_none());
        assert_eq!(
            effects.calls.lock().unwrap().as_slice(),
            ["stop:1000", "suppress:1000", "cleanup:1000", "cleanup:1000"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn conflicting_or_tampered_package_record_fails_closed() {
        let (store, root) = store("doc-sum-package-conflict");
        let mut effects = MockEffects::default();
        prepare(&store, PackageKind::Upgrade, "0.0.9", "0.1.0", &mut effects).unwrap();
        assert!(matches!(
            finish(
                &store,
                PackageKind::Remove,
                "0.1.0",
                &mut effects,
                None,
                None
            ),
            Err(PackageControlError::Conflict)
        ));
        fs::write(store.record_path(), b"{}\n").unwrap();
        assert!(matches!(store.read(), Err(PackageControlError::Conflict)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn logged_out_enablement_requires_the_exact_systemd_link() {
        use std::os::unix::fs::symlink;

        let home = env::temp_dir().join(format!("doc-sum-package-home-{}", Uuid::new_v4()));
        let wants = home.join(".config/systemd/user/default.target.wants");
        fs::create_dir_all(&wants).unwrap();
        let link = wants.join("document-summarizer-connect.service");
        symlink(
            "/usr/lib/systemd/user/document-summarizer-connect.service",
            &link,
        )
        .unwrap();
        assert_eq!(exact_enablement_link(&home).unwrap(), Some(link.clone()));
        fs::remove_file(&link).unwrap();
        symlink("/tmp/foreign.service", &link).unwrap();
        assert!(matches!(
            exact_enablement_link(&home),
            Err(PackageControlError::Conflict)
        ));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn production_package_authority_is_fixed_and_not_environment_selected() {
        assert_eq!(PackageStore::production().root, Path::new(PACKAGE_ROOT));
    }

    #[test]
    fn discovery_uses_owner_authenticated_receipt_with_exact_custom_paths() {
        let (store, root) = store("doc-sum-package-participant-receipt");
        let uid = unsafe { libc::geteuid() };
        let runtime = root.join("custom-runtime/docsum");
        let app_data = root.join("custom-data/docsum");
        let control = root.join("custom-config/docsum");
        let manager_parent = root.join("custom-config/systemd/user/default.target.wants");
        for path in [&runtime, &app_data, &control] {
            fs::create_dir_all(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::create_dir_all(&manager_parent).unwrap();
        fs::set_permissions(&manager_parent, fs::Permissions::from_mode(0o755)).unwrap();
        let runtime_metadata = fs::metadata(&runtime).unwrap();
        let app_data_metadata = fs::metadata(&app_data).unwrap();
        let control_metadata = fs::metadata(&control).unwrap();
        let manager_metadata = fs::metadata(&manager_parent).unwrap();
        let acknowledgement = ParticipantAcknowledgement {
            format_version: FORMAT_VERSION,
            package_id: PACKAGE_ID.to_string(),
            uid,
            user: nss_user_name(uid).unwrap(),
            generation: Uuid::new_v4().to_string(),
            runtime_root: runtime.to_string_lossy().into_owned(),
            runtime_device: runtime_metadata.dev(),
            runtime_inode: runtime_metadata.ino(),
            app_data_root: app_data.to_string_lossy().into_owned(),
            app_data_device: app_data_metadata.dev(),
            app_data_inode: app_data_metadata.ino(),
            control_root: control.to_string_lossy().into_owned(),
            control_device: control_metadata.dev(),
            control_inode: control_metadata.ino(),
            manager_link: manager_parent
                .join("document-summarizer-connect.service")
                .to_string_lossy()
                .into_owned(),
            manager_parent_device: manager_metadata.dev(),
            manager_parent_inode: manager_metadata.ino(),
            enabled: true,
        };
        let path = root
            .join(PARTICIPANTS_DIRECTORY)
            .join(format!("{uid}.json"));
        fs::write(&path, serde_json::to_vec(&acknowledgement).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let mut effects = SystemEffects {
            package_root: root.clone(),
        };
        let participants = effects.discover().unwrap();
        assert_eq!(participants.len(), 1);
        assert_eq!(participants[0].runtime_root, runtime.to_string_lossy());
        assert_eq!(participants[0].app_data_root, app_data.to_string_lossy());
        assert_eq!(participants[0].control_root, control.to_string_lossy());
        fs::rename(&runtime, root.join("old-runtime-inode")).unwrap();
        assert_eq!(effects.discover().unwrap().len(), 1);
        fs::create_dir_all(&runtime).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(
            effects.discover(),
            Err(PackageControlError::Conflict)
        ));
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_admission_blocks_package_mutation_and_record_blocks_admission() {
        let (store, root) = store("doc-sum-package-lock-order");
        let admission = enter_admission_store(&store).unwrap();
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(store.root.join(LOCK_FILE))
            .unwrap();
        assert!(matches!(
            competing.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(admission);
        drop(competing);
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(store.root.join(LOCK_FILE))
            .unwrap();
        competing.lock().unwrap();
        drop(competing);

        let mut effects = MockEffects::default();
        prepare(&store, PackageKind::Upgrade, "0.0.9", "0.1.0", &mut effects).unwrap();
        assert!(enter_admission_store(&store).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn package_then_user_authority_lock_order_blocks_concurrent_user_control() {
        let root = env::temp_dir().join(format!("doc-sum-user-authority-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        for name in [
            ".background-control-v1.lock",
            ".background-admission-v1.lock",
        ] {
            let path = root.join(name);
            fs::write(&path, b"").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut recorded = participant(unsafe { libc::geteuid() }, true);
        recorded.control_root = root.to_string_lossy().into_owned();
        let guards = lock_participant_authorities(&[recorded], true).unwrap();
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join(".background-control-v1.lock"))
            .unwrap();
        assert!(matches!(
            competing.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(guards);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn logged_out_manager_mutation_is_bound_to_the_recorded_directory_inode() {
        let root = env::temp_dir().join(format!("doc-sum-manager-dirfd-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        let metadata = fs::metadata(&root).unwrap();
        let mut recorded = participant(unsafe { libc::geteuid() }, true);
        recorded.manager_link = root
            .join("document-summarizer-connect.service")
            .to_string_lossy()
            .into_owned();
        recorded.manager_parent_device = metadata.dev();
        recorded.manager_parent_inode = metadata.ino();
        create_participant_enablement_link(&recorded).unwrap();
        assert_eq!(
            fs::read_link(&recorded.manager_link).unwrap(),
            Path::new("/usr/lib/systemd/user/document-summarizer-connect.service")
        );
        remove_participant_enablement_link(&recorded).unwrap();
        assert!(!Path::new(&recorded.manager_link).exists());
        std::os::unix::fs::symlink("/tmp/foreign.service", &recorded.manager_link).unwrap();
        assert!(matches!(
            remove_participant_enablement_link(&recorded),
            Err(PackageControlError::Conflict)
        ));
        assert_eq!(
            fs::read_link(&recorded.manager_link).unwrap(),
            Path::new("/tmp/foreign.service")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_exact_pending_successor_may_start_while_jobs_remain_barred() {
        let (store, root) = store("doc-sum-package-successor-handoff");
        let app_data = root.join("custom-data");
        let runtime = root.join("custom-runtime");
        let control = root.join("custom-control");
        let manager_parent = root.join("manager/default.target.wants");
        for path in [&app_data, &runtime, &control] {
            fs::create_dir_all(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::create_dir_all(&manager_parent).unwrap();
        fs::set_permissions(&manager_parent, fs::Permissions::from_mode(0o755)).unwrap();
        let mut recorded = participant(unsafe { libc::geteuid() }, true);
        recorded.app_data_root = app_data.to_string_lossy().into_owned();
        let app_data_metadata = fs::metadata(&app_data).unwrap();
        recorded.app_data_device = app_data_metadata.dev();
        recorded.app_data_inode = app_data_metadata.ino();
        recorded.runtime_root = runtime.to_string_lossy().into_owned();
        let runtime_metadata = fs::metadata(&runtime).unwrap();
        recorded.runtime_device = Some(runtime_metadata.dev());
        recorded.runtime_inode = Some(runtime_metadata.ino());
        recorded.control_root = control.to_string_lossy().into_owned();
        let control_metadata = fs::metadata(&control).unwrap();
        recorded.control_device = control_metadata.dev();
        recorded.control_inode = control_metadata.ino();
        recorded.manager_link = manager_parent
            .join("document-summarizer-connect.service")
            .to_string_lossy()
            .into_owned();
        let manager_metadata = fs::metadata(&manager_parent).unwrap();
        recorded.manager_parent_device = manager_metadata.dev();
        recorded.manager_parent_inode = manager_metadata.ino();
        let record = PackageRecord {
            format_version: FORMAT_VERSION,
            package_id: PACKAGE_ID.to_string(),
            generation: Uuid::new_v4().to_string(),
            kind: PackageKind::Upgrade,
            phase: PackagePhase::Settling,
            source_version: "0.1.0".to_string(),
            target_version: "0.2.0".to_string(),
            predecessor_removal_generation: None,
            participants: vec![recorded],
        };
        store.write(&record).unwrap();
        let startup =
            enter_startup_admission_store(&store, unsafe { libc::geteuid() }, &app_data, &runtime)
                .unwrap();
        drop(startup);
        assert!(enter_admission_store(&store).is_err());
        assert!(enter_startup_admission_store(
            &store,
            unsafe { libc::geteuid() },
            &root.join("wrong-data"),
            &runtime,
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn settlement_is_durable_across_restore_failure_and_logged_out_enablement_is_deferred() {
        let (store, root) = store("doc-sum-package-settlement");
        let mut effects = MockEffects {
            participants: vec![participant(1000, true)],
            fail_restore_once: true,
            defer_enabled: true,
            ..Default::default()
        };
        prepare(&store, PackageKind::Upgrade, "0.0.9", "0.1.0", &mut effects).unwrap();
        assert!(matches!(
            finish(
                &store,
                PackageKind::Upgrade,
                "0.1.0",
                &mut effects,
                None,
                None
            ),
            Err(PackageControlError::Manager)
        ));
        let interrupted = store.read().unwrap().unwrap();
        assert_eq!(interrupted.phase, PackagePhase::Settling);
        assert_eq!(
            interrupted.participants[0].settlement,
            ParticipantSettlement::Pending
        );
        finish(
            &store,
            PackageKind::Upgrade,
            "0.1.0",
            &mut effects,
            None,
            None,
        )
        .unwrap();
        assert!(store.read().unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finish_releases_quiesce_before_successor_start_and_reacquires_in_order() {
        let (store, root) = store("doc-sum-package-successor-flock-handoff");
        let mut effects = MockEffects {
            participants: vec![participant(1000, true)],
            restore_flock_paths: Some((root.join(QUIESCE_LOCK_FILE), root.join(LOCK_FILE))),
            ..Default::default()
        };
        prepare(&store, PackageKind::Upgrade, "0.0.9", "0.1.0", &mut effects).unwrap();
        let quiesce = store.quiesce_lock(true).unwrap();
        let package = store.lock().unwrap();

        finish(
            &store,
            PackageKind::Upgrade,
            "0.1.0",
            &mut effects,
            Some(&quiesce),
            Some(&package),
        )
        .unwrap();

        assert!(quiesce.metadata().is_ok());
        assert!(store.read().unwrap().is_none());
        let calls = effects.calls.lock().unwrap();
        let restore = calls
            .iter()
            .position(|call| call == "restore:1000:true")
            .unwrap();
        let validate = calls
            .iter()
            .position(|call| call.starts_with("validate:1000:"))
            .unwrap();
        assert_eq!(validate, restore + 1);
        drop(calls);
        drop(package);
        drop(quiesce);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn successor_child_lock_wait_times_out_while_startup_authority_is_held() {
        let (store, root) = store("doc-sum-package-successor-flock-timeout");
        let _quiesce = store.quiesce_lock(true).unwrap();
        let _package = store.lock().unwrap();
        let started = Instant::now();
        assert!(matches!(
            bounded_command(
                Command::new("flock")
                    .arg("-s")
                    .arg(root.join(QUIESCE_LOCK_FILE))
                    .arg("flock")
                    .arg("-s")
                    .arg(root.join(LOCK_FILE))
                    .arg("true"),
                Duration::from_millis(150),
            ),
            Err(PackageControlError::Manager)
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(_package);
        drop(_quiesce);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finish_rejects_record_change_while_successor_starts_before_settlement() {
        let (store, root) = store("doc-sum-package-successor-record-recheck");
        let mut effects = MockEffects {
            participants: vec![participant(1000, true)],
            restore_record_path: Some(store.record_path()),
            ..Default::default()
        };
        prepare(&store, PackageKind::Upgrade, "0.0.9", "0.1.0", &mut effects).unwrap();
        let quiesce = store.quiesce_lock(true).unwrap();
        let package = store.lock().unwrap();

        assert!(matches!(
            finish(
                &store,
                PackageKind::Upgrade,
                "0.1.0",
                &mut effects,
                Some(&quiesce),
                Some(&package),
            ),
            Err(PackageControlError::Conflict)
        ));
        assert!(!effects
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call.starts_with("validate:")));
        assert_eq!(
            store.read().unwrap().unwrap().participants[0].settlement,
            ParticipantSettlement::Pending
        );
        drop(package);
        drop(quiesce);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bootstrap_adoption_resumes_after_each_durable_cleanup_boundary() {
        fn bootstrap_intent(phase: BootstrapPhase, bytes: &[u8]) -> QuiesceIntent {
            QuiesceIntent {
                generation: Uuid::new_v4().to_string(),
                kind: PackageKind::Upgrade,
                source_version: "0.0.9".to_string(),
                target_version: "0.1.0".to_string(),
                bootstrap: Some(BootstrapQuiesce {
                    phase,
                    legacy_device: 11,
                    legacy_inode: 12,
                    legacy_sha256: format!("{:x}", Sha256::digest(bytes)),
                }),
            }
        }

        let bytes = b"legacy executable fixture";
        let (store_one, root_one) = store("doc-sum-bootstrap-adopted-before-cleanup");
        let intent = bootstrap_intent(BootstrapPhase::Adopted, bytes);
        let quarantine = store_one
            .bootstrap_quarantine_path(&intent.generation)
            .unwrap();
        fs::write(&quarantine, bytes).unwrap();
        fs::set_permissions(&quarantine, fs::Permissions::from_mode(0o600)).unwrap();
        store_one.write_quiesce(&intent).unwrap();
        adopt_bootstrap(&store_one, &intent, &mut MockEffects::default()).unwrap();
        assert!(!quarantine.exists());
        assert!(store_one.read_quiesce().unwrap().is_none());
        assert_eq!(
            store_one.read().unwrap().unwrap().phase,
            PackagePhase::PublishersStopped
        );
        fs::remove_dir_all(root_one).unwrap();

        let (store_two, root_two) = store("doc-sum-bootstrap-adopted-after-cleanup");
        let intent = bootstrap_intent(BootstrapPhase::Adopted, bytes);
        store_two.write_quiesce(&intent).unwrap();
        adopt_bootstrap(&store_two, &intent, &mut MockEffects::default()).unwrap();
        assert!(store_two.read_quiesce().unwrap().is_none());
        assert_eq!(
            store_two.read().unwrap().unwrap().generation,
            intent.generation
        );
        fs::remove_dir_all(root_two).unwrap();

        let (store_three, root_three) = store("doc-sum-bootstrap-tampered-quarantine");
        let intent = bootstrap_intent(BootstrapPhase::Quiesced, bytes);
        let quarantine = store_three
            .bootstrap_quarantine_path(&intent.generation)
            .unwrap();
        fs::write(&quarantine, b"tampered executable").unwrap();
        fs::set_permissions(&quarantine, fs::Permissions::from_mode(0o600)).unwrap();
        store_three.write_quiesce(&intent).unwrap();
        assert!(matches!(
            adopt_bootstrap(&store_three, &intent, &mut MockEffects::default()),
            Err(PackageControlError::Conflict)
        ));
        assert_eq!(store_three.read_quiesce().unwrap(), Some(intent));
        assert!(quarantine.exists());
        fs::remove_dir_all(root_three).unwrap();
    }

    #[test]
    fn remove_then_reinstall_uses_a_new_generation_and_preserves_choices_in_receipt() {
        let (store, root) = store("doc-sum-package-reinstall");
        let mut effects = MockEffects {
            participants: vec![participant(1000, true), participant(1001, false)],
            ..Default::default()
        };
        prepare(&store, PackageKind::Remove, "0.1.0", "0.1.0", &mut effects).unwrap();
        let removal_generation = store.read().unwrap().unwrap().generation;
        let controller = root.join(format!("package-controller-{removal_generation}"));
        fs::write(&controller, b"controller").unwrap();
        fs::set_permissions(&controller, fs::Permissions::from_mode(0o700)).unwrap();
        finish(
            &store,
            PackageKind::Remove,
            "0.1.0",
            &mut effects,
            None,
            None,
        )
        .unwrap();
        assert!(store.removal_receipt_path().exists());

        prepare_reinstall(&store, "0.2.0").unwrap();
        let reinstall = store.read().unwrap().unwrap();
        assert_ne!(reinstall.generation, removal_generation);
        assert_eq!(
            reinstall.predecessor_removal_generation.as_deref(),
            Some(removal_generation.as_str())
        );
        assert!(enter_admission_store(&store).is_err());
        finish(
            &store,
            PackageKind::Reinstall,
            "0.2.0",
            &mut effects,
            None,
            None,
        )
        .unwrap();
        assert!(!store.removal_receipt_path().exists());
        let installed: InstallReceipt = read_json_record(&store.install_receipt_path())
            .unwrap()
            .unwrap();
        assert_eq!(installed.predecessor_removal_generation, removal_generation);
        assert_eq!(
            installed
                .participants
                .iter()
                .map(|participant| participant.enabled)
                .collect::<Vec<_>>(),
            [true, false]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reinstall_resumes_controller_adoption_after_record_persistence_crash() {
        let (store, root) = store("doc-sum-package-reinstall-controller-resume");
        let mut effects = MockEffects {
            participants: vec![participant(1000, true)],
            ..Default::default()
        };
        prepare(&store, PackageKind::Remove, "0.1.0", "0.1.0", &mut effects).unwrap();
        let removal = store.read().unwrap().unwrap();
        let predecessor = root.join(format!("package-controller-{}", removal.generation));
        fs::write(&predecessor, b"controller").unwrap();
        fs::set_permissions(&predecessor, fs::Permissions::from_mode(0o700)).unwrap();
        finish(
            &store,
            PackageKind::Remove,
            "0.1.0",
            &mut effects,
            None,
            None,
        )
        .unwrap();

        let receipt = store.read_removal_receipt().unwrap().unwrap();
        let interrupted = PackageRecord {
            format_version: FORMAT_VERSION,
            package_id: PACKAGE_ID.to_string(),
            generation: Uuid::new_v4().to_string(),
            kind: PackageKind::Reinstall,
            phase: PackagePhase::PublishersStopped,
            source_version: receipt.removed_version.clone(),
            target_version: "0.2.0".to_string(),
            predecessor_removal_generation: Some(receipt.producer_generation.clone()),
            participants: receipt
                .participants
                .iter()
                .cloned()
                .map(|mut participant| {
                    participant.settlement = ParticipantSettlement::Pending;
                    participant
                })
                .collect(),
        };
        store.write(&interrupted).unwrap();

        prepare_reinstall(&store, "0.2.0").unwrap();
        assert!(!predecessor.exists());
        assert!(root
            .join(format!("package-controller-{}", interrupted.generation))
            .exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepare_reinstall_action_completes_an_interrupted_removal() {
        let (store, root) = store("doc-sum-package-reinstall-action-resume");
        let mut effects = MockEffects {
            participants: vec![participant(1000, true)],
            ..Default::default()
        };
        prepare(&store, PackageKind::Remove, "0.1.0", "0.1.0", &mut effects).unwrap();
        let removal = store.read().unwrap().unwrap();
        let controller = root.join(format!("package-controller-{}", removal.generation));
        fs::write(&controller, b"controller").unwrap();
        fs::set_permissions(&controller, fs::Permissions::from_mode(0o700)).unwrap();

        run_with(
            &store,
            PackageAction::PrepareReinstall { target: "0.2.0" },
            &mut effects,
            None,
            None,
        )
        .unwrap();
        let reinstall = store.read().unwrap().unwrap();
        assert_eq!(reinstall.kind, PackageKind::Reinstall);
        assert_eq!(reinstall.target_version, "0.2.0");
        assert_eq!(
            reinstall.predecessor_removal_generation.as_deref(),
            Some(removal.generation.as_str())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reinstall_resumes_after_successor_receipt_and_controller_retirement() {
        let (store, root) = store("doc-sum-package-reinstall-finalize-resume");
        let mut effects = MockEffects {
            participants: vec![participant(1000, true)],
            controller_root: Some(root.clone()),
            ..Default::default()
        };
        prepare(&store, PackageKind::Remove, "0.1.0", "0.1.0", &mut effects).unwrap();
        let removal = store.read().unwrap().unwrap();
        let predecessor = root.join(format!("package-controller-{}", removal.generation));
        fs::write(&predecessor, b"controller").unwrap();
        fs::set_permissions(&predecessor, fs::Permissions::from_mode(0o700)).unwrap();
        finish(
            &store,
            PackageKind::Remove,
            "0.1.0",
            &mut effects,
            None,
            None,
        )
        .unwrap();
        prepare_reinstall(&store, "0.2.0").unwrap();

        let mut interrupted = store.read().unwrap().unwrap();
        interrupted.phase = PackagePhase::Settling;
        interrupted.participants[0].settlement = ParticipantSettlement::SuccessorReady;
        store.write(&interrupted).unwrap();
        write_install_receipt(&store, &interrupted).unwrap();
        fs::remove_file(root.join(format!("package-controller-{}", interrupted.generation)))
            .unwrap();

        finish(
            &store,
            PackageKind::Reinstall,
            "0.2.0",
            &mut effects,
            None,
            None,
        )
        .unwrap();
        assert!(store.read().unwrap().is_none());
        assert!(!store.removal_receipt_path().exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quiesce_intent_bars_admission_before_package_exclusive_and_replays_exact_generation() {
        let (store, root) = store("doc-sum-package-quiesce");
        let generation = Uuid::new_v4().to_string();
        let intent = ensure_quiesce_intent(
            &store,
            PackageKind::Upgrade,
            "0.0.9",
            "0.1.0",
            Some(&generation),
        )
        .unwrap();
        assert!(enter_admission_store(&store).is_err());
        let mut effects = MockEffects::default();
        prepare_from_intent(&store, &intent, &mut effects).unwrap();
        let record = store.read().unwrap().unwrap();
        assert_eq!(record.generation, generation);
        assert_eq!(record.phase, PackagePhase::PublishersStopped);
        let package = store.lock().unwrap();
        store.clear_quiesce(&generation).unwrap();
        drop(package);
        assert!(enter_admission_store(&store).is_err());
        fs::write(store.quiesce_path(), b"tampered\n").unwrap();
        fs::set_permissions(store.quiesce_path(), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            store.read_quiesce(),
            Err(PackageControlError::Conflict)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn absent_runtime_is_refreshed_only_before_exact_generation_settlement() {
        let (store, root) = store("doc-sum-runtime-refresh");
        let mut missing = participant(1000, true);
        missing.runtime_device = None;
        missing.runtime_inode = None;
        let intent = QuiesceIntent {
            generation: Uuid::new_v4().to_string(),
            kind: PackageKind::Upgrade,
            source_version: "0.0.9".to_string(),
            target_version: "0.1.0".to_string(),
            bootstrap: None,
        };
        store.write_quiesce(&intent).unwrap();
        let mut effects = MockEffects {
            participants: vec![missing],
            refreshed_runtime: Some((41, 42)),
            ..MockEffects::default()
        };
        prepare_from_intent(&store, &intent, &mut effects).unwrap();
        let record = store.read().unwrap().unwrap();
        assert_eq!(record.participants[0].runtime_device, Some(41));
        assert_eq!(record.participants[0].runtime_inode, Some(42));
        assert_eq!(
            effects.calls.lock().unwrap().first().map(String::as_str),
            Some("refresh:1000")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unproven_runtime_path_replacement_is_never_accepted() {
        let root = env::temp_dir().join(format!("doc-sum-runtime-spoof-{}", Uuid::new_v4()));
        let runtime = root.join("runtime");
        let app_data = root.join("data");
        let control = root.join("control");
        let manager = root.join("manager");
        for path in [&runtime, &app_data, &control] {
            fs::create_dir_all(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::create_dir_all(&manager).unwrap();
        fs::set_permissions(&manager, fs::Permissions::from_mode(0o755)).unwrap();
        let uid = unsafe { libc::geteuid() };
        let mut recorded = participant(uid, true);
        recorded.runtime_root = runtime.to_string_lossy().into_owned();
        recorded.runtime_device = None;
        recorded.runtime_inode = None;
        recorded.app_data_root = app_data.to_string_lossy().into_owned();
        let metadata = fs::metadata(&app_data).unwrap();
        recorded.app_data_device = metadata.dev();
        recorded.app_data_inode = metadata.ino();
        recorded.control_root = control.to_string_lossy().into_owned();
        let metadata = fs::metadata(&control).unwrap();
        recorded.control_device = metadata.dev();
        recorded.control_inode = metadata.ino();
        recorded.manager_link = manager
            .join("document-summarizer-connect.service")
            .to_string_lossy()
            .into_owned();
        let metadata = fs::metadata(&manager).unwrap();
        recorded.manager_parent_device = metadata.dev();
        recorded.manager_parent_inode = metadata.ino();
        assert!(matches!(
            validate_participant_directories(&recorded),
            Err(PackageControlError::Conflict)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn login_runtime_refresh_requires_matching_system_and_user_manager_proofs() {
        let recorded = participant(1000, true);
        let loginctl = format!("{}\n", recorded.runtime_root);
        let manager = format!("LANG=C\nXDG_RUNTIME_DIR={}\n", recorded.runtime_root);
        assert!(runtime_session_proof_matches(
            &recorded,
            loginctl.as_bytes(),
            manager.as_bytes(),
        ));
        assert!(!runtime_session_proof_matches(
            &recorded,
            b"/run/user/9999\n",
            manager.as_bytes(),
        ));
        assert!(!runtime_session_proof_matches(
            &recorded,
            loginctl.as_bytes(),
            b"XDG_RUNTIME_DIR=/run/user/9999\n",
        ));
    }

    #[test]
    fn preinst_bootstrap_is_cryptographically_adoptable_and_tamper_closed() {
        let root = env::temp_dir().join(format!("doc-sum-preinst-bootstrap-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let package_root = root.join("package-root");
        let installed_binary = root.join("document-summarizer");
        fs::copy("/bin/true", &installed_binary).unwrap();
        fs::set_permissions(&installed_binary, fs::Permissions::from_mode(0o755)).unwrap();
        let runtime_root = root.join("run-user");
        fs::create_dir(&runtime_root).unwrap();
        let script = include_str!("../../linux/debian/preinst")
            .replace(
                "/var/lib/document-summarizer",
                package_root.to_str().unwrap(),
            )
            .replace(
                "/usr/bin/document-summarizer",
                installed_binary.to_str().unwrap(),
            )
            .replace("/run/user", runtime_root.to_str().unwrap())
            .replace(
                "expected_root_uid=0",
                &format!("expected_root_uid={}", unsafe { libc::geteuid() }),
            )
            .replace(" -o root -g root", "");
        let script_path = root.join("preinst");
        fs::write(&script_path, script).unwrap();
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Command::new("sh")
            .arg(&script_path)
            .args(["upgrade", "0.0.9"])
            .status()
            .unwrap()
            .success());
        let store = PackageStore::for_test(package_root.clone());
        let intent = store.read_quiesce().unwrap().unwrap();
        assert_eq!(intent.kind, PackageKind::Upgrade);
        assert_eq!(intent.source_version, "0.0.9");
        assert_eq!(intent.target_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(
            intent.bootstrap.as_ref().map(|state| state.phase),
            Some(BootstrapPhase::Quiesced)
        );
        let quarantine = store.bootstrap_quarantine_path(&intent.generation).unwrap();
        assert!(quarantine.is_file());
        let path = store.quiesce_path();
        let original = fs::read(&path).unwrap();
        assert!(Command::new("sh")
            .arg(&script_path)
            .args(["upgrade", "0.0.9"])
            .status()
            .unwrap()
            .success());
        assert_eq!(fs::read(&path).unwrap(), original);
        let mut effects = MockEffects::default();
        prepare_from_intent(&store, &intent, &mut effects).unwrap();
        prepare_from_intent(&store, &intent, &mut effects).unwrap();
        let record = store.read().unwrap().unwrap();
        assert_eq!(record.generation, intent.generation);
        assert_eq!(record.phase, PackagePhase::PublishersStopped);

        let mut bytes = original.clone();
        let source = bytes
            .windows(b"source_version=0.0.9".len())
            .position(|window| window == b"source_version=0.0.9")
            .unwrap();
        bytes[source + "source_version=".len()] = b'9';
        fs::write(&path, bytes).unwrap();
        assert!(!Command::new("sh")
            .arg(&script_path)
            .args(["upgrade", "0.0.9"])
            .status()
            .unwrap()
            .success());
        assert!(matches!(
            store.read_quiesce(),
            Err(PackageControlError::Conflict)
        ));
        fs::write(&path, original).unwrap();
        let mut effects = MockEffects::default();
        adopt_bootstrap(&store, &intent, &mut effects).unwrap();
        assert!(store.read_quiesce().unwrap().is_none());
        assert!(!quarantine.exists());
        assert_eq!(
            store.read().unwrap().unwrap().phase,
            PackagePhase::PublishersStopped
        );
        fs::remove_dir_all(root).unwrap();
    }
}
