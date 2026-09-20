#![cfg(target_os = "linux")]

use serde::{Deserialize, Serialize};
use std::env;
use std::fs::TryLockError;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

const APP_DIRECTORY: &str = "com.juan-canfield.docsum";
const PACKAGE_ID: &str = "document-summarizer-deb";
const FORMAT_VERSION: u32 = 1;
const MAX_RECORD_BYTES: u64 = 32 * 1024;
const TRANSITION_FILE: &str = "background-mode-transition-v1.json";
const CONTROL_LOCK_FILE: &str = ".background-control-v1.lock";
const ADMISSION_LOCK_FILE: &str = ".background-admission-v1.lock";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TransitionKind {
    Enable,
    Disable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum TransitionDisposition {
    Forward,
    Rollback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum TransitionPhase {
    IntentRecorded,
    SourceStopped,
    ManagerEnabled,
    SuccessorReady,
    PublisherStopped,
    ManagerDisabled,
    RollbackTargetStopped,
    PriorChoiceRestored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateIdentity {
    path: String,
    device: u64,
    inode: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BackgroundTransition {
    format_version: u32,
    package_id: String,
    package_version: String,
    artifact_scope: String,
    pub(crate) generation: String,
    pub(crate) kind: TransitionKind,
    disposition: TransitionDisposition,
    phase: TransitionPhase,
    prior_enabled: bool,
    target_enabled: bool,
    state: StateIdentity,
    expected_v2_instance_id: Option<String>,
    created_unix_ms: u64,
    graceful_deadline_unix_ms: u64,
    force_deadline_unix_ms: u64,
    control_deadline_unix_ms: u64,
}

#[derive(Debug, Error)]
pub(crate) enum LifecycleControlError {
    #[error("Connect background lifecycle storage is unavailable")]
    Storage,
    #[error("Connect background lifecycle record is invalid")]
    InvalidRecord,
    #[error("A different Connect background lifecycle operation is incomplete")]
    ConflictingTransition,
    #[error("Connect background lifecycle manager operation failed")]
    Manager,
    #[error("Connect background lifecycle readiness could not be proven")]
    Readiness,
    #[cfg(test)]
    #[error("Connect background lifecycle operation was interrupted")]
    Interrupted,
}

pub(crate) trait TransitionEffects {
    fn manager_enabled(&mut self) -> Result<bool, LifecycleControlError>;
    fn stop_publishers(
        &mut self,
        transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError>;
    fn enable_and_start(
        &mut self,
        transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError>;
    fn wait_until_ready(
        &mut self,
        transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError>;
    fn disable_and_stop(
        &mut self,
        transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError>;
    fn restore_prior_choice(
        &mut self,
        transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError>;
    fn before_clear(
        &mut self,
        transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError>;
}

trait TransitionProbe {
    fn after_persist(
        &mut self,
        transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError>;
}

struct NoProbe;

impl TransitionProbe for NoProbe {
    fn after_persist(
        &mut self,
        _transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError> {
        Ok(())
    }
}

pub(crate) struct TransitionStore {
    root: PathBuf,
}

impl TransitionStore {
    pub(crate) fn from_environment() -> Result<Self, LifecycleControlError> {
        let root = if let Some(root) = env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
            PathBuf::from(root).join(APP_DIRECTORY)
        } else {
            let home = env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .ok_or(LifecycleControlError::Storage)?;
            PathBuf::from(home).join(".config").join(APP_DIRECTORY)
        };
        Self::new(root)
    }

    pub(crate) fn new(root: PathBuf) -> Result<Self, LifecycleControlError> {
        ensure_private_directory(&root)?;
        Ok(Self { root })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    fn transition_path(&self) -> PathBuf {
        self.root.join(TRANSITION_FILE)
    }

    fn control_lock(&self) -> Result<File, LifecycleControlError> {
        open_private_lock(&self.root.join(CONTROL_LOCK_FILE), true, true)
    }

    fn admission_lock(&self, exclusive: bool) -> Result<File, LifecycleControlError> {
        open_private_lock(&self.root.join(ADMISSION_LOCK_FILE), exclusive, true)
    }

    pub(crate) fn current(&self) -> Result<Option<BackgroundTransition>, LifecycleControlError> {
        read_transition(&self.transition_path())
    }

    #[cfg(test)]
    pub(crate) fn barrier_present(&self) -> Result<bool, LifecycleControlError> {
        let _gate = self.admission_lock(false)?;
        Ok(self.current()?.is_some())
    }

    pub(crate) fn admitted_transition_child(
        &self,
        generation: Option<&str>,
    ) -> Result<bool, LifecycleControlError> {
        let _gate = self.admission_lock(false)?;
        let Some(record) = self.current()? else {
            return Ok(generation.is_none());
        };
        Ok(generation == Some(record.generation.as_str())
            && record.kind == TransitionKind::Enable
            && record.disposition == TransitionDisposition::Forward
            && matches!(
                record.phase,
                TransitionPhase::SourceStopped
                    | TransitionPhase::ManagerEnabled
                    | TransitionPhase::SuccessorReady
            ))
    }

    pub(crate) fn enter_job_admission(&self) -> Result<JobAdmissionGuard, LifecycleControlError> {
        let gate = self.admission_lock(false)?;
        if self.current()?.is_some() {
            return Err(LifecycleControlError::ConflictingTransition);
        }
        Ok(JobAdmissionGuard { _gate: gate })
    }

    pub(crate) fn run(
        &self,
        requested: Option<TransitionKind>,
        state_path: &Path,
        expected_v2_instance_id: Option<String>,
        effects: &mut dyn TransitionEffects,
    ) -> Result<(), LifecycleControlError> {
        self.run_with_probe(
            requested,
            state_path,
            expected_v2_instance_id,
            effects,
            &mut NoProbe,
        )
    }

    fn run_with_probe(
        &self,
        requested: Option<TransitionKind>,
        state_path: &Path,
        expected_v2_instance_id: Option<String>,
        effects: &mut dyn TransitionEffects,
        probe: &mut dyn TransitionProbe,
    ) -> Result<(), LifecycleControlError> {
        let _control = self.control_lock()?;
        let mut transition = match self.current()? {
            Some(existing) => {
                validate_transition(&existing, state_path)?;
                if requested.is_some_and(|kind| kind != existing.kind) {
                    return Err(LifecycleControlError::ConflictingTransition);
                }
                existing
            }
            None => {
                let kind = requested.ok_or(LifecycleControlError::InvalidRecord)?;
                let prior_enabled = effects.manager_enabled()?;
                let transition =
                    new_transition(kind, prior_enabled, state_path, expected_v2_instance_id)?;
                self.persist_new(&transition)?;
                probe.after_persist(&transition)?;
                transition
            }
        };

        loop {
            if transition.disposition == TransitionDisposition::Rollback {
                return self.resume_rollback(&mut transition, effects, probe);
            }
            match (transition.kind, transition.phase) {
                (TransitionKind::Enable, TransitionPhase::IntentRecorded) => {
                    effects.stop_publishers(&transition)?;
                    transition.phase = TransitionPhase::SourceStopped;
                    self.persist_exact(&transition)?;
                    probe.after_persist(&transition)?;
                }
                (TransitionKind::Enable, TransitionPhase::SourceStopped) => {
                    effects.enable_and_start(&transition)?;
                    transition.phase = TransitionPhase::ManagerEnabled;
                    self.persist_exact(&transition)?;
                    probe.after_persist(&transition)?;
                }
                (TransitionKind::Enable, TransitionPhase::ManagerEnabled) => {
                    if effects.wait_until_ready(&transition).is_err() {
                        transition.disposition = TransitionDisposition::Rollback;
                        self.persist_exact(&transition)?;
                        probe.after_persist(&transition)?;
                        continue;
                    }
                    transition.phase = TransitionPhase::SuccessorReady;
                    self.persist_exact(&transition)?;
                    probe.after_persist(&transition)?;
                }
                (TransitionKind::Enable, TransitionPhase::SuccessorReady) => {
                    effects.before_clear(&transition)?;
                    self.clear_exact(&transition.generation)?;
                    return Ok(());
                }
                (TransitionKind::Disable, TransitionPhase::IntentRecorded) => {
                    effects.stop_publishers(&transition)?;
                    transition.phase = TransitionPhase::PublisherStopped;
                    self.persist_exact(&transition)?;
                    probe.after_persist(&transition)?;
                }
                (TransitionKind::Disable, TransitionPhase::PublisherStopped) => {
                    effects.disable_and_stop(&transition)?;
                    transition.phase = TransitionPhase::ManagerDisabled;
                    self.persist_exact(&transition)?;
                    probe.after_persist(&transition)?;
                }
                (TransitionKind::Disable, TransitionPhase::ManagerDisabled) => {
                    effects.before_clear(&transition)?;
                    self.clear_exact(&transition.generation)?;
                    return Ok(());
                }
                _ => return Err(LifecycleControlError::InvalidRecord),
            }
        }
    }

    fn resume_rollback(
        &self,
        transition: &mut BackgroundTransition,
        effects: &mut dyn TransitionEffects,
        probe: &mut dyn TransitionProbe,
    ) -> Result<(), LifecycleControlError> {
        if transition.kind != TransitionKind::Enable {
            return Err(LifecycleControlError::InvalidRecord);
        }
        match transition.phase {
            TransitionPhase::ManagerEnabled => {
                effects.stop_publishers(transition)?;
                transition.phase = TransitionPhase::RollbackTargetStopped;
                self.persist_exact(transition)?;
                probe.after_persist(transition)?;
            }
            TransitionPhase::RollbackTargetStopped => {}
            TransitionPhase::PriorChoiceRestored => {
                effects.before_clear(transition)?;
                self.clear_exact(&transition.generation)?;
                return Err(LifecycleControlError::Readiness);
            }
            _ => return Err(LifecycleControlError::InvalidRecord),
        }
        effects.restore_prior_choice(transition)?;
        transition.phase = TransitionPhase::PriorChoiceRestored;
        self.persist_exact(transition)?;
        probe.after_persist(transition)?;
        effects.before_clear(transition)?;
        self.clear_exact(&transition.generation)?;
        Err(LifecycleControlError::Readiness)
    }

    fn persist_new(&self, transition: &BackgroundTransition) -> Result<(), LifecycleControlError> {
        let _gate = self.admission_lock(true)?;
        if self.current()?.is_some() {
            return Err(LifecycleControlError::ConflictingTransition);
        }
        atomic_write(&self.transition_path(), transition)
    }

    fn persist_exact(
        &self,
        transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError> {
        let _gate = self.admission_lock(true)?;
        let current = self
            .current()?
            .ok_or(LifecycleControlError::InvalidRecord)?;
        if current.generation != transition.generation
            || current.kind != transition.kind
            || current.state != transition.state
        {
            return Err(LifecycleControlError::ConflictingTransition);
        }
        atomic_write(&self.transition_path(), transition)
    }

    fn clear_exact(&self, generation: &str) -> Result<(), LifecycleControlError> {
        let _gate = self.admission_lock(true)?;
        let current = self
            .current()?
            .ok_or(LifecycleControlError::InvalidRecord)?;
        if current.generation != generation {
            return Err(LifecycleControlError::ConflictingTransition);
        }
        fs::remove_file(self.transition_path()).map_err(|_| LifecycleControlError::Storage)?;
        sync_directory(&self.root)
    }
}

pub(crate) struct JobAdmissionGuard {
    _gate: File,
}

fn new_transition(
    kind: TransitionKind,
    prior_enabled: bool,
    state_path: &Path,
    expected_v2_instance_id: Option<String>,
) -> Result<BackgroundTransition, LifecycleControlError> {
    let state_path = state_path
        .canonicalize()
        .map_err(|_| LifecycleControlError::Storage)?;
    let metadata = fs::metadata(&state_path).map_err(|_| LifecycleControlError::Storage)?;
    let created_unix_ms = unix_ms()?;
    Ok(BackgroundTransition {
        format_version: FORMAT_VERSION,
        package_id: PACKAGE_ID.to_string(),
        package_version: env!("CARGO_PKG_VERSION").to_string(),
        artifact_scope: "shared-debian".to_string(),
        generation: Uuid::new_v4().to_string(),
        kind,
        disposition: TransitionDisposition::Forward,
        phase: TransitionPhase::IntentRecorded,
        prior_enabled,
        target_enabled: kind == TransitionKind::Enable,
        state: StateIdentity {
            path: state_path.to_string_lossy().into_owned(),
            device: metadata.dev(),
            inode: metadata.ino(),
        },
        expected_v2_instance_id,
        created_unix_ms,
        graceful_deadline_unix_ms: created_unix_ms.saturating_add(35_000),
        force_deadline_unix_ms: created_unix_ms.saturating_add(40_000),
        control_deadline_unix_ms: created_unix_ms.saturating_add(45_000),
    })
}

fn validate_transition(
    transition: &BackgroundTransition,
    state_path: &Path,
) -> Result<(), LifecycleControlError> {
    if transition.format_version != FORMAT_VERSION
        || transition.package_id != PACKAGE_ID
        || transition.package_version != env!("CARGO_PKG_VERSION")
        || transition.artifact_scope != "shared-debian"
        || Uuid::parse_str(&transition.generation).is_err()
        || transition.target_enabled != (transition.kind == TransitionKind::Enable)
    {
        return Err(LifecycleControlError::InvalidRecord);
    }
    let state_path = state_path
        .canonicalize()
        .map_err(|_| LifecycleControlError::Storage)?;
    let metadata = fs::metadata(&state_path).map_err(|_| LifecycleControlError::Storage)?;
    if transition.state.path != state_path.to_string_lossy()
        || transition.state.device != metadata.dev()
        || transition.state.inode != metadata.ino()
    {
        return Err(LifecycleControlError::InvalidRecord);
    }
    Ok(())
}

fn ensure_private_directory(path: &Path) -> Result<(), LifecycleControlError> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_dir() && metadata.uid() == unsafe { libc::geteuid() } => {}
        Ok(_) => return Err(LifecycleControlError::Storage),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|_| LifecycleControlError::Storage)?;
        }
        Err(_) => return Err(LifecycleControlError::Storage),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| LifecycleControlError::Storage)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| LifecycleControlError::Storage)?;
    if !metadata.file_type().is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(LifecycleControlError::Storage);
    }
    Ok(())
}

fn open_private_lock(
    path: &Path,
    exclusive: bool,
    nonblocking: bool,
) -> Result<File, LifecycleControlError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options
        .open(path)
        .map_err(|_| LifecycleControlError::Storage)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| LifecycleControlError::Storage)?;
    let metadata = file
        .metadata()
        .map_err(|_| LifecycleControlError::Storage)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(LifecycleControlError::Storage);
    }
    if nonblocking {
        let result = if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        };
        match result {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(LifecycleControlError::ConflictingTransition)
            }
            Err(TryLockError::Error(_)) => return Err(LifecycleControlError::Storage),
        }
    } else if exclusive {
        file.lock().map_err(|_| LifecycleControlError::Storage)?;
    } else {
        file.lock_shared()
            .map_err(|_| LifecycleControlError::Storage)?;
    }
    Ok(file)
}

fn read_transition(path: &Path) -> Result<Option<BackgroundTransition>, LifecycleControlError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(LifecycleControlError::Storage),
    };
    let metadata = file
        .metadata()
        .map_err(|_| LifecycleControlError::Storage)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.mode() & 0o777 != 0o600
        || metadata.len() == 0
        || metadata.len() > MAX_RECORD_BYTES
    {
        return Err(LifecycleControlError::InvalidRecord);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| LifecycleControlError::Storage)?;
    if bytes.len() as u64 != metadata.len() {
        return Err(LifecycleControlError::InvalidRecord);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| LifecycleControlError::InvalidRecord)
}

