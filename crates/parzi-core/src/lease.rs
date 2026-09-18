//! File leases (PLAN §4): who holds which files, for which task, until when.
//!
//! Pure and in memory. The table knows nothing about PROJECT.md, the hub or
//! the filesystem — a caller asks `project::is_critical` when a transfer
//! needs a human, and the journal records what happened. Everything that
//! needs a clock takes one (`expire`) or stamps `Utc::now()` once.
//!
//! A lease path is a file, a folder or a glob. A path holds itself and
//! everything under it; a glob holds every path it matches and everything
//! under those. Case is ignored where the file system ignores it.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};
use crate::plan::TaskId;
use crate::project::{match_segments, normalize_path, path_key, segments};

/// A lease lives while its heartbeat does (§4).
pub const TTL_SECS: u64 = 90;
/// No answer to a request in this long is a deny (§4).
pub const REQUEST_TIMEOUT_SECS: i64 = 120;
/// Two requests per file per task, then the orchestrator convenes (§15.6).
pub const MAX_REQUESTS_PER_PATH: usize = 2;

/// Who holds a lease. `run` is the session id when one is live; `lane` is the
/// plan lane, which is what a person reads on the card.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Holder {
    pub user: String,
    pub machine: String,
    #[serde(default)]
    pub run: Option<String>,
    pub lane: String,
}

impl std::fmt::Display for Holder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} · lane {}", self.user, self.lane)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub task: TaskId,
    pub holder: Holder,
    /// Repo-relative and repo-prefixed: `shop-api/src/routes.rs`.
    pub paths: BTreeSet<String>,
    /// Paths inside `paths` handed to another lane since (a granted request
    /// for one file of a claimed folder): the lease no longer holds them.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub given: BTreeSet<String>,
    pub granted_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub ttl_secs: u64,
}

impl Lease {
    /// Past its TTL: the card shows *stale* and `expire` will drop it (§15.6).
    #[must_use]
    pub fn is_stale(&self, now: DateTime<Utc>) -> bool {
        now - self.last_seen > Duration::seconds(i64::try_from(self.ttl_secs).unwrap_or(i64::MAX))
    }

    /// Does the lease hold this file now: claimed, and not handed on since.
    #[must_use]
    pub fn holds(&self, path: &str) -> bool {
        self.in_scope(path) && !self.given.iter().any(|g| covers(g, path))
    }

    /// Is this file inside what the lease claimed, handed on or not? That
    /// is the task's scope: a write outside it is scope creep.
    #[must_use]
    pub fn in_scope(&self, path: &str) -> bool {
        self.paths.iter().any(|h| covers(h, path))
    }

    /// Could the lease hold anything `path` names (a file, a folder or a
    /// glob)? What it has handed on does not count.
    fn meets(&self, path: &str) -> bool {
        self.paths.iter().any(|h| overlap(h, path)) && !self.given.iter().any(|g| contains(g, path))
    }
}

/// Does the lease path `held` hold `path`: is `path`, or a folder above it,
/// matched by `held`?
#[must_use]
pub fn covers(held: &str, path: &str) -> bool {
    let (held, path) = (path_key(held), path_key(path));
    let (h, p) = (segments(&held), segments(&path));
    !h.is_empty() && (1..=p.len()).any(|n| match_segments(&h, &p[..n]))
}

/// Can two lease paths hold one file? Exact for plain paths. With a glob it
/// says yes whenever some path could fall under both, so `api/Makefile`
/// meets `api/**/*.rs` (the plain path could be a folder): two lanes must
/// never hold one file, and an extra question costs less than a clash.
fn overlap(a: &str, b: &str) -> bool {
    let (a, b) = (path_key(a), path_key(b));
    let (a, b) = (segments(&a), segments(&b));
    !a.is_empty() && !b.is_empty() && meet(&a, &b)
}

/// One side ran out: what it matched so far is a folder the rest of the
/// other lies under, so both hold it.
fn meet(a: &[&str], b: &[&str]) -> bool {
    match (a.split_first(), b.split_first()) {
        (None, _) | (_, None) => true,
        (Some((&"**", rest)), _) => meet(rest, b) || meet(a, &b[1..]),
        (_, Some((&"**", rest))) => meet(a, rest) || meet(&a[1..], b),
        (Some((x, ra)), Some((y, rb))) => segments_meet(x, y) && meet(ra, rb),
    }
}

