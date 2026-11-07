use parzi_core::brain::Vault;
use parzi_core::plan::{self, Plan, PlanWrite};
use parzi_core::store::{SessionMeta, SessionStore};
use parzi_core::text::{fold, short_id, NAME_CHARS, NAME_WORDS};
use serde_json::Value;

pub(crate) fn execute(store: &SessionStore, sid: &str, name: &str, args: &Value) -> (bool, String) {
    let out = match name {
        "plan.read" => read(store, sid, args),
        "plan.write" => write(store, sid, args),
        "session.rename" => rename(store, sid, args),
        _ => Err(format!("unknown tool `{name}`")),
    };
    match out {
        Ok(text) => (true, text),
        Err(e) => (false, e),
    }
}

fn str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn vault() -> Result<Vault, String> {
    Vault::open().map_err(|e| e.to_string())
}

fn rename(store: &SessionStore, sid: &str, args: &Value) -> Result<String, String> {
    let title = str_arg(args, "title")
        .map(|t| fold(&t))
        .ok_or("session.rename needs a `title`")?;
    let (len, words) = (title.chars().count(), title.split(' ').count());
    if len > NAME_CHARS || words > NAME_WORDS {
        return Err(format!(
            "that name is {len} chars in {words} words; keep it to {NAME_CHARS} chars, 2 to {NAME_WORDS} words"
        ));
    }
    store.set_title(sid, &title).map_err(|e| e.to_string())?;
    Ok(format!("This session is now called \"{title}\"."))
}

/// The project a write goes to and what it says. Agents often name the
/// project the way plan.read takes it; that is fine when it is this
/// session's own, the only plan a session writes.
fn parse_write(
    store: &SessionStore,
    sid: &str,
    args: &Value,
) -> Result<(String, PlanWrite), String> {
    let meta = store.get(sid).map_err(|e| e.to_string())?;
    let slug = plan::check_slug(&meta.project)?.to_string();
    let mut args = args.clone();
    if let Some(named) = args.as_object_mut().and_then(|o| o.remove("project")) {
        let named = named.as_str().map(str::trim).unwrap_or_default();
        if !named.is_empty() && !plan::check_slug(named).is_ok_and(|n| n == slug) {
            return Err(format!(
                "plan.write changes this session's project, {slug}. To plan {named}, move there with project.switch first"
            ));
        }
    }
    let w: PlanWrite = serde_json::from_value(args).map_err(|e| format!("plan.write: {e}"))?;
    Ok((slug, w))
}

/// What a plan.write would change, for the person to approve before it is
/// saved. The same checks as the write, so a bad write fails before anyone
/// is asked.
pub(crate) fn proposal(
    store: &SessionStore,
    sid: &str,
    args: &Value,
) -> Result<plan::Proposal, String> {
    let (slug, w) = parse_write(store, sid, args)?;
    plan::preview(&vault()?, &slug, &w, sid)
}

/// The two answers offered under a plan proposal. Anything else the person
/// types is what they want changed.
pub(crate) const USE_PLAN: &str = "Use this plan";
pub(crate) const NOT_NOW: &str = "Not now";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Save it. `note` is what to tell the agent besides the saved result.
    Save(&'static str),
    /// Nothing is saved; the text goes back to the agent.
    Hold(String),
}

pub(crate) fn verdict(answer: &str) -> Verdict {
    let a = answer.trim();
    if a == USE_PLAN {
        return Verdict::Save("The person approved this plan.");
    }
    if a == NOT_NOW {
        return Verdict::Hold(
            "The person said not now, so nothing was saved. Carry on without changing the plan, and do not propose it again unless they ask.".into(),
        );
    }
    let lower = a.to_ascii_lowercase();
    if lower.starts_with("skipped") {
        return Verdict::Save("The person skipped the review, so it is saved as you wrote it.");
    }
    if a.is_empty() || lower.starts_with("the user didn't answer") {
        return Verdict::Hold(
            "Nobody answered, so nothing was saved. Say in your reply what plan you would propose; the person can ask you to save it.".into(),
        );
    }
    Verdict::Hold(format!(
        "The person wants changes before this plan is saved: \"{a}\". Nothing was saved. Change it and call plan.write again."
    ))
}

