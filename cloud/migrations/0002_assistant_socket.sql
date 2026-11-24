-- Apply after 0001_app_sessions.sql. Existing app grants remain inference-only.
ALTER TABLE app_sessions ADD COLUMN capability TEXT NOT NULL DEFAULT 'inference' CHECK(capability IN ('inference','assistant_remote','assistant_pc'));
CREATE TABLE IF NOT EXISTS assistant_devices (
 id TEXT PRIMARY KEY,account_id TEXT NOT NULL REFERENCES accounts(id),name TEXT NOT NULL,
 app_session_id TEXT REFERENCES app_sessions(id),
 token_hash TEXT UNIQUE NOT NULL,last_seen INTEGER NOT NULL,revoked INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS assistant_devices_account ON assistant_devices(account_id,revoked);
CREATE TABLE IF NOT EXISTS assistant_ws_requests (
 id TEXT PRIMARY KEY,account_id TEXT NOT NULL REFERENCES accounts(id),device_id TEXT NOT NULL REFERENCES assistant_devices(id),
 client_id TEXT NOT NULL,device_connection TEXT NOT NULL,fingerprint TEXT NOT NULL,state TEXT NOT NULL,
 created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS assistant_ws_account ON assistant_ws_requests(account_id,created_at);
CREATE INDEX IF NOT EXISTS assistant_ws_device ON assistant_ws_requests(device_id,state,expires_at);
CREATE INDEX IF NOT EXISTS assistant_ws_connection ON assistant_ws_requests(device_connection,state);
CREATE INDEX IF NOT EXISTS assistant_ws_exp ON assistant_ws_requests(expires_at);
