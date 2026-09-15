//! Sidebar + workspace navigation (Phase 3). Structure parity with
//! `Sidebar.svelte` / `ThreadRow.svelte`; styling is NOT ported. Pure logic
//! over [`ThreadView`] plus thin `egui` views returning [`SidebarAction`].
//! Provider marks are simplified glyphs (initial + brand colour): this
//! repo's `egui_extras` enables only `image`, not `svg`, so the SVG paths
//! from `providerMarks.ts` cannot render yet.

use egui::{Color32, RichText, Ui};
use parzi_core::store::{SessionMeta, SessionStatus};

use crate::theme::Tokens;

pub const DAY_SECS: i64 = 86_400;
pub const INDENT_PX: f32 = 14.0;
const MAX_FOLLOW: u8 = 6;
pub const MAX_DEPTH: usize = 2;

/// Time bucket; labels match the Svelte group headers verbatim.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bucket {
    Today,
    Yesterday,
    Prev7,
    Older,
}

impl Bucket {
    pub const ALL: [Bucket; 4] = [Self::Today, Self::Yesterday, Self::Prev7, Self::Older];
    pub fn label(self) -> &'static str {
        match self {
            Bucket::Today => "Today",
            Bucket::Yesterday => "Yesterday",
            Bucket::Prev7 => "Previous 7 days",
            Bucket::Older => "Older",
        }
    }
}

/// Bucket for `updated_ts` given `today_start_ts` (local midnight, unix
/// secs). Future rows land in Today; boundaries are calendar days.
pub fn bucket_for(updated_ts: i64, today_start_ts: i64) -> Bucket {
    let diff = ((today_start_ts - updated_ts - 1).div_euclid(DAY_SECS) + 1).max(0);
    match diff {
        0 => Bucket::Today,
        1 => Bucket::Yesterday,
        2..=7 => Bucket::Prev7,
        _ => Bucket::Older,
    }
}

/// `provider/model` -> `provider`; `auto` stays `auto`. Mirrors `provOf`.
pub fn provider_of(model: &str) -> &str {
    match model {
        "auto" => "auto",
        m => m.split('/').next().unwrap_or(""),
    }
}

/// Simplified glyph: provider initial + brand colour from `providerMarks.ts`.
pub fn provider_glyph(provider: &str) -> (char, Color32) {
    const GREY: Color32 = Color32::from_rgb(0x9A, 0x9D, 0xA8);
    let color = match provider {
        "claude" | "anthropic" | "claude-code" => Color32::from_rgb(0xD9, 0x77, 0x57),
        "gemini" => Color32::from_rgb(0x31, 0x86, 0xFF),
        "google" => Color32::from_rgb(0x42, 0x85, 0xF4),
        "meta" | "llama" => Color32::from_rgb(0x00, 0x82, 0xFB),
        "mistral" => Color32::from_rgb(0xE1, 0x05, 0x00),
        "auto" => Color32::from_rgb(0xE8, 0xB4, 0x40),
        _ => GREY,
    };
    let init = provider.chars().next().map(|c| c.to_ascii_uppercase());
    (init.unwrap_or('\u{25A1}'), color)
}

/// Minimal row data, so the pure logic never imports a clock.
pub struct ThreadView<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub pinned: bool,
    pub project: &'a str,
    pub parent_id: Option<&'a str>,
    pub updated_ts: i64,
    pub created_ts: i64,
    pub status: SessionStatus,
    pub model: &'a str,
    pub lane: &'a str,
}

/// Adapt a store row. Empty projects read as `default`.
pub fn view_of(m: &SessionMeta) -> ThreadView<'_> {
    ThreadView {
        id: &m.id,
        title: &m.title,
        pinned: m.pinned,
        project: Some(m.project.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("default"),
        parent_id: m.parent_id.as_deref(),
        updated_ts: m.updated.timestamp(),
        created_ts: m.created.timestamp(),
        status: m.status,
        model: &m.model,
        lane: &m.lane,
    }
}

