//! Shared product budgets hosted by the native Rust harness.
//! Provider wire formats, prompts, persistence and approvals belong to the host.
//! Reservations are shared across delegated/product frames; copied limits never create extra allowance.
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

    /// Restore charges already committed by a durable host session.
    pub fn restore(&mut self, rounds: usize, tools: usize) {
        let mut usage = self.usage.lock().unwrap();
        usage.rounds = rounds;
        usage.tools = tools;
        usage.proposals = tools;
        self.local_rounds = rounds;
        self.local_tools = tools;
        self.local_proposals = tools;
    }

    pub fn next_round(&mut self) -> Option<usize> {
        let round = self.usage.lock().unwrap().rounds;
        self.charge_round().then_some(round)
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
    fn write_is_a_barrier_between_observations() {
        let calls = [false, true, true, false, true];
        assert_eq!(read_batch_end(&calls, 0, 4, |read| *read), 1);
        assert_eq!(read_batch_end(&calls, 1, 4, |read| *read), 3);
        assert_eq!(read_batch_end(&calls, 3, 4, |read| *read), 4);
        assert_eq!(read_batch_end(&calls, 4, 4, |read| *read), 5);
    }
}
