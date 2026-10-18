//! Store initialization belongs to startup. Operational connections never migrate.
use super::{schema, Connection, StoreError};
use rusqlite::OpenFlags;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
        0xef53 | 0x5846_5342 | 0x9123_683e | 0x0102_1994 | 0x8584_58f6 | 0x794c_7630 | 0xf2f5_2010
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
        return Err(StoreError::NotInitialized(
            "Store filesystem is not admitted for local WAL".into(),
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
        return Err(StoreError::NotInitialized(
            "Store volume is not admitted for local WAL".into(),
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
        "docsum-store-v1:{}:{}:{}",
        identity.0,
        identity.1,
        schema::CURRENT_SCHEMA_VERSION
    )
}

fn verify_readiness(file: &mut File, identity: (u64, u64)) -> Result<(), StoreError> {
    if file.metadata()?.len() > 128 {
        return Err(StoreError::NotInitialized(
            "Invalid store readiness record".into(),
        ));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    file.take(129).read_to_string(&mut text)?;
    if text != readiness_record(identity) {
        return Err(StoreError::NotInitialized(
            "Prepared store identity does not match".into(),
        ));
    }
    Ok(())
}

impl Store {
    /// Bootstrap only while the application's existing lifecycle authority is held.
    /// Competing initializers are excluded; operations use attach/open below.
    pub fn initialize(path: &Path) -> Result<Self, StoreError> {
        let path = canonical_store_path(path)?;
        let guard = lock_file(&sidecar(&path, ".initialize.lock"), true)?;
        exclusive(&guard)?;
        let mut lease = lock_file(&sidecar(&path, ".ready.lock"), true)?;
        match lease.try_lock() {
            Ok(()) => {
                // No active owner: validate once and publish while the init gate
                // excludes attaches, including the exclusive-to-shared handoff.
                let bootstrap = initialize_connection(&path)?;
                // Setup's connection may not have entered the WAL read path.
                // Publish an operational anchor, not the journal-mode switcher.
                drop(bootstrap);
                let conn = operational_connection(&path, false)?;
                let identity = file_identity(&path)?;
                lease.set_len(0)?;
                lease.write_all(readiness_record(identity).as_bytes())?;
                lease.sync_data()?;
                lease.unlock()?;
                shared(&lease)?;
                Self::bind(path, identity, conn, lease)
            }
            Err(TryLockError::WouldBlock) => {
                // An active owner exists. Join its compatible prepared lifetime;
                // never migrate underneath it, even for a newer schema binary.
                shared(&lease)?;
                let conn = operational_connection(&path, false)?;
                let identity = file_identity(&path)?;
                verify_readiness(&mut lease, identity)?;
                Self::bind(path, identity, conn, lease)
            }
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }

    /// Attach only to an active, already validated owner. Never initialize or
    /// trust a stale marker left after process exit.
    pub fn attach(path: &Path) -> Result<Self, StoreError> {
        let path = fs::canonicalize(path)?;
        require_local_filesystem(path.parent().expect("canonical store parent"))?;
        let guard = lock_file(&sidecar(&path, ".initialize.lock"), false)?;
        shared(&guard)?;
        let mut lease = lock_file(&sidecar(&path, ".ready.lock"), false)?;
        match lease.try_lock() {
            Ok(()) => {
                return Err(StoreError::NotInitialized(
                    "No active prepared store owner".into(),
                ))
            }
            Err(TryLockError::WouldBlock) => shared(&lease)?,
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
        let conn = operational_connection(&path, false)?;
        let identity = file_identity(&path)?;
        verify_readiness(&mut lease, identity)?;
        Self::bind(path, identity, conn, lease)
    }

    fn bind(
        path: PathBuf,
        identity: (u64, u64),
        conn: Connection,
        lease: File,
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
        })))
    }

    pub fn path(&self) -> &Path {
        &self.0.path
    }

    fn check_identity(&self) -> Result<(), StoreError> {
        if file_identity(self.path())? != self.0.identity {
            return Err(StoreError::NotInitialized(
                "Prepared store file was replaced".into(),
            ));
        }
        Ok(())
    }

    fn open(&self, read_only: bool) -> Result<Connection, StoreError> {
        self.check_identity()?;
        let conn = operational_connection(self.path(), read_only)?;
        self.check_identity()?;
        Ok(conn)
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
        let before = schema::setup_counts();
        let store = Store::initialize(&file.0).unwrap();
        let initialized = schema::setup_counts();
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
            schema::setup_counts(),
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
    fn readiness_requires_an_active_owner_and_survives_clone_only() {
        let file = DatabaseFixture::new();
        assert!(Store::attach(&file.0).is_err());
        assert!(!file.0.exists());
        let store = Store::initialize(&file.0).unwrap();
        let clone = store.clone();
        drop(store);
        let attached = Store::attach(&file.0).unwrap();
        drop(clone);
        assert!(attached.reader().is_ok());
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
            let mut slot = store.0._anchor.lock().unwrap();
            let conn = std::mem::replace(&mut *slot, Connection::open_in_memory().unwrap());
            let busy: u32 = conn
                .query_row("PRAGMA main.wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
                .unwrap();
            assert_eq!(busy, 0);
            conn.close().unwrap();
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
            let counts = schema::setup_counts();
            let start = Instant::now();
            let reader = store.reader().unwrap();
            let count: u32 = reader
                .query_row("SELECT count(*) FROM cost_probe", [], |r| r.get(0))
                .unwrap();
            let read_ms = start.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(count, rows);
            assert_eq!(schema::setup_counts(), counts);
            eprintln!(
                "STORE_COST bytes={} bootstrap_ms={bootstrap_ms} operational_read_ms={read_ms}",
                fs::metadata(&file.0).unwrap().len()
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
            0x6573_5546,
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
