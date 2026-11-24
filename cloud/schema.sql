CREATE TABLE IF NOT EXISTS accounts (
  id TEXT PRIMARY KEY,
  email TEXT NOT NULL DEFAULT '',
  credits INTEGER NOT NULL DEFAULT 0 CHECK (credits >= 0),
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
  token_hash TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS usage (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL,
  model TEXT NOT NULL,
  reserved INTEGER NOT NULL,
  charged INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS usage_account ON usage(account_id, created_at);

CREATE TABLE IF NOT EXISTS free_quota (account_day TEXT PRIMARY KEY, used INTEGER NOT NULL DEFAULT 0);

-- Account-scoped payment intents. Prices and entitlements are set by the Worker.
CREATE TABLE IF NOT EXISTS checkout_orders (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  plan TEXT NOT NULL CHECK (plan IN ('pro')),
  amount_krw INTEGER NOT NULL CHECK (amount_krw > 0),
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','processing','paid','failed')),
  payment_key TEXT UNIQUE,
  created_at INTEGER NOT NULL,
  paid_at INTEGER
);
CREATE INDEX IF NOT EXISTS checkout_orders_account ON checkout_orders(account_id,created_at);
CREATE TABLE IF NOT EXISTS subscriptions (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id),
  plan TEXT NOT NULL DEFAULT 'free' CHECK (plan IN ('free','pro')),
  status TEXT NOT NULL DEFAULT 'inactive' CHECK (status IN ('inactive','active','past_due','canceled')),
  current_period_end INTEGER,
  updated_at INTEGER NOT NULL
);

-- Provider-verified paid access; never store resident numbers or raw identity payloads.
CREATE TABLE IF NOT EXISTS identity_verifications (
 account_id TEXT PRIMARY KEY REFERENCES accounts(id),
 provider TEXT NOT NULL,
 verified_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS identity_attempts (
 id TEXT PRIMARY KEY,
 account_id TEXT NOT NULL REFERENCES accounts(id),
 status TEXT NOT NULL CHECK(status IN ('pending','verified')),
 created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS identity_attempts_account ON identity_attempts(account_id,created_at);

-- Email magic-link requests never contain raw bearer sessions or link tokens.
-- A separate secret poll token authorizes delivery only to the initiating device.
CREATE TABLE IF NOT EXISTS email_login_attempts (
  id TEXT PRIMARY KEY,
  email TEXT NOT NULL,
  email_hash TEXT NOT NULL,
  ip_hash TEXT NOT NULL,
  poll_hash TEXT NOT NULL,
  verify_hash TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending','verified','consumed')),
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS email_login_attempts_rate_email ON email_login_attempts(email_hash,created_at);
CREATE INDEX IF NOT EXISTS email_login_attempts_rate_ip ON email_login_attempts(ip_hash,created_at);

-- Assistant uses the same accounts and sessions as desktop. Scoped device credentials
-- are distinct from account sessions. No arbitrary PC endpoint is exposed.
CREATE TABLE IF NOT EXISTS assistant_login (
 id TEXT PRIMARY KEY,poll_hash TEXT NOT NULL,ticket_hash TEXT NOT NULL,nonce TEXT NOT NULL,
 ip_hash TEXT NOT NULL,browser_hash TEXT,account_id TEXT REFERENCES accounts(id),session_until INTEGER,
 status TEXT NOT NULL,created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS assistant_login_ip ON assistant_login(ip_hash,created_at);
CREATE INDEX IF NOT EXISTS assistant_login_exp ON assistant_login(expires_at);
CREATE TABLE IF NOT EXISTS assistant_devices (
 id TEXT PRIMARY KEY,account_id TEXT NOT NULL REFERENCES accounts(id),name TEXT NOT NULL,
 token_hash TEXT UNIQUE NOT NULL,last_seen INTEGER NOT NULL,revoked INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS assistant_devices_account ON assistant_devices(account_id,revoked);
CREATE TABLE IF NOT EXISTS assistant_rpc (
 id TEXT PRIMARY KEY,account_id TEXT NOT NULL REFERENCES accounts(id),device_id TEXT NOT NULL REFERENCES assistant_devices(id),
 route TEXT NOT NULL,payload TEXT NOT NULL,fingerprint TEXT NOT NULL,state TEXT NOT NULL,
 status INTEGER,result TEXT,created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS assistant_rpc_queue ON assistant_rpc(device_id,state,created_at);
CREATE INDEX IF NOT EXISTS assistant_rpc_account ON assistant_rpc(account_id,created_at);
CREATE INDEX IF NOT EXISTS assistant_rpc_exp ON assistant_rpc(expires_at);
