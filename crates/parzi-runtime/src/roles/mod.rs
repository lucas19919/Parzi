//! Project roles (PLAN §1.2 and §5): who a run is, what it may call, and what
//! it is allowed to see. Three roles bind to three models in the project
//! roster; each gets its own system prompt (`parzi_core::prompts`) and its own
//! context, assembled deterministically from files — never from another
//! session's transcript.

mod context;
mod digest;

use serde::{Deserialize, Serialize};

use parzi_core::project::{Project, Roster};
use parzi_core::prompts;

pub use digest::workspace_digest;

/// One labelled block of role context. Plain text so any provider takes it;
/// the micro-context for a coder lane builds the same type, so all three
/// roles speak in one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPart {
    pub label: String,
    pub text: String,
}

impl ContextPart {
    pub fn new(label: &str, text: impl Into<String>) -> Self {
        Self {
            label: label.to_string(),
            text: text.into(),
        }
    }
}

/// Header reads, orchestrator plans, coder works. Nothing else exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Header,
    Orchestrator,
    Coder,
}

impl Role {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Header => "header",
            Role::Orchestrator => "orchestrator",
            Role::Coder => "coder",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "header" => Some(Role::Header),
            "orchestrator" => Some(Role::Orchestrator),
            // `implementation` is what the legacy lane roster calls a coder.
            "coder" | "implementation" => Some(Role::Coder),
            _ => None,
        }
    }

    /// The model this role runs on, from the project roster.
    #[must_use]
    pub fn model_of(self, roster: &Roster) -> String {
        match self {
            Role::Header => roster.header.clone(),
            Role::Orchestrator => roster.orchestrator.clone(),
            Role::Coder => roster.coder.clone(),
        }
    }

    /// Default session lane name for a role run. Coders override it with the
    /// lane they were dispatched for.
    #[must_use]
    pub fn lane_name(self) -> &'static str {
        self.as_str()
    }
}

/// Tools a role run may call — the whole list, not an addition to the lane's.
/// This is the enforcement behind the prompts: the header cannot claim a file
/// because `lease.claim` is not in its list, not because it was asked nicely.
#[must_use]
pub fn role_tools(role: Role) -> Vec<String> {
    let names: &[&str] = match role {
        Role::Header => &[
            "fs.read",
            "fs.list",
            "project.draft_plan",
            "project.status",
            "board.list",
            "knowledge.read",
        ],
        Role::Orchestrator => &[
            "fs.read",
            "fs.list",
            "project.audit",
            "project.status",
            "board.list",
            "knowledge.read",
            "knowledge.record",
        ],
        Role::Coder => &[
            "fs.read",
            "fs.write",
            "fs.list",
            "shell.exec",
            "lease.claim",
            "lease.release",
            "lease.request",
            "lease.grant",
            "lease.deny",
            "board.list",
            "board.handoff",
            "board.block",
            "knowledge.read",
            "knowledge.record",
        ],
    };
    names.iter().map(|s| (*s).to_string()).collect()
}

/// What a role run is about: always a project, sometimes a lane and a task.
/// Built by `project_flow`, never by the model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleCtx {
    pub workspace: String,
    pub slug: String,
    /// Lane name for a coder run; empty for header and orchestrator.
    pub lane: String,
    /// `TSK-7` for a coder run.
    pub task: Option<String>,
}

impl RoleCtx {
    pub fn new(workspace: impl Into<String>, slug: impl Into<String>) -> Self {
        Self {
            workspace: workspace.into(),
            slug: slug.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn lane(mut self, lane: impl Into<String>) -> Self {
        self.lane = lane.into();
        self
    }

    #[must_use]
    pub fn task(mut self, task: impl Into<String>) -> Self {
        self.task = Some(task.into());
        self
    }
}

/// What a session is, when it is a role run. Stored next to the transcript by
/// the orchestrator, so every later turn of that session keeps the same
/// briefing and the same tool list — a header session cannot become a coder
/// because somebody typed into it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleBinding {
    pub role: Role,
    pub ctx: RoleCtx,
}

impl RoleBinding {
    pub fn new(role: Role, ctx: RoleCtx) -> Self {
        Self { role, ctx }
    }

