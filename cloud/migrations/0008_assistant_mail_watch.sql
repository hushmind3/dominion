
CREATE TABLE IF NOT EXISTS assistant_mail_watch (
 account_id TEXT PRIMARY KEY REFERENCES accounts(id), app_session_id TEXT NOT NULL REFERENCES app_sessions(id),
 cursor TEXT NOT NULL, filters TEXT NOT NULL, state TEXT NOT NULL,
 lease TEXT, lease_until INTEGER NOT NULL DEFAULT 0, next_check INTEGER NOT NULL,
 last_check INTEGER, error TEXT, created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS assistant_mail_watch_due ON assistant_mail_watch(state,next_check,lease_until);
CREATE TABLE IF NOT EXISTS assistant_mail_alerts (
 account_id TEXT NOT NULL REFERENCES accounts(id), id TEXT NOT NULL, cipher TEXT NOT NULL,
 created_at INTEGER NOT NULL, PRIMARY KEY(account_id,id)
);
CREATE INDEX IF NOT EXISTS assistant_mail_alerts_time ON assistant_mail_alerts(account_id,created_at);
