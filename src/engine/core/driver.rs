//! Intrinsic agent execution protocol, independent of providers and add-ons.
//! The host owns I/O, approval, tool implementations, context, planning and
//! history. Only this driver admits model rounds and provider-visible calls.
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Answer,
    Execute { calls: usize },
}

/// One turn-local monotonic state machine; no storage or transport ownership.
pub struct Driver {
    max_rounds: usize,
    max_tools: usize,
    rounds: usize,
    proposals: usize,
}

impl Driver {
    pub fn new(max_rounds: usize, max_tools: usize) -> Self {
        Self { max_rounds, max_tools, rounds: 0, proposals: 0 }
    }

    pub fn begin_model_round(&mut self, cancelled: bool) -> Result<(), &'static str> {
        if cancelled {return Err("execution cancelled");}
        if self.rounds >= self.max_rounds {return Err("model round limit exceeded");}
        self.rounds += 1;
        Ok(())
    }

    pub fn select_step(&self, count: usize, tools_enabled: bool)
        -> Result<Step, &'static str> {
        if count == 0 {return Ok(Step::Answer);}
        if !tools_enabled {return Err("tool execution disabled");}
        if count > self.max_tools.saturating_mul(2).saturating_sub(self.proposals) {
            return Err("tool proposal limit exceeded");
        }
        Ok(Step::Execute { calls: count })
    }

    /// Compare against exactly what the provider was shown in the CURRENT
    /// model request. Hidden MCP names may not be guessed, including inside a
    /// parallel read group where only the first item was formerly checked.
    pub fn advertised(name: &str, visible: &[Value]) -> bool {
        !name.trim().is_empty() && visible.iter().any(|schema| {
            schema.pointer("/function/name")
                .or_else(||schema.get("name"))
                .and_then(Value::as_str)==Some(name)
        })
    }

    pub fn batch_advertised<'a>(
        names: impl IntoIterator<Item = &'a str>, visible: &[Value],
    ) -> bool {
        names.into_iter().all(|name|Self::advertised(name,visible))
    }

    pub fn admit_tool(&mut self, name: &str, visible: &[Value], enabled: bool)
        -> Result<(), &'static str> {
        if !enabled {return Err("tool execution disabled");}
        if self.proposals >= self.max_tools.saturating_mul(2) {
            return Err("tool proposal limit exceeded");
        }
        self.proposals += 1;
        if !Self::advertised(name,visible) {
            return Err("tool was not advertised");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rounds_and_cancellation_are_monotonic() {
        let mut gate=Driver::new(2,3);
        assert_eq!(gate.begin_model_round(true),Err("execution cancelled"));
        assert!(gate.begin_model_round(false).is_ok());
        assert!(gate.begin_model_round(false).is_ok());
        assert_eq!(gate.begin_model_round(false),Err("model round limit exceeded"));
    }

    #[test]
    fn only_currently_advertised_tools_can_execute() {
        let visible=vec![json!({"type":"function","function":{
            "name":"mcp__safe__read","parameters":{"type":"object"}
        }})];
        let mut gate=Driver::new(3,2);
        assert_eq!(gate.select_step(0,false),Ok(Step::Answer));
        assert_eq!(gate.select_step(1,false),Err("tool execution disabled"));
        assert_eq!(gate.admit_tool("mcp__safe__read",&visible,false),
            Err("tool execution disabled"));
        assert_eq!(gate.admit_tool("mcp__safe__delete",&visible,true),
            Err("tool was not advertised"));
        assert!(gate.admit_tool("mcp__safe__read",&visible,true).is_ok());
        assert_eq!(gate.select_step(4,true),Err("tool proposal limit exceeded"));
        assert!(!Driver::advertised("mcp__safe__read_more",&visible));
    }

    #[test]
    fn parallel_read_batch_never_bypasses_hidden_tool_authority() {
        let visible=vec![json!({"type":"function","function":{
            "name":"workspace_read","parameters":{"type":"object"}
        }})];
        assert!(Driver::batch_advertised(["workspace_read","workspace_read"],&visible));
        assert!(!Driver::batch_advertised(["workspace_read","file_read"],&visible));
        assert!(!Driver::batch_advertised(["workspace_read","mcp__other__read"],&visible));
    }

    #[test]
    fn tool_proposal_budget_has_no_wraparound() {
        let mut gate=Driver::new(2,1);
        let visible=vec![json!({"name":"workspace_read"})];
        assert!(gate.admit_tool("workspace_read",&visible,true).is_ok());
        assert!(gate.admit_tool("workspace_read",&visible,true).is_ok());
        assert_eq!(gate.admit_tool("workspace_read",&visible,true),
            Err("tool proposal limit exceeded"));
    }
}
