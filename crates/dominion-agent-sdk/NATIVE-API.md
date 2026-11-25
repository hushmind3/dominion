# DOMINION native API v1
Same existing SDK exports and public host contract in all branches. Alias the
dependency as dominion. Branch1–3: dominion-agent-sdk; branch4: dominion-sdk,
including --no-default-features. No second agent runtime.

- run / run_with_options: async verified execution.
- run_sync / run_sync_with_options: synchronous local Tokio runtime, no nesting.
- run_channel: caller-polled result future plus lifecycle events.
- run_streamed: actual text/tool-argument provider deltas, requires the host
  streaming port. No synthesized deltas.
- run_batch: sequential tasks, unique run IDs.
- RunControl::pause / continue_run / step / step_n / cancel: live operation
  admission. Already dispatched operations may finish. Step counts external
  operations, not engine instructions.
- RunControl::suspend: permits the current batch to settle and saves a durable
  continuation before the next provider call. May complete if no next call.
- resume / resume_with_options / resume_channel: use the original engine,
  request and cumulative budgets. Verified checkpoints return saved results.
- NativeHook::before / event: async approval/veto plus event observation.
  Cannot authorize hidden tools. Independent rejection can request bounded
  correction within the existing native engine (default two retries).

NativeHost supplies real model/tool/evidence I/O, approvals, atomic storage and
exclusive run ownership. Hosts must enforce provider/tool timeouts. Native
admission deadlines include live waits but exclude time between suspension and
resume. Poll the channel result and drain events concurrently; no hidden task.
Token usage stays unknown when the provider omits it. Unknown crash/transport
outcomes cannot be replayed. Only explicitly suspended checkpoints or verified
results are portable through this API. No cross-engine or old-envelope
migration. Hooks reject rather than fabricate outcomes. Existing engine-specific
recovery APIs remain available.

Run cargo test -p dominion-agent-sdk native::tests on branches 1–3; on branch4
cargo test -p dominion-sdk --no-default-features native::tests. These are source
tests; compilation and external 9B rebenchmark must be run separately.

## Provider-context ownership and uncertainty

Set RunOptions::provider_context = Some(ProviderContext::default()) to opt into
SDK-managed model context (12,000-byte soft history budget and receipt-based
repeat advice). Hosts forward ModelRequest.messages unchanged. Original tool
JSON and paired IDs are preserved; the structured kernel's latest receipt
previews are replaced in-place rather than duplicated. This is an opt-in
projection; the default existing raw port behavior is unchanged. Configuration
is bound to the checkpoint. ContextPrepared reports projection time.

For effectful tools, NativeHost::dispatch returning Err means an unknown
external outcome. The portable session fences additional model/tool/verify
operations and refuses resume. Return ToolOutcome { success: false, ... } only
for a known failure. Do not replay an uncertain effect automatically.
