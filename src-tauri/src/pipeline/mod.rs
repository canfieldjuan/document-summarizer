pub mod chunk;
pub mod contracts;
pub mod db;
pub mod ingest;
pub mod normalize;
pub mod parser;
mod schema;
pub mod state;
pub mod structure;

#[cfg(test)]
mod tests {
    use super::contracts::{PipelineProgress, PipelineRun, PipelineStage, PipelineState};
    use super::state::StateMachine;
    use chrono::Utc;
    use uuid::Uuid;

    fn create_run() -> PipelineRun {
        PipelineRun {
            run_id: Uuid::new_v4().to_string(),
            document_id: Uuid::new_v4().to_string(),
            state: PipelineState::Received,
            state_version: 1,
            pipeline_version: "1.0".to_string(),
            created_at: Utc::now(),
            started_at: None,
            updated_at: Utc::now(),
            completed_at: None,
            current_stage: None,
            progress: PipelineProgress {
                total_units: 0,
                completed_units: 0,
                failed_units: 0,
            },
            warnings: vec![],
            failure: None,
            cancellation_requested: false,
            resumable: true,
        }
    }

    #[test]
    fn test_valid_transition() {
        let mut run = create_run();

        // Transition to Ingesting
        let event = StateMachine::transition(
            &mut run,
            PipelineState::Received,
            PipelineState::Ingesting,
            1,
            1,
            Some(PipelineStage::Ingest),
            None,
        )
        .expect("Should transition to Ingesting");

        assert_eq!(run.state, PipelineState::Ingesting);
        assert_eq!(run.state_version, 2);
        assert_eq!(run.current_stage, Some(PipelineStage::Ingest));
        assert!(run.started_at.is_some());
        assert_eq!(event.next_state, PipelineState::Ingesting);

        // Transition to Ingested
        let _event = StateMachine::transition(
            &mut run,
            PipelineState::Ingesting,
            PipelineState::Ingested,
            2,
            2,
            None,
            None,
        )
        .expect("Should transition to Ingested");

        assert_eq!(run.state, PipelineState::Ingested);
        assert_eq!(run.state_version, 3);
    }

    #[test]
    fn test_invalid_transition() {
        let mut run = create_run();

        let result = StateMachine::transition(
            &mut run,
            PipelineState::Received,
            PipelineState::Parsed, // Can't go directly from Received to Parsed
            1,
            1,
            None,
            None,
        );

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            super::state::TransitionError::InvalidTransition { .. }
        ));
    }

    #[test]
    fn test_concurrent_modification() {
        let mut run = create_run();

        let result = StateMachine::transition(
            &mut run,
            PipelineState::Received,
            PipelineState::Ingesting,
            2, // Wrong version, should be 1
            1,
            None,
            None,
        );

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            super::state::TransitionError::ConcurrentModification { .. }
        ));
        assert_eq!(run.state, PipelineState::Received);
        assert_eq!(run.state_version, 1);
    }

    #[test]
    fn test_stale_expected_state_does_not_mutate_run() {
        let mut run = create_run();
        run.state = PipelineState::Ingested;
        run.state_version = 3;

        let result = StateMachine::transition(
            &mut run,
            PipelineState::Received,
            PipelineState::Ingesting,
            3,
            3,
            None,
            None,
        );

        assert!(matches!(
            result,
            Err(super::state::TransitionError::StaleExpectedState { .. })
        ));
        assert_eq!(run.state, PipelineState::Ingested);
        assert_eq!(run.state_version, 3);
    }

    #[test]
    fn exhausted_state_version_rejects_transition_without_mutation() {
        let mut run = create_run();
        run.state_version = u32::MAX;
        let original = run.clone();

        let result = StateMachine::transition(
            &mut run,
            PipelineState::Received,
            PipelineState::Ingesting,
            u32::MAX,
            u32::MAX,
            Some(PipelineStage::Ingest),
            None,
        );

        assert!(matches!(
            result,
            Err(super::state::TransitionError::VersionExhausted(u32::MAX))
        ));
        assert_eq!(run, original);
    }
}
