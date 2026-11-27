# SUB3 — full source and UX audit (2026-11-21)

Branch: branch3 only. Source: GitHub recursive tree, exact Rust/UI modules, workflow definitions, 54 Chromium screenshots and cross-branch blob comparisons. **Tree inventory is broader than the deep-reviewed call paths; this is not a statement that all 439 blobs received line-by-line manual review.**

## Inventory and comparison

- 520 tree entries and 439 blobs (not truncated); Rust host, three dominion-agent-* crates, Rust/TypeScript SDK, React/Vite UI, Tauri, Cloudflare Worker, benchmarks, tests and workflows.
- Compared with main, 68 same-path files differ by blob SHA, including 22 React/frontend sources. Sub2/Sub4 have different architectural and UI trees; naming or README alone cannot establish parity.
- Screenshots: 1440, 768, 390, 320px actual Chromium viewports. A contact sheet of 16 representative pages was visually inspected.

## Input → durable output trace

| Surface | Concrete implementation |
| --- | --- |
| Direct Chat | web/src/pages/Chat.tsx → src/api/chat.rs → src/conversation/chat/runtime/turn.rs → model/tool gateway, kernel_bridge.rs → SQLite messages, execution operations, SSE |
| Autonomous Agent | NativeAgentPanel.tsx → /api/sdk/runs → sdk_runs.rs → host/execution/jobs.rs → native_agent.rs → crates/dominion-agent-algorithms/src/agent_loop.rs → persisted decision/action receipts |
| Rooms | Rooms.tsx → /api/rooms → features/collaboration/rooms.rs → room job and chat runtime → room events, participant cursor and scoped evidence |
| Task Team | TaskTeams.tsx → /api/task-teams → features/collaboration/task_team.rs DAG supervisor → genuine child jobs → verified outcomes and optional Git worktree preview/stage |
| CLI Agents | external_agent child → integrations/registry.rs → integrations/agent_bridge.rs → Codex, Claude or Qwen guarded read-only CLI |
| Remote devices | external_agent with remote runtime → integrations/registry.rs → host/execution/remote_worker.rs → independent worker gateway, SQLite remote_task lease → externally unverified text output |
| Cloud | DominionCloudAccount.tsx + cloud/src/index.js and cloud/schema.sql → isolated, **not deployed**, Cloudflare staging |

## Verified strengths

- Native SDK runtime, model/tool strategy and host adapters are separated. Native model response + usage settlement is fenced against paid replays while model decisions are uncertain.
- Team DAG validation enforces workspace write leases, artifact checks, immutable child binding, evidence classes and explicit owner Git change promotion.
- Remote worker protocol is opt-in, text-only, authenticated, leased and abortable. It cannot dispatch host write tools.
- Room cursor/evidence persistence, native crash recovery, limits and tool receipts have focused regression coverage.
- UI Chromium checks operate actual React/Vite screens, use real clicks/inputs and test 9 destinations and responsive sizes; backend API replies are fixtures, not real provider credentials.

## Found and repaired in this audit

### Local owner API DNS-rebinding read surface
Previously src/api/mod.rs guarded mutation Origin, but a supplied non-local HTTP Host on GET was not checked. Such an address can expose local owner data through a browser DNS-rebinding scenario despite same-origin/CORS policies. Added guard_local_host to reject non-loopback Host/authority on /api/*, with exact 127.0.0.1/localhost port binding. The separate remote worker router is unchanged. A real local HTTP request to /api/sessions with a foreign Host must now return 403 without private session details; test name: api::host_security_tests. The product workflow now also runs api::sdk_runs::tests (missed by previous green CI filters).

### Owner cannot refuse a single native tool
NativeAgentPanel showed a button to allow one pending tool but none to refuse it. Existing generic approval gateway supported denial. Added run-scoped /api/sdk/runs/:id/deny, the same live session + callId binding as approve, a UI refusal button, a Rust regression that delivers false to the actual tool waiter and prevents subsequent approval, and a Chromium UI exercise of the refusal request, and a public TypeScript SDK nativeRuns.deny method with exact call-ID transport tests.

## Unfinished and unmeasured

1. **Single Harness convergence**: Direct Chat still owns a model/tool round loop in src/conversation/chat/runtime/turn.rs (~1792 lines). Native autonomous jobs run a separate loop in crates/dominion-agent-algorithms/src/agent_loop.rs (~841 lines). The shared kernel snapshot does not make these one execution loop; the Nov 20 architecture target is only partially satisfied.
2. **Live browser-to-Rust E2E**: Chromium intercepts all /api routes using mocks. Rust has separate real HTTP tests (including native jobs, rooms and leases), but no automated browser → live Rust server → real native effect → durable UI receipt walkthrough. External OAuth, provider billing, desktop WebView2 and Safari mobile were not verified.
3. **Cloud not provisioned**: cloud/wrangler.toml deliberately uses REPLACE_WITH_SUB3_D1_DATABASE_ID and cloud CI has no deploy job. Paid streaming/tool calls are disabled by current gateway contract. This is a staging source implementation, not an active production account/payment system.
4. **Mobile UX**: Visual no-overflow checks pass, but 26–32px Chat header controls, long multi-provider state, touch-target quality, screen-reader navigation and mobile keyboard behaviour need deeper nonempty/live-case testing.
5. **Performance**: No matched-task, matched-model, matched-budget agent success benchmark against 2026 Qwen Code, Claude Code or OpenHands; no proven provider-billed cache savings or 27B sustained task-completion result.
6. **Maintenance complexity**: Very large src/storage/sqlite.rs (~2907 lines), src/features/cognition/algorithms.rs (~2186), Chat turn and web/src/pages/Extensions.tsx (~1399) increase change risk. Standalone RSI campaign is retired; reflect.rs retains owner-grounded correction and optional strategy proposal records, not an independent automatic core rewrite.
7. **CI coverage**: Workflow uses selected dev-profile Rust filters, TypeScript tests and Chromium fixtures. It is not equivalent to running every Rust test, genuine paid provider calls or desktop installers.

## 2026 upstream reference points

- Qwen Code headless and session policies: https://qwenlm.github.io/qwen-code-docs/en/users/features/headless/
- Qwen Code agent and registered worktrees: https://qwenlm.github.io/qwen-code-docs/en/developers/tools/task/
- Claude Agent SDK hook/tool permissions: https://code.claude.com/docs/en/agent-sdk/permissions
- OpenHands Agent Canvas/Server separation: https://github.com/OpenHands/OpenHands/blob/main/docs/architecture.md

These references show comparable design concerns, **not** equivalence of DOMINION performance or completeness.

## Verification rules

The user-facing source of truth is the actual GitHub Actions run on the latest code commit. Browser mocks and unit tests must not be described as real third-party integration or end-to-end paid-model success. No release build or deployment was performed in this audit.
