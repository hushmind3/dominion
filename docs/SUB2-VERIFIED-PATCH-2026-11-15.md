# Sub2 — Proof-carrying autonomous code modification (2026-11-15)

## Scope: complete one narrow frontier-pattern mechanism

This milestone intentionally concentrates on verifiable code changes rather than claiming broad frontier equivalence. A model's final sentence, a successful tool-call **name**, or a passing test from **before** a source edit is insufficient to finish a source-modification job. The native Rust engine remains responsible for acceptance.

## Entrypoint and complete runtime path

`POST /api/jobs` or the unchanged main UI `POST /api/sdk/runs` → `src/engine/jobs.rs::run_prompt` → `src/engine/autonomy.rs::execute` → `src/engine/mod.rs::run_quiet_outcome` → `src/engine/tools.rs::checked_write/write_text` → tool-end evidence → `src/engine/verification.rs::check_acceptance` → final revision seal → completed result or bounded repair / `needs_review`.

1. The owner instruction and any explicit `verifyCommand` enter the native job, not a second Node agent.
2. The source-write tools return `written: true` only on successful writes and `changed: false` when the final bytes are unchanged. The `postSha256` fingerprint is computed from bytes written by Rust.
3. The verifier checks that the receipt describes a supported first-party mutation, has `written=true`, was not a no-op, points inside the canonical user workspace, and its SHA-256 is **still equal** to the actual bounded file read. A forged success flag, unrelated path, stale digest, missing file or external modification does not satisfy this proof.
4. Local coding/edit instructions require this real modification evidence even if a model ran a test or reported success.
5. An explicitly specified `verifyCommand` is a frozen tool-execution acceptance predicate and must pass *after* the last write. Without a concrete file/command predicate, a local code edit additionally requires an actual successful recognized test/build check after the most recent modification.
6. Failed conditions are fed back to the next model attempt; fixed bounds, no-progress guards, side-effect replay restrictions and owner steering revision safety remain active. `result.verified` is only true if current checks have real supporting evidence. `result.proof.workspaceMutation` and `result.proof.postEditTest` state the observed kind of proof.

## Deterministic tests

`src/engine/verification.rs::verified_patch_tests` covers:
- Unexecuted `success=true` tool names cannot impersonate actual edits
- `written=true, changed=false` is not a mutation
- Missing, incorrect or stale SHA-256 cannot pass
- Real first-party `tools::execute(workspace_write)` returns a usable receipt, and repeated identical writes report `changed=false`
- External source modification invalidates a previously true receipt
- A test only qualifies when it executed after the last edit
- Read-only test and repository inspection do not falsely require a write
- Explicit file-target inference and filesystem checks still apply

## Boundaries and future evaluation

This proves a **narrow deterministic invariant**: a completed code-modification task cannot rely solely on model prose or a stale/false native file-write receipt, and a naked success with no acceptance evidence is rejected. It is not proof that arbitrary code is functionally correct. Model reliability, multi-file integration correctness, tool approval policy, long-duration interruption recovery, and frontier benchmark success are separate measures. Tests must pass CI; their existence alone is not a pass report.
