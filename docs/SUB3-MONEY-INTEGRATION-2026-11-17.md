# Dominion Sub3 · Money integration (2026-11-17)

The `dominion-sub3` branch now includes the Money research UI and its entire source-backed business opportunity pipeline, adapted to the independent Rust SDK. Only this branch changed.

## Execution architecture

- **UI:** `web/src/pages/Money.tsx`, navigation in `WorkspaceShell.tsx` and lazy route in `App.tsx`. Investigate from archived SQLite observations, optionally ask an existing agent to reason once, or enable a budgeted recurring research campaign.
- **Zero-token acquisition:** `money_sources` ingests public RSS/news; `money_catalog` captures GitHub/Hugging Face technology activity; `money_folding` converts source revisions into events and indexed asymmetries. The domain uses the existing SSRF-guarded `http_fetch` adapter.
- **Durable store:** `src/db/money.rs` implements the Money methods as a child extension of **the same** `Store`. `money_schema.sql` adds FTS5, indexed fact terms, entity/date, source, active-status and market-pair indexes. All candidate/source/report commits use the shared SQLite transactions, not a competing database.
- **Ranking improvements:** publication age uses the source's published date, not crawl time. Syndicated identical headlines on different hosts are not accepted as independent market evidence. Explicit buyer requests and directly fetched pages receive bounded priority, not claims of actual revenue.
- **Hypothesis generation:** `money_mechanism`, `money_asymmetry`, `money_intelligence`, `money_retrieval`, `money_rank`, and the added evidence-prioritized, cross-publisher diversity selection rank causal opportunities. Publication date is preferred over crawl date, two independent sources over a headline, and explicit buyer requests over unsupported estimates. All amounts with no evidence remain unknown.
- **Model inference:** `money_discover` is dispatched by sub3's existing `engine::jobs` worker into **`native_agent::run`** and the independent Rust SDK. Input `prompt` is a short, non-mutating research task; detailed market evidence is passed via `ownerInstructions` to avoid misclassifying quoted source text as file-edit authorization. Tools and MCP are disabled, actual model calls capped at 3, and native SDK has a 4,000–120,000 token ceiling. Text-only/compat 27B models may return plain JSON, normalized to the SDK's terminal `answer` envelope; native-tool-capable providers already return plain final answers.
- **Idle efficiency:** the host retains its 30-second approval housekeeping interval but calls the optional Money scheduling hook only once every 60 seconds (50% fewer idle plug-in marker lookups). Explicit user enable still schedules immediately. No second worker, database, or always-on model loop.
- **Plug-in boundary:** `src/engine/plugins.rs` registers Money job kinds into the existing host queue. Money stays dormant at startup unless an explicitly enabled saved campaign needs recovery; per-user collection and analysis execute when requested, with the same SQLite database and no duplicate engine. The optional host heartbeat checks enabled campaigns, not independent domain workers.
- **Money UI execution:** completed Rust jobs store domain payloads under `job.result.result`; the UI now reads that envelope and the persisted execution record for model calls and estimated tokens. Money offers native `/api/jobs/:id/cancel` for ongoing inference, explicit candidate rejection, and a user-confirmed, tool-allowlisted prototype implementation job using the existing Rust SDK. No external purchase, customer contact or deployment is approved by that action.
- **Persistence and agency:** all saved candidates have source provenance and speculative status; rejected hypotheses and reported business outcomes feed existing deduplication and follow-up. First revenue is a user-reported or independently witnessed event, not a model guess. Unattended research uses the shared maintenance heartbeat, single campaign claims, and restart-safe run IDs; external commercial actions remain user-controlled.

## Public interfaces

- `GET|POST /api/money/find` and `GET /api/money/stats` are directly connected to the Money UI.
- The full campaign, candidate, experiments, validation-kit, and autopilot endpoints remain available.
- `sdk/typescript` and `sdk/rust` expose Money calls without launching another execution framework.

## Verification and limits

`Sub3 Money domain acceptance` GitHub Actions verifies the Rust domain algorithms, SQLite/API unit tests, native JSON envelope, TypeScript page build, and lean-product checks. Only Windows/macOS are checked in focused Money CI. Full Windows/macOS release CI is a PR/manual gate, not a per-commit automatic build.

This implementation does **not** claim real revenue, 27B long-running performance, external customer confirmation or model success rates without measured jobs. It never runs unapproved purchases, outreach or deployment.