/// Could one name match both segment patterns (`*`, `?`)?
fn segments_meet(x: &str, y: &str) -> bool {
    let (x, y): (Vec<char>, Vec<char>) = (x.chars().collect(), y.chars().collect());
    // can[i][j]: what is left of each, x[i..] and y[j..], can match one name.
    let mut can = vec![vec![false; y.len() + 1]; x.len() + 1];
    for i in (0..=x.len()).rev() {
        for j in (0..=y.len()).rev() {
            can[i][j] = match (x.get(i), y.get(j)) {
                (None, None) => true,
                (Some(&'*'), _) => can[i + 1][j] || (j < y.len() && can[i][j + 1]),
                (_, Some(&'*')) => can[i][j + 1] || (i < x.len() && can[i + 1][j]),
                (Some(a), Some(b)) => (a == b || *a == '?' || *b == '?') && can[i + 1][j + 1],
                _ => false,
            };
        }
    }
    can[0][0]
}

/// Does `outer` hold everything `inner` can name? Only claimed when `outer`
/// holds the folder `inner` starts in; a glob with no fixed start never is.
fn contains(outer: &str, inner: &str) -> bool {
    let key = path_key(inner);
    let root: Vec<&str> = segments(&key)
        .into_iter()
        .take_while(|s| !s.contains(['*', '?']))
        .collect();
    !root.is_empty() && covers(outer, &root.join("/"))
}

/// The answer to `lease.claim`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Claim {
    Granted,
    Held { by: Lease },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(pub u64);

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "req-{}", self.0)
    }
}

