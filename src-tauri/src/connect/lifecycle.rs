use super::provider::{ConnectProvider, ProviderStartError};
use std::env;
use std::io;
use std::path::PathBuf;
use std::time::Duration;
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
    let Some(provider) = acquire_after_foreground(
        || {
            ConnectProvider::start_background_with_control(
                db_path.clone(),
                app_data_dir.clone(),
                || signals.wait_timeout(Duration::ZERO).unwrap_or(true),
            )
        },
        |timeout| signals.wait_timeout(timeout),
    )?
    else {
        return Ok(());
    };
    loop {
        if let Some(error) = provider.terminal_failure() {
            provider.shutdown();
            return Err(error.into());
        }
        if signals.wait_timeout(Duration::from_millis(250))? {
            break;
        }
    }
    provider.shutdown();
    Ok(())
}

fn acquire_after_foreground<T>(
    mut start: impl FnMut() -> Result<T, ProviderStartError>,
    mut stop_requested: impl FnMut(Duration) -> io::Result<bool>,
) -> Result<Option<T>, BackgroundProviderError> {
    loop {
        if stop_requested(Duration::ZERO)? {
            return Ok(None);
        }
        match start() {
            Ok(provider) => return Ok(Some(provider)),
            Err(ProviderStartError::ProviderAlreadyRunning) => {
                if stop_requested(Duration::from_millis(250))? {
                    return Ok(None);
                }
            }
            Err(ProviderStartError::StartupCancelled) => return Ok(None),
            Err(error) => return Err(error.into()),
        }
    }
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

    fn wait_timeout(&self, timeout: Duration) -> io::Result<bool> {
        let timeout = libc::timespec {
            tv_sec: timeout.as_secs().try_into().unwrap_or(libc::time_t::MAX),
            tv_nsec: timeout.subsec_nanos().into(),
        };
        let mut info = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::sigtimedwait(&self.shutdown, &mut info, &timeout) };
        if result == libc::SIGTERM || result == libc::SIGINT {
            return Ok(true);
        }
        if result == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::EAGAIN) {
            return Ok(false);
        }
        Err(io::Error::last_os_error())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn background_retry_loop_waits_for_foreground_release_without_exiting() {
        let attempts = AtomicUsize::new(0);
        let retry_waits = AtomicUsize::new(0);
        let acquired = acquire_after_foreground(
            || match attempts.fetch_add(1, Ordering::SeqCst) {
                0 | 1 => Err(ProviderStartError::ProviderAlreadyRunning),
                _ => Ok("background-owner"),
            },
            |timeout| {
                if !timeout.is_zero() {
                    retry_waits.fetch_add(1, Ordering::SeqCst);
                }
                Ok(false)
            },
        )
        .unwrap();
        assert_eq!(acquired, Some("background-owner"));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(retry_waits.load(Ordering::SeqCst), 2);
    }
}
