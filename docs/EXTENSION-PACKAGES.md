# DOMINION extension packages (SDK contract v1)

Extension packages are optional products, not part of the core, MCP or model catalog.
The host uses the independent `dominion-extension-manager` crate and the
`dominion-agent-sdk::plugin_protocol` contract. MCP remains its own connector.
Hugging Face remains a model source. Neither supplies the extension catalog.

A registry is an owner-selected HTTPS JSON document:
```json
{"schemaVersion":1,"items":[{"sdkVersion":1,"id":"example","name":"Example","version":"1.0.0","description":"Echo example","runtime":"node","artifact":{"url":"https://your-release-host.example/entry.mjs","sha256":"<actual 64 lowercase hex SHA-256>"},"tools":[{"name":"echo","description":"Echo text","parameters":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}}]}]}
```
This is a schema example, not an available registry entry. Publish your own
registry and artifact; add its URL under Connections → Extensions.
`runtime` is node, python or native. Native requires explicit targets such as
macos-aarch64, linux-x86_64, windows-x86_64. Scripts must be bundled/self-contained
and require an already installed runtime (node, python3/python). No arbitrary
npm/pip package installation, dependency resolution, binary loading or sandbox
is implied. The manager downloads a single immutable entry, verifies SHA-256,
and installs disabled. Updates require review and activation again.

Invocation: one JSON request on stdin, close stdin, one JSON response on stdout:
```json
{"protocol":"dominion.extension.v1","id":"request-id","tool":"echo","arguments":{"text":"hello"}}
{"protocol":"dominion.extension.v1","id":"request-id","ok":true,"result":{"text":"hello"}}
```
Errors: `ok:false,error:"description"`. A missing/malformed receipt after
dispatch is outcome-unknown; the harness must not automatically replay it.
No shell interpolation, no application credentials in child environment.
This is process separation, not a security sandbox. The owner must trust code.
Approval is tied to a manifest revision. Chat and group agent execution share
the same tool discovery, approvals and receipt pipeline. Ordinary chat remains
tool-free. SDK panel runs opt in with extensionsEnabled separately from mcpEnabled.

The registry and installed package APIs use /api/extensions/*, not /api/mcp/*.
Public repository discovery is enabled by default. Publish a public repository
with topic `dominion-extension` and a root `dominion-extension.json` containing
one Manifest object (the item inside the registry example above). Search uses
the public GitHub repository API, then reads each matching manifest. It never
installs arbitrary repositories, and requires review and exact manifest
re-validation before downloading the SHA-256-pinned artifact. GitHub is an
optional discovery adapter: disable it in Search sources and add independent
HTTPS registries instead. No fake or built-in product listings are supplied.
Public discovery is bounded to 20 repositories per query and can encounter
public API rate limits; registry errors are shown separately from empty results.

## Offline or private distribution

Build a single installable package (no public registry required):
```sh
node scripts/package-extension.mjs manifest.json entry.mjs example.dominion.json
```
The command verifies the declared SHA-256 and packages the manifest and bytes
as a versioned JSON bundle. In the extension manager choose **Install from
file**. This imports up to 8 MiB of executable bytes (12 MiB JSON body cap)
using the same verification path as remote installation. A file import uses
a separate local package identity; it cannot replace a package installed
from another publisher or registry. A local version update requires a new
file and activation. Scripts and executable artifacts are never started by
installation. Node/Python runtime availability is required at execution.
