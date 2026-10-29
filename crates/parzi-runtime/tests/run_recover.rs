mod common;

use common::*;
use parzi_core::store::{RunEndKind, RunOutcome, SessionStatus};

#[tokio::test]
async fn a_run_cut_off_by_a_restart_reads_as_interrupted() {
    home("recover");
    let (orch, store) = orch(&[]);
    let m = store.create("cut off", "recover-tests", "claude").unwrap();
    assert_eq!(m.status, SessionStatus::Active);
    assert_eq!(orch.recover().unwrap(), 1);
    let after = store.get(&m.id).unwrap();
    assert_eq!(after.status, SessionStatus::Idle);
    let out = after.last_run.unwrap();
    assert_eq!(
        out,
        RunOutcome {
            ended: out.ended,
            ..RunOutcome::bare(RunEndKind::Interrupted, 0)
        }
    );
    assert!(out.ended > 0);
}
