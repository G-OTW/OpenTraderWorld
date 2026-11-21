# Trading Bots {#trading-robots}

A **trading bot** is a Backtest strategy running forward. It freezes the strategy's settings, runs at each close of the strategy's timeframe on a set of instruments, and either simulates its fills or sends its orders to a broker account. On top of the strategy, each bot has its own money (a **capital policy**), **exclusions**, **guards** that react to its own results, and a **priority**.

There is no second engine: every run reads the window with the ordinary backtest and acts only on the candle that just closed. A signal bot carries the positions it took as it took them; what the window shows on earlier candles is history, never recorded or sent.

The strategy gives the signals, its stop and its target, and its order types. The bot gives the money and the calendar: its [capital policy](#capital-policy) and its [exclusions](#exclusions). The instrument (point value, price tick, size step, minimum) is the venue's. The strategy's capital, size, day and session filters and costs are not read.

The module needs **Backtest** installed. It lives at **/robots**, as **Trading Bots** in the module switcher.

## The fleet

The fleet page lists every bot: cards on a phone, a sortable table on a desktop. It refreshes every 10 seconds while the page is visible, and the countdowns tick every second.

- **Key figures**: **Bots** (with how many are active), **Equity** (split by bot), **Today** (today's P&L, and the unread events), **Open** (open positions, and how many bots hold one).
- **Fleet P&L**: the summed equity curve of the bots, with **P&L**, **Realized** and **Unrealized**. Filter it by environment (**Every environment**, Simulated, Paper, Testnet, Live) and by a **From** and **To** date. With no end date, the last point is marked to the open positions.
- **Ranking**: the bots ordered by the P&L of the trades they closed **Today**, over **1W**, **1M** or **All**, with the return on their starting equity.
- **Exposure by asset type**: what the open positions put to work at the last price, split by asset type (crypto, futures, forex, stocks...), with how many bots hold each.
- **State filters**: one click on a chip shows the bots in that state (**Active**, **Held by a guard**, **Stopped**, **Disarmed, arm it again**, **Degraded**, **Blocked**, **Broker unreachable**, **Error**), with a count on each. A second click clears it. The account picker narrows to **Simulated** or one account.
- **Needs attention**: the bots with an error, a guard holding them, or unread events, each with the reason.
- **Next ticks**: the eight bots due soonest, with their instruments and a countdown.
- **Mark N as read** clears the unread events of every bot. **New bot** opens the wizard.

## Create a bot {#create-a-robot}

**New bot** on the fleet page, or **Run as bot** in Backtest: on a result (the run exactly as it ran) or on a saved strategy in the library. Either way the wizard opens, one step at a time:

1. **Strategy**: the bot's name, and a saved strategy to freeze (or the run it came from). **Create a strategy** opens Backtest. A later edit of the strategy changes nothing in the bot until you update it (see [Settings](#settings)).
2. **Instruments**: **Add an instrument** opens the chart's data picker, which searches each provider's symbol lookup. No stored history is needed: each run fetches the candles the strategy reads. Up to 8 instruments, on one **Timeframe** they all share. Each instrument is read through the data source it was picked from, which **Bot settings** can change later. Where the provider streams a live price, stops, targets and limits trigger on it; elsewhere they are checked at candle close. The bot's settings say which applies to each instrument.
3. **Account**: **Simulate** (nothing is sent anywhere) or **Broker account** (see [On a broker account](#on-a-broker-account)).
4. **Capital**: the [capital policy](#capital-policy).
5. **Guards**: the [guards](#guards). A new bot starts with the default set.
6. **Schedule and notifications**: the bot runs at each close of its candles and decides on the candle that just closed. The step holds its [**Exclusions**](#exclusions), an optional **Status report** (**Send a status report**, then when it goes out: every N minutes, daily, weekly, monthly or once, in a time zone), **Trailing bars** (how much history each run feeds the engine, 5000 by default), **Priority**, **Notify** (**Every run**, **Only on a new fill**, **Never**) and **Channels**.
7. **Review**, then **Create the bot**.

The server checks everything again when the bot is created. A refusal is shown as it was said, and names what to change.

The bot starts at once. The **Starting** *bot name* window follows its first run while it downloads its history and learns its book; **Run in background** closes it and opens the bot's page while the run goes on. Once it is ready, a simulated bot runs at each close from then on, and a broker bot offers **Arm** in the same window (see [On a broker account](#on-a-broker-account)). **Open the bot** opens its page.

Channels are the notification channels granted to Trading Bots; the channel button on the step adds or manages them without leaving the wizard. With no channel picked, the bot sends to every channel granted to Trading Bots; with none granted, nothing is sent and every event is still recorded in the bot's log. **Notify** **Never** sends nothing at all. The status report gives the bot's state, its figures since the last report and what it holds; it is not sent while the bot is excluded and holds nothing. **Bot settings** also hold alerts on entries and exits per instrument, and your own wording for the entry and exit messages, and for the summary when a status report is on.

### Exclusions

Exclusions are periods when the bot takes no new entry. Exits are still managed inside one. They are read in the bot's time zone, and any mix works:

- **Whole days**: the weekdays picked.
- **Hour ranges**: from one time to another, the end excluded; a range that ends before it starts runs past midnight.
- **Date ranges**: from one date to another, both days included.

Up to 20 hour ranges and 20 date ranges. With **Leave the market**, the bot closes its positions when a period starts, and trades again when it ends. Entering and leaving a period is in the log, and goes to the bot's channels when it holds a position.

## A bot's page {#a-robots-page}

The header keeps the bot's state in sight: its name, the strategy and its version, badges for the state, the environment of a broker bot (**Paper**, **Testnet**, **Live**), the guard (paused, halted, or the scale such as x0.5) and, on a broker account, **Armed** or **Disarmed**. Below it: the next evaluation as a countdown, the last evaluation and the last sync.

- **Run now** runs the bot at once. **Stop strategy** pauses its schedule; it does not close positions or clear orders. **Start strategy** resumes it.
- **Insights** reads the bot's closed trades, see [Insights](/modules/journal#insights).
- While a guard holds the bot, a notice says why, with **Resume**.

The page refreshes every few seconds; open positions follow the live price where the provider streams one. Eight tabs:

| Tab | Shows |
|---|---|
| **Overview** | **Equity**, **Realized P&L**, **Unrealized P&L**, **Today** (P&L and orders sent), **Win rate**, **Profit factor**, **Max drawdown**, **Expectancy**; the equity curve; the latest price of each instrument (live, stored candle, or stale); **P&L by instrument** and **P&L by direction**; the P&L calendar; a chart of the strategy's timeframe with entries and exits. A broker bot also shows its [execution panel](#on-a-broker-account) |
| **Positions** | instrument, side, quantity at the bot's size, average price, price (live or stored), unrealized, stop, target and age |
| **Pending** | a simulated bot's orders waiting for the next open; a broker bot's orders working at the broker |
| **Trades** | the closed trades, 25 a page, with the guard's scale where it applied. **Not taken** counts the entries the bot refused and lists the latest 10, each with its reason |
| **Statistics** | the trade statistics, the performance curve, and for signal strategies a 3D scatter and the streaks |
| **Guards** | the rules as sentences, what they hold now, and the timeline of the latest 100 triggers, restorations and resumes, newest first |
| **Log** | the latest 200 events: entries, exits, refusals, guard actions, errors |
| **Settings** | the strategy, account, schedule, priority, capital and guards, with **Bot settings**, the update to the strategy's version, and **Delete this bot** |

A bot's book starts with its first run: only what happens from the candle that just closed is recorded. What the window shows before it is history, never in its book, its statistics or its guards.

### Settings

**Bot settings** edits the name, the data source of each instrument (**Provider default**, or a connector granted to Trading Bots), the exclusions, the status report, the window, priority, capital, guards and notifications. The strategy, the instruments and the account are frozen: another strategy or account is a new bot. The **Traded forward** note lists the instruments and the strategy's entry and exit order types: its signals, stop and target run as they were frozen, while its capital, size, calendar and costs are not read.

**Update to the strategy's current version** (shown when the bot came from a saved strategy) takes the saved strategy as it reads now. The bot's book is cleared and learned again at the next run, and an armed bot is disarmed.

Deleting a bot deletes its book, its guard timeline and its log.

## Capital policy

The bot's money sizes every decision of the strategy: the strategy runs on the bot's capital, not its own. Every field is optional:

| Field | Meaning |
|---|---|
| **Allocation** | the capital the bot trades with. Empty: the strategy's capital and size |
| **Currency** | the account's currency, for reading the figures |
| **Share in one position (%)** | one position is worth this share of the bot's capital. Needs an allocation |
| **Leverage** | the bot sizes with it, up to 125. Empty: 1 |
| **Entries per position** | how many entries one position may take, up to 50. Empty: 1. A broker bot takes 1 only |
| **Most open positions** | an entry past it is not taken |
| **Most orders a day** | entries counted per UTC day. Exits are never held back |

Each entry is then multiplied by a guard's scale and cut down to the venue's size step (never up). On a broker account, the **Maximum position (units)** set at creation refuses a larger entry instead of cutting it.

**A refused entry names its fix**, in the Trades tab and the log:

> the bot already holds 3 position(s), its maximum of 3: the entry on BTCUSDT was not taken. Raise the maximum of positions

The others read the same way: a size under the venue's step (raise the allocation, the share in one position or the leverage) or under its minimum (raise the allocation, the share in one position or the strategy's size), the day's maximum of orders, a size over the maximum position.

## Guards

Guards are rules over the bot's **own closed trades**, at the bot's size. Up to 10 per bot. Exits are always managed, whatever a guard holds.

Each guard pairs one condition with one action, any with any.

| When | Then |
|---|---|
| **consecutive losses**, **consecutive wins** (a count) | **pause** for days or hours (one minute to 365 days) |
| **drawdown %**, **from the peak** or **from the start** | **halt until resumed** |
| **loss % over**, **loss amount over**, **losses over**, **gain % over** a window: **today**, **this week**, **this month**, **since the start** (in the bot's time zone) | **scale the size down**: **Divide by** more than 1 and at most 10, **Back to full size** **after trades**, **after wins**, **after days** or **by hand** |

The default set offered to a new bot: pause one day after 3 consecutive losses, and halve the size after a 10% drawdown from the peak, back to full size after 5 trades.

- A paused or halted bot takes no new entry and keeps managing its positions. A scaled bot sizes its next entries down.
- A pause ends on its own. A halt waits for **Resume**. A scale comes back to full size by its restore rule, in one step.
- A count rule counts again from its last firing: a pause after three losses fires again after three more. A level rule fires when the level is crossed, and again once it was no longer met or its window rolled.
- Several at once: a halt beats a pause, a pause beats a scale, and a scale fired with them applies when entries resume. A scale on a scale multiplies, never below x0.1.
- Percentages are of the bot's starting equity (the allocation, else the strategy's starting capital). A drawdown from the peak is of the peak equity; a loss or gain over today, this week or this month is of the equity when that period began.
- **Resume** clears everything a guard holds. Editing the rules keeps what is held now.

Every trigger and restoration is in the **Guards** tab and the log, and goes to the bot's channels.

## Priority and rate limits

Each bot has a **Priority** from 0 to 1000, 100 by default. **Lower runs first** when several bots are due at once or share a broker. Bots due at the same close run side by side, up to 8 at once.

Bots on a broker account are synced every 15 seconds, broker by broker, in priority order:

- one pass sends at most 10 orders to one broker, bot by bot in priority order. Exits and closes are never held back by that limit; an entry past it waits for the next pass, and the bot says so;
- when a broker refuses requests for its rate limit, **only the bots on that broker wait**: until the time it gave (an hour at most), or for one pass when it gave none. Each bot shows a note naming the wait and what to change (fewer bots or instruments on the account, or a higher limit on its plan). Bots on other brokers go on.

## On a broker account

A bot can send its orders to any account with [trading](/config/trading) on and granted to **Trading Bots** in [Settings → Brokers](/config/brokers), including a [paper trading](/config/paper-trading) account. The environment badge says where they go: **Paper** (the app's paper broker), **Testnet** (a venue's test or demo account), **Live** (real money).

The engine decides at each close and the account executes. At creation:

- only a signal strategy executes on a broker, with one entry per position (**Entries per position** 1);
- each instrument is looked up at the account, which shows the symbol and the size step it found. An instrument the account cannot name is refused: the bot never guesses;
- **Exits**: **At the broker** (the stop and the target rest at the broker, attached to the entry, where the broker can hold them) or **Watched** (checked on the live price and closed at the market when crossed);
- several bots may name one instrument on one account, but only one of them is armed at a time, so two bots never trade one position against each other.

A broker bot is **created disarmed**: nothing is sent until you **Arm** it, in the **Starting** *bot name* window once its first run is done, or later from its page. Arming needs **background trading** signed on the account (**Sign background trading** opens the terms to sign), and your authenticator code when the [trading lock](/config/trading#trading-lock) is on and the account is not paper. From the next close, entries go at the market and exits as the strategy decides, with nobody watching. Every order passes the account's [risk limits](/config/trading#risk-limits) and is in the [audit log](/config/trading#audit-log).

Arming reads the account first: it is refused when the broker cannot be reached, or when the account holds less than the bot's money (its allocation, else the strategy's capital). Each bot is checked on its own, so several bots may share one account; an entry the account cannot pay for later is not sent, and the log says what it needed and what the account had.

The **execution panel** on the Overview shows the account's level, its limits, the engine's book beside the account's, and each trade's simulated price beside the broker's. Its state:

| State | Means |
|---|---|
| **Disarmed** | nothing is sent until you arm the bot |
| **In sync** | the account holds what the bot expects |
| **Degraded** | an entry filled only in part. The bot adds no exposure and keeps managing its exits until you flatten or adopt |
| **Stopped, arm it again** | a position was opened by hand on the bot's instrument while the bot was flat, or the bot's position was closed, added to or reduced by hand. The bot was disarmed and its resting entries cancelled; a closed position's exits are cancelled too. Arm it again to go on |
| **Sending paused** | a guard holds new entries, or trading is halted on the account: only what takes risk off is sent |
| **Check at the broker** | an order got no answer: nothing more is sent on that instrument until it is settled |
| **Broker unreachable** | the broker did not answer three syncs in a row (45 s). Nothing is sent until it answers, and the account is read again before anything is |
| **Account error** | the account cannot be used (its credentials or its grant): nothing is sent until it can |

The bot reads the account at every sync (every 15 s, and first thing after a restart) before it sends anything. Any change you make by hand to the bot's position stops it until you arm it again. A stop or a target you move by hand at the broker stays where you put it: the bot no longer moves it. Cancel both its stop and its target by hand and the next sync places them again, once; after that the bot watches them on the live price. Cancelling any other of its pending orders changes nothing for it. The account's own position check leaves a bot's instrument to the bot: a bot's trades never halt the account.

**Positions** and **Pending** show only what is at the broker. An entry that did not leave (refused by a cap, a guard, the venue's size rules, the account's funds, or the broker) is a line in the **Log**, its exact reason on hover. A closed trade's prices and P&L are what its orders filled at the broker, fees included, once its exit filled; until then the simulated figure stands. A stop or a target that fills at the broker, Stop and flatten, or a close you make at the broker closes the bot's trade at that fill, even when the bot's own candles never reached the level; an entry that never left is not taken. A candle the provider corrects after its close changes nothing: the bot does what it decided at that close, and a reverse is one message.

- **Disarm** stops sending. Orders already at the broker stay there.
- **Stop and flatten** disarms, cancels the bot's working orders and closes what it holds at the market.
- **Adopt the difference** leaves what the account holds outside the bot to you, so a degraded bot trades again.

Arming, disarming, flattening and adopting are always done by you from the page, never by an agent or a workflow.

## Paper sessions

Backtest's paper sessions are now bots, with their book, events, account and channel grants. An old paper session link opens the bot's page. **Run as bot** replaces **Paper trade** in Backtest.

## Good to know

- [AI agents (MCP)](/config/ai-agents) can read bots (the list, one bot, its dashboard, its trades, its guard events, its execution panel, the fleet, the inbox and the message variables). Creating, editing, running or arming one stays in the app.
- A bot reads each instrument's candles and live price through the data source picked for it, else a [data connector](/config/connectors) granted to Trading Bots, else one granted to Backtest, else the provider's first connector.
- In [demo mode](/guide/demo) the module is read only: two seeded bots on a demo paper account, one scaled down by its guard and one paused, neither armed nor running.
- How a simulated bot fills on the live price, and the options it shares with the backtest: [Backtest](/modules/market-data#backtest).
