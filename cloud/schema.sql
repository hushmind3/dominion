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

-- Billing records are always scoped to the authenticated DOMINION account.
-- A payment is never marked paid by a browser request.
CREATE TABLE IF NOT EXISTS subscriptions (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id),
  plan TEXT NOT NULL DEFAULT 'free' CHECK (plan IN ('free','pro')),
  status TEXT NOT NULL DEFAULT 'inactive' CHECK (status IN ('inactive','active','past_due','canceled')),
  current_period_end INTEGER,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS payments (
  order_id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  amount_krw INTEGER NOT NULL CHECK (amount_krw > 0),
  status TEXT NOT NULL CHECK (status IN ('pending','paid','failed','canceled')),
  provider TEXT NOT NULL DEFAULT 'toss',
  payment_key TEXT UNIQUE,
  created_at INTEGER NOT NULL,
  confirmed_at INTEGER
);
CREATE INDEX IF NOT EXISTS payments_account ON payments(account_id,created_at);

-- Checkout intents belong to the authenticated account; prices are server-side.
CREATE TABLE IF NOT EXISTS checkout_orders (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  plan TEXT NOT NULL,
  amount_krw INTEGER NOT NULL CHECK (amount_krw > 0),
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','processing','paid','failed')),
  payment_key TEXT UNIQUE,
  created_at INTEGER NOT NULL,
  paid_at INTEGER
);
CREATE INDEX IF NOT EXISTS checkout_orders_account ON checkout_orders(account_id,created_at);
CREATE TABLE IF NOT EXISTS subscriptions (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id),
  plan TEXT NOT NULL,
  valid_until INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
