# Sub2 — Native harness integration, bounded parallel research and video cost controls (2026-11-15)

## Single execution engine, historical frontend baseline

This section describes the original 2026-11-15 baseline, before the 2026-11-16 video studio implementation. The current sub2 `web/` tree is deliberately different from main. The main GUI's `NativeAgentPanel.tsx` uses `/api/sdk/runs`; `src/api/sdk_bridge.rs` maps its request shape to `engine::jobs::enqueue` and the same Rust `run_prompt`/autonomy loop. It does **not** import the older main-branch `dominion_harness` crate or create a second kernel. Jobs are tagged `input.uiSource=native_sdk_panel` so this API lists only its own submissions.

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


## 2026-11-16: video creation as a turnkey first-run flow

The actual **video creation** tab in `web/src/pages/Video.tsx` now exposes installed/missing Wan 2.1 models, one-click model selection and weight download, per-file download receipts, the ComfyUI endpoint, required GGUF custom-node diagnostics, storyboarding, live output and publication. Model selection/download is **not** hidden behind a separate settings tab. Advanced FFmpeg and model-directory paths remain configurable. This change belongs only to `dominion-sub2`; main is not touched.

The real path is GUI `POST /api/video/config` (select engine and model) → `POST /api/video/models/:id/download` → `GET /api/video/models/downloads` → `GET /api/video/catalog` (filesystem verification) → `GET /api/video/status` (actual ComfyUI availability and GGUF node checks) → `POST /api/video/jobs` → Rust `engine::video::run`. Existing GUI and TypeScript SDK consume these same Rust endpoints. Download tasks are tracked in process memory; after a crash an orphaned persisted "downloading" record becomes "interrupted" so the user can retry. The model downloader rejects incomplete / tiny weight files. No model file is sent to the Git repository.

With an eligible text-model provider, the opt-in **automatic storyboard** mode sends a single topic to the shared model gateway, validates a bounded JSON scene list, checkpoints the plan and uses the existing scene-aware ComfyUI generator and FFmpeg assembly. This consumes actual configured provider tokens, not zero-cost prompt magic. Without a text-model provider, quick mode keeps single-scene generation. GPU renders remain bounded; manual storyboard and verified partial regeneration remain available.

On Windows, offline SAPI narration now complements macOS `say`. Audio is generated through OS voices when installed; unsupported systems use the documented silent fallback. The native fallback produces card-and-caption videos, not photorealistic AI motion. Generated video is never uploaded without the user triggering the upload action.

### Explicit external prerequisites

- A running ComfyUI renderer is still needed for **AI motion footage**. The download button installs weight files onto the machine hosting DOMINION, not automatically onto a separate remote renderer.
- The Q4 GGUF model additionally requires `UnetLoaderGGUF` and `CLIPLoaderGGUF` in the actual ComfyUI instance. Status checks `/object_info/{class}` rather than claiming that files alone make generation ready. The fp16 model is an alternative without GGUF custom loaders and needs substantially more memory.
- Model availability is checked against the configured `comfyModelsDir` and file sizes, not assumed from a completed HTTP request; model selection does not install the ComfyUI application itself.
- No actual GPU video-quality score, VRAM guarantee or model execution success is claimed from a frontend build. CI validates the source and regression suite; end-to-end GPU generation requires an attached ComfyUI instance with the corresponding models and nodes.
- User-visible previews from `http://127.0.0.1:4317/#/video` require building `web/dist` on the `dominion-sub2` checkout and launching its Rust engine. A remote branch commit does not modify a separately running main process's static assets.
