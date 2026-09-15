//! Model picker / effort / slash data models (Phase 2, M4 data side).
//!
//! Ports the STRUCTURE of the Omnibar picker: provider sections in roster order,
//! a synthetic Smart Auto entry, a favourites set, Ctrl+1..5 mapping, the effort
//! ladder, and the slash list. Pure logic plus tests; egui popovers are a follow-up.
//!
//! Slash commands come from the real runtime API (`plugins::slash_commands`,
//! i.e. installed `commands` packs). With no packs — or an unreadable dir —
//! the menu degrades to the built-in list below (web Omnibar parity).

use super::Effort;
use std::collections::HashSet;

/// Roster order: the backend `PROVIDERS` list, so picker, settings, doctor
/// and CLI can never disagree about it.
pub const PROVIDER_ORDER: &[&str] = parzi_providers::PROVIDERS;

/// One collapsed family row: `(provider, family)` with the effort variant
/// resolved at send time ([`variant_for`]). Mirrors the Omnibar `FamRow`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRow {
    pub provider: String,
    pub family: String,
    pub label: String,
    /// Picker value: `provider/family`, or `"auto"`.
    pub value: String,
    pub legacy: bool,
}

impl ModelRow {
    /// Synthetic picker row: Smart Auto routes across signed-in providers.
    pub fn auto() -> Self {
        Self {
            provider: "auto".into(),
            family: "auto".into(),
            label: "Smart Auto".into(),
            value: "auto".into(),
            legacy: false,
        }
    }
}

/// One provider section of the picker, in [`PROVIDER_ORDER`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSection {
    pub id: String,
    pub display_name: String,
    pub rows: Vec<ModelRow>,
}

/// Read-only catalog snapshot: one section per roster provider, families collapsed
/// (first row per family wins under `sort_models`: flagship first, legacy last).
pub fn sections_from_catalog() -> Vec<ProviderSection> {
    let mut out = Vec::with_capacity(PROVIDER_ORDER.len());
    for id in PROVIDER_ORDER {
        let mut models = parzi_providers::catalog::for_provider(id);
        parzi_providers::catalog::sort_models(&mut models);
        let mut seen = HashSet::new();
        let mut rows = vec![];
        for m in models {
            let family = if m.family.is_empty() {
                m.id.clone()
            } else {
                m.family.clone()
            };
            if !seen.insert(family.clone()) {
                continue;
            }
            let label = if m.family_name.is_empty() {
                m.name.clone()
            } else {
                m.family_name.clone()
            };
            rows.push(ModelRow {
                value: format!("{id}/{family}"),
                provider: id.to_string(),
                family,
                label,
                legacy: m.legacy,
            });
        }
        out.push(ProviderSection {
            id: id.to_string(),
            display_name: parzi_providers::display_name(id).into(),
            rows,
        });
    }
    out
}

/// Picker state: query, favourites, keyboard selection. Visible = Smart Auto
/// first (when it matches), then families; searching overrides sections.
#[derive(Debug, Clone, Default)]
pub struct ModelPicker {
    rows: Vec<ModelRow>,
    query: String,
    favourites: HashSet<String>,
    selected: usize,
}

impl ModelPicker {
    pub fn from_catalog() -> Self {
        Self::from_rows(
            sections_from_catalog()
                .into_iter()
                .flat_map(|s| s.rows)
                .collect(),
        )
    }
    pub fn from_rows(rows: Vec<ModelRow>) -> Self {
        Self {
            rows,
            ..Self::default()
        }
    }
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.selected = 0;
    }
    pub fn favourites(&self) -> &HashSet<String> {
        &self.favourites
    }
    /// Star/unstar a model value. Returns true when now starred.
    pub fn toggle_favourite(&mut self, value: &str) -> bool {
        if self.favourites.remove(value) {
            false
        } else {
            self.favourites.insert(value.to_string());
            true
        }
    }
    fn matches(row: &ModelRow, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        row.label.to_lowercase().contains(query)
            || row.provider.to_lowercase().contains(query)
            || parzi_providers::display_name(&row.provider)
                .to_lowercase()
                .contains(query)
            || row.family.to_lowercase().contains(query)
    }
    /// Visible rows (Smart Auto + matches, stable order); filtering resets selection.
    pub fn visible_rows(&self) -> Vec<ModelRow> {
        let q = self.query.trim().to_lowercase();
        let mut out = vec![];
        let auto = ModelRow::auto();
        if Self::matches(&auto, &q) {
            out.push(auto);
        }
        out.extend(self.rows.iter().filter(|r| Self::matches(r, &q)).cloned());
        out
    }
    /// Arrow-key selection with wraparound; only the highlight moves.
    pub fn move_selection(&mut self, delta: i32) {
        let len = self.visible_rows().len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as i32 + delta).rem_euclid(len as i32) as usize;
    }
    pub fn pick_selected(&self) -> Option<ModelRow> {
        self.visible_rows().get(self.selected).cloned()
    }
    /// Ctrl+1..5: the Nth visible row (1-based). Pure; `None` out of range.
    pub fn shortcut(&self, n: u8) -> Option<ModelRow> {
        if (1..=5).contains(&n) {
            self.visible_rows().get(usize::from(n) - 1).cloned()
        } else {
            None
        }
    }
}

