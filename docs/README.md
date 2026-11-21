# OpenTraderWorld Documentation

This folder is the source of the documentation site, built with [VitePress](https://vitepress.dev)
and deployed to GitHub Pages by `.github/workflows/docs.yml` on every push to `master` that
touches `docs/` or the workflow itself (or by hand from the Actions tab).

**Read it online:** https://g-otw.github.io/OpenTraderWorld/

**Project website:** https://opentraderworld.com (demo, community docs, suggestions & polls)

## Working on the docs

```bash
cd docs
npm install
npm run dev      # live preview at http://localhost:5173
npm run build    # production build (output: .vitepress/dist)
```

Layout:

- `index.md`: landing page
- `guide/`: introduction, Docker, install, first steps, updating, backup, troubleshooting,
  demo, and `hosting/` (server install recovery)
- `config/`: settings, network, security, social login, connectors, broker accounts, trading,
  paper trading, voice, AI agents, external control
- `modules/`: one page per module group
- `de/`, `es/`, `fr/`, `it/`, `pt/`, `zh/`: translated pages, same tree as the English ones
- `public/`: static files served as is (favicon, screenshots)
- `.vitepress/config.mjs`: nav, sidebar, site options, locales
- `.vitepress/i18n/`: translated nav and sidebar labels per locale
