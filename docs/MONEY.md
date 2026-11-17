# DOMINION Money — continuously collected market intelligence

Money originated in the experimental `money` branch and is selectively integrated into `main`. It uses the existing Rust harness and durable jobs without replacing the execution kernel.

## Detachable capability boundary

Money is **one tool in Dominion**, not a second engine, independent backend or long-lived separate database. The core owns the agent runtime, scheduled job workers, approval/cancellation/recovery, HTTP retrieval, providers and shared SQLite connection. Money contributes only:

- `src/api/money_routes.rs` — one optional HTTP route bundle registered by the common API router.
- `src/engine/money_tool.rs` — one scheduler hook plus one job-kind dispatch adapter registered by the common job worker.
- `src/db/money.rs` — Money-owned schema/index declarations and reads/writes on the same shared `records` connection. Stored rows retain their `money_*` kind and can remain archived after detaching the tool.
- `web/src/pages/Money.tsx` and the `money` navigation entry — removable presentation only.

No Money-specific polling loop, database file, custom LLM runner, or parallel durable queue is required. During free-model rotation, the selected Money agent ID persists while the common provider router honors the explicitly chosen provider/model for each round. This preserves the persona and agent-specific instructions without creating another execution engine. Removing the capability means unregistering its route/worker hooks and optional navigation and disabling its schema extension; core records and all unrelated workflows remain intact. This is a code boundary, **not** a runtime plugin installation/uninstallation API.


## Current user experience — one button, three saved drafts (2026-11-17)

Money has a **single primary action**: `돈벌이 찾기`. Pressing it calls `POST /api/money/find` to select up to three opportunities from the already-collected, source-labelled SQLite corpus, immediately store their business drafts, and show each draft's potential customer, economic mechanism, implementation outline, first paid validation step, and source links. There is no form, model selector, separate draft-approval click, news dashboard or analytics submenu on the Money home. The prior six Money UI-only components have been removed.

The Rust adapter (`src/api/money_find.rs`) immediately selects source-backed hypotheses from independent market-tension records. Where no pairs are available it generates explicitly lower-confidence drafts from observed procurement, cost increases, demand, shortages or regulation events. Each guess includes a causal bridge and a counterfactual failure condition, not just joined headlines. Unknown prices, verified orders and customer commitments remain unfilled. The original records and generated drafts are durable and deduplicated.

**Data acquisition, indexed retrieval and the first draft use zero model tokens. Business invention uses one AI inference job when an enabled provider is available.** The same button launches an existing native `money_discover` job in `infer` mode with a configurable token budget (24,000 tokens by default for the one-click AI action). The prompt includes only a bounded, provenance-labelled DB sample, existing leads and unresolved cross-market contradictions. Search and execution tools are disabled; the model's task is abductive reasoning, not repeating the web investigation. It must propose three nontrivial causal business hypotheses, separate observed evidence from speculative bridges, describe the buyer and first paid transaction, test counterfactuals, and avoid fabricated economic amounts.

During model inference, the existing initial drafts stay visible and the button cannot submit a second costly job. The UI polls the durable job status and replaces the first shortlist with saved model-created opportunities on completion. Native session metadata reveals the actual routed model. Failed or absent models leave the DB-only drafts usable, with a visible error. Existing reports, autopilot controls, old candidate decisions, and model connectors stay intact, while the Money home retains a single primary action.

## Evidence folding and low-token discovery

Money runs four bounded layers:

1. Source archive (money_signal): RSS, GitHub/Hugging Face metrics, and limited website excerpts, collected without model tokens.
2. Incremental events (money_event): deterministic source-to-topic/change/region/entity/date extraction, stored with source ID and short quotation. money_source_fold records the exact source revision processed; retry is idempotent.
3. Evidence pairing (money_asymmetry): SQLite indexed entity+region+date join creates pending two-source research questions. The engine never infers that a product is missing from a market just because no source was collected.
4. Model review (money_discover): at most 8 pending pairs per job plus bounded context. Both stored source IDs, URLs, and relationship axis must match before an asymmetry-derived idea may persist. Reviewed pairs transition to evaluated only in the same transaction as a successful report.

Other indexes cover Korean bigram retrieval, dated market metrics, recorded research outcomes and active job statuses. The whole source archive is not sent to the model.

