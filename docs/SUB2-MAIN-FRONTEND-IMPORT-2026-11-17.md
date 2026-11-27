# Sub2 main frontend transplant — 2026-11-17

Source: `main` at `76e2e5e5de742d8f43761e8a9cd3265158030b00` (latest HEAD when imported).
Destination: `branch2` based on `0a25a4080503cd4d795acba69603c436a09108f7`.

All 16 differing files under `web/` were taken directly from the pinned
main source tree, including the new Chat/Rooms conversation settings and
execution-permissions controls, Settings dialog layout, navigation, theme,
translation strings and the two new shared React components.

- Frontend files unchanged between branches were retained as identical blobs.
- The root Rust agent execution engine, separate `dominion-agent-kernel`,
  TypeScript SDK, SQLite schema, models, accounts and existing user data were
  **not merged from main** or modified in this transplant.
- Main itself was not changed.
- UI options are wired to the existing Rust APIs; functional compatibility is
  subject to the destination branch's TypeScript/build and CI integration checks.
- Source `web/src/pages/Rooms.tsx` renders the complete
  `roster.map(...)` in its header, preserving visibility of all participants.

The pinned commit is recorded to make future UI-only updates reproducible.

The imported main Operations UI does not display Sub2's previous evaluation summary panel. The Rust `/api/evaluation/summary` endpoint and the TypeScript SDK `evaluation` API remain available. The architectural test checks those preserved backend/SDK contracts while validating that the new Settings UI is mounted; it does not require retired Sub2-only screen elements.
