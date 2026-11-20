# DOMINION backend extension boundary — 2026-11-20

## Where the kernel ends

`crates/dominion-core/` owns contracts, durable journal admission, cancellation
and budgets. The **single mandatory agent iteration loop** is now
`crates/dominion-algorithms/src/engine.rs`; the product host wires the
model and tool phases into it. Neither crate imports MCP, messenger,
memory or skills clients.

`src/addons/integrations/extensions/` is the **application host boundary** for four
optional add-on families. These are runtime capabilities, not additional chat
executors or another engine:

| Extension | Backend owner | Data/transport |
|---|---|---|
| `mcp` | `extensions/tools.rs` → `mcp.rs` | External MCP servers and their tool schemas |
| `memory` | `extensions/knowledge.rs` | Retrieved long-term memory, learned experience, reflection and extraction |
| `skills` | `extensions/knowledge.rs` | Opt-in skill instructions and callable skill views |
| `channels` | `extensions/channels.rs` → `discord.rs`, `telegram.rs`, `channel_adapter.rs` | Inbound/outbound messenger drivers |

The host lists optional native-tool schemas and MCP servers only if permitted.
Model tool execution passes **the same extension admission check again**, after
model output, on the native gateway. Disabling a feature cannot be bypassed by
a stale discovered tool schema. The MCP server's actual tool execution and
approval remain in the existing effect/receipt journal.

Memory is **not** the durable conversation journal. Disabling `memory` leaves
original user/assistant messages, core SQLite records, model routing, executable
Rust tools and task checkpoints intact, while suppressing optional long-term
recall and automatic learning/reflection. Disabling `skills` removes the skill
index and view/proposal schemas, not normal agent prompts or owner instructions.
Disabling `mcp` avoids remote server enumeration; the rest of the built-in
Rust tool set remains usable. `channels` has its own supervisors; changing
the global policy stops external polls/outbox flushing without stopping the
agent or deleting message records.

## Policy: backwards-compatible, strictly attenuating

Existing installations default to every extension enabled. The absence of a
policy is **not** an assertion that an MCP server or messenger is connected:
each server/binding still has its own `enabled` and permission checks.
An explicit `false` at *any* of global, room, agent or session scope denies
use, even if a more specific scope contains `true`. Only known boolean keys
are accepted; policies never contain credentials or arbitrary executable code.

Global backend control (no frontend tab changes):

```http
GET /api/extensions/runtime
POST /api/extensions/runtime
Content-Type: application/json

{"mcp":false,"memory":false,"skills":false,"channels":false}
```

Session-level override, inherited by forks and preserved with the transcript:

```http
POST /api/sessions
Content-Type: application/json

{"title":"local-only agent","extensions":{"mcp":false,"memory":false}}
```

```http
POST /api/sessions/{id}/extensions
Content-Type: application/json

{"mcp":false,"skills":false}
```

Session policy changes are rejected while that session runs. This avoids a
mid-turn rebase race in the Context Engine. Global revocation is checked at
the actual tool gateway, as well as on each messenger poll. Changing policy
does not uninstall an add-on, revoke its stored credentials, erase history, or
change any frontend menu.

## Verification and architecture guardrails

```bash
cargo test --locked --bin dominion-rust engine::extensions::tests
cargo test --locked --test e2e room_identity::backend_extensions_are_isolated_from_the_core_conversation
node scripts/check-runtime-architecture.mjs
```

- Product API policy validation, inherited global/session denials,
  memory/skills/MCP tool schema suppression and core conversation continuity
  are covered by unit and real-server end-to-end tests.
- `scripts/check-runtime-architecture.mjs` rejects reintroduction of direct
  MCP discovery/calls or optional memory reads into the chat kernel paths.
- The existing Windows and macOS CI, native Codex/Anthropic/OpenAI adapters,
  original room E2E cases and unchanged frontend test contracts remain active.

## Next architectural boundary (not yet implemented)

This change adds **runtime isolation and central admission** inside the Rust
product. The optional implementation crates are **still linked into the
application binary**; unloading them as Cargo features, hot-loading arbitrary
third-party binaries, or supporting the modern MCP `2026-07-28` wire protocol
is separate work. Runtime extension opt-out must not be described as a
compile-time or process-isolation guarantee.

Reference: [MCP client best practices (2026-07-28)](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/docs/2026-07-28/develop/clients/client-best-practices.mdx)
emphasize progressive discovery and lazy server activation.