Research recall now runs three independent bounded searches (goal, market/skills, distribution/preferences) through the existing FTS5+Korean-bigram indexes. A zero-token RRF fusion combines them with a weak recent-record channel, then character-trigram redundancy and publisher-diversity selection cap the prompt at 22 provenance-labelled facts. Cross-campaign claims remain explicitly unverified, and no embedding, graph-wide model summary, new table or paid API call is introduced.

**Money excludes video/audio production and autonomous self-modifying RSI.** Its autonomous cycle discovers markets, evaluates hypotheses and learns from reported outcomes, never edits the app's own source code. Source mentions, market counters, model profit estimates and actual received customer payments are distinct forms of evidence.

## Architecture and entry points

| Function | Module / entry |
| --- | --- |
| Market ingestion and RSS/Atom parsing | src/engine/money_sources.rs |
| Safe public HTTP retrieval and existing search tool | src/engine/tools.rs |
| Economic invention operators | src/engine/money_strategy.rs |
| Bounded RRF and cross-publisher context retrieval | src/engine/money_retrieval.rs |
| Agent inference and evidence collection | src/engine/money.rs |
| Durable scheduler, worker queue, restart recovery | src/engine/jobs.rs |
| Atomic campaign/job and result transactions | src/db.rs |
| HTTP endpoints | src/api/money.rs, src/api/money_find.rs, src/api/mod.rs |
| One-button opportunity home | web/src/pages/Money.tsx |
| User-facing opportunity lab and expense ledger | web/src/pages/Money.tsx |

Data kinds in SQLite: money_signal (source feed title, page excerpt, original URL, publication time if provided and capture timestamp), money_fact (lexical indexed knowledge and provenance), money_candidate (economic invention), money_experiment (user-reported test and payments), money_report (durable agent artifact), money_ingest_report (crawler observation), money_campaign (workspace and equipment investment), money_autopilot (run generation, daily quotas and schedule).

Source observations and model claims have explicit DIFFERENT verification labels. Neither a news headline nor model-generated profit is an audited sales outcome. Cross-market hypotheses also require distinct underlying publisher hosts (not merely distinct RSS feed labels), and the candidate shortlist limits repeated reuse of any single supply or buyer article. Big-data read paths use bounded SQL and lexical indexing (FTS5 plus Korean bigrams), not replay of the entire corpus through the model.

All Money job claims update campaign and job together in a SQLite transaction, with generation-specific run IDs and CAS checks. All candidates, source facts and final report commit atomically; replay never overwrites manual changes. The existing job harness handles runtime cancellation and recovery. Jobs interrupted mid-inference may still require user review before repeating external actions.

## API

- GET /api/money : recent signals, corpus stats, last ingest, recent business opportunities, aggregated lifetime reported net results, active policies
- POST /api/money/campaigns : create project, hardwareInvestmentWon optional
- GET + POST /api/money/campaigns/:id : inspect project and save hardware cost baseline
- POST /api/money/campaigns/:id/run : research once in signals, discover, cross, mutate, challenge mode
- GET + POST /api/money/campaigns/:id/autopilot : inspect, enable, pause, resume; continuous=true enables operation without a daily reactivation step, maxRounds is per UTC day
- GET + POST /api/money/candidates/:id : inspect or edit economics, status
- POST /api/money/candidates/:id/experiments : record results and real reported expenses

## Validation

Rust regression tests cover RSS and Atom parsing; stable source keys and deduplication; missing-cost safe unit economics; generation-aware queuing under two requests; failed ingestion and idempotent replay; immutable candidate updates. GitHub Actions covers macOS and Windows: web typecheck/build and Rust release workspace tests.

Not yet measured in real-world use: 27B 24-hour throughput, actual first customers/revenue conversion, causal improvement versus prompt-only research, superiority to Google Deep Research, 100% RSS source availability, or actual payment-platform reconciliation. API provider spending remains subject to its billing policy.

## Relevant 2026 architecture references

- Agent Zero Memory: provenance-aware hybrid retrieval (Aug 2026): https://arxiv.org/abs/2608.29606
- Agent Memory: cost and quality of stateful agent memory (Jun 2026): https://arxiv.org/abs/2606.06448
- Evidence tracing and execution provenance (Jun 2026): https://arxiv.org/abs/2606.04990

The implementation adapts general design principles, not claimed benchmark results.

## Core retirement and dispatcher sync (2026-11-17)

