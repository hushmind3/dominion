# Sub2 — Scoped GPU cancellation and durable video checkpoints (2026-11-16)

## End-to-end path

This change is **only on `dominion-sub2`**, not main. The video studio keeps its four existing tabs and creates video through the same Rust engine:

`web/src/pages/Video.tsx` → `POST /api/video/jobs` → `src/api/video.rs` → `src/engine/video.rs` → ComfyUI / FFmpeg → durable `job` and `videoState` → `GET /api/video/jobs/:id` → GUI/TypeScript SDK.

The new **Stop video creation** control (in the active preview and the project library) calls `POST /api/video/jobs/:id/cancel`. `@dominion/sdk` exposes the identical endpoint as `video.cancel(id)`. The API first cancels the selected native job through the existing Rust `jobs::cancel` path, then, only for a ComfyUI prompt whose UUID was recorded as in-flight, sends `POST {comfyUrl}/api/jobs/{prompt_id}/cancel`. It does **not** call the global `/interrupt` endpoint, which could stop a different user's job on a shared renderer.

A request accepted by ComfyUI is recorded as `remoteCancel.status=requested`, **not** as proof that the GPU has already stopped. An unavailable/legacy server or an absent prompt ID produces `unconfirmed` with a reason. The durable job retains this receipt, exposes it in `public_job`, and audits the cancellation. Queued/FFmpeg jobs do not require an external cancellation request. Invalid/terminal local jobs fail with 404/409 rather than modifying another job.

The submission-response race is handled by the Rust worker: if the owner cancels while `POST /prompt` is still returning, the worker sends the *new* UUID to the prompt-scoped cancellation endpoint immediately after receiving it. The late remote receipt is persisted under the already-cancelled local job, when possible, without resurrecting the execution. Older ComfyUI servers without the prompt-specific endpoint cannot be safely interrupted using a global operation; the UI tells the user to inspect the remote queue.

## Critical checkpoint correction

`src/engine/jobs.rs::JobContext::checkpoint` performs a **videoState-only shallow object merge** before the existing cancellation CAS. Previously `videoState:{inFlight:true}` replaced `videoState:{scenes,sceneTotal,sceneIndex,...}`, erasing the storyboard and counters in subsequent scene submissions. Only keys supplied by the new checkpoint are updated; existing storyboard, scene count, progress and prompt provenance survive.

Each new ComfyUI submission explicitly clears the previous `promptId` to JSON null until a fresh UUID is received. This prevents an owner cancellation during scene N+1 submission from targeting the completed scene N by mistake. The job retains previously recorded clip SHA-256 manifests, so a subsequent project can still reuse verified completed clips.

## External source inspection

- ComfyUI upstream `server.py` (2026-11-16 inspection): `POST /api/jobs/{job_id}/cancel` classifies running/queued/terminal jobs, interrupts only the specified active job via `interrupt_if_running(job_id)`, and dequeues a matching pending job. It returns `{"cancelled":true|false}`. `/api/interrupt` is deprecated and not job-scoped. https://github.com/Comfy-Org/ComfyUI/blob/master/server.py
- ComfyUI OpenAPI source: https://github.com/Comfy-Org/ComfyUI/blob/master/openapi.yaml
- OpenHands SDK cancellation/recovery approach: https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/conversation/impl/local_conversation.py — agent interruption also requires persisting a consistent resulting state instead of assuming an in-flight action completed.
- Compared source on `main`, `dominion-sub3` and `dominion-sub2`; this is an isolated sub2 change. `dominion-sub4` was not accessible for this specific file during inspection. A simple source-text comparison is not a performance benchmark.

## Regression contracts

- Rust `src/engine/video.rs`: invalid remote IDs/schemes rejected before network requests; a local fake HTTP server accepts only the prompt-scoped cancel path; cancellation during the post-submission checkpoint attempts remote cancellation and audits the result.
- Rust `src/engine/jobs.rs`: consecutive video checkpoints preserve the frozen scenes, counters and explicit prompt ID nulling without changing unrelated fields.
- Rust `src/api/video.rs`: cancellation of a queued video changes only that job and creates a `not_active` remote receipt.
- TypeScript `sdk/typescript/tests/sdk.test.mjs`: `video.cancel()` calls the real scoped route and distinguishes remote uncertainty.
- GUI: cancellation is visible in video preview/library; remote uncertainty is surfaced explicitly. Existing playback, download, editing, partial reuse and publication controls remain.
- Validation status should be read from the exact commit's GitHub Actions job matrix; do not infer a pass from the presence of tests.

## Measured vs not measured

**Source-backed:** the route, receipt, checkpoint merge, worker race handling, SDK contract and UI control are connected to the existing native Rust jobs. **CI-backed:** only once a post-change commit runs the 3-platform test matrix successfully. **Not yet measured:** savings in GPU time or money, external ComfyUI integration on a real model, YouTube publication, user-perceived video quality and post-crash remote reconciliation. ComfyUI's request acceptance does not guarantee a billing refund or immediate preemption. This change adds no new paid API calls or model dependency.
