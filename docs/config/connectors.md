# Data connectors

Every module that reads market data — [Historical Data](/modules/market-data#histdata), [Visualization](/modules/market-data#histviz), [Watchlists](/modules/portfolio#watchlists) — draws from **one shared list of connectors**. A provider account is created once and granted to the modules that may use it.

Manage them in **Settings → Data connectors**, on the standalone **/connectors** page, or from the connector button any data module puts next to its provider picker. All three render the same screen.

## What a connector is

A **connector is a named instance of a provider**, not the provider itself. Four things belong to it:

- **the provider** — Binance, Yahoo Finance, EODHD…;
- **its credentials** — typed in, or plugged from the [Vault](/config/settings#vault). Write-only: the app only ever knows *which* secret names are set;
- **an optional request limit** — a maximum number of calls per period;
- **the modules allowed to use it** — one or more, or *all modules* (a wildcard that also covers data modules added in future versions).

Several connectors of the same provider can coexist. That is the point: a read-only key for charting and a separate key for bulk downloads, each with its own limit, each granted to a different module.

## Providers

| Provider | Credentials | Asset types | Symbol search | Live stream |
|---|---|---|---|---|
| **Binance** | none | crypto | yes | yes |
| **Kraken** | none | crypto | yes | yes |
| **Coinbase** | none | crypto | yes | yes |
| **Yahoo Finance** | none | equity, ETF, index, crypto | yes | — |
| **Alpha Vantage** | `api_key` | equity, ETF, crypto, FX | yes | — |
| **EODHD** | `api_key` | equity, ETF, FX, crypto | yes | — |
| **Alpaca** | `api_key`, `api_secret` | equity, crypto, option | yes | — |
| **Massive (Polygon.io)** | `api_key` | equity, ETF, option, future, crypto, FX, index | yes | — |

The keyless ones work the moment you create the connector. Each provider row links to its own API documentation and carries a note on its rate limits.

## Create one

1. **Add connector**, pick the provider, and name it — the name is what module pickers show, so *Binance — charts* beats *Binance 2*.
2. Fill the credentials the provider requires, or pick them from the Vault. Keyless providers skip this.
3. Choose the **modules** it serves. Opened from a module, the new connector is granted to that module only; opened from Settings or `/connectors`, it is granted to everything.
4. Optionally set a **request limit** (see below).

A connector missing a required credential is shown as *needs credentials* and is skipped by every module until you set it.

## Module grants

The grant list is **server-side**: a module asking for a connector it was never given is refused, so a checkbox that only lived in the browser would have been decoration. Ticking every data module collapses back to the *all modules* wildcard, which keeps future data modules covered.

The grantable modules today are **Historical Data**, **Watchlists** and **Visualization**.

## Request limits

A limit is a count of outbound calls per **day**, **hour** or **minute**, tracked per connector.

- Everywhere else in the app it is **observe-only** — it feeds the counters in [Settings → API rate](/config/settings#api-rate) and warns you, but nothing is throttled.
- On the chart's on-demand fetches (`/api/histviz/series`) it **blocks**: once the connector is at its limit the window comes back with the bars already stored and a *request limit reached* notice, instead of quietly burning through a metered plan.

Leave the limit off if you'd rather let the provider be the one that says no.

## Where connectors are used

- **Historical Data** — the download form's provider list, and the symbol lookup.
- **Visualization** — the Data tab searches every connector granted to the chart at once, and the chart's live stream picks a stream-capable one.
- **Watchlists** — the quote source of a list, or of a single symbol. CoinGecko and Yahoo remain available with no connector at all.

::: tip Upgrading from the old per-module settings
Historical Data's *Settings* tab and Watchlists' *Sources* tab no longer exist — they were two disconnected copies of this screen over two disconnected lists. Accounts created in either one are now connectors here, each still granted to the module it came from, so nothing changes reach on upgrade. Names are globally unique again: a name that existed in both lists is kept once and the other renamed `<name> #2`.
:::