fn atomic_write(
    path: &Path,
    transition: &BackgroundTransition,
) -> Result<(), LifecycleControlError> {
    let parent = path.parent().ok_or(LifecycleControlError::Storage)?;
    let bytes = serde_json::to_vec(transition).map_err(|_| LifecycleControlError::Storage)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(LifecycleControlError::Storage);
    }
    let temporary = parent.join(format!(".{TRANSITION_FILE}.{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let mut file = options
            .open(&temporary)
            .map_err(|_| LifecycleControlError::Storage)?;
        file.write_all(&bytes)
            .map_err(|_| LifecycleControlError::Storage)?;
        file.sync_all()
            .map_err(|_| LifecycleControlError::Storage)?;
        fs::rename(&temporary, path).map_err(|_| LifecycleControlError::Storage)?;
        sync_directory(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn sync_directory(path: &Path) -> Result<(), LifecycleControlError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| LifecycleControlError::Storage)
}

fn unix_ms() -> Result<u64, LifecycleControlError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| LifecycleControlError::Storage)?;
    u64::try_from(duration.as_millis()).map_err(|_| LifecycleControlError::Storage)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlAction {
    Enable,
    Disable,
    Recover,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlStatus {
    Enabled,
    Disabled,
    TransitionPending,
}

pub(crate) fn run_control(action: ControlAction) -> Result<ControlStatus, LifecycleControlError> {
    let _package_authority = crate::connect::package_control::enter_admission()
        .map_err(|_| LifecycleControlError::ConflictingTransition)?;
    let store = TransitionStore::from_environment()?;
    let app_data_dir = app_data_directory()?;
    ensure_private_directory(&app_data_dir)?;
    let runtime_root = env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or(LifecycleControlError::Storage)?;
    let manager_config_root =
        if let Some(root) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
            PathBuf::from(root)
        } else {
            PathBuf::from(
                env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .ok_or(LifecycleControlError::Storage)?,
            )
            .join(".config")
        };
    let manager_link = manager_config_root
        .join("systemd/user/default.target.wants")
        .join(service_unit());
    fs::create_dir_all(
        manager_link
            .parent()
            .ok_or(LifecycleControlError::Storage)?,
    )
    .map_err(|_| LifecycleControlError::Storage)?;
    let mut effects = SystemdUserEffects {
        app_data_dir: app_data_dir.clone(),
        runtime_root,
        control_root: store.root().to_path_buf(),
        manager_link,
    };
    if action == ControlAction::Status {
        return if store.current()?.is_some() {
            Ok(ControlStatus::TransitionPending)
        } else if effects.manager_enabled()? {
            Ok(ControlStatus::Enabled)
        } else {
            Ok(ControlStatus::Disabled)
        };
    }
    let requested = match action {
        ControlAction::Enable => Some(TransitionKind::Enable),
        ControlAction::Disable => Some(TransitionKind::Disable),
        ControlAction::Recover => None,
        ControlAction::Status => unreachable!(),
    };
    crate::connect::package_control::record_participant_acknowledgement(
        effects.manager_enabled()?,
        &effects.runtime_root,
        &effects.app_data_dir,
        &effects.control_root,
        &effects.manager_link,
    )
    .map_err(|_| LifecycleControlError::Storage)?;
    let expected_v2_instance_id = read_expected_v2_instance_id(&app_data_dir)?;
    store.run(
        requested,
        &app_data_dir,
        expected_v2_instance_id,
        &mut effects,
    )?;
    let enabled = effects.manager_enabled()?;
    Ok(if enabled {
        ControlStatus::Enabled
    } else {
        ControlStatus::Disabled
    })
}

