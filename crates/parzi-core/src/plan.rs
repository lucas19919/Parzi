
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::brain::Vault;
use crate::text::{clip, fold};

pub const MAX_ITEMS: usize = 40;
pub const TITLE_CAP: usize = 80;
pub const DETAIL_CAP: usize = 400;
pub const NOTE_CAP: usize = 400;
pub const ID_CAP: usize = 24;
pub const SUMMARY_CAP: usize = 1_500;
const GOAL_CAP: usize = 400;
const MAX_PHASES: usize = 8;
const PHASE_CAP: usize = 40;
const BRANCH_CAP: usize = 120;
const FENCE: &str = "parzi-plan";
const DESCRIPTION: &str =
    "The living plan of this project. Agents read and change it with plan.read and plan.write.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanState {
    #[default]
    Planned,
    Active,
    Done,
    Dropped,
}

impl PlanState {
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim().to_ascii_lowercase().as_str() {
            "planned" | "todo" | "open" => Self::Planned,
            "active" | "started" | "working" => Self::Active,
            "done" | "finished" => Self::Done,
            "dropped" | "cancelled" | "canceled" => Self::Dropped,
            _ => return None,
        })
    }

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Active => "active",
            Self::Done => "done",
            Self::Dropped => "dropped",
        }
    }

    #[must_use]
    pub fn settled(self) -> bool {
        matches!(self, Self::Done | Self::Dropped)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub state: PlanState,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub version: u32,
    pub title: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub phases: Vec<String>,
    #[serde(default)]
    pub items: Vec<PlanItem>,
    #[serde(default)]
    pub updated: u64,
    #[serde(default)]
    pub by: String,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            version: 1,
            title: String::new(),
            goal: String::new(),
            phases: vec![],
            items: vec![],
            updated: 0,
            by: String::new(),
        }
    }
}

impl Plan {
    #[must_use]
    pub fn item(&self, id: &str) -> Option<&PlanItem> {
        self.items.iter().find(|i| i.id == id)
    }

