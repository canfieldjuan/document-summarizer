use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub trait ExecutionControl: Send + Sync {
    fn cancellation_requested(&self) -> bool;

    fn request_timeout(&self) -> Option<Duration> {
        None
    }
}

pub struct UncontrolledExecution;

impl ExecutionControl for UncontrolledExecution {
    fn cancellation_requested(&self) -> bool {
        false
    }
}

pub static UNCONTROLLED_EXECUTION: UncontrolledExecution = UncontrolledExecution;

#[derive(Clone, Default)]
pub struct CancellationToken {
    requested: Arc<AtomicBool>,
    request_timeout: Option<Duration>,
    deadline: Option<Instant>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_request_timeout(request_timeout: Duration) -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
            request_timeout: Some(request_timeout),
            deadline: None,
        }
    }

    pub fn child_with_deadline(&self, timeout: Duration) -> Self {
        Self {
            requested: Arc::clone(&self.requested),
            request_timeout: Some(timeout),
            deadline: Instant::now().checked_add(timeout),
        }
    }

    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }
}

impl ExecutionControl for CancellationToken {
    fn cancellation_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
            || self
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
    }

    fn request_timeout(&self) -> Option<Duration> {
        match (self.request_timeout, self.deadline) {
            (Some(timeout), Some(deadline)) => {
                Some(timeout.min(deadline.saturating_duration_since(Instant::now())))
            }
            (timeout, None) => timeout,
            (None, Some(deadline)) => Some(deadline.saturating_duration_since(Instant::now())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_token_is_shared_and_monotonic() {
        let token = CancellationToken::with_request_timeout(Duration::from_secs(30));
        let observer = token.clone();

        assert!(!observer.cancellation_requested());
        assert_eq!(observer.request_timeout(), Some(Duration::from_secs(30)));
        token.request();
        assert!(observer.cancellation_requested());
    }

    #[test]
    fn uncontrolled_execution_never_requests_cancellation() {
        assert!(!UNCONTROLLED_EXECUTION.cancellation_requested());
    }

    #[test]
    fn child_deadline_bounds_the_complete_job_not_each_request() {
        let owner = CancellationToken::new();
        let child = owner.child_with_deadline(Duration::from_millis(5));
        assert!(!child.cancellation_requested());
        std::thread::sleep(Duration::from_millis(10));
        assert!(child.cancellation_requested());
        assert_eq!(child.request_timeout(), Some(Duration::ZERO));
    }
}
