use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub trait ExecutionControl: Send + Sync {
    fn cancellation_requested(&self) -> bool;
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
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }
}

impl ExecutionControl for CancellationToken {
    fn cancellation_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_token_is_shared_and_monotonic() {
        let token = CancellationToken::new();
        let observer = token.clone();

        assert!(!observer.cancellation_requested());
        token.request();
        assert!(observer.cancellation_requested());
    }

    #[test]
    fn uncontrolled_execution_never_requests_cancellation() {
        assert!(!UNCONTROLLED_EXECUTION.cancellation_requested());
    }
}