/// One effort rung for the effort segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffortOption {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
}

/// The five rungs, cheapest first. Ids round-trip through `Effort`.
pub fn effort_options() -> Vec<EffortOption> {
    [
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::Extra,
        Effort::Ultra,
    ]
    .into_iter()
    .map(|e| EffortOption {
        id: e.as_str(),
        label: e.label(),
        hint: e.hint(),
    })
    .collect()
}

/// Effort variant within an Antigravity-style family. Only low/medium/high
/// variants exist, so upper rungs ride `high`. Mirrors `variantFor`.
pub fn variant_for(family: &str, effort: &str) -> &'static str {
    if family.starts_with("gemini-3.8-flash")
        || family.starts_with("gemini-3.7-flash")
        || family.starts_with("gemini-3.6-flash")
    {
        return match effort {
            "low" => "low",
            "medium" => "medium",
            _ => "high",
        };
    }
    if family == "gemini-3.1-pro" {
        return if effort == "low" { "low" } else { "high" };
    }
    ""
}

/// One slash-menu entry: `/name` + hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashMenuItem {
    pub name: String,
    pub hint: String,
}

/// Built-in commands, mirroring the web Omnibar's static list. Plugin packs
/// (below) append; they never replace these.
pub const BUILTIN_SLASH: &[(&str, &str)] = &[
    ("/plan", "task planner"),
    ("/auto", "smart auto routing"),
    ("/model", "open model picker"),
    ("/effort", "cycle effort"),
    ("/new", "new thread"),
    ("/clear", "clean thread, same lane"),
    ("/fork", "branch this thread"),
    ("/subsession", "spawn a child subsession"),
    ("/kill", "stop the run"),
    ("/doctor", "health checks"),
    ("/help", "all commands"),
];

/// Full slash menu: built-ins plus installed `commands` packs. An unreadable
/// plugins dir degrades to built-ins only — the menu never blocks on disk.
pub fn slash_menu_items() -> Vec<SlashMenuItem> {
    let mut out: Vec<SlashMenuItem> = BUILTIN_SLASH
        .iter()
        .map(|(n, h)| SlashMenuItem {
            name: n.to_string(),
            hint: h.to_string(),
        })
        .collect();
    if let Ok(packs) = parzi_runtime::plugins::slash_commands() {
        for cmd in packs {
            let name = format!("/{}", cmd.name);
            if !out.iter().any(|i| i.name == name) {
                out.push(SlashMenuItem {
                    name,
                    hint: cmd.description,
                });
            }
        }
    }
    out
}

