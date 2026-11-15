# Dominion native architecture — 2026-11-15

## Unified product

~~~text
Main React/Vite workspace   External TypeScript SDK
            |                         |
            +---- HTTP / SSE API -----+
                        |
                   Native Rust SDK
                        |
       +----------------+----------------+
       |                |                |
    SQLite          ModelClient      Tool dispatcher
    task/event      provider select  workspace/MCP
    session         economy/quality  owner permission
    memory          usage/cost       receipts/verify
    presets         prompt context   child execution
       |                |                |
       +---- native task lifecycle ------+
              |        |        |
       Multi-agent   Video    Improvement
       group turns   FFmpeg   frozen worktree
              |        |
        Durable workflows (step tasks)
~~~

Only Rust decides task admission, planning, model turns, side effects, explicit user corrections and completion. The TypeScript package provides typed transport and a thread convenience layer; it cannot substitute an external agent engine. React uses the same backend routes. The app is loopback-bound, single-owner and not a hardened multi-user service.

## Source ownership

| Area | Native implementation | Browser integration |
| --- | --- | --- |
| SDK / task lifecycle | src/runtime.rs, src/store.rs | NativeWorkspace.tsx |
| Agent model/tool loop | src/engine.rs, src/engine/files.rs, src/engine/strategy.rs | Task event stream |
| Source revision checks and real test-driven repair | src/engine/verification.rs, src/engine/files.rs, src/engine/acceptance_tests.rs | NativeWorkspace.tsx test settings and diagnostics |
| Verified long-term semantic context | src/engine/context.rs, src/engine/semantic.rs, src/context_memory.rs, src/store.rs | Evidence inspector and task_receipt/context_search tools |
| Provider routing and usage | src/provider.rs | NativeSystem.tsx models |
| MCP | src/mcp.rs | NativeSystem.tsx extensions |
| User-defined personas | src/store.rs agents table, src/runtime.rs | NativeAgents.tsx |
| Sequential workflow execution | src/workflows.rs, SQLite workflow_runs | NativeWorkflows.tsx |
| Group chat | src/group.rs | NativeWorkspace.tsx groups |
| Local and remote video | src/video.rs, src/veo.rs | NativeWorkspace.tsx video |
| Isolated improvement experiments | src/improvement.rs | NativeWorkspace.tsx improvement |
| Memory / skills | src/store.rs, src/knowledge.rs | NativeWorkspace.tsx knowledge |
| Historical data migration | src/legacy.rs, src/store.rs | CLI opt-in |
| API routes and SPA host | src/api.rs | web/dist after Vite build |
| Main visual language | web/src/App.tsx, web/src/components, web/src/i18n, web/src/theme.css | All live pages |

The original main React source and look are preserved; old API-incompatible page modules remain as references and are not used for active primary navigation. Do not restore retired specialized modules or create a second TypeScript SDK under packages.

### Proof-preserving context management

The completed native context feature is documented in [docs/CONTEXT_EVIDENCE.md](CONTEXT_EVIDENCE.md), including the exact invariants, hard encoded-byte budgets, parent/child tool graph parsing, deterministic projections and SHA-256 source/projection receipts. The task_receipt and session_history tools retrieve persisted UTF-8 byte pages; no effects are replayed. A resumed task's lineage is committed atomically before any worker can claim it. Projected bytes are neither billed-token counts nor claims about external frontier success rates.

### Native coding acceptance loop

