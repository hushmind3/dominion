# DOMINION sub3: verified architecture implementation (2026-11-20)

Scope: **dominion-sub3 only**. No UI/navigation change. Never merge automatically.

## Source-accurate structure

- The **runtime** is \`crates/dominion-agent-runtime\`: typed \`RunRef\`/\`RunPhase\`, limits, snapshot, approvals, tool-call admission and dispatch.
- \`crates/dominion-agent-algorithms\` owns \`ReActLoop\` with host-selected version-pinned strategy; \`crates/dominion-agent-sdk\` re-exports one public harness API. The streaming chat adapter already uses the same core dispatcher and journal. No second runnable Rust model loop has been introduced.
- Product \`src/host/execution\` owns SQLite, workspace, approval, and job leases. \`src/features/collaboration\` adds **Room Chat** and separately **Task Team**. React and Tauri are unmodified.
- \`src/features/providers/gateway.rs\` includes model+endpoint capability profiles; only explicit false fails native tool admission, legacy unknown profiles retain compatibility. Quality/cost routing also filters explicitly incompatible candidates.
- \`src/events.rs\` provides optional typed RunRef on job/execution events without breaking the existing SSE message envelope. SQLite alone allocates durable sequence numbers; reconnect drains all available event pages.
- Strategies are compared from **actual terminal native receipts and usage** through GET \`/api/algorithms/evaluation\`. No RSI or automated source acceptance.

## Task Team HTTP contract

POST \`/api/task-teams\` with:
\`\`\`json
{
 "goal":"Implement safe feature",
 "maxParallel":3,
 "tasks":[
   {"id":"investigate","prompt":"Read relevant source","mode":"read","runtime":"native"},
   {"id":"implement","prompt":"Implement requested change","mode":"write",
    "dependsOn":["investigate"],"runtime":"native",
    "checks":[{"type":"file_exists","path":"src/feature.rs"}]}
 ]
}
\`\`\`

GET \`/api/task-teams\`, GET \`/api/task-teams/:id\`,
POST \`/api/task-teams/:id/cancel\`. This is API-only; existing UI unchanged.

A team lead executes as a durable \`job(kind=team)\`; workers are ordinary
\`job(kind=prompt)\` with the same Rust SDK kernel and budgets. At most 3
read workers may run concurrently. Writers serialize by workspace lease
and do not overlap local read workers in the same team. The DAG is validated
before enqueue; child IDs are idempotent and parent-scoped. Dependency
observations are imported from committed child jobs as **lower-trust data**
with bounded answer excerpts and operation IDs; raw tool arguments and
model reasoning are not copied.

A writer must have an explicit artifact acceptance check and a successful
effectful tool receipt before unlocking dependent work. Task status alone or
a textual "done" is never sufficient. An external worker's completed response
is tagged \`external_unverified\`, not a native tool receipt.

## External agent bridge

For opt-in external read-only workers, set
\`DOMINION_EXTERNAL_AGENTS=1\`. Use \`"runtime":"codex"\` or
\`"runtime":"claude"\` on a read task. Optional binary overrides:
\`DOMINION_CODEX_BINARY\` and \`DOMINION_CLAUDE_BINARY\`.
They are standalone agent runtimes, **not** ModelProviders. The Codex
adapter uses App Server thread/turn events; the Claude adapter uses
headless JSON with plan/read-only mode. They have independent session IDs,
bounded execution/output, and do not inherit DOMINION internal tool
permissions. Unmediated approval requests fail closed. Interrupted or
uncertain external calls are held for manual review, not retried as clean runs.

## Remaining deployment constraints

- No native Codex or Claude binary is bundled, installed, authenticated or
  implicitly trusted. Live vendor integration requires environment-specific
  protocol testing. No cross-process native SSE broadcast bus; durable
  replay provides reconnect recovery.
- Native and external Task Team workers are supervised by the existing
  Rust job scheduler, not a distributed cluster. Read/write restrictions
  are bounded to Task Team scope and the file-operation host gates; not a
  security claim about arbitrary third-party CLI code.
- No full OS-specific Tauri release build is asserted unless CI confirms it.

## Regression entrypoints

\`cargo test --bin dominion-rust features::collaboration::task_team::tests\`
\`cargo test --bin dominion-rust integrations::agent_bridge::tests\`
\`cargo test --bin dominion-rust features::providers::gateway::tests\`
\`cargo test --bin dominion-rust features::cognition::strategy_eval::tests\`
\`cargo test --bin dominion-rust conversation::chat::room_context::tests\`

\`cargo test -p dominion-agent-sdk --test kernel_replanning\`

Architecture baseline: user-supplied 2026-11-20 Rust design review.
