# Native account login with gh / gws (sub2)

The old Connect button required the user to configure a standalone OAuth
client. It now has two real transport choices, selected from backend status:

1. A packaged OAuth client (operator-configured): unchanged GitHub/Google
   browser OAuth flow, with OAuth tokens in DOMINION's vault.
2. No OAuth client configured: the local gh or gws CLI is detected.
   Clicking Connect launches that client's native interactive login in a
   system terminal. DOMINION does not ask for API keys or client secrets.
   Once signed in, GitHub requests use the current gh auth token from the
   system credential store; Google Workspace operations run via the gws
   structured JSON command surface with its own encrypted credentials.
   Neither CLI token is copied into DOMINION persistent storage.

A provider CLI must be installed by the device owner (GitHub CLI gh,
Google Workspace CLI gws). On first use, gws auth setup may need gcloud:
Google's OAuth app/Cloud project provisioning still requires a real Google
Cloud project, even when the wizard creates it. The app reports missing
CLI/setup instead of pretending a functional login exists. Cloud deployments
can still configure their OAuth app once for fully browser-based consent.

CLI work is limited to fixed Google/GitHub API operations, not arbitrary
shell execution or credential export. Google writes retain approval,
read-before-write and post-write verification. Disconnect in DOMINION
disables CLI access only for this app, without logging the OS account out.

Docs: https://cli.github.com/manual/gh_auth_login
Docs: https://github.com/googleworkspace/cli

## Key-free browser fallback (when no registered OAuth app or CLI)

If Chrome/Chromium is present, the Connect button now opens a **visible**
browser window with DOMINION's dedicated persistent Chrome profile at the
fixed Google Drive or GitHub sign-in URL. The user signs in once through the
actual provider website. DOMINION's browser_control agent uses the **same
isolated Chrome profile**, so it can navigate that signed-in website and
interact through the graphical browser workflow after explicit approval.
No API key, CLI install, client ID, client secret, token export, or scraping
of the user's existing browser cookies is required for this browser mode.

Browser mode is distinct from typed API mode: a web login does **not**
grant the REST tools Google Drive/Docs/Sheets access tokens or the native
GitHub API token. Account cards therefore advertise *browser login
available*, never a fabricated OAuth connected status. The user can
install gh/gws or use a packaged OAuth app for typed API actions later.

Opening the visible Chrome profile does not inspect or modify normal Chrome
user profiles. Browser CDP is bound to localhost and existing tab ownership,
tool approval and user policy gates remain in effect. An active headless
browser task is never silently terminated to switch login modes.
