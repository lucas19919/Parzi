//! Tasks: one list across projects that people and agents share. Each
//! project keeps its own file under `~/.parzi/tasks` (`inbox.json` holds the
//! tasks with no project). Writes are atomic and serialized in this process,
//! and every change is published to the watchers (the desktop's live view).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    #[default]
    Todo,
    Doing,
    Review,
    Done,
    Failed,
}

impl TaskStatus {
    /// Lenient: agents write "in progress" or "complete" as often as the names.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim().to_ascii_lowercase().as_str() {
            "todo" | "to do" | "open" | "pending" => Self::Todo,
            "doing" | "in progress" | "in_progress" | "active" | "started" => Self::Doing,
            "review" | "in review" | "needs review" => Self::Review,
            "done" | "complete" | "completed" | "finished" => Self::Done,
            "failed" | "fail" | "blocked" | "error" => Self::Failed,
            _ => return None,
        })
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Doing => "doing",
            Self::Review => "review",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub status: TaskStatus,
    /// The project slug; None is the Inbox. Set from the file it lives in.
    #[serde(default)]
    pub project: Option<String>,
    /// Ids of tasks in the same project that must be done first.
    #[serde(default)]
    pub needs: Vec<String>,
    /// "build", "work", or "" for no preference.
    #[serde(default)]
    pub lane: String,
    /// The session working (or that worked) on it.
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub updated: u64,
    /// "you" or the id of the session that added it.
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub order: i64,
}

/// A task with what its project says about it.
#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    #[serde(flatten)]
    pub task: Task,
    /// To do with every need done.
    pub ready: bool,
    /// The needs still open (empty unless it waits).
    pub blocked_by: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewTask {
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub status: Option<TaskStatus>,
    /// Existing task ids, or the `key` of another task in the same batch.
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub lane: String,
    /// A name for this task inside one add, so the batch can need itself.
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaskPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub status: Option<TaskStatus>,
    /// A project slug; "" moves the task to the Inbox.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub needs: Option<Vec<String>>,
    #[serde(default)]
    pub lane: Option<String>,
    /// A session id; "" unlinks.
    #[serde(default)]
    pub session: Option<String>,
}

/// What changed, for the watchers. `finished` carries tasks a run just
/// handed back (review, done or failed) so the user can be told.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Change {
    pub ids: Vec<String>,
    pub finished: Vec<Task>,
}

type Watcher = Box<dyn Fn(&Change) + Send + Sync>;

static WATCHERS: Mutex<Vec<Watcher>> = Mutex::new(Vec::new());

/// Serializes every read-modify-write of the task files.
static WRITE: Mutex<()> = Mutex::new(());

pub fn watch(f: impl Fn(&Change) + Send + Sync + 'static) {
    WATCHERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Box::new(f));
}

pub fn publish(change: &Change) {
    if change.ids.is_empty() && change.finished.is_empty() {
        return;
    }
    for w in WATCHERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
    {
        w(change);
    }
}

/// The task scope of a session's project: "default" and "" are the Inbox.
#[must_use]
pub fn project_of_session(project: &str) -> Option<String> {
    let p = project.trim();
    (!p.is_empty() && p != "default").then(|| p.to_string())
}

fn same_scope(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    }
}

#[must_use]
pub fn now_ms() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(0)
}

/// Short ids people can type: five letters and digits, no look-alikes.
fn fresh_id(taken: &HashSet<String>) -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    loop {
        let raw = uuid::Uuid::new_v4();
        let id: String = raw
            .as_bytes()
            .iter()
            .take(5)
            .map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char)
            .collect();
        // Never all digits: an id must not read as a count or an index.
        if id.chars().any(|c| c.is_ascii_alphabetic()) && !taken.contains(&id) {
            return id;
        }
    }
}

fn check_project(project: &str) -> Result<&str> {
    let p = project.trim();
    if p.is_empty() || p.contains(['/', '\\']) || p.starts_with('.') || p.len() > 120 {
        return Err(ParziError::Validation(format!(
            "`{project}` is not a project name"
        )));
    }
    Ok(p)
}

