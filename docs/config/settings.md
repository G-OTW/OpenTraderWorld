# Settings reference

Everything under the **Settings** entry of the module switcher, section by section.

## Account

Change your username or password. Your current password is required to save changes, and changing the password **signs you out of all sessions**.

A new password must be at least 12 characters and is refused if it appears in public breach lists. See [Password rules](/config/security#password).

## Security

Two-factor authentication, the trading lock, social sign-in, the browsers currently signed in to your account, and how long a single request may run. Covered in detail in [Account security](/config/security).

## Defaults

- **Language**: applies to the whole app immediately (en, fr, de, es, it, pt, zh).
- **Default currency** and **timezone**: the starting values modules use for new items and display.
- **Exchange in the top bar**: shows that exchange's code in the top bar and whether it is trading now. *None* shows nothing.
- **Show the floating Agent button**: the [Agent](/modules/agent#the-floating-assistant) button in the corner of every page.

## Appearance

The app's **accent colour**, the one used by primary buttons, active states, links and chart highlights. Pick a preset swatch or any colour from the picker; it applies live across the app and is saved for every session. *Reset* puts it back to the theme default.

## Network

Who can reach the app: localhost, LAN, LAN + HTTPS, or public. Covered in detail in [Network & remote access](/config/network).

## Vault {#vault}

One place for the API keys and secrets the app uses on your behalf, instead of pasting the same key into every module that needs it.

A **vault** represents one external service (e.g. *Binance*) and holds named **keys**: `apikey`, `secretkey`, and so on. Create as many vaults and keys as you need; modules then **plug a key in by reference** through a shared picker, wherever a secret is asked for (provider connectors, feed credentials…).

- **Write-only values.** A secret is sealed on save and can never be viewed again, only replaced or deleted. Key *names* stay visible. Everything is encrypted at rest with the app's master key.
- **Unplug before deleting.** Deleting a vault or a key that is still plugged into a module is blocked; remove the reference there first. Each vault shows how many connections use it.
- **Request tracking** is optional and **per vault, not per key**: every key of a vault counts toward the same counter. The limit is informational only (observe-and-display); nothing is ever throttled. It feeds the same view as [API rate](#api-rate).

News feed secrets also accept inline <code v-pre>{{vault.item}}</code> placeholders, resolved by the scheduler at poll time. See [News](/modules/news-research#news).

## Modules

Install and detach modules. Everything ships with the app: installing just makes a module available in the switcher and on the dashboard; nothing is downloaded. Detaching hides it and makes it inaccessible; tick *also delete data* to wipe its stored data too (permanent).

## Manage data

Per-module storage usage (tables, rows, size) with the database total, and a **Wipe** action to permanently delete one module's data (type its name to confirm). Wiping cannot be undone.

Modules that keep dated records (mail, news, calendar events, notifications, webhook events, run history, agent chats, paper trading events, download jobs, fundamentals documents) also have **Clean**: it deletes only what is older than an age you pick in days, weeks, months or years. The dialog says what each one keeps (starred mail, reminders, workflows, candles…). Cleaning cannot be undone either.

## Versioning {#versioning}

Two switches: **Editor files** and **Strategies**, each with the number of versions stored and their size. Turning one on warns that every version is a full copy and the database grows with each one (images and videos are never duplicated). Turning one off asks whether to keep the versions (hidden until you turn it back on) or delete them all. Once an area is on, versioning is switched on per file or per strategy from its history menu: see [Editor](/modules/productivity#editor) and [Backtest](/modules/market-data#strategies-and-custom-indicators).

## Backup & restore

Two tabs, each with a **Backup** and a **Restore** side:

- **Full**: ready-to-copy `pg_dump` and `psql` commands for your deployment, including encrypted variants, plus the status of the automatic backup on instances that run one.
- **Partial**: pick the modules you want, download them as one zip, and load that zip back here or on another instance. Both sides are counted first: what you are taking, per module and per table, and what a file you load holds against what is already here.

See [Backup & restore](/guide/backup-restore).

## Update app

Shows the current version, checks GitHub for a newer one, and lists the update commands to run on the host. See [Updating](/guide/updating).

## Logs

The app's own log store, searchable by message/target. The **capture level** sets the minimum severity written to storage (takes effect immediately). Lower levels capture more detail and use more space. You can clear stored logs here.

## API rate {#api-rate}

A dashboard of outbound calls to external data providers (market data, FX, quotes, feeds), counted per UTC day: request counts per provider, errors, rate-limit responses, published limits where known, and a list of recent rate-limit hits. It exists so you can see how close you are to a provider's free-tier limits.

**This page never throttles anything**: it only observes. The one place a limit is actually enforced is a [connector's own request limit](/config/connectors#request-limits) on the chart's on-demand fetches; everywhere else a limit informs and warns, and the provider remains the one that says no.

## Data connectors

The shared list of market-data provider accounts used by Historical Data, Charts, Watchlists, the Trading Journal, Portfolio Tracker, Quant Tools and Fundamentals: credentials, request limits, and which modules may use each one. Covered in [Data connectors](/config/connectors). The same screen is also available on its own at **/connectors**, and from the connector button inside each data module.

## Brokers {#brokers}

The shared list of **broker accounts** used by the Trading Journal, Portfolio Tracker, Charts and the Tax Calculator: credentials, per-broker settings, and which modules may use each one. Covered in [Broker accounts](/config/brokers). An account only reads until you turn on its **trading** grant; the chart can then place, change and cancel orders and close positions on it. Live trading, risk limits and [paper accounts](/config/paper-trading) are set here too. See [Trading from the chart](/config/trading).

## Market sessions {#sessions}

Named trading windows (start, end, days) in a venue's own timezone, so they follow daylight saving. A chart alert can fire once per session.

## Exchanges {#exchanges}

Each exchange's regular hours in its own timezone, lunch breaks, holidays and half days, and whether it is open now. The calendar is refreshed from the project once a week; **Check now** fetches it on demand.

## Notifications {#notifications}

The shared list of **notification channels**: where the app is allowed to push, created once and reused by every module that notifies.

A channel is a destination **you own**. Each holds one secret, typed here or plugged from the [Vault](#vault), sealed on save and never shown again.

| Channel | What you bring | Secret | Other fields |
|---|---|---|---|
| **Email** | your own SMTP server | password | host, port (587 STARTTLS, 465 TLS), from, to, username |
| **Telegram** | a BotFather bot | bot token | chat id |
| **Slack** | an Incoming Webhook | the webhook URL | none |
| **Discord** | a channel Webhook | the webhook URL | none |

All four are free for the host: you bring the account, the app brings nothing to sign up for. Some **non-secret** fields take a vault item too, the Telegram **chat id** for one, so a channel can be set up without that id sitting in clear in the config.

The modules that can be granted a channel:

| Module | What it pushes |
|---|---|
| **RemindMe** | a reminder that fired |
| **Watchlists** | a price alert |
| **Mailbox** | a mail account that needs attention |
| **Webhooks** | an inbound payload redirected to a module |
| **Historical Data** | a long park, and the end of a download batch |
| **Charts** | a chart alert that fired |
| **Journal** | the market-data enrichment of a new trade, a broker sync that brought trades, a failed sync, a background broker pull that finished |
| **Backtest** | a paper trading fill, or its grouped summary |
| **Portfolio Tracker** | a broker sync that wrote or held back operations, a failed sync, a background broker pull that finished |
| **Fundamentals** | a new period for a series, new filings for a company |
| **Tax Calculator** | a background broker pull for a tax year that finished or failed |
| **Automator** | whatever a `notify` block sends |
| **Security (sign-ins, tokens, exposure)** | not a module: a sign-in from a new address, wrong passwords or codes, agent tokens and gateways, external control, network exposure, sign-in methods, the trading lock, a backup taken with credentials. See [Account security](/config/security#sessions) |

- **Grants, per module.** Every channel names the modules allowed to send to it, or *all*. The check runs server-side: a module that was never granted a channel cannot reach it, and that channel's secret is never even decrypted for it.
- **One switch per channel.** Disabling a channel silences it everywhere without deleting it or its credentials.
- **Test send** before relying on one.

The same screen opens from inside each notifying module, so a channel can be added in the moment without leaving the page you are on. It is deliberately absent from the [MCP](#mcp) catalog: no agent can create a channel or widen a grant.

## MCP {#mcp}

Let AI agents use the app through a controlled gateway. Covered in [AI agents (MCP)](/config/ai-agents).

## External control {#external-control}

Drive the app from a chat channel (Telegram, Slack, Discord). Covered in [External control (chat)](/config/external-control).

## Voice {#voice}

Push-to-talk commands and dictation: the speech engine, the two shortcuts and your voice commands. Covered in [Voice control](/config/voice). The microphone only works over HTTPS or on localhost.

## Credits

The data sources and upstream projects each module can use, including providers you haven't configured.

## About

Version, project links, and share buttons.

## Legal & risk

Three reading pages: **Terms & liability** (the terms of use), **Licence** (the FSL-1.1-MIT licence in plain language; the `LICENSE` file is the authoritative text) and **Trading & data risks** (trading, third-party and cyber risks).
