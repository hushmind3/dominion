# DOMINION Cloud Gateway (initial implementation)

This is the first independent DOMINION account gateway, **not** a Nous Portal integration.
The existing desktop UI and Rust execution engine remain unchanged.

Cloudflare Worker + D1 provides Google ID-token verification (RS256 against Google's JWKs),
a DOMINION bearer session, account balance, and a single-model OpenAI-compatible relay.
Users never receive the upstream provider API key. No paid model call is permitted without
a positive prepaid balance. The first release intentionally rejects streaming, tools,
unconfigured models, and unsupported options.

## Provisioning

Create a Cloudflare D1 database, replace the placeholder ID in `wrangler.toml`,
and apply `schema.sql` with Wrangler D1 migrations/execute. Create a Google OAuth
**web** client ID with your deployed frontend origin authorized.

Configure Worker vars: `GOOGLE_CLIENT_ID`, `ALLOWED_ORIGIN` (exact HTTPS origin),
`UPSTREAM_URL`, `MODEL_ID`, `PRICE_INPUT_MICRO_KRW`, `PRICE_OUTPUT_MICRO_KRW`.
Store `UPSTREAM_API_KEY` using `wrangler secret put`.
All prices are integer micro-KRW per token; one KRW equals 1,000 micro-KRW.

POST `/v1/auth/google` with `{"credential":"<Google ID token>"}` and receive
a DOMINION bearer token. GET `/v1/me` and POST `/v1/chat/completions`
require `Authorization: Bearer <DOMINION token>`.

## Before production

This is an **un-deployed backend foundation**, not a ready billing service.
Add a payment webhook with verified signatures and idempotent credit grants;
never let clients set balances. Add rate limits, replay protection, session
revocation, provider-specific pricing, streaming/tool support, admin audit,
recovery of stranded reservations, and production tests. A failed upstream
request is refunded; a successful request uses reported usage to settle.
The byte-count reservation is deliberately conservative but is not a formal
upper bound for every tokenizer, so reconcile and strengthen before charging
real customers. Do not put the Worker upstream secret in the desktop client.

Google login must use the official Google Identity Services client on the
frontend and submit the resulting ID token to this backend. Google OAuth
credentials, Cloudflare D1 ID, and model supplier credentials are not
committed to this repository.
