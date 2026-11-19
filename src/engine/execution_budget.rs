//! SQLite host adapter for sub4-style hierarchical budgets. Model transports
//! share this admission; no parallel agent runner or alternate store exists.
use crate::state::AppState;
use dominion_core::delegation::{Tree, Unit};
use serde_json::{json, Value};

fn binding(state: &AppState, session: &str) -> Option<Value> {
    state
        .store
        .get("budget_binding", session)
        .filter(|b| b["status"] == "active")
}
fn mutate<F>(state: &AppState, id: &str, f: F) -> Result<Value, String>
where
    F: FnOnce(&mut Tree, &mut Value) -> Result<Vec<(String, Value)>, String>,
{
    state
        .store
        .mutate_record("budget_tree", id, json!({"id":id}), |record| {
            let mut tree: Tree = serde_json::from_value(record["tree"].clone())
                .map_err(|e| format!("예산 기록 손상: {e}"))?;
            let extra = f(&mut tree, record)?;
            record["tree"] = serde_json::to_value(tree).map_err(|e| e.to_string())?;
            Ok(extra)
        })
}
fn requested(state: &AppState, session: &Value) -> Result<(u64, Unit, bool), String> {
    let owner = session["budgetOwnerId"].as_str();
    let prior = if let Some(owner) = owner {
        state
            .store
            .all("budget_tree")
            .into_iter()
            .filter(|row| row["ownerId"] == owner)
            .filter_map(|row| {
                let tree: Tree = serde_json::from_value(row["tree"].clone()).ok()?;
                let root = tree.account(row["id"].as_str()?).ok()?;
                Some(root.limit.saturating_sub(root.remaining()))
            })
            .fold(0u64, u64::saturating_add)
    } else {
        0
    };
    if let Some(usd) = session["budgetUsd"].as_f64() {
        if !usd.is_finite() || usd <= 0.0 || usd > 10000.0 {
            return Err("금액 예산은 0 초과 10000 USD 이하여야 합니다.".into());
        }
        return Ok((
            ((usd * 1_000_000.0).floor() as u64).saturating_sub(prior),
            Unit::UsdMicros,
            true,
        ));
    }
    let explicit = session["tokenBudget"].as_u64();
    Ok((
        explicit
            .unwrap_or(crate::engine::chat::MAX_TOKENS)
            .saturating_sub(if explicit.is_some() { prior } else { 0 }),
        Unit::Tokens,
        explicit.is_some(),
    ))
}
pub struct Execution {
    state: AppState,
    session: String,
    execution: String,
}
impl Execution {
    pub fn finish(&self) {
        let Some(b) =
            binding(&self.state, &self.session).filter(|b| b["executionId"] == self.execution)
        else {
            return;
        };
        let record = self
            .state
            .store
            .get("execution", &self.execution)
            .unwrap_or(Value::Null);
        let uncertain = record["kernelState"]["modelInFlight"] == true
            || !record["kernelState"]["native"]["in_flight"].is_null()
            || record["kernelState"]["native"]["read_batch_in_flight"]
                .as_array()
                .is_some_and(|calls| !calls.is_empty())
            // On DB error retain the reservation; releasing an uncertain
            // external effect without evidence would permit duplicate billing.
            || self.state.store.has_unknown_execution_effect(&self.execution)
                .unwrap_or(true);
        let result = mutate(
            &self.state,
            b["treeId"].as_str().unwrap_or(""),
            |tree, _| {
                let account = b["accountId"]
                    .as_str()
                    .ok_or("예산 계정 식별자가 없습니다.")?;
                tree.close(account, uncertain)?;
                let mut saved = b.clone();
                saved["status"] = json!(if tree.account(account)?.closed {
                    "closed"
                } else {
                    "waiting"
                });
                Ok(vec![("budget_binding".into(), saved)])
            },
        );
        if let Err(error) = result {
            self.state.store.audit(
                "budget.close_failed",
                json!({"executionId":self.execution,"error":error}),
            );
        }
    }
}
impl Drop for Execution {
    fn drop(&mut self) {
        self.finish();
    }
}
pub fn begin(state: &AppState, session: &Value, execution: &str) -> Result<Execution, String> {
    let id = session["id"].as_str().ok_or("세션 식별자가 없습니다.")?;
    if session["delegated"] == true
        && state
            .store
            .get("budget_binding", id)
            .is_some_and(|b| b["status"] == "closed")
    {
        return Err("종료된 자식 작업은 부모 예산을 새로 예약한 작업으로 시작하세요.".into());
    }
    let old = state.store.get("budget_binding", id).filter(|b| {
        session["delegated"] == true
            && matches!(
                b["status"].as_str(),
                Some("reserved" | "active" | "waiting")
            )
    });
    if let Some(mut b) = old {
        let tree_id = b["treeId"]
            .as_str()
            .ok_or("부모 예산 식별자가 없습니다.")?
            .to_string();
        b["executionId"] = json!(execution);
        b["status"] = json!("active");
        mutate(state, &tree_id, |tree, _| {
            let a = tree.account(
                b["accountId"]
                    .as_str()
                    .ok_or("자식 예산 식별자가 없습니다.")?,
            )?;
            if a.closed || a.blocked || !a.calls.is_empty() {
                return Err("재개 전 자식 작업 비용과 불명확한 결과를 확인하세요.".into());
            }
            Ok(vec![("budget_binding".into(), b)])
        })?;
    } else {
        let (limit, unit, limited) = requested(state, session)?;
        let tree = Tree::new(execution, limit, unit);
        state.store.transaction(&[
            ("budget_tree".into(),json!({"id":execution,"tree":tree,"limited":limited,"ownerId":session["budgetOwnerId"]})),
            ("budget_binding".into(),json!({"id":id,"treeId":execution,"accountId":execution,"executionId":execution,"status":"active"}))
        ])?;
    }
    if let Some(mut record) = state.store.get("execution", execution) {
        record["budgetBinding"] = state.store.get("budget_binding", id).unwrap_or(Value::Null);
        state
            .store
            .put("execution", &record)
            .ok_or("실행 예산 연결을 저장할 수 없습니다.")?;
    }
    Ok(Execution {
        state: state.clone(),
        session: id.into(),
        execution: execution.into(),
    })
}
/// Release a stopped tool hold only after durable operator observation. Paid
/// model reservations remain held until their own billing receipt is reconciled.
pub fn refresh_after_observation(state: &AppState, execution_id: &str) -> Result<(), String> {
    let run = state
        .store
        .get("execution", execution_id)
        .ok_or("실행 기록이 없습니다.")?;
    let session = run["sessionId"].as_str().ok_or("세션이 없습니다.")?;
    if state.active.lock().unwrap().contains(session) {
        return Err("실행 중인 예산은 복구할 수 없습니다.".into());
    }
    let Some(mut binding) = state
        .store
        .get("budget_binding", session)
        .filter(|b| b["executionId"] == execution_id)
    else {
        return Ok(());
    };
    let unknown = run["kernelState"]["modelInFlight"] == true
        || !run["kernelState"]["native"]["in_flight"].is_null()
        || run["kernelState"]["native"]["read_batch_in_flight"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        || state.store.has_unknown_execution_effect(execution_id)
            .map_err(|error|format!("미확정 실행 효과 조회 실패: {error}"))?;
    let tree_id = binding["treeId"]
        .as_str()
        .ok_or("예산 트리가 없습니다.")?
        .to_owned();
    let account = binding["accountId"]
        .as_str()
        .ok_or("예산 계정이 없습니다.")?
        .to_owned();
    mutate(state, &tree_id, |tree, _| {
        let blocked = unknown || !tree.account(&account)?.calls.is_empty();
        tree.accounts
            .get_mut(&account)
            .ok_or("예산 계정이 없습니다.")?
            .blocked = blocked;
        tree.close(&account, blocked)?;
        binding["status"] = json!(if tree.account(&account)?.closed {
            "closed"
        } else {
            "waiting"
        });
        Ok(vec![("budget_binding".into(), binding)])
    })?;
    Ok(())
}
/// Reserve before making the new job visible. Creation and the parent's
/// allowance move together; a rejected sibling cannot leave an orphan worker.
pub fn enqueue_child(
    state: &AppState,
    parent_session: &str,
    mut session: Value,
    mut job: Value,
    amount: Option<u64>,
) -> Result<Value, String> {
    let b = binding(state, parent_session).ok_or("부모 실행 예산이 없습니다.")?;
    let tree_id = b["treeId"].as_str().ok_or("부모 예산 식별자가 없습니다.")?;
    let child = session["id"]
        .as_str()
        .ok_or("자식 세션 식별자가 없습니다.")?
        .to_owned();
    let parent = b["accountId"]
        .as_str()
        .ok_or("부모 계정 식별자가 없습니다.")?;
    mutate(state, tree_id, |tree, _| {
        let available = tree.account(parent)?.remaining();
        let allowance = amount.unwrap_or_else(|| {
            (available / 2).min(if tree.unit == Unit::Tokens {
                50_000
            } else {
                available
            })
        });
        tree.reserve_child(parent, &child, allowance)?;
        let inherited = json!({"id":child,"treeId":tree_id,"accountId":child,"parentAccountId":parent,"status":"reserved"});
        session["delegationBudget"] =
            json!({"treeId":tree_id,"accountId":child,"unit":tree.unit,"limit":allowance});
        job["input"]["delegationBudget"] = session["delegationBudget"].clone();
        Ok(vec![
            ("session".into(), session),
            ("job".into(), job.clone()),
            ("budget_binding".into(), inherited),
        ])
    })?;
    state.bus.record("job", &job);
    Ok(job)
}
#[derive(Clone, Copy)]
struct Rates {
    input: f64,
    output: f64,
}
impl Rates {
    fn amount(self, input: u64, output: u64) -> u64 {
        ((input as f64 * self.input + output as f64 * self.output).ceil()).min(u64::MAX as f64)
            as u64
    }
}
fn rates(state: &AppState, provider: &str, model: &str, local: bool) -> Result<Rates, String> {
    let p = state.store.get("provider", provider).unwrap_or(Value::Null);
    let price = p["modelPricing"].get(model).unwrap_or(&p["pricing"]);
    match (price["inputUsdPerMillion"].as_f64(),price["outputUsdPerMillion"].as_f64()){
        (Some(input),Some(output)) if input.is_finite()&&output.is_finite()&&input>=0.0&&output>=0.0&&input<=10000.0&&output<=10000.0=>Ok(Rates{input,output}),
        (None,None) if local||p["free"]==true=>Ok(Rates{input:0.0,output:0.0}),
        _=>Err("금액 예산을 사용하려면 제공자 pricing.inputUsdPerMillion/outputUsdPerMillion을 설정하세요.".into())
    }
}
pub struct Call {
    state: AppState,
    session: String,
    tree: String,
    account: String,
    id: String,
    before: Value,
    rates: Option<Rates>,
    maximum: u64,
    finished: bool,
}
impl Call {
    /// Settles observed per-session usage from the authoritative provider
    /// audit, including planner/reviewer/CLI calls. No provider prices invented.
    pub fn completed_without_usage(mut self) -> Result<(), String> {
        mutate(&self.state, &self.tree, |tree, _| {
            tree.settle_call(&self.account, &self.id, self.maximum)?;
            Ok(vec![])
        })?;
        self.state.store.audit("budget.model_charge_estimated",json!({"treeId":self.tree,"accountId":self.account,
            "callId":self.id,"amount":self.maximum,"reason":"completed response without usable provider usage"}));
        self.finished = true;
        Ok(())
    }
    pub fn finish(mut self, success: bool, error: Option<&str>) -> Result<(), String> {
        let after = self.state.store.session_usage(&self.session);
        let input = after["promptTokens"]
            .as_u64()
            .unwrap_or(0)
            .saturating_sub(self.before["promptTokens"].as_u64().unwrap_or(0));
        let output = after["completionTokens"]
            .as_u64()
            .unwrap_or(0)
            .saturating_sub(self.before["completionTokens"].as_u64().unwrap_or(0));
        let total = after["totalTokens"]
            .as_u64()
            .unwrap_or(0)
            .saturating_sub(self.before["totalTokens"].as_u64().unwrap_or(0));
        if success
            && after["calls"].as_u64().unwrap_or(0) == self.before["calls"].as_u64().unwrap_or(0)
        {
            return self.completed_without_usage();
        }
        let no_generation = error.is_some_and(|e| {
            e.contains("[model_not_started]")
                || e.contains("HTTP 4")
                || e.contains("HTTP 5")
                || e.contains("Connection refused")
                || e.contains("tcp connect error")
        });
        if total == 0 && !success && !no_generation {
            return Ok(());
        }
        let observed = self.rates.map(|r| r.amount(input, output)).unwrap_or(total);
        mutate(&self.state, &self.tree, |tree, _| {
            tree.settle_call(&self.account, &self.id, observed)?;
            Ok(vec![])
        })?;
        self.finished = true;
        Ok(())
    }
}
impl Drop for Call {
    fn drop(&mut self) {
        if !self.finished {
            self.state.store.audit(
                "budget.model_outcome_unknown",
                json!({"treeId":self.tree,"accountId":self.account,"callId":self.id}),
            );
        }
    }
}
pub fn prepare_call(
    state: &AppState,
    session: &str,
    provider: &str,
    model: &str,
    local: bool,
    input: u64,
    max_output: u64,
) -> Result<(Option<Call>, u64), String> {
    let Some(b) = binding(state, session) else {
        return Ok((None, max_output));
    };
    let tree_id = b["treeId"]
        .as_str()
        .ok_or("예산 식별자가 없습니다.")?
        .to_owned();
    let account = b["accountId"]
        .as_str()
        .ok_or("예산 계정이 없습니다.")?
        .to_owned();
    let call_id = uuid::Uuid::new_v4().to_string();
    let snapshot = state
        .store
        .get("budget_tree", &tree_id)
        .ok_or("실행 예산 기록이 없습니다.")?;
    let unit: Unit =
        serde_json::from_value(snapshot["tree"]["unit"].clone()).map_err(|e| e.to_string())?;
    let rate = if unit == Unit::UsdMicros {
        Some(rates(state, provider, model, local)?)
    } else {
        None
    };
    let mut output = max_output.max(1);
    mutate(state, &tree_id, |tree, record| {
        let a = tree.account(&account)?;
        let limited = record["limited"] == true || a.parent.is_some() || !a.children.is_empty();
        let reserve = if limited {
            let available = a.remaining();
            if let Some(r) = rate {
                let input_cost = r.amount(input, 0);
                if input_cost > available {
                    return Err("금액 예산이 부족해 모델 호출 전에 중단했습니다.".into());
                }
                if r.output > 0.0 {
                    output =
                        output.min(((available - input_cost) as f64 / r.output).floor() as u64);
                }
                if output == 0 {
                    return Err("남은 금액 예산으로 출력을 생성할 수 없습니다.".into());
                }
                r.amount(input, output)
            } else {
                if input >= available {
                    return Err("토큰 예산이 부족해 모델 호출 전에 중단했습니다.".into());
                }
                output = output.min(available - input);
                input.saturating_add(output)
            }
        } else {
            0
        };
        tree.reserve_call(&account, &call_id, reserve)?;
        Ok(vec![])
    })?;
    let maximum = rate
        .map(|r| r.amount(input, output))
        .unwrap_or(input.saturating_add(output));
    Ok((
        Some(Call {
            state: state.clone(),
            session: session.into(),
            tree: tree_id,
            account,
            id: call_id,
            before: state.store.session_usage(session),
            rates: rate,
            maximum,
            finished: false,
        }),
        output,
    ))
}
pub fn finish_call(call: Option<Call>, success: bool, error: Option<&str>) -> Result<(), String> {
    if let Some(call) = call {
        call.finish(success, error)?;
    }
    Ok(())
}
pub fn close_unstarted_child(state: &AppState, session: &str) {
    let Some(b) = state
        .store
        .get("budget_binding", session)
        .filter(|b| b["status"] == "reserved")
    else {
        return;
    };
    let _ = mutate(state, b["treeId"].as_str().unwrap_or(""), |tree, _| {
        tree.close(b["accountId"].as_str().unwrap_or(""), false)?;
        let mut saved = b.clone();
        saved["status"] = json!("closed");
        Ok(vec![("budget_binding".into(), saved)])
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};
    fn state() -> AppState {
        let root =
            std::env::temp_dir().join(format!("dominion-tree-budget-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = Config {
            port: 0,
            data_dir: root.join("data"),
            web_dir: root.join("web"),
            workspace: root,
        };
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let store = Store::open(&config.db_path()).unwrap();
        let vault = Vault::open(&config.key_path());
        AppState::new(config, store, vault)
    }
    fn session(state: &AppState, id: &str, tokens: u64) -> Value {
        let s = json!({"id":id,"tokenBudget":tokens});
        state.store.put("session", &s).unwrap();
        s
    }
    fn child(id: &str) -> (Value, Value) {
        (
            json!({"id":id,"delegated":true,"parentSessionId":"parent"}),
            json!({"id":format!("job-{id}"),"status":"queued","kind":"prompt","input":{"sessionId":id,"parentSessionId":"parent"}}),
        )
    }
    #[test]
    fn sqlite_reserves_one_winner_without_orphan_jobs() {
        let state = state();
        let s = session(&state, "parent", 100);
        let execution = begin(&state, &s, "parent-run").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles = (0..2)
            .map(|i| {
                let state = state.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let (s, j) = child(&format!("c{i}"));
                    barrier.wait();
                    enqueue_child(&state, "parent", s, j, Some(60))
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .filter(Result::is_ok)
                .count(),
            1
        );
        assert_eq!(state.store.all("job").len(), 1);
        assert_eq!(state.store.all("session").len(), 2);
        drop(execution);
    }
    #[test]
    fn settled_children_cannot_refund_observed_usage_twice() {
        let state = state();
        let s = session(&state, "parent", 100);
        let parent = begin(&state, &s, "parent-run").unwrap();
        let (s, j) = child("c");
        enqueue_child(&state, "parent", s.clone(), j, Some(60)).unwrap();
        let worker = begin(&state, &s, "child-run").unwrap();
        let (call, output) = prepare_call(&state, "c", "p", "m", true, 10, 90).unwrap();
        assert_eq!(output, 50);
        state.store.audit(
            "model.usage",
            json!({"sessionId":"c","promptTokens":10,"completionTokens":5,"totalTokens":15}),
        );
        finish_call(call, true, None).unwrap();
        worker.finish();
        worker.finish();
        let t = state.store.get("budget_tree", "parent-run").unwrap();
        assert_eq!(t["tree"]["accounts"]["parent-run"]["spent"], 15);
        drop(worker);
        drop(parent);
    }
    #[test]
    fn provider_uses_configured_prices_and_reserves_before_the_call() {
        let state = state();
        let s = json!({"id":"parent","budgetUsd":0.001});
        state.store.put("session", &s);
        state.store.put(
            "provider",
            &json!({"id":"p","pricing":{"inputUsdPerMillion":2.0,"outputUsdPerMillion":4.0}}),
        );
        let execution = begin(&state, &s, "budget-case").unwrap();
        let (call, output) = prepare_call(&state, "parent", "p", "m", false, 100, 1000).unwrap();
        assert_eq!(output, 200);
        assert!(prepare_call(&state, "parent", "p", "m", false, 1, 1).is_err());
        state.store.audit("model.usage",json!({"sessionId":"parent","promptTokens":100,"completionTokens":50,"totalTokens":150}));
        finish_call(call, true, None).unwrap();
        assert_eq!(
            state.store.get("budget_tree", "budget-case").unwrap()["tree"]["accounts"]["budget-case"]["spent"],
            400
        );
        drop(execution);
    }
    #[test]
    fn unknown_call_remains_reserved_after_a_dropped_future() {
        let state = state();
        let s = session(&state, "parent", 100);
        let execution = begin(&state, &s, "run").unwrap();
        let (call, _) = prepare_call(&state, "parent", "p", "m", true, 10, 50).unwrap();
        drop(call);
        assert!(prepare_call(&state, "parent", "p", "m", true, 1, 1).is_err());
        execution.finish();
        let record = state.store.get("budget_tree", "run").unwrap();
        let tree: Tree = serde_json::from_value(record["tree"].clone()).unwrap();
        assert_eq!(tree.account("run").unwrap().remaining(), 40);
        assert!(!tree.account("run").unwrap().closed);
        drop(execution);
    }
    #[test]
    fn owner_observation_resolves_only_a_stopped_exact_paid_call_once() {
        let state = state();
        let s = session(&state, "parent", 100);
        let execution = begin(&state, &s, "run").unwrap();
        let (call, _) = prepare_call(&state, "parent", "p", "m", true, 10, 50).unwrap();
        drop(call);
        let binding = state.store.get("budget_binding", "parent").unwrap();
        state.store.put(
            "execution",
            &json!({"id":"run","sessionId":"parent","status":"interrupted","budgetBinding":binding,
            "kernelState":{"modelInFlight":true}}),
        );
        execution.finish();
        let tree = state.store.get("budget_tree", "run").unwrap();
        let id = tree["tree"]["accounts"]["run"]["calls"]
            .as_object()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone();
        let record = state.store.get("execution", "run").unwrap();
        let fixed = reconcile(
            &state,
            &record,
            &id,
            20,
            "provider receipt shows the request ended at 20 tokens",
        )
        .unwrap();
        assert_eq!(fixed["tree"]["accounts"]["run"]["spent"], 20);
        assert_eq!(fixed["tree"]["accounts"]["run"]["closed"], true);
        assert!(reconcile(&state, &record, &id, 0, "cannot refund twice").is_err());
        drop(execution);
    }
    #[test]
    fn observed_tool_releases_hold_but_unknown_model_cost_does_not() {
        let state = state();
        let s = session(&state, "parent", 100);
        let execution = begin(&state, &s, "run").unwrap();
        state.store.put("execution", &json!({"id":"run","sessionId":"parent","kernelState":{"native":{"in_flight":{"id":"tool"}}}}));
        execution.finish();
        assert_eq!(
            state.store.get("budget_binding", "parent").unwrap()["status"],
            "waiting"
        );
        state.store.put(
            "execution",
            &json!({"id":"run","sessionId":"parent","kernelState":{"native":{}}}),
        );
        refresh_after_observation(&state, "run").unwrap();
        refresh_after_observation(&state, "run").unwrap();
        assert_eq!(
            state.store.get("budget_binding", "parent").unwrap()["status"],
            "closed"
        );
        let s = session(&state, "other", 100);
        let execution = begin(&state, &s, "paid").unwrap();
        let (call, _) = prepare_call(&state, "other", "p", "m", true, 10, 50).unwrap();
        drop(call);
        state.store.put(
            "execution",
            &json!({"id":"paid","sessionId":"other","kernelState":{"modelInFlight":true}}),
        );
        execution.finish();
        refresh_after_observation(&state, "paid").unwrap();
        let row = state.store.get("budget_tree", "paid").unwrap();
        assert_eq!(row["tree"]["accounts"]["paid"]["closed"], false);
        assert_eq!(row["tree"]["accounts"]["paid"]["blocked"], true);
    }
    #[test]
    fn cancellation_cascades_and_refunds_only_unstarted_children() {
        let state = state();
        let s = session(&state, "parent", 100);
        let parent = begin(&state, &s, "root").unwrap();
        let (s, j) = child("c");
        enqueue_child(&state, "parent", s, j, Some(60)).unwrap();
        state.cancel_session("parent");
        assert!(state.is_cancelled("c"));
        assert_eq!(
            state.store.get("budget_binding", "c").unwrap()["status"],
            "closed"
        );
        let tree: Tree =
            serde_json::from_value(state.store.get("budget_tree", "root").unwrap()["tree"].clone())
                .unwrap();
        assert_eq!(tree.account("root").unwrap().remaining(), 100);
        drop(parent);
    }
}

/// Explicit owner observation for a stopped paid call. This is budget evidence,
/// not a fabricated provider bill or permission to replay a tool effect.
pub fn reconcile(
    state: &AppState,
    execution: &Value,
    call_id: &str,
    charged: u64,
    details: &str,
) -> Result<Value, String> {
    if execution["status"] == "running"
        || execution["sessionId"]
            .as_str()
            .is_some_and(|id| state.active.lock().unwrap().contains(id))
    {
        return Err("실행 중인 모델 비용은 수동 정산할 수 없습니다.".into());
    }
    if details.trim().is_empty() || details.len() > 4000 || charged > 1_000_000_000_000 {
        return Err("관측한 비용과 1~4000자의 근거를 입력하세요.".into());
    }
    let tree_id = execution["budgetBinding"]["treeId"]
        .as_str()
        .ok_or("실행 예산이 없습니다.")?;
    let account = execution["budgetBinding"]["accountId"]
        .as_str()
        .ok_or("예산 계정이 없습니다.")?;
    let other_unknown = !execution["kernelState"]["native"]["in_flight"].is_null()
        || execution["kernelState"]["native"]["read_batch_in_flight"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
        || state.store.has_unknown_execution_effect(
            execution["id"].as_str().ok_or("실행 식별자가 없습니다.")?
        ).map_err(|error|format!("미확정 실행 효과 조회 실패: {error}"))?;
    let saved = mutate(state, tree_id, |tree, _| {
        if !tree.account(account)?.calls.contains_key(call_id) {
            return Err("이 실행에서 불명확한 호출을 찾을 수 없습니다.".into());
        }
        tree.settle_call(account, call_id, charged)?;
        tree.accounts.get_mut(account).unwrap().blocked = other_unknown;
        tree.close(account, other_unknown)?;
        let mut record = execution.clone();
        if record["kernelState"].is_object() {
            record["kernelState"]["modelInFlight"] = json!(false);
        }
        record["budgetReconciliation"] = json!({"callId":call_id,"charged":charged,"unit":tree.unit,"details":details,"at":chrono::Utc::now().to_rfc3339()});
        Ok(vec![("execution".into(), record)])
    })?;
    state.store.audit(
        "budget.owner_reconciled",
        json!({"executionId":execution["id"],"callId":call_id,"charged":charged,"details":details}),
    );
    Ok(saved)
}
