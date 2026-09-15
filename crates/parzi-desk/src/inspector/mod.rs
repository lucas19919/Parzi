//! Inspector: Agents deck and Docs reader side panels.
//!
//! Mirrors `AgentVisualizer.svelte` (runs table, focus/fork/kill, live tool
//! trace) and `DocReader.svelte` (quick tabs, toolbar, preview/raw, TOC).

pub mod agents;
pub mod docs;

pub use agents::{AgentAction, AgentsState};
pub use docs::DocsState;

/// Inspector tabs, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectorTab {
    #[default]
    Agents,
    Docs,
}

impl InspectorTab {
    /// All tabs, in display order.
    pub const ALL: [Self; 2] = [Self::Agents, Self::Docs];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Docs => "Docs",
        }
    }
}

/// Both inspector panes plus tab routing.
#[derive(Debug, Default)]
pub struct InspectorState {
    pub tab: InspectorTab,
    pub agents: AgentsState,
    pub docs: DocsState,
}

impl InspectorState {
    /// Tab bar plus the active pane. Returns the agent action (if any) the
    /// bridge should execute (focus/fork/kill/spawn/approve).
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<AgentAction> {
        ui.horizontal(|ui| {
            for tab in InspectorTab::ALL {
                ui.selectable_value(&mut self.tab, tab, tab.label());
            }
        });
        ui.separator();
        match self.tab {
            InspectorTab::Agents => agents::show(ui, &mut self.agents),
            InspectorTab::Docs => {
                docs::show(ui, &mut self.docs);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tab_is_agents() {
        let s = InspectorState::default();
        assert_eq!(s.tab, InspectorTab::Agents);
        assert_eq!(InspectorTab::Agents.label(), "Agents");
        assert_eq!(InspectorTab::Docs.label(), "Docs");
    }
}
