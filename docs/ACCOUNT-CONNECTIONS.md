# DOMINION GitHub account connection

## Account flow

Open **Connections → Connect accounts (계정 연결)** and connect **GitHub**.
DOMINION opens an authorization window and stores the grant in its local
encrypted vault. Reconnect and disconnect actions appear in the same panel.

If a GitHub OAuth app has not been configured, the **GitHub CLI** browser
authentication path is offered. GitHub OAuth app settings remain available
inside that single panel, rather than on duplicated settings screens.

To configure a GitHub OAuth app, register callback
`http://127.0.0.1:4317/api/oauth/github/callback` and provide
`DOMINION_GITHUB_CLIENT_ID` and `DOMINION_GITHUB_CLIENT_SECRET`
as backend runtime credentials. Set the callback port accordingly if
`DOMINION_PORT` changes. Never place a confidential client secret in the
React build or a publicly downloadable desktop executable.

Supported grant: `repo read:user user:email`, for repository discovery,
file reading, approval-gated edits and commit verification.

## Cloud Google sign-in is a separate feature

The old native Google account connection requested only `openid email profile`
and supplied no Drive/Gmail/Docs/Sheets actions, so its redundant provider
and login controls were removed. **DOMINION Cloud Google sign-in** for model
access and subscriptions continues through the separate Cloudflare gateway.