/// Prefix filter over the menu (`/pl` → `/plan`). Empty query shows all.
pub fn filter_slash<'a>(items: &'a [SlashMenuItem], query: &str) -> Vec<&'a SlashMenuItem> {
    let q = query.strip_prefix('/').unwrap_or(query).to_lowercase();
    items
        .iter()
        .filter(|i| {
            i.name
                .strip_prefix('/')
                .unwrap_or(i.name.as_str())
                .to_lowercase()
                .starts_with(&q)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(provider: &str, family: &str, label: &str, legacy: bool) -> ModelRow {
        ModelRow {
            provider: provider.into(),
            family: family.into(),
            label: label.into(),
            value: format!("{provider}/{family}"),
            legacy,
        }
    }
    fn rows() -> Vec<ModelRow> {
        vec![
            row("xai", "grok-4.3", "Grok 4.3", false),
            row("claude", "claude-opus-4-8", "Claude Opus 4.8", true),
        ]
    }
    #[test]
    fn shortcut_maps_ctrl_1_to_5_over_visible_rows() {
        let p = ModelPicker::from_rows(rows());
        assert_eq!(p.shortcut(1).map(|r| r.value), Some("auto".into()));
        assert_eq!(p.shortcut(2).map(|r| r.value), Some("xai/grok-4.3".into()));
        assert_eq!(p.shortcut(3).unwrap().value, "claude/claude-opus-4-8");
        for n in [0, 4, 5, 6, 255] {
            assert_eq!(p.shortcut(n), None, "n={n}");
        }
    }
    #[test]
    fn query_searches_across_providers_and_resets_selection() {
        let mut p = ModelPicker::from_rows(rows());
        p.move_selection(2);
        p.set_query("grok");
        let vis: Vec<_> = p.visible_rows().iter().map(|r| r.value.clone()).collect();
        assert_eq!(vis, vec!["xai/grok-4.3"]);
        assert_eq!(p.pick_selected().unwrap().value, "xai/grok-4.3");
        p.set_query("auto");
        assert_eq!(p.visible_rows().len(), 1);
        p.set_query("zzz");
        assert!(p.visible_rows().is_empty());
        assert_eq!(p.pick_selected(), None);
        p.move_selection(1); // no rows: stays put, never panics.
        assert_eq!(p.pick_selected(), None);
    }
    #[test]
    fn selection_wraps_around() {
        let mut p = ModelPicker::from_rows(rows());
        assert_eq!(p.pick_selected().unwrap().value, "auto");
        p.move_selection(-1);
        assert_eq!(p.pick_selected().unwrap().value, "claude/claude-opus-4-8");
        p.move_selection(1);
        assert_eq!(p.pick_selected().unwrap().value, "auto");
    }
    #[test]
    fn favourites_toggle_round_trips() {
        let mut p = ModelPicker::from_rows(rows());
        assert!(p.toggle_favourite("xai/grok-4.3"));
        assert!(p.favourites().contains("xai/grok-4.3"));
        assert!(!p.toggle_favourite("xai/grok-4.3"));
        assert!(p.favourites().is_empty());
    }
    #[test]
    fn variant_for_picks_antigravity_effort_variants() {
        assert_eq!(variant_for("gemini-3.8-flash", "low"), "low");
        assert_eq!(variant_for("gemini-3.8-flash", "medium"), "medium");
        assert_eq!(variant_for("gemini-3.8-flash", "ultra"), "high");
        assert_eq!(variant_for("gemini-3.1-pro", "low"), "low");
        assert_eq!(variant_for("gemini-3.1-pro", "extra"), "high");
        assert_eq!(variant_for("kimi-k3", "high"), "");
    }
    #[test]
    fn effort_ladder_has_five_ordered_rungs() {
        let opts = effort_options();
        let ids: Vec<_> = opts.iter().map(|o| o.id).collect();
        assert_eq!(ids, vec!["low", "medium", "high", "extra", "ultra"]);
        assert_eq!(opts[1].hint, "16k output");
        for o in &opts {
            assert_eq!(Effort::normalize(o.id).as_str(), o.id);
        }
    }
    #[test]
    fn slash_filter_is_prefix_based_and_merges_packs() {
        let items: Vec<SlashMenuItem> = BUILTIN_SLASH
            .iter()
            .map(|(n, h)| SlashMenuItem {
                name: n.to_string(),
                hint: h.to_string(),
            })
            .collect();
        assert_eq!(items.len(), 11);
        let names: Vec<_> = filter_slash(&items, "/pl")
            .iter()
            .map(|i| i.name.clone())
            .collect();
        assert_eq!(names, vec!["/plan"]);
        assert_eq!(filter_slash(&items, "").len(), 11);
        assert!(filter_slash(&items, "/zzz").is_empty());
        // Merged menu always contains the built-ins (packs only append).
        let merged = slash_menu_items();
        for (name, _) in BUILTIN_SLASH {
            assert!(merged.iter().any(|i| &i.name == name), "missing {name}");
        }
    }
    #[test]
    fn catalog_sections_cover_the_roster_without_dupes() {
        let sections = sections_from_catalog();
        assert_eq!(sections.len(), PROVIDER_ORDER.len());
        for (sec, id) in sections.iter().zip(PROVIDER_ORDER.iter()) {
            assert_eq!(&sec.id, id);
            let mut seen = HashSet::new();
            for r in &sec.rows {
                assert!(seen.insert(r.value.clone()), "dup family {}", r.value);
            }
        }
        assert!(sections.iter().any(|s| !s.rows.is_empty()));
    }
}