## Source ownership split (2026-11-20)

Native execution contracts remain in `crates/dominion-core`; the sole
agent loop belongs to `crates/dominion-algorithms`. The product's
`src/engine` entrypoints own model/tool adapters, approval/effect receipts,
recovery and verification.

All 43 optional top-level modules were moved **without reimplementing
their behavior** to `src/addons/` and grouped by concern:

- `knowledge/`: long-term memory, skill records, learning and reflection.
- `strategies/`: algorithm candidates, improvement and adaptation experiments.
- `integrations/`: MCP, channels, remote/model adapters, rooms and browser.
- `operations/`: backups, rotation and maintenance.

`src/engine/mod.rs` re-exports these modules to preserve existing Rust
callers temporarily; those re-exports are compatibility wiring, not code
ownership. `src/addons/mod.rs` is the single optional-implementation inventory,
enforced by the runtime architecture check. No frontend navigation, persisted
record type, wire contract, or database schema was changed in this move.

These modules are **still compiled into the application executable** and some
legacy product entrypoints still call add-ons through re-exported paths. This
is source ownership separation; Cargo feature/process isolation and eliminating
all old imports are separate milestones, not claims made by this commit.

## Chat host extraction (same commit series)

The native chat adapter remains at `src/engine/chat/runtime.rs`.
The **single mandatory loop** is `dominion-algorithms::engine::Engine::drive`.
The product-specific chat implementation is now owned by:
- `src/addons/integrations/chat_product.rs` — product host callbacks
- `src/addons/knowledge/chat_context.rs` — memory/prompt composition
- `src/addons/integrations/chat_room_context.rs` — multi-agent room context
- `src/addons/strategies/chat_program.rs` — optional execution programs
- `src/addons/strategies/chat_adaptation.rs` — adaptation experiments

Each module is linked with an explicit Rust `#[path]` from its old logical
module entrypoint. This preserves all visibility and `super::` references
without placing implementation files in the native execution directory.
The paths are enforced by the runtime architecture guard. The kernel and
SQLite/API schemas are unchanged.

## Further core slimming (2026-11-20)

Nine implementation modules were originally moved from `src/engine/` into categorized add-ons.
The superseded `goal_control.rs` shim was later deleted; current modules are:
- `src/addons/knowledge/compact.rs`
- `src/addons/strategies/obligation_repair.rs`
- `src/addons/strategies/program.rs`
- `src/addons/strategies/reasoning.rs`
- `src/addons/integrations/delegation_worktree.rs`
- `src/addons/operations/jobs.rs`
- `src/addons/operations/wake.rs`
- `src/addons/operations/job_control.rs`

The app's `src/engine/mod.rs` keeps only the compatibility exports still used by product callers; the obsolete goal-outcome collector and deprecated transport entrypoint have been removed. The
standalone `crates/dominion-algorithms` owns the only model/tool iteration loop,
while `crates/dominion-core` owns stable execution infrastructure and ports.

## Optional strategy runtime boundary (2026-11-20)

The product hook `src/addons/runtime_hooks.rs` now owns the pre-turn
strategy/experience lookup, algorithm selection and post-turn learning.
`src/engine/chat/runtime.rs` does not query improvement history directly.
The extension admission policy additionally accepts `"strategies": false`
at global, room, agent or session level. Missing values default to enabled
to preserve existing installations.

With `strategies=false`, the native turn still has model routing, built-in
file/shell tools, approvals, durable journal, cancellation and verification.
It skips strategy selection, strategy observation writes, improvement updates,
and optional adaptation retries. With `memory=false`, it additionally skips
long-term recall, turn-learning and agent-growth updates. Neither flag deletes
stored records or disables a native execution capability. This is a runtime
opt-out, not a dynamic-library unloader or a Cargo compilation feature.

## Connector tool source extraction (2026-11-20)

The native `src/engine/tools.rs` retains file/shell and capability admission,
while `src/addons/integrations/native_tools.rs` dispatches optional browser,
network, accounts, knowledge and delegation calls. Remote HTTP/GitHub code is
owned by `native_web.rs` and `native_github.rs`; prior GitHub and delegation
modules moved to integrations. Tool effects still pass through native approval,
role admission and receipt tracking. The new `connectors=false` policy option
hides and denies built-in web/browser/GitHub tools independently of external MCP.
