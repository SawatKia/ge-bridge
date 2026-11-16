//! `ge-bridge-cli acp`: an Agent Client Protocol agent over stdio, so OpenMausBot (driver `customAcp`) and other
//! ACP clients can run Gemini Enterprise as a coding agent. The model is reached through the running ge-bridge
//! server's `/v1/chat/completions`; the HTTP API itself is unchanged.

pub mod agent;
pub mod llm;
pub mod mcp;
pub mod rpc;
pub mod tools;
pub mod types;

use std::sync::Arc;

/// Serves ACP on stdin/stdout until the client closes stdin. Nothing but protocol JSON goes to stdout.
pub fn run() -> std::process::ExitCode {
    let rpc = rpc::Rpc::new();
    let agent = agent::Agent::new(rpc.clone());
    let a = agent.clone();
    rpc.serve(Arc::new(move |method, params| a.handle(method, params)));
    agent.shutdown();
    std::process::ExitCode::SUCCESS
}
