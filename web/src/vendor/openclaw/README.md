# OpenClaw plugin UI attribution

MIT-licensed sources from https://github.com/openclaw/openclaw (retrieved 2026-11-28).
The accompanying LICENSE is retained.

- `plugins.css`: `ui/src/styles/plugins.css`.
- `../assets/plugin-prompts-ember-{r,g,b}.png`: `ui/src/assets/` artwork referenced by the original styles.
- `install-action.css`: `ui/src/pages/plugins/install-action.css`.
- `RegistryCategoryIcon.tsx`: React adaptation of `ui/src/components/solid/registry-category-icon.tsx`. SVG artwork is retained; framework imports and the standard icon wrapper use this app's React/Lucide components.
- The plugin card/detail/install components in `pages/extensions` retain the upstream plugin-card, detail-shell, and install-action markup/class structure. Solid reactive bindings are adapted to React, Tailwind layout and Dominion API transport. Branding and agent execution remain Dominion's.

The compatibility host reuses the installed OpenClaw distribution, SDK and plugin lifecycle. It does not emulate its exports using no-op handlers.
