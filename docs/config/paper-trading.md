# Paper trading

A **paper trading account** is a broker run by OpenTraderWorld itself. Orders sent to it fill on the live price of the chart they come from, against your own simulated cash, with the commissions, slippage and leverage you set. No key, no venue, no real money.

It trades from the chart exactly like a real broker: the same ticket, quick bar, order and position lines, panel tabs and audit log. Those are described in [Trading from the chart](/config/trading); this page covers what is specific to paper.

## Create one

**Settings → Brokers → Add account**, broker **Paper trading**. There are no credentials. The settings:

| Setting | Default | Meaning |
|---|---|---|
| **Account currency** | `USD` | The currency of cash and results, 3 to 5 letters |
| **Starting balance** | `100000` | Booked as the first deposit |
| **Commission per order** | 0 | A fixed amount per fill |
| **Commission per unit** | 0 | Per share or contract |
| **Commission (% of value)** | 0 | `0.1` means 0.1% of the traded value |
| **Minimum commission** | 0 | A fill never costs less |
| **Slippage (basis points)** | 0 | `5` means 0.05%, against you |
| **Leverage** | 1 | 1 is a cash account |
| **Short selling** | yes | `no` refuses a sell with nothing to sell |
| **Fill on** | `quote` | `quote` deals on the bid and ask, `last` on the last price |

Numbers must be zero or more, and leverage at least 1. The settings are read again at every fill, so a change applies from the next one.

