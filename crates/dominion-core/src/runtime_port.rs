//! Transport-neutral execution contracts and durable model-call admission.
//!
//! Budget, checkpoint, approval, cancellation and journal semantics are core.
//! The execution algorithm and phase sequencing live in dominion-algorithms.
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
    Continue,
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
    pub fn phase(&mut self, name: &str) -> Result<(), String> {
        self.snapshot.native.event(
            "phase",
            name,
            Some(json!({"phase":name,"round":self.round})),
        );
        self.journal.save(&self.snapshot)
    }
    pub fn check_cancelled(&self) -> Result<(), String> {
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
    /// Commit a verified, already-observed result without buying another model
    /// turn. Hosts must recheck owner revision, postconditions and durable tool
    /// receipts; a proposed plan or model claim cannot pass this barrier.
    fn reconcile_verified<'a>(
        &'a mut self,
        _ctx: &'a mut Context<'_>,
    ) -> BoxFuture<'a, Result<Flow, String>> {
        Box::pin(async { Ok(Flow::Continue) })
    }
    fn exhausted(&mut self) -> Result<(), String>;
    fn completion(&self) -> Result<(String, usize, u64), String>;
    fn confirm_completion(&mut self) -> Result<Flow, String> {
        Ok(Flow::Complete)
    }
}

/// Plug an iteration algorithm into the native runtime without granting it
/// extra tool authority. The algorithm crate implements this contract; the
/// core never depends on an algorithm or chooses one.
pub trait AgentLoop<H: Host>: Send + Sync {
    fn run<'a>(
        &'a self,
        host: &'a mut H,
        ctx: Context<'a>,
    ) -> BoxFuture<'a, Result<(), String>>;
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

