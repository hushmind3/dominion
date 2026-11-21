# Dominion Sub2 — execution wiring and source audit (2026-11-21)

## What was inspected

Scope: the **dominion-sub2** tree (444 tracked blobs in the GitHub tree at audit start), plus read-only tree comparisons with `main`, `dominion-sub3`, and `dominion-sub4`. No other branch was edited.

The source-wide automated gate, `scripts/sub2-source-audit.mjs`, recursively reads executable source across `src/`, `crates/`, `web/src/`, `sdk/`, cloud, scripts, services, and desktop adapters. It detects identical executable files, high-confidence copied logic, duplicate Axum route shapes, and absent Team UI routes/handlers. **This is not a claim that every file was manually code-reviewed.** Manual path-level review prioritized the following end-to-end execution chain and adjacent contracts.

## Native request → actual observed effect

1. `web/src/App.tsx` mounts Chat and existing Activity/Settings pages; `web/src/pages/Chat.tsx` and `web/src/pages/Operations.tsx` mount `TeamRunStrip` / `TeamActivity` without adding navigation.
2. `web/src/components/TeamActivity.tsx` creates a Lead job, shows real job/team/task statuses, selects eligible remote agents, and previews remote file diffs. `web/src/api.ts` handles authenticated fetch. `web/src/refreshEvents.ts` invalidates only relevant consumers.
3. `src/api/mod.rs` mounts `src/api/surfaces/automation_routes.rs` through the existing guarded Axum router. `src/api/services/automation/team_controls.rs` handles start / stop / resume / preview / apply / reject. `src/api/services/automation/executions.rs` exposes a bounded, source-redacted status projection.
4. A start request queues a real `prompt` job using `src/engine/services/automation/jobs.rs`. The dispatcher/JobContext own attempts, epochs, cancellation, finalization, and persistence. `src/engine/services/automation/autonomy.rs` invokes the existing chat turn; `crates/dominion-turn-core/src/agent_loop.rs` / `driver.rs` own model/tool admission.
5. Model `team_workers` / `team_execute` calls are implemented in `src/engine/services/tools/tools_team.rs`, using `crates/dominion-agent-loop/src/task_team.rs` for DAG waves. SQLite CAS in `src/db.rs` protects team member launch claims from parent cancellation.
6. Local children use `delegate` → the same Rust Job executor, keeping their own immutable job/session receipts. Remote children use `src/remote.rs` (optional authenticated control plane) and `src/remote_worker.rs` (outbound-only native Rust Worker). Late/stale/cancelled results are fenced by worker epoch, lease and cancellation guards.
7. `src/events.rs` publishes durable record/run-state events; TypeScript and React receive the same typed states. The remote event payload strips source-file contents.
8. `src/engine/services/tools/tools_remote_artifacts.rs` constructs a patch against frozen workspace SHA-256 baselines, checks the reviewed digest, acquires the existing native writer lock and SQLite lease from `tools.rs`, and uses `tools_patch.rs` for the actual effect. An atomic DB claim rejects a cancelled Team before mutation. Post-write SHA-256 checks settle the receipt.

## Source findings and fixes

