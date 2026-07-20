# Settings reference

Everything under the **Settings** entry of the module switcher, section by section.

## Account

Change your username or password. Your current password is required to save changes, and changing the password **signs you out of all sessions**.

## Defaults

- **Language** — applies to the whole app immediately (en, fr, de, es, it, pt, zh).
- **Default currency** and **timezone** — the starting values modules use for new items and display.

## Network

Who can reach the app: localhost, LAN, LAN + HTTPS, or public. Covered in detail in [Network & remote access](/config/network).

## Vault {#vault}

One place for the API keys and secrets the app uses on your behalf, instead of pasting the same key into every module that needs it.

A **vault** represents one external service (e.g. *Binance*) and holds named **keys** — `apikey`, `secretkey`, and so on. Create as many vaults and keys as you need; modules then **plug a key in by reference** through a shared picker, wherever a secret is asked for (provider connectors, feed credentials…).

- **Write-only values.** A secret is sealed on save and can never be viewed again — only replaced or deleted. Key *names* stay visible. Everything is encrypted at rest with the app's master key.
- **Unplug before deleting.** Deleting a vault or a key that is still plugged into a module is blocked; remove the reference there first. Each vault shows how many connections use it.
- **Request tracking** is optional and **per vault, not per key** — every key of a vault counts toward the same counter. The limit is informational only (observe-and-display); nothing is ever throttled. It feeds the same view as [API rate](#api-rate).

News feed secrets also accept inline <code v-pre>{{vault.item}}</code> placeholders, resolved by the scheduler at poll time — see [News](/modules/news-research#news).

## Modules

Install and detach modules. Everything ships with the app — installing just makes a module available in the switcher and on the dashboard; nothing is downloaded. Detaching hides it and makes it inaccessible; tick *also delete data* to wipe its stored data too (permanent).

## Manage data

Per-module storage usage (tables, rows, size) with the database total, and a **Wipe** action to permanently delete one module's data (type its name to confirm). Wiping cannot be undone.

## Backup

Ready-to-copy `pg_dump` commands for your deployment, including encrypted variants and restore instructions. See [Backup & restore](/guide/backup-restore).

## Update app

Shows the current version, checks GitHub for a newer one, and lists the update commands to run on the host. See [Updating](/guide/updating).

## Logs

The app's own log store, searchable by message/target. The **capture level** sets the minimum severity written to storage (takes effect immediately) — lower levels capture more detail and use more space. You can clear stored logs here.

## API rate {#api-rate}

An **observe-only** dashboard of outbound calls to external data providers (market data, FX, quotes, feeds), counted per UTC day: request counts per provider, errors, rate-limit responses, published limits where known, and a list of recent rate-limit hits. Nothing here throttles or blocks anything — it exists so you can see how close you are to a provider's free-tier limits.

## MCP

Let AI agents use the app through a controlled gateway. Covered in [AI agents (MCP)](/config/ai-agents).

## Credits

The data sources and upstream projects each module can use — including providers you haven't configured.

## About

Version, project links, and share buttons.
