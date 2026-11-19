# Dominion native/cloud integration (sub3)

The main Cloudflare Worker implementation has been ported to `dominion-sub3`
with a separate staging Worker/D1 configuration. No production deployment has
occurred from this branch.

**Current actual client wiring:** the existing React
`web/src/components/DominionCloudAccount.tsx` supports Google and Kakao/Naver
account sign-in. On model selection, it stores a DOMINION bearer session as the
`apiKey` of an ordinary local OpenAI-compatible Rust provider, with base URL
`<staging-worker-url>/v1`. The native provider's secret vault handles this
credential; the React account card also maintains an ephemeral browser
`sessionStorage` copy. This is the observed implementation, not a claim of
native system-browser PKCE or a dedicated OS credential-store workflow.

The Rust chat/provider gateway and tools remain unchanged. Local models, BYOK,
Codex OAuth, chat/room execution and agent jobs must continue operating without
DOMINION Cloud login. The Worker owns the OAuth verification, per-account
free quota, operator upstream credentials and prepaid/entitlement handling.

**Hardening needed before public launch:** migrate the desktop login flow to
system-browser OAuth with explicit callback state; manage session revocation,
bound origins and secure refresh; validate SSE/tool use and actual upstream
usage receipts under billing; enforce atomic spend caps and reconciled
reservations; configure real merchant callbacks and verified provider
commercial terms. The current paid Cloud path rejects stream/tool requests.

See `../../docs/SUB3-CLOUD-GATEWAY-2026-11-19.md` and
`../../cloud/README.md` for the isolated setup and tested contract.
