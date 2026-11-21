# DOMINION Sub3 — MCP 2026-07-28 transport compatibility

Status: code integrated on `dominion-sub3` on 2026-11-21. Source and test fixtures are committed; a complete Rust build and actual GitHub Actions passing result are **not established**.

## Versions and modes

The MCP client in `src/integrations/mcp.rs` now distinguishes protocol eras.

- HTTP and stdio default to `protocolMode: "auto"`. On a new connection, the client sends a read-only `server/discover` probe using the 2026-07-28 `_meta` envelope.
- On a successful discovery advertising `2026-07-28`, it uses stateless modern requests: no `initialize`, `notifications/initialized` or `Mcp-Session-Id`. HTTP routes with matching `MCP-Protocol-Version`, `Mcp-Method` and, where required, `Mcp-Name`.
- A handshake-era peer that clearly rejects modern discovery can fall back to `initialize`. A modern HTTP `-32022` error naming a supported legacy revision can select that revision without guessing.
- `protocolMode: "modern"` pins the 2026 revision; it does not silently downgrade.
- `protocolMode: "legacy"` pins initialization. `legacyProtocolVersion` may be `2025-11-25`, `2025-06-18`, `2025-03-26`, or `2024-11-05`. Negotiated legacy HTTP versions >= 2025-06-18 send the matching `MCP-Protocol-Version` header.

For a server record:

```json
{"name":"Modern MCP","transport":"http","url":"https://mcp.example.org/mcp","enabled":true,"protocolMode":"auto"}
```

The existing global approval policy, server credential vault, pinned tool schema, host operation ledger and execution receipts continue to authorize individual calls. Protocol discovery **does not** give an MCP server additional host authority.

## Request and effect contract

- `src/integrations/mcp/modern.rs` sets per-request protocol version, client identity and capabilities and encodes header values using the spec sentinel `=?base64?…?=` when needed.
- Modern HTTP `tools/call` rechecks a live `tools/list` schema on the same borrowed transport and mirrors supported `x-mcp-header` parameters into `Mcp-Param-*` headers. Invalid, duplicated or unreachable annotations are rejected. A tool is excluded from model discovery instead of being published with missing mandatory headers.
- Multiple SSE terminal results, a mismatched JSON-RPC ID, missing completion or a malformed SSE frame are errors, never successful tool receipts.
- A 2026 `resultType: "input_required"` **does not trigger an automatic replay** of an effecting call. The host must later implement explicit MRTR `inputResponses` coordination if it wishes to complete such requests.
- An HTTP authentication rejection, modern JSON-RPC protocol validation error, timeout or 5xx response does **not** trigger speculative fallback or reuse a failed effecting call.
- Stdio modern discovery and real process reuse are tested using `scripts/mcp-echo-modern-server.mjs`. Original stdio legacy tests remain in place.

## Reproduction

```sh
node scripts/check-runtime-architecture.mjs
node --check scripts/mcp-echo-modern-server.mjs
cargo check --locked --all-targets
cargo test --locked --bin dominion-rust modern_mcp_http_uses_discovery_stateless_headers_and_live_pinned_schema
cargo test --locked --bin dominion-rust real_modern_stdio_never_initializes_and_persists_one_live_process
cargo test --locked --bin dominion-rust http_reuses_mcp_session_and_reconnects_after_credential_rotation
cargo test --locked --bin dominion-rust stdio_reuses_one_live_mcp_process_and_rotates_on_config_change
```

The Sub3 CI workflow also records Rustfmt and Clippy diagnostics; these are currently visible but non-blocking because whole-workspace formatting and warning cleanup remain incomplete.

## What is not claimed

This is a modern **transport plus basic tools/list/tools/call** implementation, not a complete 2026 MCP SDK. It does not yet provide full MRTR user-input continuation, modern OAuth/CIMD issuer validation, Tasks extension orchestration, interoperable notifications/subscriptions or a benchmarked tool catalog cache. Real remote server compatibility and hosted CI success remain unverified.

Primary sources:
- https://modelcontextprotocol.io/specification/2026-07-28/basic/transports
- https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/transports/streamable-http.mdx
- https://blog.modelcontextprotocol.io/posts/2026-07-28/
