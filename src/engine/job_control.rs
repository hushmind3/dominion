//! Revisioned owner input for the one native driver. This module stores
//! directives and completion gates; it never runs another model/tool loop.
use crate::state::AppState;
use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct Owner {
    pub kind: &'static str,
    pub id: String,
}
#[derive(Debug)]
pub enum SteeringError {
    BadInput(String),
    NotFound,
    Conflict(String),
}
impl std::fmt::Display for SteeringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadInput(s) | Self::Conflict(s) => f.write_str(s),
            Self::NotFound => f.write_str("작업을 찾을 수 없습니다."),
        }
    }
}
impl Owner {
    pub fn from_session(state: &AppState, session: &Value) -> Option<Self> {
        for (field, kind) in [("nativeJobId", "job"), ("nativeSdkRunId", "sdk_run")] {
            if let Some(id) = session[field].as_str() {
                if state
                    .store
                    .get(kind, id)
                    .is_some_and(|r| matches!(r["status"].as_str(), Some("queued" | "running")))
                {
                    return Some(Self {
                        kind,
                        id: id.into(),
                    });
                }
            }
        }
        None
    }
    pub fn record(&self, state: &AppState) -> Result<Value, String> {
        state
            .store
            .get(self.kind, &self.id)
            .ok_or("작업 지시 기록이 없습니다.".into())
    }
    pub fn revision(&self, state: &AppState) -> Result<u64, String> {
        Ok(self.record(state)?["steeringRevision"]
            .as_u64()
            .unwrap_or(0))
    }
    pub fn close(&self, state: &AppState, revision: u64) -> Result<bool, String> {
        let row = self.record(state)?;
        if row["steeringRevision"].as_u64().unwrap_or(0) != revision {
            return Ok(false);
        }
        if row["steeringGate"]["closed"] == true {
            return Ok(true);
        }
        Ok(state
            .store
            .patch_if(
                self.kind,
                &self.id,
                "steeringGate",
                row.get("steeringGate").unwrap_or(&Value::Null),
                &json!({"steeringGate":{"revision":revision,"closed":true}}),
            )
            .is_some())
    }
}
pub fn intent(base: &str, row: &Value) -> String {
    let mut goal = base.to_string();
    for directive in row["steering"].as_array().into_iter().flatten() {
        if let Some(text) = directive["instruction"].as_str() {
            if directive["mode"] == "replace" {
                goal = text.into();
            } else {
                goal.push_str("\n추가 지시:\n");
                goal.push_str(text);
            }
        }
    }
    goal
}
pub fn steer(
    state: &AppState,
    kind: &'static str,
    id: &str,
    instruction: &str,
    mode: &str,
) -> Result<Value, SteeringError> {
    let instruction = instruction.trim();
    if !matches!(mode, "append" | "replace") || instruction.is_empty() || instruction.len() > 4000 {
        return Err(SteeringError::BadInput(
            "지시는 1~4000자, 방식은 append 또는 replace입니다.".into(),
        ));
    }
    for _ in 0..4 {
        let row = state.store.get(kind, id).ok_or(SteeringError::NotFound)?;
        if kind == "job" && row["kind"] != "prompt" {
            return Err(SteeringError::BadInput(
                "일반 프롬프트 작업에서만 지시를 변경할 수 있습니다.".into(),
            ));
        }
        if !matches!(row["status"].as_str(), Some("queued" | "running"))
            || row["steeringGate"]["closed"] == true
        {
            return Err(SteeringError::Conflict(
                "완료·중단이 확정된 작업입니다. 새 작업으로 지시하세요.".into(),
            ));
        }
        let mut directives = row["steering"].as_array().cloned().unwrap_or_default();
        if directives.len() >= 8 {
            return Err(SteeringError::Conflict(
                "작업당 지시 변경은 최대 8회입니다.".into(),
            ));
        }
        let revision = row["steeringRevision"].as_u64().unwrap_or(0) + 1;
        directives.push(json!({"revision":revision,"mode":mode,"instruction":instruction,"at":chrono::Utc::now().to_rfc3339()}));
        if let Some(saved)=state.store.patch_if(kind,id,"steeringGate",row.get("steeringGate").unwrap_or(&Value::Null),
            &json!({"steering":directives,"steeringRevision":revision,"steeringGate":{"revision":revision,"closed":false}})){
            state.bus.record(kind,&saved);
            state.store.audit("job.owner_steering",json!({"kind":kind,"id":id,"revision":revision,"mode":mode}));
            return Ok(json!({"id":id,"revision":revision,"appliesAt":"next_native_boundary"}));
        }
    }
    Err(SteeringError::Conflict(
        "작업 지시가 동시에 수정되었습니다.".into(),
    ))
}
pub struct Scope {
    state: AppState,
    session: String,
    job: String,
    previous: Value,
}
impl Drop for Scope {
    fn drop(&mut self) {
        let _ = self.state.store.mutate_record(
            "session",
            &self.session,
            json!({"id":self.session}),
            |session| {
                if session["nativeJobId"] != self.job {
                    return Ok(vec![]);
                }
                for key in [
                    "nativeJobId",
                    "ownerAppliedRevision",
                    "tokenBudget",
                    "budgetUsd",
                    "budgetOwnerId",
                    "jobTokenBaseline",
                    "nativeJobVerifyCommand",
                    "shellSandbox",
                ] {
                    match self.previous.get(key) {
                        Some(value) => {
                            session[key] = value.clone();
                        }
                        None => {
                            session.as_object_mut().unwrap().remove(key);
                        }
                    }
                }
                Ok(vec![])
            },
        );
    }
}
pub fn configure(
    state: &AppState,
    job: &Value,
    session_id: &str,
) -> Result<(Scope, String), String> {
    let id = job["id"].as_str().ok_or("작업 식별자가 없습니다.")?;
    let latest = state.store.get("job", id).ok_or("작업 기록이 없습니다.")?;
    let input = &latest["input"];
    let previous = state
        .store
        .get("session", session_id)
        .ok_or("작업 세션이 없습니다.")?;
    let prior_owner = previous["nativeJobId"].as_str().map(str::to_owned);
    if let Some(owner) = prior_owner.as_deref().filter(|owner| *owner != id) {
        if state
            .store
            .get("job", owner)
            .is_some_and(|j| j["status"] == "running")
        {
            return Err("deferred: 세션의 이전 작업이 실행 중입니다.".into());
        }
    }
    let baseline = state.store.session_usage(session_id)["totalTokens"]
        .as_u64()
        .unwrap_or(0);
    let saved = state
        .store
        .mutate_record("job", id, latest.clone(), |record| {
            if record["sessionTokenBaseline"].as_u64().is_none() {
                record["sessionTokenBaseline"] = json!(baseline);
            }
            if !record["sessionControlPrevious"].is_object() {
                let mut controls = json!({});
                for key in [
                    "nativeJobId",
                    "ownerAppliedRevision",
                    "tokenBudget",
                    "budgetUsd",
                    "budgetOwnerId",
                    "jobTokenBaseline",
                    "nativeJobVerifyCommand",
                    "shellSandbox",
                ] {
                    if let Some(value) = previous.get(key) {
                        controls[key] = value.clone();
                    }
                }
                record["sessionControlPrevious"] = controls;
            }
            Ok(vec![])
        })?;
    let workspace = input["workspaceDir"]
        .as_str()
        .map(|raw| super::workspace_scope::resolve(state, raw))
        .transpose()?;
    state
        .store
        .mutate_record("session", session_id, previous.clone(), |session| {
            if session["nativeJobId"].as_str() != prior_owner.as_deref() {
                return Err("deferred: 세션의 작업 소유자가 변경되었습니다.".into());
            }
            session["nativeJobId"] = json!(id);
            session["ownerAppliedRevision"] = saved["steeringRevision"].clone();
            session["budgetOwnerId"] = json!(format!("job:{id}"));
            session["jobTokenBaseline"] = saved["sessionTokenBaseline"].clone();
            if let Some(path) = workspace {
                session["workspaceDir"] = json!(path);
            }
            if let Some(object)=session.as_object_mut() {
                if !input["tokenBudget"].is_null() { object.remove("budgetUsd"); }
                if !input["budgetUsd"].is_null() { object.remove("tokenBudget"); }
            }
            for key in ["tokenBudget", "budgetUsd", "shellSandbox"] {
                if !input[key].is_null() {
                    session[key] = input[key].clone();
                }
            }
            if let Some(command) = input["verifyCommand"].as_str() {
                session["nativeJobVerifyCommand"] = json!(command);
            }
            Ok(vec![])
        })?;
    let goal = intent(input["prompt"].as_str().unwrap_or(""), &saved);
    Ok((
        Scope {
            state: state.clone(),
            session: session_id.into(),
            job: id.into(),
            previous: saved["sessionControlPrevious"].clone(),
        },
        goal,
    ))
}

