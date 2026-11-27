# DOMINION MCP Connector

Independent SDK v2 tool-provider application, not a model or agent runtime. This standalone Cargo workspace carries the existing Rust MCP transport, 2026-07-28 stateless HTTP, legacy handshake, stdio, live schema checks and session pool. No product database, vault, core, harness or algorithm crate is imported. The copied protocol tests run in this workspace.

Install the independently published OCI package through the common extension manager; start it, open Extension settings, configure servers, then register its tools. Existing host settings are not silently copied. HTTP credentials and stdio environment are stored only in the private extension volume and masked on read. Local stdio executables run inside the isolated container; host paths and arbitrary host executables are not automatically mounted. The SDK token is not inherited by subprocesses. A configuration change requires re-registering its catalog. The call journal refuses to replay a call ID with an uncertain prior outcome.

Use `cargo test --manifest-path Cargo.toml` or `docker build -t dominion-mcp-connector .`. The extension does not invoke an LLM. For source migration to a dedicated repository, copy this directory as-is; the host knows only the generic SDK.

The source repository is `hushmind3/dominion`, branch `branch3`. The public digest-pinned GHCR connector remains under `hushmind3/dominion-rust/mcp-connector`: a GitHub repository rename does not relocate that published artifact. The host recognizes this exact connector alias for the renamed source repository; other repositories, sibling images and mutable tags remain rejected.
