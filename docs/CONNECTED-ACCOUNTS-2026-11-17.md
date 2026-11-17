# Connected Google and GitHub accounts — Dominion Sub3 (2026-11-17)

## Connect

Open **Connections → Accounts**. Select **GitHub** or **Google** and sign in through the system browser. If the deployment did not register an OAuth app, choose **OAuth 앱 설정**, enter its Client ID / Secret, then **저장 후 연결**. This is a normal OAuth consent flow, not a password scraper.

A deployment can set registered application credentials once:

| Account | OAuth configuration environment variables |
| --- | --- |
| GitHub | DOMINION_GITHUB_OAUTH_CLIENT_ID, DOMINION_GITHUB_OAUTH_CLIENT_SECRET |
| Google | DOMINION_GOOGLE_OAUTH_CLIENT_ID, DOMINION_GOOGLE_OAUTH_CLIENT_SECRET |
| YouTube, if separate | DOMINION_YOUTUBE_OAUTH_CLIENT_ID, DOMINION_YOUTUBE_OAUTH_CLIENT_SECRET; falls back to Google |

The developer must register a local callback URI with each provider. Example GitHub callback: http://127.0.0.1:PORT/api/oauth/github/callback and Google callback: http://127.0.0.1:PORT/api/oauth/google/callback, where PORT is the actual DOMINION server port. Nothing is preconfigured with a shared public client secret.

Google requires consent to Gmail read/send, Drive, Docs and Calendar access. A previous Google identity-only connection must reconnect before the agent can use these tools. For production distribution, Google may require sensitive/restricted-scope app verification or a user allowlist. Enable the Workspace APIs in the Google Cloud project.

## Agent operations

**GitHub**: github_work_read can list or search repositories and list branches, pull requests, issues and CI runs. github_work_write creates a branch, PR or issue, and reads the remote object back. Existing github_file_read/github_file_commit perform SHA-checked source reads and branch-specific commits with remote verification. Mutations require owner approval. No automatic merge occurs.

**Google**: google_workspace_read exposes Gmail message search and read, Drive file search and bounded text retrieval, Google Docs document text and Calendar event reads. google_workspace_write sends text Gmail messages; creates and updates Drive plain text files; appends Docs text; and creates, updates or deletes Calendar events. Mutations require owner approval. Dates must use RFC3339 time zones.

Illustrative GitHub tool arguments:

~~~json
{"operation":"search_repositories","query":"dominion"}
{"operation":"create_branch","owner":"owner","repo":"repo","from":"main","branch":"feature/fix"}
{"operation":"open_pr","owner":"owner","repo":"repo","head":"feature/fix","base":"main","title":"Verified fix","body":"Tests passed"}
~~~

Illustrative Google tool arguments:

~~~json
{"service":"gmail","operation":"search","query":"from:team@example.com"}
{"service":"gmail","operation":"send","to":"team@example.com","subject":"Update","body":"Done."}
{"service":"calendar","operation":"create","summary":"Planning","start":"2026-11-18T09:00:00+09:00","end":"2026-11-18T10:00:00+09:00"}
~~~

## Integrity and boundaries

Credentials remain in the encrypted local vault, and access tokens are not copied to model prompts. Account tools are offered only when the matching OAuth connection and required permissions are present; disconnecting hides them from subsequent sessions. All external writes require existing owner approval and produce tool receipts.

GitHub branches, PRs and issues are verified through independent GET requests; conflict responses are reported instead of forced writes. Drive text edits require a previously observed version and subsequent byte verification (the version precheck is not an atomic remote CAS). Docs currently supports append, not arbitrary document replacement. Gmail sends return a provider-acknowledged message ID. External effects are never silently retried when the result is uncertain.

## Validation

The focused Rust SDK workflow uses real localhost HTTP fixtures to check GitHub repo discovery/search, branch/PR/issue creates, duplicate branch rejection, CI reads, Gmail search/read/send, Drive search/create, Docs read/append, Calendar list/create, OAuth scope boundaries and write-approval policy.

Those tests do not use the owner's live tokens. Actual external account access depends on provider permissions, OAuth app registration, user consent and successful provider calls. The tests are not evidence of real GitHub/Google user-account actions.
