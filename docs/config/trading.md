# Trading from the chart

A broker account can do more than read your book: with **trading** switched on, the chart sends orders to it, moves and cancels them from their line, and closes positions from theirs. These brokers take orders today:

- **Interactive Brokers (Gateway)**, through the IB Gateway or TWS you run yourself. This page.
- **Binance** and **Binance USDⓈ-M Futures**, on live accounts and on their test environments (Spot Demo Mode, the futures testnet). A live account takes orders once [live trading](#live-trading) is on for it.
- **Bybit** (spot or linear book), on live accounts and on the Bybit testnet. Rated **E2D**: built from Bybit's documentation and never tested against Bybit; a live account takes orders once [live trading](#live-trading) is on for it with the acknowledgement.
- **Bybit EU** (spot), on live accounts and on the Bybit EU testnet. Rated **E2D** like Bybit, with the same acknowledgement for a live account.
- **OKX** (spot, perpetual swaps and dated futures), on live accounts and in OKX demo trading. Rated **E2D**, with the same acknowledgement for a live account.
- **Bitget** (spot and USDT-M futures), on live accounts and in Bitget demo trading. Rated **E2D**.
- **Gate.io** (spot and USDT perpetuals), on live accounts and on the Gate.io testnet. Rated **E2D**.
- **KuCoin** (spot) and **KuCoin Futures**, on live accounts only: KuCoin has no test environment. Rated **E2D**.
- **Kraken** (spot), on live accounts only: Kraken has no spot test environment. Rated **E2D**.
- **Kraken Futures** (multi-collateral contracts, PF_ and FF_), on live accounts and on the Kraken Futures demo. Rated **E2D**, with the same acknowledgement for a live account.
- **Coinbase Advanced Trade** (spot), on live accounts only: Coinbase has no test account. Rated **E2D**.
- **MEXC** (spot and USDT perpetuals), on live accounts only: MEXC has no test environment. Rated **E2D**.
- **Bitfinex** (spot exchange wallet and perpetuals), on live accounts only: Bitfinex has no test host. Rated **E2D**.
- **Deribit** (futures and perpetuals: linear USDC contracts and inverse BTC and ETH contracts), on live accounts and on test.deribit.com. Rated **E2D**, with the same acknowledgement for a live account.
- **Paper trading**, a simulated account run by the app itself. See [Paper trading](/config/paper-trading).

Every other broker answers that it does not take orders from OpenTraderWorld, and stays [read only](/config/brokers).

::: danger Experimental
Trading from the chart is **experimental**: it works with Interactive Brokers, Binance, Binance USDⓈ-M Futures, Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex and Deribit, and with paper trading. Interactive Brokers and the two Binance accounts are heavily tested, against simulated venues and the brokers' own test environments. The others are built from each broker's documentation and have not been tried on a real account yet. Bugs can remain with any of them. Check every order at your broker, start small, and trade at your own risk.
:::

::: tip Live accounts
Interactive Brokers, Binance and Binance USDⓈ-M Futures are rated **E4**: a live account takes orders once you turn on [live trading](#live-trading) for it, with its limits set. Paper accounts and test environments need none of it.

Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex and Deribit are rated **E2D**: a testnet or demo account takes orders at once; a live account needs live trading on with the acknowledgement that the broker was never tested against the venue (see [Live trading](#live-trading)).

Cancelling orders is never blocked, whatever the account.
:::

## Set it up

### 1. The Gateway

Run IB Gateway (or TWS) signed in to your paper or live account, and in its API settings:

- tick **Enable ActiveX and Socket Clients**;
- untick **Read-Only API**, or every order is refused by IB itself;
- with Docker Engine (Linux), untick **Allow connections from localhost only** and add `172.28.53.10` to **Trusted IPs**.

The client id is picked by the app (3100 to 3115), so several sessions can share one Gateway.

### 2. The account

In **Settings → Brokers**, **Add account**, pick **Interactive Brokers (Gateway)** and fill:

| Setting | Value |
|---|---|
| **Gateway host** | `host.docker.internal` when the app runs in Docker, `127.0.0.1` otherwise |
| **Gateway port** | `4002` paper Gateway, `4001` live Gateway, `7497` paper TWS, `7496` live TWS |

There is no secret to give: the Gateway is already signed in. **Test connection** checks that it answers.

### 3. The trading switch

Each account row has a **Trading** chip, next to its module chips. It is separate on purpose:

- **All modules** does not include it. Reading your book and sending orders are two different permissions.
- **Turning it on asks for your password, every time.** Turning it off asks nothing.
- Paper trading accounts are the exception: no password, since no real money is involved.

### 4. The chart

A chart trades when three things line up:

1. an account is picked in the chart's **Broker book**;
2. trading is on for that account;
3. the chart's data comes from the same broker: an IBKR account trades a chart fed by the Interactive Brokers connector.

When one is missing, the order panel says which and how to fix it. Futures need the **contract month**: a continuous future is refused, so open `MNQZ6` rather than the continuous chart.

## The trading lock {#trading-lock}

An optional second factor for orders. With the lock on, the first order after you sign in asks for the code from your authenticator, once, and every trading account then stays open in that browser until you sign out.

- It guards placing, changing and closing. Cancelling, one order or all of them, never asks.
- It covers every trading account except [paper ones](/config/paper-trading).
- Turn it on in **Settings → Security → Trading lock**, once two-factor authentication is set up. Details, wrong codes and how to reset it: [Account security](/config/security#trading-lock).

::: danger Removing two-factor with the lock on turns trading off everywhere
Removing two-factor authentication, from Settings or with `disable-totp`, turns the lock off. When the lock was on, it also switches trading off on **every** account, paper included. Switch each one back on afterwards.
:::

## Sending orders

### The ticket

The order panel's **Ticket** tab sends one order, a bracket or an OCO. **It offers what the account's broker takes, and nothing more**: a type, time in force or option the broker does not declare is not shown (see [What each broker takes](#what-each-broker-takes)).

- **Types:**
  - **Market**, **Limit**, **Stop**, **Stop limit**.
  - **Trailing stop**: a stop that follows the market by a distance, as a percentage or a price amount, and never backs off. **Start at**, where the broker has it, makes it wait for a price before it starts trailing.
  - **MIT** (market if touched) and **LIT** (limit if touched): the mirror of a stop, triggered when the market comes to the price from the limit side (a buy below the market, a sell above), then a market or a limit order.
  - **OCO**: a limit and a stop on the same side, where the first to fill cancels the other. Its two prices must differ.
- **Exits:** the **Take profit** and **Stop loss** switches add the opposite order for the same quantity. Switching one on prefills it 0.5% (take profit) or 0.25% (stop loss) from the entry. **Exits time in force** picks GTC (default) or Day. Where the broker takes reduce-only orders, exits are sent reduce-only. Not available on an OCO, nor on a broker that attaches no exits. At Interactive Brokers the entry is held until its exits are accepted, so the bracket goes out whole.
- **Time in force:** Day, GTC, GTD with a date and time in the future, IOC, FOK (all or nothing at once).
- **Prices:** typed as a price, or as a distance from the bid, ask or last (from the entry, for exits), in ticks when the tick is known.
- **Size:** in **units** (shares, contracts), as an **amount** of money (divided by the entry's value), or as the money at **risk** to the stop loss (divided by the distance from the entry to the stop loss, which must be on). A future counts its point value; contracts and shares round down to whole units.
- **Options**, each shown only where the broker takes it:
  - **Reduce only**: only shrinks the position, never opens or turns one.
  - **Post only**: a limit that only rests on the book; refused rather than filled at once.
  - **Iceberg**: the size shown on the book, the rest hidden.
  - **Trigger on**: the price a stop or trigger watches (last, mark, bid/ask, mid).
  - **Close position**: a stop or MIT that closes the whole position on its trigger, whatever its size.
  - **Outside RTH**: also triggers and fills outside regular trading hours.
  - **Adaptive algo**: the broker works a market or limit order for a better price, patient, normal or urgent.

Before anything leaves, the app checks:

- every price and quantity is above zero, and each option fits the type (post-only on a limit, an iceberg below the order's size, a trailing percentage below 100);
- the broker takes the type, the time in force and every option asked: one it does not is refused, naming it;
- a price off the instrument's tick is refused with the two nearest valid prices (`… is not on MNQZ6's 0.25 tick: use … or …`);
- the instrument resolves at the broker. **It is never guessed.**

If one leg of a ticket is refused, the whole ticket is cancelled.

### Orders the app follows {#followed-orders}

Each order is stored under its own id **before** it is sent, and the broker's answer is stored with it. So:

- **An order id is sent once per account, ever.** The chart gives every order a new id; a ticket sent again (after stating a margin, say) goes under new ids. An id already used is refused, even after a restart.
- **No answer is not a refusal.** An order the broker does not confirm in time (10 seconds for the Gateway) is looked up at the broker by its id. When it cannot be found yet, it is marked **unknown** and is **never sent again**: new orders on that instrument wait until a lookup settles it. If it is still unsettled after 5 minutes, you get a notification (chart notifications channel) telling you to check the order at the broker.
- **An order the broker never received** (still not found after the lookups) waits for you. The Orders tab shows *the broker never received this order* with **Send again**, which sends it under new ids through every check a new ticket passes, and **Dismiss**, which sends nothing. You get a notification too.
- **A restart forgets nothing.** Orders sent just before the app stopped are looked up when it starts, and orders the app links itself (Binance USDⓈ-M brackets) are taken back up.

The app then compares its view with the broker's, a few seconds after start and on a cycle (every minute while it has orders out, every 10 minutes otherwise, and at once when a live stream reports missed events). **The broker wins**: an order that filled or was cancelled while nobody looked takes the broker's state, and an order placed elsewhere (TWS, the venue's site) is noted as external. A position that changed without fills to explain it is **drift**: it is written to the audit log and notified.

### What each broker takes

| | Interactive Brokers | Binance spot | Binance USDⓈ-M | Paper | Bybit | Bybit EU | OKX | Bitget | Gate.io | KuCoin |Kraken | Kraken Futures | Coinbase | MEXC | Bitfinex | Deribit |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **Types** | all | all | all | all | spot: market, limit; linear: market, limit, stop, stop limit | market, limit | market, limit | spot: market, limit, stop; futures: market, limit, stop, stop limit | market, limit | spot: market, limit; futures: market, limit, stop, stop limit |market, limit, stop, stop limit, MIT, LIT | market, limit, stop, stop limit, MIT, LIT | market, limit, stop limit, LIT | market, limit | market, limit, stop, stop limit | market, limit, stop, stop limit, MIT, LIT |
| **Time in force** | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK | GTC, GTD, IOC, FOK | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK (a market order works IOC: FOK refused) | GTC, IOC, FOK (a market order works IOC: FOK refused) | GTC, IOC, FOK (FOK refused on a market order) | GTC, IOC, FOK (FOK refused on a market order; a stop is GTC) | GTC, IOC, FOK (a market order works IOC; futures: FOK refused on a market order) | GTC, IOC, FOK (FOK on a limit only) |GTC, GTD, IOC, FOK (FOK on a limit only; IOC on a limit or a market order) | GTC, IOC, FOK (a market order works IOC: FOK refused; stops, MIT and LIT GTC) | GTC, GTD, IOC, FOK (a market order works IOC: FOK and GTD refused; a stop limit is GTC or GTD) | GTC, IOC, FOK (FOK on a limit only) | GTC, GTD, IOC, FOK (IOC and FOK on a limit only; a stop is GTC or GTD) | GTC, Day, IOC, FOK (Day refused on a market order) |
| **Exits, OCO** | linked by IB | order lists (OTO, OTOCO, OCO) | linked by the app | linked by the app | linear: take profit and stop loss on the whole position, at the market | | | futures: a position's take profit and stop loss, moved or removed from the order panel | | || | | | | |
| **Trailing stop** | amount or %, from the market | %, in steps of 0.01%, start price | % (0.1 to 20), start price | amount or %, start price | | | | | | || | | | | |
| **Reduce only** | | | yes | yes | linear | | swaps and futures | futures | futures | futures || yes | | futures | perpetuals | yes |
| **Post only** | | yes (limit maker) | yes (GTX) | yes | | | yes | yes | | yes |yes | yes | yes | yes | yes | yes |
| **Iceberg** | yes, whole units | yes, GTC limits | | | | | | | | || | | | | |
| **Trigger on** | last, bid/ask, mid | | last, mark | | last (linear stops) | | | spot: last; futures: last, mark | | last (futures stops) |last | last, mark, index | last | | | last, mark, index (stated on every stop) |
| **Close position** | | | yes | | | | | | | || | | | | |
| **Outside RTH** | yes | | | | | | | | | || | | | | |
| **Adaptive algo** | yes | | | | | | | | | || | | | | |

- **Binance spot** holds exits for a **limit** entry (OTO, OTOCO) and places a **market** entry's exits once it filled; a stop entry takes no exits. The limit half of an OCO goes as a limit maker. A sell exit is for the quantity bought: a fee taken in the coin itself leaves less than that, and Binance refuses the exit.
- **Binance USDⓈ-M** attaches nothing itself: the app sends exits as reduce-only orders beside their entry, and links them and OCO pairs by reading them every 5 seconds. After a restart the app takes the links back up from the orders it stored. Hedge mode is followed: each order names its long or short side, and a symbol held both ways closes one side at a time.
- **Interactive Brokers** trailing stops trail from the moment they are placed (no start price), and show an iceberg in whole units.

### Bybit

- **Each order goes to its instrument's book.** A Bybit account shows its orders and positions on every book it reads. An order goes to the book of the chart's instrument: the one the chart's Bybit data connector is set to (its **Market**: spot or linear), or else the one book that lists the symbol. A symbol both books list (`BTCUSDT`, a spot pair and a perpetual) on a chart whose data comes from elsewhere is refused with that fix. The ticket offers what that book takes.
- **Testnet.** Set the account's **Site** to `testnet` (a key from testnet.bybit.com): its orders take no money and need no live trading.
- **Checks before sending.** Tick, size step, minimum and maximum size (limit and market apart) and minimum order value, from Bybit's instrument list. A refused order names the rule and the nearest values; nothing is changed. An instrument that is not trading now (pre-launch, delivering, closed) takes no new order; cancels still go out.
- **Position mode.** One-way or hedge mode is read from your Bybit account (once, then kept up to date from Bybit's stream) and never changed. Each linear order names the position it acts on.
- **Protective stops.** A reduce-only linear stop is sent with Bybit's *close on trigger*: it reduces the position even when margin is short, and Bybit may then cancel or shrink other orders on that contract.
- **Before the first live order of a session** (linear), the order goes to Bybit's pre-check, which answers the margin it would take; a refusal there sends nothing. A conditional first order is refused: send a plain market or limit order first.
- **Live updates.** Orders follow Bybit's private stream; positions are read again when they change. If the stream is unavailable (an RSA key, a regional site without a stream), the account is read on a cycle.
- **If the app goes away**, Bybit keeps your orders working: nothing cancels them.
- **Take profit and stop loss (linear).** A ticket's take profit and stop loss go out with the entry in one call and cover the whole position (Bybit's *Full* mode): each fires as a market order when its price trades. Bybit creates them when the entry fills; they show in the order panel as take profit and stop loss orders linked to the entry while it works. Moving or removing one changes it on the position. A limit take profit fires at the market, not as a limit.
- **Not offered yet:** take profit and stop loss on spot or on part of a position, OCO, changing an order in place, trailing stops, post only.

### Bybit EU

- **Spot only**, through the same order path as Bybit spot: market and limit orders, the same checks before sending, a market order sized in the base coin.
- **Testnet.** Set the account's **Site** to `eu-testnet` (a key from testnet.bybit.eu): its orders take no money and need no live trading.
- **Live updates.** Bybit documents no private stream for Bybit EU: the account is read on a cycle.
- **Your key.** Bybit's guide says the EU site's API serves third-party applications for API broker users. A personal key may then be refused: the order shows Bybit's refusal with its code, and nothing else is sent.

### OKX

- **Books.** Spot, perpetual swaps and dated futures. Each order goes to its instrument's book: `BTC-USDT` is spot, `BTC-USDT-SWAP` a swap, `BTC-USDT-251226` a dated future.
- **Region.** Set the account's **Region** to where it was opened: `global` (okx.com, the default), `eea` (my.okx.com) or `us` (app.okx.com, US and Australia). A key works on its own region only.
- **Demo trading.** Set **Environment** to `demo` with a demo trading key: its orders take no money and need no live trading.
- **Sizes.** Swaps and futures trade in contracts: a size that is not a whole number of contracts is refused, naming the nearest sizes.
- **Changing an order** in place: its price and its size.
- **Position mode** (net or long/short) is read from your account and never changed. Leverage is shown, never set. Spot orders use your own coins only, never margin.
- **Not offered:** stop and trigger orders, take profit and stop loss, trailing stops, OCO, iceberg, options, inverse contracts.

### Bitget

- **Books.** Spot and USDT-M futures. An order goes to the book the chart's Bitget data connector is set to (its **Market**: `spot`, or `usdt-futures`, also written `futures`), or else to the one book that lists the symbol. A symbol both books list, on a chart whose data comes from elsewhere, is refused with that fix.
- **Demo trading.** Set **Demo trading** to `yes` with a Bitget demo key: its orders take no money and need no live trading.
- **Stops** go as Bitget trigger orders, on the last price (futures: last or mark). A spot stop fires at the market.
- **A spot market buy** is an amount to spend in the quote coin: the app reads the best ask just before sending and sends that amount, so the size filled may differ a little. The answer says so.
- **Every order of the account** shows in the order panel, trigger orders included, and cancels from it.
- **Position take profit and stop loss (futures).** One set on a position shows in the order panel; moving or removing it there changes it on the position.
- **Position mode and margin** are read from your account and never changed. Leverage is shown, never set.
- **Not offered:** changing an order in place, a take profit and stop loss sent with the entry, trailing stops, spot stop limit, OCO.

### Gate.io

- **Books.** Spot and the USDT perpetuals, chosen as for Bitget: the chart's data connector **Market** (`spot`, or `futures`, also written `usdt-futures`), or else the one book that lists the symbol.
- **Testnet.** Set the account's **Site** to `testnet` with a testnet key: its orders and its reads go to the Gate.io testnet, take no money and need no live trading.
- **A spot market buy** is an amount to spend in the quote coin, valued at the best ask read just before sending and rounded down to the pair's precision. The size filled may differ a little; the answer says so.
- **Futures sizes** are whole contracts: a size that is not is refused, naming the nearest sizes.
- **Changing an order** in place: on spot its price or its size (one at a time), on futures its price and its size.
- **Not offered:** stops, take profit and stop loss, trailing stops, post only, OCO, iceberg.

### KuCoin and KuCoin Futures

- **No test environment.** KuCoin has none: every account is live and needs live trading on with the acknowledgement. Before the first order of a session, the app sends it to KuCoin's order test, which checks it without placing it.
- **Spot:** market and limit orders, sized in the base coin.
- **Futures:** market, limit, stop and stop limit on the last price, reduce only, post only. Sizes are whole contracts. Stops show in the order panel and cancel from it.
- **Position and margin mode** are read from your account and never changed. Leverage is shown, never set.
- **Not offered:** changing an order in place, a take profit and stop loss sent with the entry, spot stops, OCO, coin-margined futures, margin trading.

### Kraken

- **No test environment.** Kraken has none for spot: every account is live and needs live trading on with the acknowledgement. Before the first order of a session, the app sends it to Kraken's validate call, which checks it without placing it.
- **Your key.** Give it Query Funds, Query Open Orders & Trades, Query Closed Orders & Trades, Create & Modify Orders, Cancel/Close Orders and Access WebSocket API, never Withdraw Funds. Give the app a key of its own: Kraken refuses a call whose nonce is not above the key's last one, so a key another program also uses fails at random.
- **Sizes and fees.** Orders are sized in the base coin; fees are taken in the quote coin.
- **Changing an order** in place: its price, trigger and size. Cancel all is one call.
- **Not offered:** margin, a close order attached to the entry, trailing stops, iceberg, triggers on the index price, tokenized stocks.

### Kraken Futures

- **Contracts.** Orders go on the multi-collateral contracts (PF_, FF_), sized in the coin. Inverse contracts (PI_, FI_) show with their orders and cancel from the order panel, but take no new order.
- **Demo.** Set the account's **Environment** to `demo` with a key from the Kraken Futures demo: its orders take no money and need no live trading.
- **Your key.** General API: Full Access to send orders; a Read Only key still reads. Never grant the Withdrawal API.
- **Changing an order** in place: its size, limit and trigger.
- **Every order of the account** shows in the order panel, trigger orders included, and cancel all covers them.
- **Position mode** is one-way. Leverage is shown, never set.
- **Live updates.** No private stream is used: the account is read on a cycle. A fill's fee is shown without its currency, which Kraken does not state.
- **Not offered:** trailing stops, a take profit and stop loss sent with the entry, OCO, options, orders on inverse contracts.

### Coinbase

- **No test account.** Coinbase has none: every account is live and needs live trading on with the acknowledgement. Before the first order of a session, the app sends it to Coinbase's order preview.
- **Spot only**, sized in the base coin. A stop limit triggers on the last price.
- **Checks before sending.** Tick, size step, minimum and maximum size, from Coinbase's product list. A product in limit-only or post-only mode takes only those orders.
- **Changing an order** in place: a limit's price and size.
- **Every order of the account** shows in the order panel, orders placed in the Coinbase app included.
- **Your key.** It needs the Trade permission to send orders; a View-only key still reads.
- **Live updates.** Orders follow Coinbase's user stream; fills are read on the cycle. A fill Coinbase adjusted (reversal, correction) is left out of the import and named in the server log; the other fills are kept.
- **If the app goes away**, Coinbase keeps your orders working.
- **Not offered:** stop market, take profit and stop loss, trailing stops, OCO, futures and perpetuals.

### MEXC

- **No test environment.** MEXC has none: every account is live and needs live trading on with the acknowledgement. Before the first spot order of a session, the app sends it to MEXC's order test.
- **Books.** Spot and the USDT perpetuals, chosen as for Bitget: the chart's data connector **Market**, or else the one book that lists the symbol.
- **A spot market buy** is an amount to spend in the quote coin, valued at the best ask read just before sending and rounded down to the pair's precision.
- **Futures** sizes are whole contracts. New orders go out in hedge mode only: on a one-way account they are refused before anything is sent (switch the account to hedge mode at MEXC), while a position still closes. Margin mode and leverage are read, never set.
- **Every order of the account** shows in the order panel, futures plan, take profit and stop loss, and trailing orders placed elsewhere included, and cancel all covers them.
- **Your key.** Spot trading and futures order placing, and each spot pair added to the key's trading pairs; futures order placing needs KYC.
- **Not offered:** placing stops, take profit and stop loss or trailing stops, chase orders, changing an order in place, OCO, coin-margined futures.

### Bitfinex

- **No test host.** Bitfinex has none: every account is live, paper sub-accounts included, and needs live trading on with the acknowledgement.
- **Books.** The exchange wallet's spot pairs and the perpetuals (`BTCF0:USTF0`). An order goes to the book the chart's Bitfinex data connector is set to (its **Market**: spot, or derivatives, also written perp or futures), or else to the one book that lists the symbol. Margin trading is not offered.
- **Leverage** is the held position's, sent only when it is a whole number from 1 to 100; with no position, Bitfinex applies its default (10). The app never chooses it.
- **Prices** carry at most 5 significant digits: another price is refused with the two nearest.
- **Changing an order** moves its prices only: to change its size, cancel it and place a new one.
- **Your key.** Write access to Orders to send; a read-only key still reads. Give the app a key of its own (Bitfinex checks a rising nonce per key).

### Deribit

- **Books.** Futures and perpetuals: linear USDC contracts sized in the coin, inverse BTC and ETH contracts sized in USD. On an inverse contract the ticket takes the size in the coin or in USD: a USD size rounds down to the contract step (10 USD on BTC), a coin size is converted at the last price, then rounded down. Positions and orders on inverse contracts show in USD. Your risk limits value an order given in USD at that amount.
- **Test environment.** Set the account's **Environment** to `test` with a key from test.deribit.com: its orders take no money and need no live trading.
- **Stops** need their trigger (last, mark or index): there is no default.
- **Post only** is refused rather than moved when it would fill at once.
- **Changing an order** in place: its price, trigger and size. A stop that fired is followed to the order it became.
- **Every order of the account** shows in the order panel, triggers included, and cancel all covers them.
- **One position per instrument.** Leverage is shown, never set.
- **Not offered:** options, spot, a take profit and stop loss sent with the entry, OCO, trailing stops, iceberg.

### Binance

- **API key.** Orders need a key with trading enabled ("Enable Spot & Margin Trading" for spot, "Enable Futures" for USDⓈ-M) and withdrawals off. On a live account, before the first order of a session, the app reads the key's rights and refuses to send when trading is off, withdrawals are on or the key is not restricted to trusted IPs. It then dry runs the order on Binance's test route.
- **Checks before sending.** Every order is held to the symbol's filters as Binance publishes them: tick, size step and limits, market-order size, minimum and maximum order value, the price band around the current reference price (mark price on futures), and the open-order cap per symbol. A refused order names the filter and the nearest values that would pass. Nothing is changed for you.
- **Order rate.** When the account's order limit would be exceeded, the order is refused with the number of seconds to wait. It is never queued.
- **Changing a futures stop.** Binance cannot change a conditional order in place. The app places the changed stop first and then cancels the old one, so a stop is always working. The order gets a new id (`….r1`).
- **If the app goes away (USDⓈ-M).** While it has orders working on a futures symbol, the app keeps Binance's auto-cancel countdown armed (120 s, refreshed every 30 s). If it stops refreshing, Binance cancels every open order on that symbol, including ones placed elsewhere.
- **Positions.** Futures positions show the leverage and margin mode (cross or isolated) set at Binance. The app only reads them.
- **Account tab (spot).** Cash is the USDT balance; available is its free part. Equity is Binance's own value of the spot wallet in USDT, on live only (Spot Demo Mode does not publish it).

### Interactive Brokers

- A size the contract does not take (under its minimum, or off its size step, for example 0.5 of a future) is refused before anything is sent, with the valid sizes.
- A cancel that arrives after the order filled is not an error: the order shows as filled.
- While IB reports it lost its servers (1100), new orders and changes are refused; cancels still go through.
- On a live account, the first order of each Gateway connection is sent to IB as a what-if first. IB refusing it, or saying the account cannot carry it, refuses the ticket with IB's reason.

The ticket docks at the bottom, top, left or right of the chart, or between rows or columns of a layout. Drag its inner edge to resize it (arrow keys work too, Shift for bigger steps); the size is kept per dock.

### The quick bar

The **Quick Buy/Sell bar** (toolbar toggle) shows Sell, a quantity and Buy with the bid and ask. **A click sends a market order at once, with no confirmation.** Drag it by its grip anywhere on the chart. Dropped near an edge it snaps to it, in a row along the top or the bottom, in a column along a side; dropped anywhere else it stays there, and **Turn the bar** (next to the cog) switches it between a row and a column. Its place is saved with the workspace.

Its cog picks an [entry preset](#entry-presets) for this chart: both buttons then send their market order with the preset's take profit and stop loss, and its size when it has one. The preset in use shows as a chip on the bar; click it to drop the preset.

### Entry presets {#entry-presets}

A preset is a market entry with its take profit and stop loss at a set distance, ready on one click or one key. Open the list from the quick bar's cog (**Edit presets…**) or from the preset bar of the ticket. The presets belong to the chart settings: every workspace offers the same list.

| Field | |
|---|---|
| **Name** | shown on the chips and in the menus |
| **Direction** | Long, Short, or **Long / Short**: the ticket and the keys then keep the side the ticket shows. In the quick bar the direction is always the button's |
| **Size** | a size in units, or **The chart's size** (the one the bar or the ticket shows) |
| **Take profit**, **Stop loss** | a distance from the entry price, or **Off**. Units: **ticks** of the instrument, **points** of price, an **amount** of money on the whole position, or a **%** of the price |
| **Key** | 1 to 9, or none. A key already used by another preset moves to this one |

- **The exits are priced from the side the entry crosses**: the ask for a buy, the bid for a sell (the last price when there is no quote), then rounded to the instrument's tick.
- **Quick bar**: a click sends the market entry and its exits at once, with no confirmation, like the bar's plain orders.
- **Ticket**: the preset bar above the side fills the ticket with one (side, market, size, take profit and stop loss prices). Nothing goes out until you send it.
- **Keys 1 to 9**: off until you tick **Keys 1 to 9 fill the order ticket** in the presets dialog. A key opens the ticket filled with its preset; you send it from there. Keys do nothing while you type in a field or while a dialog is open.
- A preset that cannot be priced is refused with the fix: no price yet (wait for one), a distance in ticks on an instrument whose tick size is unknown (set it in the instrument settings, or use points), an amount without a size, or a stop loss that would land at or below zero.

### The `+` on the price scale

Hover the price scale and click **+**: the menu offers a buy or a sell at that price, as a limit or a stop depending on which side of the market it sits. It is sent at once, Day, or opened in the ticket with **Open in ticket…**.

## On the chart

- **Working orders** are drawn as lines. Drag a line's label to move the order (it snaps to the tick); a refused move puts the line back. Its **✕** cancels the order and its exits.
- **Double-click** an order line, or use the Orders tab, to open it in the ticket and change its prices, quantity or time in force. Side and type cannot change. Orders placed in TWS show too and can be edited the same way when they are a market, limit, stop or stop-limit order.
- **Positions** are drawn at their average price, with a **✕** to close at the market. Closing sends a market order for the quantity the broker holds now, then **cancels the position's exits**: the working orders on its instrument, on the closing side, that the broker links to an entry or flags reduce-only. Other orders on the instrument stay.
- **Protect with exits** (the shield in the Positions tab) opens the ticket on the position's chart with an OCO on the other side for the quantity held: a take profit and a stop loss, one cancelling the other, reduce-only where the broker has it. The position must be open on a chart of that account.
- **Brackets:** hovering an order of a bracket draws its links, dashed green to the take profit and dashed red to the stop loss.
- **Fills:** with **Entry and exit markers** on, the account's fills on the instrument are drawn as markers at their price and bar (a buy points up, a sell down), over the last 30 days at most. It is off by default.

**Chart settings → Trading** sets how they are drawn:

| | Options |
|---|---|
| **Position** | Result on the line (none, percent, amount), Line (full width, short, tag on the price scale only), show the side, show the quantity |
| **Order** | Line, show the side, show the quantity |
| **Fills** | Entry and exit markers, Buy marker and Sell marker colours |

## The order panel

Beside **Ticket**, filtered by broker:

| Tab | Shows |
|---|---|
| **Positions** | symbol, side, quantity, average price, market price, P&L, open time and entry fees (from the last 7 days of fills), with a close button |
| **Orders** | type, quantity, limit, stop and status, with edit and cancel. Its **Cancel all** cancels the orders listed in this browser |
| **Account** | the figures the broker states, see below |
| **Logs** | the last 200 calls sent to the broker, from the audit log |

### Account figures

**Equity, Cash, Open result, Realized, Initial margin, Maintenance margin, Available, Excess liquidity, Buying power, Positions value**, in the account's base currency, one row per account the Gateway reaches. For Interactive Brokers they are IB's own (`NetLiquidation`, `TotalCashValue`, `UnrealizedPnL`, today's `RealizedPnL`, `InitMarginReq`, `MaintMarginReq`, `AvailableFunds`, `ExcessLiquidity`, `BuyingPower`, `GrossPositionValue`). A figure the broker does not state is not shown: IB states no commission total, so that line is absent.

### Live updates

Orders, fills and positions arrive as the broker reports them, over one stream for every trading account on the page: the Gateway's own messages for Interactive Brokers, Binance's user data stream for Binance (the WebSocket API on spot, the account's listen key on USDⓈ-M, both with the account's existing key). Hedge-mode futures positions are read again in full rather than updated in place. The book is also re-read every 30 seconds, and one second after a position moves. If the stream drops, the page falls back to reading the book every 4 seconds. A Gateway that disconnects is reconnected on the next call; working orders stay at IB in the meantime.

## Risk limits and the kill switch {#risk-limits}

Your own limits, checked before any order leaves, on every trading account (paper included). Each is off until you set it, in **Settings → Brokers → Risk limits** on the account:

| Limit | Refuses |
|---|---|
| **Largest order (size)** | an order larger than this many units |
| **Largest order value (account currency)** | an order worth more than this, in the account's currency (size × price × a future's point value) |
| **Largest position (size)** | an order that would leave more than this many units held on the instrument |
| **Largest position value (account currency)** | an order that would leave a position worth more than this, in the account's currency (size held × price × a future's point value) |
| **Price collar (%)** | a limit or stop further than this percentage from the chart's last price |
| **Orders per minute** | more orders than this in the last minute |
| **Open orders at once** | more working orders than this |
| **Loss limit per period** | anything once the account lost this much since the period started (hour, day or week, see below): the account halts. Its unit is an **Amount** in the account currency or a **% of equity** the period started with |
| **Duplicate window (seconds)** | the same order (side, type, size, prices) sent again within these seconds, until you confirm it |

- A breach is refused with the limit and the value. **The order is never resized to fit.**
- **Taking risk off always passes**: a reduce-only order, a close, or an order that only shrinks what is held goes out through every limit and every halt.
- Changing a limit asks for your password.

### Halts

A halt refuses new orders until it ends; cancels and orders that reduce a position still go out, and working orders cannot be changed meanwhile (cancel them instead).

- **Kill switch**: it cancels every working order on **every** trading account (orders placed elsewhere included, each cancel in the audit log), then halts them all. In **Settings → Brokers** it asks for a confirmation. On the chart it sits in the toolbar next to **Freeze**, while an account trades, and works like it: one click arms it for 3 seconds, a second click fires it. Never a password.
- **Freeze** (chart toolbar): blocks every new entry sent from the charts, on every connected account (paper included), for 10 minutes, and cancels working entry orders. Closing, reducing, stops and targets keep working. One click arms it, a second starts it; nothing ends it early.
- **Loss limit**: an account that reaches its loss limit halts on its own (checked before each order and on the reconciliation cycle), and you get a notification.
- **Position drift**: when a position moves without fills to explain it (an order the app did not see, a corrected fill), the account halts until you **acknowledge** it, and you get a notification. Check the account at the broker first; acknowledging always takes your password, whatever the policy below.

In **Settings → Brokers → Risk and kill switch** you choose how a halt ends, for every account:

- **after**: the start of the next hour, the next day or the next week (Monday), in the policy's **Time zone**; or **only when lifted**. The zone is UTC until you save one, and the form starts at your browser's zone;
- **lift sooner with my password**: a halt can be lifted early from the same place or from the chart's banner. A halt that only ends when lifted always lifts with the password.

Saving the policy asks for your password.

The loss period follows the same choice (a day when halts end only when lifted). The loss is measured from the account's equity at the start of the period, as the broker states it; a deposit or withdrawal during the period counts in it. A broker that states no equity (Binance Spot Demo Mode) cannot be held to a loss limit: its orders are refused while one is set.

## Guards on chart trading {#guards}

Guards watch the trades you send from the charts during a **session** and act when a rule is reached. Open them from **Guards** in the chart toolbar: the button counts the guards switched on, and turns amber while a guard holds a session stopped.

- **Only chart trading counts**: orders sent from a chart (ticket, quick bar, the `+` of the price scale) and their fills. Robots and trades on alert are neither counted nor stopped.
- A guard covers one broker account or **Every account**, each account read on its own. Accounts with trading and the charts granted in **Settings → Brokers** are offered. At most 30 guards.
- The rules are read on the server every 20 seconds, and again right after a fill on the chart, so they keep working with no chart open.
- **Each occurrence fires once**: a rule read again finds what it already did.

### Sessions

**A session ends** **Each day (next day)**, at midnight in the chosen time zone (the form starts at your browser's zone), or **Only when I reset it**. **Reset session** (one account) or **Reset all** in the dialog always starts a new session, clears what fired and lifts a stop.

### Rules

| Rule | Fires when |
|---|---|
| **Losing trades** | N losing trades, **In a row** or **Over the session** (again at each further N) |
| **Number of trades** | N entries within a window (minutes or hours): **Sliding**, or **Every period from session start** |
| **Drawdown since session start** | the account's equity fell by an amount or a percentage from the session's start |
| **Loss on one trade** | one trade lost more than an amount, or a percentage of **The trade's value** or of **Equity at session start** |
| **Fees** | the session's fees reach an amount, in the account currency |

- **A trade** is a round trip rebuilt from the session's chart fills, per instrument, at average cost: it ends when the instrument is flat again, and its result is net of fees. A position held before the session has no known cost: the round trip that closes it is not counted.
- **Equity** is the broker's own figure, read at the session's first check; a deposit or withdrawal counts in it. A broker that states no equity cannot run the drawdown rule, nor a loss in percent of equity.
- **Fees** billed in the instrument's currency, or in the coin bought, are counted; others are not.
- A broker that serves no recent fills gives the rules on trades and fees nothing to read: they never fire there.

### What happens

| Action | |
|---|---|
| **Alert** | a notice on the chart |
| **Window to acknowledge** | a window over the chart, which you can drag, until you **Acknowledge** it (or **Acknowledge all**). Nothing is blocked |
| **Stop the session** | new chart entries on the account are refused until you reset the session (or, with daily sessions, until the next day). Orders that reduce a position still go out. The chart shows the stop in its halt banner, with a reset button. Option: **Also cancel the chart's working orders and close the positions it opened**, at the market, on the instruments the session traded |
| **Kill switch** | the [kill switch](#halts), as its button fires it: every working order cancelled, every trading account halted |

Every firing and every reset is written to the security log, and the orders a stop cancels or closes go to the [audit log](#audit-log) like the chart's own.

## Live trading {#live-trading}

Each broker's order path has a level, shown on the account in **Settings → Brokers**: E2 (tested against a simulated venue), E3 (tested on the broker's own test environment, over days), E4 (a live dry run and a live order at the smallest size passed). Below E4, an account that trades real money is refused by the connector itself.

| Level | Means | Live orders |
|---|---|---|
| **E2** | tested against a simulated venue | refused |
| **E2D** | E2, built from the venue's documentation alone, never tested against the venue | on an account whose owner acknowledged that, with its limits set; the venue's own check runs before the first order of each session where it has one |
| **E3** | tested on the broker's own test environment, over days | refused |
| **E4** | a live dry run and a live order at the smallest size passed | once live trading is on, with its limits set |

**E2D (Bybit, Bybit EU, OKX, Bitget, Gate.io, KuCoin, KuCoin Futures, Kraken, Kraken Futures, Coinbase, MEXC, Bitfinex, Deribit).** Turning live trading on asks you to tick an acknowledgement that this broker's order path was built from the venue's documentation and has never been tested against the venue; it is stored with the date and shown on the account. The same limits as E4 are required. An answer the broker does not understand, an order whose outcome stays unknown, or a position that moves without fills is handled as on every broker: logged, notified, and reconciled with the broker.

At E4, a live account still takes no order until you turn on **Live trading** on it:

- it needs the trading switch on, and the account's **size or value limit per order**, **size or value limit per position** and **loss limit** set;
- the [trading lock](#trading-lock) is your choice. With it on, turning live trading on, and the first order of each sign-in, ask for your authenticator code. With it off, the account shows a reminder and its orders go out without a code;
- the account shows the level, whether the broker cancels your orders if the app stops (Binance USDⓈ-M: yes; spot and Interactive Brokers: no, orders keep working), and which limits are set;
- turning it off asks nothing; turning trading off turns it off too, and it has to be turned on again.

Test environments, demo accounts and paper accounts never need it.

## Audit log and order metrics {#audit-log}

Every place, change, cancel, cancel-all and close is written to an **append-only audit log**: time, account, broker, action, request, broker answer or error, and latency. The database refuses to change or delete a row. Calls refused before reaching the broker because trading is off or the lock is closed are not logged; every other refusal is. What the [reconciliation](#followed-orders) finds (a missed fill or cancel, an external order, an order settled after a lost answer, position drift) is logged too, as **reconcile**, and is not counted in the order metrics.

In **Settings → Brokers**, each trading account shows its **order metrics** (calls, placed, refused, latency p50 / p99, last order) and a **Cancel all orders** button. Unlike the [kill switch](#halts), it covers that one account and halts nothing:

- it asks for confirmation, then cancels **every working order** of the account at the broker, orders placed outside OpenTraderWorld included. At Interactive Brokers that is every order of the Gateway's accounts, including orders placed by hand in TWS;
- it reports how many were cancelled. At Interactive Brokers it first waits up to 10 seconds for none to be left working;
- it works with trading off and with the lock closed.

## Trade on alert {#trade-on-alert}

A chart alert can send an order when it fires, with nobody watching, the app closed included. Set it up in the alert's **Trade** section (*Send an order to a broker account when this alert fires*); the alert itself is described in [Charts → Alerts](/modules/market-data#alerts).

- **The account** must have trading on, be granted to Charts, and carry [background trading](/config/brokers#background-trading), signed once in **Settings → Brokers**. A live account also needs [live trading](#live-trading) on with its limits set.
- **When it fires**: **Enter** sends the ticket you describe (side, type, size in **Units**, **Amount** or **Risk**, prices as a distance from the price that fires, take profit and stop loss); **Exit** cancels the working orders on the instrument and closes its position at the market.
- **If it is refused**: do nothing more, cancel the orders on the instrument, also close it, or cancel everything and close every position. When the broker's answer is lost, only the orders on the instrument are cancelled.
- **Preview** shows the order at the chart's price, the broker's dry run where it has one, and what else is armed on the instrument. **Arm** comes after a preview and asks for your authenticator code when the [trading lock](#trading-lock) is on and the account is not paper.
- **It fires once**, then waits to be armed again. Each order passes every check a chart order passes ([risk limits](#risk-limits), halts, the audit log, with the alert as its source), and the outcome is notified on the alert's channels.
- **Disarming.** Turning off background trading or trading on the account, removing two-factor, turning the trading lock on, or the kill switch disarms what was armed; arm it again knowingly.

## Limits

- At Interactive Brokers, orders go to the Gateway's default account.
- No change of side or type on a working order: cancel and send a new one.
- Orders are in the contract's currency, with no conversion.
- AI agents ([MCP](/config/ai-agents)) and Automator workflows cannot send orders, nor arm a trade on alert. A [trading bot](/modules/robots) or a [trade on alert](#trade-on-alert) sends orders only once you arm it, on an account with [background trading](/config/brokers#background-trading) signed.
- Broker accounts, and so trading, are disabled in [demo mode](/guide/demo).