pub fn options(state: &AppState, body: &Value) -> Result<Value, String> {
    let mut out = json!({});
    if !body["tokenBudget"].is_null() && !body["budgetUsd"].is_null() {
        return Err("tokenBudget 또는 budgetUsd 중 하나를 선택하세요.".into());
    }
    if !body["tokenBudget"].is_null() {
        let amount = body["tokenBudget"]
            .as_u64()
            .filter(|n| (1024..=2_000_000).contains(n))
            .ok_or("토큰 예산은 1024~2000000입니다.")?;
        out["tokenBudget"] = json!(amount);
    }
    if !body["budgetUsd"].is_null() {
        let amount = body["budgetUsd"]
            .as_f64()
            .filter(|n| n.is_finite() && *n > 0.0 && *n <= 10000.0)
            .ok_or("금액 예산은 0 초과 10000 USD 이하여야 합니다.")?;
        out["budgetUsd"] = json!(amount);
    }
    if !body["workspaceDir"].is_null() {
        let raw = body["workspaceDir"]
            .as_str()
            .ok_or("작업 폴더는 절대경로 문자열이어야 합니다.")?;
        out["workspaceDir"] = json!(super::workspace_scope::resolve(state, raw)?);
    }
    if !body["shellSandbox"].is_null() {
        let mode = body["shellSandbox"]
            .as_str()
            .filter(|m| matches!(*m, "required" | "off"))
            .ok_or("shellSandbox는 required 또는 off입니다.")?;
        out["shellSandbox"] = json!(mode);
    }
    if !body["verifyCommand"].is_null() {
        let command = body["verifyCommand"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.len() <= 2048)
            .ok_or("검증 명령은 1~2048자입니다.")?;
        out["verifyCommand"] = json!(command);
    }
    Ok(out)
}

