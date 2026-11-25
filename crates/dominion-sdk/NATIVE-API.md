# DOMINION native embedding API v1

The same Rust source can use all four independently developed native engines.
Import the existing SDK crate under the Cargo dependency alias `dominion` and
call `dominion::native::run(&host, request).await`. No new engine, server,
provider SDK, external protocol, or product/UI dependency is added.

- branch1/2/3: package `dominion-agent-sdk` at `crates/dominion-agent-sdk`.
- branch4: package `dominion-sdk` at `crates/dominion-sdk`, optionally with
  `default-features = false`.
- branch3's existing HTTP SDK also re-exports the native module.

Only the Cargo dependency target/package changes; the public native host
traits, model requests, tool calls, results, limits and events are identical.
Keep the current native SDK exports for advanced callers.

## Contract

The host implements `NativeHost` once: model completion, authorized tool
execution, independent outcome verification, and atomic checkpoint storage.
It must own an exclusive run-ID lease, enforce approvals/sandbox policy,
and configure timeouts for its external I/O. Provider messages and call IDs
must not be dropped or rewritten. Model usage is optional; unknown usage
never becomes a measured zero. A configured token cap requires actual usage.
The deadline stops admission, not an already-running external operation.

The engine owns its own decision loop, tool admission and native policies.
`ModelRequest.messages` is the native provider transcript or planner view;
`observations` carries available full receipts separately. Different
native algorithms can still choose different context and tool catalogues.
`run` returns success only after independent host verification and final
checkpoint persistence. Native internal completion alone is not success.
A rejected answer returns an error. Existing low-level APIs remain available
for engine-specific repair policies; v1 does not promise automatic repair.

`run_batch` is sequential and stops on first error. `NativeHost::event`
receives turn and tool lifecycle events; this is not token streaming.
Fresh runs refuse existing checkpoint IDs. Automatic cross-engine checkpoint
migration/resume, token streaming, synchronous convenience, and portable
step-by-step driving are not promised by v1. Do not replay uncertain work
or use an engine's checkpoint in a different branch.

## Verification

Identical SDK contract tests cover native tool roundtrip, verified completion,
usage, rejection, token-cap handling and duplicate batch IDs. Run
`cargo test -p dominion-agent-sdk native::tests` on branches 1–3, and
`cargo test -p dominion-sdk --no-default-features native::tests`
on branch4. Then use one actual external adapter and the fixed 7-task 9B
protocol against all four branches. The host is still responsible for model
transport, real tools, and durable checkpoint implementation.