struct SystemdUserEffects {
    app_data_dir: PathBuf,
    runtime_root: PathBuf,
    control_root: PathBuf,
    manager_link: PathBuf,
}

impl SystemdUserEffects {}

impl TransitionEffects for SystemdUserEffects {
    fn manager_enabled(&mut self) -> Result<bool, LifecycleControlError> {
        let status = systemctl(&["is-enabled", "--quiet", service_unit()])?;
        match status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(LifecycleControlError::Manager),
        }
    }

    fn stop_publishers(
        &mut self,
        transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError> {
        let _ = systemctl(&["stop", service_unit()]);
        crate::connect::provider::stop_and_cleanup_registered_provider(
            &self.app_data_dir,
            &self.runtime_root,
            instant_for_unix_deadline(transition.control_deadline_unix_ms)?,
        )
        .map_err(|_| LifecycleControlError::Manager)
    }

    fn enable_and_start(
        &mut self,
        transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError> {
        let _ = transition;
        require_success(systemctl(&["enable", "--now", service_unit()])?)
    }

    fn wait_until_ready(
        &mut self,
        transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError> {
        crate::connect::provider::wait_for_registered_provider(
            &self.runtime_root,
            transition.expected_v2_instance_id.as_deref(),
            instant_for_unix_deadline(transition.control_deadline_unix_ms)?,
        )
        .map_err(|_| LifecycleControlError::Readiness)
    }

    fn disable_and_stop(
        &mut self,
        _transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError> {
        require_success(systemctl(&["disable", "--now", service_unit()])?)
    }

    fn restore_prior_choice(
        &mut self,
        transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError> {
        if transition.prior_enabled {
            require_success(systemctl(&["enable", service_unit()])?)
        } else {
            require_success(systemctl(&["disable", "--now", service_unit()])?)
        }
    }

    fn before_clear(
        &mut self,
        _transition: &BackgroundTransition,
    ) -> Result<(), LifecycleControlError> {
        crate::connect::package_control::record_participant_acknowledgement(
            self.manager_enabled()?,
            &self.runtime_root,
            &self.app_data_dir,
            &self.control_root,
            &self.manager_link,
        )
        .map_err(|_| LifecycleControlError::Storage)
    }
}

fn service_unit() -> &'static str {
    "document-summarizer-connect.service"
}

fn systemctl(arguments: &[&str]) -> Result<ExitStatus, LifecycleControlError> {
    let mut child = Command::new("systemctl")
        .arg("--user")
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| LifecycleControlError::Manager)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| LifecycleControlError::Manager)?
        {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(LifecycleControlError::Manager);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn require_success(status: ExitStatus) -> Result<(), LifecycleControlError> {
    status
        .success()
        .then_some(())
        .ok_or(LifecycleControlError::Manager)
}

fn app_data_directory() -> Result<PathBuf, LifecycleControlError> {
    if let Some(root) = env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(root).join(APP_DIRECTORY));
    }
    let home = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or(LifecycleControlError::Storage)?;
    Ok(PathBuf::from(home).join(".local/share").join(APP_DIRECTORY))
}

fn read_expected_v2_instance_id(
    app_data_dir: &Path,
) -> Result<Option<String>, LifecycleControlError> {
    let path = app_data_dir.join("connect-v2-instance-id");
    let value = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(LifecycleControlError::Storage),
    };
    let value = value.trim();
    if Uuid::parse_str(value).is_err() {
        return Err(LifecycleControlError::InvalidRecord);
    }
    Ok(Some(value.to_string()))
}