    /// The system parts this run opens with.
    #[must_use]
    pub fn brief(&self) -> Vec<String> {
        brief(self.role, &self.ctx)
    }

    /// The whole tool allowlist for this run.
    #[must_use]
    pub fn tools(&self) -> Vec<String> {
        role_tools(self.role)
    }
}

/// The role's standing instructions plus the one line that binds it to this
/// project. Deterministic: same role, same context, same string.
#[must_use]
pub fn system_prompt(role: Role, ctx: &RoleCtx) -> String {
    let base = match role {
        Role::Header => prompts::HEADER,
        Role::Orchestrator => prompts::ORCHESTRATOR,
        Role::Coder => prompts::CODER,
    };
    let mut binding = format!(
        "You are the {} of project `{}` in workspace `{}`.",
        role.as_str(),
        ctx.slug,
        ctx.workspace
    );
    if !ctx.lane.is_empty() {
        binding.push_str(&format!(" Your lane is `{}`.", ctx.lane));
    }
    if let Some(task) = &ctx.task {
        binding.push_str(&format!(" Your task is `{task}` and nothing else."));
    }
    binding.push_str(&format!(
        " Tools you may call: {}. Anything else is not yours.",
        role_tools(role).join(", ")
    ));
    format!("{}\n\n{binding}", base.trim_end())
}

/// Everything the role is allowed to see, in reading order. Missing files are
/// simply absent — a project that has no PLAN.md yet is a fact, not an error.
#[must_use]
pub fn context_for(role: Role, ctx: &RoleCtx) -> Vec<ContextPart> {
    match role {
        Role::Header => context::header(ctx),
        Role::Orchestrator => context::orchestrator(ctx),
        Role::Coder => context::coder(ctx),
    }
}

/// System parts for an `AgentRun`: the prompt, then one part per context
/// block. This is what the orchestrator hands the handler for a role run,
/// instead of the lane's SYSTEM.md.
#[must_use]
pub fn brief(role: Role, ctx: &RoleCtx) -> Vec<String> {
    let mut parts = vec![system_prompt(role, ctx)];
    parts.extend(context_for(role, ctx).iter().map(render_part));
    parts
}

/// One context block as the model sees it.
#[must_use]
pub fn render_part(part: &ContextPart) -> String {
    format!("# {}\n\n{}", part.label, part.text.trim_end())
}

/// The roster a project binds to its roles; kept here so callers do not have
/// to know which field belongs to which role.
#[must_use]
pub fn model_for(project: &Project, role: Role) -> String {
    role.model_of(&project.roster)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_names_round_trip() {
        for role in [Role::Header, Role::Orchestrator, Role::Coder] {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
        assert_eq!(Role::parse("implementation"), Some(Role::Coder));
        assert_eq!(Role::parse("nobody"), None);
    }

    #[test]
    fn header_cannot_claim_and_coder_cannot_plan() {
        let header = role_tools(Role::Header);
        assert!(!header.iter().any(|t| t.starts_with("lease.")));
        assert!(!header.iter().any(|t| t == "project.audit"));
        assert!(!header.iter().any(|t| t == "fs.write"));
        assert!(header.iter().any(|t| t == "project.draft_plan"));
        let coder = role_tools(Role::Coder);
        assert!(!coder.iter().any(|t| t.starts_with("project.")));
        assert!(coder.iter().any(|t| t == "lease.claim"));
        let orch = role_tools(Role::Orchestrator);
        assert!(orch.iter().any(|t| t == "project.audit"));
        assert!(!orch.iter().any(|t| t == "fs.write"));
    }

    #[test]
    fn system_prompt_binds_the_run_to_its_task() {
        let ctx = RoleCtx::new("acme", "checkout-flow")
            .lane("api")
            .task("TSK-7");
        let p = system_prompt(Role::Coder, &ctx);
        assert!(p.contains("lease.claim"));
        assert!(p.contains("`TSK-7`"));
        assert!(p.contains("lane is `api`"));
        // The standing prompt is the one on disk, not a paraphrase.
        assert!(p.starts_with(prompts::CODER.trim_end().lines().next().unwrap()));
    }
}
