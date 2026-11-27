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

## Deterministic regression cases (executed in focused GitHub verification)

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

## Executed verification — 2026-11-15

**[GitHub Actions run 43684016242](https://github.com/hushmind3/dominion/actions/runs/43684016242)** on commit `58c37744763506268a369674e3f828ae56b2a699` **concluded SUCCESS**:

- `cargo test --manifest-path crates/dominion-agent-sdk/Cargo.toml -- --nocapture`: **35 passed, 0 failed** (12 unit + 6 kernel integration + 15 replanning + 2 real-file/shell tests)
- The two real-file tests run actual filesystem writes, `sh -c` commands, a real nonzero exit code, repair, re-verification and disk checkpoint restore. A scripted model supplies deterministic decisions; this is **not** a claim that a live frontier model solved a fresh task.
- `cargo test --manifest-path Cargo.toml --bin dominion-rust native_ -- --nocapture`: **22 passed, 0 failed** (filtered Rust host tests; 724 others not run)
- `cargo test --manifest-path Cargo.toml --bin dominion-rust recover_new_workflow_reuses_durable_rust_sdk_child -- --nocapture`: **1 passed, 0 failed** (745 other host tests not run)
- `npm ci --prefix web`, `npm run typecheck --prefix web`, `npm run build --prefix web`: **all succeeded**. Vite reported a successful production build.

**Total executed Rust cases in this narrow gate: 58 passed / 0 failed.** Full product CI, live-model coding success, live-browser UI, paid provider budget, and SWE-bench remain independently unverified. The regular full CI workflow was not invoked by this gate; verification is a dedicated `sub3-rust-sdk-check.yml` workflow and is scoped to core/host/UI validation. This narrows one proven execution capability, not a declaration that all of Dominion or its coding intelligence is frontier-equivalent.

The Rust SDK and focused native host tests have executed successfully on a GitHub runner. The current container still has no local `cargo` toolchain. The real filesystem/shell integration case succeeded with a deterministic scripted planner; a real-provider coding task, live GUI/API approval exercise and restart drill remain to be run. Full CI and SWE-bench remain deferred until product-wide integration. Arbitrary shell commands may mutate source outside the native workspace editing tool contracts; those mutations are conservatively treated as unverified for file-edit proof. That case requires a later source-diff evidence mechanism.
