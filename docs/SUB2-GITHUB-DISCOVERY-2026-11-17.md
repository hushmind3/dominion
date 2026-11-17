# Sub2 GitHub connected repository discovery (2026-11-17)

The model can now perform the full supported connected-account workflow:

1. `account_status`: inspect the already configured GitHub OAuth account.
2. `github_repositories`: list up to 50 visible repos/page with owner, repository,
   default branch and explicit read/write permission; optional local name filter,
   `writableOnly`, pages 1–10.
3. `github_file_read`: read the requested UTF-8 file and get the current SHA.
4. `github_file_commit`: after user tool approval, recheck permissions and
   optimistic SHA, PUT, and re-read the remote file for content verification.

Tokens are taken only from `oauth::fresh_access_token(state,"github")` and sent
to `api.github.com`, not arbitrary MCP servers. Repo discovery is read-only
and marks unknown `push` permission as not writable, never escalates it. A
shared `reqwest::Client` connection pool avoids rebuilding the HTTP client
per GitHub operation. Large accounts can paginate; filters apply **within
each page**, not as a remote full-text repository search.

References: GitHub REST API `GET /user/repos` contract and existing
Sub2 `github_work::read/commit` implementation. No OAuth credentials, DB
records or main-branch files were changed. Real-account end-to-end success
still requires a connected GitHub grant and the user's chosen repository.
