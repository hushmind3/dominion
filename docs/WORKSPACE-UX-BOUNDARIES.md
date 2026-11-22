# Workspace experience and implementation boundaries

The default experience is a conversation with an empty top area. Model selection
lives in the composer; navigation and the single settings entry live in the
sidebar. Reply controls render into that settings screen while retaining the
active conversation state. A new user can connect an existing
account, enter a provider key, or choose an AI on their computer. Model IDs,
request limits, execution internals and developer tools are optional controls.
Existing saved conversation and reasoning preferences continue to apply.

## Frontend ownership

- `App.tsx` keeps visited conversations mounted and composes the settings overlay.
- `SettingsWorkspace.tsx` owns settings layout and its navigation preference.
- `components/chat/` owns the welcome screen, searchable history and AI picker.
  The chat page retains turn admission, streaming, approvals and cancellation.
- `QuickConnect.tsx` presents connection choices using the existing account and
  provider operations. It never invents a model ID or silently imports credentials.
- `api/types.ts` defines wire records; `api/transport.ts` owns authentication,
  cancellation and response errors. Chat, accounts and attachments own their
  product operations. `api.ts` remains the compatible public entry point.
- `hooks/useRemote.ts` owns request lifetime and scoped invalidation.
  `services/engineEvents.ts` owns the single event bridge, coalescing and reconnect
  recovery. Display components do not implement request lifetimes.
- `styles/` separates foundation tokens, conversation surfaces, groups,
  connections and workspace navigation. Main headings are no longer globally
  overwritten by element selectors.

Settings search includes page descriptions. The compact sidebar keeps its
navigation buttons. Mobile navigation keeps an explicit way to close the history
and return from settings. Detailed activity, developer consoles and permission
controls are progressively disclosed and request their data when opened.

## Backend ownership

`api/routes/` composes the same HTTP endpoints by product area. The root router
attaches static files, request-size limits and HTTP policy exactly once.
`api/http_policy.rs` owns local-owner Host/origin admission, remote-worker
exceptions, maintenance and cache headers. The previous middleware order and
all 197 literal route paths are preserved.

`api/providers/` separates saved records, credential import, sign-in, public
catalogs, model discovery, pricing evidence and connection probes. Public handler
names remain available through `api/providers.rs`. Existing provider regressions
retain their `api::providers::tests` module path.

Explicit free-catalog refresh calls model discovery directly with at most four
providers in flight, a 20-second deadline per provider and stable result ordering.
It no longer calls the local HTTP server without the owner's proxy/admin context.
Failure for one provider is recorded without discarding other providers' results.

Shared JSON errors keep the `error` field and add `code` and `retryable` so clients
can offer recovery without parsing localized messages. The browser also accepts
empty 204/205 responses and cancels attachment reads/uploads when a turn stops.

## Delivery constraints

At the owner's request, no further build or test suite was run after they asked
to stop those actions. The updated UI was inspected in the development server
at desktop and mobile sizes. The development UI must use the existing configured engine on port 4317.
Pointing it at an empty or stopped temporary runtime hides existing connections
and breaks chat. Rust source changes still require a later build to run.
Existing source-contract references were adjusted for the new file locations.

## Interaction review

The nine management pages and both conversation modes were reviewed at desktop
and phone widths. The follow-up refactor addresses the underlying interactions:

- Conversation sidebars share visibility, persisted width, pointer resizing,
  keyboard resizing and a collapsed navigation rail. On phones they become
  focus-contained drawers with Escape and focus restoration.
- Chat and group navigation use the same locations. Neither conversation has a
  top toolbar. Reply options belong to one collapsible section in settings.
- Profile editing uses a viewport-positioned panel outside clipped sidebars,
  stays inside an existing modal's focus boundary, and cannot be overwritten by
  a slower pre-save profile read.
- `useModalFocus` owns dialog/drawer nesting, scroll locks and keyboard focus.
  Kept-alive hidden pages deactivate their popovers and modal listeners.
- `useRemote` separates request lifetime from visibility subscriptions, exposes
  refresh state, and refreshes once on return. Provider changes invalidate the
  matching model list; first-connect recovery also reloads previously failed
  views. Mutation retries are never automatic.
- Remote failures have explicit reload actions. Group setup offers the missing
  assistant-creation step and preserves the room draft when navigating away.

Read-only TypeScript diagnostics passed. Desktop/phone inspection confirmed
sidebar collapse/expand/resize, profile bounds, modal/drawer focus restoration,
settings return and group-to-assistant navigation. Production builds and test
suites remain skipped under the owner's delivery constraint.

## Conversation focus follow-up

Group messages and the composer now share an 860px reading column instead of
stretching the transcript across the window. Saved and streamed replies use the
same Markdown renderer as direct chat. Message identity stays next to the reply;
copy and message details stay below it. Token counts and execution insights load
inside an optional message panel in both modes. Panels support outside click,
Escape, viewport bounds and dismissal on navigation.

The group composer keeps attachments, responder selection and sending together.
A compact actions menu opens collaboration, room settings, shared knowledge and
prompt editing. Room settings and execution history use dialogs instead of a
second side column. Basic room setup presents the name, topic, participants and
response mode; limits and model options are collapsed and loaded on demand.
Creating a room only requires selecting participants; an empty name receives a
default. Dialog actions remain visible while the settings body scrolls.
Direct-chat permissions and reasoning effort live in the shared settings screen;
permission panels stay inside that screen's focus boundary.

Discussion turn limits are caps, not an instruction to make every participant
speak. Normal routing still chooses a relevant subset; selecting Everyone sends
an explicit responder list. New rooms default to one round with automatic
learning off. The existing idle nine-member room on engine 4317 was updated to
one round, automatic turn limits and automatic learning off using the supported
room API. Its roster and transcript were preserved. This applies the cost policy
without restarting the engine; the source-level cap correction still needs a
future Rust build. No new model requests were made for this follow-up, so token
savings have not been measured.

Read-only TypeScript diagnostics and live browser inspection cover desktop
1440px, wide 2560px and phone 390px layouts, menu dismissal, responder selection,
blank-name creation readiness and visible dialog actions. Production builds and
test suites were not run under the owner's constraint.
