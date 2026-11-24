## External executable MCP extensions

The native app and external hosts share this optional MCP adapter.
The independent SDK itself contains no MCP transport. From an external Rust host, call
`dominion_mcp_adapter::register_mcp_stdio(&mut registry, "my_plugin", &config, workspace).await` with a
trusted explicit command/args/env object. It starts the process, discovers
its tools and returns the exact grant names. Pass those grants to the
existing `execute` API. Unregister closes the process; a timed-out or
cancelled session cannot replay an uncertain call automatically. Discovery
supports bounded pagination. Aliases accept lowercase ASCII letters,
digits and single underscores, normalizing hyphens/dots; collisions or
unsupported names require an explicit adapter instead of ambiguous dispatch.

This shared path supports both initialized MCP stdio negotiation
(2024-11-05 through 2025-11-25) and 2026-07-28 stateless tool discovery/calls.
By default it probes server/discover with per-request metadata, then starts
a fresh legacy process only when the read-only probe cannot establish the
modern protocol. Recognized modern negotiation errors never downgrade.
Set protocolVersion explicitly to force either era. Owner-input-required
results are surfaced as errors, not silently accepted or replayed.
The native HTTP transport retains its separate existing modern/legacy
handling. The caller owns permission to start the executable; this is
not an OS sandbox or a Rust dynamic-library ABI. The integration test
runs the repository's existing Node stdio fixture through the actual
SDK model/tool loop (Node is required for that test).
