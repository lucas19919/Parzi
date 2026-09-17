//! parzi-runtime: handler loop, orchestrator, tools, MCP, plugins, doctor.

pub mod board_tools;
pub mod circuit_breaker;
pub mod compact;
pub mod context_micro;
pub mod doctor;
pub mod git_checkpoints;
pub mod git_worktree;
pub mod handler;
pub mod hooks;
pub mod inter;
pub mod lease_tools;
pub mod mcp;
pub mod orchestrator;
pub mod plugins;
pub mod project_flow;
pub mod roles;
pub mod sync_timer;
pub mod tools;

pub use circuit_breaker::CircuitBreaker;
pub use handler::AgentRun;
pub use orchestrator::Orchestrator;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
