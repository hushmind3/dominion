# Sub2 RSI: worker provenance display (2026-11-15)

The user's existing Self-Improvement UI now reports three distinct live child proof states: pending, verified and missing/mismatched. The Rust API recomputes the live proof using the **same** `rsi_evolve::worker_proof_matches` predicate that gates experimental adoption. It does not trust a model's success narrative.

Each expanded generation separately shows its own stored spawned-executable SHA-256 fragment, controller proof status, and independent paired receipt replay result. Neither missing proof nor an unevaluated proposal is labeled as successful recursive improvement.

Runtime path: `GET /api/rsi/campaigns/:id` → `campaign_view` → `activeWorker.proofStatus` → `SelfImprovementPage`. Historical generation receipts are already included in the API history and now rendered in the existing evidence section, without creating a second store.

Validation: `npm run typecheck --prefix web`, `cargo test --release`. The change does not trigger a model/API expense and does not claim a fresh successful 3-generation loop.
