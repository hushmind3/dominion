# Sub2 — Native Rust multi-file patch execution, third development phase (2026-11-15)

## Public source basis

- [Codex Rust apply-patch parser](https://github.com/openai/codex/blob/main/codex-rs/apply-patch/src/parser.rs): Begin/End Patch, Add/Update/Delete File, optional Move to, context hunks and End of File.
- [DeepSeek Harness filesystem package](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/fs/tool-fs/README.md): whole-patch preflight with simulated file states, staged commit recording, explicit partial/unknown outcomes and safe retry via re-read.
- [DeepSeek actual preflight](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/fs/tool-fs/src/patch/preflight.ts), [executor](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/fs/tool-fs/src/patch/execute.ts).

These references inspired the execution contract. Dominion owns the implementation, tool policy, session identity and actual Rust job worker. It does not import the other agent SDKs or assume their scores.

## Runtime path — real user action to persisted receipt

The native Rust chat and durable autonomous job both expose the same tool catalog. When a model proposes workspace_apply_patch, the existing Rust operation scheduler treats it as exclusive, verifies the owner's tool approval and executes tools::execute. The patch module resolves every file against the active workspace root, rejects traversal or .git internals, parses the requested Add/Update/Delete/Move operations and their @@ hunks (maximum 32 touched operations, maximum 512 KiB input).

All source text (up to 1.5 MB per file, 8 MB aggregate source/output) is checked before any source mutation. Every patch hunk must match an unambiguous current context, avoiding a model's guess silently replacing repeated code. All generated files are prepared in same-directory staging files. The engine rechecks originals against their preflight snapshots and checks the target path again immediately before commit. The patch holds the same canonical, ordered per-file Tokio mutexes already used by workspace_write and workspace_replace, so separate native agent sessions cannot overwrite one another during the commit.

On success the tool returns an individual before/after SHA-256 receipt for every written, deleted or moved file. The Rust independent verifier checks these receipts against the actual final workspace bytes and against every requested path. The autonomous retry loop requires changed-file evidence and a post-edit test when no explicit acceptance check exists.

On preflight rejection, no source changes occur. Once the commit begins it is **not a cross-file atomic transaction**: an I/O error stops further operations, with completed entries, not-executed count, optional unknown outcomes, and a direction to re-read each changed path. This is consistent with DeepSeek's declared partial-effect policy and avoids a fake rollback guarantee. The general effect engine and owner-review path handle interrupted or uncertain calls. Only explicitly uncertain effects force mandatory review; ordinary rejected preflights can be re-planned.

The existing code_search repository index is marked dirty after any actual patch changes, including partial changes, and rebuilt lazily when next needed rather than re-indexing a large repository after every single write.

## Concrete test coverage

The patch module tests a four-file patch combining update, delete, add and move; all-or-none preflight on a bad second file; duplicate targets and path traversal; ambiguous repeated context; and a Codex insertion-only @@ hunk. The native tools tests exercise the actual execute entrypoint and check the repository index invalidation. Verification tests require matching SHA-256 evidence for each specified path. Autonomy tests reject uncertain partially committed patches rather than blindly retry them.

## Remaining scope and truthful guarantees

This is a composable multi-file patch capability, not proof of full autonomous large-repository benchmark performance. It does not introduce a second agent harness; Git operations remain available through the existing approved terminal/GitHub tools. An OS crash between two file commits may leave known or unknown partial effects; the persisted operation record and restart review process must be used before any replay. The frontend remains the main-branch UI snapshot; the tool is reachable through the existing native Rust run/jobs API and optional TypeScript client.
