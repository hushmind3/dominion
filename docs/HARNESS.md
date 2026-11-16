# DOMINION Rust agent harness — current contract (2026-11-17)

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

## Provider connection reuse

All ordinary chats, room turns and SDK invocations use pooled reqwest transports on cloned AppState handles, keyed by the existing per-request timeout policy (room and standard). This reuses sockets and TLS negotiation; bearer credentials remain attached to individual model requests and are not cached in shared state. The same provider fallback and request deadlines remain in force.

## Background behavior

Ordinary chat and rooms default to `autoLearning: false`. Owner corrections remain independently recordable. One bounded event-driven dispatcher wakes on queued jobs; a ten-second fallback reads only indexed queued/running records to protect durable work from missed events. Unrelated bus traffic never triggers dispatch scans. The 30-second automation and approval maintenance pass uses indexed SQLite queries for due/enabled automations and pending approvals rather than deserializing all historical records. Due automation claims enqueue their job and advance the next execution time in the same SQLite transaction, preventing duplicate launches across concurrent ticks and process crashes. Restart recovery scans only active jobs and running executions/tool operations; uncertain effects are detected in a single SQL join. Completed history stays untouched and in-flight tools are held for review. Deterministic experience guidance and distillation run only after changed evidence rather than constantly reevaluating untouched history. Free-model catalog refreshes run only via the explicit Models-page action; an idle server does not poll every provider's model endpoint. No RSI autonomous sweeps run.

## Evidence-based SDK completion

The SDK retains the model reply for inspection but exposes the terminal `taskOutcome`, `verified`, and `acceptancePassed` from the canonical native execution. SDK status views query only the latest execution of SDK-owned sessions and unknown associated tool effects, using SQLite partial indexes rather than deserializing full historical execution/tool collections on every UI poll. A requested verification command without independent success evidence cannot return a successful SDK status. Terminal writes use a status compare-and-patch so cancelled runs and owner edits cannot be overwritten by an obsolete completion. On restart, completed SDK runs are settled from their persisted execution verdict, not answer text alone. Legacy conversational clients still receive a `completed` status without an explicit verification request, alongside `verified: false` for unproven outcomes.

## Regression contract

CI validates TypeScript UI and SDK types, the native architecture graph, and Rust workspace tests on Windows, macOS and Linux. Core acceptance cases include durable state, cancellation, approvals, tool receipts, delegation isolation, chat, memory and retrieval. Deployment is not declared successful until all matrix jobs pass.
