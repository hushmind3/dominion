# Sub2: evidence-guided hypothesis probes in the existing Rust adaptation engine
Date: 2026-11-18 | Branch: dominion-sub2

## What changed

This is a **live tool-dispatch change**, not a new SDK, a renamed strategy, or an always-on RSI process.

Old failure path: failed tool -> one model response with hypotheses/interventions -> weighted intervention choice -> dispatch -> record.

New failure path:

1. Existing `attempt_model_adaptation` obtains hypotheses, task-specific intervention candidates and **optional diagnostic probes** in **the same model response**.
2. `adaptation::probes::validate` rejects unsupported or mutating probe tools, malformed predicted outputs and excessive candidates. Legacy outputs without `probes` remain valid.
3. `adaptation::probes::rank` groups the predicted values for every *live* causal hypothesis at a shared result JSON path. Its expected discrimination is the entropy of the predicted outcomes (bits), discounted by bounded model-estimated probe cost. Identical predicted outcomes or incomplete predictions are **not** treated as informative. Duplicate tool+arguments are deduplicated after ranking.
4. At most **two** native, read-only probes execute sequentially via existing `dispatch_tool_call`. Each consumes the same `BudgetLedger` tool-call allowance and produces ordinary `execution_operation` receipts, cancellation, panel policy and transport events. The exact previously failed tool+arguments are excluded.
5. **Only after real successful tool execution**, `program::predicate_verdict` compares actual tool JSON with each model-predicted value. Missing output is `Unknown` (not false). Contradictory hypotheses are excluded; an uninformative/failed probe does not refute any hypothesis. If all hypotheses contradict the observed result, the engine refuses to perform an unsupported intervention.
6. Existing `choose_intervention_with_policy` receives the updated hypothesis set and the actual probe observations as precondition context. The selected intervention still executes through the same approval/effect/no-replay gateway. No arbitrary program execution or autonomous external writes are introduced.
7. Native tool receipts remain in `execution_operation`. A bounded `probeResults` trail containing call IDs, expected information, observed predicate verdicts and refuted hypothesis IDs is stored with the existing adaptation record and exposed to the model continuation. Raw tool contents are not recopied into that trail.

Important correctness repair: previous code re-used refuted hypothesis IDs from **unrelated** adaptation proposals in the same execution. A generated `h1` is not a globally stable semantic identity. This change scopes refutations to the actual current proposal, preventing accidental contamination.

## Scope and failure behavior

- If a provider proposes no probes, existing adaptation behavior is preserved.
- If the proposed probes lack distinguishable predictions, no speculative tool call is made.
- If the user disables a tool or approval/policy denies it, the normal dispatcher records failure; this does **not** refute a causal hypothesis.
- Unknown tool output never refutes a hypothesis.
- If every executed probe was denied, failed, or returned only Unknown verdicts, the engine does not proceed to a speculative state-changing intervention; the existing model continuation must repair the plan.
- If any executed probe cannot distinguish candidates, a different admissible probe may be tried until the shared two-probe cap or existing tool budget is reached.
- If every live hypothesis is refuted, no intervention is dispatched from that bad proposal; the existing model continuation receives an error and may repair.
- Previously successful external effects still use the existing effect barrier; read probes do not override safety/approval.
- No always-on scheduler, extra planner model call, UI menu or checkpoint format migration is required.

## Direct code paths

- `src/engine/adaptation/probes.rs`: validation, conditional expected entropy reduction, semantic replay guard, tri-state result evaluation and unit tests
- `src/engine/adaptation/mod.rs`: module registration
- `src/engine/adaptation_loop.rs`: optional prompt contract, contract validation, persisted `probeResults`, integration regression test
- `src/engine/chat/runtime/program.rs`: actual tool dispatch, bounded evidence collection, hypothesis refutation and intervention selection
- `src/engine/chat/runtime/turn_body.rs`: concise probe evidence in the existing adaptation continuation

## Public code examined; independent design decisions

- Google Research Harness² (October 2026), `harness2/core/recursion.py`: external task-level recursive rollouts and stage validation. We do **not** copy its harness rollout engine; native Dominion can distinguish hypotheses with cheaper, gated read-only probes within an existing failed-turn budget: https://github.com/google-research/harness2/blob/main/harness2/core/recursion.py
- DeepSeek Harness (2026), `packages/core/tool-executor-ptc/src/ptc.ts`: composed tool calls must respect underlying admission, cancellation and audit. We reuse Dominion's existing `dispatch_tool_call` instead of introducing a second TypeScript/Node tool runner: https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/tool-executor-ptc/src/ptc.ts
- Airbnb Agent Harness Optimizer / PRISM, `agent_harness_optimizer/optimizers/prism/loop.py`: candidate evaluation and distinct train/holdout passes. Production diagnostic probes are *not* a replacement for offline comparative harness evaluation: https://github.com/airbnb/agent-harness-optimizer/blob/main/agent_harness_optimizer/optimizers/prism/loop.py

## Targeted tests committed (not executed here)

- Predicted output entropy ranks informative, task-specific probes instead of fixed strategy names
- Actual result contradictions remove wrong hypotheses, missing fields remain Unknown
- Duplicate tool+args retain the best-ranked plan; incomplete predictions cannot earn information
- Mutating, shell and invalid prediction probes are rejected
- Same failed observational tool+arguments cannot be immediately replayed
- Probe evidence switches the selected existing adaptation intervention

**Verification status:** Source changes and tests are committed. No Rust compiler/cargo is present in this session's isolated container, so actual Rust unit test passes and real-model completion/token improvements remain **unverified**. Full release builds/CI were intentionally not attempted. A quantitative comparison must hold task/model/provider/tools/budget constant and measure successful verified completions, failures, total tokens, time and extra probe overhead.

## Not claimed

This does **not** implement isolated candidate code branches, multi-rollout counterfactuals, a full Propose–Probe–Compose optimizer, persistent Bayesian posterior learning, or global optimality. It does implement actual evidence-gathering actions and evidence-dependent intervention selection inside the product runtime.