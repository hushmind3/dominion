use crate::db::Store;
use dominion_core::run_event::{RunEventEnvelope,RunEventRef,RunLifecycle};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::broadcast;

/// In-process event bus backing `GET /api/events`, backed by a bounded durable
/// sequence (`event_log`). A client that reconnects after `Lagged` or a
/// disconnect can replay from its last `seq` instead of silently missing writes.
/// Infer only execution IDs already provided by trusted product records.
/// Never turn an unrelated room ID or job ID into a fabricated execution ID.
fn run_reference(event:&Value)->Option<RunEventRef>{
    let kind=event["type"].as_str()?;
    let value=match kind{
        "record"=>event.get("record")?,
        "message"=>event.get("message")?,
        "approval"=>event.get("approval")?,
        _=>event,
    };
    let pick=|obj:&Value,key:&str| -> Option<String> {
        obj.get(key)?.as_str().filter(|id|!id.trim().is_empty())
            .map(str::to_string)
    };
    let metadata=value.get("metadata").unwrap_or(&Value::Null);
    let execution_id=if kind=="record"&&event["kind"]=="execution"{
        pick(value,"id")
    }else{
        pick(value,"executionId")
            .or_else(||pick(value,"runId"))
            .or_else(||pick(metadata,"executionId"))
            .or_else(||pick(value,"evidenceExecutionId"))
    }?;
    let mut reference=RunEventRef::new(&execution_id)?;
    reference.session_id=pick(value,"sessionId").or_else(||pick(value,"session_id"));
    reference.job_id=pick(value,"jobId")
        .or_else(||(kind=="record"&&event["kind"]=="job").then(||pick(value,"id")).flatten());
    reference.parent_execution_id=pick(value,"parentExecutionId");
    reference.external_run_id=pick(value,"externalRunId")
        .or_else(||(kind=="record"&&event["kind"]=="external_agent_run")
            .then(||pick(value,"id")).flatten());
    reference.task_id=pick(value,"taskId").or_else(||pick(metadata,"taskId"));
    reference.room_id=pick(value,"roomId").or_else(||pick(metadata,"roomId"));
    reference.agent_id=pick(value,"agentId").or_else(||pick(value,"agent_id"));
    reference.source_message_id=pick(value,"sourceMessageId")
        .or_else(||pick(metadata,"replyToMessageId"));
    if kind=="record"&&event["kind"]=="execution" {
        reference.lifecycle=value["status"].as_str().and_then(RunLifecycle::parse);
    }
    Some(reference)
}

#[derive(Clone)]
pub struct Bus {
    sender: broadcast::Sender<Value>,
    store: Store,
    seq: Arc<AtomicU64>,
}

impl Bus {
    pub fn new(store: Store) -> Self {
        let seq = store.last_event_seq();
        let (sender, _) = broadcast::channel(512);
        Self {
            sender,
            store,
            seq: Arc::new(AtomicU64::new(seq)),
        }
    }

    pub fn emit(&self, mut value: Value) {
        // Add a typed, versioned correlation reference without renaming any
        // existing wire fields. Room/job presentation states remain distinct
        // from authoritative execution lifecycle states.
        if value.get("runRef").is_none() {
            if let Some(reference)=run_reference(&value) {
                value["runRef"]=json!(reference);
            }
        }
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        value["seq"] = json!(seq);
        self.store.append_event(seq, &value);
        let _ = self.sender.send(value);
    }

    pub fn record(&self, kind: &str, record: &Value) {
        self.emit(json!({ "type": "record", "kind": kind, "record": record }));
    }

    pub fn message(&self, message: &Value) {
        self.emit(json!({ "type": "message", "message": message }));
    }

    pub fn approval(&self, approval: &Value) {
        self.emit(json!({ "type": "approval", "approval": approval }));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.sender.subscribe()
    }

