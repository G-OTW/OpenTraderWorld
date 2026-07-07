<div align="center">

# OpenTraderWorld

**Your trading workspace, on your own machine.**

[![License](https://img.shields.io/badge/license-FSL--1.1--MIT-blue)](LICENSE)
[![Docs](https://img.shields.io/github/actions/workflow/status/G-OTW/OpenTraderWorld/docs.yml?branch=master&label=docs)](https://g-otw.github.io/OpenTraderWorld/)
[![Docker Compose](https://img.shields.io/badge/deploy-Docker%20Compose-2496ED?logo=docker&logoColor=white)](https://g-otw.github.io/OpenTraderWorld/guide/install)

A free, self-hosted platform bundling the tools traders and investors actually use —
trading journal, market data & backtesting, portfolios, news, planning — with your
data staying yours.

[**Documentation**](https://g-otw.github.io/OpenTraderWorld/) ·
[Install in 5 minutes](https://g-otw.github.io/OpenTraderWorld/guide/install) ·
[Module tour](https://g-otw.github.io/OpenTraderWorld/modules/) ·
[Suggest a feature](https://github.com/G-OTW/OpenTraderWorld/issues)

</div>

---

## Why OpenTraderWorld?

- 🆓 **Free.** No subscription, no paywall, no "pro tier". Free for everyone — personal
  or professional — the guiding principle: *get profitable before spending a dime.*
- 🔒 **Private & self-hosted.** Runs on your computer or server with Docker. No account,
  no cloud, no telemetry. After install it's localhost-only until *you* decide otherwise.
- 🧩 **Modular.** 20+ modules — install only what you use, detach the rest.
- 🔁 **Continuously updated.** New modules and improvements land regularly. The app tells
  you when a new version is out; updating is `git pull` + one rebuild command, and your
  data always survives.
- 🗳️ **Built with its users.** Feature ideas come from people using it — suggest yours,
  vote on others, and watch the roadmap follow. See [Contributing ideas](#-suggest-vote-contribute).
- 🌍 **7 languages.** English, French, German, Spanish, Italian, Portuguese, Chinese.
- 🤖 **AI-agent ready.** A built-in [MCP server](https://g-otw.github.io/OpenTraderWorld/config/ai-agents)
  lets any MCP-compatible agent read or update your modules through a token-scoped
  gateway — off by default.

## What's inside

| | Modules |
|---|---|
| **Trading** | Trading Journal (templates, fee schedules, multi-currency PnL, full stats) · Trading Routines · Mindset check-ins |
| **Market data & analysis** | Historical Data downloads (OHLCV, multi-provider) · Charting with indicators · Rule-based Backtester · Quant Tools (VaR, efficient frontier, position sizing, Kelly) |
| **Portfolios & money** | Portfolio Tracker (daily auto-priced) · MyWealth net worth · Superinvestor 13F portfolios · Tax Calculator · Subscriptions |
| **News & research** | RSS/API news dashboards · Economic Calendar · 300k-instrument search catalog · Resources · Community Docs |
| **Notes & organization** | Notion-style Editor with databases · ToDo · Goals · Calendar · Reminders (email/Telegram/Slack/Discord) · Time Tracker |

Full tour with details: [the module docs](https://g-otw.github.io/OpenTraderWorld/modules/).

## Quick start

Requires [Docker](https://g-otw.github.io/OpenTraderWorld/guide/docker) (macOS, Linux, or Windows).

```bash
git clone https://github.com/G-OTW/OpenTraderWorld.git
cd OpenTraderWorld/deploy
./setup.sh
```

The script asks a few questions, generates strong secrets, builds and starts the stack,
creates your admin account and prints its password once. Open **http://localhost:5454**
and sign in.

Full walkthrough (manual setup, headless servers, LAN/HTTPS/public exposure, backups):
[the documentation](https://g-otw.github.io/OpenTraderWorld/guide/install).

## 💡 Suggest, vote, contribute

This project grows from user feedback:

- **Suggest a feature or module** — open an
  [issue](https://github.com/G-OTW/OpenTraderWorld/issues) describing what you're
  missing and how you'd use it.
- **Vote** — 👍 the existing issues you want most; popular requests get prioritized.
- **Report bugs** — an issue with the symptom and a few log lines is enough.
- **Write for the community** — guides written in the app's Editor can be submitted for
  publication in the shared Community Docs library, readable by every install.

## Under the hood

| Layer | Tech |
|-------|------|
| Backend / core | **Rust** (Axum) — API, scheduler, background jobs |
| Frontend | **SvelteKit** single-page app |
| Data | **PostgreSQL** — the only place your data lives |
| Reverse proxy / TLS | **Caddy** — automatic HTTPS certificates |
| Deployment | **Docker Compose** — non-intrusive, quickly rebuilt |

```
OpenTraderWorld/
├── deploy/        # docker-compose stack, setup.sh, Caddy config
├── core/          # Rust workspace (API, store, scheduler, feeds…)
├── frontend/      # SvelteKit app (modules UI)
├── docs/          # documentation site (VitePress)
├── scripts/       # cross-validation harnesses (backtest & quant math)
├── landing/       # website landing page
└── ProjectSpecs/  # specifications & roadmap notes
```

## Status

Actively developed. All modules are shipping at **v0.1** — usable today, iterating fast.
Check **Settings → Update app** in-app to know when a new version is available.

## License

[FSL-1.1-MIT](LICENSE) (Functional Source License) — free for everyone, including
commercial use, self-hosting at work, modification, redistribution and forks. The only
thing not granted is a **Competing Use**: selling the software or offering it to others
as a paid product or hosted service. Each release automatically becomes **MIT** two
years after publication.
