# Data connectors

Every module that reads market data draws from **one shared list of connectors**. A provider account is created once and granted to the modules that may use it.

Manage them in **Settings → Data connectors**, on the standalone **/connectors** page, or from the connector button any data module puts next to its provider picker. All three render the same screen.

## What a connector is

A **connector is a named instance of a provider**, not the provider itself. Four things belong to it:

- **the provider**: Binance, Yahoo Finance, EODHD…;
- **its credentials**: typed in, or plugged from the [Vault](/config/settings#vault). Write-only: the app only ever knows *which* secret names are set;
- **an optional request limit**: a maximum number of calls per period;
- **the modules allowed to use it**: one or more, or *all modules* (a wildcard that also covers data modules added in future versions).

Several connectors of the same provider can coexist. That is the point: a read-only key for charting and a separate key for bulk downloads, each with its own limit, each granted to a different module.

## Providers

| Provider | Credentials | Asset types | Symbol search | Live stream |
|---|---|---|---|---|
| **Binance** | none | crypto | yes | yes |
| **Binance USDⓈ-M Futures** | none | crypto | yes | yes |
| **Bitget** | none | crypto | yes | yes |
| **OKX** | none | crypto | yes | yes |
| **Kraken** | none | crypto | yes | yes |
| **Coinbase** | none | crypto | yes | yes |
| **OANDA** | `api_token` (+ account id) | FX and CFDs | yes | no |
| **Yahoo Finance** | none | equity, ETF, index, crypto | yes | no |
| **Alpha Vantage** | `api_key` | equity, ETF, crypto, FX | yes | no |
| **EODHD** | `api_key` | equity, ETF, FX, crypto | yes | no |
| **Alpaca** | `api_key`, `api_secret` | equity, crypto, option | yes | intraday |
| **Massive (Polygon.io)** | `api_key` | equity, ETF, option, future, crypto, FX, index | yes | intraday, paid plan |
| **TradeStation** | `client_id`, `client_secret`, `refresh_token` | equity, ETF, option, future, index | by symbol | no |
| **FOREX.com (StoneX)** | `username`, `password`, `app_key` | FX and CFDs | yes | no |
| **Capital.com** | `api_key`, `identifier`, `api_password` | FX, indices, shares, crypto (all CFDs) | yes | yes |
| **Interactive Brokers** | none (host + port) | equity, ETF, crypto, FX, index, future, option | yes | intraday |

The keyless ones work the moment you create the connector. Each provider row links to its own API documentation and carries a note on its rate limits, and a connector that streams also carries a note on what live costs there.

### Live streaming

Live reach is narrower than download reach, and deliberately so.

- The crypto exchanges publish a candle channel per interval, so **every** timeframe they download they also stream, daily included: on a 24/7 market the daily candle *is* the epoch day. Bitget and OKX anchor their own daily and weekly candles on Hong Kong midnight, so both the download and the live feed ask for their UTC-aligned variants instead, and a series downloaded there lines up with one downloaded anywhere else.
- **Three providers do not stream here.** OANDA and TradeStation publish live prices over a long-lived HTTP response, and FOREX.com over Lightstreamer; none of the three is the WebSocket the live charts speak. Their history downloads and their account side work; the live candle does not.
- Alpaca, Massive and Interactive Brokers publish one grain each (one-minute bars, one-minute aggregates and five-second bars respectively) and the chart's timeframe is folded from it. That makes every **intraday** timeframe available and leaves **daily and weekly to the download**: an equity session is not 1440 epoch-aligned minutes, so a daily candle built that way would disagree with the one the download stores. The chart says so rather than hiding the control.
- Live is often sold separately from history. A free Massive key downloads history and is refused at the live login; Alpaca's free key streams IEX and the indicative options feed but not SIP or OPRA; Interactive Brokers serves what your account subscribes to. When a feed cannot run, the chart names which of those it is and stops, rather than reconnecting behind a dot that never turns green.
- Most of these vendors allow **one live connection per account**, so a second program on the same key takes the seat. That case is reported as such and keeps retrying, since it clears when you close the other one.

**Alpaca** carries one setting for this: *Market data feed*, `iex` (free plan, the default) or `sip` (paid). It selects the live socket only; downloads are unaffected.

### The crypto derivatives providers

`BTCUSDT` is a spot pair **and** a perpetual, and the two are different series: the perpetual trades at a basis to spot and a dated contract converges to it. So which market a dataset holds is never inferred from the ticker.

- **Binance USDⓈ-M Futures** is its own provider next to Binance, not a setting on it. Contracts are spelled as the futures market spells them: `BTCUSDT` for a perpetual, `ETHUSDT_250926` for a dated one. Coin-M (inverse) contracts are not served.
- **Bitget** carries a *Market* setting, `spot` (the default) or `usdt-futures`, because it spells the same ticker identically on both books.
- **OKX** needs no setting: its own instrument ids say which market a ticker is on, `BTC-USDT` for spot, `BTC-USDT-SWAP` for a perpetual, `BTC-USD-241227` for a dated contract.

### OANDA

The only keyed FX provider here, and its key is the account's: OANDA issues no market-data-only token, so the connector asks for the same personal access token the [broker account](/config/brokers) uses, plus the account number it reads prices through.

- **Settings**: *Account ID* (`001-004-1234567-001`) and *Environment* (`live` or `practice`, which are different hosts with different tokens).
- **Instruments** are written `base_quote`, indices and commodities included: `EUR_USD`, `XAU_USD`, `SPX500_USD`. *Test connection* reports how many the account is allowed to price.
- **Daily candles are pinned to midnight UTC.** OANDA's own default rolls the day at 17:00 New York, which is the FX session but not the day every other dataset here is stored on, so the connector asks for the UTC one.
- Volume is a **tick count**, not a traded size: a dealing desk publishes how many prices it made, not how much changed hands.

When a provider holds several connectors, the chart's live control grows an account picker: two keys are two entitlements and two connection seats, so which one is spent is your choice, not a fallback.

### TradeStation

Its market-data scope rides on the account's own OAuth key, so the connector asks for the same API key pair and refresh token the [broker account](/config/brokers) uses. There is no separate market-data credential.

- **Settings**: *Environment* (`live` or `sim`).
- **Symbols** are TradeStation's own: `AAPL` for a share, `@ES` for the continuous future, `ESH26` for a contract, `$SPX.X` for a cash index, `MSFT 260116C400` for an option. Typing one looks it up and shows what it is, which is the fastest way to catch a typo.
- **Bars are stamped at their close**, so the connector subtracts the interval and stores the open, like every other series here.
- **Only 1m, 5m, 15m and 1d are offered.** TradeStation builds intraday bars from the *session* open, so its hourly candle starts at 9:30 and would not line up with the hourly candle anywhere else in your library. Download 15m and read it at any intraday timeframe instead; the refusal says so.

### FOREX.com (StoneX)

Same story: no market-data credential of its own, so it signs in with the same username, password and AppKey as the [broker account](/config/brokers), and the two share one session.

- **A market is a number.** The API takes a numeric market id; you type `EUR/USD` and the connector resolves it. When a name matches several markets the error lists them with their ids, and you download by id.
- **No volume.** A dealing desk publishes prices, not size, so the volume column is zero rather than a plausible number.
- **A daily bar is the venue's session**, which rolls at the New York close, not at midnight UTC. That is the period StoneX actually traded and it is stored as such, so a daily series from here does not sit on top of one from a UTC-day provider.
- `4h` is refused: StoneX does not say where it starts counting one. Download `1h` and read it at 4h.

### Capital.com

The third keyed one whose key is the account's: Capital.com issues no market-data credential, so the connector signs in with the same API key, login and custom password as the [broker account](/config/brokers), and the two share one session.

- **Settings**: *Environment* (`live` or `demo`).
- **An instrument is an epic**, Capital.com's own market name: `EURUSD`, `US500`, `AAPL`, `BTCUSD`. The symbol search returns them.
- **A candle is the mid of the two sides** the desk deals on, in the download and on the live chart alike.
- **A daily bar is the venue's session**, not the UTC day, so a daily series from here does not sit on top of one from a UTC-day provider.
- **Live** rides the same session and allows 40 instruments at once. Capital.com streams the bid and the ask as two separate candles, so a pane fills once both sides have ticked.

### Interactive Brokers

The odd one out: there is no vendor URL and no API key. You run **IB Gateway** or **TWS** on your own machine and the connector speaks its socket protocol, so what it carries is an **address**, not a credential: a host and a port, stored in the clear so a failed connection can be diagnosed. The data is whatever your IB account is subscribed to.

- **Settings**: *Gateway host* (`host.docker.internal` for a gateway on the same machine, since OpenTraderWorld runs in a container) and *API port* (4001 live / 4002 paper for the Gateway, 7496 / 7497 for TWS).
- **In the gateway**: Global Configuration → API → Settings, tick *Enable ActiveX and Socket Clients*, and check the port matches. On Docker Desktop the call arrives from the host loopback, so *Allow connections from localhost only* already covers it; on Docker Engine untick it and add `172.28.53.10` to *Trusted IPs*, which takes single addresses and not a range.
- **Test connection** reports what answered, and names the setting to change when nothing does.
- **Tickers**: `AAPL`, `SAN:EUR` or `7203@TSEJ:JPY` for shares, `EURUSD` for a cash pair, an OCC symbol for an option. A future is written with its month, `ES.202512`, or with the local symbol TWS shows, `MNQU6`. Futures are looked up against the gateway before anything is downloaded, so the exchange is optional: when the ticker names more than one listing, the error lists them and you pick.
- The client id is chosen by the app in a high private band, never asked for, so nothing else you have connected to the gateway is kicked off.
- Interactive Brokers allows 60 historical requests per rolling 10 minutes **per account**: the connector paces itself, so a long backfill is slow by design.

Tested against **IB Gateway build 10.50.1e (25 Aug 2026)**. Older builds are expected to work, the socket protocol being negotiated down, but that one is the version this connector was verified on.

## Create one

1. **Add connector**, pick the provider, and name it: the name is what module pickers show, so *Binance charts* beats *Binance 2*.
2. Fill the credentials the provider requires, or pick them from the Vault. Keyless providers skip this.
3. Choose the **modules** it serves. Opened from a module, the new connector is granted to that module only; opened from Settings or `/connectors`, it is granted to everything.
4. Optionally set a **request limit** (see below).

A connector missing a required credential is shown as *needs credentials* and is skipped by every module until you set it.

## Module grants

The grant list is **server-side**: a module asking for a connector it was never given is refused, so a checkbox that only lived in the browser would have been decoration. Ticking every data module collapses back to the *all modules* wildcard, which keeps future data modules covered.

The grantable modules today:

| Module | What it reads |
|---|---|
| **Historical Data** | the download form's provider list, and the symbol lookup |
| **Visualization** | the chart's symbol search, its on-demand windows and its live stream |
| **Watchlists** | the quote source of a list, or of a single symbol |
| **Journal** | the candles behind the Market data and Open risk tabs |

## Request limits

A limit is a count of outbound calls per **day**, **hour** or **minute**, tracked per connector.

- Everywhere else in the app it is **observe-only**: it feeds the counters in [Settings → API rate](/config/settings#api-rate) and warns you, but nothing is throttled.
- On the chart's on-demand fetches (`/api/histviz/series`) it **blocks**: once the connector is at its limit the window comes back with the bars already stored and a *request limit reached* notice, instead of quietly burning through a metered plan.

Leave the limit off if you'd rather let the provider be the one that says no.

## Where connectors are used

- **Historical Data**: the download form's provider list, and the symbol lookup.
- **Visualization**: the Data tab searches every connector granted to the chart at once; the live stream runs on the connector you pick in the live control, or on the oldest one the chart is granted for that provider.
- **Watchlists**: the quote source of a list, or of a single symbol. CoinGecko and Yahoo remain available with no connector at all.
- **Trading Journal**: the candles behind the Market data and Open risk tabs, with a source pickable per asset type.

::: tip Upgrading from the old per-module settings
Historical Data's *Settings* tab and Watchlists' *Sources* tab no longer exist: they were two disconnected copies of this screen over two disconnected lists. Accounts created in either one are now connectors here, each still granted to the module it came from, so nothing changes reach on upgrade. Names are globally unique again: a name that existed in both lists is kept once and the other renamed `<name> #2`.
:::