/// Dedupe workspace names + thread projects; current first, `default` last,
/// rest alphabetical. Empty input yields `["default"]`.
pub fn workspace_names(projects: &[String], threads: &[ThreadView], cur: &str) -> Vec<String> {
    let mut out: Vec<String> = projects
        .iter()
        .filter(|p| !p.is_empty())
        .cloned()
        .chain(threads.iter().map(|t| t.project.to_owned()))
        .collect();
    out.sort();
    out.dedup();
    if out.is_empty() {
        out.push("default".to_owned());
    }
    out.sort_by_key(|a| (a != cur, a == "default", a.clone()));
    out
}

pub fn count_for(threads: &[ThreadView], name: &str) -> usize {
    threads.iter().filter(|t| t.project == name).count()
}

pub fn pinned_threads<'a>(threads: &'a [ThreadView<'a>]) -> Vec<&'a ThreadView<'a>> {
    let mut out: Vec<&ThreadView> = threads.iter().filter(|t| t.pinned).collect();
    out.sort_by_key(|t| std::cmp::Reverse(t.updated_ts));
    out
}

/// Unpinned threads per bucket (index with `Bucket as usize`), newest first.
pub fn bucketed<'a>(
    threads: &'a [ThreadView<'a>],
    today_start_ts: i64,
) -> [(Bucket, Vec<&'a ThreadView<'a>>); 4] {
    let mut out = Bucket::ALL.map(|b| (b, Vec::new()));
    let mut rest: Vec<&ThreadView> = threads.iter().filter(|t| !t.pinned).collect();
    rest.sort_by_key(|t| std::cmp::Reverse(t.updated_ts));
    for t in rest {
        out[bucket_for(t.updated_ts, today_start_ts) as usize]
            .1
            .push(t);
    }
    out
}

/// Visual indent level, following `parent_id` with the `depthOf` cycle guard.
pub fn depth_of(threads: &[ThreadView], id: &str) -> usize {
    let par = |id: &str| {
        threads
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.parent_id)
    };
    let (mut depth, mut guard) = (0, 0u8);
    let mut parent = par(id);
    while let Some(pid) = parent {
        if guard >= MAX_FOLLOW {
            break;
        }
        guard += 1;
        depth += 1;
        parent = par(pid);
    }
    depth.min(MAX_DEPTH)
}

/// Compact age (`now`, `5m`, `3h`, `2d`). Live rows prefix it with `working`.
pub fn relative_age(ts: i64, now_ts: i64) -> String {
    match (now_ts - ts).max(0) {
        0..60 => "now".to_owned(),
        60..3600 => format!("{}m", (now_ts - ts) / 60),
        3600..DAY_SECS => format!("{}h", (now_ts - ts) / 3600),
        _ => format!("{}d", (now_ts - ts) / DAY_SECS),
    }
}

/// What the sidebar wants `app.rs` to do. Pin/Rename/Delete/Fork are the
/// context-menu subset; the rest map the Svelte `dispatch(...)` clicks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarAction {
    SelectThread { id: String },
    NewThread,
    SelectProject { name: String },
    Pin { id: String, pinned: bool },
    Rename { id: String, title: String },
    DeleteThread { id: String },
    Fork { id: String },
    OpenPalette,
    OpenSettings,
    ToggleSidebar,
}

/// Pending menu; rendered by the future app.rs wiring (follow-up).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextMenu {
    pub id: String,
    pub title: String,
    pub confirm_delete: bool,
    pub pos: egui::Pos2,
}

#[derive(Clone, Debug, Default)]
pub struct SidebarState {
    pub workspaces_open: bool,
    pub current_project: String,
    pub active_thread: Option<String>,
    pub renaming_id: Option<String>,
    pub rename_draft: String,
    pub menu: Option<ContextMenu>,
}

