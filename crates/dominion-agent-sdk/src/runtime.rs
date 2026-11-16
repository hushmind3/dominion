//! Shared execution admission for native planners and product protocol adapters.
//! Provider wire formats, prompts, persistence and approvals belong to the host.
//! This kernel owns bounded rounds and tool admission, including parallel work.
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
struct Usage {
    rounds: usize,
    tools: usize,
    proposals: usize,
}

#[derive(Clone, Debug)]
pub struct BudgetLedger {
    max_rounds: usize,
    max_tools: usize,
    max_proposals: usize,
    max_elapsed: Duration,
    usage: Arc<Mutex<Usage>>,
    started: Instant,
    // Branch-local accounting is merged for compatibility with host frames;
    // admission always uses the shared account, never a copied allowance.
    local_rounds: usize,
    local_tools: usize,
    local_proposals: usize,
}

impl BudgetLedger {
    pub fn new(max_rounds: usize, max_tools: usize, max_elapsed: Duration) -> Self {
        Self { max_rounds, max_tools, max_proposals: max_tools.saturating_mul(2),
            max_elapsed, usage: Arc::new(Mutex::new(Usage::default())),
            started: Instant::now(), local_rounds: 0, local_tools: 0, local_proposals: 0 }
    }

    /// Restore durable charges without refunding consumption already
    /// admitted by another branch of this shared execution.
    /// Admission is monotonic even if a stale checkpoint is supplied.
    pub fn restore(&mut self, rounds: usize, tools: usize) {
        let mut usage = self.usage.lock().unwrap();
        usage.rounds = usage.rounds.max(rounds);
        usage.tools = usage.tools.max(tools);
        // Older host checkpoints record only rounds and tools. They provide
        // a proposal lower bound, not permission to rewind live proposals.
        usage.proposals = usage.proposals.max(tools);
        self.local_rounds = self.local_rounds.max(rounds);
        self.local_tools = self.local_tools.max(tools);
        self.local_proposals = self.local_proposals.max(tools);
    }

    /// Assign a unique round sequence while charging the shared budget.
    /// Reading before charging would race on parallel branches.
    pub fn next_round(&mut self) -> Option<usize> {
        let mut usage = self.usage.lock().unwrap();
        if usage.rounds >= self.max_rounds || self.expired() { return None; }
        let round = usage.rounds;
        usage.rounds += 1;
        self.local_rounds += 1;
        Some(round)
    }

    pub fn charge_round(&mut self) -> bool {
        let mut usage = self.usage.lock().unwrap();
        if usage.rounds >= self.max_rounds || self.expired() { return false; }
        usage.rounds += 1;
        self.local_rounds += 1;
        true
    }

    pub fn charge_proposal(&mut self) -> bool {
        let mut usage = self.usage.lock().unwrap();
        if usage.proposals >= self.max_proposals || self.expired() { return false; }
        usage.proposals += 1;
        self.local_proposals += 1;
        true
    }

    pub fn charge_tool(&mut self) -> bool { self.reserve_tools(1) }

    /// Reserve the complete batch before starting any member.
    pub fn reserve_tools(&mut self, count: usize) -> bool {
        let mut usage = self.usage.lock().unwrap();
        if count > self.max_tools.saturating_sub(usage.tools) || self.expired() { return false; }
        usage.tools += count;
        self.local_tools += count;
        true
    }

    pub fn branch(&self) -> Self {
        Self { local_rounds: 0, local_tools: 0, local_proposals: 0, ..self.clone() }
    }

    pub fn merge_usage(&mut self, branch: &Self) {
        assert!(Arc::ptr_eq(&self.usage, &branch.usage), "cannot merge unrelated executions");
        self.local_rounds = self.local_rounds.saturating_add(branch.local_rounds);
        self.local_tools = self.local_tools.saturating_add(branch.local_tools);
        self.local_proposals = self.local_proposals.saturating_add(branch.local_proposals);
    }

    pub fn can_execute_tool(&self) -> bool {
        !self.expired() && self.usage.lock().unwrap().tools < self.max_tools
    }