    #[must_use]
    pub fn progress(&self) -> (usize, usize) {
        let counted = self
            .items
            .iter()
            .filter(|i| i.state != PlanState::Dropped)
            .count();
        let done = self
            .items
            .iter()
            .filter(|i| i.state == PlanState::Done)
            .count();
        (done, counted)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanView {
    pub project: String,
    pub path: String,
    pub plan: Option<Plan>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanWrite {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub phases: Option<Vec<String>>,
    #[serde(default)]
    pub items: Option<Vec<ItemWrite>>,
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemWrite {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub needs: Option<Vec<String>>,
    #[serde(default, alias = "status")]
    pub state: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub remove: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    pub plan: Plan,
    pub activated: Vec<String>,
    pub archived: Option<String>,
}

#[must_use]
pub fn valid_id(id: &str) -> bool {
    (1..=ID_CAP).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn validate(items: &[PlanItem]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for i in items {
        if !seen.insert(i.id.as_str()) {
            return Err(format!("item `{}` is used twice", i.id));
        }
    }
    let pos: HashMap<&str, usize> = items
        .iter()
        .enumerate()
        .map(|(n, i)| (i.id.as_str(), n))
        .collect();
    for i in items {
        for need in &i.needs {
            if !pos.contains_key(need.as_str()) {
                return Err(format!(
                    "item `{}` needs `{need}`, which is not an item of this plan",
                    i.id
                ));
            }
        }
    }
    let mut color = vec![0u8; items.len()];
    for start in 0..items.len() {
        if color[start] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, bool)> = vec![(start, false)];
        while let Some((n, closing)) = stack.pop() {
            if closing {
                color[n] = 2;
                continue;
            }
            match color[n] {
                2 => continue,
                1 => {
                    return Err(format!(
                        "items depend on each other in a circle through `{}`",
                        items[n].id
                    ))
                }
                _ => {}
            }
            color[n] = 1;
            stack.push((n, true));
            for need in &items[n].needs {
                if let Some(&m) = pos.get(need.as_str()) {
                    stack.push((m, false));
                }
            }
        }
    }
    Ok(())
}

#[must_use]
pub fn sanitize(items: Vec<PlanItem>) -> Vec<PlanItem> {
    let mut seen = HashSet::new();
    let mut list: Vec<PlanItem> = items
        .into_iter()
        .filter(|i| seen.insert(i.id.clone()))
        .collect();
    let ids: HashSet<String> = list.iter().map(|i| i.id.clone()).collect();
    for i in &mut list {
        let own = i.id.clone();
        let mut kept = HashSet::new();
        i.needs
            .retain(|n| *n != own && ids.contains(n) && kept.insert(n.clone()));
    }
    let pos: HashMap<String, usize> = list
        .iter()
        .enumerate()
        .map(|(n, i)| (i.id.clone(), n))
        .collect();
    let mut color = vec![0u8; list.len()];
    for start in 0..list.len() {
        if color[start] == 0 {
            cut_back_edges(&mut list, &pos, &mut color, start);
        }
    }
    list
}

fn cut_back_edges(
    list: &mut [PlanItem],
    pos: &HashMap<String, usize>,
    color: &mut [u8],
    start: usize,
) {
    let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
    color[start] = 1;
    while let Some(top) = stack.len().checked_sub(1) {
        let (n, next) = stack[top];
        if next >= list[n].needs.len() {
            color[n] = 2;
            stack.pop();
            continue;
        }
        match pos.get(&list[n].needs[next]).map(|&m| (m, color[m])) {
            Some((_, 1)) => {
                list[n].needs.remove(next);
            }
            Some((m, 0)) => {
                stack[top].1 += 1;
                color[m] = 1;
                stack.push((m, 0));
            }
            _ => stack[top].1 += 1,
        }
    }
}

fn block(note: &str) -> Option<String> {
    let mut lines = note.lines();
    lines.find(|l| {
        l.trim()
            .strip_prefix("```")
            .is_some_and(|rest| rest.trim() == FENCE)
    })?;
    let mut out = String::new();
    for l in lines {
        if l.trim() == "```" {
            return Some(out);
        }
        out.push_str(l);
        out.push('\n');
    }
    Some(out)
}

fn text_of(v: &Value, key: &str, cap: usize) -> String {
    clip(&fold(v.get(key).and_then(Value::as_str).unwrap_or("")), cap)
}

fn strings_of(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn parse(note: &str) -> Result<Plan, String> {
    let raw = block(note).ok_or("the note has no ```parzi-plan block")?;
    let v: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("the parzi-plan block is not valid JSON: {e}"))?;
    if !v.is_object() {
        return Err("the parzi-plan block is not a JSON object".into());
    }
    let mut phases: Vec<String> = vec![];
    for p in strings_of(&v, "phases") {
        let p = clip(&fold(&p), PHASE_CAP);
        if !phases.contains(&p) && phases.len() < MAX_PHASES {
            phases.push(p);
        }
    }
    let mut items = vec![];
    for it in v
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let id = it
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !valid_id(&id) {
            continue;
        }
        let mut title = text_of(it, "title", TITLE_CAP);
        if title.is_empty() {
            title.clone_from(&id);
        }
        let mut phase = text_of(it, "phase", PHASE_CAP);
        if !phase.is_empty() && !phases.contains(&phase) {
            if phases.len() < MAX_PHASES {
                phases.push(phase.clone());
            } else {
                phase.clear();
            }
        }
        items.push(PlanItem {
            id,
            title,
            detail: clip(
                it.get("detail")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim(),
                DETAIL_CAP,
            ),
            phase,
            needs: strings_of(it, "needs"),
            state: it
                .get("state")
                .and_then(Value::as_str)
                .and_then(PlanState::parse)
                .unwrap_or_default(),
            branch: text_of(it, "branch", BRANCH_CAP),
            note: clip(
                it.get("note").and_then(Value::as_str).unwrap_or("").trim(),
                NOTE_CAP,
            ),
        });
    }
    let mut items = sanitize(items);
    items.truncate(MAX_ITEMS);
    let items = sanitize(items);
    Ok(Plan {
        version: 1,
        title: text_of(&v, "title", TITLE_CAP),
        goal: clip(
            v.get("goal").and_then(Value::as_str).unwrap_or("").trim(),
            GOAL_CAP,
        ),
        phases,
        items,
        updated: v.get("updated").and_then(Value::as_u64).unwrap_or(0),
        by: text_of(&v, "by", 80),
    })
}

fn yaml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

#[must_use]
pub fn grouped(plan: &Plan) -> Vec<(String, Vec<&PlanItem>)> {
    let first = plan.phases.first().cloned().unwrap_or_default();
    let mut groups: Vec<(String, Vec<&PlanItem>)> =
        plan.phases.iter().map(|p| (p.clone(), vec![])).collect();
    if groups.is_empty() {
        groups.push((String::new(), vec![]));
    }
    for i in &plan.items {
        let phase = if i.phase.is_empty() { &first } else { &i.phase };
        match groups.iter_mut().find(|(p, _)| p == phase) {
            Some((_, list)) => list.push(i),
            None => groups[0].1.push(i),
        }
    }
    groups
}

#[must_use]
pub fn render(plan: &Plan) -> String {
    let title = if plan.title.trim().is_empty() {
        "Plan".to_string()
    } else {
        format!("Plan: {}", plan.title.trim())
    };
    let mut out = format!(
        "---\ntitle: {}\nkind: plan\ndescription: {}\n---\n\n# {}\n\n",
        yaml_str(&title),
        yaml_str(DESCRIPTION),
        title
    );
    if !plan.goal.trim().is_empty() {
        out.push_str(&format!("{}\n\n", fold(&plan.goal)));
    }
    let (done, counted) = plan.progress();
    out.push_str(&format!("{done} of {counted} done.\n"));
    for (phase, items) in grouped(plan) {
        if items.is_empty() {
            continue;
        }
        if !phase.is_empty() {
            out.push_str(&format!("\n## {phase}\n\n"));
        } else {
            out.push('\n');
        }
        for i in items {
            let mark = match i.state {
                PlanState::Done => "[x]",
                PlanState::Dropped => "[-]",
                PlanState::Active => "[>]",
                PlanState::Planned => "[ ]",
            };
            out.push_str(&format!("- {mark} {} (`{}`)", i.title, i.id));
            if !i.needs.is_empty() {
                out.push_str(&format!(", after {}", i.needs.join(", ")));
            }
            out.push('\n');
        }
    }
    let json = serde_json::to_string_pretty(plan).unwrap_or_else(|_| "{}".into());
    out.push_str(&format!(
        "\nThe plan itself is the block below; Parzi and its agents keep it.\n\n```{FENCE}\n{json}\n```\n"
    ));
    out
}

fn capped(what: &str, text: &str, cap: usize) -> Result<String, String> {
    let t = text.trim();
    if t.chars().count() > cap {
        return Err(format!(
            "{what} is {} chars; keep it to {cap}",
            t.chars().count()
        ));
    }
    Ok(t.to_string())
}

pub fn apply(
    plan: Option<Plan>,
    w: PlanWrite,
    by: &str,
    now: u64,
) -> Result<(Plan, Vec<String>), String> {
    let mut plan = match plan {
        Some(p) if !w.replace => p,
        _ => Plan::default(),
    };
    if let Some(t) = &w.title {
        plan.title = fold(&capped("the title", t, TITLE_CAP)?);
    }
    if let Some(g) = &w.goal {
        plan.goal = capped("the goal", g, GOAL_CAP)?;
    }
    if let Some(phases) = &w.phases {
        let mut list: Vec<String> = vec![];
        for p in phases {
            let p = fold(&capped("a phase name", p, PHASE_CAP)?);
            if !p.is_empty() && !list.contains(&p) {
                list.push(p);
            }
        }
        if list.len() > MAX_PHASES {
            return Err(format!("a plan has at most {MAX_PHASES} phases"));
        }
        plan.phases = list;
    }
    let mut activated = vec![];
    for iw in w.items.unwrap_or_default() {
        let id = iw.id.trim().to_string();
        if !valid_id(&id) {
            return Err(format!(
                "item id `{id}` must be 1 to {ID_CAP} of a-z, 0-9 and -"
            ));
        }
        if iw.remove {
            plan.items.retain(|i| i.id != id);
            for i in &mut plan.items {
                i.needs.retain(|n| *n != id);
            }
            continue;
        }
        let state = match iw.state.as_deref() {
            Some(s) => Some(PlanState::parse(s).ok_or_else(|| {
                format!("item `{id}`: state `{s}` is not planned, active, done or dropped")
            })?),
            None => None,
        };
        let at = match plan.items.iter().position(|i| i.id == id) {
            Some(at) => at,
            None => {
                let title = iw.title.as_deref().map(str::trim).unwrap_or("");
                if title.is_empty() {
                    return Err(format!("item `{id}` is new and needs a title"));
                }
                plan.items.push(PlanItem {
                    id: id.clone(),
                    title: String::new(),
                    detail: String::new(),
                    phase: String::new(),
                    needs: vec![],
                    state: PlanState::Planned,
                    branch: String::new(),
                    note: String::new(),
                });
                plan.items.len() - 1
            }
        };
        let item = &mut plan.items[at];
        if let Some(t) = &iw.title {
            let t = fold(&capped(&format!("item `{id}`'s title"), t, TITLE_CAP)?);
            if t.is_empty() {
                return Err(format!("item `{id}` needs a title"));
            }
            item.title = t;
        }
        if let Some(d) = &iw.detail {
            item.detail = capped(&format!("item `{id}`'s detail"), d, DETAIL_CAP)?;
        }
        if let Some(n) = &iw.note {
            item.note = capped(&format!("item `{id}`'s note"), n, NOTE_CAP)?;
        }
        if let Some(b) = &iw.branch {
            item.branch = fold(&capped(&format!("item `{id}`'s branch"), b, BRANCH_CAP)?);
        }
        if let Some(p) = &iw.phase {
            let p = fold(&capped(&format!("item `{id}`'s phase"), p, PHASE_CAP)?);
            if !p.is_empty() && !plan.phases.contains(&p) {
                if plan.phases.len() >= MAX_PHASES {
                    return Err(format!(
                        "item `{id}`'s phase `{p}` is not one of {}",
                        plan.phases.join(", ")
                    ));
                }
                plan.phases.push(p.clone());
            }
            plan.items[at].phase = p;
        }
        let item = &mut plan.items[at];
        if let Some(needs) = &iw.needs {
            let mut list: Vec<String> = vec![];
            for n in needs.iter().map(|n| n.trim().to_string()) {
                if !n.is_empty() && !list.contains(&n) {
                    list.push(n);
                }
            }
            item.needs = list;
        }
        if let Some(s) = state {
            item.state = s;
            if s == PlanState::Active && !activated.contains(&id) {
                activated.push(id.clone());
            }
        }
    }
    if plan.items.len() > MAX_ITEMS {
        return Err(format!(
            "a plan holds at most {MAX_ITEMS} items; this one would have {}. Drop or merge some",
            plan.items.len()
        ));
    }
    validate(&plan.items)?;
    plan.version = 1;
    plan.updated = now;
    plan.by = by.to_string();
    Ok((plan, activated))
}

#[must_use]
pub fn summary(plan: &Plan) -> String {
    let (done, counted) = plan.progress();
    let mut head = format!("Plan \"{}\"", fold(&plan.title));
    if !plan.goal.trim().is_empty() {
        head.push_str(&format!(": {}", clip(&fold(&plan.goal), 200)));
    }
    head.push_str(&format!(". {done} of {counted} done."));
    let mut groups: Vec<(String, Vec<(String, String)>)> = vec![];
    for (phase, items) in grouped(plan) {
        let open: Vec<(String, String)> = items
            .iter()
            .filter(|i| !i.state.settled())
            .map(|i| {
                let mut tags = vec![i.state.word().to_string()];
                let waits: Vec<&str> = i
                    .needs
                    .iter()
                    .filter(|n| plan.item(n).is_some_and(|x| !x.state.settled()))
                    .map(String::as_str)
                    .collect();
                if !waits.is_empty() {
                    tags.push(format!("after {}", waits.join(", ")));
                }
                if !i.branch.is_empty() {
                    tags.push(i.branch.clone());
                }
                let base = format!("- {} ({}): {}", i.id, tags.join("; "), i.title);
                let detail = if i.detail.is_empty() {
                    String::new()
                } else {
                    format!(". {}", clip(&fold(&i.detail), 100))
                };
                (base, detail)
            })
            .collect();
        if !open.is_empty() {
            groups.push((phase, open));
        }
    }
    let ids = |s: PlanState| -> Vec<&str> {
        plan.items
            .iter()
            .filter(|i| i.state == s)
            .map(|i| i.id.as_str())
            .collect()
    };
    let mut tail = vec![];
    for (s, word) in [(PlanState::Done, "Done"), (PlanState::Dropped, "Dropped")] {
        let list = ids(s);
        if !list.is_empty() {
            tail.push(format!("{word}: {}.", list.join(", ")));
        }
    }
    let close = "Read it in full with plan.read.";
    let build = |details: bool, limit: usize| -> String {
        let mut out = vec![head.clone()];
        let mut shown = 0;
        let total: usize = groups.iter().map(|(_, l)| l.len()).sum();
        for (phase, list) in &groups {
            if shown >= limit {
                break;
            }
            if !phase.is_empty() {
                out.push(format!("{phase}:"));
            }
            for (base, detail) in list {
                if shown >= limit {
                    break;
                }
                out.push(if details {
                    format!("{base}{detail}")
                } else {
                    base.clone()
                });
                shown += 1;
            }
        }
        if total > shown {
            out.push(format!("…and {} more open items.", total - shown));
        }
        out.extend(tail.iter().cloned());
        out.push(close.into());
        out.join("\n")
    };
    let total: usize = groups.iter().map(|(_, l)| l.len()).sum();
    let mut text = build(true, total);
    if text.chars().count() > SUMMARY_CAP {
        text = build(false, total);
    }
    let mut limit = total;
    while text.chars().count() > SUMMARY_CAP && limit > 0 {
        limit -= 1;
        text = build(false, limit);
    }
    clip(&text, SUMMARY_CAP + 200)
}

#[must_use]
pub fn quote_block(project: &str, item: &PlanItem, pr: Option<&str>) -> String {
    let attr = |s: &str| fold(s).replace('"', "&quot;");
    let mut body = vec![];
    if !item.detail.trim().is_empty() {
        body.push(item.detail.trim().to_string());
    }
    if !item.note.trim().is_empty() {
        body.push(format!("Note: {}", item.note.trim()));
    }
    let needs = if item.needs.is_empty() {
        "none".to_string()
    } else {
        item.needs.join(", ")
    };
    let mut status = format!("State: {}. Needs: {needs}.", item.state.word());
    if !item.branch.is_empty() {
        status.push_str(&format!(" Branch: {}.", item.branch));
    }
    if let Some(pr) = pr.map(str::trim).filter(|p| !p.is_empty()) {
        status.push(' ');
        status.push_str(pr);
    }
    body.push(status);
    format!(
        "<plan-item project=\"{}\" id=\"{}\" title=\"{}\">\n{}\n</plan-item>",
        attr(project),
        attr(&item.id),
        attr(&item.title),
        body.join("\n")
    )
}

pub fn check_slug(slug: &str) -> Result<&str, String> {
    let s = slug.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("default") {
        return Err(
            "this session has no project to plan in; make one with project.create first".into(),
        );
    }
    if s.eq_ignore_ascii_case(crate::brain::EVERYWHERE)
        || s.contains(['/', '\\'])
        || s.starts_with('.')
    {
        return Err(format!("`{s}` is not a project slug"));
    }
    Ok(s)
}

#[must_use]
pub fn note_path(slug: &str) -> String {
    format!("projects/{slug}/plan.md")
}

#[must_use]
pub fn project_of_note(path: &str) -> Option<String> {
    let p = path.replace('\\', "/");
    let rest = p.strip_prefix("projects/")?;
    let (slug, file) = rest.split_once('/')?;
    (file.eq_ignore_ascii_case("plan.md") && check_slug(slug).is_ok()).then(|| slug.to_string())
}

#[must_use]
pub fn load(vault: &Vault, slug: &str) -> PlanView {
    let path = note_path(slug.trim());
    let mut view = PlanView {
        project: slug.trim().to_string(),
        path: path.clone(),
        plan: None,
        error: None,
    };
    if check_slug(slug).is_err() {
        return view;
    }
    match read_note(vault, &path) {
        Ok(None) => {}
        Ok(Some(text)) => match parse(&text) {
            Ok(p) => view.plan = Some(p),
            Err(e) => view.error = Some(e),
        },
        Err(e) => view.error = Some(e),
    }
    view
}

fn read_note(vault: &Vault, path: &str) -> Result<Option<String>, String> {
    let full = vault.resolve(path).map_err(|e| e.to_string())?;
    if !full.is_file() {
        return Ok(None);
    }
    vault.read(path).map(Some).map_err(|e| e.to_string())
}

fn file_slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 40 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "plan".into()
    } else {
        out
    }
}

fn archive(vault: &Vault, slug: &str, text: &str, title: &str) -> Result<String, String> {
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let stem = format!("projects/{slug}/plans/{date}-{}", file_slug(title));
    let mut n = 1;
    let to = loop {
        let candidate = if n == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem}-{n}.md")
        };
        let full = vault.resolve(&candidate).map_err(|e| e.to_string())?;
        if !full.exists() {
            break candidate;
        }
        n += 1;
    };
    vault.write(&to, text).map_err(|e| e.to_string())?;
    Ok(to)
}

