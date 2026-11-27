# sub3 owner command and persistence refactor

Scope: `branch3`, including the frontend callers of changed backend
contracts. The native `dominion-agent-sdk` / `dominion-harness`, approval gates,
worker attempt fences and durable tool evidence remain the execution authority.

## Responsibilities and resulting behavior

| Flow | Previous failure | Current owner |
| --- | --- | --- |
| Connection edits | Full-record replacement dropped omitted configuration; credentials changed before the configuration write could fail | `features/providers/connections` validates a merged draft; `storage/sqlite/provider_connections` commits configuration, encrypted key and exclusive reflection selection in one transaction, guarded against intervening edits |
| Connection removal | Configuration and credentials were separate writes; account-refresh credentials remained | The same repository removes the record, access credential, refresh credential and account catalog together |
| Connection list | GET could call an external account API and overwrite settings with an old complete object | List is read-only; account discovery belongs to explicit save/check operations |
| Connection check | Some checks generated a paid response; keyless local servers were rejected before a request | `features/providers/probe` shares catalog endpoints/auth with model browsing and performs no generation |
| Conversation route | A disabled saved provider could disappear from route selection and trigger automatic rerouting | An explicitly selected or saved route remains pinned; automatic candidates must form valid routes and ties use stable IDs |
| Chat admission | Malformed request fields could silently default, and invalid connections could create empty sessions | `ChatCommand` validates mode, request types, referenced AI/session and route before spawning SSE; runtime lease and cancellation checks still run |
| Plain system prompt | Actual plain-chat execution ignored the full override and used different generated text from preview | `conversation/chat/plain_context` compiles both. Empty overrides remain intentional, including on the Codex wire. Tool authority stays disabled outside the prompt |
| Group submission | HTTP owned channel submission policy; cross-process duplicate admission could return a storage error; retry revalidated dependencies that had changed after acceptance | `features/collaboration/submission` serves HTTP and channel adapters. `storage/sqlite/room_admission` inserts message, job and room receipt metadata atomically and returns an existing immutable receipt for a retry |
| Group configuration | Malformed or empty participant selection could silently pick other AIs; concurrent settings edits were unversioned | `room_config` validates owner fields, rejects malformed selections, and checks `expectedConfigRevision`. Worker progress is preserved |
| Prompt editing | Persona updates replaced an entire AI record; a combined group/persona save could partially succeed | `prompt_settings` uses a multi-record conditional transaction. Group common/final text and persona either commit together or all remain unchanged |
| Task steering | Status-only comparison could lose concurrent appended instructions | `host/execution/job_control` compares directive, revision, cancellation and attempt together. Queued cancellation does not stop another running task sharing the transcript |

`api` handlers translate request/response status; these services own command
validation and side effects; SQLite modules own atomic persistence. Channel
ingestion no longer calls into an HTTP handler. This does not introduce another
model executor or replace sub3 with another branch's architecture.

## Compatible contract extensions

- `POST /api/agents/:id/persona`: `{prompt, expectedPrompt?}` changes only the
  persona and returns the updated AI. Both prompt editors use this authority;
  the group dialog submits its persona in the atomic room command instead.
- `POST /api/rooms/:id/system-prompt` retains `common` and `final` string/null
  fields and accepts `expectedCommon`, `expectedFinal`, and
  `persona: {agentId, prompt, expectedPrompt}`. Null clears an override; an empty
  string intentionally replaces it with empty text.
- Room configuration responses include `configRevision`; callers may submit
  `expectedConfigRevision`. The frontend sends the draft's captured revision.
- A connection check reports `verification: "catalog_read"` and
  `generationVerified: false`. Public catalog information is shown as catalog
  information instead of proof of a working account.
- Room jobs include normalized `submissionIntent`. Legacy jobs are compared
  through their existing immutable input fields, preserving old receipt IDs.

## Verification

- Rust `cargo check --locked --offline --bin dominion-rust`: passed.
- Frontend `npm run typecheck --prefix web`: passed.
- Architecture source gate: passed after replacing its requirement for the
  previously deleted browser fixture with checks that channel ingestion cannot
  call HTTP handlers and owner command modules do not depend on Axum.
- Local development executable: built to apply backend changes to preview;
  this is not a release or frontend production build.
- Manual HTTP exercise used isolated SQLite/vault data and a loopback synthetic
  OpenAI-compatible server. It verified encrypted-key preservation after a
  failed edit, read-only connection listing, catalog-only checking, exact/empty
  plain-chat prompts on the actual model wire, rejection before session creation,
  persona conflict handling, room revision conflicts, atomic prompt rollback,
  actual group-worker completion, replay after connection disable, and task
  directive/cancellation behavior.
- A second real engine process shared the isolated database. Concurrent copies
  of one group submission returned the same message and completed job.
- No test suites, release builds, real paid model calls, credential imports or
  mutations of main's data were used. Synthetic provider results are not evidence
  that an unconfigured external AI account is authorized or reachable.

The disposable review script and its report are local verification artifacts,
not committed application code or user conversation data.
