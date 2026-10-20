//! Store initialization belongs to startup. Operational connections never migrate.
use super::{schema, Connection, StoreError};
use rusqlite::OpenFlags;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Store(Arc<PreparedStore>);

#[derive(Debug)]
struct PreparedStore {
    path: PathBuf,
    identity: (u64, u64),
    // Keep the WAL alive until the last owner/worker is gone. Never share this
    // connection for operations; every reader/writer gets a separate connection.
    _anchor: Mutex<Connection>,
    // Dropped after the anchor. File presence alone never proves readiness.
    _lease: File,
    // Primary liveness is distinct from consumer usage leases. Drop it before
    // the application's authority guards, even when consumers are still alive.
    _primary: Option<File>,
    _authority: Option<Authority>,
    generation: String,
}

struct Authority {
    _guard: Box<dyn Send + Sync>,
}
impl std::fmt::Debug for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StoreAuthority")
    }
}

pub(crate) struct StoreBootstrap {
    path: PathBuf,
    _gate: File,
}

fn busy() -> StoreError {
    rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY), None)
        .into()
}

fn fixed_runtime() -> Result<(), StoreError> {
    if rusqlite::version_number() < 3_051_003 {
        return Err(StoreError::NotInitialized(
            "SQLite WAL-reset fix is required".into(),
        ));
    }
    Ok(())
}

