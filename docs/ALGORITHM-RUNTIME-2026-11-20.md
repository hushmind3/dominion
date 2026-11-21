# Sub3 runtime / intelligence separation
Date: 2026-11-20. Scope: `dominion-sub3` branch only.

## 2026-11-21 product behavior: failed-effect completion gate

- `crates/dominion-agent-algorithms/src/agent_loop.rs` refuses an unsupported final answer when the owner's request needs a real external effect, evidence is required, and the latest attempted non-read effect has a failed durable receipt. A prior successful effect and later read-only inspections cannot turn that failed operation into a successful final result.
- The planner receives a specific `last_external_effect` verification gap and can perform a corrective action using the normal Host permission, budget, approval, cancellation and journal gates. A subsequent successful effect allows the normal independent acceptance checks. Repeated unsupported completion attempts end as `NeedsReview` under the existing bounded invalid-decision rule.
- `crates/dominion-agent-sdk/tests/kernel_replanning.rs` contains scripted real-kernel tests for failed deploy → unsupported success claim → corrective operation → true completion, and failed deploy → repeated false claims → review. This is a deterministic host/tool fixture, **not** a live deployment or model accuracy benchmark.
- This complements provider-status correctness discussed in [Qwen Code issue #9509](https://github.com/QwenLM/qwen-code/issues/9509); the Dominion rule is an independently implemented receipt-based final-answer gate.
- `crates/dominion-agent-algorithms/src/ranking.rs` also adds Korean connector-intent normalization, shared with Chat/Native, with full SDK and Chat selection regressions. See `docs/SUB3-PROGRESSIVE-TOOLS-2026-11-16.md`.
- Exact submitted SHA and CI job results must be read from GitHub Actions; no synthetic fixture constitutes measured paid-model savings or a user's existing local app validation.

## Problem

A model-loop SDK that directly owns runtime admission, natural-language intent rules, tool relevance, memory projection and repair heuristics becomes expensive to modify or evaluate. In the previous layout, `crates/dominion-agent-sdk/src/lib.rs` assembled all of these and `src/engine/` mixed the product's SQLite-backed host services. The DB and host services were separated first; this phase moves intelligence out of the execution substrate.

## Dependency direction

```text
User / API / session / provider / channels
                   |
         src/host/execution/  (select profile + loop)
                   |
             AgentLoop trait  (core contract)
                   |
       +-----------+-----------+
       |                       |
  Algorithms                Runtime
  agent_loop.rs          contracts.rs
  ReActLoop              Model/Tools/Evidence/Journal
  context/plan           budget, scheduler
  tool ranking           acceptance/dispatch
       |                       ^
       +-----------------------+
                   |
             SDK facade
     (stable re-export for API clients)
```

The **dependency direction** is `algorithms → runtime` and `sdk → algorithms + runtime`. `runtime` never imports the algorithms or SDK crates. `AgentLoop` and the basic `AgentPorts` capability contract are defined in `runtime/contracts.rs`; the actual 760-line ReAct-compatible orchestration, replanning and model-turn workflow live in `algorithms/agent_loop.rs`. The SDK's `lib.rs` only re-exports the stable names (13 lines). Host-owned DB/Journal, credentials and channels remain separate from all three crates.

| Crate | Responsibility | Must not contain |
| --- | --- | --- |
| `dominion-agent-runtime` | Model/Tools/Evidence/Journal and AgentLoop traits; approved contracts, budgets, safe dispatch/acceptance, durable snapshot types | Agent turn loops, prompts, memory DB, provider SDKs |
| `dominion-agent-algorithms` | ReActLoop / Kernel turn orchestration, profile prompts, compaction, tool ranking, replanning, failure guidance, offline replay | Database, direct frontend routes, privileged network credentials |
| `dominion-agent-sdk` | Stable re-export facade preserving existing public APIs | New implementations, parallel agent loops, host data stores |
| `src/host/` | Choose version-pinned algorithm, bind Loop+Ports, enforce host permissions and durable job identity | Model planning algorithms |
| `src/storage/` | SQLite persistence, vault, migrations | Model reasoning and workflow policy |

This is a **compiled Rust trait and crate boundary**, not sandbox isolation: a new agent-loop implementation is trusted Rust code and can call the host's supplied ports. Untrusted plugin loops require an additional WASM/process isolation and capability validation layer. We have not installed WASM or shipped a user-facing loop marketplace.

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

The job record pins `nativeAlgorithm: {format, id, revision}` before any provider/model call. The trusted runtime SDK snapshot separately pins `algorithmId` before any model invocation or external tool call. On recovery, either an ID/revision change or a snapshot mismatch fails closed instead of silently switching policies. A preexisting, unpinned snapshot can resume only with the original default profile. Increment the immutable revision when algorithm semantics change; this is an explicit implementation revision, **not a source-code-derived cryptographic hash**.

The host now invokes `ReActLoop` through the runtime's `AgentLoop::execute(AgentPorts, request, checkpoint)` trait. Replacing the loop implementation is now a compile-time extension point independent of policy profile selection; no second production loop has been introduced.

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
