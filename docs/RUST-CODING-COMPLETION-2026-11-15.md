# Rust-owned autonomous coding completion gate — 2026-11-15

**Scope:** one product-connected coding execution capability, not an assertion that all of DOMINION or this capability is benchmark-certified frontier-equivalent.

## Contract and entrypoint

The same path serves the main-based GUI, TypeScript HTTP client and Rust hosts:

`web/src/components/NativeAgentPanel.tsx` → `POST /api/jobs` with `input.acceptanceChecks` → `src/api/jobs.rs::native_acceptance_checks` → `src/engine/native_agent.rs` → `dominion-agent-sdk::Kernel::run` / `Kernel::resume` → `crates/dominion-agent-sdk/src/acceptance.rs`.

A caller can supply **more than one** concrete command gate:

```json
{
  "kind": "prompt",
  "input": {
    "prompt": "Fix the failing source and tests",
    "acceptanceChecks": [
      {"type": "file_contains", "path": "src/lib.rs", "contains": "corrected_function"},
      {"type": "command_succeeded", "command": "cargo test"},
      {"type": "command_succeeded", "command": "cargo clippy"}
    ]
  }
}
```

These inputs set required postconditions, not client-asserted success. A `command_succeeded` request maps to a typed Rust `ToolSucceeded` criterion with exact `shell_exec` command matching. The server removes any client-supplied `satisfied`, `receipt` or `evidence` fields on that criterion. The shell host still requires normal user approval.

## Deterministic completion and repair state machine

1. Rust receives the model-proposed final answer but does **not** trust it as proof of completion.
2. For a requested source edit, require a host-confirmed write receipt (`written=true`). General external tasks use their own side-effect receipt; running a command alone is not required to pretend a file was edited.
3. Perform inexpensive independent file checks first. If a required file condition is false, return the failure as a new planning observation without spending the test execution.
4. The Rust-only acceptance planner uses the latest matching exact command receipt **after any possible intervening side effect**. A newer failed test invalidates an older pass. Two requested test commands are not considered changes to each other's source epoch. Unrelated shell/MCP effects conservatively invalidate a previous passing test.
5. For a missing or stale check, Rust dispatches the exact requested command through the existing, approved `shell_exec` tool. It journals the in-flight action **before** dispatch and a result receipt **after** completion. A storage failure before dispatch prevents effects; uncertain results are never blindly replayed.
6. Success is accepted without an extra model call for each verification command. A failing command's exit status and the actionable trailing stderr/stdout are preserved for the next model planning step, which must repair the source. Rust does not blindly retry a failed test without an intervening change.
7. A new edit invalidates preceding test success. Permission denial, lost authorization, missing tool, owner cancellation, and a changed live owner directive stop or replan. Completion rechecks owner direction after lengthy verification and requires all postconditions to pass.

The model proposes edits. Rust determines permissions, ordering, persistence, acceptance and terminal status. There is no imported Claude/Codex/Hermes agent executor.

## Deterministic regression cases (authored, not yet executed)

- exact command identity, latest failure superseding pass, stale pass after edit (including failed partial effects), two independent checks, read-only observation, unknown external tool effect
- automatic green-path test without an extra model call
- failure → real tool result/diagnostic feedback → model repair → rerun → completion
- failed check without repair → stop without rerun
- missing shell permission, forgery of client-side proof and post-approval owner steering/cancellation
- no requested file edit actually observed, cheap file criteria not met
- committed checkpoint continuation without command replay
- write-ahead persistence failure preventing a command from executing

Implementation and tests are in `crates/dominion-agent-sdk/src/acceptance.rs`, `src/lib.rs`, `tests/kernel_replanning.rs`, `src/engine/native_agent.rs` and `src/api/jobs.rs`.

## Design references

- OpenAI, *Unrolling the Codex agent loop* (2026-01-23): https://openai.com/index/unrolling-the-codex-agent-loop/
- DeepSeek Harness, concrete agent-loop package: https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/core/agent-loop
- OpenAI Codex, Rust tool scheduling source: https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/parallel.rs

The design choices adopted here are persistent tool receipts, explicit owner-controlled acceptance criteria and terminal gating in Dominion's independent kernel; no foreign SDK runtime was copied or embedded.

## Limits / verification status

The code and scripted tests were authored and compared statically. The current execution container lacks `rustc` and `cargo`; **no executable Rust test results or live model/provider evidence are asserted**. Required next local commands are `cargo test --manifest-path crates/dominion-agent-sdk/Cargo.toml` and `cargo test --manifest-path Cargo.toml` followed by a real GUI/API coding task with a failing test, source repair, approval and restart. Full CI and SWE-bench remain deferred until repository cleanup and product integration. Arbitrary shell commands may mutate source outside the native workspace editing tool contracts; those mutations are conservatively treated as unverified for file-edit proof. That case requires a later source-diff evidence mechanism.