pub(super) fn initialize_connection(path: &Path) -> Result<Connection, StoreError> {
    fixed_runtime()?;
    let path = if path == Path::new(":memory:") {
        path.to_path_buf()
    } else {
        canonical_store_path(path)?
    };
    let mut conn = Connection::open(&path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    schema::migrate(&mut conn)?;
    if path.as_path() != Path::new(":memory:") {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
        if mode != "wal" {
            return Err(StoreError::NotInitialized(
                "WAL activation was refused".into(),
            ));
        }
    }
    Ok(conn)
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn canonical_store_path(path: &Path) -> Result<PathBuf, StoreError> {
    #[cfg(windows)]
    {
        let parent = path
            .parent()
            .ok_or_else(|| StoreError::NotInitialized("Store parent missing".into()))?;
        crate::connect::windows_storage::validate_private_directory(parent, parent)?;
    }
    let parent = fs::canonicalize(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    require_local_filesystem(&parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| StoreError::NotInitialized("Store filename missing".into()))?;
    let path = parent.join(name);
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(StoreError::NotInitialized("Store file is a symlink".into()));
    }
    Ok(path)
}

#[cfg(target_os = "linux")]
fn local_filesystem_magic(value: impl Into<i64>) -> bool {
    // The app store is local. Unknown/FUSE/network filesystems are not attested
    // for multi-connection WAL; do not silently run them in rollback mode.
    matches!(
        value.into() as u64 & 0xffff_ffff,
        // Linux magic.h: ext, xfs, btrfs, tmpfs, ramfs, overlayfs, f2fs,
        // ecryptfs, bcachefs, reiserfs, nilfs2.
        // https://github.com/torvalds/linux/blob/a0ff06b3a7bd3aa0cd99d0294e60b3ed28c9bfac/include/uapi/linux/magic.h
        0xef53 | 0x5846_5342 | 0x9123_683e | 0x0102_1994 | 0x8584_58f6 | 0x794c_7630 | 0xf2f5_2010
        | 0xf15f | 0xca45_1a4e | 0x5265_4973 | 0x3434
        // JFS: https://github.com/torvalds/linux/blob/a0ff06b3a7bd3aa0cd99d0294e60b3ed28c9bfac/fs/jfs/jfs_incore.h
        | 0x3153_464a
        // ZFS: https://github.com/openzfs/zfs/blob/7c3d85aaf1670d7386b0d4a157ef4ce96e17cba2/include/sys/fs/zfs.h
        | 0x2fc1_2fc1
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn require_local_filesystem(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::ffi::OsStrExt;
    let text = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| StoreError::NotInitialized("Store path contains NUL".into()))?;
    let mut information = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::statfs(text.as_ptr(), information.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let information = unsafe { information.assume_init() };
    #[cfg(target_os = "linux")]
    let local = local_filesystem_magic(information.f_type);
    #[cfg(target_os = "macos")]
    let local = information.f_flags & libc::MNT_LOCAL as u32 != 0;
    if !local {
        return Err(StoreError::UnsupportedFilesystem(
            "remote, FUSE, or unrecognized local filesystem".into(),
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn local_drive_type(kind: u32) -> bool {
    use windows_sys::Win32::System::WindowsProgramming::{
        DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOVABLE,
    };
    matches!(kind, DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_RAMDISK)
}

#[cfg(windows)]
fn require_local_filesystem(path: &Path) -> Result<(), StoreError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW};
    let text: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut volume = vec![0u16; 32768];
    if unsafe { GetVolumePathNameW(text.as_ptr(), volume.as_mut_ptr(), volume.len() as u32) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if !local_drive_type(unsafe { GetDriveTypeW(volume.as_ptr()) }) {
        return Err(StoreError::UnsupportedFilesystem(
            "remote or unrecognized Windows volume".into(),
        ));
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn require_local_filesystem(_path: &Path) -> Result<(), StoreError> {
    Err(StoreError::NotInitialized(
        "Store platform is not qualified for WAL".into(),
    ))
}

fn lock_file(path: &Path, create: bool) -> Result<File, StoreError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(create)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

fn exclusive(file: &File) -> Result<(), StoreError> {
    file.try_lock().map_err(|e| match e {
        TryLockError::WouldBlock => busy(),
        TryLockError::Error(e) => e.into(),
    })
}

fn shared(file: &File) -> Result<(), StoreError> {
    file.try_lock_shared().map_err(|e| match e {
        TryLockError::WouldBlock => busy(),
        TryLockError::Error(e) => e.into(),
    })
}

fn readiness_record(identity: (u64, u64)) -> String {
    format!(
        "docsum-store-v2:{}:{}:{}:{}",
        identity.0,
        identity.1,
        schema::CURRENT_SCHEMA_VERSION,
        uuid::Uuid::new_v4()
    )
}

fn read_readiness(file: &mut File, identity: (u64, u64)) -> Result<String, StoreError> {
    if file.metadata()?.len() > 128 {
        return Err(StoreError::NotInitialized(
            "Invalid store readiness record".into(),
        ));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    file.take(129).read_to_string(&mut text)?;
    let prefix = format!(
        "docsum-store-v2:{}:{}:{}:",
        identity.0,
        identity.1,
        schema::CURRENT_SCHEMA_VERSION
    );
    let valid = text
        .strip_prefix(&prefix)
        .is_some_and(|value| uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value));
    if !valid {
        return Err(StoreError::NotInitialized(
            "Prepared store identity does not match".into(),
        ));
    }
    Ok(text)
}

fn primary_is_live(primary: &File) -> Result<bool, StoreError> {
    match primary.try_lock() {
        Ok(()) => Ok(false),
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

fn require_primary(path: &Path) -> Result<(), StoreError> {
    let primary = lock_file(&sidecar(path, ".owner.lock"), false)?;
    if primary_is_live(&primary)? {
        Ok(())
    } else {
        Err(StoreError::Unavailable)
    }
}

impl StoreBootstrap {
    pub(crate) fn initialize_with_authority<A: Send + Sync + 'static>(
        self,
        authority: A,
    ) -> Result<Store, StoreError> {
        self.prepare(Some(Authority {
            _guard: Box::new(authority),
        }))
    }

    fn prepare(self, authority: Option<Authority>) -> Result<Store, StoreError> {
        let path = self.path;
        let primary = lock_file(&sidecar(&path, ".owner.lock"), true)?;
        let mut lease = lock_file(&sidecar(&path, ".ready.lock"), true)?;
        let (conn, generation, primary) = match primary.try_lock() {
            Ok(()) => {
                // Consumers of a departed primary may still have open work.
                // Do not migrate underneath them or treat them as a live owner.
                exclusive(&lease)?;
                let bootstrap = initialize_connection(&path)?;
                drop(bootstrap);
                let conn = operational_connection(&path, false)?;
                let generation = readiness_record(file_identity(&path)?);
                lease.set_len(0)?;
                lease.write_all(generation.as_bytes())?;
                lease.sync_data()?;
                lease.unlock()?;
                shared(&lease)?;
                (conn, generation, Some(primary))
            }
            Err(TryLockError::WouldBlock) => {
                // An actual primary exists: no setup or migration here.
                if authority.is_some() {
                    return Err(busy());
                }
                shared(&lease)?;
                let conn = operational_connection(&path, false)?;
                let generation = read_readiness(&mut lease, file_identity(&path)?)?;
                (conn, generation, None)
            }
            Err(TryLockError::Error(error)) => return Err(error.into()),
        };
        let identity = file_identity(&path)?;
        Store::bind(path, identity, conn, lease, primary, authority, generation)
    }
}

impl Store {
    /// Acquire initialization intent before application authority. An attaching
    /// GUI can then wait on this real OS gate, including the pre-bootstrap gap.
    pub(crate) fn begin(path: &Path) -> Result<StoreBootstrap, StoreError> {
        let path = canonical_store_path(path)?;
        let gate = lock_file(&sidecar(&path, ".initialize.lock"), true)?;
        exclusive(&gate)?;
        Ok(StoreBootstrap { path, _gate: gate })
    }

    pub fn initialize(path: &Path) -> Result<Self, StoreError> {
        Self::begin(path)?.prepare(None)
    }

    /// Attach only to active primary readiness; consumers cannot prolong it.
    pub fn attach(path: &Path) -> Result<Self, StoreError> {
        let path = canonical_store_path(path)?;
        let guard = lock_file(&sidecar(&path, ".initialize.lock"), false)?;
        shared(&guard)?;
        Self::attach_under_gate(path)
    }

    fn attach_under_gate(path: PathBuf) -> Result<Self, StoreError> {
        require_primary(&path)?;
        let mut lease = lock_file(&sidecar(&path, ".ready.lock"), false)?;
        shared(&lease)?;
        let identity = file_identity(&path)?;
        let generation = read_readiness(&mut lease, identity)?;
        let conn = operational_connection(&path, true)?;
        require_primary(&path)?;
        Self::bind(path, identity, conn, lease, None, None, generation)
    }

    /// Startup only. Wait at most two seconds for the already-held init gate;
    /// never poll filenames or infer readiness from their presence.
    pub(crate) fn attach_wait(path: &Path) -> Result<Self, StoreError> {
        Self::attach_with_deadline(path, Duration::from_secs(2))
    }

    fn attach_with_deadline(path: &Path, budget: Duration) -> Result<Self, StoreError> {
        let path = canonical_store_path(path)?;
        let gate = lock_file(&sidecar(&path, ".initialize.lock"), false)?;
        let deadline = Instant::now() + budget;
        loop {
            match shared(&gate) {
                Ok(()) => return Self::attach_under_gate(path),
                Err(error) if is_contention(&error) && Instant::now() < deadline => {
                    std::thread::park_timeout(
                        Duration::from_millis(5)
                            .min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn bind(
        path: PathBuf,
        identity: (u64, u64),
        conn: Connection,
        lease: File,
        primary: Option<File>,
        authority: Option<Authority>,
        generation: String,
    ) -> Result<Self, StoreError> {
        #[cfg(windows)]
        {
            let parent = path.parent().expect("canonical store has a parent");
            for file in [
                path.clone(),
                sidecar(&path, "-wal"),
                sidecar(&path, "-shm"),
                sidecar(&path, ".initialize.lock"),
                sidecar(&path, ".ready.lock"),
                sidecar(&path, ".owner.lock"),
            ] {
                if file.exists() {
                    crate::connect::windows_storage::validate_inherited_regular_file(
                        &file, parent,
                    )?;
                }
            }
        }
        if file_identity(&path)? != identity {
            return Err(StoreError::NotInitialized(
                "Store changed during initialization".into(),
            ));
        }
        Ok(Self(Arc::new(PreparedStore {
            path,
            identity,
            _anchor: Mutex::new(conn),
            _lease: lease,
            _primary: primary,
            _authority: authority,
            generation,
        })))
    }

    pub fn path(&self) -> &Path {
        &self.0.path
    }

    pub(crate) fn generation(&self) -> &str {
        &self.0.generation
    }

    pub(crate) fn check_ready(&self) -> Result<(), StoreError> {
        let gate = lock_file(&sidecar(self.path(), ".initialize.lock"), false)?;
        shared(&gate)?;
        require_primary(self.path())?;
        let mut record = lock_file(&sidecar(self.path(), ".ready.lock"), false)?;
        if read_readiness(&mut record, self.0.identity)? != self.0.generation {
            return Err(StoreError::NotInitialized(
                "Store owner generation changed".into(),
            ));
        }
        if file_identity(self.path())? != self.0.identity {
            return Err(StoreError::NotInitialized(
                "Prepared store file was replaced".into(),
            ));
        }
        Ok(())
    }

    fn open(&self, read_only: bool) -> Result<Connection, StoreError> {
        self.check_ready()?;
        let conn = operational_connection(self.path(), read_only)?;
        self.check_ready()?;
        Ok(conn)
    }

    /// Private-fixture fault injection only: checkpoint main and close all
    /// SQLite anchor handles before a test moves its main database file.
    #[cfg(test)]
    pub(crate) fn close_fixture_sqlite_handles(&self) {
        let mut slot = self.0._anchor.lock().unwrap();
        let conn = std::mem::replace(&mut *slot, Connection::open_in_memory().unwrap());
        assert!(conn.is_autocommit());
        let busy: u32 = conn
            .query_row("PRAGMA main.wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(busy, 0);
        conn.close().unwrap();
    }

    #[cfg(test)]
    pub(crate) fn with_synthetic_exclusive_lock<T>(&self, work: impl FnOnce() -> T) -> T {
        let anchor = self.0._anchor.lock().unwrap();
        anchor
            .pragma_update(None, "locking_mode", "EXCLUSIVE")
            .unwrap();
        anchor.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
        anchor.execute_batch("ROLLBACK").unwrap();
        anchor
            .pragma_update(None, "locking_mode", "NORMAL")
            .unwrap();
        let _: u32 = anchor
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        drop(anchor);
        match result {
            Ok(value) => value,
            Err(error) => std::panic::resume_unwind(error),
        }
    }

    pub fn reader(&self) -> Result<Connection, StoreError> {
        self.open(true)
    }
    pub fn writer(&self) -> Result<Connection, StoreError> {
        self.open(false)
    }
}

fn operational_connection(path: &Path, read_only: bool) -> Result<Connection, StoreError> {
    fixed_runtime()?;
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    };
    let conn = Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(if read_only {
        Duration::ZERO
    } else {
        Duration::from_secs(5)
    })?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    if !read_only {
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "locking_mode", "NORMAL")?;
        let sync: u32 = conn.pragma_query_value(None, "synchronous", |r| r.get(0))?;
        let mode: String = conn.pragma_query_value(None, "locking_mode", |r| r.get(0))?;
        if sync != 2 || mode != "normal" {
            return Err(StoreError::NotInitialized(
                "Store writer requires FULL sync and NORMAL locking".into(),
            ));
        }
    }
    let version = schema::version(&conn)?;
    if version != schema::CURRENT_SCHEMA_VERSION {
        return Err(StoreError::NotInitialized(
            "Store schema is not prepared for this binary".into(),
        ));
    }
    let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
    if mode != "wal" {
        return Err(StoreError::NotInitialized(
            "Store is not prepared in WAL mode".into(),
        ));
    }
    Ok(conn)
}

// Do not open/close raw DB descriptors on Unix: that can discard SQLite's
// process-wide POSIX locks. Metadata supplies identity without that operation.
#[cfg(unix)]
fn file_identity(path: &Path) -> Result<(u64, u64), StoreError> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::metadata(path)?;
    Ok((m.dev(), m.ino()))
}

#[cfg(windows)]
fn file_identity(path: &Path) -> Result<(u64, u64), StoreError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let file = File::open(path)?;
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), info.as_mut_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let info = unsafe { info.assume_init() };
    Ok((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

/// One typed classifier for SQL errors wrapped by pipeline/Connect owners.
pub fn is_contention(mut error: &(dyn std::error::Error + 'static)) -> bool {
    loop {
        if let Some(rusqlite::Error::SqliteFailure(code, _)) =
            error.downcast_ref::<rusqlite::Error>()
        {
            return matches!(
                code.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            );
        }
        if let Some(code) = error.downcast_ref::<rusqlite::ffi::Error>() {
            return matches!(
                code.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            );
        }
        match error.source() {
            Some(source) => error = source,
            None => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_store::DatabaseFixture;
    use std::time::Instant;

    #[test]
    fn readers_are_read_only_and_ordinary_opens_do_not_initialize() {
        let file = DatabaseFixture::new();
        let before = schema::setup_counts(&file.0);
        let store = Store::initialize(&file.0).unwrap();
        let initialized = schema::setup_counts(&file.0);
        assert_eq!(initialized, (before.0 + 1, before.1 + 1));
        let writer = store.writer().unwrap();
        writer
            .execute_batch(
                "CREATE TABLE role_probe(value INTEGER); INSERT INTO role_probe VALUES(7)",
            )
            .unwrap();
        let reader = store.reader().unwrap();
        assert_eq!(
            reader
                .query_row("SELECT value FROM role_probe", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
        let error = reader
            .execute("INSERT INTO role_probe VALUES(8)", [])
            .unwrap_err();
        assert!(
            matches!(error,rusqlite::Error::SqliteFailure(e,_) if e.code==rusqlite::ErrorCode::ReadOnly)
        );
        assert_eq!(
            reader
                .pragma_query_value(None, "busy_timeout", |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            writer
                .pragma_query_value(None, "synchronous", |r| r.get::<_, u32>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            writer
                .pragma_query_value(None, "foreign_keys", |r| r.get::<_, u32>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            reader
                .pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        let joined = Store::initialize(&file.0).unwrap();
        let attached = Store::attach(&file.0).unwrap();
        drop(joined.reader().unwrap());
        drop(attached.writer().unwrap());
        assert_eq!(
            schema::setup_counts(&file.0),
            initialized,
            "ordinary opens reran setup/integrity validation"
        );
        let source: String = reader
            .query_row("SELECT sqlite_source_id()", [], |r| r.get(0))
            .unwrap();
        eprintln!(
            "STORE_RUNTIME version={} number={} source_id={source}",
            rusqlite::version(),
            rusqlite::version_number()
        );
    }

    #[test]
    fn setup_observations_follow_store_across_request_threads() {
        let file = DatabaseFixture::new();
        let store = Store::initialize(&file.0).unwrap();
        let before = schema::setup_counts(&file.0);
        std::thread::spawn(move || {
            let mut conn = store.writer().unwrap();
            schema::migrate(&mut conn).unwrap();
        })
        .join()
        .unwrap();
        assert_eq!(
            schema::setup_counts(&file.0),
            (before.0 + 1, before.1 + 1),
            "request thread setup was invisible to the store observer"
        );
    }

    #[test]
    fn exceptional_busy_read_is_prompt_and_recovers() {
        let file = DatabaseFixture::new();
        let store = Store::initialize(&file.0).unwrap();
        store.with_synthetic_exclusive_lock(|| {
            let started = Instant::now();
            let error = store.reader().unwrap_err();
            assert!(is_contention(&error));
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "read waited for busy timeout"
            );
        });
        assert!(store.reader().is_ok());
    }

    #[test]
    fn primary_authority_outlives_ready_clones_and_observes_readiness_teardown() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Probe {
            path: PathBuf,
            released: Arc<AtomicBool>,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                assert!(
                    Store::attach(&self.path).is_err(),
                    "application authority released while primary readiness remained"
                );
                self.released.store(true, Ordering::SeqCst);
            }
        }
        let file = DatabaseFixture::new();
        let released = Arc::new(AtomicBool::new(false));
        let primary = Store::begin(&file.0)
            .unwrap()
            .initialize_with_authority(Probe {
                path: file.0.clone(),
                released: Arc::clone(&released),
            })
            .unwrap();
        let consumer = Store::attach(&file.0).unwrap();
        let clone = primary.clone();
        drop(primary);
        assert!(!released.load(Ordering::SeqCst));
        assert!(consumer.reader().is_ok());
        drop(clone);
        assert!(released.load(Ordering::SeqCst));
        assert!(consumer.reader().is_err());
        assert!(
            is_contention(&Store::initialize(&file.0).unwrap_err()),
            "migration admitted beneath stale consumer usage"
        );
        drop(consumer);
        assert!(Store::initialize(&file.0).unwrap().reader().is_ok());
    }

    #[test]
    fn attach_wait_observes_bootstrap_gate_and_failed_initialization() {
        let file = DatabaseFixture::new();
        let ticket = Store::begin(&file.0).unwrap();
        let path = file.0.clone();
        let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || {
            waiting_tx.send(()).unwrap();
            Store::attach_wait(&path)
        });
        waiting_rx.recv().unwrap();
        let primary = ticket.initialize_with_authority(()).unwrap();
        assert!(waiter.join().unwrap().unwrap().reader().is_ok());
        drop(primary);
        let failed = DatabaseFixture::new();
        let ticket = Store::begin(&failed.0).unwrap();
        let now = Instant::now();
        assert!(is_contention(
            &Store::attach_with_deadline(&failed.0, Duration::from_millis(20)).unwrap_err()
        ));
        assert!(now.elapsed() < Duration::from_secs(1));
        drop(ticket);
        assert!(
            Store::attach_wait(&failed.0).is_err(),
            "failed bootstrap made a ready store"
        );
    }

    #[test]
    fn overlapping_absence_probe_cannot_impersonate_primary() {
        let file = DatabaseFixture::new();
        let primary = Store::initialize(&file.0).unwrap();
        let consumer = Store::attach(&file.0).unwrap();
        let access = crate::pipeline::db::StoreAccess::prepared(consumer.clone());
        drop(primary);

        std::thread::scope(|scope| {
            let (probed_tx, probed_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let path = consumer.path();
            scope.spawn(move || {
                let gate = lock_file(&sidecar(path, ".initialize.lock"), false).unwrap();
                shared(&gate).unwrap();
                let probe = lock_file(&sidecar(path, ".owner.lock"), false).unwrap();
                assert!(!primary_is_live(&probe).unwrap());
                // Suspend the real probe after acquiring its absence lock, before
                // require_primary would drop the File. No scheduling delay needed.
                probed_tx.send(()).unwrap();
                let _ = release_rx.recv();
                drop(probe);
                drop(gate);
            });
            probed_rx.recv().unwrap();
            assert!(
                matches!(access.get(), Err(StoreError::Unavailable)),
                "desktop inherited departed primary readiness from another probe"
            );
            assert!(matches!(consumer.reader(), Err(StoreError::Unavailable)));
            assert!(matches!(consumer.writer(), Err(StoreError::Unavailable)));
            assert!(matches!(Store::attach(&file.0), Err(StoreError::Unavailable)));
            assert!(is_contention(&Store::initialize(&file.0).unwrap_err()));
            release_tx.send(()).unwrap();
        });
        assert!(matches!(consumer.reader(), Err(StoreError::Unavailable)));
        drop(access);
        drop(consumer);
        assert!(Store::initialize(&file.0).unwrap().reader().is_ok());
    }

    #[test]
    fn readiness_probes_preserve_live_primary_and_release_without_authority() {
        let file = DatabaseFixture::new();
        let primary = Store::initialize(&file.0).unwrap();
        let consumer = Store::attach(&file.0).unwrap();
        let access = crate::pipeline::db::StoreAccess::prepared(consumer.clone());
        let probes = [
            lock_file(&sidecar(consumer.path(), ".owner.lock"), false).unwrap(),
            lock_file(&sidecar(consumer.path(), ".owner.lock"), false).unwrap(),
        ];
        for probe in &probes {
            assert!(primary_is_live(probe).unwrap());
        }
        assert!(access.get().is_ok());
        assert!(consumer.reader().is_ok());
        assert!(consumer.writer().is_ok());
        assert!(Store::attach(&file.0).is_ok());
        drop(primary);
        // Contended probes acquired no authority and cannot retain a departed owner.
        assert!(matches!(access.get(), Err(StoreError::Unavailable)));
        assert!(matches!(consumer.reader(), Err(StoreError::Unavailable)));
    }

    #[test]
    fn attached_consumer_cannot_extend_primary_readiness() {
        let file = DatabaseFixture::new();
        let primary = Store::initialize(&file.0).unwrap();
        let consumer = Store::attach(&file.0).unwrap();
        drop(primary);
        assert!(
            consumer.reader().is_err(),
            "attached consumer manufactured primary readiness"
        );
        assert!(
            Store::attach(&file.0).is_err(),
            "stale consumer admitted another consumer"
        );
    }

    #[test]
    fn readiness_requires_an_active_owner_and_survives_clone_only() {
        let file = DatabaseFixture::new();
        assert!(Store::attach(&file.0).is_err());
        assert!(!file.0.exists());
        let store = Store::initialize(&file.0).unwrap();
        let clone = store.clone();
        drop(store);
        let attached = Store::attach(&file.0).unwrap();
        assert!(attached.reader().is_ok());
        drop(clone);
        assert!(attached.reader().is_err());
        drop(attached);
        assert!(
            Store::attach(&file.0).is_err(),
            "stale readiness file granted access"
        );
        assert!(Store::initialize(&file.0).unwrap().reader().is_ok());
    }

    #[test]
    fn replaced_store_does_not_inherit_readiness() {
        let file = DatabaseFixture::new();
        let other = DatabaseFixture::new();
        let first = Store::initialize(&file.0).unwrap();
        let second = Store::initialize(&other.0).unwrap();
        // Checkpoint and close every real SQLite handle before this test-only
        // identity fault injection. Never move a live main file away from WAL.
        for store in [&first, &second] {
            store.close_fixture_sqlite_handles();
        }
        fs::rename(&file.0, file.directory().join("held-original.db")).unwrap();
        fs::rename(&other.0, &file.0).unwrap();
        assert!(first.reader().is_err());
        assert!(
            Store::attach(&file.0).is_err(),
            "different store inherited active readiness"
        );
        assert!(second.reader().is_err());
    }

    #[test]
    fn failed_validation_and_future_schema_never_publish_readiness() {
        let file = DatabaseFixture::new();
        let store = Store::initialize(&file.0).unwrap();
        drop(store);
        let conn = Connection::open(&file.0).unwrap();
        conn.execute_batch("DROP TRIGGER ocr_lineage_no_update")
            .unwrap();
        drop(conn);
        assert!(Store::initialize(&file.0).is_err());
        assert!(Store::attach(&file.0).is_err());
        let future = DatabaseFixture::new();
        let conn = Connection::open(&future.0).unwrap();
        let version = schema::CURRENT_SCHEMA_VERSION + 1;
        conn.pragma_update(None, "user_version", version).unwrap();
        drop(conn);
        assert!(Store::initialize(&future.0).is_err());
        let conn = Connection::open(&future.0).unwrap();
        assert_eq!(schema::version(&conn).unwrap(), version);
        assert_eq!(
            conn.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
                .unwrap(),
            "delete"
        );
        assert!(Store::attach(&future.0).is_err());
    }

    #[test]
    fn corrupt_missing_and_wrong_type_stores_never_become_ready() {
        let missing = DatabaseFixture::new();
        assert!(Store::attach(&missing.0).is_err());
        assert!(!missing.0.exists());
        let corrupt = DatabaseFixture::new();
        let bytes = b"not a SQLite database: public corruption probe";
        fs::write(&corrupt.0, bytes).unwrap();
        let error = Store::initialize(&corrupt.0).unwrap_err();
        assert!(!is_contention(&error));
        assert!(Store::attach(&corrupt.0).is_err());
        assert_eq!(fs::read(&corrupt.0).unwrap(), bytes);
        let directory = DatabaseFixture::new();
        fs::create_dir(&directory.0).unwrap();
        let error = Store::initialize(&directory.0).unwrap_err();
        assert!(!is_contention(&error));
        assert!(Store::attach(&directory.0).is_err());
    }

    #[test]
    fn wal_refusal_never_publishes_readiness_or_changes_committed_data() {
        let file = DatabaseFixture::new();
        let mut blocker = Connection::open(&file.0).unwrap();
        schema::migrate(&mut blocker).unwrap();
        blocker.execute_batch("CREATE TABLE refusal_probe(v INTEGER); INSERT INTO refusal_probe VALUES(19); BEGIN; SELECT * FROM refusal_probe;").unwrap();
        // An existing rollback-journal reader permits validation but prevents
        // the journal transition. Keep it held throughout the bootstrap call.
        let error = Store::initialize(&file.0).unwrap_err();
        assert!(is_contention(&error));
        assert!(Store::attach(&file.0).is_err());
        assert_eq!(
            blocker
                .pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
                .unwrap(),
            "delete"
        );
        blocker.execute_batch("ROLLBACK").unwrap();
        drop(blocker);
        let store = Store::initialize(&file.0).unwrap();
        assert_eq!(
            store
                .reader()
                .unwrap()
                .query_row("SELECT v FROM refusal_probe", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            19
        );
    }

    #[cfg(unix)]
    #[test]
    fn denied_sidecar_access_is_not_busy_and_never_falls_back_to_a_writer() {
        use std::os::unix::fs::PermissionsExt;
        // A privileged test process can bypass Unix modes; do not claim proof.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let file = DatabaseFixture::new();
        let store = Store::initialize(&file.0).unwrap();
        let wal = sidecar(&file.0, "-wal");
        fs::set_permissions(&wal, fs::Permissions::from_mode(0o000)).unwrap();
        let result = store.reader();
        fs::set_permissions(&wal, fs::Permissions::from_mode(0o600)).unwrap();
        let error = result.unwrap_err();
        assert!(!is_contention(&error));
        assert!(store.reader().is_ok());
    }

    #[test]
    fn contention_classifier_preserves_wrapped_and_extended_sql_codes() {
        for code in [
            rusqlite::ffi::SQLITE_BUSY,
            rusqlite::ffi::SQLITE_LOCKED,
            rusqlite::ffi::SQLITE_BUSY_SNAPSHOT,
        ] {
            let e = rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None);
            assert!(is_contention(&e));
            let nested = crate::connect::store::ConnectStoreError::PipelineStore(
                StoreError::Migration(schema::MigrationError::Sqlite(e)),
            );
            assert!(is_contention(&nested));
        }
        for code in [
            rusqlite::ffi::SQLITE_CORRUPT,
            rusqlite::ffi::SQLITE_IOERR,
            rusqlite::ffi::SQLITE_READONLY,
            rusqlite::ffi::SQLITE_CONSTRAINT,
        ] {
            assert!(!is_contention(&rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(code),
                None
            )));
        }
    }

    #[test]
    fn bootstrap_gate_blocks_partial_readiness_and_other_initializer() {
        let file = DatabaseFixture::new();
        let guard = lock_file(&sidecar(&file.0, ".initialize.lock"), true).unwrap();
        exclusive(&guard).unwrap();
        let error = Store::initialize(&file.0).unwrap_err();
        assert!(is_contention(&error));
        assert!(!file.0.exists());
        drop(guard);
        let store = Store::initialize(&file.0).unwrap();
        let guard = lock_file(&sidecar(&file.0, ".initialize.lock"), false).unwrap();
        exclusive(&guard).unwrap();
        assert!(is_contention(&Store::attach(&file.0).unwrap_err()));
        drop(guard);
        assert!(store.reader().is_ok());
    }

    #[test]
    fn legacy_migration_preserves_values_and_failure_rolls_back() {
        let file = DatabaseFixture::new();
        let conn = Connection::open(&file.0).unwrap();
        schema::create_v21_fixture(&conn);
        conn.execute_batch("INSERT INTO documents(document_id,original_filename,file_type,byte_size,content_hash,local_source_path,created_at) VALUES('preserved','fixture.pdf','pdf',17,'public-hash','/fixture.pdf','2026-10-18T00:00:00Z')").unwrap();
        let before =
            serde_json::to_value(super::super::get_document(&conn, "preserved").unwrap()).unwrap();
        drop(conn);
        let store = Store::initialize(&file.0).unwrap();
        let conn = store.reader().unwrap();
        assert_eq!(
            schema::version(&conn).unwrap(),
            schema::CURRENT_SCHEMA_VERSION
        );
        assert_eq!(
            serde_json::to_value(super::super::get_document(&conn, "preserved").unwrap()).unwrap(),
            before
        );
        let broken = DatabaseFixture::new();
        let conn = Connection::open(&broken.0).unwrap();
        schema::create_v21_fixture(&conn);
        // The existing schema rollback fixture, exercised through the startup owner.
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        conn.execute("INSERT INTO ocr_lineage VALUES('missing-handoff','artifact','missing-run','2026-09-28T00:00:00Z')",[]).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        let before: (String, String, String, String) = conn
            .query_row("SELECT * FROM ocr_lineage", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        drop(conn);
        assert!(Store::initialize(&broken.0).is_err());
        assert!(Store::attach(&broken.0).is_err());
        let conn = Connection::open(&broken.0).unwrap();
        assert_eq!(schema::version(&conn).unwrap(), 21);
        let after: (String, String, String, String) = conn
            .query_row("SELECT * FROM ocr_lineage", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        assert_eq!(before, after);
        let definition: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='ocr_handoffs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(definition.contains("ocr_pdf_byte_size <= 2097152"));
    }

    #[test]
    fn sidecars_preserve_private_access() {
        let file = DatabaseFixture::new();
        let store = Store::initialize(&file.0).unwrap();
        store
            .writer()
            .unwrap()
            .execute_batch(
                "CREATE TABLE permission_probe(v INTEGER); INSERT INTO permission_probe VALUES(1)",
            )
            .unwrap();
        for path in [
            file.0.clone(),
            sidecar(&file.0, "-wal"),
            sidecar(&file.0, "-shm"),
            sidecar(&file.0, ".initialize.lock"),
            sidecar(&file.0, ".ready.lock"),
        ] {
            assert!(path.exists(), "missing sidecar {}", path.display());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
            }
            #[cfg(windows)]
            crate::connect::windows_storage::validate_inherited_regular_file(
                &path,
                file.directory(),
            )
            .unwrap();
        }
    }

    #[test]
    fn bootstrap_and_status_costs_are_measured_separately() {
        for rows in [0, 32] {
            let file = DatabaseFixture::new();
            let store = Store::initialize(&file.0).unwrap();
            let writer = store.writer().unwrap();
            writer
                .execute_batch("BEGIN; CREATE TABLE cost_probe(payload BLOB NOT NULL)")
                .unwrap();
            for _ in 0..rows {
                writer
                    .execute("INSERT INTO cost_probe VALUES(zeroblob(1048576))", [])
                    .unwrap();
            }
            writer.execute_batch("COMMIT").unwrap();
            drop(writer);
            drop(store);
            let start = Instant::now();
            let store = Store::initialize(&file.0).unwrap();
            let bootstrap_ms = start.elapsed().as_secs_f64() * 1000.0;
            let counts = schema::setup_counts(&file.0);
            let start = Instant::now();
            let reader = store.reader().unwrap();
            let count: u32 = reader
                .query_row("SELECT count(*) FROM cost_probe", [], |r| r.get(0))
                .unwrap();
            let read_ms = start.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(count, rows);
            assert_eq!(schema::setup_counts(&file.0), counts);
            eprintln!(
                "STORE_COST bytes={} bootstrap_ms={bootstrap_ms} operational_read_ms={read_ms}",
                fs::metadata(&file.0).unwrap().len()
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn standard_local_filesystems_are_admitted() {
        for (name, magic) in [
            ("zfs", 0x2fc1_2fc1_i64),
            ("ecryptfs", 0xf15f),
            ("bcachefs", 0xca45_1a4e),
            ("jfs", 0x3153_464a),
            ("reiserfs", 0x5265_4973),
            ("nilfs2", 0x3434),
        ] {
            assert!(
                local_filesystem_magic(magic),
                "local filesystem rejected: {name}"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn remote_unknown_and_falsy_filesystems_are_not_admitted() {
        for value in [
            0,
            -1,
            0x6969,
            0xff53_4d42,
            0xfe53_4d42,
            0x6573_5546, // FUSE
            0x0102_1997, // 9p
            0x00c3_6400, // ceph
            0x5346_414f,
            0x6b41_4653, // AFS
            0x7375_7245,
            0x517b,
            0x564c,      // coda, SMB, NCP
            0x1234_5678, // unknown
            i64::MAX,
        ] {
            assert!(!local_filesystem_magic(value));
        }
        for value in [
            0xef53_i64,
            0x5846_5342,
            0x9123_683e,
            0x0102_1994,
            0x794c_7630,
        ] {
            assert!(local_filesystem_magic(value));
        }
    }

    #[cfg(windows)]
    #[test]
    fn remote_unknown_and_falsy_volumes_are_not_admitted() {
        for value in [0, 1, 4, 5, u32::MAX] {
            assert!(!local_drive_type(value));
        }
        for value in [2, 3, 6] {
            assert!(local_drive_type(value));
        }
    }

    #[test]
    fn crash_writer_child() {
        let Some(path) = std::env::var_os("DOC_SUM_STORE_CRASH_PROBE") else {
            return;
        };
        if !std::env::args().any(|arg| arg == "--exact") {
            return;
        }
        assert!(
            Path::new(&path)
                .parent()
                .and_then(|p| p.file_name())
                .unwrap()
                .to_string_lossy()
                .starts_with("doc-sum-store-"),
            "crash probe requires its private temporary store"
        );
        let store = Store::initialize(Path::new(&path)).unwrap();
        let conn = store.writer().unwrap();
        conn.execute_batch("CREATE TABLE crash_probe(value INTEGER); INSERT INTO crash_probe VALUES(1); BEGIN IMMEDIATE; INSERT INTO crash_probe VALUES(2);").unwrap();
        std::process::exit(0);
    }

    #[test]
    fn process_crash_preserves_committed_and_discards_uncommitted_records() {
        let file = DatabaseFixture::new();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "pipeline::db::lifecycle::tests::crash_writer_child",
                "--nocapture",
            ])
            .env("DOC_SUM_STORE_CRASH_PROBE", &file.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let store = Store::initialize(&file.0).unwrap();
        let conn = store.reader().unwrap();
        let rows: Vec<i64> = conn
            .prepare("SELECT value FROM crash_probe ORDER BY value")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows, vec![1]);
        assert_eq!(
            conn.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    }
}
