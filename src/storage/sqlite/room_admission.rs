//! Insert-only owner-message admission with a durable retry receipt.
use super::*;

pub struct RoomReceipt {
    pub room: Value,
    pub message: Value,
    pub job: Value,
    pub created: bool,
}
pub enum RoomAdmissionError {
    Changed,
    Storage(String),
}
fn storage(error: impl ToString) -> RoomAdmissionError {
    RoomAdmissionError::Storage(error.to_string())
}
fn record(
    tx: &rusqlite::Transaction<'_>,
    kind: &str,
    id: &str,
) -> Result<Option<Value>, RoomAdmissionError> {
    tx.query_row(
        "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
        params![kind, id],
        record_from_row,
    )
    .optional()
    .map_err(storage)
}

impl Store {
    pub fn admit_room_message(
        &self,
        expected_room: &Value,
        message: &Value,
        job: &Value,
    ) -> Result<RoomReceipt, RoomAdmissionError> {
        let id = expected_room["id"]
            .as_str()
            .ok_or_else(|| storage("Room ID missing"))?;
        let job_id = job["id"]
            .as_str()
            .ok_or_else(|| storage("Job ID missing"))?;
        let session = message["session_id"]
            .as_str()
            .ok_or_else(|| storage("Message session missing"))?;
        let mut conn = self.conn.lock().map_err(storage)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage)?;
        let mut room = record(&tx, "room", id)?.ok_or(RoomAdmissionError::Changed)?;
        if let Some(existing) = record(&tx, "job", job_id)? {
            let source = existing["input"]["sourceMessageId"]
                .as_str()
                .or_else(|| existing["input"]["messageId"].as_str())
                .ok_or_else(|| storage("Receipt source missing"))?;
            let message =
                query_message(&tx, source).ok_or_else(|| storage("Receipt message missing"))?;
            return Ok(RoomReceipt {
                room,
                message,
                job: existing,
                created: false,
            });
        }
        // Compare owner-controlled fields only; ordinary worker progress does
        // not invalidate a submission. A reset or permission edit does.
        for field in [
            "sessionId",
            "agentIds",
            "routingMode",
            "rounds",
            "maxTurns",
            "tokenBudget",
            "contextBudgetTokens",
            "toolsEnabled",
            "autoLearning",
            "autoModels",
            "models",
            "reasoningEffort",
            "systemPromptCommon",
            "systemPromptOverride",
        ] {
            if room[field] != expected_room[field] {
                return Err(RoomAdmissionError::Changed);
            }
        }
        let bound = record(&tx, "session", session)?.ok_or(RoomAdmissionError::Changed)?;
        let epoch = record(&tx, "session_cancel", session)?.unwrap_or(Value::Null);
        if room["sessionId"].as_str() != Some(session)
            || bound["roomId"].as_str() != Some(id)
            || epoch["generation"].as_u64().unwrap_or(0)
                != job["input"]["generation"].as_u64().unwrap_or(0)
        {
            return Err(RoomAdmissionError::Changed);
        }
        let source =
            insert_message(&tx, message).ok_or_else(|| storage("Owner message insert failed"))?;
        tx.execute(
            "INSERT INTO records(kind,id,data) VALUES('job',?1,?2)",
            params![job_id, job.to_string()],
        )
        .map_err(storage)?;
        room["lastOwnerMessageId"] = json!(source);
        room["lastSubmittedAt"] = json!(chrono::Utc::now().to_rfc3339());
        room["stoppedAt"] = Value::Null;
        upsert_tx(&tx, "room", &room).map_err(storage)?;
        let message =
            query_message(&tx, &source).ok_or_else(|| storage("Saved owner message missing"))?;
        let job = record(&tx, "job", job_id)?.ok_or_else(|| storage("Saved room job missing"))?;
        let room = record(&tx, "room", id)?.ok_or_else(|| storage("Saved room missing"))?;
        tx.commit().map_err(storage)?;
        Ok(RoomReceipt {
            room,
            message,
            job,
            created: true,
        })
    }
}