fn write(store: &SessionStore, sid: &str, args: &Value) -> Result<String, String> {
    let (slug, w) = parse_write(store, sid, args)?;
    let done = plan::write(&vault()?, &slug, w, sid)?;
    let (finished, counted) = done.plan.progress();
    let mut out = format!(
        "Saved {} ({} items, {finished} of {counted} done).",
        plan::note_path(&slug),
        done.plan.items.len()
    );
    if let Some(old) = &done.archived {
        out.push_str(&format!(" The old plan is kept at {old}."));
    }
    if let Some(item) = done.activated.first() {
        store
            .set_plan_item(sid, Some(item))
            .map_err(|e| e.to_string())?;
        out.push_str(&format!(" This session now works on `{item}`."));
    }
    Ok(out)
}

fn item_of<'a>(m: &'a SessionMeta, all: &'a [SessionMeta]) -> Option<&'a str> {
    m.plan_item.as_deref().or_else(|| {
        let parent = m.parent_id.as_deref()?;
        all.iter()
            .find(|p| p.id == parent)
            .and_then(|p| p.plan_item.as_deref())
    })
}

fn shared_branch(b: &str) -> bool {
    matches!(b, "" | "main" | "master" | "HEAD" | "develop" | "trunk")
}

fn when(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map_or_else(String::new, |t| t.format("%Y-%m-%d %H:%M UTC").to_string())
}

fn listing(slug: &str, path: &str, p: &Plan, sessions: &[SessionMeta]) -> String {
    let (done, counted) = p.progress();
    let by = match p.by.as_str() {
        "" => String::new(),
        "you" => " by you".into(),
        s => format!(" by session {}", short_id(s)),
    };
    let mut out = vec![format!(
        "Plan \"{}\" of project {slug} ({path}), changed {}{by}. {done} of {counted} done.",
        p.title,
        when(p.updated)
    )];
    if !p.goal.is_empty() {
        out.push(format!("Goal: {}", p.goal));
    }
    if !p.phases.is_empty() {
        out.push(format!("Phases: {}.", p.phases.join(", ")));
    }
    for (phase, items) in plan::grouped(p) {
        if items.is_empty() {
            continue;
        }
        if !phase.is_empty() {
            out.push(format!("\n{phase}"));
        }
        for i in items {
            listed(&mut out, i, sessions);
        }
    }
    out.join("\n")
}

fn listed(out: &mut Vec<String>, i: &plan::PlanItem, sessions: &[SessionMeta]) {
    let mut head = format!("- {}: {} [{}]", i.id, i.title, i.state.word());
    if !i.needs.is_empty() {
        head.push_str(&format!(", after {}", i.needs.join(", ")));
    }
    let linked: Vec<&SessionMeta> = sessions
        .iter()
        .filter(|m| item_of(m, sessions) == Some(i.id.as_str()))
        .collect();
    let mut branches: Vec<String> = vec![];
    for b in std::iter::once(i.branch.as_str()).chain(linked.iter().map(|m| m.branch.as_str())) {
        if !shared_branch(b) && !branches.iter().any(|x| x == b) {
            branches.push(b.to_string());
        }
    }
    if !branches.is_empty() {
        head.push_str(&format!(", branch {}", branches.join(", ")));
    }
    out.push(head);
    if !i.detail.is_empty() {
        out.push(format!("  {}", i.detail));
    }
    if !i.note.is_empty() {
        out.push(format!("  Note: {}", i.note));
    }
    if !linked.is_empty() {
        let rows: Vec<String> = linked
            .iter()
            .take(6)
            .map(|m| {
                let mut s = format!("{} \"{}\" ({}", short_id(&m.id), m.title, m.status.word());
                if !shared_branch(&m.branch) {
                    s.push_str(&format!(", on {}", m.branch));
                }
                if let Some(r) = &m.last_run {
                    s.push_str(&format!(", last run {}", r.end.word()));
                }
                s.push(')');
                s
            })
            .collect();
        let more = linked.len().saturating_sub(rows.len());
        let mut line = format!("  Sessions: {}", rows.join("; "));
        if more > 0 {
            line.push_str(&format!("; and {more} more"));
        }
        out.push(line);
    }
}

