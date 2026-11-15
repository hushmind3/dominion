//! Independent DOMINION agent harness.
//!
//! This crate deliberately has no dependency on the product server. It owns
//! the loop that turns a model into an agent: messages, bounded rounds, tool
//! dispatch, permission decisions, context budget and a structured event
//! stream. A product embeds it and supplies a provider, tools and a policy.
//!
//! Design goals, in order:
//! 1. A user instruction is never silently replaced or expanded.
//! 2. A round is one model call; tools are bounded per run and per call.
//! 3. Every tool passes a policy before it runs, and every denial is recorded.
//! 4. Context is trimmed explicitly, never grown without a limit.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub mod context;
pub mod openai;
pub mod policy;
pub mod tools;
pub mod provider;
pub mod tool;

pub use context::ContextBudget;
pub use policy::{AllowAll, Decision, DenyWrites, Policy, ReadOnlyPolicy};
pub use provider::{ModelReply, Provider};
pub use tool::{Permission, Tool, ToolCall, ToolSpec};

/// One conversation entry, independent of any provider's wire format.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: Role::System, content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: content.into() }
    }
    pub fn tool(content: impl Into<String>) -> Self {
        Self { role: Role::Tool, content: content.into() }
    }
}

/// Hard limits for one run. Values are checked, not advisory.
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_rounds: usize,
    pub max_tool_calls: usize,
    pub max_context_chars: usize,
    pub max_tool_output_chars: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_rounds: 12,
            max_tool_calls: 40,
            max_context_chars: 120_000,
            max_tool_output_chars: 8_000,
        }
    }
}

/// Why a run ended. Callers must treat every non-`Completed` reason as a
/// failure to report, not as success.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopReason {
    Completed,
    MaxRounds,
    MaxToolCalls,
    Denied { tool: String },
    ProviderError(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    RoundStarted(usize),
    AssistantText(String),
    ToolStarted { name: String, arguments: Value },
    ToolFinished { name: String, ok: bool, preview: String },
    Denied { name: String, permission: Permission },
    ContextTrimmed { removed: usize },
}

pub struct RunReport {
    pub messages: Vec<Message>,
    pub events: Vec<Event>,
    pub rounds: usize,
    pub tool_calls: usize,
    pub stop: StopReason,
}

impl RunReport {
    pub fn completed(&self) -> bool {
        self.stop == StopReason::Completed
    }
}

/// The harness itself. Owns the loop, not the model or the tools.
pub struct Harness<P: Provider> {
    provider: P,
    tools: BTreeMap<String, Box<dyn Tool>>,
    policy: Box<dyn Policy>,
    limits: Limits,
    context: ContextBudget,
}

impl<P: Provider> Harness<P> {
    pub fn new(provider: P, policy: Box<dyn Policy>, limits: Limits) -> Self {
        let context = ContextBudget::new(limits.max_context_chars);
        Self { provider, tools: BTreeMap::new(), policy, limits, context }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.spec().name;
        self.tools.insert(name, tool);
    }

    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    /// Replace the provider without restarting the run. Used to hand the
    /// provider the exact tool list the policy admitted.
    pub fn provider_mut(&mut self) -> &mut P {
        &mut self.provider
    }

    /// Run one user instruction to a bounded stop.
    ///
    /// The system message is prepended once. The loop never rewrites the
    /// user's instruction; it only appends assistant and tool messages.
    pub fn run(&mut self, system: &str, instruction: &str) -> RunReport {
        let mut messages = vec![Message::system(system), Message::user(instruction)];
        let mut events = Vec::new();
        let mut rounds = 0usize;
        let mut tool_calls = 0usize;
        let mut stop = StopReason::Completed;

        while rounds < self.limits.max_rounds {
            rounds += 1;
            events.push(Event::RoundStarted(rounds));
            self.trim(&mut messages, &mut events);

            let reply = match self.provider.complete(&messages) {
                Ok(reply) => reply,
                Err(error) => {
                    stop = StopReason::ProviderError(error);
                    break;
                }
            };
            let (content, calls) = match reply {
                ModelReply::Assistant { content, tool_calls } => (content, tool_calls),
                ModelReply::Final { content } => {
                    events.push(Event::AssistantText(content.clone()));
                    messages.push(Message::assistant(content));
                    stop = StopReason::Completed;
                    break;
                }
            };
            if !content.is_empty() {
                events.push(Event::AssistantText(content.clone()));
            }
            messages.push(Message::assistant(content));

            if calls.is_empty() {
                stop = StopReason::Completed;
                break;
            }
            for call in calls {
                if tool_calls >= self.limits.max_tool_calls {
                    stop = StopReason::MaxToolCalls;
                    break;
                }
                tool_calls += 1;
                let Some(tool) = self.tools.get_mut(&call.name) else {
                    let result = format!("unknown tool: {}", call.name);
                    messages.push(Message::tool(result.clone()));
                    events.push(Event::ToolFinished {
                        name: call.name,
                        ok: false,
                        preview: result,
                    });
                    continue;
                };
                let spec = tool.spec();
                match self.policy.decide(&spec) {
                    Decision::Allow => {}
                    Decision::Deny | Decision::Ask => {
                        let result = format!(
                            "tool {} was not executed: policy {}",
                            spec.name,
                            match self.policy.decide(&spec) {
                                Decision::Deny => "denied",
                                Decision::Ask => "requires approval",
                                Decision::Allow => "allowed",
                            }
                        );
                        messages.push(Message::tool(result.clone()));
                        events.push(Event::Denied {
                            name: spec.name,
                            permission: spec.permission,
                        });
                        stop = StopReason::Denied { tool: call.name };
                        continue;
                    }
                }
                events.push(Event::ToolStarted {
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                });
                let outcome = tool.invoke(&call.arguments);
                let (ok, text) = match outcome {
                    Ok(value) => (true, value.to_string()),
                    Err(error) => (false, error),
                };
                let text = truncate(&text, self.limits.max_tool_output_chars);
                events.push(Event::ToolFinished {
                    name: call.name,
                    ok,
                    preview: truncate(&text, 200),
                });
                messages.push(Message::tool(text));
            }
            // A denied or unapproved tool ends the run: the caller must
            // surface the pending decision instead of continuing silently.
            if matches!(stop, StopReason::MaxToolCalls | StopReason::Denied { .. }) {
                break;
            }
        }
        if rounds >= self.limits.max_rounds && stop == StopReason::Completed {
            stop = StopReason::MaxRounds;
        }
        RunReport { messages, events, rounds, tool_calls, stop }
    }

