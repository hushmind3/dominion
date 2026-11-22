# sub3 consumer workspace

This change belongs to `dominion-sub3`. Main and sub4 are separate worktrees.
The sub3 execution kernel remains `dominion-agent-sdk` with `dominion-harness`;
jobs, team launches, approvals, receipts and streamed room events keep their
existing engine contracts.

## Ownership

- `App` owns one persistent `WorkspaceNavigation`, room selection and new-chat /
  new-group actions. Management pages are ordinary workspace pages, not a second
  shell over an independently navigable chat. Returning to Chat preserves its
  route; only New Chat resets it.
- `Primitives` and `ui/WorkspaceUI` own controls, surfaces, headers, search, empty
  states, view navigation and form actions. Conversation input and target
  selection have dedicated React components shared where behavior is identical.
- `styles/foundation.css` owns the Tailwind theme. Chat and sidebar share
  the same `surface` token (`#FFFDF9`); coral `#C96442` remains the action color. Components use semantic
  tokens. User-selected persona colors remain runtime data. Remaining room
  configuration / team-dialog CSS consumes the same tokens.
- `features/models` separates service facts, connection operations and dialog
  rendering. A stored credential is configuration, not proof of a successful
  connection. Account, API-key and local-model methods remain available without
  promoting a particular vendor.
- `services/remoteStore` owns shared GET snapshots and in-flight requests by
  endpoint. Readers subscribe through `useRemote`; scoped engine events invalidate
  active readers. Writes are explicit and never automatically retried. A stale
  GET cannot overwrite a locally confirmed update. Inactive resource caching is
  bounded; conversation content is not persisted by this cache.
- `features/prompts` owns scoped drafts and explicit saves. Shared instructions,
  personas and the full private-chat system prompt are directly accessible from
  navigation. Unsaved prompt drafts survive route changes in memory and report
  conflicts instead of silently replacing the text. Backend compare-and-patch
  updates preserve unrelated settings and full-prompt overrides.
- `PromptsPage` is the single prompt/persona workspace. Shared/system instructions,
  AI/persona identities and group instructions have distinct tabs. Memory only owns
  remembered information. Groups no longer render a second persona/prompt form.
  The original group POST endpoint remains; `GroupPromptPanel` edits common/final
  text there. Persona text is edited only in the AI/persona form, whose conditional
  save preserves unrelated fields and rejects stale baselines.
- Room messages and live replies share `RoomMessageFrame`. The transcript and
  composer use one conversation width. Token details are optional message
  information instead of permanent byline badges. Group creation asks the owner
  to choose participants; its defaults are one round and no automatic learning.
  Existing saved settings are retained.
  A high turn cap does not itself request every speaker.
- Room attachment uploads retain accepted attachment IDs and pending previews
  until message admission succeeds, so retrying does not re-upload accepted files.

## HTTP composition

`api/routes` groups existing routes into conversations, connections, work,
knowledge, integrations and system. `api/http_policy` owns transport guards.
Collection routes, fallback behavior, middleware ordering, Host/Origin checks and
execution authorization remain in force. Error JSON retains `error` and adds
machine-readable status metadata.

## Verification and deployment boundary

TypeScript diagnostics and browser inspection cover desktop and mobile layouts,
navigation, prompt / conversation drafts, collapse / reopen, connection dialogs
and new-conversation / group controls. Browser reads use the sub3 engine on 4322;
the frontend preview runs on 5173. Static route-declaration comparison checks
path and handler composition, not only route counts.

A populated group was also inspected with browser-only synthetic GET responses
at 1440px and 390px. It showed no horizontal overflow, matching transcript and
composer widths, and the former inline group prompt editors. This historical rendering check
made no backend writes and does not stand in for a real model response.

The initial frontend pass did not compile or replace the backend. The subsequent
[backend command refactor](SUB3-BACKEND-COMMANDS-2026-11-23.md) supersedes that
deployment boundary: it includes Rust diagnostics, a local development executable
and isolated real-HTTP verification of the changed persistence and execution
paths. Test suites and release builds remain separate. No external paid-model
success or imported credentials are claimed.

## Follow-up: one editing location and uninterrupted conversation

- The empty-chat composer stays at the bottom on desktop and mobile. AI selection
  opens an anchored picker; adding/editing a connection reuses `ConnectionDialog`
  inside chat, including the zero-provider case. No picker action changes pages.
  Typed messages stay intact while connecting.
- The legacy agents route resolves to the persona tab. Navigation notifies mounted
  tab readers when query parameters change through the sidebar or browser history.
- Local model search puts file choice, size, download/resume and live progress in
  `ModelDownloadPanel` immediately below the model title. Hardware explanations
  are optional detail. Paused jobs have an enabled resume action. A details error
  exposes retry instead of leaving an indefinite spinner, and download failures
  appear beside the action. Search-format filters do not hide downloadable files
  inside a selected repository. Mobile selection scrolls to the selected model.
  Completed receipts retain the original repository/file labels in the installed
  library without changing runtime IDs or rewriting existing model files.
- Validation uses an isolated engine/database and a loopback AI fixture for chat
  and persistence. A small public GGUF download exercises the real worker without
  adding model files, credentials or fixture records to the live user store.

The follow-up browser pass exercised an actual loopback chat response, inline
connection persistence without route/draft loss, canonical persona and group
saves, and a stale persona save returning 409. The real download worker completed
`ggml-org/models` / `tinyllamas/stories260K.gguf` (1,185,376 bytes) at an immutable
revision and listed it under the original repository after reload. Recovery
branches (paused resume and details-error retry) used browser HTTP fixtures;
they do not claim a second real transfer. Desktop 1440×1000 and mobile 390×844
showed a bottom composer and no horizontal overflow. No paid model calls or live
user-store writes were made.
