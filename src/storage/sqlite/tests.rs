//! Unit tests for the stable SQLite Store and legacy records contract.
    use super::*;

    fn temp_store() -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dominion-db-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("dominion.sqlite")).unwrap();
        (store, dir)
    }

    #[test]
    fn failed_restore_snapshot_does_not_disconnect_or_truncate_live_database() {
        let (store, dir) = temp_store();
        store.put("mcp", &json!({"id":"saved-mcp","name":"Keep me"})).unwrap();
        assert!(store.replace_database_from_file(&dir.join("missing.sqlite")).is_err());
        assert_eq!(store.get("mcp", "saved-mcp").unwrap()["name"], "Keep me");

        let invalid = dir.join("invalid.sqlite");
        let mut header_only = vec![0u8; 4096];
        header_only[..16].copy_from_slice(b"SQLite format 3\\0");
        std::fs::write(&invalid, header_only).unwrap();
        assert!(Store::validate_restore_snapshot_file(&invalid).is_err());
        assert!(store.replace_database_from_file(&invalid).is_err());
        assert_eq!(store.get("mcp", "saved-mcp").unwrap()["name"], "Keep me");
        store.put("mcp", &json!({"id":"still-writable","name":"Yes"})).unwrap();
        assert_eq!(store.get("mcp", "still-writable").unwrap()["name"], "Yes");
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn restored_snapshot_is_complete_and_reopened_with_saved_contents() {
        let (store, dir) = temp_store();
        store.put("mcp", &json!({"id":"old","name":"Old"})).unwrap();
        let source = Store::open(&dir.join("source.sqlite")).unwrap();
        source.put("mcp", &json!({"id":"new","name":"New"})).unwrap();
        let snapshot = dir.join("snapshot.sqlite");
        source.backup_to(&snapshot).unwrap();
        store.replace_database_from_file(&snapshot).unwrap();
        assert!(store.get("mcp", "old").is_none());
        assert_eq!(store.get("mcp", "new").unwrap()["name"], "New");
        drop(source);
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn one_kind_algorithm_lookup_uses_partial_expression_index() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for n in 0..150 {
            let kind=if n%10==0 {"adaptation.selection"}else{"memory.rerank"};
            store.put("algorithm",&json!({"id":format!("policy-{n:03}"),
                "kind":kind,"implementation":"test","status":"available"
            })).unwrap();
        }
        store.put("algorithm_stat",&json!({"id":"historical-noise",
            "kind":"adaptation.selection","uses":9999999})).unwrap();
        let rows=store.algorithms_for_kind("adaptation.selection").unwrap();
        assert_eq!(rows.len(),15);
        assert!(rows.iter().all(|row|row["kind"]=="adaptation.selection"));
        let conn=store.conn.lock().unwrap();
        let plan:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT data,id,created_at,updated_at
             FROM records INDEXED BY idx_algorithm_kind_lookup
             WHERE kind='algorithm' AND json_extract(data,'$.kind')=?1 ORDER BY id",
            ["adaptation.selection"],|row|row.get(3)).unwrap();
        assert!(plan.contains("idx_algorithm_kind_lookup"),"{plan}");
    }

    #[test]
    fn native_route_usage_is_indexed_precise_and_excludes_invalid_jobs() {
        let (store,dir)=temp_store();
        for i in 0..150 {
            let route=if i%3==0 {"selected"}else{"unrelated"};
            store.put("job",&json!({"id":format!("job-{i}"),
                "status":"completed","nativeRoute":{"binding":route},
                "nativeModelUsage":{"cacheReportedCalls":1,"unreportedCalls":0,
                    "inputTokens":10000,"cacheReadTokens":8000,"cacheWriteTokens":0}})).unwrap();
        }
        store.put("job",&json!({"id":"unfinished","status":"running",
            "nativeRoute":{"binding":"selected"},
            "nativeModelUsage":{"cacheReportedCalls":1,"inputTokens":10000,
                "cacheReadTokens":8000}})).unwrap();
        store.put("job",&json!({"id":"unreported","status":"completed",
            "nativeRoute":{"binding":"selected"},
            "nativeModelUsage":{"cacheReportedCalls":1,"unreportedCalls":1,
                "inputTokens":10000,"cacheReadTokens":8000}})).unwrap();
        store.put("job",&json!({"id":"malformed","status":"completed",
            "nativeRoute":{"binding":"selected"},
            "nativeModelUsage":{"cacheReportedCalls":"1","unreportedCalls":0,
                "inputTokens":"10000"}})).unwrap();
        assert_eq!(store.native_route_cache_aggregate("selected").unwrap(),
            (50,50,500000,400000,0));
        assert_eq!(store.native_route_cache_aggregate("absent").unwrap(),
            (0,0,0,0,0));
        let conn=store.conn.lock().unwrap();
        let plan:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM records WHERE kind='job'
             AND json_extract(data,'$.status')='completed'
             AND json_extract(data,'$.nativeRoute.binding')=?1",
            ["selected"],|r|r.get(3)).unwrap();
        assert!(plan.contains("idx_native_job_route_completed"),"{plan}");
        drop(conn);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn indexed_execution_timeline_pages_without_loading_unrelated_json() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for i in 0..120 {
            let status=if i%11==0 {"running"} else {"completed"};
            let started_at=format!("2026-11-17T{:02}:{:02}:00Z",i/60,i%60);
            store.put("execution",&json!({"id":format!("run-{i:03}"),
                "startedAt":started_at,"status":status,
                "evidence":format!("fixture-{i}")})).unwrap();
        }
        let page=store.execution_page(7,5).unwrap();
        assert_eq!(page.len(),7);
        assert_eq!(page[0]["id"],"run-114");
        assert_eq!(page[6]["id"],"run-108");
        assert!(store.execution_page(10,200).unwrap().is_empty());
        let active=store.active_executions_indexed().unwrap();
        assert_eq!(active.len(),11);
        assert!(active.iter().all(|record|record["status"]=="running"));
        let conn=store.conn.lock().unwrap();
        let page_plan:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT data FROM records INDEXED BY idx_execution_page
             WHERE kind='execution'
             ORDER BY COALESCE(json_extract(data,'$.startedAt'),created_at) DESC,
                      updated_at DESC,id DESC LIMIT 7 OFFSET 5",[],|r|r.get(3)).unwrap();
        assert!(page_plan.contains("idx_execution_page"),"{page_plan}");
        let active_plan:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT data FROM records
             WHERE kind='execution' AND json_extract(data,'$.status')='running'
             ORDER BY updated_at DESC,id DESC",[],|r|r.get(3)).unwrap();
        assert!(active_plan.contains("idx_execution_active_status"),"{active_plan}");
    }

    #[test]
    fn indexed_task_recovery_reads_only_the_owners_execution_history() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for i in 0..200 {
            let task=if i==23 || i==107 {"requested-task"}else{"other-task"};
            store.put("execution",&json!({
                "id":format!("execution-{i:03}"),"taskId":task,
                "status":"interrupted","programResumeApproved":true,
                "programCheckpoint":{"programRevision":"p1"}
            })).unwrap();
        }
        let selected=store.task_executions("requested-task").unwrap();
        assert_eq!(selected.len(),2);
        assert!(selected.iter().all(|item|item["taskId"]=="requested-task"));
        assert_eq!(store.task_executions("absent-task").unwrap().len(),0);
        let conn=store.conn.lock().unwrap();
        let plan:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT data FROM records WHERE kind='execution'
             AND json_extract(data,'$.taskId')=?1",
            ["requested-task"],|row|row.get(3)).unwrap();
        assert!(plan.contains("idx_execution_task"),"{plan}");
    }

    #[test]
    fn indexed_operation_views_ignore_unrelated_history_and_preserve_recent_order() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for i in 0..300 {
            let run=if i==80||i==120 {"run-focus"}else{"run-other"};
            let session=if i==80||i==120 {"session-focus"}else{"session-other"};
            store.put("execution_operation",&json!({
                "id":format!("op-{i:03}"),"runId":run,"sessionId":session,
                "opSeq":i,"status":"completed","tool":"workspace_read",
                "result":{"content":"x".repeat(1000)}
            })).unwrap();
        }
        let run=store.execution_operations_recent("run-focus").unwrap();
        let session=store.session_execution_operations("session-focus").unwrap();
        assert_eq!(run.len(),2);
        assert_eq!(session.len(),2);
        assert!(run.iter().all(|op|op["runId"]=="run-focus"));
        assert!(session.iter().all(|op|op["sessionId"]=="session-focus"));
        assert_eq!(store.execution_operations_recent("absent").unwrap().len(),0);
        let conn=store.conn.lock().unwrap();
        for (sql,key,index_name) in [
            ("EXPLAIN QUERY PLAN SELECT data FROM records WHERE kind='execution_operation' AND json_extract(data,'$.runId')=?1","run-focus","idx_operation_run"),
            ("EXPLAIN QUERY PLAN SELECT data FROM records WHERE kind='execution_operation' AND json_extract(data,'$.sessionId')=?1","session-focus","idx_operation_session")
        ] {
            let explain:String=conn.query_row(sql,[key],|row|row.get(3)).unwrap();
            assert!(explain.contains(index_name),"{explain}");
        }
    }

    #[test]
    fn indexed_active_job_view_excludes_terminated_history_and_keeps_order(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for i in 0..500 {
            let status=if i==10||i==20 {"queued"}else if i==30 {"running"}else{"completed"};
            store.put("job",&json!({"id":format!("job-{i:03}"),
                "kind":"prompt","status":status,
                "queuedAt":format!("2026-11-16T{:04}Z",i)})).unwrap();
        }
        let active=store.active_jobs().unwrap();
        assert_eq!(active.len(),3);
        assert_eq!(active[0]["id"],"job-010");
        assert_eq!(active[1]["id"],"job-020");
        assert_eq!(active[2]["id"],"job-030");
        let conn=store.conn.lock().unwrap();
        let explain:String=conn.query_row(
            "EXPLAIN QUERY PLAN SELECT data FROM records WHERE kind='job'
             AND json_extract(data,'$.status') IN ('queued','running')",
            [],|row|row.get(3)).unwrap();
        assert!(explain.contains("idx_job_active_status"),"{explain}");
    }

    #[test]
    fn run_leases_are_exclusive_and_recover_after_crash() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert!(store.acquire_lease("session:s", "owner-a", 60_000).is_some());
        assert!(
            store.acquire_lease("session:s", "owner-b", 60_000).is_none(),
            "a live lease must refuse a second owner"
        );
        assert!(store
            .acquire_lease("session:s", "owner-a", 60_000)
            .is_some());
        assert!(store.renew_lease("session:s", "owner-b", 60_000).is_none());
        assert!(store.renew_lease("session:s", "owner-a", 60_000).is_some());

        // Another instance's live lease stays active; expired leases are
        // reclaimed without revoking an unrelated worker.
        assert!(store.acquire_lease("session:t", "dead", -1).is_some());
        let released = store.release_stale_leases("new-instance");
        assert_eq!(released, 1, "do not revoke a live foreign owner");
        assert_eq!(store.active_lease("session:s").unwrap()["owner"], "owner-a");
        assert!(store.acquire_lease("session:s", "owner-c", 60_000).is_none());
        assert!(store.release_lease("session:s", "owner-a"));
        assert!(store.acquire_lease("session:s", "owner-c", 60_000).is_some());
    }


    #[test]
    fn stolen_lease_cannot_be_removed_or_renewed_by_its_old_owner() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert!(store.acquire_lease("session:writer", "old-owner", -1).is_some());
        assert!(store.acquire_lease("session:writer", "new-owner", 120_000).is_some());
        assert!(!store.release_lease("session:writer", "old-owner"));
        assert!(store.renew_lease("session:writer", "old-owner", 120_000).is_none());
        assert_eq!(store.active_lease("session:writer").unwrap()["owner"], "new-owner");
        assert_eq!(store.release_stale_leases("another-process"), 0);
        assert_eq!(store.active_lease("session:writer").unwrap()["owner"], "new-owner");
        assert!(store.release_lease("session:writer", "new-owner"));
        assert!(store.active_lease("session:writer").is_none());
    }

    #[test]
    fn live_multi_connection_lease_is_never_revoked_by_startup_sweep() {
        let root = std::env::temp_dir().join(format!(
            "dominion-cross-process-lease-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("leases.sqlite");
        let first = Store::open(&path).unwrap();
        let second = Store::open(&path).unwrap();
        assert!(first.acquire_lease("session:team-writer", "first:worker", 120_000).is_some());
        assert_eq!(second.release_stale_leases("second:instance"), 0);
        assert!(second.acquire_lease("session:team-writer", "second:worker", 120_000).is_none());
        assert!(first.renew_lease("session:team-writer", "first:worker", 120_000).is_some());
        assert!(first.release_lease("session:team-writer", "first:worker"));
        assert!(second.acquire_lease("session:team-writer", "second:worker", 120_000).is_some());
        assert!(!first.release_lease("session:team-writer", "first:worker"));
        assert_eq!(second.active_lease("session:team-writer").unwrap()["owner"], "second:worker");
        drop(second);
        drop(first);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn expired_owner_cannot_heartbeat_itself_back_to_active() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert!(store.acquire_lease("session:expired", "old", -1).is_some());
        assert!(store.renew_lease("session:expired", "old", 120_000).is_none());
        assert_eq!(store.release_stale_leases("other"), 1);
        assert!(store.active_lease("session:expired").is_none());
    }


    #[test]
    fn workspace_access_preserves_parallel_readers_and_excludes_writers_across_connections(){
        let dir=std::env::temp_dir().join(format!(
            "dominion-rw-cross-process-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path=dir.join("shared.sqlite");
        let left=Store::open(&path).unwrap();
        let right=Store::open(&path).unwrap();
        let scope="/test/workspace-a";
        assert!(left.acquire_workspace_access("r1","read-one",60_000,scope,"read").is_some());
        assert!(right.acquire_workspace_access("r2","read-two",60_000,scope,"read").is_some());
        assert!(left.acquire_workspace_access("w1","writer-one",60_000,scope,"write").is_none());
        // A different workspace never inherits another project's locks.
        assert!(right.acquire_workspace_access("w2","writer-two",60_000,
            "/test/workspace-b","write").is_some());
        assert!(left.release_lease("r1","read-one"));
        assert!(right.acquire_workspace_access("w1","writer-one",60_000,scope,"write").is_none());
        assert!(right.release_lease("r2","read-two"));
        assert!(left.acquire_workspace_access("w1","writer-one",60_000,scope,"write").is_some());
        assert!(right.acquire_workspace_access("r3","read-three",60_000,scope,"read").is_none());
        assert!(left.acquire_workspace_access("w4","writer-four",60_000,scope,"write").is_none());
        assert!(!right.release_lease("w1","not-the-owner"));
        assert!(left.release_lease("w1","writer-one"));
        assert!(right.acquire_workspace_access("r3","read-three",60_000,scope,"read").is_some());
        drop(right);drop(left);let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn expired_workspace_writer_frees_readers_without_replaying_old_owner(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        assert!(store.acquire_workspace_access("old","dead",-1,"workspace","write").is_none());
        assert!(store.acquire_workspace_access("old","dead",1,"workspace","write").is_some());
        // Explicitly force expiry through an owner-fenced update to avoid
        // wall-clock sleeps in CI.
        let past=chrono::Utc::now().timestamp_millis()-1;
        store.patch_if("run_lease","old","owner",&json!("dead"),
            &json!({"expiresAtMs":past})).unwrap();
        assert!(store.renew_lease("old","dead",60_000).is_none());
        assert!(store.acquire_workspace_access("read","new",60_000,"workspace","read").is_some());
        assert!(!store.release_lease("old","someone-else"));
    }
    #[test]
    fn usage_summary_aggregates_calls_and_estimates() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.audit(
            "model.usage",
            json!({"providerId":"p1","model":"m","purpose":"chat","totalTokens":100,"estimated":true}),
        );
        store.audit(
            "model.usage",
            json!({"providerId":"p1","model":"m","purpose":"chat","totalTokens":50,"estimated":true}),
        );
        store.audit(
            "model.usage",
            json!({"providerId":"p2","model":"n","purpose":"helper","promptTokens":10,"completionTokens":5,"totalTokens":15}),
        );
        store.audit("tool.call", json!({"tool":"x"}));
        let summary = store.usage_summary();
        assert_eq!(summary["calls"], 3);
        assert_eq!(summary["totalTokens"], 165);
        assert_eq!(summary["promptTokens"], 10);
        assert_eq!(summary["completionTokens"], 5);
        assert_eq!(summary["estimatedCalls"], 2);
        let groups = summary["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["providerId"], "p1");
        assert_eq!(groups[0]["calls"], 2);
        assert_eq!(groups[1]["estimatedCalls"], 0);
    }

    #[test]
    fn session_usage_shows_model_call_amplification_and_unknown_cache_honestly() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for (session, prompt, cached) in [
            ("few-turns",57_000,Some(56_000)),
            ("few-turns",57_000,Some(56_000)),
            ("few-turns",57_000,None),
            ("different-session",99_999,Some(90_000)),
        ] {
            store.audit("model.usage",json!({
                "sessionId":session,"promptTokens":prompt,"completionTokens":250,
                "totalTokens":prompt+250,"cachedPromptTokens":cached,
            }));
        }
        let result=store.session_usage("few-turns");
        assert_eq!(result["calls"],3);
        assert_eq!(result["totalTokens"],171_750);
        assert_eq!(result["promptTokens"],171_000);
        assert_eq!(result["cachedPromptTokensKnown"],112_000);
        assert_eq!(result["newPromptTokensKnown"],2_000);
        assert_eq!(result["cacheReportedCalls"],2);
        assert_eq!(result["cacheUnknownCalls"],1,
            "older calls without cache telemetry must not be reported as misses");
    }

    #[test]
    fn attempt_creation_is_atomic_and_idempotent() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let root = json!({
            "id":"root-1","contract":{"revision":"r1"},
            "attempts":[],"rootOutcome":"running"
        });
        let execution = json!({"id":"exec-1","taskId":"root-1"});
        let attempt = json!({"executionId":"exec-1","status":"running"});
        assert_eq!(
            store.create_attempt(&root, &execution, &attempt).unwrap()["id"],
            "exec-1"
        );
        let episode = store.get("task_episode", "root-1").unwrap();
        assert_eq!(episode["attempts"].as_array().unwrap().len(), 1);
        store.create_attempt(&root, &execution, &attempt).unwrap();
        assert_eq!(
            store.get("task_episode", "root-1").unwrap()["attempts"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "re-linking the same attempt must be idempotent"
        );
        store
            .create_attempt(
                &root,
                &json!({"id":"exec-2","taskId":"root-1"}),
                &json!({"executionId":"exec-2","status":"running"}),
            )
            .unwrap();
        assert_eq!(
            store.get("task_episode", "root-1").unwrap()["attempts"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn attempt_creation_rolls_back_when_the_episode_is_corrupt() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put(
            "task_episode",
            &json!({"id":"root-2","attempts":"broken"}),
        );
        assert!(store
            .create_attempt(
                &json!({"id":"root-2"}),
                &json!({"id":"exec-3","taskId":"root-2"}),
                &json!({"executionId":"exec-3"}),
            )
            .is_none());
        assert!(
            store.get("execution", "exec-3").is_none(),
            "a failed attempt link must not leave a detached execution"
        );
    }

    #[test]
    fn records_round_trip_with_client_fields() {
        let (store, dir) = temp_store();
        let saved = store
            .put(
                "provider",
                &json!({ "name": "Local", "baseUrl": "http://127.0.0.1:4199/v1" }),
            )
            .expect("saved");
        let id = saved["id"].as_str().unwrap().to_string();
        assert_eq!(saved["name"], "Local");
        assert!(saved.get("createdAt").is_some());

        let fetched = store.get("provider", &id).unwrap();
        assert_eq!(fetched["baseUrl"], "http://127.0.0.1:4199/v1");

        let updated = store
            .put(
                "provider",
                &json!({ "id": id, "name": "Renamed", "baseUrl": "http://x" }),
            )
            .unwrap();
        assert_eq!(updated["name"], "Renamed");
        assert_eq!(store.all("provider").len(), 1);

        store.remove("provider", &id);
        assert!(store.get("provider", &id).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn messages_preserve_reasoning_and_order() {
        let (store, dir) = temp_store();
        store.write_message(&json!({
            "session_id": "s1", "role": "user", "content": "hello"
        }));
        store.write_message(&json!({
            "session_id": "s1", "role": "assistant", "content": "hi",
            "reasoning": "thought", "tool_calls": [{ "id": "c1" }]
        }));
        let messages = store.messages("s1");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["reasoning"], "thought");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "c1");
        store.clear_messages("s1");
        assert!(store.messages("s1").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn try_messages_reads_the_same_rows_with_error_preservation() {
        let (store, dir) = temp_store();
        store.write_message(&json!({
            "session_id": "s1", "role": "user", "content": "hello"
        }));
        store.write_message(&json!({
            "session_id": "s1", "role": "assistant", "content": "hi"
        }));
        let strict = store.try_messages("s1").unwrap();
        assert_eq!(strict, store.messages("s1"));
        assert_eq!(strict.len(), 2);
        assert!(store.try_messages("missing").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_workers_claim_a_job_only_once() {
        let (store, dir) = temp_store();
        store
            .put_if_absent("job", &json!({"id":"one","status":"queued"}))
            .unwrap();
        assert!(store
            .put_if_absent("job", &json!({"id":"one","status":"overwritten"}))
            .is_none());
        let workers: Vec<_> = (0..12)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || {
                    store
                        .patch_if(
                            "job",
                            "one",
                            "status",
                            &json!("queued"),
                            &json!({"status":"running"}),
                        )
                        .is_some()
                })
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .filter_map(|worker| worker.join().ok())
                .filter(|claimed| *claimed)
                .count(),
            1
        );
        assert_eq!(store.get("job", "one").unwrap()["status"], "running");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_finalizations_merge_improvement_counters() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put(
            "improvement",
            &json!({
                "id":"imp-1","status":"adopted",
                "uses":5,"successes":5,"failures":0,"appliedExecutionIds":[]
            }),
        );
        for (execution_id, status) in [("exec-a", "validated"), ("exec-b", "adopted")] {
            let execution = json!({"id":execution_id});
            let marker = json!({
                "id": format!("execution:{execution_id}"),
                "payloadHash": format!("hash-{execution_id}"),
            });
            let improvement = json!({
                "id":"imp-1","uses":6,"successes":6,"failures":0,"status":status,
                "appliedExecutionIds":[execution_id],
                "__counterDelta":{"uses":1,"successes":1,"failures":0},
            });
            assert_eq!(
                store
                    .finalize_once(&execution, &marker, &[improvement], &[])
                    .unwrap(),
                FinalizeOutcome::Applied
            );
        }
        let merged = store.get("improvement", "imp-1").unwrap();
        assert_eq!(merged["uses"], 7, "one observation was lost: {merged}");
        assert_eq!(merged["successes"], 7);
        assert_eq!(merged["appliedExecutionIds"].as_array().unwrap().len(), 2);
        assert_eq!(merged["status"], "validated");
        assert!(merged.get("__counterDelta").is_none());
    }

    #[test]
    fn owner_dispatch_is_atomic_and_metadata_is_preserved() {
        let (store, dir) = temp_store();
        let message = json!({"id":"m1","session_id":"room:r","role":"user","content":"내 지시","metadata":{"authorKind":"owner","roomId":"r"}});
        let job = json!({"id":"j1","kind":"room","status":"queued"});
        let (saved, _) = store
            .write_message_with_record(&message, "job", &job)
            .unwrap();
        assert_eq!(saved["metadata"]["authorKind"], "owner");
        assert_eq!(store.message("m1").unwrap()["content"], "내 지시");
        let mut conflicting = message.clone();
        conflicting["id"] = json!("m2");
        assert!(store
            .write_message_with_record(&conflicting, "job", &job)
            .is_none());
        assert!(
            store.message("m2").is_none(),
            "queue failure must roll back owner insertion"
        );
        assert_eq!(store.messages("room:r").len(), 1);
        store.put("agent", &json!({"id":"a","model":"old"}));
        assert!(store
            .patch_records(
                "agent",
                &["a".into(), "missing".into()],
                &json!({"model":"new"})
            )
            .is_none());
        assert_eq!(store.get("agent", "a").unwrap()["model"], "old");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancelled_or_interrupted_attempt_rejects_stale_finalization_atomically() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id,status) in [("cancel-race","cancelled"),("restart-race","interrupted")] {
            store.put("execution",&json!({"id":id,"status":status,
                "reason":"owner terminal decision"})).unwrap();
            let stale=json!({"id":id,"status":"completed","taskOutcome":"succeeded"});
            let marker=json!({"id":format!("execution:{id}"),"payloadHash":"stale"});
            let counters=json!({"id":format!("imp:{id}"),"uses":1});
            assert_eq!(store.finalize_once(&stale,&marker,&[counters],&[]).unwrap(),
                FinalizeOutcome::Conflict);
            let current=store.get("execution",id).unwrap();
            assert_eq!(current["status"],status);
            assert_eq!(current["reason"],"owner terminal decision");
            assert!(store.get("execution_finalization",
                &format!("execution:{id}")).is_none());
            assert!(store.get("improvement",&format!("imp:{id}")).is_none());
        }
    }

    #[test]
    fn cancelled_paid_dispatch_only_relabels_as_review_with_both_durable_fences() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        let persisted=json!({"id":"cancelled-paid","status":"cancelled",
            "kernelState":{"modelInFlight":true,"inFlight":[]},
            "reason":"owner cancellation"});
        store.put("execution",&persisted).unwrap();
        let mut review=persisted.clone();
        review["status"]=json!("needs_review");
        review["ownerCancellationRequested"]=json!(true);
        let marker=json!({"id":"execution:cancelled-paid",
            "payloadHash":"uncertain-paid"});
        assert_eq!(store.finalize_once(&review,&marker,&[],&[]).unwrap(),
            FinalizeOutcome::Applied);
        assert_eq!(store.get("execution","cancelled-paid").unwrap()["status"],
            "needs_review");
        assert_eq!(store.get("execution","cancelled-paid").unwrap()
            ["kernelState"]["modelInFlight"],true);
        assert_eq!(store.finalize_once(&review,&marker,&[],&[]).unwrap(),
            FinalizeOutcome::AlreadyApplied);
    }

    #[test]
    fn cancelled_completed_task_never_reopens_on_unproven_review_claim(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id,committed_fence,claimed_fence,claim_owner) in [
            ("no-paid-fence",false,true,true),
            ("no-review-fence",true,false,true),
            ("missing-cancel-authority",true,true,false)
        ] {
            store.put("execution",&json!({"id":id,"status":"cancelled",
                "kernelState":{"modelInFlight":committed_fence}})).unwrap();
            let claim=json!({"id":id,"status":"needs_review",
                "ownerCancellationRequested":claim_owner,
                "kernelState":{"modelInFlight":claimed_fence}});
            let marker=json!({"id":format!("execution:{id}"),
                "payloadHash":format!("bad-{id}")});
            assert_eq!(store.finalize_once(&claim,&marker,&[],&[]).unwrap(),
                FinalizeOutcome::Conflict);
            assert_eq!(store.get("execution",id).unwrap()["status"],
                "cancelled");
        }
    }

    #[test]
    fn finalization_is_idempotent_and_conflict_aware() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let execution = json!({"id":"exec-1","status":"completed"});
        let marker = json!({"id":"execution:exec-1","payloadHash":"hash-1"});
        let improvement = json!({"id":"imp-1","uses":0,"status":"adopted"});
        let observation = json!({"id":"obs-1"});
        assert_eq!(
            store
                .finalize_once(&execution, &marker, &[improvement.clone()], &[observation.clone()])
                .unwrap(),
            FinalizeOutcome::Applied
        );
        assert_eq!(
            store
                .finalize_once(&execution, &marker, &[improvement.clone()], &[observation.clone()])
                .unwrap(),
            FinalizeOutcome::AlreadyApplied
        );
        let conflicting = json!({"id":"execution:exec-1","payloadHash":"hash-2"});
        assert_eq!(
            store
                .finalize_once(&execution, &conflicting, &[], &[])
                .unwrap(),
            FinalizeOutcome::Conflict
        );
        assert_eq!(store.get("improvement", "imp-1").unwrap()["uses"], 0);
        assert_eq!(store.try_all("improvement").unwrap().len(), 1);
    }

    #[test]
    fn concurrent_finalizations_apply_once() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let execution = json!({"id":"exec-2","status":"completed"});
        let marker = json!({"id":"execution:exec-2","payloadHash":"hash-2"});
        let improvement = json!({"id":"imp-2","uses":0});
        let mut handles = Vec::new();
        for _ in 0..2 {
            let store = store.clone();
            let execution = execution.clone();
            let marker = marker.clone();
            let improvement = improvement.clone();
            handles.push(std::thread::spawn(move || {
                store
                    .finalize_once(&execution, &marker, &[improvement], &[])
                    .unwrap()
            }));
        }
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert!(outcomes.contains(&FinalizeOutcome::Applied));
        assert!(outcomes.contains(&FinalizeOutcome::AlreadyApplied));
        assert_eq!(store.try_all("execution_finalization").unwrap().len(), 1);
    }

    #[test]
    fn try_all_preserves_records_and_returns_them() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("memory", &json!({"id":"m1","text":"a"}));
        store.put("memory", &json!({"id":"m2","text":"b"}));
        let records = store.try_all("memory").unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn broken_finalization_marker_is_an_error() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO records(kind,id,data) VALUES('execution_finalization','execution:broken','{bad')",
                [],
            )
            .unwrap();
        }
        let execution = json!({"id":"broken","status":"completed"});
        let marker = json!({"id":"execution:broken","payloadHash":"hash"});
        assert!(store.finalize_once(&execution, &marker, &[], &[]).is_err());
    }
    #[test]
    fn room_answer_and_cursor_are_atomic_and_cannot_rewind() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        let owner=json!({"id":"owner","session_id":"room:a","role":"user",
            "content":"실행","metadata":{"authorKind":"owner"}});
        store.write_message(&owner).unwrap();
        store.write_message(&json!({"id":"peer","session_id":"room:a",
            "role":"assistant","agent_id":"b","content":"참고",
            "metadata":{"authorKind":"agent"}})).unwrap();
        let reply=|text:&str|json!({
            "session_id":"room:a","role":"assistant","agent_id":"a",
            "content":text,"metadata":{"authorKind":"agent","replyToMessageId":"owner"}
        });
        let cursor=|through:&str|json!({
            "id":"cursor:room-a-agent-a","sessionId":"room:a",
            "agentId":"a","sourceMessageId":"owner",
            "throughMessageId":through,"schemaVersion":1
        });
        let saved=store.write_room_answer_with_cursor(&reply("ok"),&cursor("peer")).unwrap();
        assert_eq!(saved["content"],"ok");
        assert_eq!(store.get("room_read_cursor","cursor:room-a-agent-a").unwrap()["throughMessageId"],"peer");
        let before=store.messages("room:a").len();
        assert!(store.write_room_answer_with_cursor(&reply("stale"),&cursor("owner")).is_err());
        assert_eq!(store.messages("room:a").len(),before,
            "A stale cursor must roll back the reply, not only the cursor");
        assert_eq!(store.get("room_read_cursor","cursor:room-a-agent-a").unwrap()["throughMessageId"],"peer");
        let invalid=json!({"session_id":"room:foreign","role":"assistant",
            "agent_id":"a","metadata":{"authorKind":"agent","replyToMessageId":"owner"}});
        assert!(store.write_room_answer_with_cursor(&invalid,&cursor("peer")).is_err());
        assert_eq!(store.messages("room:a").len(),before);
    }

    #[test]
    fn legacy_room_transcripts_migrate_before_event_backfill(){
        let dir=std::env::temp_dir().join(format!(
            "dominion-legacy-room-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file=dir.join("legacy.sqlite");
        {
            let legacy=Connection::open(&file).unwrap();
            legacy.execute_batch(
                "CREATE TABLE messages(
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL DEFAULT '',
                    created_at TEXT DEFAULT (datetime('now'))
                );
                INSERT INTO messages(id,session_id,role,content)
                VALUES('old-user','room:legacy','user','original');"
            ).unwrap();
        }
        let upgraded=Store::open(&file).unwrap();
        let events=upgraded.room_events("room:legacy",0,10).unwrap();
        assert_eq!(events.len(),1);
        assert_eq!(events[0]["eventId"],"old-user");
        assert_eq!(events[0]["seq"],1);
        drop(upgraded);
        let _=std::fs::remove_dir_all(dir);
    }
    #[test]
    fn room_events_commit_150_messages_in_order_and_survive_restart(){
        let dir=std::env::temp_dir().join(format!(
            "dominion-room-events-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file=dir.join("events.sqlite");
        let store=Store::open(&file).unwrap();
        for i in 0..150 {
            let room=if i%3==0 {"room:a"}else{"room:b"};
            store.write_message(&json!({
                "id":format!("event-{i}"),"session_id":room,
                "role":if i%3==0 {"user"}else{"assistant"},
                "content":format!("record {i}"),
                "agent_id":if i%3==0 {Value::Null}else{json!("b")},
                "metadata":{"authorKind":if i%3==0 {"owner"}else{"agent"},
                            "executionId":"original-run"}
            })).unwrap();
        }
        for (room,total) in [("room:a",50),("room:b",100)]{
            let events=store.room_events(room,0,150).unwrap();
            assert_eq!(events.len(),total);
            for (index,e) in events.iter().enumerate(){
                assert_eq!(e["seq"],index+1);
                assert_eq!(e["sessionId"],room);
                assert_eq!(e["originRun"],"original-run");
            }
            assert_eq!(store.room_events(room,25,150).unwrap().len(),total-25);
        }
        assert!(store.room_events("private",0,100).is_err());
        drop(store);
        let again=Store::open(&file).unwrap();
        assert_eq!(again.room_events("room:a",0,150).unwrap().len(),50);
        assert_eq!(again.room_events("room:b",0,150).unwrap().len(),100);
        drop(again);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn remote_claim_is_unique_and_terminal_receipts_are_fenced(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for id in ["remote:one","remote:two"]{
            store.put("remote_task",&json!({"id":id,"kind":"remote_inference",
                "status":"queued","prompt":"Summarize","maxTokens":200,
                "queuedAt":id,"attempts":0})).unwrap();
        }
        let one=store.claim_remote_task("worker-a").unwrap().unwrap();
        let two=store.claim_remote_task("worker-b").unwrap().unwrap();
        assert_ne!(one["taskId"],two["taskId"]);
        assert!(store.claim_remote_task("worker-c").unwrap().is_none());
        let id=one["taskId"].as_str().unwrap();
        let lease=one["leaseId"].as_str().unwrap();
        assert!(!store.heartbeat_remote_task(id,"worker-b",lease).unwrap());
        assert!(store.finish_remote_task(id,"worker-b",lease,"completed",
            "forged",&json!({})).unwrap().is_none());
        assert!(store.heartbeat_remote_task(id,"worker-a",lease).unwrap());
        let saved=store.finish_remote_task(id,"worker-a",lease,"completed",
            "true result",&json!({"total_tokens":8})).unwrap().unwrap();
        assert_eq!(saved["status"],"completed");
        assert_eq!(saved["result"]["verified"],false);
        assert_eq!(saved["result"]["answer"],"true result");
        let repeat=store.finish_remote_task(id,"worker-a",lease,"completed",
            "different result",&Value::Null).unwrap().unwrap();
        assert_eq!(repeat["result"]["answer"],"true result");
        let second=two["taskId"].as_str().unwrap();
        let second_lease=two["leaseId"].as_str().unwrap();
        store.cancel_remote_task(second).unwrap().unwrap();
        assert!(store.finish_remote_task(second,"worker-b",second_lease,
            "completed","late",&Value::Null).unwrap().is_none());
    }
    #[test]
    fn crashed_remote_worker_requires_review_without_another_worker_claim(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("remote_task",&json!({"id":"remote:crash","status":"queued",
            "prompt":"paid inference","queuedAt":"1"})).unwrap();
        let claimed=store.claim_remote_task("workstation").unwrap().unwrap();
        let task=claimed["taskId"].as_str().unwrap();
        store.patch_if("remote_task",task,"status",&json!("running"),
            &json!({"leaseExpiresAtMs":0})).unwrap();
        assert_eq!(store.review_expired_remote_tasks().unwrap(),1);
        assert_eq!(store.review_expired_remote_tasks().unwrap(),0);
        let saved=store.get("remote_task",task).unwrap();
        assert_eq!(saved["status"],"needs_review");
        assert!(saved["reviewReason"].as_str().unwrap().contains("no replay"));
        assert!(store.claim_remote_task("replacement").unwrap().is_none());
        assert!(store.cancel_remote_task(task).unwrap().is_none());
    }
    #[test]
    fn expired_remote_lease_requires_owner_review_and_cannot_replay(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("remote_task",&json!({"id":"remote:expired","status":"queued",
            "prompt":"Inference","maxTokens":80,"queuedAt":"1"})).unwrap();
        let claim=store.claim_remote_task("w").unwrap().unwrap();
        let task=claim["taskId"].as_str().unwrap();
        let lease=claim["leaseId"].as_str().unwrap();
        store.patch_if("remote_task",task,"status",&json!("running"),
            &json!({"leaseExpiresAtMs":0})).unwrap();
        assert!(store.claim_remote_task("other").unwrap().is_none());
        let saved=store.get("remote_task",task).unwrap();
        assert_eq!(saved["status"],"needs_review");
        let late=store.finish_remote_task(task,"w",lease,"completed",
            "late",&Value::Null).unwrap().unwrap();
        assert_eq!(late["status"],"needs_review");
        assert!(late["result"].is_null());
        assert_eq!(store.get("remote_task",task).unwrap()["status"],"needs_review");
    }
    #[test]
    fn id_guarded_legacy_compare_and_patch_remains_compatible(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("room",&json!({"id":"room-id","maxTurns":9})).unwrap();
        let updated=store.patch_if("room","room-id","id",&json!("room-id"),
            &json!({"maxTurns":1})).unwrap();
        assert_eq!(updated["maxTurns"],1);
        assert_eq!(updated["id"],"room-id");
        assert!(store.patch_if("room","room-id","id",&json!("foreign"),
            &json!({"maxTurns":2})).is_none());
    }
    #[test]
    fn dual_field_cas_rejects_stale_phase_and_completed_run_revival(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.put("execution",&json!({
            "id":"race","status":"running","phase":"running",
            "providerUsage":{"realTokens":42}
        })).unwrap();
        let running=json!("running");
        let waiting=json!("waiting_approval");
        let updated=store.patch_if_all("execution","race",
            &[("status",&running),("phase",&running)],
            &json!({"phase":"waiting_approval"})).unwrap();
        assert_eq!(updated["phase"],waiting);
        assert_eq!(updated["providerUsage"]["realTokens"],42);
        assert!(store.patch_if_all("execution","race",
            &[("status",&running),("phase",&running)],
            &json!({"phase":"finalizing"})).is_none());
        store.patch_if("execution","race","status",&running,
            &json!({"status":"cancelled"})).unwrap();
        assert!(store.patch_if_all("execution","race",
            &[("status",&running),("phase",&waiting)],
            &json!({"phase":"running"})).is_none());
        assert_eq!(store.get("execution","race").unwrap()["status"],"cancelled");
    }

    #[test]
    fn room_context_reads_exact_commit_order_even_if_imported_timestamps_differ(){
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id,session) in [("first","room:a"),("second","room:a"),
            ("p-first","private"),("p-second","private")]{
            store.write_message(&json!({"id":id,"session_id":session,
                "role":"assistant","content":id})).unwrap();
        }
        {
            let conn=store.conn.lock().unwrap();
            conn.execute("UPDATE messages SET created_at='2099-01-01' WHERE id IN ('first','p-first')",[]).unwrap();
            conn.execute("UPDATE messages SET created_at='2000-01-01' WHERE id IN ('second','p-second')",[]).unwrap();
        }
        let read=store.try_messages("room:a").unwrap();
        assert_eq!(read[0]["id"],"first");
        assert_eq!(read[1]["id"],"second");
        let listed=store.messages("room:a");
        assert_eq!(listed[0]["id"],"first");
        assert_eq!(listed[1]["id"],"second");
        assert_eq!(store.messages("private")[0]["id"],"p-second");
        assert_eq!(store.room_events("room:a",0,10).unwrap()[0]["eventId"],"first");
    }

    #[test]
    fn room_cursor_acknowledgement_requires_the_actual_owner_message() {
        let store=Store::open(std::path::Path::new(":memory:")).unwrap();
        store.write_message(&json!({"id":"source","session_id":"room:a",
            "role":"user","content":"요청","metadata":{"authorKind":"agent"}})).unwrap();
        let answer=json!({"session_id":"room:a","role":"assistant","agent_id":"a",
            "content":"완료","metadata":{"authorKind":"agent","replyToMessageId":"source"}});
        let cursor=json!({"id":"cursor","sessionId":"room:a","agentId":"a",
            "sourceMessageId":"source","throughMessageId":"source"});
        assert!(store.write_room_answer_with_cursor(&answer,&cursor).is_err());
        assert_eq!(store.messages("room:a").len(),1);
        assert!(store.get("room_read_cursor","cursor").is_none());
    }