fn instant_for_unix_deadline(unix_deadline_ms: u64) -> Result<Instant, LifecycleControlError> {
    let now_ms = unix_ms()?;
    let remaining = unix_deadline_ms.saturating_sub(now_ms);
    Ok(Instant::now() + Duration::from_millis(remaining))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = env::temp_dir().join(format!("{label}-{}", Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Default)]
    struct MockEffects {
        enabled: bool,
        ready: bool,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    impl TransitionEffects for MockEffects {
        fn manager_enabled(&mut self) -> Result<bool, LifecycleControlError> {
            Ok(self.enabled)
        }

        fn stop_publishers(
            &mut self,
            _transition: &BackgroundTransition,
        ) -> Result<(), LifecycleControlError> {
            self.calls.lock().unwrap().push("stop");
            Ok(())
        }

        fn enable_and_start(
            &mut self,
            _transition: &BackgroundTransition,
        ) -> Result<(), LifecycleControlError> {
            self.calls.lock().unwrap().push("enable");
            self.enabled = true;
            Ok(())
        }

        fn wait_until_ready(
            &mut self,
            _transition: &BackgroundTransition,
        ) -> Result<(), LifecycleControlError> {
            self.calls.lock().unwrap().push("ready");
            self.ready
                .then_some(())
                .ok_or(LifecycleControlError::Readiness)
        }

        fn disable_and_stop(
            &mut self,
            _transition: &BackgroundTransition,
        ) -> Result<(), LifecycleControlError> {
            self.calls.lock().unwrap().push("disable");
            self.enabled = false;
            Ok(())
        }

        fn restore_prior_choice(
            &mut self,
            transition: &BackgroundTransition,
        ) -> Result<(), LifecycleControlError> {
            self.calls.lock().unwrap().push("restore");
            self.enabled = transition.prior_enabled;
            Ok(())
        }

        fn before_clear(
            &mut self,
            _transition: &BackgroundTransition,
        ) -> Result<(), LifecycleControlError> {
            self.calls.lock().unwrap().push("clear");
            Ok(())
        }
    }

    struct CrashAfter {
        remaining: usize,
    }

    impl TransitionProbe for CrashAfter {
        fn after_persist(
            &mut self,
            _transition: &BackgroundTransition,
        ) -> Result<(), LifecycleControlError> {
            if self.remaining == 0 {
                return Err(LifecycleControlError::Interrupted);
            }
            self.remaining -= 1;
            Ok(())
        }
    }

    #[test]
    fn enable_and_disable_resume_after_every_persisted_crash_point() {
        for kind in [TransitionKind::Enable, TransitionKind::Disable] {
            for crash_after in 0..4 {
                let root = TestDirectory::new("doc-sum-background-transition");
                let state = root.0.join("state");
                fs::create_dir(&state).unwrap();
                let store = TransitionStore::new(root.0.join("control")).unwrap();
                let mut effects = MockEffects {
                    enabled: kind == TransitionKind::Disable,
                    ready: true,
                    ..MockEffects::default()
                };
                let result = store.run_with_probe(
                    Some(kind),
                    &state,
                    None,
                    &mut effects,
                    &mut CrashAfter {
                        remaining: crash_after,
                    },
                );
                if matches!(result, Err(LifecycleControlError::Interrupted)) {
                    store.run(None, &state, None, &mut effects).unwrap();
                } else {
                    result.unwrap();
                }
                assert_eq!(effects.enabled, kind == TransitionKind::Enable);
                assert!(store.current().unwrap().is_none());
            }
        }
    }

    #[test]
    fn transition_record_is_an_admission_barrier_and_wrong_toggle_cannot_replace_it() {
        let root = TestDirectory::new("doc-sum-background-barrier");
        let state = root.0.join("state");
        fs::create_dir(&state).unwrap();
        let store = TransitionStore::new(root.0.join("control")).unwrap();
        let mut effects = MockEffects {
            ready: true,
            ..MockEffects::default()
        };
        let error = store
            .run_with_probe(
                Some(TransitionKind::Enable),
                &state,
                None,
                &mut effects,
                &mut CrashAfter { remaining: 0 },
            )
            .unwrap_err();
        assert!(matches!(error, LifecycleControlError::Interrupted));
        assert!(store.barrier_present().unwrap());
        assert!(matches!(
            store.enter_job_admission(),
            Err(LifecycleControlError::ConflictingTransition)
        ));
        assert!(matches!(
            store.run(Some(TransitionKind::Disable), &state, None, &mut effects),
            Err(LifecycleControlError::ConflictingTransition)
        ));
    }

    #[test]
    fn failed_enable_durably_rolls_back_the_exact_prior_choice() {
        let root = TestDirectory::new("doc-sum-background-rollback");
        let state = root.0.join("state");
        fs::create_dir(&state).unwrap();
        let store = TransitionStore::new(root.0.join("control")).unwrap();
        let mut effects = MockEffects::default();
        assert!(matches!(
            store.run(Some(TransitionKind::Enable), &state, None, &mut effects),
            Err(LifecycleControlError::Readiness)
        ));
        assert!(!effects.enabled);
        assert!(store.current().unwrap().is_none());
        assert!(effects.calls.lock().unwrap().contains(&"restore"));
    }
}