fn check_lane(lane: &str) -> Result<String> {
    match lane.trim().to_ascii_lowercase().as_str() {
        "" | "none" => Ok(String::new()),
        "build" | "code" => Ok("build".into()),
        "work" | "research" => Ok("work".into()),
        other => Err(ParziError::Validation(format!(
            "lane `{other}` is not build or work"
        ))),
    }
}

fn clean_title(raw: &str) -> Result<String> {
    let t: String = raw.trim().chars().take(200).collect();
    if t.is_empty() {
        return Err(ParziError::Validation("a task needs a title".into()));
    }
    Ok(t)
}

/// One project's tasks are a graph: ids are unique, needs name tasks of the
/// same project, nothing waits on itself in a circle, and a task cannot be
/// done while something it needs is still open.
pub fn validate(tasks: &[Task]) -> std::result::Result<(), String> {
    let mut seen = HashSet::new();
    for t in tasks {
        if !seen.insert(t.id.as_str()) {
            return Err(format!("task id `{}` is used twice", t.id));
        }
    }
    let pos: HashMap<&str, usize> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.as_str(), i))
        .collect();
    for t in tasks {
        for need in &t.needs {
            if !pos.contains_key(need.as_str()) {
                return Err(format!(
                    "task `{}` needs `{need}`, which is not a task in this project",
                    t.id
                ));
            }
        }
    }
    // Cycle check: iterative DFS, 1 = on the stack, 2 = finished.
    let mut color = vec![0u8; tasks.len()];
    for start in 0..tasks.len() {
        if color[start] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, bool)> = vec![(start, false)];
        while let Some((i, closing)) = stack.pop() {
            if closing {
                color[i] = 2;
                continue;
            }
            match color[i] {
                2 => continue,
                1 => {
                    return Err(format!(
                        "tasks depend on each other in a circle through `{}`",
                        tasks[i].id
                    ))
                }
                _ => {}
            }
            color[i] = 1;
            stack.push((i, true));
            for need in &tasks[i].needs {
                if let Some(&j) = pos.get(need.as_str()) {
                    stack.push((j, false));
                }
            }
        }
    }
    for t in tasks.iter().filter(|t| t.status == TaskStatus::Done) {
        let open = open_needs(t, tasks);
        if !open.is_empty() {
            return Err(format!(
                "task `{}` cannot be done while it waits on {}",
                t.id,
                open.join(", ")
            ));
        }
    }
    Ok(())
}

/// The needs of `task` that are not done yet.
#[must_use]
pub fn open_needs(task: &Task, scope: &[Task]) -> Vec<String> {
    task.needs
        .iter()
        .filter(|n| {
            !scope
                .iter()
                .any(|t| &t.id == *n && t.status == TaskStatus::Done)
        })
        .cloned()
        .collect()
}

/// Ready and blocked, worked out per project.
#[must_use]
pub fn views(tasks: Vec<Task>) -> Vec<TaskView> {
    let by_scope: HashMap<String, Vec<Task>> = tasks.iter().fold(HashMap::new(), |mut m, t| {
        m.entry(scope_key(t.project.as_deref()))
            .or_default()
            .push(t.clone());
        m
    });
    tasks
        .into_iter()
        .map(|task| {
            let scope = &by_scope[&scope_key(task.project.as_deref())];
            let blocked_by = if task.status == TaskStatus::Done {
                vec![]
            } else {
                open_needs(&task, scope)
            };
            TaskView {
                ready: task.status == TaskStatus::Todo && blocked_by.is_empty(),
                blocked_by,
                task,
            }
        })
        .collect()
}

fn scope_key(project: Option<&str>) -> String {
    project.map_or_else(String::new, str::to_lowercase)
}