    pub fn expired(&self) -> bool { self.started.elapsed() > self.max_elapsed }

    pub fn snapshot(&self) -> Value {
        let usage = self.usage.lock().unwrap();
        json!({"rounds":usage.rounds,"maxRounds":self.max_rounds,
            "tools":usage.tools,"maxTools":self.max_tools,
            "proposals":usage.proposals,"maxProposals":self.max_proposals,
            "elapsedMs":self.started.elapsed().as_millis() as u64,
            "maxElapsedMs":self.max_elapsed.as_millis() as u64})
    }
}

/// Shared provider token admission for native jobs and ordinary chat.
///
/// Preflight works on an estimate before a paid call; settlement uses actual
/// provider usage (or a host estimate when exact telemetry is unavailable).
/// The charged total is monotonic even if the provider overruns the budget.
/// A model reply admitted on the estimate cannot dispatch effects when its
/// actual settlement exceeds the owner's ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelTokenBudget {
    limit: u64,
    charged: u64,
}
impl ModelTokenBudget {
    /// Recover the exact previously charged usage without resetting it.
    pub fn new(limit: u64, already_charged: u64) -> Self {
        Self { limit, charged: already_charged }
    }
    /// A conservative preflight authorization, not a promise that the
    /// provider will honor the estimate. Use `charge` after each response.
    pub fn can_admit(&self, estimated_additional: u64) -> bool {
        self.charged.saturating_add(estimated_additional) <= self.limit
    }
    /// Charge even an over-budget provider response for audit/recovery.
    /// Returning false forbids executing tools or accepting a final answer
    /// derived from that reply.
    pub fn charge(&mut self, actual_additional: u64) -> bool {
        self.charged = self.charged.saturating_add(actual_additional);
        self.charged <= self.limit
    }
    pub fn charged(&self) -> u64 { self.charged }
    pub fn remaining(&self) -> u64 { self.limit.saturating_sub(self.charged) }
    pub fn exceeded(&self) -> bool { self.charged > self.limit }
}

/// Shared classification for built-in host tools. A name not explicitly
/// known here is effectful by default, including every MCP tool. Only
/// inspection/search/GET/status functions may join a parallel read batch.
pub fn builtin_is_read_only(name:&str)->bool {
    matches!(name,
        "workspace_list" | "workspace_read" | "workspace_read_range" |
        "code_search" | "file_list" | "file_read" |
        "web_search" | "http_fetch" | "memory_search" |
        "task_status" | "task_result" | "account_status"
    )
}

