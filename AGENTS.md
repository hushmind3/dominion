# DOMINION source engineering contract

This branch is a fresh native implementation. The authoritative user request is to keep the retired autonomous-target subsystem completely absent.

- Do not restore retired target-specific menus, endpoints, Rust modules, data records, commands, types or documentation. Reject changes that introduce their four-letter English identifier; the source contract test detects it.
- The only primary public TypeScript client lives in `sdk/typescript`. Do not create a second SDK implementation under `packages` or embed an external agent runtime.
- Rust alone owns planning, tool dispatch, model turns, permission checks, event persistence and completion.
- Browser UI and TypeScript SDK call the same `/api/tasks` endpoints.
- Keep the branch isolated from main. Do not overwrite main or rebase destructive changes into it.
- Review source-contract tests and local tests before pushing. Never fabricate observed benchmark results, provider costs or successful compiled builds.
