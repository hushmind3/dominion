# DOMINION sub2 — MCP interop boundary (2026-11-21)

This audit documents the actual implementation, not a claim of complete MCP
conformance. It concerns only the branch2 branch and preserves the
existing Rust agent Harness, permissions, tool cache, and execution receipts.

## Protocol eras

| Connection | Behavior |
|---|---|
| HTTP auto (default) | Probe **read-only** tools/list with MCP 2026-07-28 request metadata and headers. On legacy 400/404/405 without a recognized modern JSON-RPC error, fall back once to legacy initialize. Cache the era under the full server/config/workspace/credential identity. |
| HTTP protocolVersion=2026-07-28 | Modern stateless HTTP only; never send initialize or Mcp-Session-Id. Fail if incompatible. |
| HTTP protocolVersion=2024-11-05 or legacy | Existing handshake/session transport. |
| stdio | Existing initialization-based transport remains; this change does not claim modern stdio compatibility. |

The HTTP modern client is isolated in src/engine/addons/mcp_modern.rs;
the single owner-approved tool gateway remains in src/engine/addons/mcp.rs.
No second agent execution runtime or frontend navigation was introduced.

## Modern HTTP implemented

- Per-request _meta keys io.modelcontextprotocol/protocolVersion,
  clientInfo, clientCapabilities; MCP-Protocol-Version, Mcp-Method, and
  Mcp-Name headers.
- Standard and annotated Mcp-Param-* header mirroring, RFC 9110 token
  validation, case-insensitive duplicate prevention, safe primitive types,
  JS-safe integers, static property path validation, and Base64 sentinel
  encoding where plain ASCII headers are unsafe.
- JSON and request-scoped SSE results with matching JSON-RPC IDs;
  bounded response size (4 MiB); tools/list pagination with a hard 16-page,
  1,000-tool ceiling and repeated cursor detection.
- A server's unsupported-version / HeaderMismatch error does **not**
  silently trigger legacy fallback. Cancellation/uncertain writes do **not**
  trigger a second tools/call against another protocol era.
- MCP tools are still subject to the native advertised-schema gate,
  owner approval, effect ledger, and existing source-specific identity cache.

## Limits that must not be advertised as complete

- Modern stdio era detection, earlier Streamable HTTP 2025 revisions,
  full deprecated 2024 HTTP+SSE GET/endpoint transport, general MRTR
  input collection/resumption, OAuth 2.1/CIMD/EMA negotiation, optional
  MCP Apps and Tasks extensions are **not** delivered here.
- A modern server returning input_required is surfaced as a blocked action;
  DOMINION does not invent user input or resubmit the paid/effectful call.
- Rust unit tests use local mock HTTP servers. They prove wire shape and
  fallback decisions, **not** physical two-device interoperation or live
  third-party server compatibility.
- Full CI demonstrates source/tests/build health, not certification.

## Evidence gates

- cargo test --locked --workspace --lib
- cargo test --lib engine::mcp::tests::modern_probe_falls_back_to_legacy_only_before_tool_effects
- cargo test --lib engine::mcp::tests::modern_unsupported_version_never_silently_downgrades
- cargo test --lib engine::mcp::modern::tests
- node --test scripts/sub2-architecture-contract.test.mjs scripts/sub2-core-boundary.test.mjs
- node scripts/sub2-source-audit.mjs
- web typecheck/build and mobile Chromium UI smoke, as tracked by this
  branch's GitHub Actions workflow.

Specification references:
- https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http
- https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning
- https://blog.modelcontextprotocol.io/posts/2026-07-28/
