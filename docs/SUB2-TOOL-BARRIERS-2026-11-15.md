# Sub2 — Model-order tool barriers and parallel reads (2026-11-15)

## Grounding in publicly readable frontier harness source

- OpenAI Codex Rust runtime: [codex-rs/core/src/tools/parallel.rs](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/parallel.rs) uses a shared parallel-execution lock to distinguish exclusive tool calls from safe concurrent calls.
- DeepSeek Harness: [tools subsystem](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/tools.md) and [agent-loop README](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/agent-loop/README.md) define a complete model-ordered tool batch, opt-in concurrency safety, bounded parallelism and exclusive barriers.

The implementation below is DOMINION-owned Rust code and uses neither project as a runtime SDK. The sources are design references, not measured performance comparisons.

## Real defect fixed

Earlier DOMINION runtime allowed the executable algorithm named "read-first" to sort all proposed read tool calls ahead of mutations. Its "parallel-reads" optimization also extracted all reads from the tool batch before running the remaining writes. For a model response that requested:

    workspace_write(file, "new") -> workspace_read(file) -> workspace_read_range(file)

the engine could execute both reads **before** the write, incorrectly feeding stale file content back into the next model request.

## New single-engine path

The existing Rust chat runner, agent model/permissions, ledger, operation receipts, and SSE message channel are unchanged. A new host-only module at src/engine/chat/tool_schedule.rs classifies only a known set of independent native reads as concurrency-safe; any unknown tool, MCP call, write, shell or browser command acts as an exclusive barrier.

The runtime processes calls in the model's order. Before each group, it reclassifies the **current unstarted call suffix**, parsing the complete JSON arguments; malformed or nonobject arguments remain exclusive. Consecutive trusted reads enter a **rolling pool capped at 4 active calls**. Whenever one finishes, the next eligible read starts without waiting for the slowest sibling. Before an exclusive call (write, shell, MCP, browser, or unknown tool), the existing group is fully drained; exclusives execute serially. The old "read-first" selection is rejected whenever it would reorder a read across a preceding exclusive operation. This override is audited and scored as not applied.

- Every proposed call is charged to the existing call/proposal budget **before dispatch**.
- Malformed arguments generate a matching tool error, rather than dropping the model's pending call ID.
- Responses are reassembled into the original model-call order even if the read futures complete in another order.
- Cancellation, permissions, operation journals and receipt handling continue through the existing `dispatch_tool_call` entrypoint. A cancellation or internal dispatch failure **stops starting new calls**, drains all already-started calls, and exits without inventing successful tool replies.
- A terminal internal dispatch error remains terminal; it is not disguised as an ordinary failed tool result or passed back to the model as a completed call.
- The previously failing E2E case `parallel_reads_answer_malformed_arguments_so_no_tool_call_is_orphaned` exposed a broken error-string compatibility contract. The native engine now preserves the `invalid tool arguments` marker in both serial and parallel execution paths; the test remains unchanged.
- Unapproved writes are never silently executed concurrently; the batch scheduler does not bypass execution policy.

This targets execution correctness under concurrency, not a universally faster runtime or frontier-level benchmark score.

## Coverage

The extracted scheduler has deterministic tests for write/read barriers, multiple parallel groups, a cap of 4, serial mode, exclusive unknown extensions, malformed-argument fail-closed classification, a rolling pool that reaches but never exceeds its concurrency cap, ordered result slots, failure quiescence and cancellation draining. src/engine/mod.rs contains a mock SSE model E2E regression: the model requests a write followed by two reads in one response, with "parallel-reads" and "read-first" forced active. The mock provider checks that it receives the write first and reads observing the *new* bytes, while the original native file tool and event recorder remain in the path.

The test's presence is not a pass claim. The GitHub Actions run must confirm Rust compilation and test success. Large-repository frontier benchmarks and GPU/video measurements remain separate.
