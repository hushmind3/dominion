# Sub3 runtime / intelligence separation
Date: 2026-11-20. Scope: `dominion-sub3` branch only.

## Problem

A model-loop SDK that directly owns runtime admission, natural-language intent rules, tool relevance, memory projection and repair heuristics becomes expensive to modify or evaluate. In the previous layout, `crates/dominion-agent-sdk/src/lib.rs` assembled all of these and `src/engine/` mixed the product's SQLite-backed host services. The DB and host services were separated first; this phase moves intelligence out of the execution substrate.

## Dependency direction

```text
           natural language / source user request
                            |
                       Product host
                   src/host/ + API
                            |
                dominion-agent-sdk  <----- model/tool/journal host ports
                 |              |
                 v              v
       dominion-agent-runtime  dominion-agent-algorithms
              ^                      |
              +----------------------+
```

**Runtime never imports Algorithms or SDK; Algorithms imports Runtime shared types; SDK composes both.** The Rust host uses the same public SDK facade and continues to own SQLite-backed Journal, approvals, model transports, cancellation and execution effects. No DB code or dependencies enter the independent crates.

| Crate | Owned responsibilities | Explicit non-responsibilities |
| --- | --- | --- |
| `dominion-agent-runtime` | BudgetLedger, ModelTokenBudget, common Strategy/Action/Receipt/ToolSpec contracts, dependency-aware scheduler, bounded JSON-schema admission | Prompt wording, task intent, tool relevance, SQLite, network, model |
| `dominion-agent-algorithms` | AgentAlgorithm trait, StandardAlgorithm, lexical/tool-schema relevance, progressive tool disclosure, compact model context, evidence-derived failure guidance | Effect permission, write-ahead enforcement, SQLite, external tool I/O |
| `dominion-agent-sdk` | Existing Kernel run/resume facade, checkpointed model/tool turn coordination, owner acceptance, evidence reconciliation, trait ports | DB backend, UI menus, algorithm implementation ownership |
| `src/host/` and `src/storage/` | Durable journal adapter, authorization, SQLite, migrations, runtime account state | Portable algorithm design |

## Policy injection contract

```rust
use dominion_agent_sdk::{AgentAlgorithm, Kernel, StandardAlgorithm, RunRequest, Snapshot};

// Existing callers still work exactly as before:
let default_snapshot: Snapshot = kernel.run(request.clone()).await?;

// A caller implementing AgentAlgorithm can replace policy for one run:
let chosen: &dyn AgentAlgorithm = &StandardAlgorithm;
let snapshot: Snapshot = kernel.run_with_algorithm(request, chosen).await?;
```

This is **per-run dependency injection**, not a dynamically installed plugin ABI, marketplace configuration, or an API/UI setting. The host currently selects the standard policy by default. A future user-facing selector requires a versioned, pinned algorithm profile across checkpoint resume.

### Policy-controlled decisions

- `planner_instructions`: system prompt policy for native-tool and text-only model formats
- `classify`: task-effect and source-edit intent (baseline rules cannot be weakened)
- `validate_plan`: extra policy checks on model proposals, after immutable SDK size limits
- `select_tools`: task-relevant shortlist over the already-authorized tool catalog
- `project_context`: compact model-visible receipt projection, full journal retained
- `failure_guidance`: model-facing next inspection hint, never an effect or verified claim

### Kernel invariants remain enforced

The model and selected algorithm cannot add new tools, change a tool schema, alter the read-only/effect bit, skip the host approval dialog, discard a durable receipt, rewrite the pinned checkpoint, bypass per-run budgets or accept an unobserved write. `Kernel` checks name/description/schema/classification against the original host catalog **before invoking the model**; writes still pass through the host and Journal.

## Actual product job selection — implemented

The Rust product now accepts an optional algorithm profile **per job**, independently of model/provider selection:

```json
{"kind":"prompt","input":{"prompt":"Inspect the repository and fix the bug","algorithmId":"focused-v1"}}
```

For the existing native SDK compatibility endpoint:

```json
{"instruction":"Inspect the repository and fix the bug","algorithmId":"focused-v1"}
```

The default when omitted is `standard-v1`. The allowlist lives under `src/features/cognition/profiles.rs`, not inside the Rust execution kernel. `focused-v1` uses shorter model plans and a compact context view for non-VerifyFirst jobs; both profiles share the same durable effect admission and approval rules.

The job record pins `nativeAlgorithm: {format, id, revision}` before any provider/model call. The SDK snapshot separately pins `algorithmId` before any model invocation or external tool call. On recovery, either an ID/revision change or a snapshot mismatch fails closed instead of silently switching policies. A preexisting, unpinned snapshot can resume only with the original default profile. Increment the immutable revision when algorithm semantics change; this is an explicit implementation revision, **not a source-code-derived cryptographic hash**.

The TypeScript SDK exposes `AlgorithmId` on both `jobs.create` and `nativeRuns.start`; SDK run responses expose `algorithmId` and `algorithmRevision`. No new UI tab, menu, permanent memory or global runtime setting is created.

## Tests and scope

```sh
cargo test -p dominion-agent-runtime --lib
cargo test -p dominion-agent-algorithms --lib
cargo test -p dominion-agent-sdk --test algorithm_policy_injection
cargo test --bin dominion-rust features::cognition::profiles::tests
cargo test --bin dominion-rust api::jobs::algorithm_profile_api_tests
cargo test -p dominion-agent-sdk --lib
node --test scripts/algorithm-boundary.test.mjs
```

The Rust tests cover unchanged default behavior, injected strict plans, no loss of the baseline edit-evidence rule and fail-closed rejection of unauthorized algorithm tool declarations. This architecture is **source/contract separation**, not proof of token savings, general-model reasoning improvement or a dynamically swappable binary plugin.

## Context from 2026 upstream sources

[OpenClaw agent runtime architecture](https://docs.openclaw.ai/agent-runtime-architecture) distinguishes reusable agent core, product runtime facade, tooling and plugin-facing boundaries. [LangGraph's repository architecture](https://github.com/langchain-ai/langgraph/blob/main/AGENTS.md) separates the runtime/framework from higher-level agent libraries, while [Google's ADK architecture](https://github.com/google/adk-recipes/blob/main/core/python/long-horizon-harness/docs/architecture.md) models runners, plugins and pluggable services. These are architectural references; DOMINION's Rust crates do not import their runtimes.
