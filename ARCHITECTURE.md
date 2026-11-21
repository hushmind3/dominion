# DOMINION — canonical Rust agent architecture (sub3, 2026-11-21)

> **Scope:** `dominion-sub3` only. This is the authoritative sub3 ownership and integration contract for the local-first Rust/React/Tauri product, not a claim of parity with a frontier model or a signed release. Design basis: the project's 2026-11-20 architecture review, current sub3 source, and the cited 2026 implementations. Other branches are not modified.

## 1. Architecture decision

**One product Host, one portable action/evidence kernel, one canonical run lifecycle, independently selectable strategies.** Do not collapse Core and Algorithms into one crate, create a second "harness" process, or create another loop to support group chat.

```text
React/Vite UI ── Tauri 2 desktop ─────── TypeScript/Rust SDK clients
     │                  │                           │
     └──────────────────┴────── HTTP/SSE ────────────┘
                              │
                     src/api (Axum)
                              │
             Host AppState / ownership / approvals
                src/host/ + src/storage/
                              │
    ┌─────────────────────────┼────────────────────────┐
    │                         │                        │
Interactive Chat/Room     Native SDK jobs          Task Team lead
src/conversation/        src/host/execution/     collaboration/task_team.rs
streaming adapter        native_agent.rs         durable DAG supervisor
    │                         │                        │
    │             ┌───────────┴──────────────┐         │
    │             │                          │         │
    └─ SDK tool   │  ReActLoop (Algorithms)   │    child native jobs /
       admission ─┤  AgentLoop + AgentPorts    │    external read bridges
                  │  (Runtime contracts)      │
                  └───────────┬──────────────┘
                              │
          model/provider + tools/evidence/journal ports
                              │
       src/features/providers/    src/integrations/
       local inference / API      MCP, Skills, files, browser,
       bounded capabilities       channels, external AgentBridge
                              │
                   SQLite + event_log + vault
                   durable source of truth
```

**Important code/runtime distinction (2026-11-21):** Native SDK prompts and Team-native workers use `AgentLoop::execute`. Chat and Room reuse existing streaming Chat adapter with shared SDK tool-admission / receipt barriers, but still have their **own upper model-turn loop** in `src/conversation/chat/runtime/turn.rs`. The control-plane and lifecycle contract is unified; the upper Chat/SDK loop is **not yet unified**. No rewrite has been silently represented as finished.

## 2. Source ownership and one-way dependencies

| Owner | Real implementation | What it owns | Forbidden dependencies |
| --- | --- | --- | --- |
| Portable Runtime/Core | `crates/dominion-agent-runtime/` | versioned RunRef/RunPhase/RunTransition, AgentLoop and ports, limits, action admission, schema, receipts, read/write scheduling | AppState, SQLite, HTTP API, prompt policy, model SDK |
| Algorithms/Harness | `crates/dominion-agent-algorithms/` | `ReActLoop`, plan → model → tools → evidence, contextual selection, failure guidance, policy injection | product DB and UI, direct owner credentials |
| Stable Rust kernel facade | `crates/dominion-agent-sdk/` | public compatibility re-exports; zero independent loop | parallel workflow and host storage |
| Product Host | `src/host/execution/`, `src/host/journal.rs`, `src/host/state.rs` | durable job ownership, approval, cancellation, workspace leases, recovery, effect evidence, loop/strategy binding | model-specific reasoning in storage |
| Conversation adapters | `src/conversation/chat/` | streaming protocol, room history/compiler, existing Chat turn compatibility | bypass of approved tool dispatch, independent write authority |
| Strategies and collaboration | `src/features/cognition/`, `src/features/collaboration/` | policy profiles, Room cursors, shared workboard, Team DAG and worktree | claiming model text as verified effects |
| Provider gateway | `src/features/providers/` | local/API route selection, provider capability, authenticated model calls, usage and cache evidence | granting filesystem/network effects |
| Integrations | `src/integrations/` | MCP, Skills, browser, messaging, local tools, account adapters, external CLI agents | taking over execution state machine |
| Storage | `src/storage/` | SQLite, migrations, vault, event sequence | model policy |
| Clients | `web/`, `src-tauri/`, `sdk/typescript/`, `sdk/rust/` | request/stream/render/cancel/review | a second autonomous executor |

Dependency arrow for crates: `dominion-agent-algorithms → dominion-agent-runtime`; `dominion-agent-sdk → {algorithms, runtime}`. The host binds the ports; neither portable crate imports Host.

