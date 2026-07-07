# News & research

## News {#news}

A self-hosted news aggregator. Build **dashboards** (e.g. *Crypto*, *Macro*), add **sources** to each, and let the scheduler poll them in the background.

### Sources

- **RSS / Atom** — paste a feed URL, done.
- **API (JSON)** — for anything without RSS: set the endpoint, method, headers and query params, then map JSON paths to item fields (items array, title, URL, date, summary, unique id for dedup). API keys go into per-feed **secrets**, stored encrypted and referenced as <code v-pre>{{secret:NAME}}</code> in headers or params — they are never shown again.

Each source has its own **poll interval**; duplicate sources are detected so the same feed isn't fetched twice across dashboards. Start/stop polling per dashboard, or refresh a source on demand.

### Reading

Filter items by search, source, type and date range; compact or full view; optional 60-second auto-refresh with a "{n} updates — click to load" banner. A news widget can also sit on your dashboard home page.

## Economic Calendar {#economics}

Upcoming macro events — central-bank decisions, CPI prints, employment data — in a calendar view, so you know what's ahead of your session. One click adds a reminder for an event.

## FinanceDatabase {#findb}

A searchable catalog of **300,000+ instruments**: equities, ETFs, funds, indices, currencies and cryptocurrencies.

On first use you **install the catalog** (a one-time ~15 MB download, imported in the background). After that it lives locally — **searches never touch the network**. Search by symbol or name, filter by asset type and attributes, and star instruments into **favorites**, organized in folders with notes (e.g. a *Watchlist* folder).

## Resources {#resources}

A bookmark library for trading books, articles, videos and tools: name, optional link, description, organized in categories, displayed as cards or a list. Simple on purpose.

## Community Docs {#community-docs}

Guides written by the community, synced from the project's website feed and **readable offline** inside the app. Browse by category, search, and star favorites.

You can contribute: write a document in the [Editor](/modules/productivity#editor) and use **Submit for publication** — it goes to a review queue and appears in everyone's library once approved.
