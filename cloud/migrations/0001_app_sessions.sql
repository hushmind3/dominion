
-- An external app generates its own bearer locally. Only the hash reaches D1.
-- Pending pairings are not accounts, and cannot invoke inference.
CREATE TABLE IF NOT EXISTS app_sessions (
  id TEXT PRIMARY KEY,
  token_hash TEXT NOT NULL UNIQUE,
  app_name TEXT NOT NULL,
  ip_hash TEXT NOT NULL,
  account_id TEXT REFERENCES accounts(id),
  status TEXT NOT NULL CHECK(status IN ('pending','approved','revoked')),
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  approved_at INTEGER
);
CREATE INDEX IF NOT EXISTS app_sessions_owner ON app_sessions(account_id,created_at);
CREATE INDEX IF NOT EXISTS app_sessions_rate ON app_sessions(ip_hash,created_at);
