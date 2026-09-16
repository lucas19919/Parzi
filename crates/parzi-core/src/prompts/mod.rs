//! Role prompt texts (PLAN.md §1.2). Kept as markdown next to the code so a
//! prompt change is a readable diff, not a string edit; `parzi-runtime::roles`
//! is the only consumer.

/// Header ("idea agent"): reads state, drafts PROJECT.md and rough plans.
pub const HEADER: &str = include_str!("header.md");
/// Orchestrator: sole writer of PLAN.md, audits drafts into sprints/lanes.
pub const ORCHESTRATOR: &str = include_str!("orchestrator.md");
/// Coder: one task, one worktree, leases first and a capsule at the end.
pub const CODER: &str = include_str!("coder.md");