## Original 2026-11-20 architecture: Room event journal and shared dispatch admission

The owner-supplied architecture drawing specifies a **single Local-first Rust
Modular Monolith** with the Room/Task Orchestrator, Context Engine, optional
Strategies, one Agent Harness, Execution Core and separate Model/Tool Gateways.

The Room log is now an indexed SQLite `room_events` table with a trigger
bound to the actual message INSERT, and idempotent backfill of prior Room
transcripts. `RoomEvent` identifiers carry the per-room sequence, speaker,
reply anchor and originating run; `RoomAgentBinding` exposes independent
`readCursor` state. `GET /api/rooms/:id/events` and both client SDKs
provide cursor pagination without creating another UI menu or service.

The portable Runtime `dispatch_fence` now checks cancellation or owner
correction **after writing the model-call intent and before dispatch**. Both
the native Algorithm `ReActLoop` and Chat streaming adapter call this exact
shared boundary. Tool dispatch already uses the same SDK action scheduler
and Host approval/receipt contracts. The upper model-decision loops still
need streaming-safe structural consolidation before full physical P0 loop
singletons can be claimed.

ModelGateway also now selects the exact model-specific context limit rather
than inheriting a larger provider default; native tool compatibility remains
checked before generation.

## 3. Canonical run identity and lifecycle (implemented)

- `RunRef { version: 1, surface: chat | room | team | sdk | external_agent, runId, parentRunId }` is defined in `crates/dominion-agent-runtime/src/run_contract.rs`.
- `RunPhase` is the **only portable state vocabulary**. The `RunTransition` reducer now lives alongside it. Host `src/host/execution/run_state.rs` delegates transition checks to Runtime.
- Terminal states: `succeeded`, `failed`, `cancelled`, `interrupted`, `needs_review`. Transitioning out of a terminal phase is forbidden, including when an older execution record has a stale `phase` but a terminal durable `status`.
- `GET /api/runs/:id` is a read-only projection from existing job/execution records via `run_view.rs`; it is **not** a new store or a parallel scheduler.
- Both `sdk/rust` (`Dominion::get_run`) and `sdk/typescript` (`Dominion.runs.get`) consume the same versioned run projection.
- `src/events.rs` emits a durable event, enriched with `runRef` and `runState`; `GET /api/events?after=N` replays persisted sequence numbers. A client must deduplicate by global `seq`, not by number of WebSocket packets.
- Never infer success from SSE disconnection. The client re-queries the durable run. Unknown outcomes map to owner review rather than silent retry.

**Ownership chain:** owner input → persisted run/request → pinned algorithm/provider/workspace → authorized model/tool intent → execution witness → durable receipt/check → terminal outcome. `runId` denotes execution, `parentRunId` denotes delegation; a model cannot mint its own parent run.

## 4. Interactive Chat, Room Chat and Task Teams are separate orchestrations

### Interactive Chat and Room

The streaming turn remains in `src/conversation/chat/runtime/turn.rs`; its tool calls pass through `ChatKernelDispatcher` / `ChatKernelJournal` in `src/conversation/chat/kernel_bridge.rs`. It must preserve token streaming, session continuity, owner approvals, tool receipts, cancellation and existing HTTP APIs during upper-loop migration.

The Room runs the same Chat turn adapter, **not** an additional Room-specific model loop. `room_cursor.rs` records each agent's independent committed unread cursor. For a new participant the baseline is the current **owner trigger**, not the last peer message visible during compilation. Subsequent unread delivery is bounded, contiguous and checkpointed atomically with a final answer in `src/storage/sqlite.rs`. An unreceived peer message may not be marked read. Long history remains indexed/retrievable; every agent need not receive the whole room transcript.

The `workboard.rs` handoff supplies only scoped host operations, with evidence grade and artifact pointers. Newer empty replies cannot erase the last receipt-backed contribution, and an operation explicitly bound to a different agent is excluded. An agent's *claim* is never a verified mutation.

### Task Team

`task_team.rs` is a **durable task DAG supervisor**, not an independent ReAct loop. Lead/task identity and dependencies are stored before dispatch. Read-only native workers may run in parallel; effects use durable workspace leases. A child is only accepted with the expected parent/task/runtime binding. Native writes require effect receipts, acceptance checks and evidence witnesses. External Codex/Claude/Qwen workers are independent, read-only bridge processes with `external_unverified` grade and must not forge SDK receipts.

