# Sub3 video: project-first, locally installable production (2026-11-16)

## Owner-visible entry point

The `dominion-sub3` branch (not `main`) routes the **Video → 자율 콘텐츠 제작** tab to `web/src/components/VideoAutopilot.tsx`. The first screen accepts a goal, audience, style, target network, aspect ratio, duration and an engine. The user's `autoRender` and `autoUpload` choices are persisted in each project; YouTube upload without the owner's opt-in does not occur.

The video-only model catalog in `VideoModelInstall.tsx` is **always shown on the first production screen**. It shows the same Wan2.1 text-to-video family with explicit Q3_K_M, Q4_K_M, Q5_K_M, Q8_0, and FP16 choices and separate base model / complete required download sizes. A single Download button saves weights and shared encoder/VAE files in a detected ComfyUI library or in Dominion's own data directory. Download is **acquisition only**: it never changes the current rendering engine or overwrites an imported custom workflow. Once the files exist, a separate **이 모델 사용** control explicitly activates that variant. No folder picker or server address is required to download. Real progress and interrupted-transfer resume remain visible. ComfyUI configuration and launch controls are collapsed in Advanced, not prerequisites for obtaining model files.

## Run the branch at its actual development port

From a checkout of **`dominion-sub3`**:

```sh
git switch dominion-sub3
npm ci --prefix web
npm run build --prefix web
cargo run --release -- --home .
```

The Rust host serves the compiled React UI at **http://127.0.0.1:4317** by default (or `DOMINION_PORT`). Serving a previously built `main` `web/dist` will obviously show the old UI, even if the Rust source has changed. `npm run dev --prefix web` is a separate Vite development server.

## Actual source and execution contracts

- `POST /api/video/models/:id/setup`: validate the requested **video-generation model + quantization**, choose an existing ComfyUI models library if present or create a managed `runtime/video-models` folder, pin the chosen engine/model and enqueue/resume only missing Hugging Face files. No path input is necessary. An explicitly supplied invalid folder still fails before changing selection. Existing download returns `ready`; an active transfer returns `downloading`.
- `GET /api/video/catalog`, `/api/video/status`, `/api/video/models/downloads`: actual selected model, filesystem availability, download status and ComfyUI runtime/node readiness. Files of unrealistically small sizes do not qualify as completed model weights.
- `POST /api/video/comfy/start`: only permits a localhost HTTP endpoint and a Python executable inside a recognized local ComfyUI Portable or venv folder. Launch logs stay in the local Dominion data directory. No externally supplied executable or shell command is evaluated. Installed ComfyUI must already have a valid Python environment.
- `GET/POST /api/video/projects` → stored `video_plan` job → AI or offline storyboarding → scene editing with version conflicts → idempotent `video` render job. Edited scripts, captions and scene timings affect the actual native FFmpeg render. Multi-shot ComfyUI plans invoke a video render per shot and concatenate completed MP4 segments.
- Completion checks the MP4 file signature and writes `captions.srt` and `production.json`; these accompany the MP4 through `/api/video/jobs/:id/*` endpoints.
- Explicit `autoUpload` is limited to private or unlisted YouTube delivery and uses the existing OAuth plus resumable-upload job. Unknown external outcomes require review rather than blind replay.

## Consumer-GPU clip budget

The integrated AI video project accepts up to **60 seconds** of intended output and divides each rendered Wan scene into clips of at most **5 seconds**. This keeps any one ComfyUI latent graph bounded and preserves scene ordering and target running time. It does not imply that 5-second inference will be fast or fit every 8 GB GPU; actual runtime depends on node offloading and the configured model. Each clip can be inspected and its visual prompt adjusted before rendering.

## Incremental scene re-rendering and real GPU cost signals

When a ComfyUI render completes, every generated short segment gets an immutable receipt containing the frozen configuration/scene render fingerprint and a SHA-256 digest of the actual MP4 bytes. For a revised project, the Rust renderer searches **only completed video jobs of that same project**, verifies matching render settings, clip header, and clip bytes, then reuses an unchanged segment instead of submitting a new ComfyUI GPU request. The project owns a deterministic base seed that is deliberately held stable across edit revisions (the renderer derives each clip's seed from that base and its index). Changed prompts, explicit seeds, duration, model or graph force regeneration. This preserves existing high-quality clips and reduces repeat GPU work without inventing a percentage or money saving.

The completed video result and production QA record expose `segmentsReused` and `segmentsGenerated`; the Video page displays both counts. Existing source files stay immutable; the new project revision gets its own copy. The test `engine::video::tests::unchanged_clips_reuse_completed_artifact_but_changed_scene_is_not_reused` proves matching, invalidation and tamper rejection with a real database and MP4 fixtures, without paid model calls.

## Model choice and resource limits

Default model suggestion for limited storage: **Wan2.1 T2V 1.3B Q4_K_M**, with other Q3/Q5/Q8 quantizations and an FP16 option. The published Q4 diffusion-model file alone is roughly 983 MB; its encoder (~3.66 GB) and VAE (~254 MB) bring total first-time download to roughly **4.9 GB**. FP16 also uses an FP8 text encoder (~6.74 GB), so total initial disk download is close to 9.84 GB. GGUF generation **requires separately installed ComfyUI-GGUF nodes**, which are an execution concern, not an obstacle to browsing/downloading model files. A multi-GB download and sufficient RAM/VRAM/disk space are required even for the 4-bit path. Local inference speed and visual continuity depend heavily on hardware, graph and scene settings; no paid API or fixed saving percentage is claimed.

**Known boundary:** the app can download and validate weight files and launch **an already-installed** ComfyUI environment. Installing a complete GPU driver, CUDA/PyTorch runtime or ComfyUI distribution from scratch is **not** silently performed. Instagram/TikTok presently get ready-to-export MP4/SRT/manifest, not direct account publishing. The native card renderer cannot be presented as cinematographic neural video. ComfyUI realism, GPU resource use and external account delivery require end-to-end device testing.

## Tests and evidence

- `web/scripts/video-studio-smoke.mjs` renders the actual React Video page and asserts that the turnkey model setup exists inside the main autonomous producer.
- `src/api/video.rs` tests reject invalid model installation folders and remote-address process launches without changing selected model settings.
- `src/engine/video.rs` asserts model asset layouts/size floors, real MP4 output and render graph wiring.
- `src/engine/video_studio.rs` exercises a persisted video project, edited scene text and a real FFmpeg/SRT production.
- `.github/workflows/sub3-product-acceptance.yml` checks the above contracts and web build; see the exact commit's Actions results. Successful synthetic/FFmpeg tests do **not** prove that untested multi-GB downloads or ComfyUI inference ran on every user's PC.

## 2026 upstream references

- [Comfy-Org/Wan2.1 model artifacts](https://huggingface.co/Comfy-Org/Wan_2.1_ComfyUI_repackaged)
- [Wan2.1 official ComfyUI text-to-video template](https://github.com/Comfy-Org/workflow_templates/blob/main/templates/text_to_video_wan.json)
- [Wan 1.3B GGUF model files](https://huggingface.co/samuelchristlie/Wan2.1-T2V-1.3B-GGUF/tree/main)
- [ComfyUI-GGUF custom loaders](https://github.com/city96/ComfyUI-GGUF)
- [ComfyUI official runtime](https://github.com/Comfy-Org/ComfyUI)
