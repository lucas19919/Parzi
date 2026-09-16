//! How a lease reads and how its ids are spelled. Small, pure, and shared by
//! every file in this module — the words a person sees on a card ("held by
//! lane api · TSK-8") are decided here, once.

use std::collections::BTreeSet;

use parzi_core::error::ParziError;
use parzi_core::lease::{Holder, Lease};
use parzi_core::plan::TaskId;

#[must_use]
pub fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}

pub fn tool_err(msg: &str) -> ParziError {
    ParziError::Tool("lease".into(), msg.to_string())
}

/// Two holders are the same lane on the same machine: the lease follows the
/// lane, not the run (§4, `lane.take`).
#[must_use]
pub fn same_holder(a: &Holder, b: &Holder) -> bool {
    a.user == b.user && a.machine == b.machine && a.lane == b.lane
}

/// How a holder reads on a card and in a tool error.
#[must_use]
pub fn held_by(lease: &Lease) -> String {
    format!(
        "lane {} · {} ({}@{})",
        lease.holder.lane,
        lease.task.as_str(),
        lease.holder.user,
        lease.holder.machine
    )
}

pub(super) fn join(paths: &BTreeSet<String>) -> String {
    paths.iter().take(8).cloned().collect::<Vec<_>>().join(", ")
}

#[must_use]
pub fn task_str(t: &TaskId) -> String {
    t.as_str().to_string()
}

#[must_use]
pub fn task_id(s: &str) -> TaskId {
    TaskId(s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holders_are_the_lane_not_the_run() {
        let a = Holder {
            user: "ada".into(),
            machine: "desk".into(),
            run: Some("run-1".into()),
            lane: "api".into(),
        };
        let b = Holder {
            run: Some("run-2".into()),
            ..a.clone()
        };
        let c = Holder {
            lane: "web".into(),
            ..a.clone()
        };
        assert!(same_holder(&a, &b));
        assert!(!same_holder(&a, &c));
    }

    #[test]
    fn a_holder_reads_as_a_lane_and_a_task() {
        let lease = Lease {
            task: TaskId("TSK-8".into()),
            holder: Holder {
                user: "ada".into(),
                machine: "desk".into(),
                run: None,
                lane: "api".into(),
            },
            paths: ["api/src/routes.rs".to_string()].into_iter().collect(),
            granted_at: now(),
            last_seen: now(),
            ttl_secs: 90,
        };
        let text = held_by(&lease);
        assert!(
            text.contains("lane api") && text.contains("TSK-8"),
            "{text}"
        );
    }
}
