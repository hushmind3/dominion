# Dominion 2026 — Source-Guided Agent Harness and SDK Development

**Reviewed:** 2026-11-15. **Branch:** `dominion-sub2`. **Decision:** Rust is the execution-policy authority; React and TypeScript SDK share the existing native Axum API.

This is a code inspection and a shipped implementation slice, not a claim that Dominion has matched any other system on SWE-bench Verified or production task success. GitHub stars/forks are discovery signals, **not performance measures**.

## Source comparison: US, China and community

| Project | Relevant inspected public source | Practical insight | Dominion implementation boundary |
| --- | --- | --- | --- |
| [OpenAI Codex](https://github.com/openai/codex/blob/main/codex-rs/core/src/session/turn_input.rs) | Rust `codex-rs/core/src/session/turn_input.rs`: explicit start-or-steer admission, live turn and persistent setting lifecycle | Owner input must not race stale completion or silently overwrite a running tool | Native Rust revisioned CAS gate and next-turn handling; no Codex runtime dependency |
| [Qwen Code](https://github.com/QwenLM/qwen-code/blob/main/packages/core/src/core/client.ts) | `packages/core/src/core/client.ts`: `Steer` control type and `getSteerInput` around tool/model control; core isolated from frontends | Let the core own task state while different interfaces steer the same job | One Rust state machine exposed through GUI and TypeScript, not duplicated JavaScript scheduling |
| [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/sdk/README.md) | `packages/sdk/README.md`: complete harness runtime available to external processes via newline-delimited JSON-RPC SDK | SDK should control real jobs, not hide a distinct remote assistant behind a client wrapper | Direct `@dominion/sdk` HTTP commands map onto genuine Rust durable jobs; no DeepSeek harness embedded |
| [OpenClaw](https://github.com/openclaw/openclaw/blob/main/docs/tools/subagents.md) | `docs/tools/subagents.md`: bounded subagent tools, session isolation, visibility and cancellation | Isolation and permission inheritance must be explicit in future delegated subtasks | Existing Dominion room/task/session policies remain native; the new steering feature does not widen subagent privileges |
| [Hermes Agent](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/features/memory-providers.md) | `website/docs/user-guide/features/memory-providers.md`: memory provider plugins and session-scoped injection | Memory and context should be pluggable without lock-in to the model-provider harness | Dominion's local SQLite/memory and tools are retained; no new externally controlled memory execution path |
| [Claude Code / Claude Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview) | Public SDK contract: tools, hooks, permissions, sessions | End-user controls must be available in the application, not just in developer SDK callbacks | Existing Home NativeAgentPanel exposes task creation, permissions, stop and resume through the Rust bridge |

At review time, public GitHub repositories reported approximately **135k** Codex stars, **30k** Qwen Code, **281k** DeepSeek Harness, **394k** OpenClaw and **263k** Hermes Agent (2026-11-15). These are not standardized downloads or adoption measurements and cannot establish which harness is smartest.

### Architecture tradeoffs

- External agent SDK embedding gives quick feature access but lets external policies own the plan/tool loop; Dominion keeps the loop, permission checks, recorded tool effects and model routing in Rust.
- Streaming input directly into a running tool risks duplicated external side effects; the owner revision is applied **after** the current model-turn boundary and checked again before final job completion.
- Adding an owner constraint is different from replacing a task. Both now have explicit semantics. A replacement invalidates obsolete planning/verifier conditions while preserving historical audit and no-replay information.
- The **unchanged main frontend** is the UI contract. Home's NativeAgentPanel launches native work; Activity shows existing job history. Per-job workspace/token and append/replace steering remain SDK/API-only until the main UX chooses to expose them.

## Additional implemented slice: per-project native jobs and bounded provider calls

**Goal:** Allow one Dominion installation to work on many repositories without accidentally applying a code-change request to the server's startup directory.

| Entry point | Real implementation | Runtime consequence |
| --- | --- | --- |
| Rust `POST /api/jobs` | `src/api/jobs.rs` validates `workspaceDir` and `tokenBudget` | Persisted user selection, not an unused UI setting |
| Native Rust SDK | `src/sdk.rs` adds `submit_in_workspace()` and `submit_with_options()`, preserving the existing `Prompt` struct | Same job queue without embedding another agent SDK |
| Workspace authority | `src/engine/workspace_scope.rs` canonicalizes an existing absolute root and rejects the internal data folder or filesystem root | Selected project is a **per-job config clone**; database, tool approvals, cancellation, events and write locks are shared |
| Job worker | `src/engine/jobs.rs::run_prompt` applies the selected root both to `run_quiet` and the Rust `autonomy::execute` verification cycle | Search, workspace-relative edits and tests in the selected repository |
| Subagent delegation | `src/engine/tools.rs::delegate` persists the parent's selected project in the child job/session | Both `delegate_task` and `delegate_parallel` retain project scope even if the global worker claims children after restart |
| Provider budget | `src/engine/chat/runtime.rs` reads the job-owned token limit at the real model-call boundary | Reserves the next input estimate and bounds requested output; avoids launching a subsequent model call without a remaining allowance |
| User budget | `src/engine/autonomy.rs` accumulates attempt usage and returns review on budget exhaustion | The frontend receives the real token-usage checkpoint and does not hide budget failures |
| Main React UI & TypeScript | `web/src/components/NativeAgentPanel.tsx`, `src/api/sdk_bridge.rs`, `sdk/typescript/src/index.ts` | Main UI uses its existing Home task controls; SDK exposes additional job workspace and token inputs without inventing frontend fields |

**Boundaries:** This is project-relative path routing, **not** a host-level OS security sandbox; existing approved shell tools and absolute-path tools retain their separate permission and runtime behavior. The per-job provider admission budget is **not** a strict aggregate cap across independently delegated subagent jobs; a separate atomic shared budget/reservation ledger is necessary for that future feature. Usage estimates and provider bills may differ. No model-dependent benchmarks or CI pass are claimed for this slice.

## Main-frontend execution bridge: live Rust behavior

The existing `web/src/components/NativeAgentPanel.tsx` receives one persisted `/api/sdk/runs` view. Backend enhancements follow these **already visible** controls:

- **Start:** the Home panel's instructions and model resolve into a real Rust prompt job, not a second SDK runtime.
- **Approve:** only approvals with active in-memory Rust waiters are shown. Orphaned pending DB records left by a restart no longer produce a button the user cannot successfully click.
- **Live progress:** running execution iterations and persisted tool-operation starts populate the existing rounds, toolCalls and recent events fields before an entire autonomous attempt finishes. The UI does not need new widgets.
- **Stop / manual confirmation / resume:** no recorded tool effect permits a model-only retry; possible external effects require the existing owner's not-applied observation. Rust stores that evidence in `resumeEvidence`, retains prior receipts in `resumeHistory` and starts a new grounded continuation message without blindly replaying the original goal. A corrected goal retains the observation.
- **Limit reached:** a token-budget review maps to the main panel's existing `exhausted` status rather than an unrelated unknown-external-effect prompt.

No new React controls were added. Existing workspace and token options are available through native Rust and TypeScript SDK APIs, not falsely claimed as part of main GUI.

## Implemented end-to-end contract

1. `web/src/components/NativeAgentPanel.tsx` (main frontend, unchanged): starts `POST /api/sdk/runs`; `src/api/sdk_bridge.rs` maps its requests to real native Rust prompt jobs, approvals, cancellation, reconciliation and guarded resume.
2. `src/engine/jobs.rs`: `steer_with_mode` stores append/replace revisions under existing `steeringGate` optimistic concurrency and rejects late/completed updates.
3. `src/api/jobs.rs`: stable `POST /api/jobs/{id}/steer`, including backwards-compatible append mode.
4. `src/engine/autonomy.rs`: reconstructs authoritative goal, replans, replaces obsolete acceptance checks and scopes evidence by replacement epoch; preserves historical side-effects and receipts.
5. `web/src/pages/Operations.tsx` (main frontend, unchanged) shows existing jobs and execution records. Owner steering is implemented in SDK/API, not an unshipped React `JobSteering` component.
6. `sdk/typescript/src/index.ts`: `jobs.steer()` and `jobs.replaceGoal()` use identical native API, with request contract coverage in `sdk/typescript/tests/sdk.test.mjs`.
7. `web/src/i18n/ko.ts` and `en.ts`: bilingual user-facing labels and help text.

### Behavioral guarantees and limitations

- Append preserves the initial user goal; replace supersedes it and earlier appended instructions; subsequent additions remain in force. The immutable original prompt is preserved as the audit input.
- A concurrent owner steering revision prevents a stale completion from closing the job. There is no mid-tool interruption or automatic replay of uncertain side effects.
- Existing verification command and tool permission policies remain intact. Replaced goals do **not** reuse old tool evidence as proof of completion.
- This vertical slice is **not** a full frontier platform certification, completed SWE-bench Verified run, production deployment, or real paid multi-generation intelligence gain.

### Cleanup and validation scope

Changes stay within the existing Rust prompt worker, API bridge and TypeScript client; main frontend bytes are kept identical, with no duplicate React task panels, competing vendor runtime, new database or obsolete endpoint. Tests added cover CAS, goal replacement precedence and exact SDK HTTP payload. One pre-existing RSI meta-test expected a failed baseline guard to invalidate the entire test; its assertion was corrected to test **candidate** guard regression (baseline failures already legitimately count as measured zero). A prior Windows CI run also exposed filesystem sharing violations across multiple test fixture cleanup paths; that separate cross-platform fixture problem must not be described as a successful run.

Only after source cleanup should CI/typecheck results be used to verify the integration. A frontier-level benchmark comparison belongs after core runtime capability is functionally complete.