Each selected writer can use `worktree.rs` to pin its source repository/base commit and run in a detached Git worktree. Promotion requires a fresh preview, a SHA-256 digest supplied by the owner, exclusive workspace admission and exact patch revalidation. Promotion stages changes; it never commits or pushes. Owner retry creates a fresh Team/child IDs and cannot silently replay uncertain past effects.

**SDK parity:** both the Rust and TypeScript SDKs expose Team creation, status, cancellation, owner retry, bridge preflight, worktree preview and digest-approved promotion. Neither SDK implements another task runner. No extra UI menu is added.

## 5. Model gateway, API/local inference and external agents

- `src/features/providers/gateway.rs` resolves route/capabilities and `native_routing.rs` pins provider and model identity. An API or local provider participates as a **Model port** inside the trusted harness.
- An external Codex/Claude/Qwen **agent** has its own session, tools and permissions. `src/integrations/agent_bridge.rs` is an AgentBridge: its output is a scoped observation, **not** an internal model/tool receipt.
- A model endpoint's advertised tool support, streaming and caches are capabilities of its exact endpoint/configuration, not a guarantee inferred from the model's marketing name.
- Host checks model/tool compatibility before dispatch. A route swap cannot silently change a running native job's pinned identity.
- Actual provider usage is distinct from inferred token/context estimates. Unsupported cache modes and unreported usage are represented as unknown; do not book optimistic savings or vendor-billed cost.

## 6. Persistence, authority and recovery invariants

1. SQLite job/execution/approval/event journals are the **authoritative** durable state. React cache, SSE packets, model context and in-memory worker maps are projections.
2. Every effect passes the host's tool-authorization and approval boundary, stays within a pinned workspace/route and obtains an appropriate receipt. Unknown tool names are treated as effectful, not parallel-safe reads.
3. Write-ahead intent is durable before a paid model call or external effect. Uncertain external mutation after interruption is a manual-review barrier. Never replay it merely because a supervisor restarted.
4. Cancellation fences parent and children; a closed/stale execution cannot be revived by a duplicate event or by a concurrent status write.
5. Algorithm strategy ID and revision are pinned in each native job/checkpoint; `src/features/cognition/profiles.rs` selects from the allowlist, not from arbitrary model output.
6. Optional memories, MCP, channels and Skills are **integration/strategy inputs**, not owners of the core loop. User decisions and old source evidence remain recoverable.
7. No automatic source adoption, self-modifying core generation, standalone RSI campaign, or embedded media render/upload pipeline is part of the active architecture.

## 7. Client and deployment topology

`web/` is the React/Vite client. The Rust Axum API serves Chat, job/SDK/Team and event endpoints. `src-tauri/` owns desktop process lifecycle and permission capabilities; local service traffic should be scoped to loopback with an authorized client. A future cloud gateway is a deployment/integration boundary and **not** a second authority for local file effects. Users can run local model inference without a cloud account and can select authorized remote APIs independently.

Preserve existing navigation and API surfaces. Operations/Task Teams are presented inside the existing UI, with no additional top-level tab. The app should show source task, worker ownership, phase, owner approvals, verification outcome and recovery action from durable state.

## Implementation map against the attached original (2026-11-20)

| Original requirement | Sub3 implementation | Status |
| --- | --- | --- |
| Core/Harness/Strategies ownership | Runtime contracts + Algorithm ReActLoop + pinned host profiles; the SDK remains a facade | Structural split implemented |
| One Run/Event identity across surfaces | `RunRef`, `RunPhase`, `RunTransition`, `/api/runs/:id`, durable SSE | Implemented |
| One admission and receipt semantics | Shared `dispatch_fence` and `dispatch_batch`, SQLite evidence journal, CAS status/phase | Implemented in both paths |
| One *physical upper model-turn loop* | Chat/Room still own legacy streaming loop, native job owns ReActLoop | **Not yet migrated** |
| Room Chat / independent agent cursors | Per-room SQLite `room_events.seq`, atomic agent `throughSeq`, 150-turn coverage | Implemented |
| Separate Task Team graph | Native/CLI workers, dependency DAG, read parallelism, writer lease, worktree owner promotion | Implemented |
| Local/API Model Gateway | Per-model context/tool capability and pinned provider identity | Implemented |
| External Codex/Claude/Qwen AgentBridge | Independent scoped read-only external sessions, host observations not native receipts | Implemented |
| Long-term correction / strategy evidence | Source-grounded memory and bounded strategy-evaluation modules | Existing implementation |
| React/Tauri/SQLite durable source | Existing UI and REST/SSE contracts, local-first Host and Tauri sidecar | Existing implementation |
| Distributed remote Gateway/Worker | Original architecture makes this conditional on multi-device demand | Out of scope |