/// The file a project's tasks live in. Plain slugs keep their name; any
/// other text (and a project literally named "inbox") gets a hashed one.
fn file_name(project: Option<&str>) -> String {
    let Some(p) = project else {
        return "inbox.json".into();
    };
    let lower = p.to_lowercase();
    let plain = lower != "inbox"
        && !lower.is_empty()
        && lower
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if plain {
        return format!("{lower}.json");
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in lower.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let safe: String = lower
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(40)
        .collect();
    format!("{safe}-{h:016x}.json")
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ScopeFile {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    tasks: Vec<Task>,
}

#[derive(Debug, Clone)]
pub struct TaskStore {
    root: PathBuf,
}

impl TaskStore {
    pub fn open() -> Result<Self> {
        Ok(Self::at(crate::paths::parzi_dir()?.join("tasks")))
    }

    #[must_use]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, project: Option<&str>) -> PathBuf {
        self.root.join(file_name(project))
    }

    fn read_file(path: &std::path::Path) -> Result<Option<ScopeFile>> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut f: ScopeFile = serde_json::from_str(&text).map_err(|e| {
                    ParziError::Store(format!("{} is not a task list: {e}", path.display()))
                })?;
                for t in &mut f.tasks {
                    t.project.clone_from(&f.project);
                }
                Ok(Some(f))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn load(&self, project: Option<&str>) -> Result<ScopeFile> {
        Ok(Self::read_file(&self.path(project))?.unwrap_or(ScopeFile {
            project: project.map(str::to_string),
            tasks: vec![],
        }))
    }

    fn save(&self, file: &ScopeFile) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.path(file.project.as_deref());
        if file.tasks.is_empty() {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
                _ => Ok(()),
            };
        }
        crate::atomic_write(&path, serde_json::to_string_pretty(file)?.as_bytes())
    }

    fn files(&self) -> Result<Vec<ScopeFile>> {
        let mut out = vec![];
        let entries = match std::fs::read_dir(&self.root) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            match Self::read_file(&path) {
                Ok(Some(f)) => out.push(f),
                Ok(None) => {}
                // One broken file must not hide every other project's tasks.
                Err(err) => tracing::warn!("{err}"),
            }
        }
        Ok(out)
    }

    fn sorted(mut tasks: Vec<Task>) -> Vec<Task> {
        tasks.sort_by(|a, b| a.order.cmp(&b.order).then(a.created.cmp(&b.created)));
        tasks
    }

    /// Every task in every project, each project in its own order.
    pub fn all(&self) -> Result<Vec<Task>> {
        let mut files = self.files()?;
        files.sort_by_key(|f| scope_key(f.project.as_deref()));
        Ok(files
            .into_iter()
            .flat_map(|f| Self::sorted(f.tasks))
            .collect())
    }

    /// One project's tasks (None is the Inbox).
    pub fn scope(&self, project: Option<&str>) -> Result<Vec<Task>> {
        Ok(Self::sorted(self.load(project)?.tasks))
    }

    pub fn get(&self, id: &str) -> Result<Task> {
        let id = id.trim();
        self.all()?
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| ParziError::Store(format!("no task `{id}`")))
    }

    /// The tasks a session is linked to.
    pub fn linked(&self, session: &str) -> Result<Vec<Task>> {
        Ok(self
            .all()?
            .into_iter()
            .filter(|t| t.session.as_deref() == Some(session))
            .collect())
    }

    /// The file holding `id`, loaded.
    fn find(&self, id: &str) -> Result<ScopeFile> {
        let id = id.trim();
        self.files()?
            .into_iter()
            .find(|f| f.tasks.iter().any(|t| t.id == id))
            .ok_or_else(|| ParziError::Store(format!("no task `{id}`")))
    }

    fn taken_ids(&self) -> Result<HashSet<String>> {
        Ok(self.all()?.into_iter().map(|t| t.id).collect())
    }

    /// Adds tasks to one project. A need may name an existing task of that
    /// project or another task of this batch by its `key`.
    pub fn add(&self, project: Option<&str>, drafts: &[NewTask], by: &str) -> Result<Vec<Task>> {
        if drafts.is_empty() {
            return Err(ParziError::Validation("nothing to add".into()));
        }
        let project = project.map(check_project).transpose()?;
        let added = {
            let _held = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
            let mut file = self.load(project)?;
            if file.project.is_none() {
                file.project = project.map(str::to_string);
            }
            let mut taken = self.taken_ids()?;
            let now = now_ms();
            let last = file.tasks.iter().map(|t| t.order).max().unwrap_or(0);
            let mut keys: HashMap<String, String> = HashMap::new();
            let mut fresh = vec![];
            for (order, d) in (last + 1..).zip(drafts) {
                let id = fresh_id(&taken);
                taken.insert(id.clone());
                if let Some(k) = d.key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
                    keys.insert(k.to_string(), id.clone());
                }
                fresh.push(Task {
                    id,
                    title: clean_title(&d.title)?,
                    notes: d.notes.trim().to_string(),
                    status: d.status.unwrap_or_default(),
                    project: file.project.clone(),
                    needs: vec![],
                    lane: check_lane(&d.lane)?,
                    session: None,
                    created: now,
                    updated: now,
                    created_by: by.to_string(),
                    order,
                });
            }
            for (t, d) in fresh.iter_mut().zip(drafts) {
                t.needs = dedup(
                    d.needs
                        .iter()
                        .map(|n| n.trim())
                        .filter(|n| !n.is_empty())
                        .map(|n| keys.get(n).cloned().unwrap_or_else(|| n.to_string())),
                );
            }
            file.tasks.extend(fresh.iter().cloned());
            validate(&file.tasks).map_err(ParziError::Validation)?;
            self.save(&file)?;
            fresh
        };
        publish(&Change {
            ids: added.iter().map(|t| t.id.clone()).collect(),
            finished: vec![],
        });
        Ok(added)
    }

    /// Changes one task. Moving it to another project drops its needs (they
    /// name tasks of the old one) and the old project's needs on it.
    pub fn update(&self, id: &str, patch: &TaskPatch) -> Result<Task> {
        let id = id.trim().to_string();
        let out = {
            let _held = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
            let mut file = self.find(&id)?;
            let at = file
                .tasks
                .iter()
                .position(|t| t.id == id)
                .ok_or_else(|| ParziError::Store(format!("no task `{id}`")))?;
            let mut task = file.tasks[at].clone();
            if let Some(t) = &patch.title {
                task.title = clean_title(t)?;
            }
            if let Some(n) = &patch.notes {
                task.notes = n.trim_end().to_string();
            }
            if let Some(s) = patch.status {
                task.status = s;
            }
            if let Some(l) = &patch.lane {
                task.lane = check_lane(l)?;
            }
            if let Some(s) = &patch.session {
                task.session = Some(s.trim().to_string()).filter(|s| !s.is_empty());
            }
            if let Some(n) = &patch.needs {
                task.needs = dedup(
                    n.iter()
                        .map(|x| x.trim().to_string())
                        .filter(|x| !x.is_empty()),
                );
            }
            task.updated = now_ms();
            let target = match &patch.project {
                None => file.project.clone(),
                Some(p) if p.trim().is_empty() => None,
                Some(p) => Some(check_project(p)?.to_string()),
            };
            if same_scope(target.as_deref(), file.project.as_deref()) {
                file.tasks[at] = task.clone();
                validate(&file.tasks).map_err(ParziError::Validation)?;
                self.save(&file)?;
            } else {
                file.tasks.remove(at);
                for t in &mut file.tasks {
                    t.needs.retain(|n| *n != id);
                }
                let mut dest = self.load(target.as_deref())?;
                if dest.project.is_none() {
                    dest.project.clone_from(&target);
                }
                task.project.clone_from(&dest.project);
                if patch.needs.is_none() {
                    task.needs.clear();
                }
                task.order = dest.tasks.iter().map(|t| t.order).max().unwrap_or(0) + 1;
                dest.tasks.push(task.clone());
                validate(&dest.tasks).map_err(ParziError::Validation)?;
                self.save(&dest)?;
                self.save(&file)?;
            }
            task
        };
        publish(&Change {
            ids: vec![out.id.clone()],
            finished: vec![],
        });
        Ok(out)
    }

    /// Removes a task and every need on it.
    pub fn delete(&self, id: &str) -> Result<Task> {
        let id = id.trim().to_string();
        let gone = {
            let _held = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
            let mut file = self.find(&id)?;
            let at = file
                .tasks
                .iter()
                .position(|t| t.id == id)
                .ok_or_else(|| ParziError::Store(format!("no task `{id}`")))?;
            let gone = file.tasks.remove(at);
            for t in &mut file.tasks {
                t.needs.retain(|n| *n != id);
            }
            self.save(&file)?;
            gone
        };
        publish(&Change {
            ids: vec![gone.id.clone()],
            finished: vec![],
        });
        Ok(gone)
    }

    /// Puts `ids` in this order. Each project keeps the order slots its
    /// listed tasks held, so tasks not listed (another column) stay put.
    pub fn reorder(&self, ids: &[String]) -> Result<()> {
        {
            let _held = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
            for mut file in self.files()? {
                let listed: Vec<&String> = ids
                    .iter()
                    .filter(|id| file.tasks.iter().any(|t| &t.id == *id))
                    .collect();
                if listed.is_empty() {
                    continue;
                }
                let mut slots: Vec<i64> = file
                    .tasks
                    .iter()
                    .filter(|t| listed.contains(&&t.id))
                    .map(|t| t.order)
                    .collect();
                slots.sort_unstable();
                slots.dedup();
                // Ties (old files, fresh imports) get room after the last slot.
                let mut next = slots.last().copied().unwrap_or(0);
                while slots.len() < listed.len() {
                    next += 1;
                    slots.push(next);
                }
                for (id, slot) in listed.iter().zip(slots) {
                    if let Some(t) = file.tasks.iter_mut().find(|t| &t.id == *id) {
                        t.order = slot;
                    }
                }
                self.save(&file)?;
            }
        }
        publish(&Change {
            ids: ids.to_vec(),
            finished: vec![],
        });
        Ok(())
    }

    /// Moves the tasks linked to `session` whose status is in `from` to `to`.
    /// Returns the tasks that changed. Never makes a task done.
    pub fn move_linked(
        &self,
        session: &str,
        from: &[TaskStatus],
        to: TaskStatus,
    ) -> Result<Vec<Task>> {
        if to == TaskStatus::Done {
            return Err(ParziError::Validation(
                "a run never marks a task done on its own".into(),
            ));
        }
        let _held = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
        let mut changed = vec![];
        for mut file in self.files()? {
            let mut dirty = false;
            for t in &mut file.tasks {
                if t.session.as_deref() == Some(session) && from.contains(&t.status) {
                    t.status = to;
                    t.updated = now_ms();
                    changed.push(t.clone());
                    dirty = true;
                }
            }
            if dirty {
                self.save(&file)?;
            }
        }
        Ok(changed)
    }

    /// Replaces every task (sync's way in): each project gets its file,
    /// projects with no tasks left lose theirs. The lists are mended first,
    /// since merging two devices can leave a need dangling.
    pub fn replace_all(&self, tasks: Vec<Task>) -> Result<()> {
        let mut by_scope: HashMap<String, (Option<String>, Vec<Task>)> = HashMap::new();
        for t in tasks {
            by_scope
                .entry(scope_key(t.project.as_deref()))
                .or_insert_with(|| (t.project.clone(), vec![]))
                .1
                .push(t);
        }
        let mut ids = vec![];
        {
            let _held = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
            let before = self.files()?;
            for (project, list) in by_scope.values() {
                let list = sanitize(list.clone());
                ids.extend(list.iter().map(|t| t.id.clone()));
                self.save(&ScopeFile {
                    project: project.clone(),
                    tasks: list,
                })?;
            }
            for old in before {
                if !by_scope.contains_key(&scope_key(old.project.as_deref())) {
                    ids.extend(old.tasks.iter().map(|t| t.id.clone()));
                    self.save(&ScopeFile {
                        project: old.project,
                        tasks: vec![],
                    })?;
                }
            }
        }
        publish(&Change {
            ids,
            finished: vec![],
        });
        Ok(())
    }
}

