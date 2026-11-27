# DOMINION private-chat context architecture and wire-cost regression — 2026-11-20

## Scope and reproducibility

Repository: `hushmind3/dominion`, `main`.

The tests exercise a **real DOMINION Rust binary** over the public HTTP
chat endpoint, with an isolated SQLite database and a deterministic mock
OpenAI-compatible provider. These are not mocked Rust context functions:
the requests below are the model-visible JSON bodies observed by the local
provider fixture.

Primary tests in `tests/e2e/room_identity.rs`:

- `private_chat_ten_turn_wire_budget_regression`
- `private_chat_hundred_turn_wire_budget_regression`
- `private_chat_selects_one_current_context_and_evicts_old_long_input`

Additional tests verify cacheable contextual snapshots and corrections,
provider-token budget adjustment for local/small-context models, retained
latest owner message, and Codex WebSocket `previous_response_id`
continuation.

Run the targeted suite:

```bash
cargo test --locked --bin dominion-rust engine::chat::context_engine::tests
cargo test --locked --bin dominion-rust private_context_epoch_keeps_stable_snapshot_and_invalidates_corrections
cargo test --locked --bin dominion-rust engine::chat::context::tests::private_
cargo test --locked --test e2e room_identity::private_chat -- --nocapture
cargo test --locked --test e2e codex_websocket_chain_reuses_identity_and_sends_only_the_new_suffix
```

## Fixture

- The same isolated session, provider, and **6,000-character static persona**
  for all turns.
- Each owner prompt is a unique marker plus **4,000 repeated characters**.
- No direct tools or auto-learning for the long transcript.
- Exactly **one** mock-provider request per conversation turn; no additional
  summary-model or retry calls in this fixture.
- The last turn's owner input must survive. An old large owner input must
  not be rebilled after eviction.
- After 10 and 100 turns, a simple `안녕` is sent with tools enabled.
  It must carry only fixed system context and that latest owner input.
- The mock provider returns **synthetic token usage**. Its reported token
  counts are *not a measurement of actual inference tokenization or billing*.

### Observed wire JSON sizes

The runner serializes each captured provider request as JSON and sums its
UTF-8 text length; the baseline compares system-message JSON plus a naive
replay of all preceding owner input strings. This baseline deliberately
**omits prior assistant text**, tool schemas and protocol overhead, so it
is conservative, not a production invoice or exact tokenizer output.

| Scenario | Observed summed provider JSON bytes | Conservative unbounded replay bytes | Relative reduction |
| --- | ---: | ---: | ---: |
| 10 owner turns | 274,399 | 288,620 | **4.9%** |
| 100 owner turns | 3,527,359 | 20,976,200 | **83.2%** |
| Greeting after either scenario | 7,497 per last request | — | Single current-turn request; no historical task transcript |

Ratios: 274399 / 288620 = **0.951** and
3527359 / 20976200 = **0.168** (rounded).

These numbers come from a passing GitHub Actions macOS dev-profile run:
[CI run 44698895126](https://github.com/hushmind3/dominion/actions/runs/44698895126).
The `PRIVATE_CONTEXT_WIRE_PROBE` and `PRIVATE_CONTEXT_SOCIAL` output is
produced by the test fixture itself.

### Distinctions essential to interpreting the benchmark

1. **Wire bytes** are serialized HTTP/WebSocket request bytes, not billed
   provider input tokens. A stateful WebSocket can transmit only new items
   while the provider still needs the inherited context for inference.
2. **Provider cache hits** are recorded only from actual provider response
   usage. Prefix reuse, shared cache keys and local token estimates do not
   prove billed cache discounts.
3. Model response quality and long-range reasoning cannot be established
   from a deterministic mock provider. The test enforces preservation of
   current inputs, ordered tool evidence and the stable context prefix.
   Real-model quality and billed-token A/B experiments remain separate
   tasks requiring actual provider usage and controlled prompts.
4. The measured 10-turn reduction is modest because the first few turns
   legitimately contain much of their short local history. The benefit
   grows after more history would otherwise accumulate indefinitely.

## Design boundaries

- `src/engine/chat/context_engine.rs`: private history selection, capacity/
  output reservation and single relevant evidence snapshot.
- `src/engine/chat/context.rs`: immutable persona/owner identity and scoped
  retrieval from SQLite.
- `src/engine/chat/continuity.rs`: cache-compatible, revisioned stateful
  projection; explicit rebase on context correction, withdrawal, social reset
  or incompatible transport contracts.
- `src/engine/chat/codex_ws.rs`: socket-local response chain and exact
  prefix-matching; full context on invalidation.
- `src/engine/chat/provider.rs`: protocol adaptation and actual provider
  usage collection.
- `src/engine/gateway.rs`, `src/db.rs`: append-only usage ledger, including
  reported cache reads and creation tokens.
- `src/engine/execution_budget.rs`: conservative budget reservation and
  provider-rate-aware cache usage settlement.

Room-context selection, Rust execution journal, existing UI/menu hierarchy,
and user SQLite records are preserved.