The original document says not to rename/recreate crates merely for labels. The
current names `dominion-agent-runtime`, `dominion-agent-algorithms` and
`dominion-agent-sdk` preserve that two-layer Core/Harness separation with
a compatibility facade and host-controlled strategy profiles.

**Operational acceptance** requires the latest exact-commit CI results; no
real model quality, token saving, signed desktop package or full physical
Chat/native upper-loop convergence is claimed from these source edits.

## 8. Mandatory regression acceptance

| Boundary | Deterministic evidence required |
| --- | --- |
| Portable lifecycle | run transitions; no terminal revival; `needs_review` is terminal |
| Native Kernel | same model/allowed tools/budget across retry; receipts are not replayed; verification before acceptance |
| Chat streaming | cancellation/approval/session restoration, identical user-visible message contract |
| Room | 150-turn independent cursor replay, no early peer acknowledgment, no duplicate delivery; cross-room separation |
| Team | DAG cycles rejected; parent cancellation fences children; reader concurrency; writer exclusive lease |
| External AgentBridge | missing CLI or account fails cleanly; no external read result upgrades to a native receipt |
| Worktree | pinned baseline, digest preview, explicit owner promotion, nonmatching digest fails |
| SDK | Rust and TypeScript API parity for `GET /api/runs/:id` and Task Team owner operations |
| Events | same durable sequence across processes, SSE replay gaps and duplicates handled |
| Release | focused Rust and TypeScript contract tests first; OS-specific signed installers and real-model paid tests are separate gates |

Fast checks on this branch: `cargo test -p dominion-agent-runtime --lib`; `cargo test --bin dominion-rust host::execution::run_state::tests`; `cargo test --bin dominion-rust host::execution::run_view::tests`; `cargo test --bin dominion-rust features::collaboration::room_cursor::tests`; `cargo test --bin dominion-rust features::collaboration::task_team::tests`; `cargo test -p dominion-sdk --lib`; and `npm test --prefix sdk/typescript`.

## 9. Migration boundary still required before claiming one physical model loop

**Current implementation is a coherent modular monolith, but not yet a single physical upper-loop engine for Chat and SDK tasks.** A safe migration must be based on existing `AgentPorts` and journaled `RunRef` contracts:

1. Extract Chat's provider streaming as a `Model` streaming port without changing tool/result wire semantics or duplicating tool dispatch.
2. Project the Chat/Room context compiler into the common Algorithm context input; preserve persona, owner trigger, read cursor and provider-dependent prompt-cache identity.
3. Execute the same versioned `AgentLoop` under native and Chat adapters. Keep separate Room/Team scheduling policies while sharing the turn execution machine.
4. Shadow-compare fixed transcripts, tool receipts, approval/steering and restart outcomes before routing all Chat requests through the portable loop; do not silently upgrade old checkpoints.
5. Remove the old upper loop **only after** exact streaming/cancel/cost regression parity, then update this document and retire compatibility aliases.

Do not describe this as implemented until the source path actually calls the common `AgentLoop` from every surface. Currently Team-native workers do, Chat/Room upper turns do not. This limitation is deliberate visibility, not another parallel feature design.

## 10. 2026 external source comparison

- [OpenAI Codex App Server](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md): independent sessions/turns and JSON-RPC notifications. DOMINION preserves an independent AgentBridge boundary for this runtime.
- [Qwen Code session multi-agent design, 2026-10-05](https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-10-05-session-multi-agent.md): agent session ownership, ordered participant message delivery, concurrency and remote runtime bridging. This motivates durable independent Room cursors.
- [Qwen Code managed Agent Teams, 2026-10-10](https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-10-10-managed-agent-teams.md): distinct Lead, tasks, dependencies and workers. DOMINION retains a separate Team orchestration layer above its native harness.
- [AgentScope Service, 2026-11-12](https://www.alibabacloud.com/blog/agentscope-service-open-source-agent-control-plane-with-deepseek-harness-support_603768): session/Team control plane, shared workspaces and cross-replica task state. DOMINION remains a local-first modular monolith until multi-host operation is needed.

These are architectural references, not claims that the upstream code is vendored or that measured equivalent performance has been obtained.
