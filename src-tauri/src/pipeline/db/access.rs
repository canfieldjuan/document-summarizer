//! Desktop availability is explicit: ordinary opens never bootstrap a store.
use super::{Store, StoreError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub(crate) type StoreRetry = Arc<dyn Fn() -> Result<Store, StoreError> + Send + Sync>;

#[derive(Clone)]
pub(crate) struct StoreAccess {
    path: PathBuf,
    cached: Arc<Mutex<Option<Store>>>,
    retry: StoreRetry,
}

impl StoreAccess {
    pub(crate) fn new(path: PathBuf, initial: Option<Store>, retry: StoreRetry) -> Self {
        Self {
            path,
            cached: Arc::new(Mutex::new(initial)),
            retry,
        }
    }

    #[cfg(test)]
    pub(crate) fn prepared(store: Store) -> Self {
        let path = store.path().to_path_buf();
        let retry_path = path.clone();
        Self::new(
            path,
            Some(store),
            Arc::new(move || Store::attach_wait(&retry_path)),
        )
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn get(&self) -> Result<Store, StoreError> {
        let mut cached = self.cached.lock().map_err(|_| StoreError::Unavailable)?;
        if let Some(store) = cached.as_ref() {
            match store.check_ready() {
                Ok(()) => return Ok(store.clone()),
                Err(error) if super::is_contention(&error) => return Err(error),
                Err(_) => {
                    cached.take();
                }
            }
        }
        Err(StoreError::Unavailable)
    }

    /// Only the explicit startup/user Retry path may call the preparer.
    pub(crate) fn retry(&self) -> Result<Store, StoreError> {
        match self.get() {
            Ok(store) => return Ok(store),
            Err(error) if super::is_contention(&error) => return Err(error),
            Err(_) => {}
        }
        let mut cached = self.cached.lock().map_err(|_| StoreError::Unavailable)?;
        if let Some(store) = cached.as_ref() {
            store.check_ready()?;
            return Ok(store.clone());
        }
        let store = (self.retry)()?;
        store.check_ready()?;
        *cached = Some(store.clone());
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn unavailable_reads_never_initialize_and_explicit_retry_can_recover() {
        let file = crate::test_store::DatabaseFixture::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        let path = file.0.clone();
        let access = StoreAccess::new(
            path.clone(),
            None,
            Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                Store::initialize(&path)
            }),
        );
        assert!(access.get().is_err());
        assert!(!file.0.exists());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(access.retry().unwrap().reader().is_ok());
        assert!(access.get().unwrap().reader().is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
