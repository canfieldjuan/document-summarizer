use super::provider::{ConnectProvider, ProviderStartError};
use std::env;
use std::io;
use std::path::PathBuf;
use thiserror::Error;

const APP_DATA_DIRECTORY: &str = "com.juan-canfield.docsum";

#[derive(Debug, Error)]
pub(crate) enum BackgroundProviderError {
    #[error("Connect background provider data directory is unavailable")]
    DataDirectoryUnavailable,
    #[error("Connect background provider signal setup failed: {0}")]
    Signal(#[from] io::Error),
    #[error(transparent)]
    Provider(#[from] ProviderStartError),
}

pub(crate) fn run() -> Result<(), BackgroundProviderError> {
    let signals = BlockedShutdownSignals::new()?;
    let app_data_dir = app_data_dir()?;
    let db_path = app_data_dir.join("summarizer.db");
    let provider = ConnectProvider::start(db_path, app_data_dir)?;
    signals.wait()?;
    provider.shutdown();
    Ok(())
}

fn app_data_dir() -> Result<PathBuf, BackgroundProviderError> {
    if let Some(root) = env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(root).join(APP_DATA_DIRECTORY));
    }
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|home| home.join(".local/share").join(APP_DATA_DIRECTORY))
        .ok_or(BackgroundProviderError::DataDirectoryUnavailable)
}

struct BlockedShutdownSignals {
    shutdown: libc::sigset_t,
    previous: libc::sigset_t,
}

impl BlockedShutdownSignals {
    fn new() -> io::Result<Self> {
        let mut shutdown = unsafe { std::mem::zeroed() };
        let mut previous = unsafe { std::mem::zeroed() };
        if unsafe { libc::sigemptyset(&mut shutdown) } != 0
            || unsafe { libc::sigaddset(&mut shutdown, libc::SIGTERM) } != 0
            || unsafe { libc::sigaddset(&mut shutdown, libc::SIGINT) } != 0
        {
            return Err(io::Error::last_os_error());
        }
        let result = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &shutdown, &mut previous) };
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result));
        }
        Ok(Self { shutdown, previous })
    }

    fn wait(&self) -> io::Result<()> {
        let mut signal = 0;
        let result = unsafe { libc::sigwait(&self.shutdown, &mut signal) };
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result));
        }
        Ok(())
    }
}

impl Drop for BlockedShutdownSignals {
    fn drop(&mut self) {
        let result = unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.previous, std::ptr::null_mut())
        };
        if result != 0 {
            eprintln!("Connect background provider could not restore its signal mask");
        }
    }
}
