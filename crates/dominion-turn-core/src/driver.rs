//! Intrinsic agent execution protocol, independent of providers and add-ons.
//! The host owns I/O, approval, tool implementations, context, planning and
//! history. Only this driver admits model rounds and provider-visible calls.
use serde_json::Value;
use std::future::Future;
use std::collections::HashSet;

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
    seen_call_ids: HashSet<String>,
}

impl Driver {
    pub fn new(max_rounds: usize, max_tools: usize) -> Self {
        Self { max_rounds, max_tools, rounds: 0, proposals: 0, seen_call_ids: HashSet::new() }
    }

    pub fn begin_model_round(&mut self, cancelled: bool) -> Result<(), &'static str> {
        if cancelled {return Err("execution cancelled");}
        if self.rounds >= self.max_rounds {return Err("model round limit exceeded");}
        self.rounds += 1;
        Ok(())
    }

    /// The core admits a model round before the supplied provider future is
    /// polled. Provider selection, streaming and fallback stay in services.
    pub async fn request_model<T, F>(&mut self, cancelled: bool, provider_call: F)
        -> Result<T, String>
    where F: Future<Output = Result<T, String>> {
        self.begin_model_round(cancelled).map_err(str::to_string)?;
        provider_call.await
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

    /// Atomically reserve every tool call in one model response before any host
    /// side effect. Duplicate provider IDs are rejected across the entire
    /// turn; names must be visible in the current model round.
    pub fn admit_batch<'a>(
        &mut self, calls: impl IntoIterator<Item = (&'a str, &'a str)>,
        visible: &[Value], enabled: bool,
    ) -> Result<(), &'static str> {
        if !enabled { return Err("tool execution disabled"); }
        let pending: Vec<_> = calls.into_iter().collect();
        if pending.len() > self.max_tools.saturating_mul(2).saturating_sub(self.proposals) {
            return Err("tool proposal limit exceeded");
        }
        let mut ids = HashSet::new();
        for (id, name) in &pending {
            if id.trim().is_empty() || !ids.insert(*id) || self.seen_call_ids.contains(*id) {
                return Err("duplicate or missing tool call id");
            }
            if !Self::advertised(name, visible) {
                return Err("tool was not advertised");
            }
        }
        let count = pending.len();
        for (id, _) in pending { self.seen_call_ids.insert(id.to_owned()); }
        self.proposals += count;
        Ok(())
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

    #[tokio::test]
    async fn cancelled_round_never_polls_its_provider() {
        use std::cell::Cell;
        let called=Cell::new(false);
        let mut driver=Driver::new(1,1);
        let future=async {called.set(true); Ok::<_,String>("response")};
        assert_eq!(driver.request_model(true,future).await,
            Err("execution cancelled".to_string()));
        assert!(!called.get());
        let result=driver.request_model(false,async {Ok::<_,String>("ok")}).await;
        assert_eq!(result,Ok("ok"));
        assert_eq!(driver.request_model(false,async {Ok::<_,String>("no")}).await,
            Err("model round limit exceeded".to_string()));
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
    #[test]
    fn atomic_batch_rejects_hidden_duplicate_and_over_budget_calls_without_reserving_any() {
        let visible=vec![json!({"name":"file_read"})];
        let mut gate=Driver::new(3,2);
        assert_eq!(gate.admit_batch([("x","file_read"),("x","file_read")],
            &visible,true),Err("duplicate or missing tool call id"));
        assert_eq!(gate.admit_batch([("x","file_read"),("y","mcp__hidden")],
            &visible,true),Err("tool was not advertised"));
        assert!(gate.admit_batch([("x","file_read"),("y","file_read")],
            &visible,true).is_ok());
        assert_eq!(gate.admit_batch([("x","file_read")],
            &visible,true),Err("duplicate or missing tool call id"));
        assert_eq!(gate.admit_batch([("z","file_read"),("w","file_read"),
            ("v","file_read")],&visible,true),Err("tool proposal limit exceeded"));
        assert!(gate.admit_batch([("z","file_read"),("w","file_read")],
            &visible,true).is_ok());
    }

}
