use crate::db::Store;
use serde_json::{json, Value};
use tokio::sync::broadcast;

/// In-process event bus backing `GET /api/events`, backed by a bounded durable
/// sequence (`event_log`). A client that reconnects after `Lagged` or a
/// disconnect can replay from its last `seq` instead of silently missing writes.
#[derive(Clone)]
pub struct Bus {
    sender: broadcast::Sender<Value>,
    store: Store,
}

impl Bus {
    pub fn new(store: Store) -> Self {
        let (sender, _) = broadcast::channel(512);
        Self { sender, store }
    }

    /// Commit before broadcasting: a subscriber must never observe an event
    /// that cannot be replayed following a disconnect.
    pub fn try_emit(&self, value: Value) -> Result<u64, String> {
        let committed=self.store.append_event_next(value)?;
        let seq=committed["seq"].as_u64()
            .ok_or("Committed event missing durable sequence")?;
        let _ = self.sender.send(committed);
        Ok(seq)
    }

    pub fn emit(&self, value: Value) {
        if let Err(error)=self.try_emit(value) {
            // Preserve the existing infallible producer API while making
            // storage failure visible instead of publishing phantom events.
            eprintln!("dominion event persistence failed: {error}");
        }
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
    fn separately_opened_process_buses_never_overwrite_sequence_or_replay() {
        let dir=std::env::temp_dir().join(format!(
            "dominion-event-clock-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path=dir.join("events.sqlite");
        let a=Bus::new(Store::open(&path).unwrap());
        let b=Bus::new(Store::open(&path).unwrap());
        assert_eq!(a.try_emit(json!({"type":"a1"})).unwrap(),1);
        assert_eq!(b.try_emit(json!({"type":"b1"})).unwrap(),2);
        assert_eq!(a.try_emit(json!({"type":"a2"})).unwrap(),3);
        let events=b.replay(0,10);
        assert_eq!(events.iter().map(|row|row["seq"].as_u64().unwrap())
            .collect::<Vec<_>>(),vec![1,2,3]);
        assert_eq!(events.iter().map(|row|row["type"].as_str().unwrap())
            .collect::<Vec<_>>(),vec!["a1","b1","a2"]);
        drop(a);
        drop(b);
        let c=Bus::new(Store::open(&path).unwrap());
        assert_eq!(c.try_emit(json!({"type":"after_restart"})).unwrap(),4);
        drop(c);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn interleaved_cross_connection_writes_are_unique_and_gap_free() {
        let dir=std::env::temp_dir().join(format!(
            "dominion-event-parallel-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path=dir.join("events.sqlite");
        let a=Bus::new(Store::open(&path).unwrap());
        let b=Bus::new(Store::open(&path).unwrap());
        let left=std::thread::spawn(move||{
            (0..50).map(|n|a.try_emit(json!({"type":"left","n":n})).unwrap())
                .collect::<Vec<_>>()
        });
        let right=std::thread::spawn(move||{
            (0..50).map(|n|b.try_emit(json!({"type":"right","n":n})).unwrap())
                .collect::<Vec<_>>()
        });
        let mut observed=left.join().unwrap();
        observed.extend(right.join().unwrap());
        observed.sort_unstable();
        assert_eq!(observed,(1..=100).collect::<Vec<_>>());
        let reopened=Bus::new(Store::open(&path).unwrap());
        assert_eq!(reopened.replay(0,200).len(),100);
        drop(reopened);
        let _=std::fs::remove_dir_all(dir);
    }

    #[test]
    fn retention_pruning_does_not_rewind_durable_event_clock() {
        let store=store();
        let bus=Bus::new(store.clone());
        for n in 0..2250 {bus.try_emit(json!({"type":"tick","n":n})).unwrap();}
        let before=store.last_event_seq();
        assert_eq!(before,2250);
        let remaining=bus.replay(0,1000);
        assert_eq!(remaining[0]["seq"],256);
        assert_eq!(bus.try_emit(json!({"type":"new"})).unwrap(),2251);
        assert_eq!(bus.replay(2250,2)[0]["seq"],2251);
    }

}
