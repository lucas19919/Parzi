//! Asking another lane for a file (PLAN §4): `lease.request` reaches the
//! holder's own session as typed, untrusted data; `lease.grant`/`lease.deny`
//! answer it; a `critical:` path is not the coder's to give, so it becomes a
//! human approval card; and silence past the timeout is a denial.

use parzi_core::error::Result;
use parzi_core::journal::JournalKind;
use parzi_core::lease::{Answer, Lease, RequestId};
use parzi_core::plan::TaskId;
use parzi_core::project::normalize_path;
use tokio::sync::oneshot;

use crate::handler::RunEvent;
use crate::inter::{self, InterKind, InterSessionMessage};
use crate::tools::{Approval, ToolCallInfo};

use super::hub::LeaseHub;
use super::naming::{same_holder, tool_err};

/// What a `lease.request` ended as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Granted,
    Denied(String),
}

/// One request waiting for its holder to answer.
pub(super) struct Pending {
    answer: oneshot::Sender<Outcome>,
    holder_run: Option<String>,
}

impl LeaseHub {
    /// Ask the holder for `path`. A critical path goes to a human instead; a
    /// third ask for the same file convenes the orchestrator (§15.6).
    pub async fn request(
        &self,
        run: &str,
        path: &str,
        for_task: &TaskId,
        reason: &str,
    ) -> Result<Outcome> {
        let holder = self
            .holder_of_run(run)
            .await
            .ok_or_else(|| tool_err("this run has no lease identity"))?;
        let path = normalize_path(path);
        let current = self
            .holder_of_path(&path)
            .await
            .ok_or_else(|| tool_err(&format!("{path} is free — claim it with lease.claim")))?;
        if same_holder(&current.holder, &holder) {
            return Err(tool_err(&format!("you already hold {path}")));
        }
        // Bound first: the table lock must not be held across the journal
        // write below.
        let convene = self.table.lock().await.should_convene(&path, for_task);
        if convene {
            let text = format!("{path}: asked twice already — the orchestrator decides");
            self.journal(run, JournalKind::Convene, Some(for_task), &text)
                .await;
            return Ok(Outcome::Denied(text));
        }
        let request_id = {
            let mut t = self.table.lock().await;
            t.request(&path, holder, for_task.clone()).to_string()
        };
        self.journal(
            run,
            JournalKind::Request,
            Some(for_task),
            &format!("asked lane {} for {path}: {reason}", current.holder.lane),
        )
        .await;

        let holder_run = self.run_of_holder(&current.holder).await;
        if self.is_critical(run, &path).await {
            // PLAN §4: a critical path is not the coder's to give away.
            return self
                .escalate(run, &request_id, &path, for_task, &current, holder_run)
                .await;
        }
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(
            request_id.clone(),
            Pending {
                answer: tx,
                holder_run: holder_run.clone(),
            },
        );
        self.deliver(
            run,
            holder_run.as_deref(),
            &request_id,
            &path,
            for_task,
            reason,
        )
        .await;
        match tokio::time::timeout(self.answer_timeout, rx).await {
            Ok(Ok(outcome)) => Ok(outcome),
            // Timed out, or the holder's run went away without answering:
            // silence is a denial, and the lease stays where it is.
            _ => {
                self.pending.lock().await.remove(&request_id);
                let why = format!("no answer in {}s", self.answer_timeout.as_secs().max(1));
                self.apply_answer(
                    &request_id,
                    Answer::Deny {
                        reason: why.clone(),
                    },
                )
                .await;
                self.journal(
                    run,
                    JournalKind::Deny,
                    Some(for_task),
                    &format!("{path}: {why}"),
                )
                .await;
                Ok(Outcome::Denied(why))
            }
        }
    }

    /// `lease.grant` / `lease.deny` from the holder's own run.
    pub async fn answer(&self, run: &str, request_id: &str, answer: Answer) -> Result<String> {
        let pending = self.pending.lock().await.remove(request_id);
        let Some(pending) = pending else {
            return Err(tool_err(&format!(
                "no open request `{request_id}` (already answered, or it timed out)"
            )));
        };
        if pending.holder_run.as_deref().is_some_and(|r| r != run) {
            // Put it back: only the holder answers, or the human card does.
            self.pending
                .lock()
                .await
                .insert(request_id.to_string(), pending);
            return Err(tool_err("that request is not addressed to this run"));
        }
        let (outcome, kind, text) = match &answer {
            Answer::Grant => (
                Outcome::Granted,
                JournalKind::Grant,
                format!("granted request {request_id}"),
            ),
            Answer::Deny { reason } => (
                Outcome::Denied(reason.clone()),
                JournalKind::Deny,
                format!("denied request {request_id}: {reason}"),
            ),
        };
        self.apply_answer(request_id, answer).await;
        self.journal(run, kind, None, &text).await;
        // The asking run may already have given up on the timeout; that was
        // its answer, and this one is simply late.
        if pending.answer.send(outcome).is_err() {
            tracing::debug!("lease request {request_id} was answered after its timeout");
        }
        Ok(text)
    }

