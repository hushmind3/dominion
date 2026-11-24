
CREATE TABLE IF NOT EXISTS assistant_google_jobs (
 account_id TEXT NOT NULL REFERENCES accounts(id),
 id TEXT NOT NULL, app_session_id TEXT NOT NULL REFERENCES app_sessions(id),
 operation TEXT NOT NULL, status TEXT NOT NULL,
 result_cipher TEXT, error TEXT, lease TEXT,
 attempts INTEGER NOT NULL DEFAULT 0, lease_until INTEGER NOT NULL DEFAULT 0,
 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
 PRIMARY KEY(account_id,id)
);
CREATE INDEX IF NOT EXISTS assistant_google_jobs_due ON assistant_google_jobs(status,lease_until,expires_at);