impl SidebarState {
    pub fn new() -> Self {
        let mut s = Self::default();
        s.workspaces_open = true;
        s.current_project = "default".to_owned();
        s
    }
    pub fn open_thread_menu(&mut self, id: &str, title: &str, pos: egui::Pos2) {
        let menu = ContextMenu {
            id: id.to_owned(),
            title: title.to_owned(),
            pos,
            ..Default::default()
        };
        self.menu = Some(menu);
    }
    pub fn begin_rename(&mut self, id: &str, title: &str) {
        self.renaming_id = Some(id.to_owned());
        self.rename_draft = title
            .is_empty()
            .then_some("untitled")
            .unwrap_or(title)
            .to_owned();
        self.menu = None;
    }
    /// Trimmed draft; `None` when empty or when nothing is being renamed.
    pub fn commit_rename(&mut self) -> Option<SidebarAction> {
        let id = self.renaming_id.take()?;
        let title = self.rename_draft.trim().to_owned();
        (!title.is_empty()).then(|| SidebarAction::Rename { id, title })
    }
    pub fn cancel_rename(&mut self) {
        self.renaming_id = None;
    }
}

/// Render the sidebar; at most one action per frame for `app.rs`.
pub fn show(
    ui: &mut Ui,
    state: &mut SidebarState,
    threads: &[SessionMeta],
    projects: &[String],
    now_ts: i64,
    today_start_ts: i64,
    t: &Tokens,
) -> Option<SidebarAction> {
    use SidebarAction::*;
    let mut action: Option<SidebarAction> = None;
    let views: Vec<ThreadView> = threads.iter().map(view_of).collect();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Parzi").strong().color(t.text));
        if ui.small_button("Search").clicked() {
            action = Some(OpenPalette);
        }
        if ui.small_button("+ New").clicked() {
            action = Some(NewThread);
        }
    });
    ui.separator();
    let names = workspace_names(projects, &views, &state.current_project);
    ui.label(RichText::new(format!("Workspaces ({})", names.len())).small());
    for name in &names {
        let seen = ["Inbox", name.as_str()][usize::from(name.as_str() != "default")];
        let n = count_for(&views, name);
        let label = if n > 0 {
            format!("{seen} ({n})")
        } else {
            seen.to_owned()
        };
        let selected = *name == state.current_project;
        let r = ui.selectable_label(selected, RichText::new(label).color(t.text));
        if r.clicked() {
            action = Some(SelectProject { name: name.clone() });
        }
    }
    ui.separator();
    egui::ScrollArea::vertical().show(ui, |ui| {
        let pinned = pinned_threads(&views);
        if !pinned.is_empty() {
            ui.label(RichText::new("\u{2605} Pinned").small().color(t.text_dim));
            for v in pinned {
                if action.is_none() {
                    action = thread_row(ui, state, v, 0, now_ts, t);
                }
            }
        }
        for (bucket, items) in bucketed(&views, today_start_ts) {
            if items.is_empty() {
                continue;
            }
            ui.label(RichText::new(bucket.label()).small().color(t.text_dim));
            for v in items {
                let d = depth_of(&views, v.id);
                if action.is_none() {
                    action = thread_row(ui, state, v, d, now_ts, t);
                }
            }
        }
    });
    action
}

