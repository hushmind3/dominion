# Dominion sub2 — shared engine and Money reasoning (2026-11-17)

- UI/TypeScript SDK -> Axum `/api/money` -> durable `jobs.rs` -> Money domain logic -> SQLite `db/money.rs`.
- Deterministic ingestion: RSS/GitHub/Hugging Face collectors, `money_folding` lexical events, `money_mechanism` complementary supply/buyer pairs, `money_asymmetry` contrasts and `money_retrieval` bounded RRF corpus recall.
- Explicit Money inference: `money::run` calls the existing `jobs::run_prompt` with `autonomous:false`, sharing provider selection, approvals, tool execution, receipts and usage. No second provider loop/harness.
- Generic autonomous coding jobs: `engine/autonomy.rs` + independent `dominion-agent-kernel` decision cycle; chat runtime executes provider/tools and persists their outcomes. Money strategies are domain heuristics, not a replacement for the kernel.
- Pair ranking now distinguishes same-publisher subdomains, shared-topic evidence vs speculative cross-sector bridges, and recent vs stale observations. Scores are research priority, not conversion probabilities.
- Learning from experiments now requires explicit marketing, model, labor and direct costs. Null is unknown, not zero. Historical incomplete records remain preserved and are excluded from profit-driven strategy selection.
- CI: only macOS targeted Money/kernel and SDK/UI type checks, Windows Rust test compilation. Linux and routine release-mode whole-suite tests removed to lower runner cost; full tests may be invoked for releases.
- Remaining gaps: real buyer contact and purchases are unverified, lexical topic joins are not causal proof, and long-horizon quality must be measured using the same model and search budget.
