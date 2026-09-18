//! parzi-runtime: runs of vendor agents (`run`), Parzi's tools and the gate
//! (`toolhost`, `mcp_host`), the orchestrator, plugins, doctor.

pub mod board_tools;
pub mod context_micro;
pub mod doctor;
pub mod git_checkpoints;
pub mod git_worktree;
pub mod handler;
pub mod hooks;
pub mod inter;
pub mod lease_tools;
pub mod mcp;
pub mod mcp_host;
pub mod orchestrator;
pub mod plugins;
pub mod project_flow;
pub mod roles;
pub mod run;
pub mod status;
pub mod sync_timer;
pub mod toolhost;
pub mod tools;

pub use orchestrator::Orchestrator;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
