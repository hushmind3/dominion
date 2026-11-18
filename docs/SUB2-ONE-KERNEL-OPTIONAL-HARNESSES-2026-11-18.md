# Sub2 — One Rust owner, zero bundled external harnesses (2026-11-18)

## Architecture

```text
React / HTTP / TypeScript SDK
  → existing Dominion Rust turn loop (the only state/permission/receipt owner)
      → provider / MCP / normal native workspace tools (existing)
      → optional contrastive repair (disabled by default)
          → optional isolated local-file candidate execution (disabled by default)
      → optional external_harness_run tool (hidden when no adapters configured)
          → already installed, independently managed executable
          → one bounded task on stdin; bounded final result on stdout
          → returns through the same native tool result / approval / effect journal
```

**No bundled Codex, Hermes, Pi, OpenCode, Goose, Claude Code, Gemini CLI, or other runtime. No extra Cargo dependencies. No extra TypeScript SDK, DB, worker, scheduler, agent loop or duplicated memory.** The owner can register any executable whose command-line interface accepts the task on stdin and returns a final answer to stdout. Agent-specific invocation is an operator-provided `command` and fixed `args` array; the model cannot choose arbitrary executables or flags.

Example for a locally installed stdin-compatible CLI (substitute your actual executable and verified flags):

```sh
export DOMINION_EXTERNAL_HARNESSES='{
  "local-coder": {
    "enabled": true,
    "command": "/absolute/path/to/installed-coding-agent",
    "args": ["--read-task-from-stdin"]
  }
}'
```

This is a **configuration format**, not a claim that those example flags exist on a particular tool. Disable an entry or unset the environment variable to remove the tool from model discovery. An unset/invalid/empty registry adds zero tool schemas and starts zero child processes.

The model sees exactly **one** optional `external_harness_run` tool and an enum of configured IDs. The tool requires an explicit `harnessId`, `prompt`, and optionally `timeoutSeconds`. User subagent-policy activation and the existing native owner approval both apply. The installed tool's own login/authentication and permissions are still **independent**: an outer approval is not a replacement for a shell/OS sandbox.

## Real native execution boundaries

- The default decision kernel remains the existing Rust turn controller. `program.repair` asks for **one** replacement by default. No experimental candidate runner starts on routine requests.
- `sdkUiPolicy.contrastiveRepair=true` opts a session into real read-only probes and up to three repair candidates using the existing Rust interpreter and receipts.
- `sdkUiPolicy.candidateRollout=true` additionally opts into locally isolated candidate execution, bounded file snapshots and separate SQLite state. The existing activated algorithm may instead set `parameters.contrastiveRepair` or `parameters.candidateRollout` explicitly. None is automatically turned on from a model name or guessed quality score.
- In a controlled `DOMINION_BENCHMARK=1` treatment arm, the advanced repair runs for the existing benchmark harness even without a UI flag; production is unaffected.
- External processes are **not** nested Dominion runs. They execute one independently delegated task and return one result through `tools::execute` → `dispatch_tool_call`. One tool call consumes the same budget, approval, cancellation generation and operation journal as other side-effecting tools.
- All external harness runs are classified as potentially mutating. They share the native workspace resource lock with other writes/shell runs, and unknown outcomes suppress blind retries.
- Up to 8 IDs can be registered. No CLI command is invoked until a user-approved task actually selects it. Bounded task input (24 KiB), shared native stdout/stderr capture (up to ~80 KiB each), timeout (5–600 seconds). A truncated/failed result or stalled stdin is **not** a verified success.
- On Unix a child starts in its own process group and cancellation/timeout kills that group. On other platforms child cancellation does not guarantee termination of all descendants. A CLI may spawn detached processes or manipulate remote services; this adapter is **not an adversarial sandbox**. Do not delegate tasks that require stronger isolation without an independent external sandbox/runtime.

## Why not merge Pi, Goose, OpenHands, etc.?

- Pi / mini-SWE-agent point toward minimizing the core. Current native loop remains singular; new tools are opt-in and not a new agent execution loop.
- Goose / DeepSeek extensibility indicates a **registration surface**, not a reason to copy plugin frameworks or carry several in-memory workers.
- OpenHands / SWE-ReX show that execution-environment isolation is separate from agent decisions; Dominion's optional native file copy is not equivalent to a Docker/remote sandbox.
- Existing `tool.discovery` already supports different native tool sets. The 2026 harness comparison at https://arxiv.org/abs/2609.20804 reports model-dependent outcomes of bash-only versus structured tools. Do not infer a tool configuration from the model's parameter count alone; run identical task/model/budget A/B comparisons.

## Source-level cleanup after reviewing the actual native runner

The first implementation added a second process spawn/kill/pipe/timeout stack in `tools_external_harness.rs` even though `tools.rs` already had the tested `run_command`/process-group supervisor. That duplication has been removed. The existing supervisor now accepts optional bounded stdin in the same timeout/cancellation scope; native shell and terminal calls still use the same code with null stdin. The external adapter only supplies the operator-registered command, task, cancellation event, and output parsing.

A separate defect caused the configured external delegate to disappear when `tool.discovery=core`, because the built-in core set only included `delegate_task`. The optional external delegate is now retained **only where the selected core already allows delegation**; an explicit core list without delegation still excludes it. Duplicate mutating-tool classifications in `turn_body.rs` were removed, and external task delegation no longer inherits a direct workspace read-before-write heuristic.

Measured source changes for the supervisor consolidation: `tools_external_harness.rs` shrank from 309 to 225 lines (**84 lines removed**); the shared native tool executor gained ~33 lines. Rust line count is not compiled binary size. CLI programs are still not bundled and binary/idle-memory differences have not been benchmarked.

## Test and source scope

Changed: `src/engine/tools_external_harness.rs`, `src/engine/tools.rs`, `src/engine/effect.rs`, `src/engine/resource_lock.rs`, `src/engine/chat/runtime/turn_body.rs`, `src/engine/chat/runtime/program.rs`, `src/engine/chat/runtime/candidate_rollout.rs` and relevant Rust debug CI targets. **No main, sub3, sub4 or UI edits.**

Tests prove that disabled adapters do not register, only operator IDs may be selected, independent command stdout returns a bounded result, Codex-style JSONL final answers can be parsed, failed events are not treated as success, effectful operations share workspace locks, and advanced local candidate experiments default to off.

This design implements a generic **one-shot CLI process adapter**, **not** a full bidirectional ACP/ACPX server, long-running external session restoration or sandboxed multi-OS transport. It does not claim equivalent integration with any particular third-party harness until that CLI is tested with its actual installed command.
