//! Own the complete temporary store directory, including SQLite sidecars.
use std::path::{Path, PathBuf};

pub struct DatabaseFixture(pub PathBuf, tempfile::TempDir);

impl DatabaseFixture {
    pub fn new() -> Self {
        Self::named("store")
    }

    pub fn named(label: &str) -> Self {
        let directory = tempfile::Builder::new()
            .prefix(&format!("doc-sum-{label}-"))
            .tempdir()
            .expect("private test store directory");
        Self(directory.path().join("summarizer.db"), directory)
    }

    pub fn directory(&self) -> &Path {
        self.1.path()
    }
}

#[test]
fn temporary_store_owns_its_entire_directory() {
    let store = DatabaseFixture::new();
    let directory = store.directory().to_path_buf();
    std::fs::write(&store.0, b"test main file").unwrap();
    std::fs::write(store.0.with_extension("db-wal"), b"test sidecar").unwrap();
    std::fs::write(store.0.with_extension("db-shm"), b"test sidecar").unwrap();
    drop(store);
    assert!(!directory.exists());
}
