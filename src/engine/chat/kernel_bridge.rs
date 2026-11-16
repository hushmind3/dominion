//! Adapter joining the existing streaming chat host to the independent
//! Rust SDK. Only host persistence, approval-aware effect calls and resource
//! snapshots live here; the SDK owns the state-machine and write-ahead barrier.
use super::{Metrics,ToolCall,TurnRequest};
use super::runtime::dispatch_tool_call;
use crate::state::AppState;
use serde_json::{json,Value};
use tokio::sync::mpsc::UnboundedSender;

pub(super) struct ChatKernelJournal<'a> {
    pub(super) state: &'a AppState,
    pub(super) execution_id: &'a str,
    pub(super) session_id: &'a str,
    pub(super) generation: u64,
}
impl dominion_agent_sdk::Journal for ChatKernelJournal<'_> {
    fn save(&self, snapshot: &dominion_agent_sdk::Snapshot) -> Result<(), String> {
        let mut execution = self.state.store.get("execution", self.execution_id)
            .ok_or("Rust 코어 실행 기록이 없습니다.")?;
        execution["runtime"] = json!("dominion-agent-sdk");
        execution["kernelState"] = serde_json::to_value(snapshot).map_err(|error| error.to_string())?;
        self.state.store.put("execution", &execution)
            .ok_or("Rust 코어 체크포인트를 저장할 수 없습니다.")?;
        Ok(())
    }
    fn cancelled(&self, _: &str) -> bool {
        self.state.is_cancelled(self.session_id)
            || self.state.cancellation_generation(self.session_id) != self.generation
    }
}

pub(super) struct ChatKernelDispatcher<'a> {
    pub(super) state: &'a AppState,
    pub(super) tx: &'a UnboundedSender<Value>,
    pub(super) session: &'a Value,
    pub(super) request: &'a TurnRequest,
    pub(super) session_id: &'a str,
    pub(super) execution_id: &'a str,
    pub(super) metrics: &'a std::sync::Mutex<Metrics>,
    pub(super) prepared: &'a [(usize, u64, Value)],
    pub(super) calls: &'a [ToolCall],
    pub(super) round: usize,
    pub(super) compat: bool,
}
impl dominion_agent_sdk::Dispatcher for ChatKernelDispatcher<'_> {
    type Output = (Value, bool, String);
    fn execute<'a>(&'a self, action: &'a dominion_agent_sdk::Action)
        -> dominion_agent_sdk::BoxFuture<'a, Result<dominion_agent_sdk::DispatchResult<Self::Output>, String>> {
        Box::pin(async move {
            let (index, seq, _) = self.prepared.iter().find(|(index, _, _)|
                action.id == format!("{}:{}", self.round, self.calls[*index].id))
                .ok_or("Rust 코어 도구 호출의 원문을 찾을 수 없습니다.")?;
            let output = dispatch_tool_call(
                self.state, self.tx, self.session, self.request, self.session_id,
                self.execution_id, &self.calls[*index], action.arguments.clone(),
                self.metrics, *seq, self.round, *index, self.compat, "model", None, None, None,
            ).await?;
            Ok(dominion_agent_sdk::DispatchResult {
                success: output.1, observation: output.0.clone(), output,
            })
        })
    }
    fn is_read_only(&self, action: &dominion_agent_sdk::Action) -> bool {
        dominion_agent_sdk::runtime::builtin_is_read_only(&action.tool)
    }
    fn max_parallel_reads(&self) -> usize { 4 }
}

pub(super) struct KernelCheckpoint<'a> {
    pub(super) state: &'a AppState,
    pub(super) execution_id: &'a str,
    pub(super) budget: dominion_agent_sdk::runtime::BudgetLedger,
}
impl Drop for KernelCheckpoint<'_> {
    fn drop(&mut self) {
        if let Some(mut execution) = self.state.store.get("execution", self.execution_id) {
            execution["runtime"] = json!("dominion-agent-sdk");
            execution["budget"] = self.budget.snapshot();
            self.state.store.put("execution", &execution);
        }
    }
}

