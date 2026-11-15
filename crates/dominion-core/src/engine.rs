//! Native execution driver, adapted from sub4's Engine::drive.
//!
//! The core owns planning → model → tools → verification → correction and
//! termination. Hosts supply product context, transports, effects and evidence;
//! they never own a model retry loop or the order of execution phases.
use crate::{
    budget::BudgetLedger,
    hosted::{BoxFuture, Journal, Snapshot},
};
use serde_json::{json, Value};
use std::future::Future;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Complete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelDecision {
    Verify,
    Tools,
}

pub struct Context<'j> {
    pub snapshot: Snapshot,
    pub budget: BudgetLedger,
    pub round: usize,
    pub journal: &'j dyn Journal,
}
impl Context<'_> {
    fn phase(&mut self, name: &str) -> Result<(), String> {
        self.snapshot.native.event(
            "phase",
            name,
            Some(json!({"phase":name,"round":self.round})),
        );
        self.journal.save(&self.snapshot)
    }
    fn check_cancelled(&self) -> Result<(), String> {
        if self.journal.cancelled(&self.snapshot.native.id.to_string()) {
            Err("실행이 중단되었습니다.".into())
        } else if self.budget.expired() {
            Err("실행 시간 제한을 초과했습니다.".into())
        } else {
            Ok(())
        }
    }
}

/// Product-specific behavior is phase-local. Only Engine advances/repeats it.
pub trait Host: Send {
    type Reply: Send;
    type ToolResult: Send;
    fn reload(&self) -> Result<Snapshot, String>;
    fn initialize<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Flow, String>>;
    fn wants_plan(&self) -> bool;
    fn plan<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Option<String>, String>>;
    fn prepare_model<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Vec<Value>, String>>;
    fn model<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Self::Reply, String>>;
    fn observe_model<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
        reply: Self::Reply,
    ) -> BoxFuture<'a, Result<ModelDecision, String>>;
    fn verify<'a>(&'a mut self, ctx: &'a mut Context<'_>) -> BoxFuture<'a, Result<Flow, String>>;
    fn begin_tools<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<usize, String>>;
    fn prepare_tool<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
        index: usize,
    ) -> BoxFuture<'a, Result<bool, String>>;
    fn dispatch_tool<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
        index: usize,
    ) -> BoxFuture<'a, Result<Self::ToolResult, String>>;
    fn observe_tool<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
        index: usize,
        result: Self::ToolResult,
    ) -> BoxFuture<'a, Result<(), String>>;
    fn verify_tools<'a>(
        &'a mut self,
        ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Flow, String>>;
    fn exhausted(&mut self) -> Result<(), String>;
    fn completion(&self) -> Result<(String, usize, u64), String>;
}

/// Gate an auxiliary paid call (review, adaptation or program repair) through
/// the same durable intent as the main model phase. Dropping the future keeps
/// intent in flight; a returned transport error still closes the attempt.
pub async fn model_call<T>(
    snapshot: &mut Snapshot,
    journal: &dyn Journal,
    phase: &str,
    call: impl Future<Output = T>,
) -> Result<T, String> {
    snapshot.native.event("model_call", phase, None);
    snapshot.begin_model(journal)?;
    let result = call.await;
    snapshot.finish_model(journal)?;
    Ok(result)
}

pub struct Engine;
impl Engine {
    /// This is the sole agent loop for every product entry point.
    pub async fn drive<H: Host>(host: &mut H, mut ctx: Context<'_>) -> Result<(), String> {
        ctx.journal.save(&ctx.snapshot)?;
        ctx.phase("initialize")?;
        let mut complete = host.initialize(&mut ctx).await? == Flow::Complete;
        if !complete && host.wants_plan() {
            ctx.snapshot = host.reload()?;
            ctx.check_cancelled()?;
            ctx.phase("planning")?;
            ctx.snapshot.begin_model(ctx.journal)?;
            let plan = host.plan(&mut ctx).await;
            if let Ok(Some(plan)) = &plan {
                ctx.snapshot.native.plan = Some(plan.clone());
            }
            ctx.snapshot.finish_model(ctx.journal)?;
            plan?;
        }
        while !complete {
            ctx.snapshot = host.reload()?;
            ctx.check_cancelled()?;
            let Some(round) = ctx.snapshot.next_round(&mut ctx.budget, ctx.journal)? else {
                host.exhausted()?;
                break;
            };
            ctx.round = round;
            ctx.phase("prepare_model")?;
            let catalog = host.prepare_model(&mut ctx).await?;
            ctx.snapshot = host.reload()?;
            ctx.snapshot.native.advertised_tools = catalog;
            ctx.phase("model")?;
            ctx.snapshot.begin_model(ctx.journal)?;
            let reply = host.model(&mut ctx).await;
            ctx.snapshot.finish_model(ctx.journal)?;
            let reply = reply?;
            let decision = host.observe_model(&mut ctx, reply).await?;
            ctx.snapshot = host.reload()?;
            match decision {
                ModelDecision::Verify => {
                    ctx.phase("verify")?;
                    complete = host.verify(&mut ctx).await? == Flow::Complete;
                }
                ModelDecision::Tools => {
                    ctx.phase("tools")?;
                    let count = host.begin_tools(&mut ctx).await?;
                    for index in 0..count {
                        ctx.check_cancelled()?;
                        if !host.prepare_tool(&mut ctx, index).await? {
                            continue;
                        }
                        let result = host.dispatch_tool(&mut ctx, index).await?;
                        host.observe_tool(&mut ctx, index, result).await?;
                        ctx.snapshot = host.reload()?;
                    }
                    ctx.phase("verify_tools")?;
                    complete = host.verify_tools(&mut ctx).await? == Flow::Complete;
                }
            }
        }
        ctx.snapshot = host.reload()?;
        let (answer, calls, tokens) = host.completion()?;
        ctx.snapshot.model_calls = ctx.snapshot.model_calls.max(calls);
        ctx.snapshot.charged_tokens = tokens;
        ctx.phase("complete")?;
        ctx.snapshot.complete(answer, ctx.journal)
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
