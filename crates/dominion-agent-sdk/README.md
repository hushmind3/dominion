# DOMINION Rust Native Agent SDK

The independent **`dominion-agent-sdk`** crate owns budgeted model/tool execution, plan/decision snapshots, tool authorization contracts, bounded JSON Schema validation, durable receipts, independent acceptance checks and fail-closed crash/recovery behavior. It is not a built-in media renderer or an autonomous self-improving RSI engine.

## Recovery authority and bounded history

The native host durably pins a SHA-256 binding over the authorized tool names, JSON schemas, read-only classification, acceptance checks and evidence requirement before model or tool execution. A changed binding on an existing run results in `needs_review` instead of adopting a broader tool contract. Legacy checkpoints remain loadable and receive a binding at their next safe continuation. The Rust kernel rejects duplicate names and attempts to shadow its reserved history/discovery tools before making a model call.

`dominion_receipt_search` pages immutable history, while `dominion_receipt_read` supports `field: "result" | "arguments"`, `startChar` and `maxChars`. Oversized arguments are shown as a bounded preview by default and can be retrieved incrementally without repeating the original tool. This limits model-visible text without deleting durable evidence.

For large allowed MCP registries, `dominion_tool_search` and fixed `dominion_tool_call` bridge offer progressive discovery without changing provider function schemas. The host validates final arguments, tool allowlists and pinned MCP manifests before effectful execution. Owner intervention never becomes a fabricated success receipt.

`replay::trace(&snapshot)` reconstructs recorded decisions and effects **offline**; it neither calls a model nor repeats tools or automatically promotes code. The product host `src/engine/native_replay.rs` cross-checks independent operation journals, reported cache usage and configured KRW tariffs.

`src/scheduler.rs` reserves all historical operation IDs but considers only successful receipts valid prerequisites for later actions; failed/skipped receipts force replanning. `replay::trace` detects mismatched regular-tool arguments and claims of success despite failed dependencies. Discovery bridge argument resolution is deliberately exempted from literal model-argument equality. These checks validate journal consistency, not current workspace state.

This SDK does not implement generation-level campaigns, autonomous core mutation, video/audio production, or unattended optimizer policies. User-requested coding/refactoring/testing tasks run through the normal Rust harness and acceptance criteria.

Run `cargo test -p dominion-agent-sdk`. See [lean product architecture](../../../docs/SUB3-LEAN-COMPLEXITY-2026-11-16.md).

## Task-local evidence-driven strategy comparison (2026-11-18)

The native execution kernel now accepts an **optional standalone** `candidatePaths` decision with 2–3 distinct hypotheses, each containing 1–3 independent, host-declared read-only tool actions. All candidate actions are admitted as a unit; an unknown or effectful tool in any candidate rejects the entire proposal before dispatch. They execute through the existing scheduler, global tool-call budget, cancellation/directive fence and write-ahead journal — **no nested optimizer or second worker**. Every candidate receipt is attributed by `replay::trace`.

The next model turn receives `candidateProbeComparison` reconstructed solely from the durable decision tape and actual tool receipts. It contains per-path successful/failed/pending counts and, only when observed outcomes differ, a tentative preferred path. Equal observations leave the preference unset. The model can then inspect evidence and replan using the ordinary agent actions. A successful probe is not an owner acceptance check and cannot authorize a deployment, code write, or final completion.

Example text-planner response (also accepted as an explicit native-model probe envelope when no native tool calls are emitted):

~~~json
{
  "plan": ["Compare read-only approaches before editing"],
  "candidatePaths": [
    {"id": "direct", "hypothesis": "Inspect the entrypoint", "actions": [
      {"id": "probe-direct-1", "tool": "workspace_read", "arguments": {"path": "src/main.rs"}}
    ]},
    {"id": "diagnose", "hypothesis": "Inspect the failing regression", "actions": [
      {"id": "probe-diagnose-1", "tool": "workspace_read", "arguments": {"path": "tests/e2e.rs"}}
    ]}
  ]
}
~~~

The two probes are exploratory only. After comparing observations, the normal Rust kernel executes owner-approved effects and independent postconditions. Legacy checkpoint JSON retains its default empty `candidatePaths` and no migration is required.

Technical reference: DeepSeek Harness PTC subcall admission; Google Research Harness² task-level Propose–Probe–Compose; Airbnb's Agent Harness Optimizer/PRISM evidence evaluation. This implementation **does not** claim their full recursive optimization, arbitrary program interpretation, benchmark lift, or paid-model efficacy.

Focused deterministic tests:
`cargo test -p dominion-agent-sdk probes::tests::`,
`cargo test -p dominion-agent-sdk --test kernel_replanning contrasting_read_only_probes_are_observed_then_the_kernel_replans`,
`cargo test -p dominion-agent-sdk replay::tests::contrastive_probe_receipts_have_model_attribution_without_tool_replay`,
plus `cargo test -p dominion-agent-sdk` for the complete SDK. These tests remain unexecuted in this source-editing environment; full CI is deferred.
