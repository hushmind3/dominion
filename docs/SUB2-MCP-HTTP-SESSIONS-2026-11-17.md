# Sub2 demand-driven MCP HTTP sessions (2026-11-17)

The original MCP client created a new HTTP session, initialized it and
discarded its session ID for **every** `tools/list` and `tools/call`.
`mcp::http_request` now reuses a complete initialized Streamable HTTP
session for matching server config + workspace + vault credential only.
Different servers or rotated tokens have distinct SHA-256 cache identities;
the plaintext token is never retained in the map key. A short lock covers
pool lookup, while each server has its own asynchronous mutex so separate
servers can run concurrently.

The pool keeps at most 16 lazily created sessions, drops idle entries after
120 seconds, and is cleared by `mcp::invalidate`. A failed call or cancelled
in-flight future drops the cache lease. It **never automatically retries**
`tools/call`, since its remote side effect may be uncertain. Subsequent
requests initialize a new MCP session. Tool approval and recorded effects
are still enforced by the existing Rust runner.

Stdio MCP remains one-shot with child process kill-on-drop, avoiding a
long-running process manager without explicit per-workspace lifecycle
contracts. The HTTP path includes credential/isolation/cancellation cache
tests; request-count, latency and cross-server integration benchmarks are
not claimed without actual connected servers.
