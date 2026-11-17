# Money as one removable Dominion tool

- Core runtime: existing Rust jobs, shared provider/tools, permissions, token budgets and SQLite database. There is no Money-specific second harness.
- Tool code: `src/engine/money/` (collection, ranked signals, hypotheses and one dispatch/timer adapter).
- API code: `src/api/money/` (all routes under one `money::routes()` mount).
- Shared database: `src/db/money.rs` extends the host Store, including domain-specific indexes. Records stay in the same DB under `money_*` kinds; no second DB or migrator.
- UI: optional `web/src/pages/Money.tsx`, mounted from `web/src/App.tsx`.
- Detach points: one `engine::money` module/re-export block, one API route mount, one database schema call, two scheduler tick calls and one job-kind dispatch arm, one UI menu/page. Removing the tool leaves existing database records intact.
- At idle with no explicitly enabled Money autopilot, the common host timer calls `money::tick`, which performs no source collection or AI calls. Manual Money Find may explicitly queue a collection job. User-enabled autopilot can resume its work.
- No global task kernel, auth system, SDK client or common DB migrations are replaced by Money.