fn read(store: &SessionStore, sid: &str, args: &Value) -> Result<String, String> {
    let project = match str_arg(args, "project") {
        Some(p) => p,
        None => store.get(sid).map_err(|e| e.to_string())?.project,
    };
    let slug = plan::check_slug(&project)?.to_string();
    let view = plan::load(&vault()?, &slug);
    if let Some(e) = view.error {
        return Err(format!(
            "the plan note {} cannot be read: {e}. Fix it with brain.write, or start over with plan.write and replace: true",
            view.path
        ));
    }
    let Some(p) = view.plan else {
        return Ok(format!(
            "Project {slug} has no plan yet. When the work spans sessions, propose one with plan.write."
        ));
    };
    let sessions: Vec<SessionMeta> = store
        .list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|m| m.project == slug)
        .collect();
    Ok(listing(&slug, &view.path, &p, &sessions))
}

pub(crate) fn brief_for(project: &str, item: Option<&str>) -> Option<String> {
    let slug = plan::check_slug(project).ok()?.to_string();
    let p = plan::load(&Vault::open().ok()?, &slug).plan?;
    let mut text = plan::summary(&p);
    if let Some(i) = item.filter(|i| p.item(i).is_some()) {
        text.push_str(&format!("\nThis session works on item {i}."));
    }
    Some(format!(
        "<parzi:plan project=\"{slug}\">\n{text}\n</parzi:plan>"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use parzi_core::store::SessionStatus;

    fn meta(id: &str, item: Option<&str>, parent: Option<&str>, branch: &str) -> SessionMeta {
        let now = chrono::Utc::now();
        SessionMeta {
            id: id.into(),
            title: format!("Session {id}"),
            pinned: false,
            project: "demo".into(),
            model: String::new(),
            parent_id: parent.map(str::to_string),
            status: SessionStatus::Idle,
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
            context_tokens: 0,
            context_limit: 0,
            cwd: String::new(),
            last_run: None,
            plan_item: item.map(str::to_string),
            branch: branch.into(),
            effort: String::new(),
            created: now,
            updated: now,
        }
    }

    #[test]
    fn the_persons_answer_decides_whether_the_plan_is_saved() {
        assert!(matches!(verdict(USE_PLAN), Verdict::Save(_)));
        assert!(matches!(
            verdict("Skipped: decide yourself and say what you assumed."),
            Verdict::Save(_)
        ));
        let Verdict::Hold(why) = verdict(NOT_NOW) else {
            panic!("not now holds")
        };
        assert!(why.contains("nothing was saved"), "{why}");
        let Verdict::Hold(why) =
            verdict("the user didn't answer — decide yourself and say what you assumed")
        else {
            panic!("no answer holds")
        };
        assert!(why.starts_with("Nobody answered"), "{why}");
        let Verdict::Hold(why) = verdict("Drop the panel, ship names first") else {
            panic!("a typed answer holds")
        };
        assert!(
            why.contains("\"Drop the panel, ship names first\"")
                && why.contains("call plan.write again"),
            "{why}"
        );
    }

    #[test]
    fn the_listing_names_each_items_sessions_and_branches() {
        let p = plan::apply(
            None,
            serde_json::from_value(serde_json::json!({
                "title": "Release",
                "phases": ["Now", "Next"],
                "items": [
                    {"id": "names", "title": "Session names", "phase": "Now", "state": "active", "branch": "plan/names"},
                    {"id": "panel", "title": "Plan panel", "phase": "Next", "needs": ["names"], "detail": "The right panel"},
                ],
            }))
            .unwrap(),
            "11111111-aaaa",
            0,
        )
        .unwrap()
        .0;
        let sessions = vec![
            meta("aaaaaaaa-1", Some("names"), None, "plan/names"),
            meta("bbbbbbbb-2", None, Some("aaaaaaaa-1"), "main"),
            meta("cccccccc-3", Some("panel"), None, "plan/panel-x"),
            meta("dddddddd-4", None, None, "plan/other"),
        ];
        let text = listing("demo", "projects/demo/plan.md", &p, &sessions);
        assert!(
            text.starts_with("Plan \"Release\" of project demo (projects/demo/plan.md)"),
            "{text}"
        );
        assert!(text.contains("by session 11111111"), "{text}");
        assert!(
            text.contains("\nNow\n- names: Session names [active], branch plan/names\n"),
            "{text}"
        );
        assert!(
            text.contains("Sessions: aaaaaaaa \"Session aaaaaaaa-1\" (idle, on plan/names); bbbbbbbb \"Session bbbbbbbb-2\" (idle)"),
            "a child counts for its parent's item: {text}"
        );
        assert!(
            text.contains("- panel: Plan panel [planned], after names, branch plan/panel-x\n  The right panel"),
            "{text}"
        );
        assert!(!text.contains("dddddddd"), "{text}");
    }
}
