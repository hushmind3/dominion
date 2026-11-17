# Sub2 — bounded per-task strategy trials in the existing Rust autonomous cycle
Date: 2026-11-18
Branch: `dominion-sub2`

## Compared public implementations (source inspected)
- DeepSeek Harness PTC (2026-06): a model can express chained tool operations as one bounded program; composition does not remove tool authorization or the host execution policy. https://github.com/deepseek-ai/deepseek-harness/blob/master/.agents/notes/implemented/feature/2026-06-15-ptc.md
- Google Research Harness² (2026-10): compare task-level harness changes using rollout evidence and fall back after invalid proposals. Its recursion driver optimizes a harness using separate staged rollouts; this is **not** the same as selecting a tactic within a single owner job. https://github.com/google-research/harness2/blob/main/harness2/core/recursion.py
- Airbnb Agent Harness Optimizer / PRISM (2026): compare prompt and middleware candidates with evaluation feedback, including held-out cases; a benchmark optimizer is not appropriate as an always-on self-modifying production loop. https://github.com/airbnb/agent-harness-optimizer

## Actual product change
The existing Rust `Cycle` is the sole attempt owner; `src/engine/autonomy.rs` still calls `run_quiet_outcome` once per authorized attempt. The implementation adds **no** perpetual RSI work, background campaign, extra process, or second tool executor.

Before a turn, the host derives current acceptance gaps from its authoritative verifier, and the kernel supplies a bounded three-approach slate:
1. **direct**: fix the remaining verifiable requirement;
2. **diagnose**: gather minimal, causally useful evidence before a fix;
3. **decompose**: split remaining requirements into independently checkable steps.

The first attempt keeps the existing direct execution contract. Before subsequent attempts the kernel combines the *current failure category* with local evidence from completed attempts. It ranks approaches using newly closed/opened acceptance gaps, genuinely novel successful tool evidence, recorded error state and measured per-turn token delta, then inserts only the chosen tactic in the existing retry instruction. No model is paid to score a model's own unverified explanation.

After the turn, the host records the observed attempt's `strategyTrial` in the existing autonomous step receipt and `kernelCycle.strategyTrials` in the normal job checkpoint. A changed owner goal clears tactic credit, preserving the global attempt budget. An owner-steering race does not credit the old goal's experiment.

Existing protections remain authoritative: uncertain remote side effects require review; repeated no-progress attempts stop; explicit approval, cancellation, maximum attempts, budget, owner steering and verifier freshness are unchanged. No strategy comparison can authorize new tools or replay effects.

## Scope / measurement
These are **sequential attempts on evolving task state**, not independent parallel counterfactual rollouts. The candidate scores are local heuristics and do not establish causal superiority. Code-level regression tests cover direct→diagnose switching after failed verification, retaining a progressing approach, score inputs, goal replacement and bounded attempts.

Unverified in this run: Rust compilation/tests (the current editing environment has no cargo/rustc), release builds, full CI, cross-platform runs, and controlled real-model measurements. The next benchmark should use matched goals, provider/model/tools/budget with enabled vs. previous kernel, comparing verified completions, prompt and completion tokens, wall time and evidence-stall recoveries.