/// One project's list made valid without losing a task: a repeated id keeps
/// its newest copy, needs on missing tasks go, a done task drops needs that
/// are still open, and a need that closes a circle is cut.
#[must_use]
pub fn sanitize(tasks: Vec<Task>) -> Vec<Task> {
    let mut newest: HashMap<String, Task> = HashMap::new();
    for t in tasks {
        match newest.get(&t.id) {
            Some(have) if have.updated >= t.updated => {}
            _ => {
                newest.insert(t.id.clone(), t);
            }
        }
    }
    let mut list: Vec<Task> = newest.into_values().collect();
    list.sort_by(|a, b| a.order.cmp(&b.order).then(a.created.cmp(&b.created)));
    let ids: HashSet<String> = list.iter().map(|t| t.id.clone()).collect();
    for t in &mut list {
        let own = t.id.clone();
        t.needs.retain(|n| *n != own && ids.contains(n));
    }
    // Cut circles: walk the graph and drop every edge back onto the path.
    let pos: HashMap<String, usize> = list
        .iter()
        .enumerate()
        .map(|(i, t)| (t.id.clone(), i))
        .collect();
    let mut color = vec![0u8; list.len()];
    for start in 0..list.len() {
        if color[start] == 0 {
            cut_back_edges(&mut list, &pos, &mut color, start);
        }
    }
    let done: HashSet<String> = list
        .iter()
        .filter(|t| t.status == TaskStatus::Done)
        .map(|t| t.id.clone())
        .collect();
    for t in list.iter_mut().filter(|t| t.status == TaskStatus::Done) {
        t.needs.retain(|n| done.contains(n));
    }
    list
}

