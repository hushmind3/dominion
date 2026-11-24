//! Convenience adapter for external hosts. The native Engine still owns
//! iteration; this module only translates a small set of ports into phases.
use crate::*;
use serde_json::{json, Value};
use std::time::Duration;

/// A journal scoped to exactly one execution. The caller must exclusively
/// lease the execution for the duration of run_embedded. Never overwrite an
/// existing checkpoint with a fresh one merely to retry a failed call.
pub trait CheckpointStore: Journal {
    fn load(&self) -> Result<Snapshot, String>;
}

#[derive(Clone, Copy, Debug)]
pub struct EmbeddedLimits {
    /// Total lifetime model rounds, including already persisted rounds.
    pub max_rounds: usize,
    /// Total lifetime tool proposals, including already persisted receipts.
    pub max_tools: usize,
    /// Wall-clock limit for this invocation, not a provider request timeout.
    pub max_elapsed: Duration,
}

#[derive(Clone, Debug)]
pub enum TurnDecision {
    Answer(String),
    Tools(Vec<Action>),
}

#[derive(Clone, Debug)]
pub struct ModelTurn {
    pub decision: TurnDecision,
    /// Tokens reported by the host for this successful call. This is not a
    /// price estimate; hosts account for failed/unknown attempts separately.
    pub charged_tokens: u64,
}

pub trait ModelPort: Send {
    fn decide<'a>(&'a mut self, snapshot: &'a Snapshot) -> BoxFuture<'a, Result<ModelTurn, String>>;
}

#[derive(Clone, Debug)]
pub enum Verification {
    Accept,
    Retry(String),
}

/// Independently check the requested outcome, not merely the model's claim.
pub trait Verifier: Send {
    fn verify<'a>(&'a mut self, snapshot: &'a Snapshot, answer: &'a str)
        -> BoxFuture<'a, Result<Verification, String>>;
}

/// Drive a caller-owned checkpoint using the native loop and durable tool
/// gateway. The caller supplies authenticated model/tool adapters, enforces
/// tool authorization/approval and sandboxing at its dispatcher, and owns
/// exclusive checkpoint access. No default shell, network or provider exists.
/// An unsettled checkpoint is rejected; reconcile it before explicitly
/// resuming. Persisted rounds/receipts consume the new invocation's limits.
pub async fn run_embedded<J, M, D, V>(
    journal: &J,
    model: &mut M,
    dispatcher: &D,
    verifier: &mut V,
    limits: EmbeddedLimits,
) -> Result<Snapshot, String>
where
    J: CheckpointStore,
    M: ModelPort,
    D: Dispatcher,
    V: Verifier,
{
    let snapshot = journal.load()?;
    if snapshot.native.status != RunStatus::Running || !snapshot.settled() {
        return Err("checkpoint is not running and settled; inspect or reconcile it first".into());
    }
    let mut budget = BudgetLedger::new(
        limits.max_rounds.min(snapshot.native.max_steps),
        limits.max_tools,
        limits.max_elapsed,
    );
    budget.restore(
        snapshot.step.max(snapshot.native.steps).max(snapshot.model_calls),
        snapshot.receipts.len(),
    );
    let mut host = PortHost {
        journal, model, dispatcher, verifier,
        id: snapshot.native.id.to_string(),
        calls: snapshot.model_calls,
        tokens: snapshot.charged_tokens,
        answer: None,
        actions: Vec::new(),
    };
    run(&mut host, Context { snapshot, budget, round: 0, journal }).await?;
    let saved = host.reload()?;
    if saved.native.status != RunStatus::Completed || !saved.settled() {
        return Err("native completion was not durably observed".into());
    }
    Ok(saved)
}

struct PortHost<'p, J, M, D, V> {
    journal: &'p J,
    model: &'p mut M,
    dispatcher: &'p D,
    verifier: &'p mut V,
    id: String,
    calls: usize,
    tokens: u64,
    answer: Option<String>,
    actions: Vec<Action>,
}

