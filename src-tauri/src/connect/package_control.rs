use serde::{Deserialize, Serialize};
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
const PARTICIPANTS_DIRECTORY: &str = "participants-v1";
const REMOVAL_RECEIPT_FILE: &str = "package-removal-receipt-v1.json";
const INSTALL_RECEIPT_FILE: &str = "package-install-receipt-v1.json";
const MAX_RECORD_BYTES: u64 = 256 * 1024;

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

pub(crate) enum PackageAction<'a> {
    Initialize,
    PrepareUpgrade { source: &'a str, target: &'a str },
    FinishUpgrade { target: &'a str },
    PrepareRemove { target: &'a str },
    FinishRemove { target: &'a str },
    RecoverInstall { target: &'a str },
    PrepareReinstall { target: &'a str },
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
    fn cleanup(&mut self, participant: &Participant) -> Result<(), PackageControlError>;
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
    let lock = store.lock()?;
    let mut effects = SystemEffects {
        package_root: store.root.clone(),
    };
    run_with(&store, action, &mut effects, Some(&lock))
}

fn run_with(
    store: &PackageStore,
    action: PackageAction<'_>,
    effects: &mut dyn PackageEffects,
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
        PackageAction::FinishUpgrade { target } => {
            finish(store, PackageKind::Upgrade, target, effects, package_lock)
        }
        PackageAction::FinishRemove { target } => {
            finish(store, PackageKind::Remove, target, effects, package_lock)
        }
        PackageAction::RecoverInstall { target } => {
            let record = store.read()?.ok_or(PackageControlError::Conflict)?;
            match record.kind {
                PackageKind::Upgrade => {
                    finish(store, PackageKind::Upgrade, target, effects, package_lock)
                }
                PackageKind::Reinstall => {
                    finish(store, PackageKind::Reinstall, target, effects, package_lock)
                }
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
                            package_lock,
                        )?;
                    }
                    PackageKind::Reinstall => {}
                    PackageKind::Upgrade => return Err(PackageControlError::Conflict),
                }
            }
            prepare_reinstall(store, target)
        }
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
            let participant = record.participants[index].clone();
            effects.cleanup(&participant)?;
            if matches!(kind, PackageKind::Upgrade | PackageKind::Reinstall) {
                drop(participant_authorities);
                if let Some(lock) = package_lock {
                    File::unlock(lock).map_err(|_| PackageControlError::Storage)?;
                }
                let restored = effects.restore(&participant, &record.generation);
                if let Some(lock) = package_lock {
                    lock.lock().map_err(|_| PackageControlError::Storage)?;
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
                record.participants[index].settlement = restored?;
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
    {
        return Err(PackageControlError::Conflict);
    }
    Ok(())
}

pub(crate) struct PackageAdmissionGuard {
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
        drop(store.lock()?);
    }
    enter_startup_admission_store(
        &store,
        unsafe { libc::geteuid() },
        app_data_root,
        runtime_root,
    )
}

fn enter_admission_store(
    store: &PackageStore,
) -> Result<PackageAdmissionGuard, PackageControlError> {
    let lock = store.lock_shared()?;
    if let Some(record) = store.read()? {
        validate(&record, record.kind, &record.target_version)?;
        return Err(PackageControlError::Conflict);
    }
    Ok(PackageAdmissionGuard { _lock: lock })
}

fn enter_startup_admission_store(
    store: &PackageStore,
    uid: u32,
    app_data_root: &Path,
    runtime_root: &Path,
) -> Result<PackageAdmissionGuard, PackageControlError> {
    let lock = store.lock_shared()?;
    let Some(record) = store.read()? else {
        return Ok(PackageAdmissionGuard { _lock: lock });
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
    if runtime.dev() != participant.runtime_device || runtime.ino() != participant.runtime_inode {
        return Err(PackageControlError::Conflict);
    }
    Ok(PackageAdmissionGuard { _lock: lock })
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
                runtime_device: acknowledgement.runtime_device,
                runtime_inode: acknowledgement.runtime_inode,
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

fn validate_participant_directories(participant: &Participant) -> Result<(), PackageControlError> {
    let matches = |path: &str, device: u64, inode: u64, private: bool| {
        safe_directory_identity(Path::new(path), participant.uid, private)
            .is_ok_and(|metadata| metadata.dev() == device && metadata.ino() == inode)
    };
    let manager_parent = Path::new(&participant.manager_link)
        .parent()
        .ok_or(PackageControlError::Conflict)?;
    let runtime_matches_or_is_absent = match fs::symlink_metadata(&participant.runtime_root) {
        Ok(_) => matches(
            &participant.runtime_root,
            participant.runtime_device,
            participant.runtime_inode,
            true,
        ),
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
        drop(store.lock().unwrap());
        (store, root)
    }

    fn participant(uid: u32, enabled: bool) -> Participant {
        Participant {
            uid,
            user: format!("user-{uid}"),
            runtime_root: format!("/run/user/{uid}"),
            runtime_device: 0,
            runtime_inode: 0,
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
        finish(&store, PackageKind::Upgrade, "0.1.0", &mut effects, None).unwrap();
        assert!(store.read().unwrap().is_none());
        assert_eq!(
            effects.calls.lock().unwrap().as_slice(),
            [
                "stop:1000",
                "suppress:1000",
                "cleanup:1000",
                "stop:1001",
                "suppress:1001",
                "cleanup:1001",
                "cleanup:1000",
                "restore:1000:true",
                "cleanup:1001",
                "restore:1001:false"
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
        finish(&store, PackageKind::Remove, "0.1.0", &mut effects, None).unwrap();
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
            finish(&store, PackageKind::Remove, "0.1.0", &mut effects, None),
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
        recorded.runtime_device = runtime_metadata.dev();
        recorded.runtime_inode = runtime_metadata.ino();
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
            finish(&store, PackageKind::Upgrade, "0.1.0", &mut effects, None),
            Err(PackageControlError::Manager)
        ));
        let interrupted = store.read().unwrap().unwrap();
        assert_eq!(interrupted.phase, PackagePhase::Settling);
        assert_eq!(
            interrupted.participants[0].settlement,
            ParticipantSettlement::Pending
        );
        finish(&store, PackageKind::Upgrade, "0.1.0", &mut effects, None).unwrap();
        assert!(store.read().unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
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
        finish(&store, PackageKind::Remove, "0.1.0", &mut effects, None).unwrap();
        assert!(store.removal_receipt_path().exists());

        prepare_reinstall(&store, "0.2.0").unwrap();
        let reinstall = store.read().unwrap().unwrap();
        assert_ne!(reinstall.generation, removal_generation);
        assert_eq!(
            reinstall.predecessor_removal_generation.as_deref(),
            Some(removal_generation.as_str())
        );
        assert!(enter_admission_store(&store).is_err());
        finish(&store, PackageKind::Reinstall, "0.2.0", &mut effects, None).unwrap();
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
        finish(&store, PackageKind::Remove, "0.1.0", &mut effects, None).unwrap();

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
        finish(&store, PackageKind::Remove, "0.1.0", &mut effects, None).unwrap();
        prepare_reinstall(&store, "0.2.0").unwrap();

        let mut interrupted = store.read().unwrap().unwrap();
        interrupted.phase = PackagePhase::Settling;
        interrupted.participants[0].settlement = ParticipantSettlement::SuccessorReady;
        store.write(&interrupted).unwrap();
        write_install_receipt(&store, &interrupted).unwrap();
        fs::remove_file(root.join(format!("package-controller-{}", interrupted.generation)))
            .unwrap();

        finish(&store, PackageKind::Reinstall, "0.2.0", &mut effects, None).unwrap();
        assert!(store.read().unwrap().is_none());
        assert!(!store.removal_receipt_path().exists());
        fs::remove_dir_all(root).unwrap();
    }
}
