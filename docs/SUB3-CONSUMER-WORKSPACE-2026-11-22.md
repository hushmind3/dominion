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
- `styles/foundation.css` owns the Tailwind theme. Ivory `#FFFDF9`, sidebar
  `#F4F1EA` and coral `#C96442` match the selected preview. Components use semantic
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
- The upstream group prompt editing change is preserved, including its real POST
  endpoint. `RoomPromptEditor` uses the shared prompt-section component and shows
  all three editable layers without collapsed sections. Persona scope and the
  precedence of a final override are explained beside the fields.
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
composer widths, and three visible group prompt editors. This rendering check
made no backend writes and does not stand in for a real model response.

The initial frontend pass did not compile or replace the backend. The subsequent
[backend command refactor](SUB3-BACKEND-COMMANDS-2026-11-23.md) supersedes that
deployment boundary: it includes Rust diagnostics, a local development executable
and isolated real-HTTP verification of the changed persistence and execution
paths. Test suites and release builds remain separate. No external paid-model
success or imported credentials are claimed.