/// Only adjacent reads may share a batch. An effect is an ordering barrier:
/// a later observation must never overtake the write it was meant to inspect.
pub fn read_batch_end<T>(calls: &[T], start: usize, max_parallel: usize,
    read_only: impl Fn(&T) -> bool) -> usize {
    let Some(first) = calls.get(start) else { return start; };
    let mut end = start + 1;
    if read_only(first) {
        while end < calls.len() && end - start < max_parallel.max(1)
            && read_only(&calls[end]) { end += 1; }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tool_effect_policy_is_consistent_and_fails_closed() {
        for name in [
            "workspace_list","workspace_read","workspace_read_range",
            "code_search","file_list","file_read","web_search","http_fetch",
            "memory_search","task_status","task_result","account_status"
        ] {
            assert!(builtin_is_read_only(name),"expected read-only: {name}");
        }
        for name in [
            "workspace_write","workspace_replace","shell_exec","terminal_exec",
            "delegate_task","github_api","mcp__server__search",
            "unrecognized_tool",""
        ] {
            assert!(!builtin_is_read_only(name),"unknown/effectful: {name}");
        }
    }

    #[test]
    fn shared_model_token_budget_reconciles_estimated_and_actual_usage() {
        let mut budget=ModelTokenBudget::new(10_000,0);
        assert!(budget.can_admit(3_000));
        assert!(budget.charge(4_000));
        assert_eq!(budget.remaining(),6_000);
        assert!(!budget.can_admit(6_001));
        assert!(budget.can_admit(6_000));
        assert!(!budget.charge(7_000),
            "provider usage overrun must be retained, but never authorized");
        assert_eq!(budget.charged(),11_000);
        assert_eq!(budget.remaining(),0);
        assert!(budget.exceeded());
        assert!(!budget.can_admit(0));
    }

    #[test]
    fn recovered_token_budget_never_refunds_reported_provider_usage() {
        let mut budget=ModelTokenBudget::new(2_000,1_900);
        assert!(budget.can_admit(100));
        assert!(!budget.can_admit(101));
        assert!(budget.charge(100));
        assert!(!budget.charge(1));
        assert_eq!(budget.charged(),2_001);
        let recovered=ModelTokenBudget::new(2_000,budget.charged());
        assert!(recovered.exceeded());
        assert_eq!(recovered.remaining(),0);
    }

    #[test]
    fn parallel_branches_share_one_admission_budget() {
        let mut root = BudgetLedger::new(8, 3, Duration::from_secs(60));
        let mut a = root.branch();
        let mut b = root.branch();
        assert!(a.reserve_tools(2));
        assert!(!b.reserve_tools(2));
        assert!(b.charge_tool());
        root.merge_usage(&a);
        root.merge_usage(&b);
        assert_eq!(root.snapshot()["tools"], 3);
        assert!(!root.charge_tool());
    }

    #[test]
    fn concurrent_branches_receive_unique_round_numbers_without_overspending() {
        let root = BudgetLedger::new(400, 1, Duration::from_secs(60));
        let threads = (0..8).map(|_| {
            let mut branch = root.branch();
            std::thread::spawn(move || {
                let mut rounds = Vec::new();
                while let Some(round) = branch.next_round() { rounds.push(round); }
                rounds
            })
        }).collect::<Vec<_>>();
        let mut all = threads.into_iter()
            .flat_map(|thread|thread.join().unwrap())
            .collect::<Vec<_>>();
        all.sort_unstable();
        assert_eq!(all, (0..400).collect::<Vec<_>>());
        assert_eq!(root.snapshot()["rounds"],400);
    }

    #[test]
    fn restore_cannot_refund_parallel_tool_or_proposal_usage() {
        let mut root = BudgetLedger::new(3, 3, Duration::from_secs(60));
        let mut branch = root.branch();
        assert!(branch.charge_tool());
        assert!(branch.charge_proposal());
        assert_eq!(branch.next_round(), Some(0));
        // A stale durable snapshot must never overwrite live shared charges.
        root.restore(0, 0);
        assert_eq!(root.snapshot()["tools"], 1);
        assert_eq!(root.snapshot()["rounds"], 1);
        assert_eq!(root.snapshot()["proposals"], 1);
        assert!(root.reserve_tools(2));
        assert!(!branch.charge_tool());
        assert!(branch.charge_proposal());
        root.restore(1, 1);
        assert_eq!(root.snapshot()["tools"], 3);
        assert_eq!(root.snapshot()["proposals"], 2);
    }

    #[test]
    fn restore_advances_but_does_not_regress_durable_admission() {
        let mut ledger = BudgetLedger::new(8, 8, Duration::from_secs(60));
        ledger.restore(4, 5);
        assert_eq!(ledger.snapshot()["rounds"], 4);
        assert_eq!(ledger.snapshot()["tools"], 5);
        assert_eq!(ledger.snapshot()["proposals"], 5);
        ledger.restore(1, 2);
        assert_eq!(ledger.snapshot()["rounds"], 4);
        assert_eq!(ledger.snapshot()["tools"], 5);
        assert_eq!(ledger.snapshot()["proposals"], 5);
        assert!(ledger.reserve_tools(3));
        assert!(!ledger.reserve_tools(1));
    }

    #[test]
    fn write_is_a_barrier_between_observations() {
        let calls = [false, true, true, false, true];
        assert_eq!(read_batch_end(&calls, 0, 4, |read| *read), 1);
        assert_eq!(read_batch_end(&calls, 1, 4, |read| *read), 3);
        assert_eq!(read_batch_end(&calls, 3, 4, |read| *read), 4);
        assert_eq!(read_batch_end(&calls, 4, 4, |read| *read), 5);
    }
}
