//! One budget ledger for model rounds, tool proposals and executed tools.
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct BudgetLedger {
    max_rounds: usize,
    max_tools: usize,
    max_proposals: usize,
    max_elapsed: Duration,
    rounds: usize,
    tools: usize,
    proposals: usize,
    started: Instant,
}

impl BudgetLedger {
    pub fn new(max_rounds: usize, max_tools: usize, max_elapsed: Duration) -> Self {
        Self {
            max_rounds,
            max_tools,
            // Blocked proposals must not silently consume executed-tool slots,
            // but they still need a bounded budget.
            max_proposals: max_tools.saturating_mul(2),
            max_elapsed,
            rounds: 0,
            tools: 0,
            proposals: 0,
            started: Instant::now(),
        }
    }

    pub fn charge_round(&mut self) -> bool {
        if self.rounds >= self.max_rounds || self.expired() {
            return false;
        }
        self.rounds += 1;
        true
    }

    pub fn charge_proposal(&mut self) -> bool {
        if self.proposals >= self.max_proposals || self.expired() {
            return false;
        }
        self.proposals += 1;
        true
    }

    pub fn charge_tool(&mut self) -> bool {
        if self.tools >= self.max_tools || self.expired() {
            return false;
        }
        self.tools += 1;
        true
    }

    /// A fresh ledger for one parallel branch. Counters start at zero so the
    /// parent can merge only the usage of this branch.
    pub fn branch(&self) -> Self {
        Self {
            max_rounds: self.max_rounds.saturating_sub(self.rounds),
            max_tools: self.max_tools.saturating_sub(self.tools),
            max_proposals: self.max_proposals.saturating_sub(self.proposals),
            max_elapsed: self.max_elapsed.saturating_sub(self.started.elapsed()),
            rounds: 0,
            tools: 0,
            proposals: 0,
            started: Instant::now(),
        }
    }

    /// Reserve *disjoint* work budgets before concurrent execution. Giving
    /// every child the entire remaining budget would overspend by fanout N.
    /// The shared wall-clock deadline is unchanged because branches overlap.
    pub fn parallel_shares(&self, count:usize)->Vec<Self>{
        if count==0 {return Vec::new();}
        let available_rounds=self.max_rounds.saturating_sub(self.rounds);
        let available_tools=self.max_tools.saturating_sub(self.tools);
        let available_proposals=self.max_proposals.saturating_sub(self.proposals);
        let left=self.max_elapsed.saturating_sub(self.started.elapsed());
        let split=|total:usize,index:usize|total/count+usize::from(index<total%count);
        (0..count).map(|index|Self{
            max_rounds:split(available_rounds,index),
            max_tools:split(available_tools,index),
            max_proposals:split(available_proposals,index),
            max_elapsed:left,
            rounds:0,tools:0,proposals:0,started:Instant::now(),
        }).collect()
    }

    /// The caller uses the exact reserved tool capacity to allocate a
    /// disjoint operation sequence range for each parallel child.
    pub fn tool_capacity(&self)->usize{self.max_tools}

    /// Adds the usage of a completed branch back into this ledger.
    pub fn merge_usage(&mut self, branch: &Self) {
        self.rounds = self.rounds.saturating_add(branch.rounds);
        self.tools = self.tools.saturating_add(branch.tools);
        self.proposals = self.proposals.saturating_add(branch.proposals);
    }

    pub fn can_execute_tool(&self) -> bool {
        self.tools < self.max_tools && !self.expired()
    }

    pub fn expired(&self) -> bool {
        self.started.elapsed() >= self.max_elapsed
    }

    pub fn snapshot(&self) -> Value {
        json!({
            "rounds": self.rounds,
            "maxRounds": self.max_rounds,
            "tools": self.tools,
            "maxTools": self.max_tools,
            "proposals": self.proposals,
            "maxProposals": self.max_proposals,
            "elapsedMs": self.started.elapsed().as_millis() as u64,
            "maxElapsedMs": self.max_elapsed.as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_fanout_cannot_multiply_remaining_executable_budget() {
        let mut parent=BudgetLedger::new(5,7,Duration::from_secs(12));
        assert!(parent.charge_round());
        assert!(parent.charge_tool());
        let mut shares=parent.parallel_shares(4);
        assert_eq!(shares.iter().map(|share|share.max_rounds).sum::<usize>(),4);
        assert_eq!(shares.iter().map(|share|share.max_tools).sum::<usize>(),6);
        assert_eq!(shares.iter().map(|share|share.max_proposals).sum::<usize>(),14);
        for share in &mut shares {
            for _ in 0..share.tool_capacity(){assert!(share.charge_tool());}
            assert!(!share.charge_tool(),"child cannot borrow another child's quota");
            while share.charge_round(){}
            while share.charge_proposal(){}
            parent.merge_usage(share);
        }
        assert_eq!(parent.snapshot()["tools"],7);
        assert_eq!(parent.snapshot()["rounds"],5);
        assert_eq!(parent.snapshot()["proposals"],14);
        assert_eq!(parent.parallel_shares(0).len(),0);
    }

    #[test]
    fn ledger_bounds_rounds_tools_and_proposals() {
        let mut ledger = BudgetLedger::new(2, 1, Duration::from_secs(60));
        assert!(ledger.charge_round());
        assert!(ledger.charge_round());
        assert!(!ledger.charge_round());
        assert!(ledger.charge_tool());
        assert!(!ledger.charge_tool());
        assert!(!ledger.can_execute_tool());
        assert!(ledger.charge_proposal());
        assert!(ledger.charge_proposal());
        assert!(!ledger.charge_proposal());
    }

    #[test]
    fn expired_kernel_budget_rejects_all_future_model_and_tool_admission() {
        let mut ledger=BudgetLedger::new(3,3,Duration::ZERO);
        assert!(ledger.expired());
        assert!(!ledger.charge_round());
        assert!(!ledger.charge_proposal());
        assert!(!ledger.charge_tool());
        assert!(!ledger.can_execute_tool());
        let branches=ledger.parallel_shares(3);
        assert_eq!(branches.len(),3);
        for mut share in branches {
            assert!(!share.charge_tool());
            assert!(!share.charge_round());
        }
        assert_eq!(ledger.snapshot()["tools"],0);
    }

    #[test]
    fn ledger_snapshot_exposes_limits() {
        let ledger = BudgetLedger::new(8, 40, Duration::from_secs(600));
        let snapshot = ledger.snapshot();
        assert_eq!(snapshot["maxRounds"], 8);
        assert_eq!(snapshot["maxTools"], 40);
        assert_eq!(snapshot["maxProposals"], 80);
    }
}
