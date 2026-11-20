//! Compatibility migrations run only after this process owns the listener.
//! Writes are verified before legacy records are removed; newer records win.
use crate::state::AppState;
use serde_json::{json, Value};

pub fn run(state: &AppState) -> Result<(), String> {
    provider_secrets(state)?;
    stale_leases(state)?;
    collection_names(state)?;
    retire_standalone_rsi(state)?;
    legacy_improvements(state)?;
    algorithm_statistics(state)?;
    interrupted_executions(state)?;
    interrupted_approvals(state)
}

/// A process that lost its listener cannot still be running work. Marking the
/// dead attempt terminal is what lets a later retry prove it is resuming an
/// interrupted run instead of racing a live one.
fn interrupted_executions(state: &AppState) -> Result<(), String> {
    for mut execution in state.store.try_all("execution")? {
        if execution["status"].as_str() != Some("running") {
            continue;
        }
        execution["status"] = json!("interrupted");
        execution["phase"] = json!("interrupted");
        execution["phaseUpdatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        execution["taskOutcome"] = json!("cancelled");
        execution["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
        execution["reason"] = json!("백엔드가 재시작되어 실행이 중단되었습니다.");
        state
            .store
            .put("execution", &execution)
            .ok_or("Cannot recover an interrupted execution")?;
    }
    for mut operation in state.store.try_all("execution_operation")? {
        if operation["status"].as_str() != Some("running") {
            continue;
        }
        operation["status"] = json!("interrupted");
        operation["outcomeUnknown"] = json!(true);
        operation["phase"] = json!("interrupted");
        operation["finishedAt"] = json!(chrono::Utc::now().to_rfc3339());
        operation["reason"] = json!("백엔드가 재시작되어 실행이 중단되었습니다.");
        state
            .store
            .put("execution_operation", &operation)
            .ok_or("Cannot recover an interrupted operation")?;
    }
    Ok(())
}

/// Only expired leases may be reclaimed. Another process can hold a valid
/// active read/write lease against this shared SQLite database, so its owner
/// must not be revoked during our startup.
fn stale_leases(state: &AppState) -> Result<(), String> {
    let released = state.store.release_stale_leases(&state.instance_id);
    if released > 0 {
        state.store.audit(
            "run_lease.recovered",
            json!({"released": released, "instance": state.instance_id}),
        );
    }
    Ok(())
}

/// One-time tombstone for the retired standalone RSI policy. This belongs
/// in migrations, not the live algorithm catalog's every-boot seed path.
/// Keep archived records for review without allowing execution or activation.
fn retire_standalone_rsi(state:&AppState)->Result<(),String>{
    let marker="retire-standalone-rsi-algorithms-v1";
    if state.store.get("migration",marker).is_some(){return Ok(());}
    let mut disabled=0usize;
    for mut old in state.store.try_all("algorithm")? {
        if old["kind"]!="selfimprove.search"||old["status"]=="disabled"{
            continue;
        }
        old["status"]=json!("disabled");
        old["retiredReason"]=json!("standalone improvement campaign removed");
        state.store.put("algorithm",&old)
            .ok_or("Could not disable obsolete RSI algorithm")?;
        disabled+=1;
    }
    state.store.put("migration",&json!({
        "id":marker,"kind":"retire_standalone_rsi","version":1,
        "disabled":disabled,"appliedAt":chrono::Utc::now().to_rfc3339()
    })).ok_or("Could not persist standalone RSI retirement marker")?;
    if disabled>0 {
        state.store.audit("migration.retire_standalone_rsi",
            json!({"disabled":disabled}));
    }
    Ok(())
}

/// One-time repair for algorithm counters polluted by unverified chat turns.
/// Everything is rebuilt from durable `algorithm_usage` evidence, so a rerun
/// after a crash cannot double-count.
fn algorithm_statistics(state: &AppState) -> Result<(), String> {
    let marker_id = "algorithm-stat-rebuild-v2";
    if state.store.get("migration", marker_id).is_some() {
        return Ok(());
    }
    let report = crate::engine::algorithms::rebuild_stats_from_usage(state)?;
    state
        .store
        .put(
            "migration",
            &json!({
                "id": marker_id,
                "kind": "algorithm_stat_rebuild",
                "version": 2,
                "appliedAt": chrono::Utc::now().to_rfc3339(),
                "report": report,
            }),
        )
        .ok_or("Cannot persist the algorithm statistic rebuild marker")?;
    state.store.audit(
        "migration.algorithm_stat_rebuild",
        json!({"report": report}),
    );
    Ok(())
}

/// Preserve old counters for display, but keep them out of the new score.
fn legacy_improvements(state: &AppState) -> Result<(), String> {
    for mut record in state.store.try_all("improvement")? {
        if record["scoreVersion"].as_u64().unwrap_or(0) >= 2 {
            continue;
        }
        let legacy = json!({
            "uses": record["uses"].as_u64().unwrap_or(0),
            "successes": record["successes"].as_u64().unwrap_or(0),
            "failures": record["failures"].as_u64().unwrap_or(0),
        });
        record["legacyCounters"] = legacy;
        record["scoreVersion"] = json!(1);
        if record["evidenceGrade"].is_null() {
            record["evidenceGrade"] = json!(if record["status"].as_str() == Some("validated") {
                "legacy_observed"
            } else {
                "legacy"
            });
        }
        state
            .store
            .put("improvement", &record)
            .ok_or_else(|| format!("cannot migrate improvement {}", record["id"]))?;
    }
    Ok(())
}
fn provider_secrets(state: &AppState) -> Result<(), String> {
    for provider in state.store.all("provider") {
        let Some(plaintext) = provider["apiKey"]
            .as_str()
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(id) = provider["id"].as_str() else {
            continue;
        };
        let name = format!("provider:{id}");
        if !state.store.has_secret(&name) {
            state.save_secret(&name, plaintext);
            if state.read_secret(&name).as_deref() != Some(plaintext) {
                return Err(format!(
                    "Cannot persist encrypted provider key for {id}; original record was preserved"
                ));
            }
        } else if state
            .read_secret(&name)
            .filter(|key| !key.is_empty())
            .is_none()
        {
            return Err(format!(
                "Cannot decrypt the existing provider key for {id}; original record was preserved"
            ));
        }
        let mut clean = provider.clone();
        clean
            .as_object_mut()
            .ok_or("Invalid provider record")?
            .remove("apiKey");
        state
            .store
            .put("provider", &clean)
            .ok_or_else(|| format!("Cannot remove the legacy plaintext provider field for {id}"))?;
        state
            .store
            .audit("migration.provider_key", json!({"providerId":id}));
    }
    Ok(())
}
fn payload(mut record: Value) -> Value {
    if let Some(map) = record.as_object_mut() {
        map.remove("createdAt");
        map.remove("updatedAt");
    }
    record
}
fn collection_names(state: &AppState) -> Result<(), String> {
    for (plural, singular) in [
        ("memories", "memory"),
        ("skills", "skill"),
        ("automations", "automation"),
        ("workflows", "workflow"),
        ("rooms", "room"),
        ("channels", "channel"),
        ("jobs", "job"),
        ("executions", "execution"),
        ("goals", "goal"),
    ] {
        for record in state.store.all(plural) {
            let Some(id) = record["id"].as_str() else {
                continue;
            };
            if let Some(current) = state.store.get(singular, id) {
                if payload(current) != payload(record.clone()) {
                    state.store.audit("migration.collection_conflict",json!({"legacyKind":plural,"currentKind":singular,"id":id,"legacyPreserved":true}));
                    continue;
                }
            } else {
                state.store.put(singular, &record).ok_or_else(|| {
                    format!("Cannot migrate {plural}/{id}; original record was preserved")
                })?;
            }
            if !state.store.remove(plural, id) && state.store.get(plural, id).is_some() {
                return Err(format!("Cannot finish migration of {plural}/{id}"));
            }
        }
    }
    Ok(())
}
fn interrupted_approvals(state: &AppState) -> Result<(), String> {
    for mut approval in state.store.all("approval") {
        if approval["status"] != "pending" {
            continue;
        }
        approval["status"] = json!("interrupted");
        // The live waiter died with the process. The decision can still be
        // recorded here and consumed once by an identical retry, but the
        // original operation is never restarted implicitly.
        approval["orphaned"] = json!(true);
        approval["reason"] = json!("백엔드가 재시작되어 실행이 중단되었습니다.");
        state
            .store
            .put("approval", &approval)
            .ok_or("Cannot recover an interrupted approval")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> (AppState, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("dominion-migrations-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = crate::config::Config {
            port: 0,
            data_dir: root.clone(),
            web_dir: root.join("web"),
            workspace: root.clone(),
        };
        let store = crate::db::Store::open(&config.db_path()).unwrap();
        let vault = crate::vault::Vault::open(&config.key_path());
        (AppState::new(config, store, vault), root)
    }
    #[test]
    fn migrations_preserve_newer_records_and_keep_legacy_conflicts_for_recovery() {
        let (state, dir) = state();
        state
            .store
            .put("memories", &json!({"id":"same","text":"old"}));
        state
            .store
            .put("memory", &json!({"id":"same","text":"new"}));
        state
            .store
            .put("memories", &json!({"id":"only-legacy","text":"retained"}));
        run(&state).unwrap();
        assert_eq!(state.store.get("memory", "same").unwrap()["text"], "new");
        assert_eq!(state.store.get("memories", "same").unwrap()["text"], "old");
        assert!(state.store.get("memories", "only-legacy").is_none());
        assert_eq!(
            state.store.get("memory", "only-legacy").unwrap()["text"],
            "retained"
        );
        crate::test_support::cleanup_dir(dir);
    }
    #[test]
    fn provider_migration_retains_existing_vault_value_and_is_idempotent() {
        let (state, dir) = state();
        state
            .store
            .put("provider", &json!({"id":"p","apiKey":"old-key"}));
        state.save_secret("provider:p", "current-key");
        state
            .store
            .put("approval", &json!({"id":"a","status":"pending"}));
        run(&state).unwrap();
        run(&state).unwrap();
        assert_eq!(
            state.read_secret("provider:p").as_deref(),
            Some("current-key")
        );
        assert!(state
            .store
            .get("provider", "p")
            .unwrap()
            .get("apiKey")
            .is_none());
        assert_eq!(
            state.store.get("approval", "a").unwrap()["status"],
            "interrupted"
        );
        crate::test_support::cleanup_dir(dir);
    }

    #[test]
    fn obsolete_rsi_algorithms_are_tombstoned_once_without_erasing_history(){
        let (state,dir)=state();
        state.store.put("algorithm",&json!({
            "id":"old-rsi","kind":"selfimprove.search",
            "status":"active","name":"old-autonomous-optimizer"
        })).unwrap();
        run(&state).unwrap();
        run(&state).unwrap();
        let historic=state.store.get("algorithm","old-rsi").unwrap();
        assert_eq!(historic["status"],"disabled");
        assert_eq!(historic["retiredReason"],
            "standalone improvement campaign removed");
        let marker=state.store.get("migration",
            "retire-standalone-rsi-algorithms-v1").unwrap();
        assert_eq!(marker["disabled"],1);
        assert!(crate::engine::algorithms::activate_checked(
            &state,"old-rsi",true).is_err());
        crate::test_support::cleanup_dir(dir);
    }

    #[test]
    fn legacy_improvement_counters_are_preserved_but_not_scored() {
        let (state, dir) = state();
        state.store.put(
            "improvement",
            &json!({
                "id":"legacy-1","kind":"strategy","status":"validated",
                "uses":7,"successes":5,"failures":2,"text":"old"
            }),
        );
        run(&state).unwrap();
        run(&state).unwrap();
        let stored = state.store.get("improvement", "legacy-1").unwrap();
        assert_eq!(stored["scoreVersion"], 1);
        assert_eq!(stored["legacyCounters"]["uses"], 7);
        assert_eq!(stored["evidenceGrade"], "legacy_observed");
        crate::test_support::cleanup_dir(dir);
    }
}