/// What the holder's session (or a human, for `critical` paths) answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Answer {
    Grant,
    Deny { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestState {
    Pending,
    Granted,
    Denied { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRequest {
    pub id: RequestId,
    pub path: String,
    pub from: Holder,
    pub for_task: TaskId,
    pub at: DateTime<Utc>,
    pub state: RequestState,
}

/// What `expire` took away, so the caller can journal it and tell the lanes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expired {
    pub leases: Vec<Lease>,
    /// Requests nobody answered inside `REQUEST_TIMEOUT_SECS`.
    pub denied: Vec<LeaseRequest>,
}

#[derive(Debug, Clone, Default)]
pub struct LeaseTable {
    leases: BTreeMap<TaskId, Lease>,
    requests: BTreeMap<RequestId, LeaseRequest>,
    next_id: u64,
}

impl LeaseTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim a task's files. Granted when no other lease can hold any of
    /// them; otherwise the first lease in the way comes back, so the lane
    /// can ask.
    pub fn claim(
        &mut self,
        task: TaskId,
        holder: Holder,
        paths: impl IntoIterator<Item = String>,
    ) -> Claim {
        let wanted: BTreeSet<String> = paths
            .into_iter()
            .map(|p| normalize_path(&p))
            .filter(|p| !p.is_empty())
            .collect();
        if let Some(clash) = self
            .leases
            .iter()
            .filter(|(id, lease)| **id != task || lease.holder != holder)
            .find(|(_, lease)| wanted.iter().any(|w| lease.meets(w)))
            .map(|(_, lease)| lease.clone())
        {
            return Claim::Held { by: clash };
        }
        let now = Utc::now();
        match self.leases.get_mut(&task) {
            // Re-claiming for the same lane widens the lease and refreshes it;
            // a path it handed on and claims again is its own again.
            Some(lease) => {
                lease
                    .given
                    .retain(|g| !wanted.iter().any(|w| contains(w, g)));
                lease.paths.extend(wanted);
                lease.last_seen = now;
            }
            None => {
                self.leases.insert(
                    task.clone(),
                    Lease {
                        task,
                        holder,
                        paths: wanted,
                        given: BTreeSet::new(),
                        granted_at: now,
                        last_seen: now,
                        ttl_secs: TTL_SECS,
                    },
                );
            }
        }
        Claim::Granted
    }

    /// End a task's lease. Returns what was held, for the journal.
    pub fn release(&mut self, task: &TaskId) -> Option<Lease> {
        self.leases.remove(task)
    }

    /// Keep a lease alive. False when there is nothing to keep alive.
    pub fn heartbeat(&mut self, task: &TaskId) -> bool {
        match self.leases.get_mut(task) {
            Some(lease) => {
                lease.last_seen = Utc::now();
                true
            }
            None => false,
        }
    }

    /// Drop stale leases and auto-deny unanswered requests (§4). Idempotent.
    pub fn expire(&mut self, now: DateTime<Utc>) -> Expired {
        let stale: Vec<TaskId> = self
            .leases
            .iter()
            .filter(|(_, l)| l.is_stale(now))
            .map(|(id, _)| id.clone())
            .collect();
        let leases: Vec<Lease> = stale
            .iter()
            .filter_map(|id| self.leases.remove(id))
            .collect();
        let cutoff = Duration::seconds(REQUEST_TIMEOUT_SECS);
        let mut denied = vec![];
        for req in self.requests.values_mut() {
            if req.state == RequestState::Pending && now - req.at > cutoff {
                req.state = RequestState::Denied {
                    reason: format!("no answer in {REQUEST_TIMEOUT_SECS}s"),
                };
                denied.push(req.clone());
            }
        }
        Expired { leases, denied }
    }

    /// Who holds this file right now (stale leases included: `expire` is the
    /// only thing that removes, so the caller sees one truth).
    #[must_use]
    pub fn holder_of(&self, path: &str) -> Option<&Lease> {
        self.leases.values().find(|l| l.holds(path))
    }

    /// Every lease that could hold something `path` names — a file, a folder
    /// or a glob: what granting a request for `path` would take from.
    #[must_use]
    pub fn meeting(&self, path: &str) -> Vec<&Lease> {
        self.leases.values().filter(|l| l.meets(path)).collect()
    }

    /// Ask the holder for a path (§4). The request is pending until the
    /// holder's session answers or `expire` denies it for them.
    pub fn request(&mut self, path: &str, from: Holder, for_task: TaskId) -> RequestId {
        self.next_id += 1;
        let id = RequestId(self.next_id);
        self.requests.insert(
            id,
            LeaseRequest {
                id,
                path: normalize_path(path),
                from,
                for_task,
                at: Utc::now(),
                state: RequestState::Pending,
            },
        );
        id
    }

    /// Answer a request. On `Grant` the path moves: it leaves the holder's
    /// lease and joins the requester's (created when the requester holds none
    /// yet, which is the case when a lane asks before it starts).
    pub fn answer(&mut self, id: RequestId, answer: Answer) -> Result<LeaseRequest> {
        let req = self
            .requests
            .get(&id)
            .ok_or_else(|| ParziError::Validation(format!("unknown lease request {id}")))?
            .clone();
        if req.state != RequestState::Pending {
            return Err(ParziError::Validation(format!(
                "lease request {id} is already answered"
            )));
        }
        let state = match answer {
            Answer::Deny { reason } => RequestState::Denied { reason },
            Answer::Grant => {
                self.transfer(&req);
                RequestState::Granted
            }
        };
        let slot = self
            .requests
            .get_mut(&id)
            .ok_or_else(|| ParziError::Validation(format!("unknown lease request {id}")))?;
        slot.state = state;
        Ok(slot.clone())
    }

    /// The path leaves every other lease that could hold it — the claim is
    /// dropped where the path covers all of it, and the path is handed on
    /// (`given`) where the claim is wider — and joins the requester's.
    fn transfer(&mut self, req: &LeaseRequest) {
        for lease in self.leases.values_mut() {
            if lease.task == req.for_task {
                continue;
            }
            let met: Vec<String> = lease
                .paths
                .iter()
                .filter(|held| overlap(held, &req.path))
                .cloned()
                .collect();
            for held in met {
                if contains(&req.path, &held) {
                    lease.paths.remove(&held);
                } else {
                    lease.given.insert(req.path.clone());
                }
            }
        }
        let now = Utc::now();
        match self.leases.get_mut(&req.for_task) {
            Some(lease) => {
                lease.given.retain(|g| !contains(&req.path, g));
                lease.paths.insert(req.path.clone());
                lease.last_seen = now;
            }
            None => {
                self.leases.insert(
                    req.for_task.clone(),
                    Lease {
                        task: req.for_task.clone(),
                        holder: req.from.clone(),
                        paths: [req.path.clone()].into_iter().collect(),
                        given: BTreeSet::new(),
                        granted_at: now,
                        last_seen: now,
                        ttl_secs: TTL_SECS,
                    },
                );
            }
        }
    }

    /// Requests waiting for an answer, oldest first.
    pub fn pending(&self) -> impl Iterator<Item = &LeaseRequest> {
        self.requests
            .values()
            .filter(|r| r.state == RequestState::Pending)
    }

    #[must_use]
    pub fn request_get(&self, id: RequestId) -> Option<&LeaseRequest> {
        self.requests.get(&id)
    }

    /// Live leases, ordered by task id so every render is deterministic.
    pub fn leases(&self) -> impl Iterator<Item = &Lease> {
        self.leases.values()
    }

    #[must_use]
    pub fn lease_of(&self, task: &TaskId) -> Option<&Lease> {
        self.leases.get(task)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.leases.is_empty()
    }

    /// How often this task has already asked for this path.
    #[must_use]
    pub fn request_count(&self, path: &str, task: &TaskId) -> usize {
        let p = path_key(path);
        self.requests
            .values()
            .filter(|r| path_key(&r.path) == p && &r.for_task == task)
            .count()
    }

    /// Asking a third time is a loop: the orchestrator convenes instead (§15.6).
    #[must_use]
    pub fn should_convene(&self, path: &str, task: &TaskId) -> bool {
        self.request_count(path, task) >= MAX_REQUESTS_PER_PATH
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::PATHS_IGNORE_CASE;

    fn holder(lane: &str) -> Holder {
        Holder {
            user: "ada".into(),
            machine: "desk".into(),
            run: None,
            lane: lane.into(),
        }
    }

    fn task(id: &str) -> TaskId {
        TaskId(id.into())
    }

    #[test]
    fn a_path_holds_what_is_under_it_and_a_glob_what_it_matches() {
        assert!(covers("api/src/payments", "api/src/payments/intent.rs"));
        assert!(covers("api/src/payments/**", "api/src/payments/a/b.rs"));
        assert!(covers("api/src/*.rs", "api/src/main.rs"));
        assert!(!covers("api/src/*.rs", "api/src/lib.ts"));
        // A glob that matches a folder holds what is in it.
        assert!(covers("api/src/*", "api/src/payments/intent.rs"));
        assert!(!covers("api/src/payments", "api/src/payment.rs"));
        assert!(!covers("", "api/src/main.rs"));
        assert_eq!(covers("API/Src", "api/src/main.rs"), PATHS_IGNORE_CASE);
    }

    #[test]
    fn two_claims_meet_when_one_file_could_fall_under_both() {
        assert!(overlap("api/src", "api/src/main.rs"));
        assert!(overlap("api/src/payments/**", "api/src/payments/intent.rs"));
        assert!(overlap("api/**/*.rs", "api/src/payments"));
        assert!(overlap("api/src/*.rs", "api/src/m?in.*"));
        assert!(!overlap("api/src/*.rs", "api/src/*.ts"));
        assert!(!overlap("api/src/checkout/**", "api/src/payments/**"));
        assert!(!overlap("api/src/routes.rs", "api/src/payments/**"));
        assert!(!overlap("api/**/*.rs", "web/src/main.rs"));
        // A plain path could be a folder: said to meet, never missed.
        assert!(overlap("api/Makefile", "api/**/*.rs"));
        assert_eq!(
            overlap("API/SRC/Main.rs", "api/src/main.rs"),
            PATHS_IGNORE_CASE
        );
    }

    #[test]
    fn one_name_meets_two_segment_patterns_only_when_it_can_match_both() {
        assert!(segments_meet("*.rs", "main.*"));
        assert!(segments_meet("a*b", "*c*"));
        assert!(segments_meet("?", "x"));
        assert!(segments_meet("*", ""));
        assert!(!segments_meet("*.rs", "*.ts"));
        assert!(!segments_meet("a?", "abc"));
        assert!(!segments_meet("x*", "y*"));
    }

    #[test]
    fn a_folder_holds_a_glob_only_when_the_glob_starts_inside_it() {
        assert!(contains("api/src", "api/src/payments/**"));
        assert!(contains("api/src/**", "api/src/main.rs"));
        assert!(!contains("api/src/payments", "api/src/**"));
        assert!(!contains("api/src", "**/*.rs"));
    }

    #[test]
    fn a_glob_lease_refuses_a_claim_inside_it() {
        let mut t = LeaseTable::new();
        let paths = ["api/src/payments/**".to_string()];
        assert_eq!(t.claim(task("TSK-8"), holder("api"), paths), Claim::Granted);
        let clash = t.claim(
            task("TSK-9"),
            holder("web"),
            ["api/src/payments/intent.rs".to_string()],
        );
        assert!(matches!(clash, Claim::Held { .. }), "{clash:?}");
        assert_eq!(
            t.holder_of("api/src/payments/deep/x.rs")
                .map(|l| l.task.clone()),
            Some(task("TSK-8"))
        );
        assert!(t.holder_of("api/src/checkout.rs").is_none());
        assert_eq!(
            t.holder_of("API/src/Payments/x.rs").is_some(),
            PATHS_IGNORE_CASE,
            "case is one file only where the file system says so"
        );
    }

    #[test]
    fn one_file_handed_out_of_a_folder_leaves_the_rest_with_its_holder() {
        let mut t = LeaseTable::new();
        let paths = ["api/src/payments/**".to_string()];
        t.claim(task("TSK-8"), holder("api"), paths);
        let id = t.request("api/src/payments/intent.rs", holder("web"), task("TSK-9"));
        t.answer(id, Answer::Grant).unwrap();
        assert_eq!(
            t.holder_of("api/src/payments/intent.rs")
                .map(|l| l.task.clone()),
            Some(task("TSK-9")),
            "the file moved"
        );
        assert_eq!(
            t.holder_of("api/src/payments/refund.rs")
                .map(|l| l.task.clone()),
            Some(task("TSK-8")),
            "the rest of the folder did not"
        );
        // A third lane is told who holds each part.
        let Claim::Held { by } = t.claim(
            task("TSK-10"),
            holder("docs"),
            ["api/src/payments/intent.rs".to_string()],
        ) else {
            panic!("the handed-on file is held");
        };
        assert_eq!(by.task, task("TSK-9"));
        // Once the new holder lets go, the file is free, not back with the
        // folder's holder, and the folder's holder may claim it again.
        t.release(&task("TSK-9"));
        assert!(t.holder_of("api/src/payments/intent.rs").is_none());
        let again = ["api/src/payments/intent.rs".to_string()];
        assert_eq!(t.claim(task("TSK-8"), holder("api"), again), Claim::Granted);
        assert!(t.lease_of(&task("TSK-8")).unwrap().given.is_empty());
    }

    #[test]
    fn a_request_for_a_whole_folder_takes_the_claims_inside_it() {
        let mut t = LeaseTable::new();
        t.claim(
            task("TSK-8"),
            holder("api"),
            ["api/src/payments/intent.rs".to_string()],
        );
        assert_eq!(t.meeting("api/src/payments").len(), 1);
        let id = t.request("api/src/payments", holder("web"), task("TSK-9"));
        t.answer(id, Answer::Grant).unwrap();
        assert!(t.lease_of(&task("TSK-8")).unwrap().paths.is_empty());
        assert_eq!(
            t.holder_of("api/src/payments/intent.rs")
                .map(|l| l.task.clone()),
            Some(task("TSK-9"))
        );
    }

    #[test]
    fn repeated_asks_count_the_same_file_in_any_case() {
        let mut t = LeaseTable::new();
        t.request("api/Src/a.rs", holder("web"), task("TSK-9"));
        t.request("api/src/a.rs", holder("web"), task("TSK-9"));
        let expected = if PATHS_IGNORE_CASE { 2 } else { 1 };
        assert_eq!(t.request_count("api/src/a.rs", &task("TSK-9")), expected);
    }
}