/// Structured append preserves earlier mandatory checks even when the new
/// owner instruction is informational or prohibits unrelated side effects.
pub fn initial_acceptance(
    state: &AppState,
    session: &Value,
    input: &str,
) -> Option<(String, Value)> {
    let owner = Owner::from_session(state, session)?;
    let row = owner.record(state).ok()?;
    let applied = session["ownerAppliedRevision"].as_u64().unwrap_or(0);
    let base = if owner.kind == "job" {
        row["input"]["prompt"].as_str()?
    } else {
        row["instruction"].as_str()?
    };
    let mut goal = base.to_string();
    let mut checks = crate::engine::chat::prepare_owner_action(state, base)
        .map(|(_, v)| v)
        .unwrap_or(json!([]));
    for d in row["steering"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| d["revision"].as_u64().unwrap_or(0) <= applied)
    {
        let text = d["instruction"].as_str()?;
        let extra = crate::engine::chat::prepare_owner_action(state, text)
            .map(|(_, v)| v)
            .unwrap_or(json!([]));
        if d["mode"] == "replace" {
            goal = text.into();
            checks = extra;
        } else {
            goal.push_str("\n추가 지시:\n");
            goal.push_str(text);
            for check in extra.as_array().into_iter().flatten() {
                if !checks.as_array()?.contains(check) {
                    checks.as_array_mut()?.push(check.clone());
                }
            }
        }
    }
    if goal != input {
        return crate::engine::chat::prepare_owner_action(state, input);
    }
    (!checks.as_array()?.is_empty()).then_some((goal, checks))
}

