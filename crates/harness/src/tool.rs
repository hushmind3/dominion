use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Tool risk class. A policy maps this to allow, deny or ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission {
    Read,
    Write,
    Execute,
    Network,
    Dangerous,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub permission: Permission,
}

/// A call requested by the model. Arguments are opaque JSON; the tool owns
/// validation because only it knows its own schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// A tool the harness may execute. Implementations must be side-effect
/// explicit: the harness never retries a call on its own.
pub trait Tool {
    fn spec(&self) -> ToolSpec;
    fn invoke(&mut self, arguments: &Value) -> Result<Value, String>;
}