| Finding from actual source path | Impact | Fix / gate |
|---|---|---|
| `tools_team.rs`: UI-selected remote device was mandatory for **all** workers, contradicting remote research + local Writer | Mixed remote/local DAG could not launch | `validate_ui_worker_route` accepts selected remote and local Workers, rejects other remote devices, and ensures at least one node uses selected remote; Rust negative tests |
| `TeamActivity.tsx` / `team_controls.rs`: remote-only Agent IDs were absent from UI and rejected by Lead API unless separately installed locally | Native Worker agents registered only on another PC could not be selected | Advertised remote-only IDs now appear in the existing UI; backend admits only online remote-advertised IDs, with remote-device selection required |
| `jobs.rs`: a Lead could return ordinary text without calling `team_execute` | False completed team task in Activity | `verify_team_ui_completion` demands a persisted `managed_team.uiOwnerJobId` matching the real checkpointed job/session; no native DAG yields `needs_review` |
| `team_controls.rs`: a previously stopped Lead had a persisted cancellation tombstone, making direct resume fail | UI resume button could not progress valid, still-running Team DAG | Resume takes the Lead session slot, rejects active turns, then re-arms only the observed cancellation generation |
| `tools_team.rs`: cancelled parent could still appear running in tool status | Contradictory native vs UI team state | `summary` returns `cancelled` with `evidenceVerified=false` |
| `tools_remote_artifacts.rs` / `db.rs`: file effect must not cross a concurrent Team cancellation | Risk of accepting effects after stop | Transactional parent Team check and frozen patch claim; cancelled-team regression test |
| Route/UI compatibility had only source-level adjacency tests | A button could point at a missing native endpoint | Source-wide router/UI gate and live Axum HTTP handler tests (no model required) |

## Evidence gates

- `.github/workflows/ci.yml` runs the Rust Job, native Team, remote Coordinator, remote artifacts, HTTP Team API, Agent Core, chat/room, and SQLite regression suites; TypeScript typecheck; all web Node tests; source-wide `sub2-source-audit.mjs`, architecture/source boundary contracts; and the public TypeScript SDK tests.
- `src/api/services/automation/team_controls.rs` exercises actual Axum listener + requests for `/api/activity/teams` and `/api/activity/teams/start`, invalid IDs and missing approval.
- `src/engine/services/tools/tools_team.rs` tests local Writer after remote read-only predecessors, foreign Worker rejection, bounded parallelism, native verification and canceled/pending file barriers.
- `src/engine/services/automation/jobs.rs` tests false Team success and immutable job attempt/cancel fencing.
- `src/engine/services/tools/tools_remote_artifacts.rs` tests source/hash preflight, once-only application and refusal for a cancelled Team.
- The source gate checks route presence and mounting in both existing React surfaces.
- CI results are per commit and **must be inspected at the specific final HEAD**. Earlier green CI is not proof for newer commits.

## Limits, not treated as completed

1. **No local browser/desktop preview, production Vite build or deployment** was executed during this web/GitHub-only audit (the development instructions deliberately skip release builds).
2. **No physically connected second PC** was available to demonstrate HTTPS/private network, credentials, offline recovery, and cross-host file hashing under real latency.
3. **No live model-backed Team task** was executed in this audit. Scripted/rust unit/API contract checks cannot assert autonomous task success, model choice quality, cost or multi-hour behavior.
4. **Updated 2026-11-21:** `team_execute(wait=false)` is now resumed by the existing `jobs.rs` dispatcher on completed local/remote receipts and on its bounded recovery tick. It re-enters the same session-fenced native tool gateway and committed DAG; no new model loop is introduced. Duplicate wake, cancellation, restarts and Writer effects still require integration verification on the final commit. A pending remote artifact never becomes a local file change without explicit approval.
5. Direct operator review is a purpose-built, explicit HTTP endpoint; the normal chat-tool approval channel is not implicitly bypassed or treated as authorization to auto-adopt remote files.

**Bottom line:** The audited Team UI/API contracts have concrete Rust and frontend test gates and the described source defects were fixed. This does **not** prove a production deployment, the quality of an actual model-selected DAG, or unattended cross-wave completion.


## Latest sub2-only scheduler change

- The existing Job dispatcher now observes terminal child Job/remote task events and resumes only `managed_team` records with live `running` status, an owner session and an immutable saved DAG.
- The original Lead session lock gives precedence to an active user turn; cancellation tombstones are never automatically cleared. Worker/member claims and workspace leases are still enforced by the original executor.
- A 10-second fallback tick also checks previously committed teams after process restarts. Team status remains read-only; no new API, menu, second scheduler, or detached model loop was introduced.
- Regression tests cover wake event filtering, cancelled/finalized team exclusion and the immutable Team readiness guard. These are unit-level protections, not a substitute for crash/network/physical-device tests.
