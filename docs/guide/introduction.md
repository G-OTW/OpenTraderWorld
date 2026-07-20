# What is OpenTraderWorld?

OpenTraderWorld is a **self-hosted web platform for traders and investors**. You install it once with Docker on your own computer or server, open it in a browser, and get a private workspace made of modules: a trading journal, historical market data with charting and backtesting, portfolio and net-worth tracking, a news aggregator, notes, checklists and more.

**Free for everyone — personal or professional. Source-available (FSL-1.1-MIT).** The only thing you can't do is resell it or offer it as a paid service. The guiding principle: *get profitable before spending a dime.*

## Why self-hosted?

- **Your data stays yours.** Trades, portfolios, notes and journal entries live in a PostgreSQL database on your machine — not on someone else's server.
- **Private by default.** After install, the app listens on `localhost` only. Exposing it to your LAN or the internet is an explicit choice you make in [Settings → Network](/config/network).
- **No subscription.** The core tools cost nothing to run. Some modules can optionally use external data providers (many with free tiers) — you bring your own API keys.

## How it works

One `docker compose` stack, four services:

| Service | Role |
|---|---|
| **core** | Rust API server (Axum) — all business logic, scheduler, background jobs |
| **postgres** | PostgreSQL — the only place your data lives |
| **frontend** | SvelteKit single-page app, built once at deploy time |
| **caddy** | Reverse proxy — serves the app, proxies `/api`, handles HTTPS certificates |

The app is **single-user**: one admin account, created at install. There is no multi-tenant mode, sharing, or user management to configure.

Docker is currently the **only supported deployment** — it keeps the install non-intrusive and quick to rebuild ([why, and how to get Docker](/guide/docker)). A native install is possible but not recommended.

## The modules

Modules are feature bundles you install or detach from **Settings → Modules**. Everything ships with the app — installing a module just switches it on. Highlights:

- **[Trading Journal](/modules/journal)** — log trades with templates, fee schedules, multi-currency PnL and full performance stats.
- **[Market data & backtesting](/modules/market-data)** — download OHLCV history from multiple providers, chart it with indicators, backtest rule-based strategies, and run quant risk analytics.
- **[Portfolios & wealth](/modules/portfolio)** — live portfolio tracking, net-worth history, superinvestor 13F holdings, tax estimates.
- **[News & research](/modules/news-research)** — RSS/API news dashboards, economic calendar, a 300k-instrument search catalog.
- **[Notes & organization](/modules/productivity)** — rich-text editor with databases, todos, goals, calendar, reminders, trading routines and mindset check-ins.
- **[AI Agent](/modules/agent)** — built-in chat assistant (bring your own provider) that can act on your data via MCP, with memory, skills and external MCP servers.

See the [full module list](/modules/).

## Next steps

1. [Install OpenTraderWorld](/guide/install) — about 5 minutes with Docker.
2. [Take your first steps](/guide/first-steps) — sign in, pick defaults, install modules.
3. [Configure network access](/config/network) — if you want to reach it from other devices.
