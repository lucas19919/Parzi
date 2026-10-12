//! One-time import of the task list Work replaces: every task not done
//! becomes a Later goal, then `~/.parzi/tasks` is renamed `tasks.imported`
//! so it never runs twice and nothing is deleted.

use std::path::Path;

use serde_json::Value;

use super::log::WorkLog;
use super::model::{Later, RunCause, WorkEvent};
use crate::error::Result;

/// Imports `<home>/tasks` into `log`. Returns how many goals it made.
pub fn import_legacy_tasks(home: &Path, log: &WorkLog) -> Result<usize> {
    let dir = home.join("tasks");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(0);
    };
    let mut events = vec![];
    let mut taken: Vec<String> = log.read(|w| w.goals.keys().cloned().collect());
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = serde_json::from_str::<Value>(&text) else {
            tracing::warn!("{} is not a task list; left as is", path.display());
            continue;
        };
        let project = file
            .get("project")
            .and_then(Value::as_str)
            .and_then(super::project_of_session);
        let tasks = file.get("tasks").and_then(Value::as_array);
        for t in tasks.into_iter().flatten() {
            let s = |k: &str| t.get(k).and_then(Value::as_str).unwrap_or("").trim();
            if s("status") == "done" || s("title").is_empty() {
                continue;
            }
            let goal = super::graph::fresh_id(5, &|id| taken.iter().any(|t| t == id));
            taken.push(goal.clone());
            let prompt = if s("notes").is_empty() {
                s("title").to_string()
            } else {
                format!("{}\n\n{}", s("title"), s("notes"))
            };
            events.push(WorkEvent::GoalCreated {
                goal,
                title: s("title").to_string(),
                project: project.clone(),
                root: None,
                prompt,
                origin: RunCause::Later,
                later: Some(Later { auto_start: false }),
                continues: None,
            });
        }
    }
    let n = events.len();
    log.append(events)?;
    let done = home.join("tasks.imported");
    if !done.exists() {
        std::fs::rename(&dir, &done)?;
    }
    if n > 0 {
        tracing::info!("imported {n} open task(s) as Later goals");
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::model::GoalStatus;

    #[test]
    fn open_tasks_come_back_as_later_goals_once() {
        let home = tempfile::tempdir().unwrap();
        let tasks = home.path().join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        std::fs::write(
            tasks.join("parzi.json"),
            r#"{"project":"parzi","tasks":[
                {"id":"a","title":"Ship it","notes":"carefully","status":"todo"},
                {"id":"b","title":"Old","status":"done"},
                {"id":"c","title":"Review me","status":"review"}]}"#,
        )
        .unwrap();
        std::fs::write(
            tasks.join("inbox.json"),
            r#"{"tasks":[{"id":"d","title":"Loose"}]}"#,
        )
        .unwrap();
        let log = WorkLog::open_as(&home.path().join("work"), "pc-ab12").unwrap();
        assert_eq!(import_legacy_tasks(home.path(), &log).unwrap(), 3);
        assert!(!tasks.exists() && home.path().join("tasks.imported").exists());
        assert_eq!(
            import_legacy_tasks(home.path(), &log).unwrap(),
            0,
            "only once"
        );
        let w = log.state();
        let ship = w.goals.values().find(|g| g.title == "Ship it").unwrap();
        assert_eq!(ship.project.as_deref(), Some("parzi"));
        assert_eq!(ship.prompt, "Ship it\n\ncarefully");
        assert_eq!(w.status(ship, 0), GoalStatus::Later);
        let loose = w.goals.values().find(|g| g.title == "Loose").unwrap();
        assert_eq!(loose.project, None);
    }
}
