# Sub3 group evidence workboard — 2026-11-20

Scope: \`dominion-sub3\` only. No UI/route/model provider changes.

## Objective

Group chat is a user-facing interface, not an authority for task completion.
The existing room transcript, cross-room parallelism, per-room serial lock and
local/API provider routing are retained.

The new \`features/collaboration/workboard.rs\` creates a **pure projection**
of the current owner's committed agent turns and host-executed operations.
\`conversation/chat/room_context.rs\` injects a single bounded, content-addressed
\`workboard:<digest>\` data frame for the next model. Only the room's actual
agent roster, owner source ID, session ID, execution ownership and operation run
ID are eligible. The projection never contains full tool arguments or raw
model claims. It carries operation IDs for inspection, tool name, optional
relative workspace path, and *verifiedEffect*, which is true only if the host
has a verified effect state **and** a verifier receipt.

The authoritative transcript, tool receipts and write-ahead audit log remain
in SQLite. The workboard is reconstructed from those durable records; it is
not another mutable database or a separate agent loop. Each frame is limited
to 6 latest speakers, 3 completed operations per speaker. Earlier records
remain retrievable. Partial tool-call messages and cross-room, cross-owner,
cross-agent and cross-execution rows are excluded.

This intentionally does not claim task success from a verified file effect,
and does not suppress user-visible peer dialogue. It is the first evidence
handoff seam toward an artifact-first group coordinator, not parallel tool
workers, WASM isolation, or speculative prefetching. No token savings claim
is made: the added frame may increase prompt tokens for evidence-heavy rooms.

## Verification

\`cargo test --bin dominion-rust features::collaboration::workboard::tests\`
\`cargo test --bin dominion-rust conversation::chat::room_context::tests\`

2026 source patterns compared: Codex multi-agent task delegation, ZeroClaw's
separate runtime/agents/providers. This adapts the isolation principle to
DOMINION's actual existing Rust session and persistence model without copying
another project's product topology.
