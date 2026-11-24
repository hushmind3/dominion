# DOMINION independent plugin package v1

A plugin is an independently maintained application distributed as an immutable OCI image, not a Rust library or model-generated install command. Publish a public GitHub repository with topic `dominion-plugin`; put `dominion-plugin.json` on its default branch. The v1 package manager resolves the commit and validates the manifest, then lets an owner install directly without an LLM. Existing generic container lifecycle manages the application after installation.

Required fields (JSON, no extra keys):
- schemaVersion: 1
- name, description, version: bounded strings
- image: ghcr.io/<owner>/<repo>@sha256:<64 hex characters>, a published image digest for that repository
- network: boolean
- port: optional TCP port (requires network=true)

A manifest is not proof of runtime compatibility or trust. The installer pins the reviewed manifest commit and digest, does not execute arbitrary source or install scripts, and creates a stopped, bounded container with /data and an optional loopback service. Docker must be available. Repeated installations of the same repository share a stable ID. Updates remain explicit via managed container updates. For host SDK/task requests use the existing /dominion/v1/request and /dominion/v1/result protocol; this manifest does not grant model, tool, or admin credentials. Other DOMINION branches can implement this same wire format independently. They do not yet implement this market merely because this document exists.


Application implementations are maintained independently. The host never includes an application-specific build recipe. Legacy installations retain their data; install a published replacement using the market.
