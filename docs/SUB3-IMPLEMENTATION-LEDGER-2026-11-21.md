# DOMINION Sub3 — implementation and verification ledger (2026-11-21)

Scope: `dominion-sub3` only. No changes to `main`, production Worker, D1 or payment credentials.

## Contracts changed

| Path | Contract | Evidence |
| --- | --- | --- |
| `crates/dominion-agent-runtime/src/drive.rs` | A paid model call persists a generation fence before dispatch; accepted decisions and rejected paid answers settle that fence only with a durable snapshot write | Native/Chat both call the shared SDK facade; atomic/failed-save unit fixtures |
| `src/conversation/chat/runtime/input_contract.rs` | User message persistence, immutable Room source ownership, acceptance-check freeze and demotion are extracted from the oversized Chat turn without changing their ordering | Existing source/acceptance logic, source contract gate |
| `src/conversation/chat/runtime.rs` | Wake/resume is blocked for the same task when an old model intent, uncertain effect, or operation journal remains | `resume_safety_tests.rs`; task-index lookup rechecked under an exclusive session lease |
| `src/conversation/chat/runtime/turn.rs` | Chat saves provider usage and model decision, rejects paid unsafe decisions before clearing the in-flight marker | `settle_rejected_chat_turn` and common SDK settlement |
| `src/features/providers/native_stream.rs` / `src/conversation/chat/protocol.rs` / `src/conversation/chat/provider.rs` | Shared SSE framing. Native accepts final-message-only Chat chunks, avoids simultaneous delta/final duplication, rejects malformed tool JSON; Chat and Native reject truncated Anthropic turn endings with the same stop-reason helper | byte-split Unicode/SSE tests, Native terminal-message fixtures, common stop-reason test |
| `src/api/sdk_runs.rs` | API keeps legacy user-facing `status` but adds canonical `phase`; no longer labels uncertain review as a denial | API unit tests for review, fail, approve/deny callId, resume |
| `web/src/components/NativeAgentPanel.tsx` | Exact approval call is revalidated server-side before an effect; status refreshes across focus/reconnect; mobile buttons have larger targets | optional real browser smoke |
| `src/conversation/chat/runtime/selection.rs` | Context-free explanations and greetings skip unneeded MCP enumeration; referential/current/source requests keep authority | classifier unit tests and a real HTTP mock Chat fixture |

## Modularization

- `src/storage/sqlite/remote_tasks.rs`: isolated remote task lease, expiry, cancellation and receipt transactions; original `Store` public methods and table layout unchanged.
- `src/features/cognition/algorithms/catalogue.rs`: static builtin algorithm declarations; original public `builtins()` and stable IDs unchanged.
- `src/storage/sqlite/tests.rs`, `src/features/cognition/algorithms/tests.rs`: existing regression modules moved unchanged to keep production sources smaller.
- Do not infer that merely moving tests proves a runtime speedup.

## Reproducible checks

From the repository root:

```sh
node scripts/check-runtime-architecture.mjs
cargo test --locked --workspace
cargo test --locked api::sdk_runs::tests
cargo test --locked api::host_security_tests
node --test sdk/typescript/tests/sdk.test.mjs
npm run typecheck --prefix web
npm run build --prefix web
cargo build --locked
npx --prefix web playwright install chromium
npm run test:sub3-browser --prefix web
```

The browser smoke is **written but has not been executed** in this authoring environment. When run, it starts the actual Rust server with an isolated temporary data directory, a temporary workspace and a locally mocked provider. It checks the final-message-only protocol, a saved model decision, exact pending approval after browser reload, a real approved workspace write and successful durable receipt, followed by refusal of a separate write and rejection of its stale approval ID. It must never connect to production credentials or services.

## Verification truth table

