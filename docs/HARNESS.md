# DOMINION Rust agent harness — current contract (2026-11-16)

## Execution graph

`Chat / Rooms / Tasks / Workflows / SDK → product adapter → dominion-core → model / MCP / tool drivers → SQLite journal`

- The **Rust core** owns execution stages, tool admission, budgets and replay-safe receipts.
- Product adapters own provider transport, task context, verification evidence and storage.
- Tools are gated by approval, read-only delegation, cancellation and remaining budget.
- Side-effect results that cannot be confirmed after a crash are marked **needs_review**, never silently replayed.
- User-requested source edits follow ordinary `inspect → edit → test → verify → report`; no RSI campaign or auto-adoption loop.
- Explicit user corrections, verified procedures, failures and existing chat/file/task records remain persistent.

## Retired subsystems

Native FFmpeg/ComfyUI video composition, narration generation, scene regeneration, YouTube auto upload, RSI campaigns, source-mutation generations, autonomous grading, automatic core adoption and associated external runtime orchestration are retired. Existing media files and job records remain available through read-only archive endpoints. New media creation belongs to explicit MCP integrations or external APIs, not a built-in engine.

## Background behavior

Ordinary chat and rooms default to `autoLearning: false`. Owner corrections remain independently recordable. One bounded event-driven dispatcher wakes on queued jobs; a ten-second fallback scan protects durable work from missed events. Deterministic experience guidance and distillation run only after changed evidence rather than constantly reevaluating untouched history. No RSI autonomous sweeps run.

## Regression contract

CI validates TypeScript UI and SDK types, the native architecture graph, and Rust workspace tests on Windows, macOS and Linux. Core acceptance cases include durable state, cancellation, approvals, tool receipts, delegation isolation, chat, memory and retrieval. Deployment is not declared successful until all matrix jobs pass.
