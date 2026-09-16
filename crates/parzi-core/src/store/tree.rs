//! Pure subtree maths, kept free of IO so it stays unit-testable.

use std::collections::{HashMap, HashSet};

use super::model::{SessionMeta, SessionStatus};

/// `root` plus all of its descendants at any depth. Unknown roots yield just
/// themselves so callers still remove the dir when listing raced.
pub fn subtree_ids(all: &[SessionMeta], root: &str) -> HashSet<String> {
    let by_parent = children_map(all);
    let mut out: HashSet<String> = HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !out.insert(id.to_string()) {
            continue;
        }
        if let Some(kids) = by_parent.get(id) {
            for k in kids {
                stack.push(k);
            }
        }
    }
    out
}

/// Every finished session plus all of its descendants at any depth.
pub fn cascade_kill_ids(all: &[SessionMeta]) -> HashSet<String> {
    let by_parent = children_map(all);
    let done: HashSet<&str> = all
        .iter()
        .filter(|m| matches!(m.status, SessionStatus::Done | SessionStatus::Killed))
        .map(|m| m.id.as_str())
        .collect();
    let mut kill: HashSet<String> = done.iter().map(|s| (*s).to_string()).collect();
    let mut stack: Vec<&str> = done.into_iter().collect();
    while let Some(id) = stack.pop() {
        if let Some(kids) = by_parent.get(id) {
            for k in kids {
                if kill.insert((*k).to_string()) {
                    stack.push(k);
                }
            }
        }
    }
    kill
}

fn children_map(all: &[SessionMeta]) -> HashMap<&str, Vec<&str>> {
    let mut by_parent: HashMap<&str, Vec<&str>> = HashMap::new();
    for m in all {
        if let Some(pid) = &m.parent_id {
            by_parent
                .entry(pid.as_str())
                .or_default()
                .push(m.id.as_str());
        }
    }
    by_parent
}
