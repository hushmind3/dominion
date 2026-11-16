# Sub3: evidence-bound execution governor (2026-11-16)

## Why this is an engine change, not a second agent or a UI mock

A successful write-tool receipt and the model saying "done" are not proof
that the final workspace still contains those bytes. Another tool, an
acceptance command, or a concurrent editor can mutate the artifact after the
first write. The prior kernel tested owner-specified criteria but did not
automatically rehash the final result of every trusted workspace edit.
Separately, max model *turns* did not bound multiple tool calls per turn;
the model could propose up to 12 operations per decision.

These are two concrete failure modes in long-horizon work: false-positive
completion and unbounded operation spending. Both are fixed inside the
existing Rust harness used by the product, not in a JavaScript prompt loop.

## Real execution path

1. `src/engine/tools.rs` returns `postSha256` with every successful
   `workspace_write` path. Range/text replacements already calculate
   `postSha256`. The hash covers the bytes passed to the actual writer.
2. `crates/dominion-agent-sdk/src/acceptance.rs` derives
   `FileSha256` witnesses for the latest successful edit per relative file.
   Failed writes and arbitrary model text cannot create a witness.
3. `crates/dominion-agent-sdk/src/lib.rs` adds these **kernel-owned** checks
   to the existing acceptance path. It evaluates them *before* expensive
   acceptance shell commands and *again after* those commands, so a test
   command that modifies the file cannot silently invalidate earlier
   evidence. At most 64 changed paths may be auto-verified per run; an excess
   fails explicitly instead of silently skipping files.
4. `src/engine/native_agent.rs` independently reads and hashes the final
   real file inside the workspace sandbox, rejects symlink escapes and files
   larger than 16 MiB per verification. A mismatch creates a failed
   `CheckResult` with the path, persists it in the execution snapshot,
   and gives the planner an actual `verificationGaps` signal next turn.
5. `src/api/sdk_runs.rs`, the public
   `sdk/typescript/src/index.ts`, and
   `web/src/components/NativeAgentPanel.tsx` expose the *same* durable
   `verificationChecks` evidence. No new product navigation or engine
   replica was added.
6. The Rust `Limits.maxToolCalls` admission gate rejects an entire
   over-budget proposed action batch before executing even the first write.
   Kernel-run owner-required shell verification shares the same budget.
   The limit survives resume because consumed actions are counted from
   durable receipts. `src/api/jobs.rs` exposes `maxToolCalls` to clients;
   `src/api/sdk_runs.rs` and the existing agent panel forward it to
   that same endpoint.

**Compatibility:** Missing `maxToolCalls` deserializes to the bounded
default (96) for direct Rust SDK consumers; the prompt API's default scales
with maxSteps. Existing receipts without `postSha256` remain readable.
This implementation intentionally does not invent retroactive hashes for
older effects. Direct code edits that were not made through a trusted
workspace-write tool must still use owner-specified acceptance checks.

## Tests and causal evidence

- `crates/dominion-agent-sdk/tests/artifact_witness.rs`: a scripted
  model claims completion after a write whose hash disagrees with an
  independent verifier. The kernel does **not** complete, gathers new
  evidence, repairs, and only then accepts.
- `crates/dominion-agent-sdk/src/acceptance.rs`: latest-byte witness,
  failed/invalid writes and relative path filtering.
- `src/engine/native_agent.rs`: real file changes after a write are
  detected by the host, including missing files and symlink safeguards.
- `crates/dominion-agent-sdk/tests/kernel_replanning.rs`: a 2-write
  batch with one remaining call is rejected with **zero** side effects;
  mandatory owner verification cannot silently exceed the same cap.
- `crates/dominion-agent-sdk/tests/context_recall.rs`: long-horizon
  history retrieval remains tested with an explicitly ample tool budget.

Exact run results belong to GitHub Actions for the commit being assessed.
A static source path is **not** evidence of benchmark success.
No numeric success-rate lift, real provider spend reduction, or
frontier-model parity is asserted before a same-model same-task experiment.

## 2026 source research and what we changed

- **Google Research VeriHarness** (2026-10),
  [README](https://github.com/google-research/veriharness/blob/main/README.md)
  and [driver](https://github.com/google-research/veriharness/blob/main/harness/driver.py):
  model agreement and model self-report are not environmental evidence.
  Dominion borrows the *principle of falsifying an artifact with independent
  environmental checks*, not the paper's four-model-turn multi-rollout method.
  Deterministic SHA readback adds no extra paid model call to a clean pass.
- **OpenHands** (2026),
  [verification stack](https://www.openhands.dev/blog/20260506-the-verification-stack)
  and [agent SDK source](https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/conversation/conversation.py):
  different failure classes require gates at different layers and a
  persisted backend-owned execution state. Our existing durable Rust
  snapshots remain authoritative; TypeScript only renders their results.
- **Qwen Code** (2026),
  [Goals source](https://github.com/QwenLM/qwen-code/blob/main/docs/users/features/goals.md):
  bounded long-term execution and independent evidence checking must be
  integrated with recorded usage. Dominion's tool cap includes the final
  verifier and fails closed when insufficient budget remains.

## Scope and open gaps

This is **artifact integrity / completion validity**, not a full semantic
judge for whether a patch meets a natural-language requirement. A logically
wrong but internally consistent change can pass a hash test: owner checks,
holdout tests and independent review are still necessary. The new admission
gate is a *tool-call* ceiling; upstream model token metering and wall-time
budgets remain separate. Exhaustive cross-platform and paid-provider
performance measurement still requires isolated repeatable runs.

The base model is fixed throughout; the SDK, Rust engine, GUI, API and RSI
keep their existing identities and no weight training is added.
