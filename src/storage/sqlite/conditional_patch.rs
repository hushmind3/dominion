//! All fields in an owner command are checked before any record is changed.
use super::*;
pub struct ConditionalPatch {
    pub kind: String,
    pub id: String,
    pub expected: Vec<(String, Value)>,
    pub patch: Value,
}
impl Store {
    pub fn patch_records_if_all(
        &self,
        changes: &[ConditionalPatch],
    ) -> Result<Option<Vec<Value>>, String> {
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let mut rows = Vec::new();
        let mut identities = std::collections::HashSet::new();
        for change in changes {
            let fields = change
                .patch
                .as_object()
                .filter(|fields| !fields.contains_key("id"))
                .ok_or("Conditional patch must be an object without an ID")?;
            if change.expected.is_empty() || !identities.insert((&change.kind, &change.id)) {
                return Err(
                    "Conditional patch requires a unique record and expected fields".into(),
                );
            }
            let row: Option<Value> = tx
                .query_row(
                    "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
                    params![change.kind, change.id],
                    record_from_row,
                )
                .optional()
                .map_err(|error| error.to_string())?;
            let Some(mut row) = row else { return Ok(None) };
            if change
                .expected
                .iter()
                .any(|(field, value)| row[field] != *value)
            {
                return Ok(None);
            }
            for (field, value) in fields {
                row[field] = value.clone();
            }
            rows.push(row);
        }
        // Validation above is complete. No partial persona/room save can escape.
        for (change, row) in changes.iter().zip(&rows) {
            if change
                .patch
                .as_object()
                .is_some_and(|fields| !fields.is_empty())
            {
                upsert_tx(&tx, &change.kind, row)?;
            }
        }
        rows.clear();
        for change in changes {
            rows.push(
                tx.query_row(
                    "SELECT data,id,created_at,updated_at FROM records WHERE kind=?1 AND id=?2",
                    params![change.kind, change.id],
                    record_from_row,
                )
                .map_err(|error| error.to_string())?,
            );
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(Some(rows))
    }
}
