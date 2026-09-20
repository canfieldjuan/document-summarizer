use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;
use uuid::Uuid;

const FORMAT_VERSION: u32 = 1;
const PACKAGE_ID: &str = "document-summarizer";
const RECORD_FILE: &str = "package-operation-v1.json";
const LOCK_FILE: &str = ".package-operation-v1.lock";
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PackagePhase {
    IntentRecorded,
    PublishersStopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Participant {
    uid: u32,
    user: String,
    home: String,
    runtime_root: String,
    app_data_root: String,
    enabled: bool,
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
    participants: Vec<Participant>,
}

pub(crate) enum PackageAction<'a> {
    PrepareUpgrade { source: &'a str, target: &'a str },
    FinishUpgrade { target: &'a str },
    PrepareRemove { target: &'a str },
    FinishRemove { target: &'a str },
    RecoverInstall { target: &'a str },
}

trait PackageEffects {
    fn discover(&mut self) -> Result<Vec<Participant>, PackageControlError>;
    fn stop(&mut self, participant: &Participant) -> Result<(), PackageControlError>;
    fn suppress(&mut self, participant: &Participant) -> Result<(), PackageControlError>;
    fn restore(&mut self, participant: &Participant) -> Result<(), PackageControlError>;
    fn cleanup(&mut self, participant: &Participant) -> Result<(), PackageControlError>;
    fn prepare_controller(&mut self, record: &PackageRecord) -> Result<(), PackageControlError>;
    fn retire_controller(&mut self, record: &PackageRecord) -> Result<(), PackageControlError>;
}

struct PackageStore {
    root: PathBuf,
}

impl PackageStore {
    fn from_environment() -> Self {
        let root = env::var_os("DOC_SUM_PACKAGE_CONTROL_ROOT")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/var/lib/document-summarizer"));
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
        Ok(())
    }

    fn record_path(&self) -> PathBuf {
        self.root.join(RECORD_FILE)
    }

    fn lock(&self) -> Result<File, PackageControlError> {
        self.prepare_root()?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let file = options
            .open(self.root.join(LOCK_FILE))
            .map_err(|_| PackageControlError::Storage)?;
        file.lock().map_err(|_| PackageControlError::Storage)?;
        Ok(file)
    }

    fn read(&self) -> Result<Option<PackageRecord>, PackageControlError> {
        read_record(&self.record_path())
    }

    fn write(&self, record: &PackageRecord) -> Result<(), PackageControlError> {
        let bytes = serde_json::to_vec(record).map_err(|_| PackageControlError::Storage)?;
        let temporary = self
            .root
            .join(format!(".{RECORD_FILE}.{}.tmp", record.generation));
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
            fs::rename(&temporary, self.record_path()).map_err(|_| PackageControlError::Storage)?;
            sync_directory(&self.root)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
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
    if unsafe { libc::geteuid() } != 0 && env::var_os("DOC_SUM_PACKAGE_CONTROL_ROOT").is_none() {
        return Err(PackageControlError::RootRequired);
    }
    let store = PackageStore::from_environment();
    let _lock = store.lock()?;
    let mut effects = SystemEffects;
    run_with(&store, action, &mut effects)
}

fn run_with(
    store: &PackageStore,
    action: PackageAction<'_>,
    effects: &mut dyn PackageEffects,
) -> Result<(), PackageControlError> {
    match action {
        PackageAction::PrepareUpgrade { source, target } => {
            prepare(store, PackageKind::Upgrade, source, target, effects)
        }
        PackageAction::PrepareRemove { target } => {
            prepare(store, PackageKind::Remove, target, target, effects)
        }
        PackageAction::FinishUpgrade { target } => {
            finish(store, PackageKind::Upgrade, target, effects)
        }
        PackageAction::FinishRemove { target } => {
            finish(store, PackageKind::Remove, target, effects)
        }
        PackageAction::RecoverInstall { target } => {
            let record = store.read()?.ok_or(PackageControlError::Conflict)?;
            match record.kind {
                PackageKind::Upgrade => finish(store, PackageKind::Upgrade, target, effects),
                PackageKind::Remove => {
                    let recorded_target = record.target_version.clone();
                    finish(store, PackageKind::Remove, &recorded_target, effects)
                }
            }
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
    let mut record = match store.read()? {
        Some(record) => {
            validate(&record, kind, target)?;
            record
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
                participants: effects.discover()?,
            };
            store.write(&record)?;
            record
        }
    };
    effects.prepare_controller(&record)?;
    if record.phase == PackagePhase::IntentRecorded {
        for participant in &record.participants {
            effects.stop(participant)?;
            if record.kind == PackageKind::Remove {
                effects.suppress(participant)?;
            }
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
) -> Result<(), PackageControlError> {
    let record = store.read()?.ok_or(PackageControlError::Conflict)?;
    validate(&record, kind, target)?;
    if record.phase != PackagePhase::PublishersStopped {
        return Err(PackageControlError::Conflict);
    }
    for participant in &record.participants {
        effects.cleanup(participant)?;
        if kind == PackageKind::Upgrade {
            effects.restore(participant)?;
        }
    }
    effects.retire_controller(&record)?;
    store.clear(&record.generation)
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
        || record
            .participants
            .windows(2)
            .any(|pair| pair[0].uid >= pair[1].uid)
    {
        return Err(PackageControlError::Conflict);
    }
    Ok(())
}

pub(crate) fn barrier_active() -> Result<bool, PackageControlError> {
    let store = PackageStore::from_environment();
    match store.read()? {
        None => Ok(false),
        Some(record) => {
            validate(&record, record.kind, &record.target_version)?;
            Ok(true)
        }
    }
}

fn read_record(path: &Path) -> Result<Option<PackageRecord>, PackageControlError> {
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

struct SystemEffects;

fn controller_path(record: &PackageRecord) -> PathBuf {
    PackageStore::from_environment()
        .root
        .join(format!("package-controller-{}", record.generation))
}

impl PackageEffects for SystemEffects {
    fn discover(&mut self) -> Result<Vec<Participant>, PackageControlError> {
        let passwd = fs::read_to_string("/etc/passwd").map_err(|_| PackageControlError::Storage)?;
        let mut participants = Vec::new();
        for line in passwd.lines() {
            let fields = line.split(':').collect::<Vec<_>>();
            if fields.len() < 6 {
                continue;
            }
            let Some(uid) = fields[2].parse::<u32>().ok() else {
                continue;
            };
            let user = fields[0].to_string();
            let home = PathBuf::from(fields[5]);
            let runtime_root = PathBuf::from(format!("/run/user/{uid}"));
            let active_session = runtime_root.is_dir();
            let enabled = if active_session {
                manager_status(&user, &["is-enabled", "--quiet"])?
            } else {
                exact_enablement_link(&home)?.is_some()
            };
            if !active_session && !enabled {
                continue;
            }
            participants.push(Participant {
                uid,
                user,
                home: home.to_string_lossy().into_owned(),
                runtime_root: runtime_root.to_string_lossy().into_owned(),
                app_data_root: home
                    .join(".local/share/com.juan-canfield.docsum")
                    .to_string_lossy()
                    .into_owned(),
                enabled,
            });
        }
        participants.sort_by_key(|participant| participant.uid);
        Ok(participants)
    }

    fn stop(&mut self, participant: &Participant) -> Result<(), PackageControlError> {
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

    fn restore(&mut self, participant: &Participant) -> Result<(), PackageControlError> {
        if !Path::new(&participant.runtime_root).is_dir() {
            return Ok(());
        }
        if participant.enabled {
            manager_success(&participant.user, &["enable", "--now"])
        } else {
            manager_success(&participant.user, &["disable", "--now"])
        }
    }

    fn suppress(&mut self, participant: &Participant) -> Result<(), PackageControlError> {
        if Path::new(&participant.runtime_root).is_dir() {
            return manager_success(&participant.user, &["disable", "--now"]);
        }
        let home = Path::new(&participant.home);
        if let Some(path) = exact_enablement_link(home)? {
            fs::remove_file(&path).map_err(|_| PackageControlError::Storage)?;
            sync_directory(path.parent().ok_or(PackageControlError::Storage)?)?;
        }
        Ok(())
    }

    fn cleanup(&mut self, participant: &Participant) -> Result<(), PackageControlError> {
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
        let metadata = fs::symlink_metadata(&path).map_err(|_| PackageControlError::Storage)?;
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

fn manager_status(user: &str, arguments: &[&str]) -> Result<bool, PackageControlError> {
    let mut command = arguments.to_vec();
    command.push("document-summarizer-connect.service");
    let status = systemctl_user(user, &command)?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(PackageControlError::Manager),
    }
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
        fn restore(&mut self, participant: &Participant) -> Result<(), PackageControlError> {
            self.calls.lock().unwrap().push(format!(
                "restore:{}:{}",
                participant.uid, participant.enabled
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
        fn prepare_controller(
            &mut self,
            _record: &PackageRecord,
        ) -> Result<(), PackageControlError> {
            Ok(())
        }
        fn retire_controller(
            &mut self,
            _record: &PackageRecord,
        ) -> Result<(), PackageControlError> {
            Ok(())
        }
    }

    fn store(label: &str) -> (PackageStore, PathBuf) {
        let root = env::temp_dir().join(format!("{label}-{}", Uuid::new_v4()));
        let store = PackageStore { root: root.clone() };
        store.prepare_root().unwrap();
        (store, root)
    }

    fn participant(uid: u32, enabled: bool) -> Participant {
        Participant {
            uid,
            user: format!("user-{uid}"),
            home: format!("/home/user-{uid}"),
            runtime_root: format!("/run/user/{uid}"),
            app_data_root: format!("/home/user-{uid}/.local/share/com.juan-canfield.docsum"),
            enabled,
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
        finish(&store, PackageKind::Upgrade, "0.1.0", &mut effects).unwrap();
        assert!(store.read().unwrap().is_none());
        assert_eq!(
            effects.calls.lock().unwrap().as_slice(),
            [
                "stop:1000",
                "cleanup:1000",
                "stop:1001",
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
        finish(&store, PackageKind::Remove, "0.1.0", &mut effects).unwrap();
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
            finish(&store, PackageKind::Remove, "0.1.0", &mut effects),
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
}
