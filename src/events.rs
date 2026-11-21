use crate::db::Store;
use dominion_turn_core::run_state::RunEvent;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use tokio::sync::broadcast;

/// In-process event bus backing `GET /api/events`, backed by a bounded durable
/// sequence (`event_log`). A client that reconnects after `Lagged` or a
/// disconnect can replay from its last `seq` instead of silently missing writes.
#[derive(Clone)]
pub struct Bus {
    sender: broadcast::Sender<Value>,
    store: Store,
    seq: Arc<AtomicU64>,
    // Ordering is atomic across allocation, SQLite persistence AND broadcast.
    emit_gate: Arc<Mutex<()>>,
}

impl Bus {
    pub fn new(store: Store) -> Self {
        let seq = store.last_event_seq();
        let (sender, _) = broadcast::channel(512);
        Self {
            sender,
            store,
            seq: Arc::new(AtomicU64::new(seq)),
            emit_gate: Arc::new(Mutex::new(())),
        }
    }

    /// One lock protects the durable sequence and broadcast for all events
    /// produced by a single record mutation. Reconnect sees record -> typed
    /// run state in order, without an unrelated writer interleaving the pair.
    fn emit_many(&self, values:impl IntoIterator<Item=Value>){
        let _emit_guard = self.emit_gate.lock().unwrap();
        for mut value in values {
            let seq=self.seq.fetch_add(1,Ordering::SeqCst)+1;
            value["seq"]=json!(seq);
            if value["type"]=="run_state"{
                // eventSeq is the canonical durable cursor. No per-room
                // guessed step arithmetic, so it remains monotonic on replay.
                value["eventSeq"]=json!(seq);
            }
            self.store.append_event(seq,&value);
            let _=self.sender.send(value);
        }
    }

    pub fn emit(&self,value:Value){
        self.emit_many([value]);
    }

    pub fn record(&self,kind:&str,record:&Value){
        let mut events=vec![json!({"type":"record","kind":kind,"record":record})];
        if let Some(typed)=RunEvent::from_record(kind,record){
            let mut event=typed.into_value();
            event["runKind"]=json!(kind);
            if kind=="task_team_work"{
                // A child run is linked to its real parent execution and
                // original dependency IDs, never to model-supplied output.
                event["parentRunId"]=record["executionId"].clone();
                event["teamNodeId"]=record["teamNodeId"].clone();
                event["nodeId"]=record["nodeId"].clone();
                event["dependsOn"]=record["dependsOn"].clone();
                event["evidenceStatus"]=record["evidence"]["kind"].clone();
            }
            events.push(event);
        }
        self.emit_many(events);
    }

    pub fn message(&self, message: &Value) {
        self.emit(json!({ "type": "message", "message": message }));
    }

    pub fn approval(&self, approval: &Value) {
        self.emit(json!({ "type": "approval", "approval": approval }));
    }

    /// Snapshot a single global journal watermark at the moment the model
    /// receives its context. The room reader must acknowledge THIS watermark,
    /// not the later cursor after provider/tool work has completed.
    pub fn cursor(&self)->u64{self.seq.load(Ordering::SeqCst)}

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.sender.subscribe()
    }

    /// Replays durable events after `after` (exclusive). Used on reconnect.
    pub fn replay(&self, after: u64, limit: usize) -> Vec<Value> {
        self.store.events_after(after, limit)
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
    fn concurrent_writers_preserve_strict_event_order() {
        let bus = Bus::new(store());
        let mut threads = Vec::new();
        for worker in 0..8 {
            let clone = bus.clone();
            threads.push(std::thread::spawn(move || {
                for index in 0..80 {
                    clone.emit(json!({"type":"tick","worker":worker,"index":index}));
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        let events = bus.replay(0, 800);
        assert_eq!(events.len(), 640);
        for (index, item) in events.iter().enumerate() {
            assert_eq!(item["seq"].as_u64(), Some(index as u64 + 1));
        }
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
    #[test]
    fn team_worker_events_include_parent_id_and_replay_after_restart(){
        let store=store();let bus=Bus::new(store.clone());
        bus.record("task_team_work",&json!({"id":"worker-1",
            "status":"assigned","executionId":"exec-1","nodeId":"read",
            "teamNodeId":"team","dependsOn":["prepare"],
            "agentId":"worker-a"}));
        let after=bus.replay(0,10);
        assert_eq!(after.len(),2);
        assert_eq!(after[1]["type"],"run_state");
        assert_eq!(after[1]["runKind"],"task_team_work");
        assert_eq!(after[1]["parentRunId"],"exec-1");
        assert_eq!(after[1]["state"],"queued");
        assert_eq!(after[1]["dependsOn"],json!(["prepare"]));
        let restarted=Bus::new(store);
        restarted.record("task_team_work",&json!({"id":"worker-1",
            "status":"completed","executionId":"exec-1","nodeId":"read",
            "teamNodeId":"team","evidence":{"kind":"tool_completed_observed"}}));
        let tail=restarted.replay(2,10);
        assert_eq!(tail.len(),2);
        assert_eq!(tail[1]["state"],"completed");
        assert_eq!(tail[1]["eventSeq"],4);
    }
    #[test]
    fn job_chat_and_room_share_durable_ordered_run_events_without_invented_owner(){
        let bus=Bus::new(store());
        bus.record("job",&json!({"id":"job-1","status":"queued"}));
        bus.record("execution",&json!({"id":"chat-1","status":"running",
            "phase":"queued","sourceMessageId":"owner-1","agentId":"worker"}));
        bus.record("room_work_assignment",&json!({"id":"room-1",
            "status":"assigned","sourceMessageId":"owner-1","agentId":"worker"}));
        let all=bus.replay(0,20);
        assert_eq!(all.len(),6);
        for (index,item) in all.iter().enumerate(){
            assert_eq!(item["seq"].as_u64(),Some(index as u64+1));
            if index%2==0 {assert_eq!(item["type"],"record");}
            else {
                assert_eq!(item["type"],"run_state");
                assert_eq!(item["schemaVersion"],1);
                assert_eq!(item["eventSeq"],item["seq"]);
            }
        }
        assert_eq!(all[1]["runKind"],"job");
        assert!(all[1]["sourceMessageId"].is_null());
        assert_eq!(all[3]["runKind"],"execution");
        assert_eq!(all[3]["state"],"queued");
        assert_eq!(all[5]["runKind"],"room_work_assignment");
        assert_eq!(all[5]["sourceMessageId"],"owner-1");
        let restarted=Bus::new(bus.store.clone());
        restarted.record("job",&json!({"id":"job-1","status":"completed"}));
        let next=restarted.replay(6,10);
        assert_eq!(next.len(),2);
        assert_eq!(next[1]["state"],"completed");
        assert_eq!(next[1]["eventSeq"],8);
    }

}
