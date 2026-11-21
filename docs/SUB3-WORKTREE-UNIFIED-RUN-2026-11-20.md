# DOMINION sub3 — unified run lifecycle, Git worktrees and recovery

Date: 2026-11-20. Branch: **dominion-sub3** only.

## Execution structure

- The portable Rust SDK remains the **single native action/evidence/receipt kernel**. Chat keeps its streaming protocol adapter; Room uses the existing Chat turn runner; Task Team supervises independent native SDK jobs. No duplicated, second native agent model loop.
- `GET /api/runs/:id` projects the job/execution journal onto the same versioned `RunRef`, `RunPhase`, `terminal`, `reviewRequired` structure for Chat, Room, Team, SDK and external workers. The original SSE record remains unchanged and gains `runState`.
- The stateful SQLite job store is authoritative for cancellation and recovery. A Team child cannot execute after parent cancellation and cannot forge its DAG parent/task/runtime binding. All direct/pooled Team workers share a per-process semaphore. Independent SQLite connections use the same read/shared vs write/exclusive workspace admission.

## Owner-controlled Task Team example

```json
{
  "goal": "Inspect and implement feature with isolated code changes",
  "maxParallel": 3,
  "maxReadRetries": 1,
  "tasks": [
    {"id":"inspect-local","prompt":"Analyze repository",
     "mode":"read","runtime":"native","providerId":"local-qwen",
     "model":"qwen-local"},
    {"id":"api-review","prompt":"Cross-check implementation approach",
     "mode":"read","runtime":"native","providerId":"cloud-api"},
    {"id":"implement","prompt":"Modify and verify files",
     "mode":"write","runtime":"native","isolation":"worktree",
     "dependsOn":["inspect-local","api-review"],
     "checks":[{"type":"file_contains","path":"src/feature.rs",
       "contains":"verified change"}]}
  ]
}
```

- `POST /api/task-teams` creates the Team Lead and immutably scoped DAG.
- `GET /api/task-teams/:id` exposes each worker, provenance, worktree ID, review state, and evidence grade.
- `POST /api/task-teams/:id/cancel` synchronously fences the Lead and children.
- `POST /api/task-teams/:id/retry` with `{"confirm":true}` **starts a fresh Team** only after failed/review-needed terminal status. It never copies or forges the previous workers' tool receipts and may incur new model charges.
- `maxReadRetries` defaults to 0 and is capped at 2. Auto-retry is allowed only when a native read failed **before** model route pinning, any execution record, model checkpoint or effect; it never silently retries an uncertain or paid call.

## Git worktree isolation

A Task with `"isolation":"worktree"` creates a detached, host-owned Git worktree under the persistent data directory. Its immutable record pins Team/Task identity, original repository root and baseline commit. Native tool dispatch, evidence witnesses and external CLI current directory all use the pinned worktree. The root source workspace stays unchanged until owner confirmation.

The source must be a Git repository root with no uncommitted tracked changes for a new worktree. Existing detached worktrees are verified and reused by their own original task after a restart; no cross-task adoption.

Only a **completed, host-verified isolated writer** may be promoted:
1. `GET /api/task-teams/:id/worktrees/:task/preview` returns bounded file list, base SHA and SHA-256 of an exact Git binary patch, not raw source content.
2. `POST /api/task-teams/:id/worktrees/:task/promote` with `{"confirm":true,"expectedDiffSha256":"<64-hex-from-preview>"}` requires the exact reviewed patch.
3. The host obtains an exclusive source workspace lease, validates a pristine source index and unchanged base commit, checks `git apply --check --index`, and stages the patch without committing or pushing it.

Untracked worktree files, changed source HEAD, dirty source, conflicting patch, oversized patch or unknown worktree record require owner review. There is no hidden auto-merge, auto-commit or branch rewrite. The worktree itself remains available for inspection after promotion.

## External Codex, Claude Code and Qwen Code

`DOMINION_EXTERNAL_AGENTS=1` enables independent read-only external worker jobs. Set per-task `runtime` to `codex`, `claude` or `qwen`. Optional `resumeId` explicitly selects a previously recorded Codex thread, Claude session or Qwen Code project-scoped session. Codex uses `thread/resume` with read-only sandbox and approval-policy `never`; Claude Code uses `--resume` plus plan/read-only tool flags. Qwen Code uses `--prompt`, `--output-format json`, `--approval-mode plan`, `--safe-mode`, bounded turn/time budgets and optional `--resume`; the bridge accepts only a successful final `result` event from its JSON output array. No auto-replay of a crashed external turn. External results retain the grade `external_unverified`, not native tool receipts.

`GET /api/task-teams/bridges` detects local CLI versions with `--version`, without authenticating or sending paid requests. Successful version discovery is not proof of working credentials, actual vendor protocol compatibility or sandbox security.

## Verification and limitations

The existing Activity/Operations screen includes a Task Team builder and running DAG monitor without any new navigation tab. The UI supports owner-approved replay and staged Git promotion; Playwright tests exercise native and Qwen workers with fake APIs and capture desktop/mobile screenshots.

Long-running Room sessions preserve independent cursor anchors; the added 150-turn regression verifies exactly-once delivery across reconstructed cursor snapshots. Live SSE now polls the durable journal every two seconds to surface cross-process writes without reconnect.

Product Rust CI exercises real Git worktree creation, separate files, owner-reviewed promotion, parent cancellation, one-worker recovered Team scheduling, cross-SQLite read/write leases, mixed local/API provider binding, and a mock Codex App Server JSONL subprocess. SDK CI tests kernel/replay/dispatch contracts.

**Still unresolved:** Chat's streaming model/tool loop has its own upper execution adapter even though it shares the same Rust kernel's budgets, dispatch, receipts and RunRef/RunPhase journal contracts. Native SDK Tasks use ReActLoop; a single upper model loop for all Chat/Room/Tasks remains a future refactor. Provider capability discovery is declarative and not fully live negotiated.

Not automatically proven: real Codex/Claude/Qwen account sessions, actual locally installed LLM/API billing, prolonged mixed-model load on a user machine, and operating-system-specific Tauri installer builds. These require installed runtimes/accounts or release build environments. The release workflow is intentionally not triggered by this feature-only branch update.

Source design references: 2026 Qwen Code worktree/session isolation and official Headless Mode documentation, Codex App Server thread/turn protocol, and official git-apply semantics. Existing UI/navigation remains unchanged: the new Task Teams panel is embedded under Activity.
