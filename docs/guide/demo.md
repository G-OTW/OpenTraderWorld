# Demo mode

There is a public sandbox at **[demo.opentraderworld.com](https://demo.opentraderworld.com)**: the real app, seeded with sample data, shared by everyone, and reset every **15 minutes**. Nothing to install, nothing to sign up for; you land signed in as `demo`.

This page explains what that mode is, so you know what you're looking at, and so you can run one yourself if you want to show the app to someone.

## What it is

Demo mode is a posture the backend adopts when it starts with `OTW_DEMO=1`. **It is never enabled implicitly**: a normal install is untouched by everything below.

- **The database resets on the quarter hour.** The seed is restored from a template, so anything you change is gone at :00, :15, :30 or :45. The in-app banner counts down to the next one.
- **You are signed in automatically** as the `demo` account. There is no password to guess, and no account to create.
- **Everyone shares one database.** Other visitors' trades, notes and conversations are visible, and yours are visible to them. Don't type anything you wouldn't publish.

## What is blocked

The gate is **default-deny**: a request must match an explicit allowlist or it is refused with `demo_disabled`. A route nobody thought about is closed, not open, the same rule the [MCP catalog](/config/ai-agents) follows.

Broadly:

| Blocked | Read-only | Full |
|---|---|---|
| Setup, logout, account & password, network, backup, update, data wipe, module install/detach, inbound webhooks, the MCP endpoint itself, notification channels, FinanceDatabase install, provider downloads | Data connectors, feeds, files, managers' portfolios, MCP settings & tokens, API rate, stored datasets, agent providers, memories and skills | Journal, backtest, quant, portfolios, calendar, todos, goals, editor & databases, prompts, resources, subscriptions, taxcalc, time, dashboard, search, agent chat |

So you can log a trade, run a backtest and talk to the assistant; you cannot change the network mode, mint a token, take the database, or make the box fetch from a metered provider on your behalf.

## Per-visitor quotas

The expensive endpoints cost real money, so each carries **two** budgets: a per-visitor slice, and a global ceiling on top. Per-IP alone wouldn't cap the spend; global alone would let one scripted visitor lock everyone out.

| | Per visitor | Across the demo | Window |
|---|---|---|---|
| **Agent runs** | 3 | 8 | 10 minutes |
| **Agent runs** | 10 | 40 | 24 hours |
| **Backtests & sweeps** | 10 | 30 | 10 minutes |

The assistant runs on a **shared key pinned to a free model**, resolved at boot; long-term memory and external MCP servers are off.

## Running your own

```bash
OTW_DEMO=1        # in the core service's environment
otw-core --seed-demo   # once, against a scratch database
```

The seed is public: it ships in the repo, contains no secrets, and short-circuits if a `demo` user already exists. The reset is a `CREATE DATABASE … TEMPLATE` restore driven from the host, not by the app.

::: warning Don't point demo mode at your data
The seed writes into whatever `DATABASE_URL` names, and the reset restores over it. Use a scratch database.
:::