    fn trim(&mut self, messages: &mut Vec<Message>, events: &mut Vec<Event>) {
        let removed = self.context.trim(messages);
        if removed > 0 {
            events.push(Event::ContextTrimmed { removed });
        }
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out = text.chars().take(limit).collect::<String>();
    out.push_str("…[truncated]");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Scripted {
        replies: Vec<ModelReply>,
        seen: Vec<Vec<Message>>,
    }

    impl Provider for Scripted {
        fn complete(&mut self, messages: &[Message]) -> Result<ModelReply, String> {
            self.seen.push(messages.to_vec());
            if self.replies.is_empty() {
                return Err("script exhausted".into());
            }
            Ok(self.replies.remove(0))
        }
    }

    struct Counter {
        calls: usize,
        deny: bool,
    }

    impl Tool for Counter {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "count".into(),
                description: "counts".into(),
                parameters: json!({"type":"object","properties":{}}),
                permission: Permission::Read,
            }
        }
        fn invoke(&mut self, _arguments: &Value) -> Result<Value, String> {
            self.calls += 1;
            Ok(json!({"count": self.calls, "ok": !self.deny}))
        }
    }

    struct AllowAll;
    impl Policy for AllowAll {
        fn decide(&self, _spec: &ToolSpec) -> Decision { Decision::Allow }
    }

    struct DenyAll;
    impl Policy for DenyAll {
        fn decide(&self, _spec: &ToolSpec) -> Decision { Decision::Deny }
    }

    fn tool_call(name: &str) -> ModelReply {
        ModelReply::Assistant {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "1".into(),
                name: name.into(),
                arguments: json!({}),
            }],
        }
    }

    #[test]
    fn a_tool_call_is_executed_and_the_final_answer_completes() {
        let provider = Scripted {
            replies: vec![tool_call("count"), ModelReply::Final { content: "done".into() }],
            seen: Vec::new(),
        };
        let mut harness = Harness::new(provider, Box::new(AllowAll), Limits::default());
        harness.register(Box::new(Counter { calls: 0, deny: false }));
        let report = harness.run("system", "do the task");
        assert!(report.completed());
        assert_eq!(report.rounds, 2);
        assert_eq!(report.tool_calls, 1);
        assert!(report.messages.iter().any(|m| m.role == Role::Tool && m.content.contains("\"count\":1")));
        assert!(report.events.iter().any(|e| matches!(e, Event::ToolFinished { ok: true, .. })));
    }

    #[test]
    fn a_denied_tool_never_runs_and_the_run_is_not_a_success() {
        let provider = Scripted {
            replies: vec![tool_call("count"), ModelReply::Final { content: "done".into() }],
            seen: Vec::new(),
        };
        let mut harness = Harness::new(provider, Box::new(DenyAll), Limits::default());
        harness.register(Box::new(Counter { calls: 0, deny: false }));
        let report = harness.run("system", "do the task");
        assert!(!report.completed());
        assert_eq!(report.stop, StopReason::Denied { tool: "count".into() });
        assert_eq!(report.tool_calls, 1);
        assert!(report.messages.iter().any(|m| m.role == Role::Tool && m.content.contains("not executed")));
    }

    #[test]
    fn the_round_and_tool_limits_stop_a_looping_model() {
        let replies = (0..50).map(|_| tool_call("count")).collect::<Vec<_>>();
        let provider = Scripted { replies, seen: Vec::new() };
        let limits = Limits { max_rounds: 3, max_tool_calls: 2, ..Limits::default() };
        let mut harness = Harness::new(provider, Box::new(AllowAll), limits);
        harness.register(Box::new(Counter { calls: 0, deny: false }));
        let report = harness.run("s", "loop");
        assert!(!report.completed());
        assert!(matches!(report.stop, StopReason::MaxToolCalls | StopReason::MaxRounds));
        assert!(report.tool_calls <= 2);
        assert!(report.rounds <= 3);
    }

    #[test]
    fn the_user_instruction_is_never_rewritten() {
        let provider = Scripted { replies: vec![ModelReply::Final { content: "ok".into() }], seen: Vec::new() };
        let mut harness = Harness::new(provider, Box::new(AllowAll), Limits::default());
        let report = harness.run("s", "정확히 이 문장을 실행해");
        assert!(report.messages.iter().any(|m| m.role == Role::User && m.content == "정확히 이 문장을 실행해"));
    }
}
