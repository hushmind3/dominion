# Plugin host

The React plugin directory combines public ClawHub categories, MCP registry entries, saved connections, Dominion SDK apps and native features. The Rust host uses the official OpenClaw runtime for compatible packages. Catalog metadata and artwork are fetched separately from installation; browsing does not execute package code.

## Runtime and isolation

An installed OpenClaw SDK and its paired Node runtime are detected automatically. If absent, Dominion provisions private Node 24 LTS (archive SHA-256 checked against nodejs.org SHASUMS256) and OpenClaw 2026.11.10. Nothing is installed globally. Explicit test overrides are `DOMINION_OPENCLAW_ROOT` and `DOMINION_PLUGIN_NODE`.

State lives under `DOMINION_DATA/plugin-host`: a private `profile`, `workspace`, dependency installation and cache. The gateway binds loopback with a newly generated token, receives a restricted environment and uses its own configuration. Existing OpenClaw settings, bot accounts and running gateways remain independent. Stop terminates only the owned child. Installed inventory is cached for display while stopped; enabling starts the host.

## User flow

A card installs its exact catalog release through `plugins.install`. Persisted receipts display actual installer stages. Details expose enable/disable, removal, schema-driven settings, included features and README. Failed or interrupted installs retain their outcome for inspection; unknown writes are never blindly retried. Only explicit pre-admission rejections are eligible for bounded retry.

The question pane uses the selected plugin context and the regular Dominion agent, provider selection, cancellation, approvals and transcript. Management operations and plugin tool calls use the existing MCP execution gate. The pane makes no model request until the user sends a message. Account/API credentials still need the appropriate plugin configuration.

## Validation and limits

The ignored Rust integration test executes a local multi-surface fixture through the real SDK: install, service startup, tool invocation, hook, configuration reload, disable, removal and stop. It has passed with the existing 2026.10.8 SDK and official 2026.11.10. CI provisions the pinned SDK independently. Run with both runtime overrides and `cargo test --locked --bin dominion-rust plugin_host::tests -- --ignored --nocapture`.

Chromium tests exercise directory categories, lifecycle controls, progress receipts, full-workspace details and selected-plugin agent chat against a real Dominion backend; catalog and install responses are deterministic fixtures in that browser test. Separate screenshots use live public metadata. Public third-party packages, external accounts, provider authentication and arbitrary plugin-specific interfaces have not all been tested.

Original OpenClaw Solid markup and styles are adapted to React components; MIT attribution is in `web/src/vendor/openclaw`, and the distributed license is `web/public/licenses/openclaw.txt`.
