use crate::pipeline::contracts::{PipelineFailure, PipelineRun, PipelineStage};
use crate::pipeline::db::{self, InterruptedRunAction, InterruptedRunTransition, StoreError};
use rusqlite::Connection;

pub const INTERRUPTION_FAILURE_CODE: &str = "PROCESS_INTERRUPTED";

pub fn reconcile_interrupted_runs(conn: &mut Connection) -> Result<Vec<PipelineRun>, StoreError> {
    let candidates = db::list_pipeline_runs_for_recovery(conn)?
        .into_iter()
        .filter_map(|run| {
            let action = if run.state == crate::pipeline::contracts::PipelineState::Cancelling {
                InterruptedRunAction::CompleteCancellation
            } else {
                InterruptedRunAction::Fail(interruption_failure(run.state.active_stage()?))
            };
            Some(InterruptedRunTransition {
                run_id: run.run_id,
                expected_state: run.state,
                expected_version: run.state_version,
                action,
            })
        })
        .collect::<Vec<_>>();

    db::reconcile_interrupted_runs_in_transaction(conn, &candidates)
}

fn interruption_failure(stage: PipelineStage) -> PipelineFailure {
    PipelineFailure {
        code: INTERRUPTION_FAILURE_CODE.to_string(),
        message: format!(
            "The application stopped while {} was in progress. Create a separate retry attempt to continue.",
            stage_label(&stage)
        ),
        stage: Some(stage),
        recoverable: true,
    }
}