pub fn recover_scopes(state: &AppState) {
    for session in state.store.all("session") {
        let Some(id) = session["id"].as_str() else {
            continue;
        };
        let Some(job_id) = session["nativeJobId"].as_str() else {
            continue;
        };
        let Some(job) = state.store.get("job", job_id) else {
            continue;
        };
        if job["status"] == "running" {
            continue;
        }
        drop(Scope {
            state: state.clone(),
            session: id.into(),
            job: job_id.into(),
            previous: job["sessionControlPrevious"].clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, db::Store, vault::Vault};
    fn state() -> AppState {
        let root =
            std::env::temp_dir().join(format!("dominion-owner-control-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = Config {
            port: 0,
            data_dir: root.join("data"),
            web_dir: root.join("web"),
            workspace: root,
        };
        std::fs::create_dir_all(&config.data_dir).unwrap();
        AppState::new(
            config.clone(),
            Store::open(&config.db_path()).unwrap(),
            Vault::open(&config.key_path()),
        )
    }
    #[test]
    fn owner_append_replace_and_completion_share_one_compare_and_swap_gate() {
        let state = state();
        state.store.put(
            "job",
            &json!({"id":"j","kind":"prompt","status":"running","input":{"prompt":"original"}}),
        );
        steer(&state, "job", "j", "extra", "append").unwrap();
        let owner = Owner {
            kind: "job",
            id: "j".into(),
        };
        assert!(!owner.close(&state, 0).unwrap());
        assert_eq!(
            intent("original", &owner.record(&state).unwrap()),
            "original\n추가 지시:\nextra"
        );
        steer(&state, "job", "j", "replacement", "replace").unwrap();
        assert_eq!(
            intent("original", &owner.record(&state).unwrap()),
            "replacement"
        );
        assert!(owner.close(&state, 2).unwrap());
        assert!(steer(&state, "job", "j", "late", "append").is_err());
        assert_eq!(
            state.store.get("job", "j").unwrap()["input"]["prompt"],
            "original"
        );
    }
    #[test]
    fn completed_or_cancelled_jobs_cannot_be_steered() {
        let state = state();
        for status in ["cancelled", "failed", "completed"] {
            state.store.put(
                "job",
                &json!({"id":"j","kind":"prompt","status":status,"input":{"prompt":"original"}}),
            );
            assert!(steer(&state, "job", "j", "change", "replace").is_err());
        }
    }
    #[test]
    fn job_scope_restores_session_controls_and_keeps_its_durable_baseline() {
        let state = state();
        state.store.put(
            "session",
            &json!({"id":"s","tokenBudget":9999,"budgetUsd":2.0,"shellSandbox":"off"}),
        );
        let job = json!({"id":"j","kind":"prompt","status":"running","input":{"prompt":"explain","tokenBudget":2048,"shellSandbox":"required"}});
        state.store.put("job", &job);
        state
            .store
            .audit("model.usage", json!({"sessionId":"s","totalTokens":30}));
        let (scope, _) = configure(&state, &job, "s").unwrap();
        assert_eq!(
            state.store.get("session", "s").unwrap()["tokenBudget"],
            2048
        );
        assert!(state.store.get("session","s").unwrap()["budgetUsd"].is_null());
        drop(scope);
        assert_eq!(state.store.get("session","s").unwrap()["budgetUsd"],2.0);
        assert_eq!(
            state.store.get("session", "s").unwrap()["tokenBudget"],
            9999
        );
        state
            .store
            .audit("model.usage", json!({"sessionId":"s","totalTokens":20}));
        let (scope, _) = configure(&state, &job, "s").unwrap();
        assert_eq!(
            state.store.get("job", "j").unwrap()["sessionTokenBaseline"],
            30
        );
        drop(scope);
    }
    #[test]
    fn prior_native_turns_and_child_reservations_cannot_renew_a_job_allowance() {
        let state = state();
        state.store.put("session", &json!({"id":"s"}));
        let job = json!({"id":"j","kind":"prompt","status":"running","input":{"prompt":"explain","tokenBudget":2048}});
        state.store.put("job", &job);
        let (scope, _) = configure(&state, &job, "s").unwrap();
        let s = state.store.get("session", "s").unwrap();
        let first = crate::engine::execution_budget::begin(&state, &s, "first").unwrap();
        let (call, _) =
            crate::engine::execution_budget::prepare_call(&state, "s", "p", "m", true, 100, 100)
                .unwrap();
        state.store.audit(
            "model.usage",
            json!({"sessionId":"s","promptTokens":100,"completionTokens":100,"totalTokens":200}),
        );
        crate::engine::execution_budget::finish_call(call, true, None).unwrap();
        drop(first);
        drop(scope);
        let (scope, _) = configure(&state, &job, "s").unwrap();
        let s = state.store.get("session", "s").unwrap();
        let second = crate::engine::execution_budget::begin(&state, &s, "second").unwrap();
        assert_eq!(
            state.store.get("budget_tree", "second").unwrap()["tree"]["accounts"]["second"]
                ["limit"],
            1848
        );
        drop(second);
        drop(scope);
    }
    #[test]
    fn selected_workspace_survives_control_cleanup_but_internal_data_is_rejected() {
        let state = state();
        let project = state.config.workspace.join("other-project");
        std::fs::create_dir(&project).unwrap();
        assert!(options(&state, &json!({"workspaceDir":state.config.data_dir})).is_err());
        let opts = options(&state, &json!({"workspaceDir":project,"tokenBudget":2048})).unwrap();
        state.store.put("session", &json!({"id":"s"}));
        let job = json!({"id":"j","kind":"prompt","status":"running","input":{"prompt":"inspect","workspaceDir":opts["workspaceDir"]}});
        state.store.put("job", &job);
        let (scope, _) = configure(&state, &job, "s").unwrap();
        drop(scope);
        assert_eq!(
            state.store.get("session", "s").unwrap()["workspaceDir"],
            opts["workspaceDir"]
        );
    }
}
