# OpenTraderWorld — Documentation

This folder is the source of the documentation site, built with [VitePress](https://vitepress.dev)
and deployed to GitHub Pages by `.github/workflows/docs.yml` on every push to `master`.

**Read it online:** https://g-otw.github.io/OpenTraderWorld/

## Working on the docs

```bash
cd docs
npm install
npm run dev      # live preview at http://localhost:5173
npm run build    # production build (output: .vitepress/dist)
```

Layout:

- `index.md` — landing page
- `guide/` — install, first steps, updating, backup, troubleshooting
- `config/` — network modes, settings reference, MCP / AI agents
- `modules/` — one page per module group
- `.vitepress/config.mjs` — nav, sidebar, site options
