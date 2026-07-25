# Dashboard & navigation

The home screen of the app, plus the two things that sit above every module: the search box and the notification inbox.

## Dashboard pages

The dashboard opens on a built-in **Modules** page — a tile per installed module, rebuilt automatically as you install and detach. It is never edited or deleted; it just reflects what you have.

On top of that you create **your own pages**. Each has a name, an optional description, and a short **tag** shown on its chip. One page is the **default** — the one the dashboard opens on, and the one whose chip sorts first.

Use them the way a trading day splits: a *Morning* page with the news feed, the economic calendar and the routine checklist; a *Positions* page with the portfolio and the watchlist; an *Admin* page with todos and timers.

## Editing a layout

**Edit layout** turns a page into a grid of rows over 12 columns. In edit mode you can:

- **add rows** and drop **module tiles** (a link to a module — the same module may appear on any number of pages) or **widgets** into them;
- **resize** any tile by column span, and drag tiles between rows;
- set a widget's **height preset** — compact, standard or tall — and open its **config** (the gear on the tile);
- insert **spacer rows** to breathe between blocks.

Tiles are links, not copies: removing one from a page never touches the module or its data.

## Widgets

A widget is a live, interactive preview of a module — it reads and writes through that module's own API, so what you do in the widget is real. Widgets whose module isn't installed simply aren't offered.

| Widget | What it does |
|---|---|
| **Free text** | A note or heading you write yourself — markdown-lite. |
| **News feed** | Latest items from a chosen feed, as a list or grid. |
| **Time tracker** | Start/stop a project timer without leaving the page. |
| **Quick trade** | Pick a category + template and open the add-trade form. |
| **Goals** | A short list of goals with progress; add one inline. |
| **ToDo** | Open tasks, tickable in place. |
| **Trading routine** | Today's checklist, tickable in place. |
| **Mindset** | The day's check-in. |
| **Reminder** | A quick add-reminder form. |
| **Calendar** | Today & this week at a glance. |
| **Economic calendar** | Upcoming macro events, compressed. |
| **Portfolio** | A portfolio summary with live value. |
| **Watchlist** | Live quotes for a chosen list — price, 24h and 7d change. |
| **Prompt store** | Your prompts by tag — click one to copy it. |
| **Resources** | Bookmarks from a chosen category. |
| **Agent** | Ask the assistant — pick model and tools, send, land in the conversation. |

## Global search

The search box in the top bar, focused from anywhere with <kbd>⌘K</kbd> / <kbd>Ctrl+K</kbd> — or plain <kbd>/</kbd> when you aren't typing in a field.

By default it matches **module names**, **Settings sections** and **Resources** entries. The **layers toggle** next to the box widens it to your content: Editor pages, Goals, Calendar events, ToDos, Routines, Reminders, Prompts and Community Docs.

Two things it deliberately does not do: it matches **titles and names only, never bodies**, and it only searches modules you have installed. Results come back grouped by type, prefix matches first; <kbd>↑</kbd>/<kbd>↓</kbd> and <kbd>Enter</kbd> navigate them.

## Notifications

The bell in the top bar carries an unread count and opens the **notification inbox** — where [RemindMe](/modules/productivity#remindme) reminders land when they fire, along with anything an inbound [webhook](/modules/productivity#webhooks) redirects there. A notification that fires while you're in the app also slides in as a banner.

Delivery to **email, Telegram, Slack or Discord** is configured per channel in RemindMe; the inbox itself is always on and needs no setup.
