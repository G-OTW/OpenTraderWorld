# Trading Journal

Log every trade, in any currency, and get honest performance stats: equity curve, win rate, expectancy, profit factor, drawdown, Sharpe and more. The journal is organized in six tabs: **Trades**, **Breakdown**, **Templates**, **Strategies & capital**, **Fees & currency**, **Pending tasks**.

## Categories

Trades live in **categories** — folders like *Crypto scalping* or *Long-term stocks*, each with its own color, capital and stats. Create them from the category bar; drag to reorder. Deleting a category deletes its trades.

## Set up capital

In **Strategies & capital**, give each category a **beginning stack** and record **refills** and **withdrawals** over time. This is what return, equity curve and drawdown are computed against — without it you still get PnL, but not returns.

You can also name **strategies** with their signal names (e.g. *Breakout, Pullback*). Tag trades with a strategy/signal and the Breakdown tab can filter by them — that's how you find out which setups actually pay.

## Templates

Templates drive the trade form. A prebuilt **standard trade** exists; create your own per market or style:

- **Reserved fields** (side, prices, quantity, fees, leverage, multiplier, currency, unit type…) feed the performance stats.
- **Custom fields** (text, numbers, choice lists…) are free-form — setup grade, market condition, whatever you track.
- A template can set a **default fee schedule**, pre-selected when logging from it (overridable per trade).

## Logging trades

From the Trades tab, pick a template (or *Quick — all fields*) and fill the form. Two levels:

- **Simple** — one entry, one exit (or leave the exit empty for an open position).
- **Advanced** — scaling in/out with multiple **entry and exit legs** (each with its own price, quantity, fees, signal), plus **SL/TP brackets**. When a bracket triggers, check it and it folds into an exit leg.

The form previews average entry, net PnL and open quantity as you type. You can attach up to two images (chart screenshots), pick leverage and contract multiplier for derivatives, and write your own feedback on the trade.

**PnL is computed on read** using weighted-average cost, and handles partially open positions. For display you can switch cost basis between **average cost** and **FIFO** (useful for tax export).

## Fees

In **Fees & currency**, save **fee schedules** — fixed or percentage, charged per lot, unit, contract, or trade (e.g. *IBKR stocks: 0.05 % per trade*). Selecting a schedule on a trade auto-computes the fee; a manually entered fee always wins.

## Multi-currency & FX

Trades keep the currency you entered them in. The **breakdown currency** (display) is converted using a daily FX feed that backfills rates automatically each business day, carrying rates forward over weekends and holidays.

If a rate can't be fetched for some date, those trades are **excluded from converted totals** and show up in **Pending tasks** — enter the missing USD-based rates there by hand (1 USD = … of that currency) and the trades count again.

## Breakdown (your stats)

Per category or across all, filterable by date range, ticker, side, asset class, strategy and signal:

- **Equity curve** in the display currency.
- Realized PnL · Return · Win rate · Trades (closed/open) · Expectancy · Profit factor · Avg win / Avg loss · Best / Worst trade · Max drawdown · Sharpe · Total fees · Invested capital · Margin deployed · Return on margin.

## Works with

- **Tax Calculator** — loads your realized journal PnL for a tax year, split into capital / derivative / crypto gains.
- **Dashboard** — a quick-trade widget logs a trade from the home page.
- **RemindMe** — add journal-linked reminders (e.g. weekly review).
