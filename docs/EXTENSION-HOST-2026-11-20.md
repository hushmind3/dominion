# DOMINION backend extension boundary — 2026-11-20

## Where the kernel ends

`crates/dominion-core/` owns the native run loop, journal, cancellation,
budget and tool sequencing. It has no MCP, messenger, memory or skills client.

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

Native execution remains in `crates/dominion-core` and the product's
`src/engine` entrypoints (natural-language input, model calls, tool dispatch,
approval/effect receipts, cancellation, recovery and verification).

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
