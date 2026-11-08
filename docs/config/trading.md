# Trading from the chart

A broker account can do more than read your book: with **trading** switched on, the chart sends orders to it, moves and cancels them from their line, and closes positions from theirs. Three brokers take orders today:

- **Interactive Brokers (Gateway)**, through the IB Gateway or TWS you run yourself. This page.
- **Binance** and **Binance USDⓈ-M Futures**, on live accounts and on their test environments (Spot Demo Mode, the futures testnet). A live account takes orders once [live trading](#live-trading) is on for it.
- **Paper trading**, a simulated account run by the app itself. See [Paper trading](/config/paper-trading).

Every other broker answers that it does not take orders from OpenTraderWorld, and stays [read only](/config/brokers).

::: danger Experimental
Trading from the chart is **experimental**: it works with Interactive Brokers, Binance and Binance USDⓈ-M Futures for now. It is heavily tested, against simulated venues and the brokers' own test environments, but bugs can remain. Check every order at your broker, start small, and trade at your own risk.
:::

::: tip Live accounts
Interactive Brokers, Binance and Binance USDⓈ-M Futures are rated **E4**: a live account takes orders once you turn on [live trading](#live-trading) for it, with its limits set. Paper accounts and test environments need none of it.

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

| | Interactive Brokers | Binance spot | Binance USDⓈ-M | Paper |
|---|---|---|---|---|
| **Types** | all | all | all | all |
| **Time in force** | Day, GTC, GTD, IOC, FOK | GTC, IOC, FOK | GTC, GTD, IOC, FOK | Day, GTC, GTD, IOC, FOK |
| **Exits, OCO** | linked by IB | order lists (OTO, OTOCO, OCO) | linked by the app | linked by the app |
| **Trailing stop** | amount or %, from the market | %, in steps of 0.01%, start price | % (0.1 to 20), start price | amount or %, start price |
| **Reduce only** | | | yes | yes |
| **Post only** | | yes (limit maker) | yes (GTX) | yes |
| **Iceberg** | yes, whole units | yes, GTC limits | | |
| **Trigger on** | last, bid/ask, mid | | last, mark | |
| **Close position** | | | yes | |
| **Outside RTH** | yes | | | |
| **Adaptive algo** | yes | | | |

- **Binance spot** holds exits for a **limit** entry (OTO, OTOCO) and places a **market** entry's exits once it filled; a stop entry takes no exits. The limit half of an OCO goes as a limit maker. A sell exit is for the quantity bought: a fee taken in the coin itself leaves less than that, and Binance refuses the exit.
- **Binance USDⓈ-M** attaches nothing itself: the app sends exits as reduce-only orders beside their entry, and links them and OCO pairs by reading them every 5 seconds. After a restart the app takes the links back up from the orders it stored. Hedge mode is followed: each order names its long or short side, and a symbol held both ways closes one side at a time.
- **Interactive Brokers** trailing stops trail from the moment they are placed (no start price), and show an iceberg in whole units.

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

The **Quick Buy/Sell bar** (toolbar toggle) shows Sell, a quantity and Buy with the bid and ask. **A click sends a market order at once, with no confirmation.** Drag it anywhere; it snaps to the nearest edge.

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
| **Loss limit per period (account currency)** | anything once the account lost this much since the period started (hour, day or week, see below): the account halts |
| **Duplicate window (seconds)** | the same order (side, type, size, prices) sent again within these seconds, until you confirm it |

- A breach is refused with the limit and the value. **The order is never resized to fit.**
- **Taking risk off always passes**: a reduce-only order, a close, or an order that only shrinks what is held goes out through every limit and every halt.
- Changing a limit asks for your password.

### Halts

A halt refuses new orders until it ends; cancels and orders that reduce a position still go out, and working orders cannot be changed meanwhile (cancel them instead).

- **Kill switch**: the button in **Settings → Brokers** and in the chart's order panel cancels every working order on **every** trading account (orders placed elsewhere included, each cancel in the audit log), then halts them all. It asks for a confirmation, never a password.
- **Loss limit**: an account that reaches its loss limit halts on its own (checked before each order and on the reconciliation cycle), and you get a notification.
- **Position drift**: when a position moves without fills to explain it (an order the app did not see, a corrected fill), the account halts until you **acknowledge** it, and you get a notification. Check the account at the broker first; acknowledging always takes your password, whatever the policy below.

In **Settings → Brokers → Risk and kill switch** you choose how a halt ends, for every account:

- **after**: the start of the next hour, the next day or the next week (Monday), in the policy's **Time zone**; or **only when lifted**. The zone is UTC until you save one, and the form starts at your browser's zone;
- **lift sooner with my password**: a halt can be lifted early from the same place or from the chart's banner. A halt that only ends when lifted always lifts with the password.

Saving the policy asks for your password.

The loss period follows the same choice (a day when halts end only when lifted). The loss is measured from the account's equity at the start of the period, as the broker states it; a deposit or withdrawal during the period counts in it. A broker that states no equity (Binance Spot Demo Mode) cannot be held to a loss limit: its orders are refused while one is set.

## Live trading {#live-trading}

Each broker's order path has a level, shown on the account in **Settings → Brokers**: E2 (tested against a simulated venue), E3 (tested on the broker's own test environment, over days), E4 (a live dry run and a live order at the smallest size passed). Below E4, an account that trades real money is refused by the connector itself.

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

## Limits

- At Interactive Brokers, orders go to the Gateway's default account.
- No change of side or type on a working order: cancel and send a new one.
- Orders are in the contract's currency, with no conversion.
- AI agents ([MCP](/config/ai-agents)) and Automator workflows cannot send orders.
- Broker accounts, and so trading, are disabled in [demo mode](/guide/demo).
