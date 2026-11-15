# Native RSI evolution — 2026-11-15

## Real entrypoint and intent

After the owner **independently evaluates and explicitly adopts** an initial improvement, Dominion can continue a bounded lineage of 1–4 further generations. The operator provides the generation count, an explicit KRW budget, and optionally an ordered source-file list such as `src/a.rs → src/b.rs → src/a.rs`. The same source objective, independent test command, numeric scoring command and Git repository remain frozen throughout. No model-weight update, LoRA or training process exists in this path.

One request goes through **React** (`web/src/pages/NativeWorkspace.tsx`) or the **TypeScript client** (`sdk/typescript/src/index.ts`) to the same HTTP Rust SDK endpoint (`src/api.rs` → `src/runtime.rs` → `src/improvement/evolution.rs`):

```http
POST /api/improvements/{adopted_trial_uuid}/evolution
Content-Type: application/json

{
  "max_generations": 3,
  "cost_budget_krw": 1000,
  "target_files": ["src/a.rs", "src/b.rs", "src/a.rs"]
}
```

The result is an `EvolutionRun` with a durable UUID and initial `running` state. Observe it with `GET /api/evolutions/{uuid}` or `GET /api/evolutions`. To stop further changes, call `POST /api/evolutions/{uuid}/stop`. The Rust worker persists `stop_requested` immediately and checks it before each generation and before adopting a completed candidate. An already executing model/evaluator call may finish, but no later generation is admitted after the request is observed. The Rust and TypeScript SDKs expose identical operations. The React research lab shows each descendant, planned file order, verified adoption count, status, estimated KRW spent, and stop reason.

## Actual fixed-evaluator lineage

```text
Owner-approved and scored seed A (adopted Git HEAD)
       │  pin accepted candidate, record adopted checkout commit
       ▼
Generation 1: B  ── detached baseline vs candidate
       │            identical tests and numeric score
       ├── no verified gain / error / over budget → STOP, needs_review or failed
       ▼
Git-pinned, independently better B candidate → explicit chain authority adopts B
       │  pass measured feedback into next model prompt
       ▼
Generation 2: A  ── new detached baseline vs candidate (A→B→A)
       │
       ▼
Continue at most 4 generations; persist every candidate ID and adoption count.
```

The native evaluator checks the tracked Git tree **and HEAD** before and after the test and numeric scoring command. A candidate whose tests change source or commit identity is rejected rather than certifying a different program. A scored candidate is stored under a stable `refs/dominion/improvements/<UUID>` Git ref before temporary worktrees are removed, so ordinary Git garbage collection does not erase its lineage. Adoption verifies the candidate commit matches that fixed ref and the checkout matches the evaluated base. After adoption, its new checkout HEAD is saved for the next generation.

`Improvements::repository_active` disallows multiple concurrent experiments on the same repository. Crucially, all improvement evaluations and the entire automatic evolution chain also take the **same native workspace writer lease** as `engine::run` source-edit and acceptance jobs, so one user's normal code agent cannot invalidate an RSI measurement mid-run. Read-only tasks may proceed concurrently.

A descendant inherits the previous generation's logged **actual baseline/candidate scores**, test outcomes and bounded reflection, not self-declared success. The same scoring criterion is retained when the target file changes. This creates a genuine cross-component feedback loop when the user selects `A → B → A`, without claiming universal improvements or self-training. The generation proposer currently emits one candidate per generation; there is no Pareto frontier, multi-proposal tournament or held-out benchmark suite.

## Persistence, budget and failure behavior

- A successful candidate is explicitly adopted only if its tests passed and the numeric score strictly improved against the frozen baseline. No score command means no automatic evolution.
- Each run is saved to `<improvement storage>/evolutions/<uuid>.json` with parent seed ID, target sequence, descendant IDs, completed generation count, measured spend and stop reason. Parent/child experiment records remain separately persisted.
- Startup converts an interrupted `running` evolution to `needs_review`; it does not silently replay a paid model request, a shell command, or a Git adoption. The operator inspects the recorded candidate lineage before a new run.
- `cost_budget_krw` is checked before each generation and after metered model calls. One remote call can exceed the ceiling before usage is reported. A proposed over-budget candidate is **not automatically adopted**. Without accurately configured provider KRW rates, the displayed `spent_krw` is not an invoice or a hard spending guarantee.
- Executing the operator's evaluation scripts requires `DOMINION_ENABLE_SHELL=true` and a separate explicit evolution request. Detached Git worktrees and in-process mutexes provide change separation, **not an OS sandbox or hostile multi-user security boundary**.
- Automatic adoption can still fail because of Git checkout state, disk failure, or exhausted provider quota. In these cases the lineage records `failed` or `needs_review` with a reason; no success is invented.

## Evidence and tests

The source includes a reproducible Rust integration test `bounded_evolution_can_improve_a_then_b_then_a_with_real_git_evaluation`: a fixture model proposes the next number while real Git worktrees, real shell score/test commands, frozen-baseline comparison, adoption, two descendants, file contents and persistence are checked. This verifies that `a.txt` changes to 1 for the seed, then `b.txt` changes to 1, then `a.txt` changes to 2. The restored lineage reports two adopted generations. The model is mocked for deterministic proposals; **Git and evaluator commands are not mocked**.

Additional tests verify interruption handling, explicit stop while the native workspace lease is held (no candidate starts), refusal to auto-adopt candidates that breach the measured KRW limit, evaluator rejection when tracked files or HEAD change, and TypeScript client routing. `scripts/source-contract.test.mjs` checks the Rust SDK/HTTP/React/TypeScript path, and the branch's GitHub Actions workflow runs Rust, Node and frontend checks.

## Comparative engineering basis

- Google DeepMind's [AlphaEvolve minimal public engine](https://github.com/google-deepmind/alphaevolve/blob/main/src/alpha_evolve.py): candidate generation, evaluator, measured registration and feedback separation.
- GEPA's [core engine](https://github.com/gepa-ai/gepa/blob/main/src/gepa/core/engine.py): feedback-backed proposal tracing, explicit acceptance criteria and selection separate from generation.
- Anthropic's [2026 long-running harness analysis](https://www.anthropic.com/research/harness-design-long-running-apps): evaluation and harness behavior treated as design problems in their own right.

These informed the *principles*, not copied external engines. A formal 2026 frontier parity score needs matched models, tasks, commands, costs and independent held-out evaluation.
