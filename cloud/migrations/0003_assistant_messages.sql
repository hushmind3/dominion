CREATE TABLE IF NOT EXISTS assistant_messages (
 id TEXT NOT NULL,account_id TEXT NOT NULL REFERENCES accounts(id),device_id TEXT NOT NULL REFERENCES assistant_devices(id),
 text TEXT NOT NULL,created_at INTEGER NOT NULL,PRIMARY KEY(device_id,id)
);
CREATE INDEX IF NOT EXISTS assistant_messages_account ON assistant_messages(account_id,created_at);
CREATE TABLE IF NOT EXISTS assistant_push (
 token TEXT PRIMARY KEY,account_id TEXT NOT NULL REFERENCES accounts(id),app_session_id TEXT NOT NULL REFERENCES app_sessions(id),created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS assistant_push_account ON assistant_push(account_id);
