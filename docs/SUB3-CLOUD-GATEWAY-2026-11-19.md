# Sub3 Cloud Gateway integration

The cloud gateway implementation, SQL schema, account OAuth handlers, model registry,
free-model inference, paid-prepaid relay, identity verification and TossPayments
flow were ported from `main` (2026-11-19). This is a **staging branch** of
the same product, not a second independent account system.

## Components

- `cloud/src/index.js`: Cloudflare Worker endpoints. Operator model secrets
  stay on the Worker; local BYOK providers remain unchanged.
- `cloud/schema.sql`: isolated D1 account, session, quota, credit and order tables.
- `cloud/wrangler.toml`: `dominion-cloud-sub3` Worker with an *unprovisioned*
  **separate** `dominion-cloud-sub3` database ID placeholder.
- `web/src/components/DominionCloudAccount.tsx`: existing React
  Google/Kakao/Naver login, provider registration and billing UI.
- `src/api/providers.rs` and `src/engine/chat/provider.rs`:
  existing OpenAI-compatible Rust provider pipeline. The account card saves
  `<gateway>/v1` and a Dominion bearer session as its provider credential.
- `services/dominion-cloud/gateway-contract.ts`: typed future backend
  accounting boundary.

## Test without deployment

```sh
node --check cloud/src/index.js
node --test cloud/tests/*.test.mjs
```

GitHub workflow `.github/workflows/sub3-cloud-gateway.yml` checks the Worker,
front-end contract and D1 schema on sub3 pushes and has **no deploy job**.

## Provision only an independent staging deployment

1. Create a fresh Cloudflare D1 named `dominion-cloud-sub3`; fill its new
   `database_id` in `cloud/wrangler.toml`.
2. Apply `cloud/schema.sql` to that new D1. Configure `ALLOWED_ORIGIN`,
   web OAuth client IDs/redirects, and any upstream provider credentials as
   Worker vars/secrets (never in Vite).
3. Set the *public-only* frontend variables using
   `web/.env.cloud-sub3.example` as a guide; build the frontend for that origin.
4. Verify signup/login, token revocation/expiry, free-model streaming, quota
   and payment test mode end to end before enabling production purchases.

The main production Worker/D1 are **not** modified, redeployed or reused.
The D1 placeholder intentionally prevents accidental deployment of this branch.

## Known gaps carried over from main

The paid route currently rejects streaming/tool-call requests; Pro entitlement
and prepaid inference credits are different accounting objects. Full payment
settlement, concurrent-spend enforcement and native desktop system-browser
OAuth still require production hardening. Green tests establish isolated
source-contract behavior, **not** production credential readiness.
