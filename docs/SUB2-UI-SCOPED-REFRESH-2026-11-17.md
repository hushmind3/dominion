# Sub2 event-scoped UI invalidation (2026-11-17)

The latest main frontend has been preserved in layout/appearance. The shared
React `useRemote` fetch hook now accepts both the existing manual
`dominion-refresh` event (global, for explicit user actions) and a new
`dominion-refresh-scoped` event (server SSE only).

`web/src/refreshScope.ts` maps known server record kinds and bus events to
API root resources. The global App SSE handler batches known resource names
into one 100ms refresh event. A single execution operation now invalidates
`/api/executions`, `/api/jobs`, and `/api/rooms`, without refetching
model/account/connection lists. Unknown future event types or record kinds
fall back to the old global refresh to avoid stale screens. Room events keep
their dedicated event path.

The old API, backend event envelope, explicit `refresh()` calls and React
layout are unchanged. This reduces redundant **possible** GET requests
per known event; no real traffic or CPU measurements are claimed. Regression
tests cover the filters and unknown-event fallback.
