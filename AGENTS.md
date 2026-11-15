# DOMINION repository engineering contract

This file applies to all agent-assisted source changes in this repository.

## Definition of done: no residue

A refactor is **not done** when the new code merely works next to abandoned code.

1. Move behavior to the intended native Rust module, preserve its real runtime entrypoint, **delete** the superseded implementation, unused wrappers, dead imports, stale fixtures and obsolete module declarations in the same change.
2. Update **all consumers** in the same change: Rust callers, API routes, SDK contracts, UI, E2E tests, benchmarks, scripts, CI, source maps and active documentation. Never restore an old deleted file just to satisfy a stale contract test.
3. Check for references to removed filenames and symbols across the repository, not only in the touched file. A static tool target pointing at a deleted file is a defect even if the main binary compiles.
4. Do not let a split public `mod.rs` become the old monolith again. Keep substantial execution, verification and lifecycle logic in focused submodules. Delete duplicate `foo.rs` vs `foo/mod.rs` entrypoints.
5. Before committing a changed entrypoint, run `node --test scripts/source-hygiene.test.mjs` and `node scripts/source-hygiene.mjs`; fix failures, never silence the gate. Update the gate if a newly retired source path warrants permanent protection.
6. Run the relevant contract tests, Rust release tests and E2E where the environment permits. A CI result still pending or failed is **not** a passed build.
7. Do not replace a real capability with a mock, synthetic receipt, placeholder score, hidden SDK sidecar or cosmetic refactor. Preserve authorization, owner corrections, independent acceptance evidence and actual tool execution.
8. Do not merge champion into `main` without an explicit owner request.

A cleanup must be included in the original implementation work. Never defer routine residue removal to a subsequent user prompt.
