# Dominion Sub3 — Browser account sign-in (2026-11-17)

## End-user experience

Open Connections → Accounts. Click GitHub or Google Connect, then complete the official browser login and provider permission consent. Dominion displays a live status, GitHub device code and the official login URL when supplied by the CLI. There is no end-user OAuth Client ID, Client Secret, PAT or API-key form.

## Credential sources in priority order

1. **Distributor-managed OAuth registration:** When the product operator provisions the registered GitHub/Google OAuth app credentials once through the existing encrypted vault or deployment environment, the standard system-browser login works directly. The user only approves the official provider consent page.
2. **GitHub CLI fallback:** Without that app configuration, Dominion calls the installed official gh binary's browser login (gh auth login --web), then verifies the consented account against the GitHub User API. Existing gh credentials can be adopted without asking the user for a developer key. Token rotation is delegated to gh and pinned to the connected identity.
3. **Google Workspace CLI fallback:** Without a deployment app, Dominion can run the installed gws CLI's browser login with the Gmail, Drive, Docs and Calendar services selected. It securely imports the already authorized grant using gws auth export --unmasked, checks refresh and the provider-reported scopes, then stores only encrypted local credentials.

These CLI fallbacks require the corresponding **official CLI to be installed**. The Google Workspace CLI must have a legitimate working OAuth client; it may require a Google Cloud project and its own first-time setup. The program does not create a cloud/billing project silently or invent an OAuth client ID. Distributors wanting seamless sign-in on arbitrary end-user computers should register and provision their own OAuth app. Google requires sensitive/restricted-scope validation for wider distribution.

GitHub CLI: https://cli.github.com
Google Workspace CLI: https://github.com/googleworkspace/cli

## Execution contracts

- src/api/cli_oauth.rs runs fixed official CLI commands through a bounded login broker, forwards safe login progress, supports cancellation and terminates stuck sessions within ten minutes.
- src/engine/oauth.rs imports and validates CLI credentials and keeps grants in the existing local encrypted vault; token material is never delivered to browser UI or model prompts.
- src/engine/tools.rs supplies account-specific tools to the Rust SDK and chat only after the account is connected and the needed Google scopes are actually granted.
- Disconnect revokes Dominion's authority without logging the owner out of gh or gws. Switching from CLI to normal OAuth removes stale source state.
- External writes (GitHub branch/PR/issue/commit; Gmail send, Drive/Docs changes, Calendar writes) still use owner approvals and existing durable tool receipts.

## Tests and limits

Focused CI includes the no-key frontend contract, official CLI command/authorization-code parser tests, OAuth disconnect regression, GitHub/Google HTTP fixtures, SDK tool selection and frontend typecheck/build. Fixtures are not a substitute for an actual owner's browser consent or Google Workspace production OAuth verification.

The product does not bundle third-party CLI binaries. If neither a distributor-managed OAuth app nor an official CLI with valid OAuth credentials is available, sign-in is not possible until one is provided.
