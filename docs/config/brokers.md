# Broker accounts

A **broker account** is a read-only key to the place you actually trade. It answers three questions nobody should be retyping by hand: what did I fill, what am I holding, what do I have working.

Manage them in **Settings → Brokers**, or from the *Brokers* button any module that reads your book puts next to its account picker. Both render the same screen.

::: warning Read only, by construction
Broker accounts read, and only read: no route behind them places, changes or cancels an order. The credentials you are asked for are the read-only kind, so give exactly that: where a broker can issue a view-only key, the form says so. Should order routing ever ship, it will be its own feature, asking for its own keys and its own permission, and it will say so here.
:::

Not to be confused with [data connectors](/config/connectors): those read **prices**, these read **your account**. Two different credentials, two different lists, two different grants, on purpose.

## What you need to connect

| Broker | Credentials | Permission to grant | Reads |
|---|---|---|---|
| **Alpaca** | `api_key`, `api_secret` | Trading API key of the environment you pick (live *or* paper, they are separate keys) | fills, positions, orders, holdings |
| **Binance** | `api_key`, `api_secret` | *Enable Reading* only. No trading, no withdrawals | fills, orders, holdings |
| **Binance USDⓈ-M Futures** | `api_key`, `api_secret` | *Enable Reading* plus futures access. No trading, no withdrawals | fills, positions, orders, margin balances |
| **Bitget** | `api_key`, `api_secret`, `api_passphrase` | *Read-only*. No trade, no withdraw | fills, positions, orders, holdings |
| **OKX** | `api_key`, `api_secret`, `api_passphrase` | *Read* only. No trade, no withdraw | fills, positions, orders, holdings |
| **OANDA** | `api_token` + the account id | A personal access token. **OANDA has no read-only token**: the same one can trade | fills, positions, orders, holdings |
| **Coinbase Advanced Trade** | `api_private_key` + the full key name | CDP key, **View** only, created as **Ed25519**. No Trade, no Transfer | fills, orders, holdings |
| **TradeStation** | `client_id`, `client_secret`, `refresh_token` + the account id | OAuth sign-in granting `ReadAccount`, `MarketData`, `openid`, `offline_access`. **Not `Trade`** | fills, positions, orders, holdings |
| **FOREX.com (StoneX)** | `username`, `password`, `app_key` | Your login plus the AppKey StoneX issues. **No read-only credential exists** | fills, positions, orders, holdings |
| **Capital.com** | `api_key`, `identifier`, `api_password` | An API key (two-factor must be on) and its custom password. **No read-only grade** | fills, positions, orders, holdings |
| **NinjaTrader** | `username`, `password`, `cid`, `sec` | Platform login plus the developer API key pair. **No read-only key** | fills, positions, orders, holdings |
| **Kraken** | `api_key`, `api_secret` | *Query Ledger & Trade History*, and *Query Open Orders* | fills, orders, holdings |
| **Interactive Brokers (Flex)** | `flex_token` + a Flex query id | Flex Web Service token: it reads statements, it cannot trade | fills, positions, holdings |

