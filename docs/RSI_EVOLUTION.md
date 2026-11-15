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
  "target_files": ["src/a.rs", "src/b.rs", "src/a.rs"],
  "candidates_per_generation": 3,
  "validation_command": "node scripts/validation-score.mjs"
}
```

The result is an `EvolutionRun` with a durable UUID and initial `running` state. Observe it with `GET /api/evolutions/{uuid}` or `GET /api/evolutions`. To stop further changes, call `POST /api/evolutions/{uuid}/stop`. The Rust worker persists `stop_requested` immediately and checks it before each generation and before adopting a completed candidate. An already executing model/evaluator call may finish, but no later generation is admitted after the request is observed. The Rust and TypeScript SDKs expose identical operations. The React research lab shows each descendant, planned file order, verified adoption count, status, estimated KRW spent, and stop reason.

## Actual fixed-evaluator lineage

```text
Owner-approved and scored seed A (adopted Git HEAD)
       │  pin accepted candidate, record adopted checkout commit
       ▼
Generation 1: B  ── 3 independently proposed candidates from one Git HEAD
       │            baseline vs each candidate, same tests/primary score
       │            then optional second scorer in fresh Git worktrees
       ├── failed tests / weaker score / failed validation → reject candidate
       ├── no eligible winner / owner stop / exhausted budget → STOP
       ▼
Highest eligible Git-pinned B candidate → chain authority adopts only that B
       │  pass measured feedback into next model prompt
       ▼
Generation 2: A  ── new detached baseline vs candidate (A→B→A)
       │
       ▼