Money retains its independent source-to-evidence-to-paid-buyer hypothesis engine, while importing the post-retirement main runtime, cached SDK run views, native journal improvements, and event-driven durable job dispatcher. The Money collectors and opt-in autopilot are now scheduled every 30 seconds, with UTC slot IDs and job-state gating to avoid duplicate collection or simultaneous model runs. Old RSI/media job records are preserved but never auto-replayed; obsolete source, scripts, visual render dependency and historical experiment artifacts are removed. The main execution engine is unchanged. End-to-end source ingestion and 27B inference throughput remain subject to real deployment measurements.

## Buyer-document evidence matching (2026-11-17)

Candidate `buyerEvidence` entries now preserve the exact quoted buyer-language fragment and the cited URL. Money performs a bounded indexed lookup against existing `money_signal` sources and only awards purchase-evidence research priority when a stored fetched page includes the literal buyer quote. An unarchived URL, invented quotation, headline-only record, or unsupported type remains unverified and receives no such boost. These matches are explicitly *source-text buyer-intent signals*, never actual paid orders. The indexed URL lookup adds zero LLM calls and caches distinct URLs within each discovery job; past rows remain readable and conservatively unverified. The Hotlist displays this distinction. Related design choices were compared with 2026 GPT Researcher source-citation and context-filter changes (https://github.com/assafelovic/gpt-researcher/releases), TED's open procurement search (https://docs.ted.europa.eu/api/latest/search.html), and SAM.gov's API-key-limited opportunities API (https://open.gsa.gov/api/get-opportunities-public-api/). Rust executable and model-backed evaluations remain pending.

Buyer links not already present in the archive can now be retrieved via the existing SSRF-safe `http_fetch` implementation: up to 4 distinct public HTTPS GETs per completed research run, before buyer quote grading. Readable page excerpts are stored as `money_signal` with a source identity; fetch attempts are deduplicated, failed URLs have a 24-hour cooldown, and both retrieval and failure history remain restart-safe in SQLite. This still cannot prove a transaction or a buyer's binding intent. External checkout, outreach and code changes are never triggered by research.

### Unrestricted discovery unless explicitly constrained

Campaign capital, monthly profit goals, hardware investment, available hours and geographic/preferences fields are now optional, stored as `null` when omitted. Quick-start no longer silently assigns 0 won capital, 1,000,000 won monthly profit, 10 hours/week or any country/sector preference. Scoring does not enforce a zero-capital constraint when no budget was provided. Legacy campaigns with explicit numbers preserve their existing restrictions. The Money detail view now exposes the buyer's claimed quote and archived-source match status.

### Multilingual buyer-led signal exploration

Money's existing scheduled RSS collector now rotates through additional Korean customer problems, Chinese procurement intentions and supplier costs, Japanese tenders, English-language buyer requests and HN Ask/Show community posts without increasing the per-cycle 3-feed/4-page and zero-model-token limits. Source metadata and content status remain unverified observations. The deterministic classifier adds a `buyer_request` lexical signal, which the cross-market tension engine can pair with a separate source's cost reduction or new tool release. The resulting `buyer_supply_spread` and `new_tool_to_buyer` records are investigation questions only, not contracts or sales. The automatic mechanism does not restrict discovery to procurement, any country, or a named industry.

### User-facing opportunity briefs and approval boundary

The engine stores up to eight short `opportunityBriefs` per money report using the existing candidate data: concrete discovery, why the buyer would pay, proposed MVP implementation, and the first paid-validation step. The Hotlist and candidate detail view show these separately without a second model call. Each record explicitly retains `executionPolicy=report_only_user_approval_before_business_implementation`; searching and drafting evidence do not authorize an actual product launch, seller outreach, a contract or expenditure.

Previously archived headline-only buyer URLs are not treated as complete. They are selectively fetched and enriched in place using the stable URL hash, preserving the original captured time, publisher and existing FTS-backed fact, without duplicate corpus rows. A database regression test covers this behavior.

## Evidence-based 24-hour Money operating scorecard

Money now records a single scheduler-process heartbeat per five-minute UTC bucket and exposes a read-only 24-hour SQLite scorecard in `GET /api/money` as `operationalScorecard`. It aggregates terminal/failed/reviewed jobs, RSS and catalog ingestion, newly archived sources, reports/parse errors, candidate leads, cited buyer quote matches, buyer page fetches, and user-reported finished experiment revenue/cost/net. Query plans use existing indexed `kind,updated_at` and `kind,created_at` lookups; they do not read the entire corpus into Rust and require no model tokens. The Money UI displays these counters, explicitly separating process heartbeat samples from **verified 24-hour model uptime**, and user-reported outcomes from audited paid conversions. No synthetic usage numbers are filled in. This is operational instrumentation for subsequent real 27B soak tests, not a claim that a soak test ran or CI passed.

## Final retired media / autonomous experiment daemon cleanup (2026-11-17)

The Money production bootstrap no longer starts the isolated `DOMINION_EXPERIMENT_SCHEDULER` loop; `src/engine/scheduler.rs` and its module registration are deleted, not merely disabled by default. The obsolete 82 `vid.*` strings and the video page eyebrow are removed from *each* locale. The `jobk.video` translation is intentionally retained as a read-only compatibility label for preserved historical job records. Dedicated video/video-upload worker kinds remain quarantined into `needs_review`; no native rendering, ComfyUI, FFmpeg or publishing implementation is in the active Money engine. The legacy API experiment queue and `dominion-benchmark` remain **explicit manual benchmark infrastructure**, not an autonomously scheduled RSI runner. Existing test cases expecting a daemon to self-spawn are converted to negative E2E regression checks; code-graph checks now reject reintroducing the daemon, video API, video engine or generation locale keys. Rust tests and platform CI still require actual execution; static source and branch commits alone are not CI proof.

The historical experiment claim, completion, release and runner-release HTTP handlers now explicitly require `DOMINION_BENCHMARK=1`; normal production requests are not allowed to take old experiment requests or apply benchmark promotion results. Historical experiment records remain queryable through the existing read-only endpoints. This is independent of Money's campaign experiments, which record user-reported commercial outcomes and remain operational.

The frontend's obsolete ComfyUI/video-only API error translations were also removed after the live error/route paths were checked; the more general workflow and job error translations remain available. Historical handoff reports are labelled as archived to avoid mistaking removed FFmpeg/YouTube UI for active functionality.

## Indexed execution receipts (2026-11-17)

The Money branch now queries execution evidence by run, session or resource, and checks unknown outcomes with indexed SQL EXISTS rather than reading years of unrelated tool records. SDK model-call accounting can sum per session in SQLite. Errors fail closed in owner evidence and budget checks. The persistent record format remains unchanged. An isolated in-memory SQLite regression test covers scope and budget unknowns; runtime CPU/latency savings have not yet been benchmarked.

## Targeted runtime queries and GitHub grant refresh (2026-11-17)

The Money branch preserves the main native execution core and SQLite store while switching its chat execution hot paths from `all("execution")` / `all("execution_operation")` materialization to existing session, resource and run indexed readers. An additional task-scoped indexed read supports resumable program checkpoints. The execution receipt/effect reconciliation logic, model-call budget, and unknown-outcome handling remain unchanged; query failures now propagate rather than silently becoming empty histories. A test verifies task isolation alongside existing receipt/budget assertions. The built-in GitHub tool now asks the OAuth vault for a fresh (or still-valid) GitHub token before invoking the GitHub REST API. It does not send GitHub credentials into unrelated MCP servers. This change is source-level only: Rust compile, provider reauthorization, and latency improvement still need measured validation.

## Money UI event-scoped refresh (2026-11-17)

The shared SSE listener now coalesces the source `record.kind` across its existing 100ms window, sends targeted domain invalidation hints, and `useRemote` ignores changes unrelated to known list endpoints. Unknown kinds and explicit manual refresh retain the conservative global path. The Money page filters SSE invalidation to `money_*` and relevant jobs while reducing its entire `/api/money` fallback poll from every eight seconds to once per minute. Agent/provider lists still load normally when mounted, and Money's dedicated active-job polling remains untouched. This reduces unnecessary HTTP and JSON parsing in long-running workloads; browser request counts and SSE disconnection behavior still require actual measurement.

## Connection-scoped MCP HTTP reuse

HTTP MCP discovery and tool calls now reuse a bounded, initialized, per-instance/workspace/server/credential session under a per-connection asynchronous mutex. Idle entries expire after 120 seconds, at most 32 slot records are retained, and configuration invalidation clears both tool and HTTP session caches. An updated scoped bearer grant creates a distinct session without sharing service tokens across unrelated MCP servers. On a failed request, the session is dropped **without retrying the tool call**, since a remote write may have taken effect before the connection error. stdio remains per-invocation so unused subprocesses do not stay alive and cancellation retains the existing kill-on-drop guarantee. Dedicated regression tests cover key reuse, server/account separation and invalidation. Actual MCP provider compatibility, concurrency and latency metrics still need runtime measurement.

## Build provenance without Git-ref invalidation, and indexed status

Money's build script no longer declares `.git/HEAD` or the current branch ref as a Cargo recompilation dependency. It still stamps the compiled binary with source commit, branch and dirty state when the Rust sources are built. Controlled CI/release builds may supply immutable `DOMINION_SOURCE_COMMIT`, `DOMINION_SOURCE_BRANCH` and `DOMINION_SOURCE_DIRTY` overrides; a documentation-only commit does not automatically alter an already-built binary's source identity. In the 12-second product status polling path, `active_job_count()` uses the existing SQLite status index and returns a scalar without deserializing years of job records. Database query errors are exposed as `jobsQueryError` rather than silently represented as a valid count. Regression tests cover queued, running and completed/failed/reviewed job separation.

## Connected GitHub work tools (2026-11-17)

Money now uses a separate Rust \`engine::github\` adapter (rather than growing \`engine::tools\`) for four typed tools: \`github_repositories\` (connected-account repository discovery), \`github_file_read\` (source text and Git blob SHA), \`github_file_update\` (explicit branch and observed SHA, commit and remote read-back), and \`github_file_replace\` (exact, one-occurrence block substitution without repeating the entire file in model output). The adapter uses \`oauth::fresh_access_token\` and the shared provider HTTP client, never sends the OAuth bearer grant through arbitrary MCP servers, and does not return secrets. Write tools stay subject to the existing Rust harness approval/effect-journal rules; isolated child sessions cannot use the new external tools. A stale SHA, duplicate/unmatched source block, missing branch, protected branch, or missing repository grant fails rather than producing an unverified success. A failed remote PUT is not retried, because an ambiguous remote write may already have committed. Source contract and tests follow the newer main-branch typed GitHub workflow and the 2026 GitHub REST contents API. This is code integration only: actual OAuth-to-repository write permissions, web UI flow and end-to-end remote verification must still be exercised with a connected account.

Typed GitHub read/update/replace now share the same `github_file:{owner}/{repo}@{branch}/{path}` effect resource, with an unresolved-effect retry gate. The adapter records a verified receipt only when the GitHub PUT is followed by successful branch read-back and both file SHA and exact bytes agree. Network errors, malformed write responses, and post-commit read-back failures return `outcomeUnknown=true` rather than an ordinary failure; the harness does not silently replay the mutation. A regular read of the same file can inspect that uncertain effect. Tests assert resource identity, retry blocking and verified/unverified remote receipts. Empty files can be written intentionally. Runtime GitHub OAuth integration remains to be exercised under a connected account.

## Rust chat runtime source boundaries (2026-11-17)

The Money chat runtime was divided without replacing the native `Engine::drive`: `runtime.rs` owns turn orchestration and finalization, `runtime/tool_catalog.rs` owns tool selection and argument normalization, `runtime/program_exec.rs` owns durable DAG / deterministic replacement execution, and `runtime/tool_gateway.rs` owns approval-mediated tool dispatch, effect receipts and evidence writes. Existing test modules are relocated to `runtime/outcome_tests.rs` and `runtime/harness_tool_selection_tests.rs`, keeping the test code. Submodules share the runtime contract with `use super::*` and explicitly export entry points `pub(super)`; relative budget paths were adjusted to the deeper module scope. File size no longer makes the root runtime approach 5,000 lines. This is a structural refactor, not a new dispatch loop, and does not claim runtime measurements or passing CI.

## Source growth guard (2026-11-17)

The previously 7,151-line `tests/e2e.rs` is now a ~222-line shared integration fixture and seven `tests/e2e/suite_0*.rs` files of 521–1,097 lines each. Those integration suites use Rust `include!` to retain crate-level test symbol visibility and exactly preserve existing test bodies. Production chat runtime uses normal Rust `mod` boundaries. The new offline, dependency-free `node scripts/check-source-size.mjs` gate scans Rust/TS/TSX/JS/MJS files in source, integration tests and scripts, failing above **3,500 lines per file**. It executes before npm install and release tests in the GitHub CI matrix and can run separately without compiling Rust. The guard checks maintainability, not measured CPU/RAM consumption.

## Indexed cancellation and bounded history reads (2026-11-17)

Session cancellation now reads only the live queued/running jobs associated with that session, preserving all five historical session ID paths, instead of deserializing every completed/archived job. Nested delegated-session cancellation uses a parent-scoped SQLite partial index instead of scanning all sessions. Durable cancellation epochs are written first, and SQL errors emit separate audit events rather than being silently interpreted as successful empty scans. This directly reduces long-lived database parsing cost without deleting any retained jobs or weakening interruption/restart behavior. Regression tests cover all session ID shapes, archived jobs and unrelated child sessions. No latency benchmark has been run yet.

The source-size gate also covers all Rust workspace crates (`crates/`) rather than only the top-level `src/` directory; no independent crate may accumulate a hidden >3,500-line module.

## Evidence-age and shortlist consistency (2026-11-17)

The one-button Rust endpoint now sorts candidate research drafts with a **deterministic opportunity-discovery priority**, not a claimed probability of profit. The priority considers paired independent source material, research priority, a buyer-request tag, fetched article text and the oldest cited publication date. Ingestion/capture timestamps **never** substitute for article publication dates: old notices re-crawled today do not become current tenders. Missing dates remain unknown rather than being fabricated. Previously persisted candidate records and manual user decisions remain immutable.

The bounded current shortlist remembers at most 128 shown IDs in the existing `money_find_report` record. New discoveries are surfaced ahead of already-shown drafts; rejected candidate records are excluded before selection; when fewer than three unshown results exist, older results can rotate back in without duplicate cards. Candidate sources are rechecked at draft construction, including original pair URL consistency and independent publisher host checks. The selection path uses existing indexed source and candidate reads, with **no extra model requests, schema migration, or new background loop**.

A process-scoped async lock serializes the report/selection/inference-enqueue sequence for concurrent clicks, while the existing SQLite campaign claim remains the durable job-level guard. The existing agent-binding integration test now issues overlapping requests and checks that only one `money_discover` job is queued. Additional pure regression tests cover dated-source prioritization, unknown publication dates, repeat avoidance and rejection handling. This is source-level validation only: no Rust CI, multi-process load test, 27B soak test, customer-payment verification or measured improvement is claimed.

## Shared-kernel checkpoint and provider continuity (2026-11-17)

The native worker rejects late checkpoints on terminal/cancelled jobs within the same SQLite transaction, removing a status race without a second DB read. Money's background free-model rotation retains `agentId`; explicit provider/model values override the agent's original model selection in the shared provider router while retaining its persona. No secondary agent runner, storage backend, or scheduler was introduced. Windows/macOS release CI and source-level regression tests check these changes; actual external provider uptime, token throughput, paid conversion and 24-hour reliability need deployment measurements.

The Money page now passes the selected `agentId` when enabling the existing autopilot endpoint. This binds the Money persona in the actual GUI-to-API-to-job path; the backend retains the same agent while switching only provider/model. Without a selected agent, the existing Money research prompt remains available. The page does not create or install an agent, scheduler or separate database.

## Atomic owner approval and native SDK launch (2026-11-18)

The existing Money candidate UI now calls `POST /api/money/candidates/:id/implement` with `approved: true`, a selected agent and a bounded token budget. The server owns and caps the untrusted research brief. This optional Money adapter uses the shared `sdk::prepare_from_value` to prepare a normal SDK run, then atomically claims the existing `money_candidate` and inserts its SDK session/run into the **same SQLite transaction** via `Store::mutate_record`. If another approval already claimed the candidate, the same SDK run is returned instead of starting another model job; missing, rejected or modified candidates fail without orphaned session/run rows. Existing candidate hypothesis status stays unchanged.

The persisted `implementationRunId` survives page reload and points to the ordinary SDK execution journal, including cancellation, tool approval, explicit resume and verification. Generic `GET /api/sdk/runs/source/money_candidate` remains for backward compatibility, while the UI resolves exact persisted run IDs even beyond its recent 40-row window. No second model loop, job worker, database or authorization system is introduced. Regression tests cover refusal without approval, atomic write, duplicate/concurrent claims and rejection of changed/rejected records. Real-model completion and release packaging remain separately unverified.

## Owner-approved implementation via the common SDK (2026-11-17)

The Money candidate card offers an explicit **MVP 구현 승인** action. It calls the thin optional Money approval adapter, which prepares the existing native `POST /api/sdk/runs` execution path with the selected agent, provider/model, bounded token and round budget, and generic `source: {kind:"money_candidate",id:"..."}` metadata. Approval authorizes only an in-workspace prototype; the generated instruction excludes outreach, purchases, paid campaigns, contractual commitments and publication. The SDK performs its normal model/tool execution and human tool approvals; Money never gets its own runner, permissions or DB. The card polls an active run with `GET /api/sdk/runs/:id`, renders its actual status and tool approval request, and offers existing cancel/resume actions. On reload, one indexed `GET /api/sdk/runs/source/money_candidate` fetches at most 40 recent linked runs rather than scanning all SDK jobs. The source reference is untrusted linking metadata, not proof of a valid business or an authorization token.

No external real-model completion or release build has been claimed for this integration. Rust source-level regression tests cover generic source metadata and the indexed lookup.

## Bounded idle dashboard reads (2026-11-17)

`GET /api/money/stats` now delegates to `Store::money_dashboard_stats` in the optional Money storage module. SQLite computes candidate counts and grouped model outcomes in place, while the page decodes only the newest ingestion report and one active autopilot. New Money-owned indexes support the last-ingest and enabled-autopilot lookups. The old endpoint deserialized all historic Money candidates, ingest reports and all job records on each refresh; that unbounded Rust allocation was removed. Existing response keys and front-end semantics are preserved. This is a source-level algorithmic improvement; no DB latency or memory benchmark has yet been run.

## Settings-controlled detachable Money module (2026-11-18)

The Money capability is opt-in from the ordinary **Settings → Money tool** panel. The existing `money_routes` adapter registers `GET/POST /api/settings/money`, and a scoped middleware rejects new Money POST requests when disabled while preserving read-only access to historical market evidence. The common job worker only invokes `money_tool::schedule`; it does not own another scheduler or model runner. When disabled, that callback is a one-row Indexed SQLite check and returns before RSS/catalog/folding/AI scheduling.

The Money-owned SQLite migration adds a single `money_module` setting in the shared `records` table. New installations default off; upgraded installations with existing Money campaigns/signals/autopilots retain their prior enabled behavior. Turning off atomically updates the setting, pauses Money autopilots, and cancels queued/running Money domain jobs; it never deletes the corpus or cancels independently approved SDK runs. New source/model work checks the setting and cancellation state. Re-enabling does not silently resume a previously paused model campaign. The existing Money UI displays archived evidence while disabled, and only enables research/implementation actions after the setting is on.

An unrelated discovery correctness bug is also fixed: the RSS ingestion job previously paired only the pre-ingestion `history` sample, postponing this cycle's new market relations. It now reloads a bounded recent sample after storing the new signals before deterministic cross-market pairing, with no extra LLM calls.

## Historical candidate archive and deferred review

The Money screen now includes a collapsed, on-demand historical archive. `GET /api/money/candidates/archive?q=&status=&limit=&offset=` performs bounded, paginated SQLite retrieval across all candidate records including previously low-ranked, rejected and explicitly deferred ideas. Search covers the buyer, product proposal, title and payment rationale; snippets keep the original source links. The user may mark an idea `deferred` or return a deferred/rejected item to `researching` through the existing candidate update API. These are user decisions, not paid model calls, and never delete original observations or promote an unverified hypothesis to fact. This feature is contained in `web/src/pages/money/Archive.tsx` and `src/db/money_archive.rs`, keeping the common Rust harness untouched.

The small-model shortlist now excludes `deferred` and `rejected` candidates from automatic re-proposal while keeping them in the searchable archive. This avoids resurfacing intentionally parked ideas on every new Money click. Catalog HTTP requests and RSS article fetches recheck module cancellation before costly work; the catalog checks again before database ingestion so disabling the tool during a remote fetch does not silently commit new work. Added a legacy database migration regression test preserving archived Money signals.

Legacy migration recognizes the full `money_*` record family (excluding the module switch itself), so upgrades that only have heartbeat/catalog/folding reports still preserve the previous active behavior. An idle-scheduler regression test proves the off state does not enqueue background jobs and that toggling off cancels only Money-domain work.
