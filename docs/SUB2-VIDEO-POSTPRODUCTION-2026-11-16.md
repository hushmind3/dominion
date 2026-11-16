# Sub2 — One-topic video production, durable clips, audio and delivery (2026-11-16)

## Connected flow
This change belongs exclusively to `dominion-sub2`.

`web/src/pages/Video.tsx` (brief / optional narration / source / output format) → `POST /api/video/jobs` / `sdk::Dominion::submit_video` / `@dominion/sdk.video.create` → `engine::jobs::enqueue` → `engine::video::run` → opt-in `plan_video_scenes` via the configured text provider → frozen visual + narration scene plan in `videoState` → per-scene ComfyUI prompts → SHA-256 scene manifests → restart-safe clip reuse → FFmpeg concat → optional native system TTS + editable/selectable MP4 captions → completed MP4 → `video.upload` / user-controlled YouTube posting.

The video composer retains the existing 4 studio tabs and source model selection. It exposes an owner-authored narration override and reports actual speech/caption flags, restarted scene reuse and warnings on the finished file. The TypeScript SDK and embedded Rust SDK share the same Rust engine; the web UI uses the same HTTP routes. `video.cancel` targets the recorded ComfyUI prompt UUID, not the global interruption API.

## Freeze and replay
The planner now requests one JSON response with `{scenes:[{visual,narration}]}` and accepts legacy JSON arrays for compatible providers. Both video prompts and narration text are checkpointed before rendering. If the Rust process restarts at a completed clip boundary, it reads that frozen plan and verifies per-scene signed input fingerprints, stored SHA-256 and actual MP4 bytes before reuse. Incomplete or corrupt clips are regenerated alone; the physical output directory and previous SHA manifests are not discarded. The final MP4 is swapped using a backup-and-rollback path on Windows/macOS/Linux rather than assuming rename-overwrite works. An in-flight remote prompt at crash time stays `needs_review` rather than blind re-submission.

## Large model download resilience

Video model installers now serialize access **per ComfyUI model directory** so concurrent Wan variants cannot overwrite their shared CLIP/VAE `.part` files. They preserve an incomplete partial file and its strong HTTP ETag on interruption. A retry requests only the missing bytes using `Range` + `If-Range`, and appends only on a verified HTTP 206 response with matching strong ETag, exact requested start offset and bounded Content-Range. An HTTP 200 response starts a new file rather than mixing model revisions. A completed transfer removes its validator sidecar. No performance savings are claimed until a real network transfer is measured. The in-process single-writer lock does not coordinate separate instances running in different OS processes.

## Audio/caption limits
Optional narration is produced with the host's existing macOS `say` or Windows SAPI voice and appended to the completed ComfyUI footage through FFmpeg. Source video remains useful without installed speech: a subtitle track can still be produced, while the persisted receipt records `narrated=false` and a warning. An owner-authored script overrides planned narration. The audio route does not claim state-of-the-art neural voice quality or an exact scene-to-phoneme alignment. Captions are an MP4 mov_text stream, not a browser-only decoration, and are toggled in the existing video settings. All results require the actual FFmpeg output; no implicit YouTube publication occurs.

## Reproducible tests
- The mock ComfyUI test generates an actual fixture MP4 with FFmpeg, runs two prompts through a local HTTP emulator, checks scene manifests and a playable merged MP4, re-runs the same job without repeating either prompt and corrupts one clip to require exactly one fresh prompt.
- Rust verifier tests reject missing/tampered files; structured storyboards and SRT timing are tested offline.
- Rust SDK and TypeScript SDK have creation/cancel/upload contract tests using the same source API paths.
- Exact Windows/macOS/Linux `cargo test --release` results are provided only by GitHub Actions for the latest commit; no real Wan GPU, genuine YouTube upload or external quality comparison is claimed.

## Frontier sources consulted
- OpenHands SDK event-sourced conversation persistence: https://github.com/OpenHands/docs/blob/main/sdk/arch/conversation.mdx
- ComfyUI prompt-scoped cancellation and graph execution: https://github.com/Comfy-Org/ComfyUI/blob/master/server.py
- 2026 clip checkpoint/reuse design: https://github.com/seitanism/ComfyUI-H3-Motion-Context-MultiRef/blob/main/MODIFICATIONS.md
- 2026 Chinese durable execution review: https://github.com/runzhliu/aik8s/blob/main/docs/ai-k8s/rag-agent/agent-long-running-durable-execution.md

Do not claim matched frontier quality or saved cost percentages without a same-model, same-task measured run. The branch still has a local OS voice fallback, no unified GPU cost billing, and no real-world end-to-end upload benchmark.