    /// Replays durable events after `after` (exclusive). Used on reconnect.
    pub fn replay(&self, after: u64, limit: usize) -> Vec<Value> {
        self.store.events_after(after, limit)
    }

    /// Durable, scoped replay for Chat/Room/Task/SDK consumers that know the
    /// actual execution ID. Event order is the shared journal order.
    pub fn replay_run(&self, execution_id:&str, after:u64, limit:usize)->Vec<RunEventEnvelope>{
        self.store.events_for_execution(execution_id,after,limit).into_iter()
            .filter_map(|event|serde_json::from_value::<RunEventEnvelope>(event).ok())
            .filter(|event|event.run_ref.execution_id==execution_id)
            .collect()
    }

    pub fn earliest_event_seq(&self)->Option<u64>{
        self.store.earliest_event_seq()
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn events_are_durable_and_replayable_after_a_disconnect() {
        let store = store();
        let bus = Bus::new(store.clone());
        bus.emit(json!({"type":"first"}));
        bus.emit(json!({"type":"second"}));
        bus.emit(json!({"type":"third"}));
        let replay = bus.replay(1, 10);
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0]["seq"], 2);
        assert_eq!(replay[0]["type"], "second");
        assert_eq!(replay[1]["seq"], 3);
        assert!(bus.replay(3, 10).is_empty());

