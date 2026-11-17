# Sub2 GitHub tasks from connected accounts

The native chat tool catalog now supports complete common repository work:
github_repositories -> github_repository_activity ->
github_branch_create -> github_file_read -> github_file_commit ->
github_pr_create. github_issue_create is a typed issue mutation.

The model provides repository and task semantics rather than arbitrary REST
paths. The Rust adapter checks connected OAuth, actual repository push
permission, branch naming, and user approval for each write. It fetches
remote issues/PRs/branches after mutations and returns either a verified
receipt or an explicit unknown-effect/review marker. Unknown writes are not
blindly retried. Original generic github_api remains for less common tasks,
under its existing mutation approval policy. No OAuth token is forwarded
to a third-party MCP server.
