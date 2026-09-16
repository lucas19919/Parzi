//! File leases (PLAN §4): who holds which files, for which task, until when.
//!
//! Pure and in memory. The table knows nothing about PROJECT.md, the hub or
//! the filesystem — a caller asks `project::is_critical` when a transfer
//! needs a human, and the journal records what happened. Everything that
//! needs a clock takes one (`expire`) or stamps `Utc::now()` once.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};
use crate::plan::TaskId;
use crate::project::normalize_path;

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

/// Two paths collide when they are the same file or one contains the other.
fn conflicts(a: &str, b: &str) -> bool {
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

impl LeaseTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim a task's files. Granted when no other lease intersects the set;
    /// otherwise the first colliding lease comes back, so the lane can ask.
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
            .find(|(_, lease)| {
                wanted
                    .iter()
                    .any(|w| lease.paths.iter().any(|held| conflicts(w, held)))
            })
            .map(|(_, lease)| lease.clone())
        {
            return Claim::Held { by: clash };
        }
        let now = Utc::now();
        match self.leases.get_mut(&task) {
            // Re-claiming for the same lane widens the lease and refreshes it.
            Some(lease) => {
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

    /// Who holds this path right now (stale leases included: `expire` is the
    /// only thing that removes, so the caller sees one truth).
    #[must_use]
    pub fn holder_of(&self, path: &str) -> Option<&Lease> {
        let p = normalize_path(path);
        self.leases
            .values()
            .find(|l| l.paths.iter().any(|held| conflicts(&p, held)))
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

    fn transfer(&mut self, req: &LeaseRequest) {
        for lease in self.leases.values_mut() {
            if lease.task != req.for_task {
                lease.paths.retain(|held| !conflicts(&req.path, held));
            }
        }
        let now = Utc::now();
        match self.leases.get_mut(&req.for_task) {
            Some(lease) => {
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
        let p = normalize_path(path);
        self.requests
            .values()
            .filter(|r| r.path == p && &r.for_task == task)
            .count()
    }

    /// Asking a third time is a loop: the orchestrator convenes instead (§15.6).
    #[must_use]
    pub fn should_convene(&self, path: &str, task: &TaskId) -> bool {
        self.request_count(path, task) >= MAX_REQUESTS_PER_PATH
    }
}
