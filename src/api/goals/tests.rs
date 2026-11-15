use super::*;

#[cfg(test)]
mod goal_resume_api_tests {
    use super::*;

    fn state() -> (AppState,std::path::PathBuf) {
        let dir=std::env::temp_dir().join(format!("dominion-resume-api-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let config=crate::config::Config {
            port:0,data_dir:dir.clone(),web_dir:dir.join("web"),workspace:dir.clone()
        };
        let store=crate::db::Store::open(&config.db_path()).unwrap();
        let vault=crate::vault::Vault::open(&config.key_path());
        (AppState::new(config,store,vault),dir)
    }

    #[tokio::test]
    async fn goal_creation_atomically_persists_runnable_job() {
        let (state,dir)=state();
        let response=create_goal(State(state.clone()),Json(json!({
            "goal":"edit two files and verify","maxIterations":4,
            "successCriteria":[{"type":"file_exists","path":"build.out"}]
        }))).await;
        assert_eq!(response.status(),StatusCode::OK);
        let goal=state.store.all("goal").into_iter().next().unwrap();
        let id=goal["id"].as_str().unwrap();
        let job_id=goal["jobId"].as_str().unwrap();
        assert_eq!(job_id,format!("goal-job-{id}"));
        let job=state.store.get("job",job_id).unwrap();
        assert_eq!(job["status"],"queued");
        assert_eq!(job["kind"],"goal");
        assert_eq!(job["input"]["goalId"],id);
        assert_eq!(goal["status"],"queued");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn resume_starts_at_completed_checkpoint_and_preserves_old_job() {
        let (state,dir)=state();
        state.store.put("session",&json!({"id":"session-1"})).unwrap();
        state.store.put("goal",&json!({
            "id":"goal-1","goal":"repair source","status":"needs_review",
            "phase":"needs_review","iterations":2,"maxIterations":4,
            "jobId":"job-old","sessionId":"session-1","planRevision":1,
            "appliedPlanRevision":1,"evidence":[{"tool":"file_read"}],
            "gaps":["external outcome unknown"]
        })).unwrap();
        state.store.put("job",&json!({
            "id":"job-old","kind":"goal","status":"needs_review",
            "input":{"goalId":"goal-1"},
            "goalState":{"nextIteration":2,"inFlight":true,
                "transcript":"two prior completed iterations"}
        })).unwrap();
        state.store.put("execution",&json!({
            "id":"run-old","sessionId":"session-1"
        })).unwrap();
        state.store.put("execution_operation",&json!({
            "id":"op-old","runId":"run-old","status":"interrupted",
            "outcomeUnknown":true
        })).unwrap();
        let accepted=resume_goal(State(state.clone()),Path("goal-1".to_string()),
            Json(json!({"acknowledgeUnknownEffects":true}))).await;
        assert_eq!(accepted.status(),StatusCode::OK);
        let updated=state.store.get("goal","goal-1").unwrap();
        assert_eq!(updated["status"],"queued");
        assert_eq!(updated["resumeCount"],1);
        assert_eq!(updated["jobId"],"goal-job-goal-1-resume-1");
        let new_job=state.store.get("job","goal-job-goal-1-resume-1").unwrap();
        assert_eq!(new_job["status"],"queued");
        assert_eq!(new_job["goalState"]["nextIteration"],3);
        assert_eq!(new_job["goalState"]["inFlight"],false);
        assert_eq!(new_job["input"]["resumeAfterReview"],true);
        assert_eq!(new_job["goalState"]["evidence"].as_array().unwrap().len(),1);
        assert_eq!(new_job["goalState"]["sessionId"],"session-1");
        assert_eq!(new_job["goalState"]["reviewedUnknownOpIds"][0],"op-old");
        let reviewed=vec!["op-old".to_string()];
        assert!(!state.store.session_has_uncertain_effects_except("session-1",&reviewed).unwrap());
        state.store.put("execution_operation",&json!({
            "id":"op-new","runId":"run-old","status":"running",
            "outcomeUnknown":true
        })).unwrap();
        assert!(state.store.session_has_uncertain_effects_except("session-1",&reviewed).unwrap());
        assert_eq!(state.store.get("job","job-old").unwrap()["goalState"]["inFlight"],true);
        let duplicate=resume_goal(State(state.clone()),Path("goal-1".to_string()),
            Json(json!({"acknowledgeUnknownEffects":true}))).await;
        assert_eq!(duplicate.status(),StatusCode::CONFLICT);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn review_ack_and_active_job_are_mandatory() {
        let (state,dir)=state();
        state.store.put("goal",&json!({
            "id":"g","goal":"inspect","status":"failed","iterations":0,
            "maxIterations":16
        })).unwrap();
        let missing_ack=resume_goal(State(state.clone()),Path("g".to_string()),
            Json(json!({}))).await;
        assert_eq!(missing_ack.status(),StatusCode::BAD_REQUEST);
        state.store.put("job",&json!({"id":"active","kind":"goal",
            "input":{"goalId":"g"},"status":"running"})).unwrap();
        let still_active=resume_goal(State(state.clone()),Path("g".to_string()),
            Json(json!({"acknowledgeUnknownEffects":true}))).await;
        assert_eq!(still_active.status(),StatusCode::CONFLICT);
        assert_eq!(state.store.get("goal","g").unwrap()["status"],"failed");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
