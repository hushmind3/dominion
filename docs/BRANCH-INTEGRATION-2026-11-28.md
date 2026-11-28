# Branch integration: desktop experience and shared execution

Base: branch4 at 0a75ba620. This is a selective integration; the other branches remain independent.

| Source | Adopted | Existing boundary retained |
| --- | --- | --- |
| branch1 | Cloud app sessions, approval/revocation UI, TypeScript Cloud client | Cloud credentials remain scoped; deploying the schema/worker is a separate operation |
| branch2 | Protocol-safe bounded tool history projection; text/code attachment interaction | Original transcripts and receipts remain durable; complete latest tool-call/result groups remain exact |
| branch3 | Receipt-derived failure guidance and conversation draft preservation | Hints only suggest advertised inspection tools; they grant no authority and do not add another execution loop |
| branch4 | Native engine, approvals, run journals, SDK apps, desktop packaging | Existing effect and recovery rules remain authoritative |

## Desktop frontend

React/TypeScript components use existing Tailwind styles, primitives, modal focus management and API transport. Ctrl/Command K opens feature search. Plugin access is visible in expanded and collapsed conversation navigation. SDK app management has search and enabled-state filtering. Attachment drafts belong to their conversation; switching away and back restores only the correct attachments. Failed uploads retain the files and draft for retry. Private text drafts are kept in memory, not browser persistence.

The old MCP-only discovery page is replaced by one plugin directory. There is no user-facing protocol filter. ClawHub supplies category labels/order, featured/trending shelves, package versions, package details and cursor pages. MCP registry entries and registered connections use the same cards and details. A category request fetches the actual category feed rather than filtering the initial overview. Searches and pagination are tied to their request identity.

The public ClawHub metadata proxy is read-only, pins the upstream origin/routes, bounds responses, caches successful reads for five minutes and observes rate-limit backoff. It never downloads or executes a package. Details display bundled tools, skills, channels, connections, configuration names and documentation from the selected package release.

References: https://github.com/openclaw/clawhub/blob/main/docs/http-api.md and https://github.com/openclaw/openclaw/blob/main/src/plugins/api-builder.ts . No upstream implementation was copied into the frontend.

## Compatibility status

Discovery does **not** imply execution compatibility. Existing MCP connections and Dominion SDK apps continue to use their actual supported installation, configuration, process and approval paths. MCP additions start disabled. ClawHub-native code/bundle plugins do not get a pretend installation button: their details state that an OpenClaw-compatible host connection is required.

Full OpenClaw compatibility remains unfinished. Its target includes the complete plugin API, not a deliberately restricted subset. The plugin host must provide registration/lifecycle, tools, skills, channels, model/media providers, hooks, memory/context, HTTP/command surfaces, services and runtime integrations, then map those operations into Dominion's real permissions, cancellation and durable outcomes. A same-named no-op SDK export does not establish compatibility. The current Dominion SDK is an external-app contract, not that complete host.

Mobile layout work, new voice features, an alternate agent engine and public releases are outside this integration. Existing responsive behavior remains available for desktop window resizing.

## Validation

- Rust workspace tests, including SDK consumer and recovery tests.
- TypeScript SDK and Cloud tests.
- Frontend typecheck, production build, existing static regressions.
- Real Chromium + Rust backend flow: model routing, chat persistence, draft/attachment isolation and rejected-upload retry, feature search, server-defined plugin categories, category cursor traversal, multi-surface plugin details, disabled-by-default MCP registration, SDK app install/process lifecycle, scoped credentials and tool approvals.
- Separate live registry smoke and desktop screenshots use actual public metadata, not the deterministic browser fixtures.

No Cloud worker deployment, database migration or public release was performed. Model-quality/performance ranking has not been measured by this integration.
