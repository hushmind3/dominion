# DOMINION Sub3 — implementation and verification ledger (2026-11-21)

Scope: `dominion-sub3` only. No changes to `main`, production Worker, D1 or payment credentials.

## Contracts changed

| Path | Contract | Evidence |
| --- | --- | --- |
| `crates/dominion-agent-runtime/src/drive.rs` | A paid model call persists a generation fence before dispatch; accepted decisions and rejected paid answers settle that fence only with a durable snapshot write | Native/Chat both call the shared SDK facade; atomic/failed-save unit fixtures |
| `src/conversation/chat/runtime.rs` | Wake/resume is blocked for the same task when an old model intent, uncertain effect, or operation journal remains | `resume_safety_tests.rs`; task-index lookup rechecked under an exclusive session lease |
| `src/conversation/chat/runtime/turn.rs` | Chat saves provider usage and model decision, rejects paid unsafe decisions before clearing the in-flight marker | `settle_rejected_chat_turn` and common SDK settlement |
| `src/features/providers/native_stream.rs` / `src/conversation/chat/protocol.rs` | Shared SSE framing. Native also accepts final-message-only OpenAI Chat chunks, never duplicates a simultaneous delta/final body, and rejects non-object tool JSON | byte-split Unicode/SSE tests and Native terminal-message fixtures |
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

The browser smoke runs the actual Rust server with an isolated temporary data directory, a temporary workspace and a locally mocked provider. It checks the final-message-only protocol, a saved model decision, exact pending approval after browser reload, a real approved workspace write and successful durable receipt, followed by refusal of a separate write and rejection of its stale approval ID. It must never connect to production credentials or services.

## Verification truth table

| Check | Current result |
| --- | --- |
| GitHub contents API: files committed on `dominion-sub3` | Confirmed |
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
