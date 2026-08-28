use crate::pipeline::contracts::{PipelineEvent, PipelineRun, PipelineStage, PipelineState};
use chrono::Utc;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum TransitionError {
    #[error("Stale state: expected {expected:?}, found {found:?}")]
    StaleExpectedState {
        expected: PipelineState,
        found: PipelineState,
    },
    #[error("Invalid transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: PipelineState,
        to: PipelineState,
    },
    #[error("Concurrent modification: expected version {expected}, found {found}")]
    ConcurrentModification { expected: u32, found: u32 },
    #[error("State version cannot be incremented beyond {0}")]
    VersionExhausted(u32),
}

pub struct StateMachine;

impl StateMachine {
    pub fn transition(
        run: &mut PipelineRun,
        expected_state: PipelineState,
        next_state: PipelineState,
        expected_version: u32,
        sequence_no: u32,
        stage: Option<PipelineStage>,
        reason: Option<String>,
    ) -> Result<PipelineEvent, TransitionError> {
        if run.state != expected_state {
            return Err(TransitionError::StaleExpectedState {
                expected: expected_state,
                found: run.state.clone(),
            });
        }

        if run.state_version != expected_version {
            return Err(TransitionError::ConcurrentModification {
                expected: expected_version,
                found: run.state_version,
            });
        }

        if !Self::is_valid_transition(&run.state, &next_state) {
            return Err(TransitionError::InvalidTransition {
                from: run.state.clone(),
                to: next_state,
            });
        }

        let next_version = run
            .state_version
            .checked_add(1)
            .ok_or(TransitionError::VersionExhausted(run.state_version))?;
        let now = Utc::now();
        let event = PipelineEvent {
            event_id: Uuid::new_v4().to_string(),
            run_id: run.run_id.clone(),
            sequence_no,
            previous_state: Some(run.state.clone()),
            next_state: next_state.clone(),
            timestamp: now,
            stage: stage.clone(),
            work_unit_id: None,
            reason,
        };

        run.state = next_state.clone();
        run.state_version = next_version;
        run.updated_at = now;
        if let Some(s) = stage {
            run.current_stage = Some(s);
        }

        if matches!(
            next_state,
            PipelineState::Complete
                | PipelineState::CompleteWithWarnings
                | PipelineState::Failed
                | PipelineState::Cancelled
        ) {
            run.completed_at = Some(now);
        } else if matches!(next_state, PipelineState::Ingesting) && run.started_at.is_none() {
            run.started_at = Some(now);
        }

        Ok(event)
    }

    pub fn is_valid_transition(from: &PipelineState, to: &PipelineState) -> bool {
        use PipelineState::*;

        // Any state can go to Cancelling or Failed (except terminal states)
        if matches!(from, Complete | CompleteWithWarnings | Failed | Cancelled) {
            return false;
        }
        if matches!(to, Failed | Cancelling) {
            return true;
        }

        match (from, to) {
            (Received, Ingesting) => true,
            (Ingesting, Ingested) => true,

            (Ingested, Parsing) => true,
            (Parsing, Parsed) => true,

            (Parsed, Normalizing) => true,
            (Normalizing, Normalized) => true,

            (Normalized, Structuring) => true,
            (Structuring, Structured) => true,

            (Structured, Chunking) => true,
            (Chunking, Chunked) => true,

            (Chunked, Analyzing) => true,
            (Analyzing, Analyzed) => true,

            (Analyzed, Synthesizing) => true,
            (Synthesizing, Synthesized) => true,

            (Synthesized, Verifying) => true,
            (Verifying, Verified) => true,

            (Verified, Complete) => true,
            (Verified, CompleteWithWarnings) => true,

            (Cancelling, Cancelled) => true,

            // No implicit retry or resume edges are implemented yet.
            _ => false,
        }
    }
}