| Check | Current result |
| --- | --- |
| GitHub contents API: files committed on `dominion-sub3` | Confirmed |
| GitHub source inspection: 12 critical module/contract references | Verified present (not compilation) |
| Source-level architecture contract: script updated for extracted modules | Written; execution pending |
| New Rust/API/HTTP regression suites | Written; execution pending |
| TypeScript SDK, web typecheck, Vite build | Not executed in this authoring environment |
| Real Rust process, Chromium, forced restart with mock provider | Browser script written; execution pending |
| Original nine Chromium views | Not executed here |
| Real model input-token reduction versus 6 schemas / ~810 tokens | Not measured |
| Production Cloud D1, Worker or paid inference | Intentionally untouched |

The repository in the available authoring environment is accessible through its GitHub connector only; no checkout or Rust compiler exists here. Do not label tests green, claim a token reduction or assert cross-surface end-to-end parity until an environment with the full repository executes these commands.

## Remaining boundary work

- The shared **model intent / settlement / replay guard** is already usable by both hosts, but Chat and Native still own separate upper orchestration bodies. A full convergence must preserve Chat SSE, Rooms, Native tool receipts and exact user approval.
- Broaden the HTTP contract tests to crash in the interval *after* the provider processes the request and *before* the host saves the observation; assert one provider call, unknown outcome and no automatic effect replay after restart.
- Recheck per-provider stop reasons, usage-only packets and true first-token timing under the same model and same prompt, then tune high-cost context paths from measured samples.
- Run the real Chromium fixture and all existing tests before promoting this candidate.

## Fast branch-only CI and the current gate

`.github/workflows/sub3-fast.yml` is configured for push to `dominion-sub3`.
It contains four independent jobs: source architecture + TypeScript SDK,
Rust `cargo check --locked --all-targets` and kernel/Chat/MCP regressions,
frontend TypeScript/Vite build, and non-blocking Rustfmt/Clippy diagnostics. This workflow is distinct from manual full
macOS/Windows release CI and has no production deployment or secrets.
**Workflow configuration is committed; successful execution is not yet verified.**

The Chat terminal finalizer now holds a `needs_review` status for a persisted
in-flight paid model or tool action, and retains provider usage and model-call
counts from the kernel journal in both the kernel and public execution ledger.
New tests cover an owner cancellation after a paid call and a nominally
successful response while a tool action remains unacknowledged. The live browser
fixture no longer consumes a successful HTTP response body twice.
Native mobile control sizes and focus outlines were widened in the existing
panel, without adding new menus.

Cancellation-review conflict now has an explicit, narrow SQLite transaction
rule: an already-cancelled run may be finalized as `needs_review` only when
the finalizer asserts owner cancellation and BOTH the persisted and proposed
snapshots contain an unacknowledged paid model/tool fence. Completed runs and
forged/stale reviews remain terminal. SQLite regression fixtures cover valid
review upgrades, missing fences, and missing owner-cancel authority. Full
process-level timing verification remains unexecuted in this authoring
environment; do not claim that the crash race is end-to-end proven.

## MCP 2026-07-28 migration (Sub3)

Modern stateless HTTP **and** stdio now probe `server/discover` and attach
request-scoped protocol/client metadata. HTTP sends the 2026 routing headers
and schema-scoped `Mcp-Param-*` headers; both transports retain safe
2025/2024 initialization compatibility and the original Host approval,
pinned-tool manifest and durable receipt boundary. Read
[SUB3-MCP-2026-MIGRATION.md](SUB3-MCP-2026-MIGRATION.md) for the exact
compatibility modes and unsupported MRTR continuation/OAuth capabilities.

Source-level checks find the new contracts and fixtures on this branch.
The independent Node stdio MCP fixture was manually exercised locally:
2026 `server/discover`, `tools/list`, `tools/call` succeeded and a
legacy `initialize` call was rejected. **This does not prove** the Rust client
or complete integrated MCP suite passed. An actual `cargo check`, GitHub
Actions result, and public-server interoperability result have not been
obtained in this authoring environment.
