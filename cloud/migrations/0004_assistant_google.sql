
CREATE TABLE IF NOT EXISTS assistant_google_pending (
 state_hash TEXT PRIMARY KEY, account_id TEXT NOT NULL REFERENCES accounts(id),
 app_session_id TEXT NOT NULL REFERENCES app_sessions(id), verifier TEXT NOT NULL,
 claimed INTEGER NOT NULL DEFAULT 0, expires_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS assistant_google_pending_exp ON assistant_google_pending(expires_at);
CREATE TABLE IF NOT EXISTS assistant_google_grants (
 account_id TEXT PRIMARY KEY REFERENCES accounts(id), refresh_cipher TEXT NOT NULL,
 scopes TEXT NOT NULL, updated_at INTEGER NOT NULL
);
