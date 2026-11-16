# Sub3 video: project-first, locally installable production (2026-11-16)

## Owner-visible entry point

The `dominion-sub3` branch (not `main`) routes the **Video → 자율 콘텐츠 제작** tab to `web/src/components/VideoAutopilot.tsx`. The first screen accepts a goal, audience, style, target network, aspect ratio, duration and an engine. The user's `autoRender` and `autoUpload` choices are persisted in each project; YouTube upload without the owner's opt-in does not occur.

The AI video option now renders `VideoModelInstall.tsx` in this first screen. It offers actual supported model IDs, the host-native folder picker, ComfyUI address, **선택·다운로드·설치**, real download progress and interrupted-transfer resume, plus **ComfyUI 실행** for an already-installed portable/venv runtime. It never marks a downloaded model as executable just because its files exist.

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

- `POST /api/video/models/:id/setup`: validate one of the built-in Wan model IDs, resolve an existing `models` folder from ComfyUI root or Windows portable root, atomically save the selected engine/model/workflow mode, and enqueue/resume missing Hugging Face weight files. Missing folder fails before changing model selection. Existing installation returns `ready`; a transfer returns `downloading`.
- `GET /api/video/catalog`, `/api/video/status`, `/api/video/models/downloads`: actual selected model, filesystem availability, download status and ComfyUI runtime/node readiness. Files of unrealistically small sizes do not qualify as completed model weights.
- `POST /api/video/comfy/start`: only permits a localhost HTTP endpoint and a Python executable inside a recognized local ComfyUI Portable or venv folder. Launch logs stay in the local Dominion data directory. No externally supplied executable or shell command is evaluated. Installed ComfyUI must already have a valid Python environment.
- `GET/POST /api/video/projects` → stored `video_plan` job → AI or offline storyboarding → scene editing with version conflicts → idempotent `video` render job. Edited scripts, captions and scene timings affect the actual native FFmpeg render. Multi-shot ComfyUI plans invoke a video render per shot and concatenate completed MP4 segments.
- Completion checks the MP4 file signature and writes `captions.srt` and `production.json`; these accompany the MP4 through `/api/video/jobs/:id/*` endpoints.
- Explicit `autoUpload` is limited to private or unlisted YouTube delivery and uses the existing OAuth plus resumable-upload job. Unknown external outcomes require review rather than blind replay.

## Model choice and resource limits

Default model suggestion: **Wan2.1 T2V 1.3B FP16**, using official Comfy-Org model weights and built-in ComfyUI nodes. The 4-bit Wan GGUF option reduces the diffusion-model weight size but **requires separately installed ComfyUI-GGUF custom nodes** and a sizeable UMT5 text encoder. A multi-GB download and sufficient RAM/VRAM/disk space are required even for the 4-bit path. Local inference speed and visual continuity depend heavily on hardware, graph and scene settings; no paid API or fixed saving percentage is claimed.

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
