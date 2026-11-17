# Sub2 — Rust SDK verifier order (2026-11-18)

Sub3 standalone SDK acceptance.rs invalidates passes after later possible writes. Sub2 previously used three different criteria: autonomy verification_after_last_edit, generic test_after_last_edit, and acceptance_gaps command_succeeded. The generic criterion accepted any historical command success, including after a failing test or partial patch.

Changed: dominion-agent-kernel/src/verification.rs owns the chronology as a pure Rust library module, independent of provider/chat/SQLite. src/engine/verification.rs normalizes persisted host tool receipts to typed events, including failed local write attempts as barriers; dispatched:false events do not mutate. Explicit verifier criteria, free-form code-edit tests and autonomous job acceptance now consult this one core logic. No model/tool runs occur during evaluation, no DB changes, no new background loop.

Scope: Rust debug unit tests added for failed retests, partial write, latest verified test, read-only probe and opaque shell. Actual live-model completion changes and billed tokens remain unmeasured. No release/frontend/platform build performed.