Continue at most 4 generations; persist every candidate ID and adoption count.
```

The native evaluator checks the tracked Git tree **and HEAD** before and after the test and numeric scoring command. A candidate whose tests change source or commit identity is rejected rather than certifying a different program. A scored candidate is stored under a stable `refs/dominion/improvements/<UUID>` Git ref before temporary worktrees are removed, so ordinary Git garbage collection does not erase its lineage. Adoption verifies the candidate commit matches that fixed ref and the checkout matches the evaluated base. After adoption, its new checkout HEAD is saved for the next generation.

`Improvements::repository_active` disallows multiple concurrent experiments on the same repository. Crucially, all improvement evaluations and the entire automatic evolution chain also take the **same native workspace writer lease** as `engine::run` source-edit and acceptance jobs, so one user's normal code agent cannot invalidate an RSI measurement mid-run. Read-only tasks may proceed concurrently.

A descendant inherits the previous generation's logged **actual baseline/candidate scores**, test outcomes and bounded reflection, not self-declared success. The same scoring criterion is retained when the target file changes. This creates a genuine cross-component feedback loop when the user selects `A → B → A`, without claiming universal improvements or self-training.

### Candidate tournament and independent validator

`src/improvement/evolution/tournament.rs` now generates 1–4 **sibling alternatives from the same adopted base HEAD** before any adoption. Rust varies the model-facing search strategy, pins each candidate commit, stores the candidate UUID and primary evaluation score, and runs the optional owner-specified `validation_command` against both the pristine baseline and pinned candidate in **fresh detached worktrees**. That validation command and its outputs are never included in the proposal prompt or the inherited reflection. Validation is an independent *evaluation pass*, not a cryptographically secret or inaccessible dataset: the operator owns the command and files.

Each candidate must pass its original tests, strictly improve the original score, and (when configured) also pass and improve the independent numeric validation score. Rust compares eligible candidates by validation score when present, then training score, then original candidate order for deterministic ties. Exactly one winning candidate is adopted; eligible but lower-ranked candidates become `rejected` in the persisted trial record, as do validation-failing proposals. There is no Pareto frontier or external benchmark success-rate claim yet.

The JSON run record includes `candidate_groups` (all attempted UUIDs grouped by generation), `candidates` (index, training/validation baseline and candidate scores, eligibility, rejection reason), `descendants` (only adopted winners), and sum of **all proposal** metered model costs rather than only the champion's cost. The frozen training and validation **baseline** evidence is reused among siblings only when its recorded Git SHA equals the current base; every contender's own test and validator are executed afresh. A real-process integration fixture checks 8 test-command executions for the adopted seed and 2 sibling candidates (rather than 10 without baseline reuse).

## Meta-RSI method selection: evidence → strategy → candidate → evidence

The production Rust entrypoint `src/improvement/evolution/tournament.rs::tournament_generation` now calls `src/improvement/evolution/strategy.rs::select` **before** generating a sibling cohort. That selector reconstructs its policy from durable `EvolutionRun.candidates` records. Each `CandidateRecord` now holds the attributed `strategy_id`, SHA-256 exact-evaluator scope, weaker same-objective/same-evaluator family scope, training baseline score, candidate score, validation score and paid provider cost. Existing historical records remain readable because new fields are optional/defaulted.

Four bounded Rust-owned source-edit methods are available: minimal change, alternative implementation, structural simplification and alternate algorithm. Their prompts are selected by measured historical outcomes, not by the model editing its own scoring rules. Scores are normalized by each baseline magnitude; successfully independently validated proposals contribute stronger reward than training-only successes. Failed attempts and metered cost count against a method. A deterministic optimistic selection policy deliberately tries untested approaches before exploiting the best-supported method and uses weaker cross-file evidence only if the objective, test and score/holdout commands match. **Different evaluator contracts never share evidence**, even when task descriptions look similar.

All method choices for one candidate group are frozen before the first model call, so a sibling never sees a competitor's holdout result. The model receives only the selected method template and a bounded statistical summary; holdout commands, private datasets and model-written success claims are not accepted as rewards. The selected policy survives process restart by being reconstructed from persisted JSON instead of storing a second mutable model-policy database. Rust remains the exclusive policy owner; TypeScript and React only transport and display its results.

The real-Git integration test now executes **three consecutive accepted-tournament runs**, reopens the persisted Rust engine between the first and second runs, checks that the first two candidates use minimal/alternative, the next two explore simplify/algorithm and a final run returns to the best independently verified strategy. This establishes a closed-loop *method selection* change without model-weight training. There is not yet learned method-template rewriting, true per-example Pareto search, generalization to arbitrary unrelated scoring tasks, or an independently measured frontier benchmark.

Design basis: [GEPA's 2026 candidate-selection guide](https://gepa-ai.github.io/gepa/guides/candidate-selection/) distinguishes exploration from the current-best and Pareto policies; [OpenAI's May 2026 agent improvement loop](https://developers.openai.com/cookbook/examples/agents_sdk/agent_improvement_loop) separates traces, feedback and rerunnable evals. Dominion uses an independent Rust policy specific to verified code changes, not their engines or model training.

## Persistence, budget and failure behavior

- A successful candidate is explicitly adopted only if its tests passed and the numeric score strictly improved against the frozen baseline. No score command means no automatic evolution.
- Each run is saved to `<improvement storage>/evolutions/<uuid>.json` with parent seed ID, frozen scoring command, target sequence, all contender IDs and scores, adopted descendant IDs, completed generation count, measured spend and stop reason. Parent/child experiment records remain separately persisted.
- Startup converts an interrupted `running` evolution to `needs_review`; it does not silently replay a paid model request, a shell command, or a Git adoption. The operator inspects the recorded candidate lineage before a new run.
- `cost_budget_krw` is checked before each generation and after metered model calls. One remote call can exceed the ceiling before usage is reported. A proposed over-budget candidate is **not automatically adopted**. Without accurately configured provider KRW rates, the displayed `spent_krw` is not an invoice or a hard spending guarantee.
- Executing the operator's evaluation scripts requires `DOMINION_ENABLE_SHELL=true` and a separate explicit evolution request. Detached Git worktrees and in-process mutexes provide change separation, **not an OS sandbox or hostile multi-user security boundary**.
- Automatic adoption can still fail because of Git checkout state, disk failure, or exhausted provider quota. In these cases the lineage records `failed` or `needs_review` with a reason; no success is invented.

## Evidence and tests

The source includes a reproducible Rust integration test `bounded_evolution_can_improve_a_then_b_then_a_with_real_git_evaluation`: a fixture model proposes the next number while real Git worktrees, real shell score/test commands, frozen-baseline comparison, adoption, two descendants, file contents and persistence are checked. This verifies that `a.txt` changes to 1 for the seed, then `b.txt` changes to 1, then `a.txt` changes to 2. The restored lineage reports two adopted generations. The model is mocked for deterministic proposals; **Git and evaluator commands are not mocked**.

The `real_git_competition_rejects_high_training_score_that_fails_independent_validation` test runs 2 competing model-generated candidates on a real repository: training scores 2 vs 26, but the higher scorer violates the independent validation command. Only the validated candidate is adopted, while the loser is recorded as rejected and remains unadoptable through the public trial endpoint. This uses actual Git commands/worktrees and actual score commands, plus a deterministic mock model.

Additional tests verify interruption handling, explicit stop while the native workspace lease is held (no candidate starts), refusal to auto-adopt candidates that breach the measured KRW limit, evaluator rejection when tracked files or HEAD change, and TypeScript client routing. `scripts/source-contract.test.mjs` checks the Rust SDK/HTTP/React/TypeScript path, and the branch's GitHub Actions workflow runs Rust, Node and frontend checks.

## Comparative engineering basis

- Google DeepMind's [AlphaEvolve minimal public engine](https://github.com/google-deepmind/alphaevolve/blob/main/src/alpha_evolve.py): candidate generation, evaluator, measured registration and feedback separation.
- GEPA's [core engine](https://github.com/gepa-ai/gepa/blob/main/src/gepa/core/engine.py) and [candidate selection strategies](https://gepa-ai.github.io/gepa/guides/candidate-selection/): feedback-backed proposal tracing, acceptance criteria, full validation and candidate-pool selection separated from generation.
- Anthropic's [2026 long-running harness analysis](https://www.anthropic.com/research/harness-design-long-running-apps): evaluation and harness behavior treated as design problems in their own right.

These informed the *principles*, not copied external engines. A formal 2026 frontier parity score needs matched models, tasks, commands, costs and independent held-out evaluation.