    /// Requests this run has been asked to answer and has not yet — what a
    /// lane sees when it wonders what is waiting on it.
    pub async fn open_requests_for(&self, run: &str) -> Vec<String> {
        self.pending
            .lock()
            .await
            .iter()
            .filter(|(_, p)| p.holder_run.as_deref() == Some(run))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Critical transfer: a human decides, through the same `Approver` the run
    /// already uses, so it renders as an ordinary approval card (§4, §8).
    async fn escalate(
        &self,
        run: &str,
        request_id: &str,
        path: &str,
        for_task: &TaskId,
        current: &Lease,
        holder_run: Option<String>,
    ) -> Result<Outcome> {
        let call = ToolCallInfo {
            id: format!("lease.transfer:{request_id}"),
            name: "lease.transfer".into(),
            args: serde_json::json!({
                "request_id": request_id,
                "path": path,
                "for_task": for_task.as_str(),
                "from_lane": current.holder.lane,
                "from_task": current.task.as_str(),
                "critical": true,
            }),
            lane: current.holder.lane.clone(),
        };
        // The card belongs where a person is watching the holder; fall back to
        // the asking run, so a transfer is never granted with nobody asked.
        let approver = match self.approver_of(holder_run.as_deref()).await {
            Some(a) => Some(a),
            None => self.approver_of(Some(run)).await,
        };
        for target in [holder_run.as_deref(), Some(run)].into_iter().flatten() {
            self.notify(target, RunEvent::ApprovalRequest { call: call.clone() })
                .await;
        }
        let allowed = match approver {
            Some(a) => matches!(
                tokio::time::timeout(self.answer_timeout, a.approve(&call)).await,
                Ok(Approval::Allow)
            ),
            None => false,
        };
        if allowed {
            self.apply_answer(request_id, Answer::Grant).await;
            self.journal(
                run,
                JournalKind::Approve,
                Some(for_task),
                &format!("critical transfer approved: {path}"),
            )
            .await;
            Ok(Outcome::Granted)
        } else {
            let reason = "critical transfer not approved by a human".to_string();
            self.apply_answer(
                request_id,
                Answer::Deny {
                    reason: reason.clone(),
                },
            )
            .await;
            self.journal(
                run,
                JournalKind::Deny,
                Some(for_task),
                &format!("critical transfer refused: {path}"),
            )
            .await;
            Ok(Outcome::Denied(reason))
        }
    }

    /// Move the path (or not) in the table. A request core already auto-denied
    /// on its own timeout is not worth failing a tool over, but it is worth a
    /// line in the log.
    async fn apply_answer(&self, request_id: &str, answer: Answer) {
        let Some(id) = parse_request_id(request_id) else {
            tracing::warn!("lease answer for unparsable request id `{request_id}`");
            return;
        };
        let mut t = self.table.lock().await;
        if let Err(e) = t.answer(id, answer) {
            tracing::warn!("lease answer {request_id} not applied: {e}");
        }
    }

    /// Put the request into the holder's session as typed, untrusted data
    /// (H-5): a `System` event carrying an `InterSessionMessage`, never a
    /// `User` turn.
    async fn deliver(
        &self,
        from_run: &str,
        to_run: Option<&str>,
        request_id: &str,
        path: &str,
        for_task: &TaskId,
        reason: &str,
    ) {
        let Some(to_run) = to_run else { return };
        let body = serde_json::json!({
            "request_id": request_id,
            "path": path,
            "for_task": for_task.as_str(),
            "reason": reason,
            "answer_with": ["lease.grant {request_id}", "lease.deny {request_id, reason}"],
        })
        .to_string();
        let from_lane = self
            .holder_of_run(from_run)
            .await
            .map(|h| h.lane)
            .unwrap_or_default();
        let msg = InterSessionMessage::new(from_run, from_lane, InterKind::LeaseRequest, body);
        if let Some(store) = self.store_of_run(to_run).await {
            if let Err(e) = inter::deliver(&store, to_run, &msg) {
                tracing::warn!("lease request delivery failed for {to_run}: {e}");
            }
        }
        self.notify(
            to_run,
            RunEvent::Notice {
                text: msg.summary(),
            },
        )
        .await;
    }
}

/// `req-7` (what a session is told) → `RequestId(7)`. A bare number is taken
/// too, because models drop prefixes.
fn parse_request_id(s: &str) -> Option<RequestId> {
    s.trim()
        .trim_start_matches("req-")
        .parse::<u64>()
        .ok()
        .map(RequestId)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_round_trip_through_the_tool_surface() {
        assert_eq!(
            parse_request_id(&RequestId(7).to_string()),
            Some(RequestId(7))
        );
        assert_eq!(parse_request_id("7"), Some(RequestId(7)));
        assert_eq!(parse_request_id("nonsense"), None);
    }
}