        // A restarted process continues the sequence instead of reusing ids.
        let restarted = Bus::new(store);
        restarted.emit(json!({"type":"fourth"}));
        assert_eq!(restarted.replay(3, 10)[0]["seq"], 4);
    }

    #[test]
    fn run_correlations_share_one_execution_id_across_record_message_and_sdk(){
        let store=store();
        let bus=Bus::new(store);
        bus.record("execution",&json!({"id":"run-1","sessionId":"room:r",
            "taskId":"task-1","roomId":"r","agentId":"a","status":"completed"}));
        bus.message(&json!({"id":"message-1","session_id":"room:r",
            "agent_id":"a","metadata":{"executionId":"run-1",
                "roomId":"r","replyToMessageId":"owner-1"}}));
        bus.record("sdk_run",&json!({"id":"sdk-1","executionId":"run-1",
            "sessionId":"room:r"}));
        let events=bus.replay(0,10);
        assert_eq!(events.len(),3);
        for (index,event) in events.iter().enumerate() {
            assert_eq!(event["runRef"]["executionId"],"run-1");
            assert_eq!(event["runRef"]["sessionId"],"room:r");
            assert_eq!(event["runRef"]["schemaVersion"],1);
            assert_eq!(event["seq"],(index+1) as u64);
        }
        assert_eq!(events[0]["runRef"]["lifecycle"],"completed");
        assert_eq!(events[1]["runRef"]["sourceMessageId"],"owner-1");
        assert!(events[2]["runRef"].get("lifecycle").is_none());
        bus.record("room",&json!({"id":"r","status":"idle"}));
        assert!(bus.replay(3,10)[0].get("runRef").is_none());
    }


    #[test]
    fn per_execution_cursor_is_durable_scoped_and_idempotent_after_restart(){
        let store=store();
        let bus=Bus::new(store.clone());
        bus.record("execution",&json!({"id":"exec-one","sessionId":"chat-a","status":"running"}));
        bus.record("execution",&json!({"id":"exec-two","sessionId":"chat-b","status":"running"}));
        bus.message(&json!({"id":"answer-one","session_id":"chat-a",
            "metadata":{"executionId":"exec-one","authorKind":"agent"}}));
        let first=bus.replay_run("exec-one",0,1);
        assert_eq!(first.len(),1);
        assert_eq!(first[0].seq,1);
        assert_eq!(first[0].run_ref.lifecycle,Some(RunLifecycle::Running));
        assert_eq!(first[0].run_ref.execution_id,"exec-one");
        let second=bus.replay_run("exec-one",first[0].seq,10);
        assert_eq!(second.len(),1);
        assert_eq!(second[0].seq,3);
        assert_eq!(second[0].run_ref.session_id.as_deref(),Some("chat-a"));
        assert!(bus.replay_run("exec-two",2,10).is_empty());
        drop(bus);
        let restored=Bus::new(store);
        restored.record("execution",&json!({"id":"exec-one","status":"completed"}));
        let continuation=restored.replay_run("exec-one",3,10);
        assert_eq!(continuation.len(),1);
        assert_eq!(continuation[0].seq,4);
        assert_eq!(continuation[0].run_ref.lifecycle,Some(RunLifecycle::Completed));
    }

    #[test]
    fn durable_global_cursor_drains_more_than_one_reconnect_page_without_gaps(){
        let store=store();
        let bus=Bus::new(store.clone());
        for n in 0..1_205 {bus.emit(json!({"type":"progress","n":n}));}
        let mut cursor=0u64;
        let mut observed=Vec::new();
        loop {
            let page=bus.replay(cursor,500);
            if page.is_empty(){break;}
            let page_len=page.len();
            for event in page {
                let seq=event["seq"].as_u64().unwrap();
                assert!(seq>cursor,"duplicate cursor on reconnect");
                cursor=seq;
                observed.push(event["n"].as_u64().unwrap());
            }
            if page_len<500{break;}
        }
        assert_eq!(observed.len(),1_205);
        assert_eq!(observed[0],0);
        assert_eq!(observed.last(),Some(&1204));
        let resumed=Bus::new(store);
        assert!(resumed.replay(cursor,500).is_empty());
    }
    #[test]
    fn room_team_and_external_receipts_keep_one_native_key_without_id_aliasing(){
        let store=store();
        let bus=Bus::new(store);
        bus.emit(json!({"type":"room_agent_reply","executionId":"native-speaker",
            "roomId":"room-a","sessionId":"session-a","jobId":"room-job",
            "sourceMessageId":"owner-msg","replyMessageId":"reply-msg","agentId":"a"}));
        bus.record("job",&json!({"id":"team-job","kind":"team","executionId":"native-lead",
            "status":"completed","result":{"nodes":[{"executionId":"native-worker"}]}}));
        bus.record("job",&json!({"id":"worker-job","kind":"prompt",
            "executionId":"native-worker","parentExecutionId":"native-lead",
            "status":"completed"}));
        bus.record("external_agent_run",&json!({"id":"external:receipt","jobId":"external-job",
            "executionId":"native-lead","status":"completed","nativeToolReceipts":false}));
        let speaker=bus.replay_run("native-speaker",0,20);
        assert_eq!(speaker.len(),1);
        assert_eq!(speaker[0].run_ref.room_id.as_deref(),Some("room-a"));
        assert_eq!(speaker[0].run_ref.source_message_id.as_deref(),Some("owner-msg"));
        assert_eq!(speaker[0].run_ref.job_id.as_deref(),Some("room-job"));
        let lead=bus.replay_run("native-lead",0,20);
        assert_eq!(lead.len(),2);
        assert_eq!(lead[0].run_ref.job_id.as_deref(),Some("team-job"));
        assert_eq!(lead[1].run_ref.external_run_id.as_deref(),Some("external:receipt"));
        assert!(lead.iter().all(|e|e.run_ref.execution_id=="native-lead"));
        let worker=bus.replay_run("native-worker",0,20);
        assert_eq!(worker.len(),1);
        assert_eq!(worker[0].run_ref.parent_execution_id.as_deref(),Some("native-lead"));
    }
    #[test]
    fn the_log_is_bounded_without_losing_recent_events() {
        let store = store();
        let bus = Bus::new(store);
        for index in 0..600 {
            bus.emit(json!({"type":"tick","index":index}));
        }
        let recent = bus.replay(0, 1_000);
        assert!(!recent.is_empty());
        assert_eq!(recent.last().unwrap()["index"], 599);
        assert!(recent.len() <= 600);
    }
}
