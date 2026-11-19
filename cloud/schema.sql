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
