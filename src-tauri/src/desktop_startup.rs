//! Application startup keeps storage failure separate from shell construction.
use crate::connect::provider::{ConnectProvider, ProviderStartError};
use crate::desktop::DesktopJobManager;
use crate::pipeline::db::{StoreAccess, StoreRetry};
use std::path::{Path, PathBuf};

pub(crate) struct Services {
    pub(crate) provider: Option<ConnectProvider>,
    pub(crate) jobs: DesktopJobManager,
}

pub(crate) fn prepare(
    app_data_dir: &Path,
    settings_path: PathBuf,
    start_provider: impl FnOnce(&Path) -> Result<ConnectProvider, ProviderStartError>,
    retry: StoreRetry,
) -> Result<Services, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(app_data_dir)?;
    crate::ensure_private_app_data_directory(app_data_dir)?;
    let db_path = app_data_dir.join("summarizer.db");
    let (provider, store) = match start_provider(&db_path) {
        Ok(provider) => {
            let store = provider.store();
            (Some(provider), Some(store))
        }
        Err(error) => {
            eprintln!("Connect provider unavailable; standalone mode continues: {error}");
            let store = match retry() {
                Ok(store) => Some(store),
                Err(error) => {
                    eprintln!("Document storage unavailable; desktop remains open: {error}");
                    None
                }
            };
            (None, store)
        }
    };
    let jobs = DesktopJobManager::with_store_access(
        StoreAccess::new(db_path, store, retry),
        settings_path,
    );
    if let Err(error) = jobs.ensure_ocr_recovery(app_data_dir.to_path_buf()) {
        eprintln!("OCR restart recovery unavailable: {error}");
    }
    Ok(Services { provider, jobs })
}

pub(crate) fn runtime_status(
    jobs: &DesktopJobManager,
    settings: &Path,
) -> crate::pipeline::workspace::RuntimeStatus {
    if let Err(error) = jobs.store() {
        let error = crate::CommandError::from(error);
        let busy = error.code == "PROVIDER_STORE_BUSY";
        return crate::pipeline::workspace::RuntimeStatus {
            provider_name: "Document storage".into(),
            ready: false,
            runtime_id: None,
            model_id: None,
            code: Some(error.code),
            message: if busy {
                "Document storage is busy. Try again.".into()
            } else {
                error.message
            },
            recoverable: true,
        };
    }
    crate::pipeline::workspace::inference_runtime_status(settings, jobs.db_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::db::StoreError;
    use std::sync::Arc;

    #[test]
    fn startup_failed_store_does_not_escape_shell_setup() {
        let root = tempfile::tempdir().unwrap();
        let services = prepare(
            root.path(),
            root.path().join("settings.json"),
            |_| Err(ProviderStartError::ProviderAlreadyRunning),
            Arc::new(|| {
                Err(StoreError::NotInitialized(
                    "bootstrap validation failed".into(),
                ))
            }),
        );
        assert!(services.is_ok(), "store failure escaped shell setup");
        let services = services.unwrap();
        assert!(services.provider.is_none());
        assert!(services.jobs.store().is_err());
        let status = runtime_status(&services.jobs, &root.path().join("settings.json"));
        assert!(!status.ready);
        assert_eq!(status.code.as_deref(), Some("DOCUMENT_STORE_UNAVAILABLE"));
        assert!(status.recoverable);
    }

    #[test]
    fn startup_attaches_to_the_provider_after_mid_initialization() {
        let root = tempfile::tempdir().unwrap();
        crate::ensure_private_app_data_directory(root.path()).unwrap();
        let path = root.path().join("summarizer.db");
        let ticket = crate::pipeline::db::Store::begin(&path).unwrap();
        let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
        let (primary_tx, primary_rx) = std::sync::mpsc::channel();
        let provider = std::thread::spawn(move || {
            waiting_rx.recv().unwrap();
            primary_tx
                .send(ticket.initialize_with_authority(()).unwrap())
                .unwrap();
        });
        let attach_path = path.clone();
        let services = prepare(
            root.path(),
            root.path().join("settings.json"),
            |_| Err(ProviderStartError::ProviderAlreadyRunning),
            Arc::new(move || {
                let _ = waiting_tx.send(());
                crate::pipeline::db::Store::attach_wait(&attach_path)
            }),
        )
        .unwrap();
        let primary = primary_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        // Startup may deliberately time out into an unavailable shell. A ready
        // owner must be usable on explicit Retry without a launch crash.
        if services.jobs.store().is_err() {
            services
                .jobs
                .retry_store(root.path().to_path_buf())
                .unwrap();
        }
        assert!(services.jobs.store().unwrap().reader().is_ok());
        assert!(services.provider.is_none());
        provider.join().unwrap();
        drop(primary);
        assert!(
            services.jobs.store().is_err(),
            "desktop inherited departed primary readiness"
        );
    }

    #[test]
    fn startup_reconcile_error_does_not_escape_shell_setup() {
        let root = tempfile::tempdir().unwrap();
        let services = prepare(
            root.path(),
            root.path().join("settings.json"),
            |_| Err(ProviderStartError::RuntimeDirectoryUnavailable),
            Arc::new(|| {
                Err(StoreError::FileAccess(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Windows reconcile ACL probe",
                )))
            }),
        );
        assert!(
            services.is_ok(),
            "non-store recovery failure escaped shell setup"
        );
    }
}
