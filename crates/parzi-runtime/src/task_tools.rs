//! The agents' side of the shared task list (`task.list`, `task.add`,
//! `task.update`), and what a run does to the tasks linked to its session:
//! they are in progress while it runs and handed back when it ends.

use parzi_core::store::{SessionMeta, SessionStatus, SessionStore};
use parzi_core::tasks::{
    self, project_of_session, Change, NewTask, Task, TaskPatch, TaskStatus, TaskStore, TaskView,
};
use serde_json::Value;

use crate::tools::clip;

pub(crate) async fn execute(
    store: &SessionStore,
    session_id: &str,
    name: &str,
    args: &Value,
) -> (bool, String) {
    let (store, sid, name, args) = (
        store.clone(),
        session_id.to_string(),
        name.to_string(),
        args.clone(),
    );
    tokio::task::spawn_blocking(move || match run(&store, &sid, &name, &args) {
        Ok(text) => (true, text),
        Err(e) => (false, e),
    })
    .await
    .unwrap_or_else(|e| (false, e.to_string()))
}

fn run(store: &SessionStore, sid: &str, name: &str, args: &Value) -> Result<String, String> {
    let caller = store.get(sid).map_err(|e| e.to_string())?;
    let ts = TaskStore::open().map_err(|e| e.to_string())?;
    let arg = |k: &str| args.get(k).and_then(Value::as_str).map_or("", str::trim);
    let home = project_of_session(&caller.project);
    match name {
        "task.list" => {
            let (list, label) = match arg("project") {
                "" => (ts.scope(home.as_deref()), scope_label(home.as_deref())),
                p if p.eq_ignore_ascii_case("all") => (ts.all(), "every project".to_string()),
                p if p.eq_ignore_ascii_case("inbox") => (ts.scope(None), scope_label(None)),
                p => (ts.scope(Some(p)), scope_label(Some(p))),
            };
            let all = arg("project").eq_ignore_ascii_case("all");
            Ok(render(
                &tasks::views(list.map_err(|e| e.to_string())?),
                &label,
                all,
            ))
        }
        "task.add" => {
            let project = match arg("project") {
                "" => home,
                p if p.eq_ignore_ascii_case("inbox") => None,
                p => Some(p.to_string()),
            };
            let drafts = drafts_from(args)?;
            let added = ts
                .add(project.as_deref(), &drafts, sid)
                .map_err(|e| e.to_string())?;
            let mut out = format!(
                "added {} to {}:",
                plural(added.len(), "task"),
                scope_label(project.as_deref())
            );
            for t in &added {
                out.push_str(&format!("\n- {} {}", t.id, t.title));
                if !t.needs.is_empty() {
                    out.push_str(&format!(", needs {}", t.needs.join(", ")));
                }
            }
            out.push_str(&rollup(&ts, project.as_deref()));
            Ok(out)
        }
        "task.update" => {
            let id = arg("id");
            if id.is_empty() {
                return Err("task.update needs an `id`".into());
            }
            let task = ts.get(id).map_err(|e| e.to_string())?;
            check_may_change(store, &caller, &task)?;
            let mut patch = TaskPatch::default();
            if !arg("status").is_empty() {
                patch.status = Some(TaskStatus::parse(arg("status")).ok_or_else(|| {
                    format!(
                        "`{}` is not a status: todo, doing, review, done or failed",
                        arg("status")
                    )
                })?);
            }
            if !arg("title").is_empty() {
                patch.title = Some(arg("title").to_string());
            }
            if let Some(n) = args.get("notes").and_then(Value::as_str) {
                patch.notes = Some(n.to_string());
            }
            if !arg("note").is_empty() {
                let base = patch.notes.clone().unwrap_or_else(|| task.notes.clone());
                patch.notes = Some(if base.trim().is_empty() {
                    arg("note").to_string()
                } else {
                    format!("{}\n\n{}", base.trim_end(), arg("note"))
                });
            }
            let t = ts.update(id, &patch).map_err(|e| e.to_string())?;
            Ok(format!(
                "task {} ({}) is {} now{}",
                t.id,
                clip(&t.title, 60),
                t.status.as_str(),
                rollup(&ts, t.project.as_deref())
            ))
        }
        _ => Err(format!("unknown task tool `{name}`")),
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn scope_label(project: Option<&str>) -> String {
    project.map_or_else(|| "the inbox".to_string(), |p| format!("project {p}"))
}

/// One task or a list of them, as agents send them.
fn drafts_from(args: &Value) -> Result<Vec<NewTask>, String> {
    let items: Vec<&Value> = match args.get("tasks") {
        Some(Value::Array(list)) => list.iter().collect(),
        Some(one @ Value::Object(_)) => vec![one],
        _ if args.get("title").is_some() => vec![args],
        _ => return Err("task.add needs `tasks`: [{title, notes, needs, lane, key}]".into()),
    };
    items
        .into_iter()
        .map(|v| {
            let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let needs = match v.get("needs") {
                Some(Value::Array(a)) => a
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
                Some(Value::String(one)) => one.split(',').map(|x| x.trim().to_string()).collect(),
                _ => vec![],
            };
            let status = match v.get("status").and_then(Value::as_str) {
                Some(raw) if !raw.trim().is_empty() => {
                    Some(TaskStatus::parse(raw).ok_or_else(|| format!("`{raw}` is not a status"))?)
                }
                _ => None,
            };
            Ok(NewTask {
                title: s("title"),
                notes: s("notes"),
                status,
                needs,
                lane: s("lane"),
                key: Some(s("key")).filter(|k| !k.trim().is_empty()),
            })
        })
        .collect()
}

/// The line under a write: where the project stands now.
fn rollup(ts: &TaskStore, project: Option<&str>) -> String {
    let Ok(list) = ts.scope(project) else {
        return String::new();
    };
    let v = tasks::views(list);
    let done = v
        .iter()
        .filter(|t| t.task.status == TaskStatus::Done)
        .count();
    let ready: Vec<&str> = v
        .iter()
        .filter(|t| t.ready)
        .map(|t| t.task.id.as_str())
        .collect();
    let mut out = format!("\n{done}/{} done", v.len());
    if !ready.is_empty() {
        out.push_str(&format!(", ready: {}", ready.join(", ")));
    }
    out
}

fn line(t: &TaskView, with_project: bool) -> String {
    let mut out = format!("- {} {}", t.task.id, clip(&t.task.title, 100));
    if with_project {
        out.push_str(&format!(
            " [{}]",
            t.task.project.as_deref().unwrap_or("inbox")
        ));
    }
    if !t.task.lane.is_empty() {
        out.push_str(&format!(" ({})", t.task.lane));
    }
    if !t.blocked_by.is_empty() {
        out.push_str(&format!(", waits on {}", t.blocked_by.join(", ")));
    }
    if let Some(s) = &t.task.session {
        out.push_str(&format!(", session {}", &s[..s.len().min(8)]));
    }
    let note = t.task.notes.lines().find(|l| !l.trim().is_empty());
    if let Some(n) = note {
        out.push_str(&format!(": {}", clip(n.trim(), 100)));
    }
    out
}

fn render(list: &[TaskView], label: &str, with_project: bool) -> String {
    if list.is_empty() {
        return format!("No tasks in {label} yet. Add some with task.add.");
    }
    let todo = |t: &&TaskView| t.task.status == TaskStatus::Todo;
    let groups: [(&str, Vec<&TaskView>); 6] = [
        (
            "Ready",
            list.iter().filter(todo).filter(|t| t.ready).collect(),
        ),
        (
            "Blocked",
            list.iter().filter(todo).filter(|t| !t.ready).collect(),
        ),
        ("Doing", by_status(list, TaskStatus::Doing)),
        ("In review", by_status(list, TaskStatus::Review)),
        ("Failed", by_status(list, TaskStatus::Failed)),
        ("Done", by_status(list, TaskStatus::Done)),
    ];
    let counts: Vec<String> = groups
        .iter()
        .filter(|(_, g)| !g.is_empty())
        .map(|(name, g)| format!("{} {}", g.len(), name.to_lowercase()))
        .collect();
    let mut out = format!("Tasks in {label}: {}.", counts.join(", "));
    for (name, group) in &groups {
        if group.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{name}:"));
        // Done piles up: the latest few say enough.
        let shown: Vec<&&TaskView> = if *name == "Done" {
            let mut d: Vec<&&TaskView> = group.iter().collect();
            d.sort_by_key(|t| std::cmp::Reverse(t.task.updated));
            d.into_iter().take(10).collect()
        } else {
            group.iter().collect()
        };
        for t in &shown {
            out.push('\n');
            out.push_str(&line(t, with_project));
        }
        if shown.len() < group.len() {
            out.push_str(&format!("\n- and {} more", group.len() - shown.len()));
        }
    }
    out
}

fn by_status(list: &[TaskView], status: TaskStatus) -> Vec<&TaskView> {
    list.iter().filter(|t| t.task.status == status).collect()
}

/// A session may change any task of its own project, and the tasks its own
/// session or the sessions under it work on.
fn check_may_change(store: &SessionStore, caller: &SessionMeta, task: &Task) -> Result<(), String> {
    let home = project_of_session(&caller.project);
    let same = match (&home, &task.project) {
        (None, None) => true,
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    };
    if same {
        return Ok(());
    }
    if let Some(linked) = &task.session {
        let all = store.list().map_err(|e| e.to_string())?;
        if parzi_core::store::subtree_ids(&all, &caller.id).contains(linked) {
            return Ok(());
        }
    }
    Err(format!(
        "task {} belongs to {}; a session changes tasks in its own project ({}) and the ones its sessions work on",
        task.id,
        scope_label(task.project.as_deref()),
        scope_label(home.as_deref()),
    ))
}

/// The task a spawn wants to staff, if this session may hand it out.
pub(crate) fn staffable(store: &SessionStore, caller_id: &str, id: &str) -> Result<Task, String> {
    let caller = store.get(caller_id).map_err(|e| e.to_string())?;
    let ts = TaskStore::open().map_err(|e| e.to_string())?;
    let task = ts.get(id).map_err(|e| e.to_string())?;
    check_may_change(store, &caller, &task)?;
    if task.status == TaskStatus::Done {
        return Err(format!("task {} is already done", task.id));
    }
    let open = tasks::open_needs(
        &task,
        &ts.scope(task.project.as_deref()).unwrap_or_default(),
    );
    if !open.is_empty() {
        return Err(format!(
            "task {} still waits on {}; staff those first",
            task.id,
            open.join(", ")
        ));
    }
    Ok(task)
}

/// Links `session` to the task and moves it to doing.
pub fn link(task_id: &str, session: &str) -> parzi_core::Result<Task> {
    TaskStore::open()?.update(
        task_id,
        &TaskPatch {
            status: Some(TaskStatus::Doing),
            session: Some(session.to_string()),
            ..TaskPatch::default()
        },
    )
}

/// What a session staffed on a task is told about it, after its prompt.
/// `scope` is the task's project, to name what it builds on.
#[must_use]
pub fn brief(task: &Task, scope: &[Task]) -> String {
    let mut out = format!("You are working task {}: {}.", task.id, task.title);
    if !task.notes.trim().is_empty() {
        out.push_str(&format!("\n\nTask notes:\n{}", task.notes.trim()));
    }
    if !task.needs.is_empty() {
        let names: Vec<String> = task
            .needs
            .iter()
            .map(|id| match scope.iter().find(|t| &t.id == id) {
                Some(t) => format!("{} ({id}, {})", t.title, t.status.as_str()),
                None => id.clone(),
            })
            .collect();
        out.push_str(&format!("\n\nIt builds on: {}.", names.join("; ")));
    }
    out.push_str(&format!(
        "\n\nWhen you finish, record the outcome with task.update on {}: done when it is complete, review when a person should check it first, or failed with a note saying what stopped you. Then reply with a short summary.",
        task.id
    ));
    out
}

/// A linked session runs (again): its open tasks are in progress.
pub(crate) fn run_started(session_id: &str) {
    let Ok(ts) = TaskStore::open() else {
        return;
    };
    match ts.move_linked(
        session_id,
        &[TaskStatus::Todo, TaskStatus::Review, TaskStatus::Failed],
        TaskStatus::Doing,
    ) {
        Ok(moved) => tasks::publish(&Change {
            ids: moved.into_iter().map(|t| t.id).collect(),
            finished: vec![],
        }),
        Err(e) => tracing::warn!("tasks of {session_id} not started: {e}"),
    }
}

/// Where a run's tasks go when it ends, from how the session settled.
/// None leaves them: another run has already taken the session.
#[must_use]
pub fn outcome(status: SessionStatus, stopped: bool) -> Option<TaskStatus> {
    if stopped {
        return Some(TaskStatus::Todo);
    }
    match status {
        SessionStatus::Done => Some(TaskStatus::Review),
        // A failed turn or a budget pause: a person has to look.
        SessionStatus::Idle => Some(TaskStatus::Failed),
        SessionStatus::Killed => Some(TaskStatus::Todo),
        SessionStatus::Active | SessionStatus::Queued => None,
    }
}

/// The run ended: tasks still in progress are handed back (review, failed,
/// or to do again when it was stopped), and every task the run finished,
/// itself or here, is reported so the user hears of it.
pub(crate) fn run_ended(store: &SessionStore, session_id: &str, started_ms: u64, stopped: bool) {
    let Ok(ts) = TaskStore::open() else {
        return;
    };
    let Ok(linked) = ts.linked(session_id) else {
        return;
    };
    if linked.is_empty() {
        return;
    }
    let status = store
        .get(session_id)
        .map_or(SessionStatus::Idle, |m| m.status);
    let Some(to) = outcome(status, stopped) else {
        return;
    };
    let moved = match ts.move_linked(session_id, &[TaskStatus::Doing], to) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("tasks of {session_id} not handed back: {e}");
            return;
        }
    };
    let mut change = Change {
        ids: moved.iter().map(|t| t.id.clone()).collect(),
        finished: moved
            .iter()
            .filter(|t| t.status != TaskStatus::Todo)
            .cloned()
            .collect(),
    };
    // The ones the agent marked itself during this run.
    for t in linked {
        let settled = matches!(
            t.status,
            TaskStatus::Review | TaskStatus::Done | TaskStatus::Failed
        );
        if settled && t.updated >= started_ms && !change.ids.contains(&t.id) {
            change.ids.push(t.id.clone());
            change.finished.push(t);
        }
    }
    tasks::publish(&change);
}

