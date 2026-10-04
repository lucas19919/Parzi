pub mod desk;
pub mod doctor;
pub mod handler;
pub mod hooks;
pub mod inter;
pub mod mcp;
pub mod mcp_host;
pub mod orchestrator;
pub mod run;
pub mod status;
pub mod toolhost;
pub mod tools;

pub use orchestrator::Orchestrator;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