A new paper account has **All modules** and **Trading** on. Trading on a paper account asks for no password and is never behind the [trading lock](/config/trading#trading-lock).

**Test connection** answers with the account's equity, cash, open positions and working orders.

## Cash

The **Paper account** panel, under the account in Settings → Brokers, has an **Amount** field and three buttons:

- **Deposit** adds the amount to cash.
- **Withdraw** takes it out, up to the cash held (not the equity).
- **Reset account** removes every order, fill and cash move, then starts over with one deposit: the amount typed, or the starting balance when the field is empty. The settings are kept, and trades already imported into the journal stay.

The panel also shows the total deposited, and the last 50 orders that stopped working with their status (filled, cancelled, rejected, expired) and the reason. **That list is where a rejection is explained.**

## How orders fill

### On which price

An order fills on the **live feed of the chart's data connector**: the connector the order came from, or the provider's connector granted to Charts. The engine runs on the server, so **orders keep filling with the chart closed**, and stop while the app is down.

- An instrument with no live feed is refused when the order is placed. An index is refused too: there is no market to trade.
- The engine watches the provider's finest live bars (1 minute when available) and its quotes when the feed publishes them. Only prices printed after the order starts working count.
- With **Fill on: quote**, buys deal on the ask and sells on the bid, while the quote is less than 15 seconds old. Otherwise, and with **last**, both deal on the last price.
- A bar older than 2 minutes is not dealt on. A market order sent while the market is closed waits for the first fresh price instead of being rejected.
- If the feed stops, it is retried every 10 seconds and orders keep waiting. If the data connector is deleted, its orders can no longer fill.

### By order type

| Type | Fills when | At |
|---|---|---|
| **Market** | the next price | that price, plus slippage |
| **Limit** | the market reaches the limit | the limit, or better if the market is already through it. No slippage |
| **Stop** | the market reaches the stop | the stop if touched, the market through a gap, plus slippage |
| **Stop limit** | the stop is reached, then the limit | as a limit, possibly on the same price |
| **Trailing stop** | the market comes back by the trail from its best price since the stop started (the highest behind a sell, the lowest behind a buy) | as a stop |
| **MIT** | the market comes to the trigger from the limit side | the market, plus slippage |
| **LIT** | the trigger, then the limit | as a limit |

- **No partial fills**: an order fills whole, with no volume or liquidity check.
- A **trailing stop** with a start price waits for the market to reach it (rise to it behind a sell, fall to it behind a buy), then trails from that price. Its best price is kept through a restart.
- **Reduce only**: placed on its own, it is refused unless the account holds a position on the other side. At its fill it is capped to what is held then, and cancelled when there is nothing left to reduce.
- **Post only**: a limit refused at placement when it would fill at once against the current bid or ask (the last price without a quote).
- No iceberg, trigger choice, outside-RTH, close-position or algo: a live price with no depth, session or mark has nothing to play them on, so the ticket does not offer them.
- The fill price is rounded to the instrument's tick when it is known.
- **Commission** per fill = per order + per unit × quantity + % × traded value, raised to the minimum. It is charged in the instrument's currency.

### Time in force

- **Day** (default) expires at the next **midnight UTC**, not at the exchange's close.
- **GTC** never expires. **GTD** expires at the date and time given.
- **IOC** and **FOK** are cancelled if the first price they meet does not fill them. With the market closed, they wait for that first price. Paper fills whole, so the two end alike.

Expiry is checked about every 10 seconds; the order then shows as expired.

### Brackets and OCO

- A **take profit** or **stop loss** waits for its entry to fill, then starts working. If the entry is cancelled, rejected or expires, its exits are cancelled.
- In an **OCO**, a fill cancels the other order of the group, and its exits.
- A working order's prices, quantity and time in force can be changed. **Close position** sends a GTC market order, then cancels the position's exits.

## Funds and leverage

- A position ties up its value ÷ leverage. Futures tie up their margin per contract instead (below), and ignore leverage.
- An order that opens or grows a position is checked when placed and again at its fill price, commission included. It is refused when the account would tie up more than its equity:

  > not enough funds: the positions would tie up X USD and the account's equity is Y USD

- An order that reduces a position is always allowed.
- With **Short selling** at `no`, a sell that would go short is refused.
- There is **no margin call and no forced liquidation**: equity can fall below the margin in use.

## Futures

A future needs its contract facts, in the chart's **Instrument settings**:

- **Tick size, point value and currency.** When empty, they are filled from the open-source [QuantConnect Lean](https://github.com/QuantConnect/Lean) contract specifications, embedded in the app and refreshed weekly. The dialog names the fields it filled; save to keep them, or type your own. Values you saved always win.
- **Margin per contract (paper)**, in the instrument's currency: use your broker's initial margin. Or tick **Count the contract at its full value instead**: quantity × price × point value.

An order on a future with no margin is refused, and the chart offers **Set margin** (opens the instrument settings) or **Use full value**. Either one sends the order again.

A ticker is reduced to its contract root (`MNQZ6`, `ESZ25`, `@ES`, `ES=F`, `/ES`). A root listed on several exchanges with no exchange given is an error naming the candidates: **the market is never guessed**. A future or option with no point value is refused, and so is any instrument with no currency, except a forex pair, whose currency is read from its name (`EURUSD` is in USD).

## Account figures

The order panel's **Account** tab and the Settings panel show:

| Figure | How it is computed |
|---|---|
| **Cash** | deposits − withdrawals + realized − commissions |
| **Open result** | Σ (mark − average price) × quantity × point value |
| **Equity** | cash + open result |
| **Realized** | since the last reset, before commissions, on an average-cost basis |
| **Commissions** | total fees |
| **Initial margin** | futures margin per contract + other positions' value ÷ leverage |
| **Available** | equity − initial margin, never below zero |
| **Positions value** | gross exposure, long and short |

The tab refreshes every 5 seconds while open.

### Other currencies

The account has one currency. An instrument in another one is converted with the **journal's FX rate of the day**: realized results and fees at the rate of their fill, open results and margin at today's.

- An order is refused when no rate exists: `no EUR to USD rate: add one in the journal's FX settings, or trade in USD`.
- A position whose rate disappears later is listed as left out of the totals.

## Into the journal

Paper fills import like any broker's: **Journal → Import → Pull from a broker**, pick the paper account, the book and the period, preview, import. See [Import trades into the journal](/config/brokers#import-trades-into-the-journal). **Sync automatically** works too.

## Good to know

- Amounts are kept as exact decimals, the way a venue books them: 0.1 bought three times and 0.3 sold leaves nothing open, and a price must sit exactly on the instrument's tick.
- Cash, orders and fills are stored in the database and survive restarts. Live prices are not: after a restart, positions are valued at their average price until the next print.
- Deleting the account deletes its cash, orders and fills.
- Every order call is in the [audit log](/config/trading#audit-log).
- Not modelled: queue position, liquidity, partial fills, interest, borrow fees, funding, dividends.
- Paper orders and cash moves are not exposed to [AI agents (MCP)](/config/ai-agents), and paper accounts are disabled in [demo mode](/guide/demo).