fn cut_back_edges(list: &mut [Task], pos: &HashMap<String, usize>, color: &mut [u8], start: usize) {
    // (task, index of the next need to look at); 1 = on the path, 2 = done.
    let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
    color[start] = 1;
    while let Some(top) = stack.len().checked_sub(1) {
        let (i, next) = stack[top];
        if next >= list[i].needs.len() {
            color[i] = 2;
            stack.pop();
            continue;
        }
        match pos.get(&list[i].needs[next]).map(|&j| (j, color[j])) {
            Some((_, 1)) => {
                list[i].needs.remove(next);
            }
            Some((j, 0)) => {
                stack[top].1 += 1;
                color[j] = 1;
                stack.push((j, 0));
            }
            _ => stack[top].1 += 1,
        }
    }
}

fn dedup(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items.filter(|x| seen.insert(x.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, TaskStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = TaskStore::at(dir.path().join("tasks"));
        (dir, s)
    }

    fn draft(title: &str) -> NewTask {
        NewTask {
            title: title.into(),
            ..NewTask::default()
        }
    }

    fn task(id: &str, status: TaskStatus, needs: &[&str]) -> Task {
        Task {
            id: id.into(),
            title: id.into(),
            notes: String::new(),
            status,
            project: None,
            needs: needs.iter().map(|s| (*s).to_string()).collect(),
            lane: String::new(),
            session: None,
            created: 0,
            updated: 0,
            created_by: "you".into(),
            order: 0,
        }
    }

    #[test]
    fn statuses_parse_the_way_agents_write_them() {
        assert_eq!(TaskStatus::parse("Done"), Some(TaskStatus::Done));
        assert_eq!(TaskStatus::parse("in progress"), Some(TaskStatus::Doing));
        assert_eq!(TaskStatus::parse(" review "), Some(TaskStatus::Review));
        assert_eq!(TaskStatus::parse("failed"), Some(TaskStatus::Failed));
        assert_eq!(TaskStatus::parse("whenever"), None);
        assert_eq!(TaskStatus::Doing.as_str(), "doing");
    }

    #[test]
    fn the_graph_rules_hold() {
        use TaskStatus::{Done, Todo};
        assert!(validate(&[task("a", Todo, &[]), task("b", Todo, &["a"])]).is_ok());
        let twice = validate(&[task("a", Todo, &[]), task("a", Todo, &[])]).unwrap_err();
        assert!(twice.contains("twice"), "{twice}");
        let unknown = validate(&[task("b", Todo, &["a"])]).unwrap_err();
        assert!(unknown.contains("not a task"), "{unknown}");
        let circle = validate(&[task("x", Todo, &["y"]), task("y", Todo, &["x"])]).unwrap_err();
        assert!(circle.contains("circle"), "{circle}");
        let selfish = validate(&[task("x", Todo, &["x"])]).unwrap_err();
        assert!(selfish.contains("circle"), "{selfish}");
        let early = validate(&[task("a", Todo, &[]), task("b", Done, &["a"])]).unwrap_err();
        assert!(early.contains("waits on a"), "{early}");
        assert!(validate(&[task("a", Done, &[]), task("b", Done, &["a"])]).is_ok());
    }

    #[test]
    fn ready_and_blocked_follow_the_needs() {
        use TaskStatus::{Doing, Done, Todo};
        let v = views(vec![
            task("a", Done, &[]),
            task("b", Todo, &["a"]),
            task("c", Todo, &["b"]),
            task("d", Doing, &[]),
        ]);
        let by = |id: &str| v.iter().find(|x| x.task.id == id).unwrap();
        assert!(by("b").ready && by("b").blocked_by.is_empty());
        assert!(!by("c").ready);
        assert_eq!(by("c").blocked_by, vec!["b".to_string()]);
        assert!(!by("d").ready, "doing is not ready");
        assert!(!by("a").ready, "done is not ready");
    }

    #[test]
    fn a_batch_can_need_itself_by_key_and_ids_stay_unique() {
        let (_d, s) = store();
        let mut api = draft("API");
        api.key = Some("api".into());
        let mut ui = draft("UI");
        ui.needs = vec!["api".into()];
        ui.lane = "Build".into();
        let added = s.add(Some("parzi"), &[api, ui], "you").unwrap();
        assert_eq!(added.len(), 2);
        assert_ne!(added[0].id, added[1].id);
        assert_eq!(added[1].needs, vec![added[0].id.clone()]);
        assert_eq!(added[1].lane, "build");
        assert_eq!(added[0].project.as_deref(), Some("parzi"));
        assert_eq!(added[0].id.len(), 5);
        let mut bad = draft("orphan");
        bad.needs = vec!["nope".into()];
        assert!(s.add(Some("parzi"), &[bad], "you").is_err());
        assert_eq!(
            s.scope(Some("parzi")).unwrap().len(),
            2,
            "a failed add writes nothing"
        );
        assert!(
            s.add(None, &[draft("  ")], "you").is_err(),
            "a title is required"
        );
    }

    #[test]
    fn projects_keep_their_own_files_and_the_inbox_is_none() {
        let (_d, s) = store();
        s.add(None, &[draft("loose")], "you").unwrap();
        s.add(Some("Parzi"), &[draft("in parzi")], "you").unwrap();
        s.add(Some("parzi"), &[draft("same project")], "you")
            .unwrap();
        s.add(Some("My Thesis"), &[draft("odd name")], "you")
            .unwrap();
        assert_eq!(s.scope(None).unwrap().len(), 1);
        assert_eq!(
            s.scope(Some("PARZI")).unwrap().len(),
            2,
            "slugs match without case"
        );
        assert_eq!(s.scope(Some("My Thesis")).unwrap().len(), 1);
        assert_eq!(s.all().unwrap().len(), 4);
        assert!(s.root.join("inbox.json").exists());
        assert!(s.root.join("parzi.json").exists());
        assert_ne!(
            file_name(Some("inbox")),
            "inbox.json",
            "a project named inbox is not the Inbox"
        );
        assert!(s.add(Some("../evil"), &[draft("x")], "you").is_err());
    }

    #[test]
    fn done_waits_for_needs_and_moves_drop_cross_project_needs() {
        let (_d, s) = store();
        let a = s.add(Some("p"), &[draft("a")], "you").unwrap().remove(0);
        let mut bd = draft("b");
        bd.needs = vec![a.id.clone()];
        let b = s.add(Some("p"), &[bd], "you").unwrap().remove(0);
        let done = TaskPatch {
            status: Some(TaskStatus::Done),
            ..TaskPatch::default()
        };
        let err = s.update(&b.id, &done).unwrap_err().to_string();
        assert!(err.contains("waits on"), "{err}");
        s.update(&a.id, &done).unwrap();
        assert_eq!(s.update(&b.id, &done).unwrap().status, TaskStatus::Done);

        // Moving `a` to the Inbox takes it out of `b`'s needs.
        let to_inbox = TaskPatch {
            project: Some(String::new()),
            ..TaskPatch::default()
        };
        let moved = s.update(&a.id, &to_inbox).unwrap();
        assert_eq!(moved.project, None);
        assert!(s.get(&b.id).unwrap().needs.is_empty());
        assert_eq!(s.scope(None).unwrap()[0].id, a.id);
    }

    #[test]
    fn delete_unhooks_needs_and_reorder_keeps_other_slots() {
        let (_d, s) = store();
        let added = s
            .add(
                None,
                &[draft("1"), draft("2"), draft("3"), draft("4")],
                "you",
            )
            .unwrap();
        let ids: Vec<String> = added.iter().map(|t| t.id.clone()).collect();
        // Swap 2 and 3 only: 1 and 4 keep their places.
        s.reorder(&[ids[2].clone(), ids[1].clone()]).unwrap();
        let order: Vec<String> = s
            .scope(None)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(order, ["1", "3", "2", "4"]);

        let patch = TaskPatch {
            needs: Some(vec![ids[0].clone()]),
            ..TaskPatch::default()
        };
        s.update(&ids[3], &patch).unwrap();
        s.delete(&ids[0]).unwrap();
        assert!(s.get(&ids[3]).unwrap().needs.is_empty());
        assert!(s.get(&ids[0]).is_err());
    }

    #[test]
    fn runs_move_linked_tasks_but_never_to_done() {
        let (_d, s) = store();
        let t = s.add(None, &[draft("work")], "you").unwrap().remove(0);
        let link = TaskPatch {
            session: Some("sess-1".into()),
            status: Some(TaskStatus::Doing),
            ..TaskPatch::default()
        };
        s.update(&t.id, &link).unwrap();
        assert_eq!(s.linked("sess-1").unwrap().len(), 1);
        let moved = s
            .move_linked("sess-1", &[TaskStatus::Doing], TaskStatus::Review)
            .unwrap();
        assert_eq!(moved.len(), 1);
        assert_eq!(s.get(&t.id).unwrap().status, TaskStatus::Review);
        assert!(s
            .move_linked("sess-1", &[TaskStatus::Doing], TaskStatus::Review)
            .unwrap()
            .is_empty());
        assert!(s
            .move_linked("sess-1", &[TaskStatus::Review], TaskStatus::Done)
            .is_err());
    }

    #[test]
    fn changes_reach_the_watchers() {
        let (_d, s) = store();
        let seen = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = seen.clone();
        watch(move |c| sink.lock().unwrap().extend(c.ids.iter().cloned()));
        let t = s.add(None, &[draft("watched")], "you").unwrap().remove(0);
        assert!(seen.lock().unwrap().contains(&t.id));
    }

    #[test]
    fn sanitize_mends_what_a_merge_can_break() {
        use TaskStatus::{Done, Todo};
        let mut old = task("a", Todo, &[]);
        old.title = "old".into();
        let mut new = task("a", Todo, &[]);
        new.title = "new".into();
        new.updated = 5;
        let list = sanitize(vec![
            old,
            new,
            task("b", Todo, &["a", "gone", "b"]),
            task("x", Todo, &["y"]),
            task("y", Todo, &["x"]),
            task("d", Done, &["b"]),
        ]);
        assert!(validate(&list).is_ok(), "{:?}", validate(&list));
        let by = |id: &str| list.iter().find(|t| t.id == id).unwrap();
        assert_eq!(list.len(), 5, "no task is lost");
        assert_eq!(by("a").title, "new", "a repeated id keeps the newest");
        assert_eq!(by("b").needs, vec!["a".to_string()]);
        assert!(by("d").needs.is_empty(), "done drops its open needs");
        assert_eq!(
            by("x").needs.len() + by("y").needs.len(),
            1,
            "one edge of the circle goes"
        );
    }

    #[test]
    fn replace_all_rewrites_every_project_and_drops_empty_ones() {
        let (_d, s) = store();
        s.add(Some("gone"), &[draft("old")], "you").unwrap();
        let mut keep = s.add(None, &[draft("inbox")], "you").unwrap();
        let mut moved = task("m1", TaskStatus::Todo, &[]);
        moved.project = Some("parzi".into());
        keep.push(moved);
        s.replace_all(keep).unwrap();
        assert!(s.scope(Some("gone")).unwrap().is_empty());
        assert!(!s.root.join("gone.json").exists());
        assert_eq!(s.scope(None).unwrap().len(), 1);
        assert_eq!(s.scope(Some("parzi")).unwrap()[0].id, "m1");
    }

    #[test]
    fn sessions_without_a_project_land_in_the_inbox() {
        assert_eq!(project_of_session("default"), None);
        assert_eq!(project_of_session(""), None);
        assert_eq!(project_of_session("parzi").as_deref(), Some("parzi"));
    }
}
