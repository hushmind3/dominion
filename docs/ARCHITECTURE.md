# Dominion Architecture — 2026-11-15

## One owner of execution

~~~text
Main React/Vite workspace ─┐
                            ├─ HTTP / SSE ── native Rust SDK runtime ── model
TypeScript SDK adapter ─────┘                     ├─ SQLite tasks and receipts
                                                  ├─ permissioned tools
                                                  ├─ group discussion
                                                  ├─ video rendering
                                                  ├─ improvement trials
                                                  └─ memory and skills
~~~

The main React/Vite presentation layer is preserved: responsive navigation, visual theme, i18n, reusable controls and existing page source. web/src/pages/NativeWorkspace.tsx replaces API-incompatible flows for the primary active workspaces with the champion native API. The UI is not its own agent runtime.

Vite builds to web/dist. The Rust loopback HTTP service uses ServeDir/ServeFile to serve the built React assets with an SPA index fallback; DOMINION_WEB_DIR overrides the build location. For development, the Vite proxy forwards /api to the Rust server.

## Source ownership

| Surface | Source |
| --- | --- |
| Native Rust SDK, task admission and concurrency | src/runtime.rs |
| HTTP/SSE and static SPA hosting | src/api.rs |
| Persisted execution and knowledge | src/store.rs |
| Model / tool runtime | src/engine.rs |
| Group discussion | src/group.rs |
| Video | src/video.rs, src/veo.rs |
| Isolated improvement experiments | src/improvement.rs |
| Main visual shell and UX controls | web/src/App.tsx, web/src/components, web/src/theme.css |
| Real Rust API page adapters | web/src/pages/NativeWorkspace.tsx |
| External TypeScript transport | sdk/typescript |

## Runtime consistency and remaining migration

The Rust engine, not the UI, controls authorization, model turns and completion. Browser actions call the same task APIs as the external SDK, and per-task event streams replay persistent SQLite records. The frontend displays actual native records and does not invent model completions or video output.

Not every imported main component has a matching champion Rust backend yet. Provider settings, detailed agent configuration, workflow design and some extensions are still missing end-to-end native API integration. The full legacy desktop packaging layer has not been adopted. This source integration does not constitute a compiled Rust build, verified browser E2E run or performance benchmark.

Design references: DeepSeek Harness, Qwen Code and OpenAI Codex Rust sources from 2026. Comparisons informed boundaries and do not grant external engines decision authority.
