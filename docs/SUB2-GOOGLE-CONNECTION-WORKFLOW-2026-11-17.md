# Sub2 Google connected-account task flow (2026-11-17)

OAuth for Google now requests real Drive read, Docs editing and Sheets editing
scopes, plus existing profile identity. Existing identity-only grants are
checked against actual token scopes and require explicit reconnect/consent.
No user OAuth token is sent to MCP, GitHub or untrusted hosts.

Native model tools:
- google_drive_search: bounded file search and pagination (Drive v3).
- google_docs_read: bounded document content and revision.
- google_docs_replace: mandatory expectedRevisionId, owner approval,
  Docs batchUpdate, then remote re-read. Unknown outcome requires review.
- google_sheets_read: A1 range values.
- google_sheets_update: user approval, expectedValues preflight, bounded
  values.update, remote re-read. No automatic retry of uncertain effects.

Tools are projected only when requested by ordinary room/Home work, and
writes use the existing native approval/recording policy. The backend-only
operator can supply DOMINION_GOOGLE_CLIENT_ID / SECRET and
DOMINION_GITHUB_CLIENT_ID / SECRET once to make end-user login one click.
The application's Google consent screen and API enablement still need
configuration by its operator. No credentials are embedded in frontend JS.
