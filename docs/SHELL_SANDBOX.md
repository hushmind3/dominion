# Native Rust shell isolation: optional, honest, fail-closed (2026-11-15)

## Functional boundary

Dominion owns the execution loop, model routing, native tools and recorded
acceptance evidence. Previously, the `workspace_check` tool ran approved
commands on the host. It had a strict program/argv allowlist and 90-second
timeout, but **a permitted test command was not an OS sandbox**.

This implementation adds an **operator-selected, per-task shell isolation**
policy at the Rust admission boundary. It is deliberately precise about its
scope: **model-proposed `workspace_check` commands and independent
acceptance tests only**. File read/edit tools use the existing workspace
containment and permission logic. External MCP calls use their own permission
and network policy. A separate sandbox for those services remains future
work; this feature must not be described as whole-agent isolation.

### One policy across all three surfaces

- **Native Rust SDK:** `TaskInput.shell_isolation: Option<String>`, valid
  `host` (legacy default) or `isolated`. An isolated request without
  `allow_shell=true` is rejected before admission.
- **Public HTTP API:** `POST /api/tasks` and
  `POST /api/tasks/:id/resume` accept `shellIsolation`. The same
  `TaskInput` validation and persisted task options apply to both.
  `GET /api/sandbox` probes the real local isolation backend and returns
  `available`, `backend`, `scope`, and a useful error when unavailable.
- **TypeScript SDK:** `DominionClient.sandboxCapability()`,
  `TaskInput.shellIsolation` and the existing `submit/resume` methods.
- **React product:** a readable selector under **Code check / Advanced
  settings** and readiness feedback; **Connections / Execution
  environment** displays the actual capability. When strict isolation was
  selected but readiness is false, submit/continue controls are disabled.
  A direct API request is also independently protected by the Rust engine.
- **Rust tool dispatch:** `src/engine.rs::tool` and
  `src/engine/verification.rs::audit` both invoke
  `src/engine/sandbox.rs::execute`; no second shell runner remains.
  Shell results record `sandboxed`, `sandboxBackend`,
  `outputTruncated` and exit status, and real acceptance events surface
  isolation proof in the UI.

### Backend selection and installation

```text
DOMINION_SANDBOX_BACKEND=auto       # auto, bwrap, docker, podman
DOMINION_SANDBOX_IMAGE=...          # required for Docker/Podman
```

**Linux Bubblewrap:** installs must provide `/usr/bin/bwrap`. The Rust
engine probes Bubblewrap with a real short isolated command, not merely a
binary-existence check. Shell commands receive a read-only host mount,
one writable workspace bind mount, a separate PID namespace and disabled
network namespace. Common HOME credential directories are masked and
sensitive parent environment variables are scrubbed. A workspace that is
the entire user HOME or filesystem root is not admitted as a writable
sandbox. Note: **other host files may still be readable**; this is primarily
a write and network boundary, not a perfect confidentiality boundary.
Unprivileged user namespaces must be enabled on the host.

**Docker/Podman:** select the backend explicitly, or on a host without
Linux Bubblewrap set `DOMINION_SANDBOX_IMAGE` and use `auto`.
Only a **locally existing** operator-selected image is accepted:
`--pull never`, `--network none`, `--read-only`, `--cap-drop ALL`,
`--security-opt no-new-privileges`, `--pids-limit 128`, a finite
RAM/CPU budget and a single writable workspace bind mount. The child
container receives **no implicit model API keys or host application
environment variables**. Container runtime credentials remain with the
trusted host-side client; they are not automatically passed into the
container. The image must already contain the needed toolchain, dependencies
and any permitted offline build/test resources. Docker is a practical
portable option for Windows/macOS users with an installed container engine.

**Fail-closed guarantee:** when `shellIsolation: "isolated"` is selected,
a missing/failed capability check returns an error; the engine does NOT
retry the command on the host. `host` remains the default for compatibility
with existing scripts and installed dependencies. Tests running in either
mode are bounded by 90 seconds; stdout/stderr are continuously drained with
a strict in-memory capture ceiling.

### API request example

```json
{
  "requestId": "verify-isolated-2026",
  "prompt": "Fix the failing source and run the configured tests",
  "allowWrite": true,
  "allowShell": true,
  "shellIsolation": "isolated",
  "requireEvidence": true,
  "acceptanceChecks": [
    {"program": "node", "args": ["--test", "calc.test.js"]}
  ]
}
```

The original direct-argv allowlist still applies. No string is interpreted
as an arbitrary shell script by the native tool launcher.

## Source-to-outcome trace and tests

1. `runtime.rs::TaskInput::validate/normalized` verifies and persists
   isolation and permissions.
2. `api.rs` submits native tasks unchanged and reports backend capability.
3. `engine.rs::run` reads the persisted mode and passes it to both
   `tool()` and `verification::audit()`.
4. `sandbox.rs::backend/probe_backend/launch/execute` builds and invokes
   the OS-backed sandbox with fixed arguments and bounded asynchronous
   stdout/stderr drains.
5. `store.rs` records tool start/result and acceptance receipts. The
   TypeScript SDK and React UI consume the same HTTP records/events.
6. Tests in `sandbox.rs` exercise backend argument construction,
   missing-backend fail-closed behavior, real host subprocess execution,
   bounded capture for large output, and (when supported) an actual
   Bubblewrap attempt to write outside the workspace.
7. `runtime.rs` rejects invalid or mismatched policy at admission and
   tests the persisted `shellIsolation` representation.
8. `sdk/typescript/tests/client.test.mjs` checks the read-only readiness
   route plus submit/resume propagation.

This is not an evidence-backed claim that any kernel sandbox is impossible
to escape, that every container image is trustworthy, or that a host process
already active at service start is isolated. **Only the identified shell
command path is sandboxed.** A future release should add stronger
resource-group termination of subprocess trees, platform-native Windows
sandboxing, independent RSI evaluator isolation, and isolation of MCP tools.

## Current public source comparisons

The implementation was informed by reading actual upstream implementations:
- Qwen Code: `packages/core/src/sandbox/bwrap-execution.ts` and
  `runtime-shell-policy.ts` (2026) separate policy admission, backend
  probing, namespace launch and explicit limitations.
- Codex: `codex-rs/exec-server/src/sandbox_integrity/bubblewrap.rs`
  resolves actual required executable dependencies before launch, and
  platform-specific sandbox selection is handled separately.
- Claude Code: the 2026 permissions/hooks system distinguishes model prompts
  from deterministic permission enforcement and OS sandboxing.

Dominion implements its own Rust-owned task policy and shell launcher;
it does not embed Qwen, Codex or Claude Code as its agent harness.
