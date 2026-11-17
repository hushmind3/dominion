# Money as one removable Dominion tool

- Core runtime: existing Rust jobs, shared provider/tools, permissions, token budgets and SQLite database. There is no Money-specific second harness.
- Tool code: `src/engine/money/` (collection, ranked signals, hypotheses and one dispatch/timer adapter).
- API code: `src/api/money/` (all routes under one `money::routes()` mount).
- Shared database: `src/db/money.rs` extends the host Store, including domain-specific indexes. Records stay in the same DB under `money_*` kinds; no second DB or migrator.
- UI: optional `web/src/pages/Money.tsx`, mounted from `web/src/App.tsx`.
- Detach points: one `engine::money` module/re-export block, one API route mount, one database schema call, two scheduler tick calls and one job-kind dispatch arm, one UI menu/page. Removing the tool leaves existing database records intact.
- At idle with no explicitly enabled Money autopilot, the common host timer calls `money::tick`, which performs no source collection or AI calls. Manual Money Find may explicitly queue a collection job. User-enabled autopilot can resume its work.
- No global task kernel, auth system, SDK client or common DB migrations are replaced by Money.

## Explicit user-approved implementation path
- The Money card presents a per-job token limit and an explicit confirmation.
- `POST /api/money/candidates/:id/implement` requires `approved: true` and `tokenBudget`; the candidate is untrusted research JSON, not executable instructions.
- `src/api/money/implement.rs` creates a normal `prompt` job inside the same SQLite transaction that stamps `money_candidate.implementationJobId`.
- Candidate approval is idempotent even for overlapping requests; a missing job receipt blocks replay. All future tool approvals, cancellation, budgets, retries and artifacts remain owned by the shared Rust harness.
- UI polls the existing `/api/jobs/:id` endpoint, can cancel through the standard route and open the owning chat session.
- This is a user-authorized implementation handoff, not evidence that paying customers or sales were confirmed.