Secrets are write-only: the app only ever knows *which* names are set. They can be typed in, or plugged from the [Vault](/config/settings#vault) so one key serves several accounts.

### Per-broker notes

- **Alpaca**: the *Environment* setting is `live` or `paper`, and has to be said rather than guessed, since the two live on different hosts with different keys. Alpaca reports no commission on a fill, so imported trades carry no fees: correct for commission-free equities, short of the truth for crypto and options, whose fees arrive as separate account activities.
- **Binance**, spot and futures alike, answers its trade history **one instrument at a time**, so a pull has to name the instruments (`BTCUSDT`, `ETHEUR`). Every other broker here answers the whole account.
- **Three crypto venues stop at 90 days.** Bitget, OKX and Binance futures serve three months of fills over the API and no more; anything older is a download from their web site. The import modal shows the limit and refuses a period that starts before it, rather than returning a quiet half-answer.
- **A derivatives account is not a spot one.** Bitget, OKX and Binance futures can be short, so *This account can go short* is ticked by default there. On a Bitget account set to the spot book only, untick it: a spot sale with nothing open is the sale of a coin bought earlier, not a short.
- **A contract is counted in contracts.** OKX reports derivative fills in contracts and publishes what one is worth (`ctVal × ctMult`), which is read from its instrument list and carried as the trade's point value. Bitget USDT-M and Binance USDⓈ-M contracts are sized in the base coin, so theirs is one. Coin-M (inverse) contracts are not read anywhere: they are sized in quote currency and their PnL is not a quantity times a price.
- **TradeStation is the only OAuth one.** There is no static key: a one-time browser sign-in produces a refresh token, which the app exchanges for a 20-minute access token as it goes. Grant `ReadAccount`, `MarketData`, `openid` and `offline_access` at that sign-in and leave `Trade` out; a token that could trade would be a standing risk for nothing. A fill here is an *order leg*, since TradeStation publishes closed orders rather than executions, and the identity a re-sync deduplicates on is the order id plus the leg's place in it.
- **FOREX.com signs in, it does not use a key.** The credentials are the account's own username and password plus the AppKey StoneX issues once its API terms are signed, so the same login can trade: treat it as a full-access secret and change the password when you remove the account. A trade imported from there carries **no commission**, because StoneX bills the spread; if your account is charged commission instead, the numbers here will be short of the truth. The trade-history endpoint takes no end date and no cursor, so a wide period is walked forward in pages of 200.
- **Capital.com answers one day at a time.** Its activity log caps the range between two dates at 24 hours, so a year of trading costs a call per day; periods wider than 400 days are refused by name rather than left to run into the rate limiter. Every instrument on the platform is a **CFD**, so a share CFD is filed as a derivative and not as the share, which is what a tax form wants. A deal id names a position rather than a fill, so the identity a re-sync deduplicates on is the deal, its timestamp and its direction together.
- **NinjaTrader allows two sessions per login**, and a third closes the oldest: this connector holds one, and a trading application signed in beside it holds the other. Its trading API is the Tradovate platform it acquired, which is why the errors say `tradovateapi`. It answers the fills its session can see and publishes no depth limit, so check the oldest row the first pull returns before relying on it for a tax year. A futures point value is read from the contract's product, never assumed from the root.
- **OANDA gives no read-only token.** The personal access token that reads this account can also trade it, so the form says so: treat it as a full-access credential and revoke it when you remove the account. Nothing in the app ever uses it to write.
- **Interactive Brokers** is not an API key at all: it reads a saved report through the Flex Web Service. Setup, the period rule and the time-zone setting have [their own section below](#interactive-brokers-the-flex-web-service).
- Rate limits are the broker's, and each row shows the one that applies. IBKR is the strict one: it builds the statement on demand and refuses a second request while one is still generating.

## Connect one

1. **Add account**, pick the broker, and name it: the name is what module pickers show, so *Kraken main* beats *Kraken 2*.
2. Fill the credentials, or pick them from the Vault. An account missing one is shown as *incomplete* and skipped by every module until you set it.
3. Fill the non-secret settings the broker needs: an environment (Alpaca, Binance futures, TradeStation, Capital.com, NinjaTrader), an account id (OANDA, TradeStation, Capital.com, FOREX.com, NinjaTrader), the Coinbase key name, the IBKR query id and offset, or the Bitget books.
4. Choose the **modules** it serves, or *all modules*.
5. **Test connection** reaches the broker and reports what answered: the account number and status, how many assets carry a balance, or which statement the Flex token returned. If something is wrong, the error names the setting to change.

Grants are enforced server-side: a module asking for an account it was never given is refused.

## What it lets you do

Four destinations, one shape. Whatever you import and wherever it lands, a broker import runs on the same two rails as a file import:

1. **Pull, then look.** The app asks the broker, folds the answer into what the module stores (positions for the journal, a balance sheet for the portfolio, closed disposals for the tax form) and shows it to you. Nothing is written at this step, so a pull you do not like costs nothing.
2. **Commit, and keep the thread.** Everything written carries the id of that import, so it stays one object afterwards: *Revert* deletes exactly what it created and nothing else, *Keep, untrack* cuts the link and leaves the rows in place. Both live in the import history of the module.

Duplicates are the rail's job, not yours. Each imported row is fingerprinted, so pulling an overlapping period again recognises what is already filed, marks it in the preview and writes it once.

### Import trades into the journal

**Journal → Import → Pull from a broker**. Pick the account, the period and the book to file into, and the fills come back folded into positions, previewed before anything is written.

1. **The account.** Only the ones granted to the journal are listed, and an incomplete one says so. The last account and period used for a book are remembered, so the next pull is two clicks.
2. **The period**, by date or with the *7 / 30 / 90 / 365 days* chips. Read it as the window the *fills* fall in, not the window the trades closed in: a position is folded from the fills inside the period, so start early enough to catch the entry.
3. **The instruments.** Binance answers its history one instrument at a time, so the symbols are required there and *Suggest* offers what the account holds. Everywhere else the field is a filter: leave it empty for the whole account.
4. **Shorts.** A spot account cannot be short, so a sell with nothing open is reported as a row error naming the fix (widen the period) rather than turned into a phantom short. On a margin account, tick *This account can go short*.
5. **Preview**, then import. The counters are fills, trades, of which closed and open, plus what is already in the book and what could not be built. Each line says which it is before you commit.

- Same destination and same folding as a file import, minus the mapping: an API answers typed fields, so the questions a CSV raises (which column is the date, is the decimal a comma) do not exist here.
- **Re-running is safe.** A position's identity is its *opening* fill, so widening the window and pulling again refreshes what has closed since and leaves the rest alone, instead of filing the same trade twice.
- **A refresh is mechanical only.** Prices, quantities, fees and dates come from the broker again; your notes, tags, strategy and template fields are yours and survive it.

### Align a portfolio on what the account holds

**Portfolio → From a broker**. It reads a **balance sheet**, not a trade history: the difference against your ledger is shown line by line, and you choose which lines to align. Each one you accept writes the single operation that makes the portfolio agree.

- A symbol the portfolio already holds resolves itself; anything else is asked, because "BTC" on an exchange is a string and an asset here is a price source.
- **Cost basis is never invented.** Interactive Brokers publishes one and it is used. The crypto exchanges publish a quantity and nothing else, so those lines say so and default to today's price, the one price nobody can mistake for a claim about the past.
- **Take the price from the exchange.** On a line with no cost basis, one click asks the venue what the asset trades at right now and fills the price in. The line then names the market that answered (`BTCUSDT`, `XBT/USD`), and says so when that market prices in something other than the asset's own currency: a Binance price is in USDT, not in dollars. An asset the venue does not price is left alone rather than valued from somewhere else, and you type the price yourself.

Alpaca, Binance (spot and futures), Bitget, Coinbase, Kraken, OKX, OANDA and TradeStation answer that question. Interactive Brokers Flex does not: a statement is not a quote feed, FOREX.com prices a market by its numeric id rather than by the name a portfolio line carries, and NinjaTrader serves prices through a separate market-data entitlement. Capital.com answers with the mid of the two sides it deals on.

### Draw your book on the chart

**Chart → Broker book**. Sync an account and its positions and working orders are drawn as price levels on the chart of the matching instrument, average cost for a position, limit and stop for an order. Matching is on the ticker, punctuation aside. A position with no average cost gets no line and is counted as such.

### Read a tax year

**Taxes → From a broker**. Pulls the fills, folds them into closed positions and totals what was realized inside the tax year, split across the capital, derivative and crypto lines of the form. It **writes nothing**: you apply the numbers to the form, and save the scenario yourself.

- **The window is not the tax year.** What you sold in March was bought earlier, and without that purchase there is no cost basis: move the start date back far enough to cover it. A disposal whose purchase is missing is reported, never valued against nothing.
- Each closed position is converted at the rate of **its own exit date**. One that has no rate is listed and left out of the totals rather than added in the wrong money.

## Interactive Brokers: the Flex Web Service

IBKR is the one broker here that is not read through a trading API. It is read through **Flex**, the report service of Account Management: you save a query describing what you want in a statement, and the app fetches that statement over HTTPS with a token.

### Why Flex and not TWS

The [market-data connector](/config/connectors#interactive-brokers) speaks the TWS socket to a Gateway you run yourself. That socket is the right tool for the present, and the wrong one for history: it answers open positions and the fills of the **current session**, so *import my trades for March* has no socket form at all. Flex serves a period, which is exactly the question an import asks.

The practical difference:

| | Flex Web Service | TWS / IB Gateway socket |
|---|---|---|
| **What you run** | nothing, it is an HTTPS call | Gateway or TWS, logged in, on the machine |
| **History** | the query's period, up to a year back | the current session only |
| **Credential** | a token that reads reports | your live session, able to trade |
| **Used here for** | journal import, portfolio holdings, tax year, chart positions | prices, charts, live bars |

### Why it is the safe way in

- **The token cannot trade.** It is issued for the Flex Web Service and that service serves statements. There is no order endpoint behind it to forget to disable, no permission checkbox to get wrong. Compare that with an exchange API key, where read-only is a box you have to remember to tick.
- **Nothing is left listening.** No Gateway running, no API port open, no Trusted IP to declare, nothing waiting on your machine while the app is idle.
- **It expires on its own.** IBKR gives a token a lifetime and mails a reminder before it lapses. A forgotten token stops working instead of staying valid forever.
- **The query is the fence.** A token can only return what the queries you saved describe. Keep the query to trades and open positions and that is all the app can ever see, whatever it asks for.
- Like every credential here, the token is **write-only in the app**: it can be typed in or plugged from the [Vault](/config/settings#vault), and is never shown again.

### How a pull actually runs

1. The app calls `SendRequest` with your token and query id. IBKR answers with a reference code and starts **building** the statement.
2. It then polls `GetStatement` with that code until the XML arrives, which is normally a few seconds and can be longer for a wide query. A statement still generating is the expected answer to the first tries, not an error.
3. The statement is parsed into fills (`Trade` rows at execution level) and holdings (`Open Positions`), and the app filters those on the period you picked.

If IBKR is still generating after a minute the app says so rather than hanging: narrow the query's date range, or try again in a moment.

### Set it up

1. **The token**: Account Management → Settings → **Flex Web Service**. Generate one, copy it once, note the expiry date.
2. **The query**: Account Management → Performance & Reports → **Flex Queries** → new *Activity* query. Include:
   - **Trades**, level of detail **Execution**, for the journal and the tax form;
   - **Open Positions**, for the portfolio alignment and the chart overlay.

   Save it and note the **query id**, the number shown next to its name.
3. In OpenTraderWorld: add the account, paste the token, fill **Flex query id** and **Statement time offset**, then **Test connection**. It reports the account number, the period the statement covers and how many trade rows it carries, which is the fastest way to see that the query is missing a section.

::: tip Two settings that decide whether the import is right
**The period is the query's, not yours.** A Flex query carries its own date range (*Last 365 Calendar Days*, *Year to Date*, a custom window) and the web service takes no dates at all. The dates you pick in the app **filter** what the statement returned, so a query set to *Last 30 days* will never yield March however far back you ask, and IBKR does not serve more than a year. Set the query wide, filter in the app.

**A Flex statement never names its time zone.** It stamps the query's own zone and says nothing about which one, so set *Statement time offset* to that zone in minutes (`-300` New York in winter, `60` Paris) or every fill lands on the wrong hour, and intraday trades land on the wrong day.
:::

### What Flex does not do

- **No working orders**, so the chart overlay draws IBKR positions at their average cost and no order levels.
- **No quotes.** A statement is not a price feed: the portfolio's *take the price from the exchange* button is offered by the API brokers, not here. IBKR is the only one of the five that publishes a **cost basis**, which is the number that matters for a ledger.
- **One statement at a time.** IBKR builds it on demand and refuses a second request while one is generating, so back-to-back pulls on the same query wait for each other.

## Module grants

| Module | What it reads |
|---|---|
| **Trading Journal** | the period's fills, for the import |
| **Portfolios** | what the account holds |
| **Visualization** | positions and working orders, for the chart overlay |
| **Tax Calculator** | the fills of a tax year |

Ticking every module collapses back to the *all modules* wildcard, which also covers modules added in future versions.

## Limits

- **A ticker is never guessed.** A symbol the app cannot resolve is an error naming the fix, not a best-effort match.
- What a broker does not publish stays empty rather than plausible: no invented cost basis, no invented fee, no invented side.
- Deleting an account removes its credentials. Whatever it already imported stays.
- Broker accounts are disabled in [demo mode](/guide/demo).