fn thread_row(
    ui: &mut Ui,
    state: &mut SidebarState,
    v: &ThreadView,
    depth: usize,
    now_ts: i64,
    t: &Tokens,
) -> Option<SidebarAction> {
    use SidebarAction::*;
    let mut out = None;
    let indent = depth as f32 * INDENT_PX;
    let title = ["untitled", v.title][usize::from(!v.title.is_empty())];
    ui.horizontal(|ui| {
        ui.add_space(indent);
        let selected = state.active_thread.as_deref() == Some(v.id);
        let resp = ui.selectable_label(selected, RichText::new(title).color(t.text));
        if resp.clicked() {
            out = Some(SelectThread {
                id: v.id.to_owned(),
            });
        }
        if resp.secondary_clicked() {
            let pos = resp.interact_pointer_pos().unwrap_or_default();
            state.open_thread_menu(v.id, title, pos);
        }
    });
    ui.horizontal(|ui| {
        ui.add_space(indent + 2.0);
        let mut meta = match v.status {
            SessionStatus::Active => format!("working {}", relative_age(v.created_ts, now_ts)),
            SessionStatus::Queued => "Queued".to_owned(),
            _ => relative_age(v.updated_ts, now_ts),
        };
        if !v.lane.is_empty() {
            meta.push_str(&format!(" \u{B7} {}", v.lane));
        }
        ui.label(RichText::new(meta).small().color(t.text_dim));
        let (glyph, color) = provider_glyph(provider_of(v.model));
        ui.label(RichText::new(glyph.to_string()).small().color(color));
        if v.pinned {
            ui.label(RichText::new("\u{2605}").small().color(t.text_dim));
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row<'a>(id: &'a str, p: &'a str, par: Option<&'a str>, u: i64, pin: bool) -> ThreadView<'a> {
        ThreadView {
            id,
            title: "t",
            pinned: pin,
            project: p,
            parent_id: par,
            updated_ts: u,
            created_ts: u,
            status: SessionStatus::Idle,
            model: "x/y",
            lane: "",
        }
    }

    #[test]
    fn buckets_use_calendar_days() {
        let t0 = DAY_SECS * 20_000;
        for (ts, want) in [
            (t0, Bucket::Today),
            (t0 + DAY_SECS, Bucket::Today),
            (t0 - 1, Bucket::Yesterday),
            (t0 - DAY_SECS, Bucket::Yesterday),
            (t0 - 2 * DAY_SECS, Bucket::Prev7),
            (t0 - 7 * DAY_SECS, Bucket::Prev7),
            (t0 - 7 * DAY_SECS - 1, Bucket::Older),
        ] {
            assert_eq!(bucket_for(ts, t0), want);
        }
    }

    #[test]
    fn workspaces_order_counts_and_split() {
        let t0 = DAY_SECS * 20_000;
        let ts = [
            row("a", "beta", None, t0, false),
            row("b", "default", None, t0, false),
            row("pin", "default", None, t0 - 30 * DAY_SECS, true),
            row("old", "default", None, t0 - 30 * DAY_SECS, false),
        ];
        let names = workspace_names(&["alpha".to_owned(), "beta".to_owned()], &ts, "alpha");
        assert!(names == ["alpha", "beta", "default"]);
        assert!(workspace_names(&[], &[], "x") == ["default"]);
        assert!(count_for(&ts, "beta") == 1 && count_for(&ts, "missing") == 0);
        assert_eq!(pinned_threads(&ts).len(), 1);
        let g = bucketed(&ts, t0);
        assert!(g[0].1.len() == 2 && g[3].1.len() == 1);
        assert!(g[1].1.is_empty() && g[2].1.is_empty());
    }

    #[test]
    fn depth_caps_cycles() {
        let ts = [
            row("a", "d", None, 1, false),
            row("b", "d", Some("a"), 1, false),
            row("c", "d", Some("b"), 1, false),
            row("dd", "d", Some("c"), 1, false),
            row("x", "d", Some("y"), 1, false),
            row("y", "d", Some("x"), 1, false),
        ];
        let ds = [depth_of(&ts, "a"), depth_of(&ts, "b"), depth_of(&ts, "dd")];
        assert_eq!(ds, [0, 1, 2]);
        assert!(depth_of(&ts, "x") <= MAX_DEPTH);
    }

    #[test]
    fn providers_ages_rename_and_menu() {
        assert_eq!(provider_of("auto"), "auto");
        assert_eq!(provider_of("openai/gpt"), "openai");
        assert_eq!(relative_age(100, 400), "5m");
        assert_eq!(relative_age(100, 100 + 2 * DAY_SECS), "2d");
        let mut s = SidebarState::new();
        s.begin_rename("id1", "");
        s.rename_draft = "  hi  ".to_owned();
        assert!(matches!(
            s.commit_rename(),
            Some(SidebarAction::Rename { .. })
        ));
        assert_eq!(s.commit_rename(), None);
        s.cancel_rename();
        s.open_thread_menu("id1", "t", egui::Pos2::ZERO);
        assert!(matches!(s.menu, Some(ref m) if !m.confirm_delete));
    }
}
