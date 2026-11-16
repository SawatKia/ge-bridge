//! Types shared by the ACP agent (`mod.rs`/`agent.rs`), the local tools (`tools.rs`) and the MCP hub (`mcp.rs`).

use serde_json::Value;
use std::path::PathBuf;

/// One tool as offered to the model (OpenAI `function` shape: name, description, JSON-schema parameters).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolSpec {
    /// `{"type":"function","function":{name,description,parameters}}` for /v1/chat/completions.
    pub fn to_openai(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": { "name": self.name, "description": self.description, "parameters": self.parameters }
        })
    }
}

/// How dangerous a call is. Write and Execute need the person's approval (`session/request_permission`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Risk {
    Read,
    Write,
    Execute,
}

/// An image a tool returned (e.g. from an MCP `image` content block).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolImage {
    pub mime: String,
    pub b64: String,
}

/// The result of one tool call, ready to report to the client and to feed back to the model.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolOutcome {
    /// Text for the model (and shown in the tool card). Errors are plain sentences the model can act on.
    pub text: String,
    pub is_error: bool,
    /// Files the call touched, for ACP `locations` (absolute paths).
    pub locations: Vec<PathBuf>,
    pub images: Vec<ToolImage>,
}

impl ToolOutcome {
    pub fn ok(text: impl Into<String>) -> Self {
        Self { text: text.into(), ..Default::default() }
    }
    pub fn err(text: impl Into<String>) -> Self {
        Self { text: text.into(), is_error: true, ..Default::default() }
    }
}
