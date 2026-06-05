# XRPL ↔ Axelar Amplifier docs (Docusaurus)

This is the Docusaurus source for [xrpl.docs.axelar.network](https://xrpl.docs.axelar.network).

## Local development

```bash
npm install
npm run start       # http://localhost:3000
```

`npm run start` serves with hot reload.

## Production build

```bash
npm run build       # outputs to ./build
npm run serve       # serve ./build locally on :3000
```

## Layout

- `docs/` — Markdown content. Frontmatter (`title`, `sidebar_position`) controls each page's title and sidebar order. The `overview.md` page is mounted at `/` via `slug: /`.
- `sidebars.js` — Sidebar structure (mirrors the previous `SUMMARY.md`).
- `docusaurus.config.js` — Site config (title, footer, theme, mermaid, prism).
- `src/css/custom.css` — Theme overrides + custom styles (e.g., the ticket-pool diagrams).
- `static/img/` — Logo and favicon assets.

## Deploying to Vercel

The repo-root [vercel.json](../vercel.json) points Vercel at this directory.

Manual Vercel settings, if configuring from scratch:

| Field | Value |
|---|---|
| Framework Preset | Docusaurus (auto-detected) |
| Root Directory | `website/` |
| Build Command | `npm run build` |
| Output Directory | `build` |
| Install Command | `npm install` |

## Migrating content

Source-of-truth docs live in this `docs/` tree. Internal links between pages use `.md` extensions and Docusaurus resolves them at build time. Glossary anchors use the `glossary.md#slug-name` form; the slugs are auto-generated from heading text by github-slugger.

## MDX quirks to keep in mind

Docusaurus parses Markdown as MDX, which is stricter than CommonMark:

- Use `<br />` (self-closing JSX) instead of `<br>`.
- Avoid bare `<word>` in prose — wrap in backticks or escape (`&lt;word&gt;`). Inside code fences (` ``` `) it's safe.
- Inline HTML accepted, but inline `style="..."` attributes do not work as plain HTML strings; use `className=` with CSS instead (see the ticket diagrams in `docs/tickets.md` + `src/css/custom.css`).
- Mermaid diagrams use a ` ```mermaid ` code fence and require `themeConfig.markdown.mermaid: true` (configured here via `@docusaurus/theme-mermaid`).
