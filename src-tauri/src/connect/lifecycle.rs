use super::provider::{ConnectProvider, ProviderStartError, ProviderStopControl};
use std::env;
use std::io;
use std::path::PathBuf;
use std::thread;
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
    super::lifecycle_control::validate_background_launch_environment()
        .map_err(|_| BackgroundProviderError::DataDirectoryUnavailable)?;
    let signals = BlockedShutdownSignals::new()?;
    let stop_control = ProviderStopControl::new();
    signals.spawn_stop_notifier(stop_control.clone())?;
    let app_data_dir = app_data_dir()?;
    let db_path = app_data_dir.join("summarizer.db");
    let Some(provider) = acquire_after_foreground(
        || {
            ConnectProvider::start_background_with_control(
                db_path.clone(),
                app_data_dir.clone(),
                stop_control.clone(),
            )
        },
        |timeout| {
            if !timeout.is_zero() {
                thread::sleep(timeout);
            }
            Ok(stop_control.requested())
        },
    )?
    else {
        return Ok(());
    };
    loop {
        if let Some(error) = provider.terminal_failure() {
            provider.shutdown();
            crate::pipeline::llama_cpp::shutdown_managed_runtimes();
            return Err(error.into());
        }
        if stop_control.requested() {
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    provider.shutdown();
    crate::pipeline::llama_cpp::shutdown_managed_runtimes();
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

    fn spawn_stop_notifier(&self, control: ProviderStopControl) -> io::Result<()> {
        let shutdown = self.shutdown;
        thread::Builder::new()
            .name("connect-signal-waiter".to_string())
            .spawn(move || {
                let mut signal = 0;
                let result = unsafe { libc::sigwait(&shutdown, &mut signal) };
                if result == 0 && matches!(signal, libc::SIGTERM | libc::SIGINT) {
                    control.request_stop();
                }
            })
            .map(|_| ())
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
