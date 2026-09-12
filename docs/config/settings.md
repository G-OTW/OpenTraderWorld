# Settings reference

Everything under the **Settings** entry of the module switcher, section by section.

## Account

Change your username or password. Your current password is required to save changes, and changing the password **signs you out of all sessions**.

## Defaults

- **Language**: applies to the whole app immediately (en, fr, de, es, it, pt, zh).
- **Default currency** and **timezone**: the starting values modules use for new items and display.

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

## Backup

Ready-to-copy `pg_dump` commands for your deployment, including encrypted variants and restore instructions. See [Backup & restore](/guide/backup-restore).

## Update app

Shows the current version, checks GitHub for a newer one, and lists the update commands to run on the host. See [Updating](/guide/updating).

## Logs

The app's own log store, searchable by message/target. The **capture level** sets the minimum severity written to storage (takes effect immediately). Lower levels capture more detail and use more space. You can clear stored logs here.

## API rate {#api-rate}

A dashboard of outbound calls to external data providers (market data, FX, quotes, feeds), counted per UTC day: request counts per provider, errors, rate-limit responses, published limits where known, and a list of recent rate-limit hits. It exists so you can see how close you are to a provider's free-tier limits.

**This page never throttles anything**: it only observes. The one place a limit is actually enforced is a [connector's own request limit](/config/connectors#request-limits) on the chart's on-demand fetches; everywhere else a limit informs and warns, and the provider remains the one that says no.

## Data connectors

The shared list of market-data provider accounts used by Historical Data, Visualization, Watchlists and the Trading Journal: credentials, request limits, and which modules may use each one. Covered in [Data connectors](/config/connectors). The same screen is also available on its own at **/connectors**, and from the connector button inside each data module.

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
| **Visualization** | a chart alert that fired |
| **Journal** | the market-data enrichment of a new trade |
| **Backtest** | a paper trading fill, or its grouped summary |
| **Portfolio Tracker** | grantable ahead of what it will send; it pushes nothing today |
| **Automator** | whatever a `notify` block sends |

- **Grants, per module.** Every channel names the modules allowed to send to it, or *all*. The check runs server-side: a module that was never granted a channel cannot reach it, and that channel's secret is never even decrypted for it.
- **One switch per channel.** Disabling a channel silences it everywhere without deleting it or its credentials.
- **Test send** before relying on one.

The same screen opens from inside each notifying module, so a channel can be added in the moment without leaving the page you are on. It is deliberately absent from the [MCP](#mcp) catalog: no agent can create a channel or widen a grant.

## MCP {#mcp}

Let AI agents use the app through a controlled gateway. Covered in [AI agents (MCP)](/config/ai-agents).

## Credits

The data sources and upstream projects each module can use, including providers you haven't configured.

## About

Version, project links, and share buttons.
