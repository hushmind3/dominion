//! Durable hierarchical admission, adapted from sub4's child reservations and
//! idempotent settlement. The host stores this tree in one SQLite transaction.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Unit {
    Tokens,
    UsdMicros,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub limit: u64,
    pub spent: u64,
    pub parent: Option<String>,
    pub children: BTreeMap<String, u64>,
    pub calls: BTreeMap<String, u64>,
    pub settled_calls: BTreeSet<String>,
    pub closing: bool,
    pub closed: bool,
    pub blocked: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tree {
    pub unit: Unit,
    pub accounts: BTreeMap<String, Account>,
}
impl Account {
    fn new(limit: u64, parent: Option<String>) -> Self {
        Self {
            limit,
            spent: 0,
            parent,
            children: BTreeMap::new(),
            calls: BTreeMap::new(),
            settled_calls: BTreeSet::new(),
            closing: false,
            closed: false,
            blocked: false,
        }
    }
    pub fn remaining(&self) -> u64 {
        self.limit
            .saturating_sub(self.spent)
            .saturating_sub(
                self.children
                    .values()
                    .fold(0u64, |sum, n| sum.saturating_add(*n)),
            )
            .saturating_sub(
                self.calls
                    .values()
                    .fold(0u64, |sum, n| sum.saturating_add(*n)),
            )
    }
}
impl Tree {
    pub fn new(root: &str, limit: u64, unit: Unit) -> Self {
        Self {
            unit,
            accounts: BTreeMap::from([(root.into(), Account::new(limit, None))]),
        }
    }
    pub fn account(&self, id: &str) -> Result<&Account, String> {
        self.accounts
            .get(id)
            .ok_or("예산 계정을 찾을 수 없습니다.".into())
    }
    fn active(&self, id: &str) -> Result<&Account, String> {
        let account = self.account(id)?;
        if account.closing || account.closed || account.blocked {
            return Err("종료되었거나 결과 확인이 필요한 예산입니다.".into());
        }
        Ok(account)
    }
    pub fn reserve_child(&mut self, parent: &str, child: &str, limit: u64) -> Result<(), String> {
        if self.accounts.contains_key(child) {
            return Err("중복 위임 예산입니다.".into());
        }
        if limit == 0 || limit > self.active(parent)?.remaining() {
            return Err("하위 작업에 예약할 부모 예산이 부족합니다.".into());
        }
        self.accounts
            .get_mut(parent)
            .unwrap()
            .children
            .insert(child.into(), limit);
        self.accounts
            .insert(child.into(), Account::new(limit, Some(parent.into())));
        Ok(())
    }
    pub fn reserve_call(&mut self, account: &str, call: &str, amount: u64) -> Result<(), String> {
        let a = self.active(account)?;
        if !a.calls.is_empty() {
            return Err("이 계정의 이전 모델 호출 결과를 먼저 확인하세요.".into());
        }
        if a.settled_calls.contains(call) || amount > a.remaining() {
            return Err("모델 호출 예산이 부족하거나 중복된 호출입니다.".into());
        }
        self.accounts
            .get_mut(account)
            .unwrap()
            .calls
            .insert(call.into(), amount);
        Ok(())
    }
    /// Observed provider overruns are recorded, never discarded. Saturating
    /// remaining admission stops the next call if an estimate was too small.
    pub fn settle_call(&mut self, account: &str, call: &str, observed: u64) -> Result<(), String> {
        let a = self
            .accounts
            .get_mut(account)
            .ok_or("예산 계정을 찾을 수 없습니다.")?;
        if a.settled_calls.contains(call) {
            return Ok(());
        }
        if a.calls.remove(call).is_none() {
            return Err("예약되지 않은 모델 비용입니다.".into());
        }
        a.spent = a.spent.saturating_add(observed);
        a.settled_calls.insert(call.into());
        self.settle_closed(account)
    }
    pub fn close(&mut self, account: &str, uncertain: bool) -> Result<(), String> {
        let a = self
            .accounts
            .get_mut(account)
            .ok_or("예산 계정을 찾을 수 없습니다.")?;
        a.closing = true;
        a.blocked |= uncertain;
        self.settle_closed(account)
    }
    fn settle_closed(&mut self, id: &str) -> Result<(), String> {
        let a = self.account(id)?;
        if a.closed || !a.closing || a.blocked || !a.children.is_empty() || !a.calls.is_empty() {
            return Ok(());
        }
        let (parent, spent) = (a.parent.clone(), a.spent);
        self.accounts.get_mut(id).unwrap().closed = true;
        if let Some(parent) = parent {
            let a = self
                .accounts
                .get_mut(&parent)
                .ok_or("부모 예산 계정을 찾을 수 없습니다.")?;
            if a.children.remove(id).is_some() {
                a.spent = a.spent.saturating_add(spent);
            }
            self.settle_closed(&parent)?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_reservations_prevent_parent_and_sibling_overspend() {
        let mut t = Tree::new("root", 100, Unit::Tokens);
        t.reserve_child("root", "a", 60).unwrap();
        assert!(t.reserve_child("root", "b", 50).is_err());
        assert!(t.reserve_call("root", "call", 41).is_err());
        t.reserve_child("root", "b", 40).unwrap();
        assert_eq!(t.account("root").unwrap().remaining(), 0);
    }
    #[test]
    fn settlement_is_idempotent_and_returns_only_unused_allowance() {
        let mut t = Tree::new("root", 100, Unit::UsdMicros);
        t.reserve_child("root", "a", 60).unwrap();
        t.reserve_call("a", "c", 50).unwrap();
        t.settle_call("a", "c", 20).unwrap();
        t.close("a", false).unwrap();
        t.close("a", false).unwrap();
        assert_eq!(t.account("root").unwrap().spent, 20);
        assert_eq!(t.account("root").unwrap().remaining(), 80);
    }
    #[test]
    fn uncertain_children_and_paid_calls_keep_reservations_after_restart() {
        let mut t = Tree::new("root", 100, Unit::Tokens);
        t.reserve_child("root", "a", 60).unwrap();
        t.reserve_call("a", "c", 50).unwrap();
        t.close("a", true).unwrap();
        let t: Tree = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(t.account("root").unwrap().remaining(), 40);
        assert_eq!(t.account("a").unwrap().remaining(), 10);
        assert!(t.account("a").unwrap().blocked);
    }
    #[test]
    fn root_waits_for_all_descendants_and_preserves_overruns() {
        let mut t = Tree::new("root", 100, Unit::Tokens);
        t.reserve_child("root", "a", 60).unwrap();
        t.reserve_child("a", "b", 40).unwrap();
        t.close("root", false).unwrap();
        t.close("a", false).unwrap();
        t.reserve_call("b", "c", 40).unwrap();
        t.settle_call("b", "c", 45).unwrap();
        t.close("b", false).unwrap();
        assert!(t.account("root").unwrap().closed);
        assert_eq!(t.account("root").unwrap().spent, 45);
        assert_eq!(t.account("a").unwrap().remaining(), 15);
    }
}
