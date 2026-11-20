//! Product adapter to the shared Dominion Rust execution kernel.
pub use dominion_agent_sdk::runtime::{BudgetLedger, ModelTokenBudget};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
    fn chat_and_native_model_calls_share_the_same_token_settlement_contract() {
        let mut tokens=ModelTokenBudget::new(250_000,245_000);
        assert!(tokens.can_admit(5_000));
        assert!(!tokens.charge(8_000));
        assert_eq!(tokens.charged(),253_000);
        assert_eq!(tokens.remaining(),0);
        assert!(tokens.exceeded());
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
