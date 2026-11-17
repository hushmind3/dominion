# Sub2 — Money review and archived decision history (2026-11-18)

## Source / comparison
- OpenHands Software Agent SDK stores an append-only event log and separates user-facing projection from persistent evidence. Source: https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/conversation/state.py (2026).
- Microsoft Event Sourcing guidance identifies optimistic concurrency and immutable historical events as requirements for reconstructing decisions: https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing
- Sub2's previous `POST /api/money/candidates/:id` read a candidate, modified its status and called `Store::put`: no revision guard, no retained decision history, and a concurrent Money implementation approval could be overwritten. UI did not expose archived candidates.

## Implementation
- New `src/api/money/review.rs` isolates candidate search, decision update and durable history from the large Money API adapter.
- Reuse the existing `Store::mutate_record` SQLite transaction to atomically update the current candidate and append one `money_decision` with revision, field deltas and owner's optional reason.
- `expectedRevision` supports optimistic concurrency, `reviewRevision` starts at zero for legacy records. Original candidate evidence, execution history, approval receipts and source links remain untouched.
- Archive supports `status=archived` (on_hold + rejected), any valid status or all, plus search q and bounded limit. Indexes cover exact candidate history and candidate status.
- An already-approved implementation cannot be silently parked/rejected. A held candidate must be returned to research status before new implementation approval.
- GET candidate detail exposes `decisions`; separate history endpoint returns up to 200 newest immutable reviews.
- A no-op update does not add a fake decision event. Incorrect numeric economics reject before mutation; concurrent expectedRevision=0 writes cannot both commit.

## Verification
- Related Rust development-profile unit tests added: hold/resume, indexed archive, history order, source preservation, concurrent stale update rejection, economics validation and approved job guard.
- No release builds/cargo check/frontend build, no synthetic model calls, no performance benchmark. Historical candidates remain queryable without schema backfill.

## GUI and SDK wiring
- Existing `web/src/pages/Money.tsx` offers a Hold action on unimplemented drafts, loads the archived candidate list with a capped query, shows immutable decision events and source URLs, and returns a held/rejected candidate to `researching` via the original candidate POST API.
- Owner approval to implement remains a distinct operation; holding a candidate never auto-starts tools or spends tokens.
- `sdk/typescript/src/index.ts` adds typed `MoneyReviewEvent` and methods `money.archive`, `money.review`, `money.history`, `money.candidateDetails` while retaining legacy calls.
- Frontend and TypeScript compilation/testing remain deferred under the project's development policy.
