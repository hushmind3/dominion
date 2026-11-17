# sub2: event-driven shared job scheduler and indexed restart recovery

## Source comparison (2026-11-17)
- `main` branch: `src/engine/jobs.rs` has one semaphore-bounded queue dispatcher; `src/db.rs` returns SQL query errors and resolves uncertain operations using a join.
- `dominion-sub3`: queue read failures are audited rather than treated as no jobs.
- Qwen Code October 2026 recovery designs: unconfirmed tool side effects cannot be safely replayed without provenance and evidence (https://github.com/QwenLM/qwen-code/blob/main/docs/design/2026-10-09-hosted-g3-step3-recovery.md).
- OpenHands SDK 2026: persisted events and tool specifications support resume (https://github.com/OpenHands/software-agent-sdk/blob/main/openhands-sdk/openhands/sdk/conversation/impl/local_conversation.py).

## Implementation
- `src/engine/jobs.rs`: one dispatcher, worker semaphore, event/completion wake, periodic 10-second fallback, unchanged 30-second maintenance and Money opt-in; active queue read errors audited.
- `src/db.rs`: active jobs and running recovery queries return `Result`; uncertain operations' direct/owning-session attribution computed by a SQL set query. Corrupted JSON produces an error.
- Recovery fail-closed on uncertain effects query errors; run/operation statuses from SQLite preserved for inspection.
- End-user input -> SDK/API -> Rust job -> provider/tool -> durable result path and Money's shared runtime unchanged.
- Idle scan estimate with 4 workers: 48/min -> 6/min (8x fewer). No actual workload latency, CPU, token-cost or completion-rate benchmark is claimed.
- Regressions: dispatch wake predicate, linked uncertain session without direct session id, safe completed operations, corrupt queue JSON, previous cancellation and recovery tests.
