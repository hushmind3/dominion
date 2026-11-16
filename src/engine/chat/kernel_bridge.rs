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
        // This is an atomic field patch, not a stale full-record rewrite:
        // concurrent stream usage, provider route and cancellation updates
        // must survive a kernel checkpoint. A terminal run is never revived.
        let data=serde_json::to_value(snapshot).map_err(|error|error.to_string())?;
        self.state.store.patch_if(
            "execution",self.execution_id,"status",&json!("running"),
            &json!({"runtime":"dominion-agent-sdk","kernelState":data}),
        ).ok_or("Rust 코어 실행이 종료되었거나 체크포인트 저장에 실패했습니다.")?;
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
        // No stale full-record rewrite during shutdown/error unwinding.
        // Preserve a terminal execution's state and all concurrent metadata.
        self.state.store.patch_if(
            "execution",self.execution_id,"status",&json!("running"),
            &json!({"runtime":"dominion-agent-sdk",
                "budget":self.budget.snapshot()}),
        );
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use dominion_agent_sdk::{Journal, RunRequest, Snapshot};
    use serde_json::json;

    fn fixture() -> (AppState, std::path::PathBuf) {
        let dir=std::env::temp_dir().join(format!(
            "dominion-kernel-bridge-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=crate::config::Config{
            port:0,data_dir:dir.clone(),web_dir:dir.join("web"),
            workspace:dir.clone(),
        };
        let store=crate::db::Store::open(&config.db_path()).unwrap();
        let vault=crate::vault::Vault::open(&config.key_path());
        (AppState::new(config,store,vault),dir)
    }

    #[test]
    fn kernel_journal_merges_live_chat_fields_and_rejects_terminal_revival() {
        let (state,dir)=fixture();
        state.store.put("execution",&json!({
            "id":"shared-run","status":"running",
            "modelContinuity":{"responseId":"durable-1"},
            "usage":{"promptTokens":42}
        })).unwrap();
        let req:RunRequest=serde_json::from_value(json!({
            "id":"shared-run","instruction":"Inspect repository"
        })).unwrap();
        let mut snapshot=Snapshot::from_request(&req);
        snapshot.step=1;
        let generation=state.cancellation_generation("shared-session");
        let journal=ChatKernelJournal{
            state:&state,execution_id:"shared-run",
            session_id:"shared-session",generation,
        };
        Journal::save(&journal,&snapshot).unwrap();
        let saved=state.store.get("execution","shared-run").unwrap();
        assert_eq!(saved["kernelState"]["step"],1);
        assert_eq!(saved["modelContinuity"]["responseId"],"durable-1");
        assert_eq!(saved["usage"]["promptTokens"],42);
        state.store.patch_if("execution","shared-run","status",
            &json!("running"),&json!({"status":"cancelled"})).unwrap();
        snapshot.step=2;
        assert!(Journal::save(&journal,&snapshot).is_err());
        let final_record=state.store.get("execution","shared-run").unwrap();
        assert_eq!(final_record["status"],"cancelled");
        assert_eq!(final_record["kernelState"]["step"],1);
        drop(journal);
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn budget_drop_does_not_overwrite_a_terminal_execution() {
        let (state,dir)=fixture();
        state.store.put("execution",&json!({
            "id":"budget-run","status":"completed","answer":"finished",
            "modelCalls":3
        })).unwrap();
        {
            let _snapshot=KernelCheckpoint{
                state:&state,execution_id:"budget-run",
                budget:dominion_agent_sdk::runtime::BudgetLedger::new(
                    4,8,std::time::Duration::from_secs(60)),
            };
        }
        let persisted=state.store.get("execution","budget-run").unwrap();
        assert_eq!(persisted["status"],"completed");
        assert_eq!(persisted["answer"],"finished");
        assert!(persisted["budget"].is_null());
        drop(state);
        let _=std::fs::remove_dir_all(dir);
    }
}
