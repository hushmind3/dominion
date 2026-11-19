# DOMINION account connections

## User flow

Open **Connections → Connect accounts (계정 연결)** and select **GitHub** or **Google**.
The app opens the provider sign-in in the system browser (native) or in a browser
popup (web). After one approval, the OAuth callback stores the grant in the local
encrypted vault. The connection panel refreshes from the event stream or when
the app regains focus. **Reconnect** and **Disconnect** are available in the same tab.

End users do **not** register an OAuth application or enter a Client ID/Secret.
The separate *OAuth app settings* tab remains as an advanced compatibility path
for local developers operating their own OAuth app.

## Distribution setup (one-time, application publisher)

The application publisher must register its own OAuth clients and provision the
following **backend runtime** environment variables before starting DOMINION:

| Provider | Environment | Registered application |
| --- | --- | --- |
| GitHub | `DOMINION_GITHUB_CLIENT_ID`, `DOMINION_GITHUB_CLIENT_SECRET` | GitHub OAuth App |
| Google | `DOMINION_GOOGLE_CLIENT_ID` | Google OAuth Desktop application |
| Google optional | `DOMINION_GOOGLE_CLIENT_SECRET` | Only when required by the registered client |

At the default `DOMINION_PORT=4317`, the local callback endpoints are:

- GitHub: `http://127.0.0.1:4317/api/oauth/github/callback`
- Google: `http://127.0.0.1:4317/api/oauth/google/callback`

If the port changes, update the registered callback details accordingly. The
Google Desktop-app OAuth flow supports localhost/loopback redirects with PKCE.
On GitHub, use an application flow with a confidential client secret; **never**
ship the GitHub client secret inside the static React/Vite bundle, repository,
or publicly downloadable desktop executable. If distributing a public binary,
provision a secure OAuth broker or manage credentials in an admin-controlled
server environment. The client ID alone cannot complete a GitHub OAuth
authorization-code exchange.

The runtime reads configured application credentials first, falling back to
locally encrypted legacy administrator credentials where available. The API
exposes `configured` and `credentialSource`, never the credentials themselves.
When no application credentials exist, the Connect button shows an actionable
configuration error instead of pretending the account is connected.

## Permissions and current capabilities

- GitHub: `repo read:user user:email`; connected-account repository discovery,
  file reads, SHA-bound edits with approval, commit/read-back and REST API calls.
- Google: `openid email profile` identity sign-in and token storage only.
  **Google Drive, Gmail, Docs, Sheets and Calendar are not wired to this grant
  as action tools.** Their APIs require separate scopes, consent and tools.

Access tokens remain in the DOMINION local vault. Google access tokens are
refreshed when expiring, when a refresh token was granted. A provider can revoke
access or require a new login; this is why Reconnect is retained.

## Browser and native behavior

- Browser: opening the login popup is triggered synchronously by the click;
  pop-up blocking is reported to the user.
- Desktop: the backend opens the OS browser without exposing the OAuth secret to
  the web UI.
- Callback: checks the provider-bound OAuth state and exchanges the code server
  side; emits an `oauth_config` event so the account tab updates.

The SDK/provider model login is independent of these **service-account** grants:
signing in to a model does not confer Google Drive or GitHub repository scopes.