fn stage_label(stage: &PipelineStage) -> &'static str {
    match stage {
        PipelineStage::Ingest => "ingestion",
        PipelineStage::Parse => "parsing",
        PipelineStage::Normalize => "normalization",
        PipelineStage::Structure => "structural interpretation",
        PipelineStage::Chunk => "chunking",
        PipelineStage::Analyze => "analysis",
        PipelineStage::Synthesize => "synthesis",
        PipelineStage::Verify => "verification",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contracts::{PipelineEvent, PipelineState};
    use crate::pipeline::db::{get_document, get_pipeline_run, init_db, list_pipeline_events};
    use crate::pipeline::ingest::ingest_pdf;
    use crate::pipeline::state::TransitionError;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct TestPath(PathBuf);

    impl TestPath {
        fn new(extension: &str, contents: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "doc-sum-recovery-{}.{}",
                Uuid::new_v4(),
                extension
            ));
            fs::write(&path, contents).expect("test path should be writable");
            Self(path)
        }

        fn empty(extension: &str) -> Self {
            Self::new(extension, b"")
        }
    }

    impl Drop for TestPath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn start_parsing_run(conn: &mut Connection, source: &TestPath) -> PipelineRun {
        let (_, ingested) = ingest_pdf(
            conn,
            source.0.to_str().expect("test source path should be UTF-8"),
        )
        .expect("source should ingest");
        db::start_parsing(conn, &ingested.run_id, ingested.state_version)
            .expect("parsing should start")
            .0
    }

    fn recovery_event(events: &[PipelineEvent]) -> &PipelineEvent {
        events.last().expect("recovery event should exist")
    }

    #[test]
    fn active_stage_mapping_is_explicit_and_excludes_stable_states() {
        let stages = [
            PipelineStage::Ingest,
            PipelineStage::Parse,
            PipelineStage::Normalize,
            PipelineStage::Structure,
            PipelineStage::Chunk,
            PipelineStage::Analyze,
            PipelineStage::Synthesize,
            PipelineStage::Verify,
        ];
        for (state, stage) in PipelineState::IMPLEMENTED_ACTIVE_STATES
            .into_iter()
            .zip(stages)
        {
            assert_eq!(state.active_stage(), Some(stage));
        }

        let inactive = [
            PipelineState::Received,
            PipelineState::Ingested,
            PipelineState::Parsed,
            PipelineState::VisualAnalysisRequired,
            PipelineState::VisualAnalyzing,
            PipelineState::VisualAnalyzed,
            PipelineState::Normalized,
            PipelineState::Structured,
            PipelineState::Chunked,
            PipelineState::Analyzed,
            PipelineState::Synthesized,
            PipelineState::Verified,
            PipelineState::Complete,
            PipelineState::CompleteWithWarnings,
            PipelineState::Failed,
            PipelineState::Cancelling,
            PipelineState::Cancelled,
        ];
        for state in inactive {
            assert_eq!(state.active_stage(), None);
        }
    }

    #[test]
    fn interrupted_run_is_failed_once_and_survives_independent_reopen() {
        let source = TestPath::new("pdf", b"%PDF-1.4\nRECOVERY_SOURCE_BYTES");
        let database = TestPath::empty("db");
        let (stable, parsing, document, events_before) = {
            let mut conn = init_db(&database.0).expect("database should initialize");
            let (document, stable) = ingest_pdf(
                &mut conn,
                source.0.to_str().expect("source path should be UTF-8"),
            )
            .expect("stable source should ingest");
            let parsing = start_parsing_run(&mut conn, &source);
            let events = list_pipeline_events(&conn, &parsing.run_id)
                .expect("pre-recovery events should load");
            (stable, parsing, document, events)
        };

        let mut reopened = init_db(&database.0).expect("database should reopen independently");
        let recovered = reconcile_interrupted_runs(&mut reopened)
            .expect("interrupted run should reconcile atomically");
        assert_eq!(recovered.len(), 1);
        let failed = &recovered[0];
        assert_eq!(failed.run_id, parsing.run_id);
        assert_eq!(failed.state, PipelineState::Failed);
        assert_eq!(failed.state_version, parsing.state_version + 1);
        assert_eq!(failed.current_stage, Some(PipelineStage::Parse));
        assert!(failed.completed_at.is_some());
        assert!(failed.resumable);
        let failure = failed.failure.as_ref().expect("failure should persist");
        assert_eq!(failure.code, INTERRUPTION_FAILURE_CODE);
        assert_eq!(failure.stage, Some(PipelineStage::Parse));
        assert!(failure.recoverable);

        let events_after =
            list_pipeline_events(&reopened, &parsing.run_id).expect("recovery events should load");
        assert_eq!(events_after.len(), events_before.len() + 1);
        let event = recovery_event(&events_after);
        assert_eq!(event.sequence_no, events_before.len() as u32);
        assert_eq!(event.previous_state, Some(PipelineState::Parsing));
        assert_eq!(event.next_state, PipelineState::Failed);
        assert_eq!(event.stage, Some(PipelineStage::Parse));
        assert_eq!(event.reason.as_deref(), Some(INTERRUPTION_FAILURE_CODE));

        assert_eq!(
            get_pipeline_run(&reopened, &stable.run_id)
                .expect("stable run should load")
                .expect("stable run should exist"),
            stable
        );
        assert_eq!(
            get_document(&reopened, &document.document_id)
                .expect("document should load")
                .expect("document should exist"),
            document
        );
        assert_eq!(
            fs::read(&source.0).expect("source should remain readable"),
            b"%PDF-1.4\nRECOVERY_SOURCE_BYTES"
        );

        let snapshot = get_pipeline_run(&reopened, &parsing.run_id)
            .expect("failed run should load")
            .expect("failed run should exist");
        assert!(reconcile_interrupted_runs(&mut reopened)
            .expect("repeated reconciliation should succeed")
            .is_empty());
        assert_eq!(
            get_pipeline_run(&reopened, &parsing.run_id)
                .expect("failed run should reload")
                .expect("failed run should exist"),
            snapshot
        );
        drop(reopened);

        let independently_reopened =
            init_db(&database.0).expect("database should reopen after reconciliation");
        assert_eq!(
            get_pipeline_run(&independently_reopened, &parsing.run_id)
                .expect("reconciled run should load")
                .expect("reconciled run should exist"),
            snapshot
        );
        assert_eq!(
            list_pipeline_events(&independently_reopened, &parsing.run_id)
                .expect("reconciled events should persist"),
            events_after
        );
    }

    #[test]
    fn interrupted_cancellation_completes_once_after_independent_reopen() {
        let source = TestPath::new("pdf", b"%PDF-1.4\nCANCELLATION_RECOVERY_BYTES");
        let database = TestPath::empty("db");
        let cancelling = {
            let mut conn = init_db(&database.0).expect("database should initialize");
            let parsing = start_parsing_run(&mut conn, &source);
            db::request_cancellation(&mut conn, &parsing.run_id, parsing.state_version)
                .expect("cancellation request should persist")
        };

        let mut reopened = init_db(&database.0).expect("database should reopen independently");
        let recovered = reconcile_interrupted_runs(&mut reopened)
            .expect("interrupted cancellation should reconcile");
        assert_eq!(recovered.len(), 1);
        let cancelled = &recovered[0];
        assert_eq!(cancelled.run_id, cancelling.run_id);
        assert_eq!(cancelled.state, PipelineState::Cancelled);
        assert_eq!(cancelled.state_version, cancelling.state_version + 1);
        assert!(cancelled.cancellation_requested);
        assert!(cancelled.completed_at.is_some());
        let events = list_pipeline_events(&reopened, &cancelled.run_id)
            .expect("cancellation history should survive reopen");
        let event = events.last().expect("recovery event should exist");
        assert_eq!(event.previous_state, Some(PipelineState::Cancelling));
        assert_eq!(event.next_state, PipelineState::Cancelled);
        assert_eq!(
            event.reason.as_deref(),
            Some("cancellation_completed_after_restart")
        );

        let second = reconcile_interrupted_runs(&mut reopened)
            .expect("repeated recovery should be idempotent");
        assert!(second.is_empty());
        assert_eq!(
            list_pipeline_events(&reopened, &cancelled.run_id)
                .expect("events should remain readable"),
            events
        );
    }

    #[test]
    fn recovery_rejects_cancelling_state_without_durable_request_flag() {
        let source = TestPath::new("pdf", b"%PDF-1.4\nINVALID_CANCELLATION_RECOVERY");
        let database = TestPath::empty("db");
        let cancelling = {
            let mut conn = init_db(&database.0).expect("database should initialize");
            let parsing = start_parsing_run(&mut conn, &source);
            let cancelling =
                db::request_cancellation(&mut conn, &parsing.run_id, parsing.state_version)
                    .expect("cancellation request should persist");
            conn.execute(
                "UPDATE pipeline_runs SET cancellation_requested = 0 WHERE run_id = ?1",
                [&cancelling.run_id],
            )
            .expect("test should create an inconsistent durable row");
            cancelling
        };

        let mut reopened = init_db(&database.0).expect("database should reopen independently");
        let events_before = list_pipeline_events(&reopened, &cancelling.run_id)
            .expect("events should load before recovery");
        let rejected = reconcile_interrupted_runs(&mut reopened);
        assert!(matches!(
            rejected,
            Err(StoreError::InvalidRecoveryTransition {
                state: PipelineState::Cancelling
            })
        ));
        let persisted = get_pipeline_run(&reopened, &cancelling.run_id)
            .expect("run should reload")
            .expect("run should remain present");
        assert_eq!(persisted.state, PipelineState::Cancelling);
        assert_eq!(persisted.state_version, cancelling.state_version);
        assert!(!persisted.cancellation_requested);
        assert_eq!(
            list_pipeline_events(&reopened, &cancelling.run_id)
                .expect("rejected recovery must not add an event"),
            events_before
        );
    }

    #[test]
    fn recovery_event_failure_rolls_back_the_entire_batch() {
        let source = TestPath::new("pdf", b"%PDF-1.4\nRECOVERY_BATCH_BYTES");
        let mut conn = init_db(":memory:").expect("database should initialize");
        let mut parsing_runs = vec![
            start_parsing_run(&mut conn, &source),
            start_parsing_run(&mut conn, &source),
        ];
        parsing_runs.sort_by(|left, right| left.run_id.cmp(&right.run_id));
        let trigger_run_id = &parsing_runs[1].run_id;
        conn.execute_batch(&format!(
            "CREATE TRIGGER fail_second_recovery_event
             BEFORE INSERT ON pipeline_events
             WHEN NEW.reason = '{INTERRUPTION_FAILURE_CODE}'
                  AND NEW.run_id = '{trigger_run_id}'
             BEGIN
                 SELECT RAISE(ABORT, 'injected recovery event failure');
             END;"
        ))
        .expect("failure trigger should install");

        let result = reconcile_interrupted_runs(&mut conn);
        assert!(matches!(result, Err(StoreError::Sqlite(_))));
        for original in &parsing_runs {
            let persisted = get_pipeline_run(&conn, &original.run_id)
                .expect("run should load after rollback")
                .expect("run should exist after rollback");
            assert_eq!(persisted.state, PipelineState::Parsing);
            assert_eq!(persisted.state_version, original.state_version);
            assert_eq!(
                recovery_event(
                    &list_pipeline_events(&conn, &original.run_id)
                        .expect("events should load after rollback")
                )
                .next_state,
                PipelineState::Parsing
            );
        }

        conn.execute_batch("DROP TRIGGER fail_second_recovery_event;")
            .expect("failure trigger should drop");
        let recovered = reconcile_interrupted_runs(&mut conn)
            .expect("both interrupted runs should reconcile after trigger removal");
        assert_eq!(
            recovered
                .iter()
                .map(|run| run.run_id.as_str())
                .collect::<Vec<_>>(),
            parsing_runs
                .iter()
                .map(|run| run.run_id.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn recovery_storage_boundary_rejects_mixed_and_malformed_candidates() {
        let source = TestPath::new("pdf", b"%PDF-1.4\nRECOVERY_GUARD_BYTES");
        let mut conn = init_db(":memory:").expect("database should initialize");
        let (_, stable) = ingest_pdf(
            &mut conn,
            source.0.to_str().expect("source path should be UTF-8"),
        )
        .expect("stable source should ingest");
        let parsing = start_parsing_run(&mut conn, &source);
        let parsing_events =
            list_pipeline_events(&conn, &parsing.run_id).expect("parsing events should load");
        let valid = InterruptedRunTransition {
            run_id: parsing.run_id.clone(),
            expected_state: parsing.state.clone(),
            expected_version: parsing.state_version,
            action: InterruptedRunAction::Fail(interruption_failure(PipelineStage::Parse)),
        };
        let invalid_stable = InterruptedRunTransition {
            run_id: stable.run_id.clone(),
            expected_state: stable.state.clone(),
            expected_version: stable.state_version,
            action: InterruptedRunAction::Fail(interruption_failure(PipelineStage::Ingest)),
        };

        let mixed = db::reconcile_interrupted_runs_in_transaction(
            &mut conn,
            &[valid.clone(), invalid_stable],
        );
        assert!(matches!(
            mixed,
            Err(StoreError::InvalidRecoveryTransition {
                state: PipelineState::Ingested
            })
        ));
        assert_eq!(
            get_pipeline_run(&conn, &parsing.run_id)
                .expect("parsing run should reload")
                .expect("parsing run should exist"),
            parsing
        );
        assert_eq!(
            list_pipeline_events(&conn, &parsing.run_id).expect("parsing events should reload"),
            parsing_events
        );
        assert_eq!(
            get_pipeline_run(&conn, &stable.run_id)
                .expect("stable run should reload")
                .expect("stable run should exist"),
            stable
        );

        let malformed = InterruptedRunTransition {
            action: InterruptedRunAction::Fail(PipelineFailure {
                code: "OTHER_FAILURE".to_string(),
                message: "Malformed recovery candidate".to_string(),
                stage: Some(PipelineStage::Parse),
                recoverable: true,
            }),
            ..valid
        };
        let rejected = db::reconcile_interrupted_runs_in_transaction(&mut conn, &[malformed]);
        assert!(matches!(
            rejected,
            Err(StoreError::InvalidRecoveryTransition {
                state: PipelineState::Parsing
            })
        ));
        assert_eq!(
            get_pipeline_run(&conn, &parsing.run_id)
                .expect("rejected run should reload")
                .expect("rejected run should exist"),
            parsing
        );
    }

    #[test]
    fn stale_recovery_snapshot_cannot_overwrite_a_newer_failure() {
        let source = TestPath::new("pdf", b"%PDF-1.4\nRECOVERY_CAS_BYTES");
        let database = TestPath::empty("db");
        let mut caller_a = init_db(&database.0).expect("database should initialize");
        let parsing = start_parsing_run(&mut caller_a, &source);
        let mut caller_b = init_db(&database.0).expect("second connection should open");
        let observed = get_pipeline_run(&caller_b, &parsing.run_id)
            .expect("run should load")
            .expect("run should exist");
        let candidate = InterruptedRunTransition {
            run_id: observed.run_id.clone(),
            expected_state: observed.state.clone(),
            expected_version: observed.state_version,
            action: InterruptedRunAction::Fail(interruption_failure(PipelineStage::Parse)),
        };

        let newer_failure = PipelineFailure {
            code: "PARSER_STOPPED".to_string(),
            message: "Parsing stopped independently.".to_string(),
            stage: Some(PipelineStage::Parse),
            recoverable: true,
        };
        let newer = db::fail_parsing(
            &mut caller_a,
            &parsing.run_id,
            parsing.state_version,
            newer_failure,
        )
        .expect("first caller should persist its failure");
        let events_after_first = list_pipeline_events(&caller_a, &parsing.run_id)
            .expect("first caller events should load");

        let stale = db::reconcile_interrupted_runs_in_transaction(&mut caller_b, &[candidate]);
        assert!(matches!(
            stale,
            Err(StoreError::Transition(
                TransitionError::StaleExpectedState { .. }
            ))
        ));
        assert_eq!(
            get_pipeline_run(&caller_b, &parsing.run_id)
                .expect("run should reload")
                .expect("run should exist"),
            newer
        );
        assert_eq!(
            list_pipeline_events(&caller_b, &parsing.run_id).expect("events should reload"),
            events_after_first
        );
    }
}