static WRITING: Mutex<()> = Mutex::new(());

pub fn write_lock() -> MutexGuard<'static, ()> {
    WRITING.lock().unwrap_or_else(PoisonError::into_inner)
}

#[must_use]
pub fn lock_if_plan(path: &str) -> Option<MutexGuard<'static, ()>> {
    project_of_note(path).map(|_| write_lock())
}

type Watcher = Arc<dyn Fn(&str) + Send + Sync>;
static WATCHER: RwLock<Option<Watcher>> = RwLock::new(None);

pub fn on_change(f: impl Fn(&str) + Send + Sync + 'static) {
    *WATCHER.write().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(f));
}

fn changed(slug: &str) {
    let w = WATCHER
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    if let Some(w) = w {
        w(slug);
    }
}

pub fn write(vault: &Vault, slug: &str, w: PlanWrite, by: &str) -> Result<Written, String> {
    let slug = check_slug(slug)?.to_string();
    let one = write_lock();
    let path = note_path(&slug);
    let old_text = read_note(vault, &path)?;
    let old = match &old_text {
        None => None,
        Some(t) => match parse(t) {
            Ok(p) => Some(p),
            Err(e) if !w.replace => {
                return Err(format!(
                    "the plan note {path} cannot be read ({e}); fix it with brain.write, or start over with replace: true"
                ))
            }
            Err(_) => None,
        },
    };
    let replace = w.replace;
    let (plan, activated) = apply(old.clone(), w, by, crate::now_ms())?;
    if plan.items.is_empty() && plan.title.is_empty() && (old.is_none() || replace) {
        return Err("a new plan needs a title and its items".into());
    }
    let archived = match (&old_text, replace) {
        (Some(text), true) => {
            let title = old.as_ref().map_or("plan", |p| p.title.as_str());
            Some(archive(vault, &slug, text, title)?)
        }
        _ => None,
    };
    vault
        .write(&path, &render(&plan))
        .map_err(|e| e.to_string())?;
    drop(one);
    changed(&slug);
    Ok(Written {
        plan,
        activated,
        archived,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, needs: &[&str]) -> ItemWrite {
        ItemWrite {
            id: id.into(),
            title: Some(format!("Item {id}")),
            needs: Some(needs.iter().map(|s| (*s).to_string()).collect()),
            ..ItemWrite::default()
        }
    }

    fn sample() -> Plan {
        let w = PlanWrite {
            title: Some("0.1.28".into()),
            goal: Some("A calmer sidebar driven by plans".into()),
            phases: Some(vec!["Now".into(), "Next".into()]),
            items: Some(vec![
                ItemWrite {
                    id: "names".into(),
                    title: Some("Session names".into()),
                    detail: Some("Name sessions in 3 to 5 words".into()),
                    phase: Some("Now".into()),
                    state: Some("active".into()),
                    branch: Some("plan/names".into()),
                    ..ItemWrite::default()
                },
                ItemWrite {
                    phase: Some("Now".into()),
                    ..item("panel", &["names"])
                },
                ItemWrite {
                    phase: Some("Next".into()),
                    state: Some("done".into()),
                    ..item("viz", &[])
                },
            ]),
            replace: false,
        };
        apply(None, w, "s1", 1_000).unwrap().0
    }

    #[test]
    fn a_plan_reads_back_as_it_was_written() {
        let p = sample();
        let note = render(&p);
        assert!(
            note.starts_with("---\ntitle: \"Plan: 0.1.28\"\nkind: plan\n"),
            "{note}"
        );
        assert!(note.contains("1 of 3 done."), "{note}");
        assert!(note.contains("- [>] Session names (`names`)"), "{note}");
        assert!(
            note.contains("- [ ] Item panel (`panel`), after names"),
            "{note}"
        );
        assert_eq!(parse(&note).unwrap(), p);
        assert_eq!(render(&parse(&note).unwrap()), note, "render is stable");
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["items"][0]["state"], "active");
        assert_eq!(v["items"][1]["needs"], serde_json::json!(["names"]));
        assert_eq!(
            (v["updated"].as_u64(), v["by"].as_str()),
            (Some(1_000), Some("s1"))
        );
    }

    #[test]
    fn a_broken_block_is_an_error_and_a_rough_one_is_mended() {
        assert!(parse("# just a note")
            .unwrap_err()
            .contains("no ```parzi-plan"));
        let broken = "x\n```parzi-plan\n{\"title\": \n```\n";
        assert!(parse(broken).unwrap_err().contains("not valid JSON"));
        assert!(parse("```parzi-plan\n[1]\n```").is_err());
        let rough = r#"```parzi-plan
{"title": "T", "items": [
  {"id": "a", "state": "weird", "needs": ["b", "ghost", "a"], "extra": 1},
  {"id": "b", "title": "B", "needs": ["a"], "phase": "Later"},
  {"id": "Bad Id!"},
  {"id": "a", "title": "again"}
]}
```"#;
        let p = parse(rough).unwrap();
        assert_eq!(p.items.len(), 2, "the bad and the repeated item go");
        assert_eq!(p.items[0].title, "a", "a missing title is its id");
        assert_eq!(p.items[0].state, PlanState::Planned);
        assert_eq!(p.phases, ["Later"], "an unknown phase joins the phases");
        assert!(validate(&p.items).is_ok(), "the circle is cut");
        assert_eq!(
            p.items[0].needs.len() + p.items[1].needs.len(),
            1,
            "one side of the circle stays"
        );
    }

    #[test]
    fn writes_merge_by_id_and_change_only_what_they_name() {
        let p = sample();
        let (p2, activated) = apply(
            Some(p.clone()),
            PlanWrite {
                items: Some(vec![
                    ItemWrite {
                        id: "panel".into(),
                        state: Some("active".into()),
                        ..ItemWrite::default()
                    },
                    item("tests", &["panel"]),
                ]),
                ..PlanWrite::default()
            },
            "s2",
            2_000,
        )
        .unwrap();
        assert_eq!(activated, ["panel"]);
        assert_eq!(p2.title, "0.1.28", "untouched fields stay");
        assert_eq!(p2.items.len(), 4, "a new id is added at the end");
        assert_eq!(p2.items[3].id, "tests");
        let panel = p2.item("panel").unwrap();
        assert_eq!(panel.state, PlanState::Active);
        assert_eq!(panel.title, "Item panel", "a partial item keeps the rest");
        assert_eq!(panel.needs, ["names"]);
        assert_eq!(p2.by, "s2");
        let (p3, _) = apply(
            Some(p2),
            PlanWrite {
                items: Some(vec![ItemWrite {
                    id: "panel".into(),
                    remove: true,
                    ..ItemWrite::default()
                }]),
                ..PlanWrite::default()
            },
            "s2",
            3_000,
        )
        .unwrap();
        assert!(p3.item("panel").is_none());
        assert!(
            p3.item("tests").unwrap().needs.is_empty(),
            "needs on it go too"
        );
    }

    #[test]
    fn needs_must_name_items_and_never_circle() {
        let w = |items| PlanWrite {
            title: Some("T".into()),
            items: Some(items),
            ..PlanWrite::default()
        };
        let unknown = apply(None, w(vec![item("a", &["ghost"])]), "s", 1).unwrap_err();
        assert!(unknown.contains("not an item of this plan"), "{unknown}");
        let circle =
            apply(None, w(vec![item("a", &["b"]), item("b", &["a"])]), "s", 1).unwrap_err();
        assert!(circle.contains("circle"), "{circle}");
        let own = apply(None, w(vec![item("a", &["a"])]), "s", 1).unwrap_err();
        assert!(own.contains("circle"), "{own}");
        let bad = apply(None, w(vec![item("Has Space", &[])]), "s", 1).unwrap_err();
        assert!(bad.contains("a-z, 0-9 and -"), "{bad}");
        let untitled = apply(
            None,
            w(vec![ItemWrite {
                id: "x".into(),
                ..ItemWrite::default()
            }]),
            "s",
            1,
        )
        .unwrap_err();
        assert!(untitled.contains("needs a title"), "{untitled}");
        let state = apply(
            None,
            w(vec![ItemWrite {
                state: Some("maybe".into()),
                ..item("x", &[])
            }]),
            "s",
            1,
        )
        .unwrap_err();
        assert!(
            state.contains("not planned, active, done or dropped"),
            "{state}"
        );
        assert!(apply(None, w(vec![item("a", &[]), item("b", &["a"])]), "s", 1).is_ok());
    }

    #[test]
    fn the_caps_hold() {
        let many: Vec<ItemWrite> = (0..41).map(|i| item(&format!("i{i}"), &[])).collect();
        let err = apply(
            None,
            PlanWrite {
                items: Some(many),
                ..PlanWrite::default()
            },
            "s",
            1,
        )
        .unwrap_err();
        assert!(err.contains("at most 40 items"), "{err}");
        for (field, cap) in [("title", 80), ("detail", 400), ("note", 400)] {
            let mut it = item("a", &[]);
            let long = Some("x".repeat(cap + 1));
            match field {
                "title" => it.title = long,
                "detail" => it.detail = long,
                _ => it.note = long,
            }
            let err = apply(
                None,
                PlanWrite {
                    items: Some(vec![it]),
                    ..PlanWrite::default()
                },
                "s",
                1,
            )
            .unwrap_err();
            assert!(err.contains(&format!("keep it to {cap}")), "{field}: {err}");
        }
    }

    #[test]
    fn the_summary_is_compact_and_names_the_open_items() {
        let s = summary(&sample());
        assert!(
            s.starts_with("Plan \"0.1.28\": A calmer sidebar driven by plans. 1 of 3 done."),
            "{s}"
        );
        assert!(
            s.contains(
                "- names (active; plan/names): Session names. Name sessions in 3 to 5 words"
            ),
            "{s}"
        );
        assert!(
            s.contains("- panel (planned; after names): Item panel"),
            "{s}"
        );
        assert!(s.contains("Done: viz."), "{s}");
        let many: Vec<ItemWrite> = (0..40)
            .map(|i| ItemWrite {
                title: Some(format!("A rather long item title, number {i} of the plan")),
                detail: Some("d".repeat(300)),
                ..item(&format!("item-{i}"), &[])
            })
            .collect();
        let (big, _) = apply(
            None,
            PlanWrite {
                title: Some("Big".into()),
                items: Some(many),
                ..PlanWrite::default()
            },
            "s",
            1,
        )
        .unwrap();
        let s = summary(&big);
        assert!(s.chars().count() <= SUMMARY_CAP, "{}", s.chars().count());
        assert!(s.contains("more open items"), "{s}");
        assert!(s.ends_with("Read it in full with plan.read."), "{s}");
    }

    #[test]
    fn a_quote_reads_as_the_desk_sends_it() {
        let p = sample();
        let q = quote_block(
            "parzi",
            p.item("names").unwrap(),
            Some("PR #42 open, checks pass."),
        );
        assert_eq!(
            q,
            "<plan-item project=\"parzi\" id=\"names\" title=\"Session names\">\nName sessions in 3 to 5 words\nState: active. Needs: none. Branch: plan/names. PR #42 open, checks pass.\n</plan-item>"
        );
        let mut odd = p.item("panel").unwrap().clone();
        odd.title = "Say \"hi\"".into();
        let q = quote_block("parzi", &odd, None);
        assert!(q.contains("title=\"Say &quot;hi&quot;\""), "{q}");
        assert!(
            q.ends_with("State: planned. Needs: names.\n</plan-item>"),
            "{q}"
        );
    }

    #[test]
    fn plan_notes_are_found_by_path() {
        assert_eq!(
            project_of_note("projects/parzi/plan.md").as_deref(),
            Some("parzi")
        );
        assert_eq!(
            project_of_note("projects/parzi/plans/2026-01-01-x.md"),
            None
        );
        assert_eq!(project_of_note("projects/parzi.md"), None);
        assert_eq!(project_of_note("notes/plan.md"), None);
        assert!(check_slug("default").is_err() && check_slug("").is_err());
        assert!(check_slug("all").is_err() && check_slug("a/b").is_err());
    }

    #[test]
    fn a_replaced_plan_moves_to_the_history() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::at(dir.path());
        let first = PlanWrite {
            title: Some("First pass".into()),
            items: Some(vec![item("a", &[])]),
            ..PlanWrite::default()
        };
        let w = write(&v, "demo", first, "s1").unwrap();
        assert_eq!(w.archived, None);
        let view = load(&v, "demo");
        assert_eq!(view.path, "projects/demo/plan.md");
        assert_eq!(view.plan.as_ref().unwrap().title, "First pass");
        assert_eq!(view.error, None);
        let second = PlanWrite {
            title: Some("Second pass".into()),
            items: Some(vec![item("b", &[])]),
            replace: true,
            ..PlanWrite::default()
        };
        let w = write(&v, "demo", second, "s2").unwrap();
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let moved = format!("projects/demo/plans/{date}-first-pass.md");
        assert_eq!(w.archived.as_deref(), Some(moved.as_str()));
        let old = parse(&v.read(&moved).unwrap()).unwrap();
        assert_eq!(old.title, "First pass");
        let now = load(&v, "demo").plan.unwrap();
        assert_eq!(now.title, "Second pass");
        assert_eq!(now.items.len(), 1, "a new plan starts from nothing");
        assert_eq!(now.items[0].id, "b");
        let again = PlanWrite {
            title: Some("First pass".into()),
            replace: true,
            items: Some(vec![item("c", &[])]),
            ..PlanWrite::default()
        };
        write(&v, "demo", again, "s3").unwrap();
        let w = write(
            &v,
            "demo",
            PlanWrite {
                title: Some("Fourth".into()),
                items: Some(vec![item("d", &[])]),
                replace: true,
                ..PlanWrite::default()
            },
            "s4",
        )
        .unwrap();
        assert_eq!(
            w.archived.as_deref(),
            Some(format!("projects/demo/plans/{date}-first-pass-2.md").as_str()),
            "a taken name is numbered"
        );
    }

    #[test]
    fn a_broken_note_is_only_replaced_never_merged_into() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::at(dir.path());
        std::fs::create_dir_all(dir.path().join("projects/demo")).unwrap();
        v.write("projects/demo/plan.md", "```parzi-plan\n{oops\n```\n")
            .unwrap();
        let view = load(&v, "demo");
        assert!(view.plan.is_none());
        assert!(view.error.unwrap().contains("not valid JSON"));
        let merge = PlanWrite {
            items: Some(vec![item("a", &[])]),
            ..PlanWrite::default()
        };
        let err = write(&v, "demo", merge, "s").unwrap_err();
        assert!(err.contains("replace: true"), "{err}");
        let fresh = PlanWrite {
            title: Some("Fresh".into()),
            items: Some(vec![item("a", &[])]),
            replace: true,
            ..PlanWrite::default()
        };
        let w = write(&v, "demo", fresh, "s").unwrap();
        assert!(w.archived.unwrap().ends_with("-plan.md"));
        assert_eq!(load(&v, "demo").plan.unwrap().title, "Fresh");
        assert!(load(&v, "nothing-here").plan.is_none());
        assert!(load(&v, "nothing-here").error.is_none());
    }

    #[test]
    fn a_bare_replace_keeps_the_plan_it_would_blank() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::at(dir.path());
        let first = PlanWrite {
            title: Some("Kept".into()),
            items: Some(vec![item("a", &[])]),
            ..PlanWrite::default()
        };
        write(&v, "demo", first, "s1").unwrap();
        let bare = PlanWrite {
            replace: true,
            ..PlanWrite::default()
        };
        let err = write(&v, "demo", bare, "s2").unwrap_err();
        assert!(err.contains("needs a title"), "{err}");
        assert_eq!(load(&v, "demo").plan.unwrap().title, "Kept");
        let history = dir.path().join("projects/demo/plans");
        assert!(!history.exists(), "nothing was archived");
    }

    #[test]
    fn a_long_item_phase_is_refused_like_a_plan_phase() {
        let long = "p".repeat(PHASE_CAP + 1);
        let w = PlanWrite {
            title: Some("T".into()),
            items: Some(vec![ItemWrite {
                phase: Some(long.clone()),
                ..item("a", &[])
            }]),
            ..PlanWrite::default()
        };
        let err = apply(None, w, "s", 1).unwrap_err();
        assert!(err.contains(&format!("keep it to {PHASE_CAP}")), "{err}");
        let at_cap = "q".repeat(PHASE_CAP);
        let mut plan = None;
        for n in 0..3 {
            let w = PlanWrite {
                title: Some("T".into()),
                items: Some(vec![ItemWrite {
                    phase: Some(at_cap.clone()),
                    ..item("a", &[])
                }]),
                ..PlanWrite::default()
            };
            plan = Some(apply(plan, w, "s", n).unwrap().0);
        }
        assert_eq!(plan.unwrap().phases, [at_cap]);
    }

    #[test]
    fn plan_write_names_what_it_does_not_know() {
        let old: PlanWrite = serde_json::from_value(serde_json::json!({
            "items": [{"id": "a", "status": "done"}]
        }))
        .unwrap();
        let items = old.items.unwrap();
        assert_eq!(
            items[0].state.as_deref(),
            Some("done"),
            "the old name works"
        );
        let err = serde_json::from_value::<PlanWrite>(serde_json::json!({
            "items": [{"id": "a", "progress": "done"}]
        }))
        .unwrap_err()
        .to_string();
        assert!(err.contains("unknown field `progress`"), "{err}");
        let err = serde_json::from_value::<PlanWrite>(serde_json::json!({"project": "x"}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown field `project`"), "{err}");
    }

    #[test]
    fn plan_notes_take_the_write_lock() {
        assert!(lock_if_plan("projects/demo/plan.md").is_some());
        assert!(lock_if_plan("notes/a.md").is_none());
    }
}
