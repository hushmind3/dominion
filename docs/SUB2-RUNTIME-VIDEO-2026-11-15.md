# Sub2 — Native harness integration, bounded parallel research and video cost controls (2026-11-15)

## Single execution engine, unchanged main frontend

The `web/` tree is identical to the main branch. The main GUI's `NativeAgentPanel.tsx` uses `/api/sdk/runs`; `src/api/sdk_bridge.rs` maps its request shape to `engine::jobs::enqueue` and the same Rust `run_prompt`/autonomy loop. It does **not** import the older main-branch `dominion_harness` crate or create a second kernel. Jobs are tagged `input.uiSource=native_sdk_panel` so this API lists only its own submissions.

`POST /api/sdk/runs` resolves a configured provider/model and persists a normal prompt job. `GET` projects real job receipts and audited session token usage. `POST /cancel` calls the existing native job cancellation path. `POST /approve` requires a real `approval` record belonging to the job's session with a live waiter; it never creates an approval from untrusted UI data.

On `needs_review`, the user may record an observation via `POST /reconcile` with `applied` and `details`. This is an **owner attestation**, `verifiedByEngine=false`; it is not counted as an independent verified execution. A job may be resumed only when the owner has recorded that the unknown effect **did not occur**, with maximum two resumes. There is no automatic replay of uncertain tool effects.

User steering and terminal verification compare-and-swap the same `steeringGate` in SQLite, preventing an owner instruction arriving during final acceptance from silently losing the race.

## Frozen verification commands

Native Rust `Prompt.verify_command` and `POST /api/jobs` `input.verifyCommand` feed the same `engine::autonomy` check. The command must actually succeed via Rust tool evidence, not a model assertion. A pass that precedes a later successful workspace edit is stale; final results demand a new successful verification after the last edit. Command execution remains governed by the existing tool approval policy.

## Parallel read-only delegation

`delegate_parallel` in `src/engine/tools.rs` launches 2–4 delegated prompt jobs before waiting. Each child has a distinct durable session and inherits the engine's depth and tool-permission limits. Maximum eight such children per parent session; each child is bounded to two autonomous attempts. The parent receives job IDs/results and may poll unfinished children through `task_status` and `task_result`. Two layers enforce read-only operation: advertised definitions in `src/engine/chat/runtime.rs` and actual native/MCP dispatch checks. Parallel branches cannot write the shared workspace or invoke shell, Git, further delegation, or arbitrary MCP tools. Parallel code editing in independent worktrees remains a separate future capability.

## Multi-scene ComfyUI video and partial GPU reuse

`src/engine/video.rs` accepts `scenes` (maximum eight) or blank-line-separated paragraphs in `brief`, an optional `continuity` visual identity description, `reuseVideoJobId` of a previous ComfyUI project and `regenerateScenes` as zero-based scene indexes. Duration is split into explicit scene allocations, with a minimum of three seconds per scene, without silently increasing the requested total.

Each scene uses a reproducible frozen seed and a fingerprint of the exact render inputs, workflow, scene text and index. A prior clip is reused only when its canonical path remains within the prior project directory, its input fingerprint matches, its output is nonempty and its SHA-256 equals a completed manifest. The digest is computed with a fixed 64 KiB buffer. Otherwise ComfyUI generates that one scene and its manifest is written after completion.

FFmpeg concatenates clips without re-encoding when possible; a local re-encode fallback preserves generated footage if formats cannot be copied. Result receipts include `sceneCount`, `renderedScenes`, `reusedScenes` and measured `renderSeconds`. This makes GPU work reductions observable but does **not** establish any universal cost reduction or audiovisual quality benchmark.

## Test scope and unresolved limits

Added Rust tests cover job/UI adapter ownership, owner reconciliation and guarded resume, command verification ordering, revision CAS conflict, parallel read-only tool enforcement and validation, scene duration allocation, fingerprint stability and streaming hash consistency. CI and real-model E2E results must be read from GitHub Actions; the existence of tests is not a claim they all pass. The primary SDK remains Rust, with TypeScript only an auxiliary REST client. No Goals API is introduced.
