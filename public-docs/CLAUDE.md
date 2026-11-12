# public-docs/CLAUDE.md — the docs site's rules

Applies to everything under `public-docs/`. The repository's own rules (root
`CLAUDE.md`) apply here too.

- **Commands.** `README.md` in this folder lists them and where pages, images
  and static files go. An agent starts the dev server in the background (`npx
  astro dev --background`) and manages it with `npx astro dev stop`, `status`
  and `logs`.
- **Stack.** Astro with Starlight and nothing else: no UI framework, no
  Tailwind, no i18n.
- **Sidebar.** The Reference group lists `reference/` automatically.
- **`reference/mcp-tools.md` is generated; never edit it by hand.**
  `scripts/generate_mcp_reference.py` writes it from a built bundle's catalog
  (`--catalog <bundle>/Contents/Resources/agent-catalog.json`) or a running
  instance's scratch socket (`--socket <scratch socket>`); `--check` reports a
  stale page without writing. Its intro lives in the script and each tool's text
  in the app's catalog (`app/Sources/AgentCatalog*.swift`).
- **`reference/features.md`** is kept current by the README sweep (root
  `CLAUDE.md`, "README").
- **Screenshots.** Screens come from `screens/*.md` through the runner
  (`../scripts/screens-probe/run.py`) and `scripts/import-screens.mjs`. Never
  hand-edit `src/assets/screens/`. Only fixtures in `screens/fixtures.json`
  are allowed.
- **Raw HTML.** `.md` pages render raw HTML, so a literal `<word>` outside code
  swallows text: write it as `&lt;word>` or put it in backticks.
- **Theme.** `README.md` in this folder describes the theme, the component
  overrides and the version badge (`src/site.ts`). Read it before changing
  styles or components, and keep that description there.
- **Not published.** No workflow builds the site, and `astro.config.mjs` sets no
  `site`.