impl<J, M, D, V> Host for PortHost<'_, J, M, D, V>
where
    J: CheckpointStore,
    M: ModelPort,
    D: Dispatcher,
    V: Verifier,
{
    type Reply = ModelTurn;
    type ToolResult = ();
    fn reload(&self) -> Result<Snapshot, String> {
        let saved = self.journal.load()?;
        if saved.native.id.to_string() != self.id {
            return Err("checkpoint identity changed".into());
        }
        Ok(saved)
    }

    fn initialize<'a>(&'a mut self, _: &'a mut Context<'_>) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async { Ok(Flow::Continue) })
    }
    fn wants_plan(&self) -> bool { false }
    fn plan<'a>(&'a mut self, _: &'a mut Context<'_>) -> BoxFuture<'a, Result<Option<String>, String>> {
        Box::pin(async { Ok(None) })
    }
    fn prepare_model<'a>(&'a mut self, ctx: &'a mut Context<'_>) -> BoxFuture<'a, Result<Vec<Value>, String>> {
        Box::pin(async move {
            if ctx.snapshot.native.model_only || !ctx.budget.can_execute_tool() {
                return Ok(vec![]);
            }
            Ok(self.dispatcher.definitions().into_iter().filter(|definition| {
                definition.pointer("/function/name").and_then(Value::as_str)
                    .is_some_and(|name| runtime::policy::allowed_tool(&ctx.snapshot.native, name))
            }).collect())
        })
    }
    fn model<'a>(&'a mut self, ctx: &'a mut Context<'_>) -> BoxFuture<'a, Result<ModelTurn, String>> {
        Box::pin(async move {
            self.calls = ctx.snapshot.model_calls;
            let reply = self.model.decide(&ctx.snapshot).await?;
            self.tokens = self.tokens.saturating_add(reply.charged_tokens);
            // Engine persists this accounting when it settles the model intent.
            ctx.snapshot.charged_tokens = self.tokens;
            Ok(reply)
        })
    }
    fn observe_model<'a>(&'a mut self, _: &'a mut Context<'_>, reply: ModelTurn) -> BoxFuture<'a, Result<ModelDecision, String>> {
        Box::pin(async move {
            match reply.decision {
                TurnDecision::Answer(answer) if !answer.trim().is_empty() => {
                    self.answer = Some(answer);
                    Ok(ModelDecision::Verify)
                }
                TurnDecision::Answer(_) => Err("model returned an empty answer".into()),
                TurnDecision::Tools(actions) if !actions.is_empty() && actions.len() <= 16 => {
                    self.answer = None;
                    self.actions = actions;
                    Ok(ModelDecision::Tools)
                }
                TurnDecision::Tools(_) => Err("expected 1..16 tool calls".into()),
            }
        })
    }
    fn verify<'a>(&'a mut self, ctx: &'a mut Context<'_>) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async move {
            ctx.check_cancelled()?;
            let answer = self.answer.as_deref().ok_or("missing answer")?;
            let verdict = self.verifier.verify(&ctx.snapshot, answer).await?;
            ctx.check_cancelled()?;
            match verdict {
                Verification::Accept => {
                    ctx.snapshot.native.event("embedded_verified", "Host verifier accepted outcome", None);
                    ctx.journal.save(&ctx.snapshot)?;
                    Ok(Flow::Complete)
                }
                Verification::Retry(reason) => {
                    ctx.snapshot.native.event("embedded_retry", "Host verifier requested repair",
                        Some(json!({"reason":reason, "rejectedAnswer":answer})));
                    ctx.journal.save(&ctx.snapshot)?;
                    self.answer = None;
                    Ok(Flow::Continue)
                }
            }
        })
    }
    fn begin_tools<'a>(&'a mut self, ctx: &'a mut Context<'_>) -> BoxFuture<'a, Result<usize, String>> {
        Box::pin(async move {
            // An empty advertised catalog must never trigger the low-level
            // dispatcher's fallback catalog in the embedding path.
            if ctx.snapshot.native.model_only || ctx.snapshot.native.advertised_tools.is_empty() {
                return Err("no tools were advertised for this model turn".into());
            }
            // Reserve the whole batch before any effect. Rejected proposals
            // consume allowance too; they cannot be used for unbounded retries.
            if !ctx.budget.reserve_tools(self.actions.len()) {
                return Err("embedded tool budget exhausted".into());
            }
            Ok(1)
        })
    }
    fn prepare_tool<'a>(&'a mut self, _: &'a mut Context<'_>, _: usize) -> BoxFuture<'a, Result<bool, String>> {
        Box::pin(async { Ok(true) })
    }
    fn dispatch_tool<'a>(&'a mut self, ctx: &'a mut Context<'_>, _: usize) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            ctx.check_cancelled()?;
            // Core owns exact call IDs, schema admission, order and receipts.
            // Individual schema errors already have model-visible receipts.
            let _results = dispatch_batch(&mut ctx.snapshot, &self.actions, self.dispatcher, ctx.journal).await?;
            Ok(())
        })
    }
    fn observe_tool<'a>(&'a mut self, _: &'a mut Context<'_>, _: usize, _: ()) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
    fn verify_tools<'a>(&'a mut self, _: &'a mut Context<'_>) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async { Ok(Flow::Continue) })
    }
    fn exhausted(&mut self) -> Result<(), String> {
        Err("embedded round budget exhausted without verified answer".into())
    }
    fn completion(&self) -> Result<(String, usize, u64), String> {
        Ok((self.answer.clone().ok_or("missing verified answer")?, self.calls, self.tokens))
    }
}