The second focused feature is documented in [docs/CODING_ACCEPTANCE.md](CODING_ACCEPTANCE.md). Owner-specified \`acceptanceChecks\` and conservative manifest autodetection are validated in Rust before execution. Code-editing tools retain verified source hashes. At the model's proposed finish, the Rust verifier requires real passing process exit codes after the last source mutation; on failure, the exact diagnostic is persisted and sent back into a bounded repair turn. No previous green check can certify a newer revision, and unchanged failing code is not automatically rechecked.

A workspace lease protects an entire mutating execution from other local Dominion tasks through final acceptance, while read-only tasks remain concurrent. \`workspace_read_lines\` permits large source inspection. \`workspace_replace_many\` prevalidates and stages up to 8 exact source edits, uses readback checks and best-effort rollback. This does not constitute a cross-process sandbox or filesystem-wide atomic transaction. \`acceptance_complete\` is a Rust-issued durable event and the persisted final task result contains the exact command proof.

## Session consistency

1. Rust SDK validates a unique requestId and stores the task in SQLite.
2. Workers atomically claim queued tasks, sharing per-session concurrency limits. They do not auto-replay uncertain effects after process loss.
3. A model receives a bounded projection preserving tool calls alongside their results, the newest user instruction and system constraints; all originals remain persisted.
4. Workspace tools require explicit permission. Writes use readback evidence and a stored tool-result receipt before another model turn.
5. Completion is decided from observations, not just model claims. Steps and provider usage accumulate in the task row and append durable events.
6. SSE resumes using the persisted cursor. An interrupted user task starts a **new linked task** only after an explicit fresh instruction, new requestId and fresh permissions. The read-only task_receipt tool can recover observations removed from model-facing context without repeating effects.
7. Multi-step workflows are owner-defined sequences of the exact same native TaskInput objects. Each step has a deterministic retry key, a shared workflow session and independent completion/verification; interrupted workflows remain needs_review.

## Provider and cost contracts

Operator-configured main and economy models use OpenAI-compatible transport; no subscriptions are resold by the runtime. Auto routing delegates uncomplicated read-only tasks to an economical model when available. Cached input tokens are a subset of prompt tokens and are billed using the separate configured cached rate. Usage is explicitly marked estimated if the provider omits prompt/output token counts. Cost budgets cannot be presented as enforceable for remote providers without configured input/output rates. Provider rates are **assumptions** until confirmed against external invoices.

Provider-cached prefixes are preserved where possible. Automatic prompt caching is model/provider-specific and cached token reports are authoritative only when returned by that provider. A stable sequence of tool schemas and system context helps caching; context projection can reduce transmitted tokens but may also invalidate cache prefixes, so no savings percentage is claimed without a matched measurement.

## Video and iterative improvement

Local videos are deterministic FFmpeg compositions at 1280x720, containing owner-provided animated images, titles, carefully wrapped captions, and optional locally synthesized Piper/eSpeak audio. Per-scene file invalidation prevents paying to regenerate unchanged clips; portable MP4 replacement retains the previous artifact until the new one has been successfully renamed. External Veo calls require a configured positive cost rate and per-video KRW ceiling before the next billable scene. The storyboard, local clips, audio, remote operation references and output path are persisted.

Improvement experiments are source-edit proposals isolated from the primary checkout, with frozen baseline comparison, test/score evidence and explicit adoption. Accepted candidates are pinned under a Git `refs/dominion/improvements/<UUID>` ref before their detached worktree is removed; adoption verifies the pinned commit matches the independently evaluated candidate. This retains candidate ancestry through Git garbage collection and prevents an orphaned SHA from silently breaking the descendant path. No model weight-training pipeline is part of RSI. Current loop requires fuller cross-domain feedback and held-out evaluation before any frontier-ranking claim.

## Migration and known limits

Legacy migration is opt-in using DOMINION_IMPORT_LEGACY_DB. It opens the source SQLite read-only and uses a destination transaction, preserving original non-executable JSON records in legacy_archive. It projects permitted user-authored data and safe transcript roles only, disables imported skills and never activates old jobs or provider credentials. Data import records are idempotent per source path.

The current GitHub-connector workflow cannot run a Rust toolchain or full React end-to-end browser tests. Static source changes, checked call paths and authored unit tests are **not** a substitute for compiled runtime proof. Full multi-provider management, browser automation, high-end generative film output, long-running distributed workers, multi-user isolation and measured 2026 frontier benchmark parity remain incomplete.

## Frontier design references

- DeepSeek Harness: event-backed sessions, scoped tool dispatch, provider separation — https://github.com/deepseek-ai/deepseek-harness/tree/master/docs
- Qwen Code 2026 session agents: durable sessions and cooperative orchestration — https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-10-05-session-multi-agent.md
- OpenAI Codex Rust: independent Rust-owned model/tool engine — https://github.com/openai/codex/tree/main/codex-rs/core/src
- OpenAI prompt caching: prefix preservation and cached usage accounting — https://developers.openai.com/api/docs/guides/prompt-caching
- MCP Rust SDK 2026: remote tool protocol state and native typed API — https://github.com/modelcontextprotocol/rust-sdk
- GEPA 2026: correctness-gated candidate evaluation and reuse of observations — https://github.com/gepa-ai/gepa

These sources inform architecture; no external engine is embedded and no independent performance score is inferred from them.
