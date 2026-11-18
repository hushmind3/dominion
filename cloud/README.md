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

## Google login and free models (November 2026)

The Model Hub now displays a DOMINION Cloud account card. Configure the frontend build
with `VITE_DOMINION_CLOUD_URL=https://YOUR_WORKER.workers.dev` and
`VITE_DOMINION_GOOGLE_CLIENT_ID=YOUR_GOOGLE_WEB_CLIENT_ID`. The Worker must use the
same `GOOGLE_CLIENT_ID` and set `ALLOWED_ORIGIN` to the exact frontend origin.
Use Google Identity Services on a supported HTTPS web origin. Google Sign-In
is not supported inside embedded desktop webviews; native desktop needs a
system-browser authorization-code/PKCE flow before production.

The Worker `FREE_MODELS_JSON` variable is a JSON array, e.g.
`[{"id":"example-free-model","upstream_model":"provider-specific-id"}]`.
Set `FREE_DAILY_LIMIT=20` for an initial per-account request limit. The
configured `UPSTREAM_URL` and secret `UPSTREAM_API_KEY` must refer to an
inference supplier that actually allows the intended commercial gateway use.
An empty model array intentionally displays no models. No free provider
credentials or quota have been provisioned by committing this code.

`GET /v1/models` lists configured free models for authenticated accounts.
`POST /v1/chat/completions` forwards supported free model requests and SSE
streams through the Worker with a daily account quota. Registering a model
in the desktop UI saves the DOMINION session as the provider credential in
the existing local Rust provider store. Reauthenticate when that session expires.

**Not production-ready:** the free quota is per request rather than tokens,
there is no global upstream spend ceiling, and upstream billing may continue
after client cancellation. Add upstream billing controls, token metering,
rate limiting by IP and account, replay/abuse protection, expiry handling,
and tests before enabling broad public signup.
