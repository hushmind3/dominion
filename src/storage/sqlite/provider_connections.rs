//! Provider configuration and its encrypted credential commit together.
use super::*;

pub enum ProviderSecretChange {
    Keep,
    Clear,
    Replace(String),
}
pub enum ProviderWriteError {
    Changed,
    Storage(String),
}
pub struct ProviderCommit {
    pub provider: Value,
    pub displaced: Vec<Value>,
}

fn storage(error: impl ToString) -> ProviderWriteError {
    ProviderWriteError::Storage(error.to_string())
}
fn current(tx: &rusqlite::Transaction<'_>, id: &str) -> Result<Option<Value>, ProviderWriteError> {
    tx.query_row(
        "SELECT data,id,created_at,updated_at FROM records WHERE kind='provider' AND id=?1",
        [id],
        record_from_row,
    )
    .optional()
    .map_err(storage)
}

impl Store {
    pub fn save_provider_connection(
        &self,
        record: &Value,
        expected: Option<&Value>,
        secret: ProviderSecretChange,
        expected_secret: Option<&str>,
    ) -> Result<ProviderCommit, ProviderWriteError> {
        let id = record["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| storage("Provider record has no ID"))?;
        let mut conn = self.conn.lock().map_err(storage)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage)?;
        if current(&tx, id)?.as_ref() != expected {
            return Err(ProviderWriteError::Changed);
        }
        let secret_name = format!("provider:{id}");
        if !matches!(&secret, ProviderSecretChange::Keep) {
            let actual: Option<String> = tx
                .query_row(
                    "SELECT value FROM secrets WHERE name=?1",
                    [&secret_name],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage)?;
            if actual.as_deref() != expected_secret {
                return Err(ProviderWriteError::Changed);
            }
        }
        upsert_tx(&tx, "provider", record).map_err(storage)?;
        match secret {
            ProviderSecretChange::Keep => {}
            ProviderSecretChange::Clear => {
                tx.execute(
                    "DELETE FROM secrets WHERE name=?1 OR name=?2",
                    params![secret_name, format!("provider:{id}:oauth")],
                )
                .map_err(storage)?;
            }
            ProviderSecretChange::Replace(sealed) => {
                if sealed.is_empty() {
                    return Err(storage("Credential encryption failed"));
                }
                upsert_secret(&tx, &secret_name, &sealed).map_err(storage)?;
            }
        }
        let mut displaced = Vec::new();
        if record["reflection"] == true {
            let others = select_records(
                &tx,
                "SELECT data,id,created_at,updated_at FROM records WHERE kind='provider' AND id<>?1
                 AND (json_extract(data,'$.reflection')=1 OR json_extract(data,'$.learning')=1)",
                &[&id],
            )
            .map_err(storage)?;
            for mut other in others {
                other["reflection"] = json!(false);
                other["learning"] = json!(false);
                upsert_tx(&tx, "provider", &other).map_err(storage)?;
                if let Some(saved) = current(&tx, other["id"].as_str().unwrap_or_default())? {
                    displaced.push(saved);
                }
            }
        }
        let provider = current(&tx, id)?.ok_or_else(|| storage("Saved provider unavailable"))?;
        tx.commit().map_err(storage)?;
        Ok(ProviderCommit {
            provider,
            displaced,
        })
    }

    pub fn remove_provider_connection(
        &self,
        id: &str,
        expected: &Value,
    ) -> Result<(), ProviderWriteError> {
        let mut conn = self.conn.lock().map_err(storage)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage)?;
        if current(&tx, id)?.as_ref() != Some(expected) {
            return Err(ProviderWriteError::Changed);
        }
        tx.execute("DELETE FROM records WHERE kind='provider' AND id=?1", [id])
            .map_err(storage)?;
        // Remove both API access and account-refresh credentials in the same commit.
        tx.execute(
            "DELETE FROM secrets WHERE name=?1 OR name=?2",
            params![format!("provider:{id}"), format!("provider:{id}:oauth")],
        )
        .map_err(storage)?;
        tx.execute(
            "DELETE FROM records WHERE kind='codex_model_catalog' AND id=?1",
            [id],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)
    }
}