/// The app closed mid-run: those sessions' tasks are to do again.
pub(crate) fn runs_interrupted(sessions: &[String]) {
    let Ok(ts) = TaskStore::open() else {
        return;
    };
    let mut ids = vec![];
    for sid in sessions {
        match ts.move_linked(sid, &[TaskStatus::Doing], TaskStatus::Todo) {
            Ok(moved) => ids.extend(moved.into_iter().map(|t| t.id)),
            Err(e) => tracing::warn!("tasks of {sid} not reset: {e}"),
        }
    }
    tasks::publish(&Change {
        ids,
        finished: vec![],
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_hand_tasks_back_by_how_they_ended() {
        assert_eq!(
            outcome(SessionStatus::Done, false),
            Some(TaskStatus::Review)
        );
        assert_eq!(
            outcome(SessionStatus::Idle, false),
            Some(TaskStatus::Failed)
        );
        assert_eq!(
            outcome(SessionStatus::Killed, false),
            Some(TaskStatus::Todo)
        );
        assert_eq!(outcome(SessionStatus::Done, true), Some(TaskStatus::Todo));
        assert_eq!(outcome(SessionStatus::Active, false), None);
    }

    #[test]
    fn drafts_take_one_task_or_many() {
        let one = drafts_from(&serde_json::json!({"title": "a", "needs": "x, y"})).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].needs, vec!["x".to_string(), "y".to_string()]);
        let many = drafts_from(&serde_json::json!({"tasks": [
            {"title": "a", "key": "k", "status": "in progress"},
            {"title": "b", "needs": ["k"], "lane": "work"},
        ]}))
        .unwrap();
        assert_eq!(many[0].status, Some(TaskStatus::Doing));
        assert_eq!(many[1].needs, vec!["k".to_string()]);
        assert!(drafts_from(&serde_json::json!({})).is_err());
        assert!(drafts_from(
            &serde_json::json!({"tasks": [{"title": "a", "status": "eventually"}]})
        )
        .is_err());
    }

    #[test]
    fn the_brief_names_the_task_and_how_to_finish_it() {
        let t = Task {
            id: "k7m2q".into(),
            title: "Write the parser".into(),
            notes: "Use the grammar in docs/".into(),
            status: TaskStatus::Todo,
            project: Some("parzi".into()),
            needs: vec!["a2b3c".into()],
            lane: "build".into(),
            session: None,
            created: 0,
            updated: 0,
            created_by: "you".into(),
            order: 1,
        };
        let mut need = t.clone();
        need.id = "a2b3c".into();
        need.title = "Write the grammar".into();
        need.status = TaskStatus::Review;
        let b = brief(&t, &[need]);
        assert!(b.contains("task k7m2q: Write the parser"), "{b}");
        assert!(b.contains("docs/"), "{b}");
        assert!(
            b.contains("builds on: Write the grammar (a2b3c, review)"),
            "a need says where it stands: {b}"
        );
        assert!(b.contains("task.update on k7m2q"), "{b}");
    }
}
