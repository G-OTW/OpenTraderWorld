# Notes & organization

The everyday modules: documents, tasks, goals, calendar, reminders — plus three built for a trader's discipline.

## Editor {#editor}

A rich document editor in the Notion style. Documents live in a folder tree; type `/` for the block menu.

- **Blocks** — headings, lists, to-do lists, quotes, code blocks, dividers, links, images (uploaded or by URL), text color, highlight, font size, normal or full width. Saves automatically.
- **Databases** — a document type with typed columns (text, select, URL…), viewable as **table**, **kanban** (grouped by a select column) or **gallery** (with a cover-image column). Drag to reorder rows and columns.
- **Submit for publication** — send a document to the [Community Docs](/modules/news-research#community-docs) review queue, formatting preserved, with language, categories and optional author credit.

## ToDo {#todos}

A task list that stays out of the way: tasks with due date, time, category and notes. Filter by pending/done/overdue, sort by due date; overdue/today/soon flags do the nagging. A dashboard widget shows what's open.

## Goals {#goals}

Goals with **measurable metrics**. Give each goal a deadline, a category, and one or more metrics with a current value, target and points — increment them as you progress, and the goal's completion follows the points. Filter open/reached/overdue; drag to order.

## Calendar {#calendar}

A personal calendar (year/month/week/day) for events with category, color, location and notes. Its trick is **overlays**: it can also display your **Reminders**, **ToDos with a due date** and **Goal deadlines**, each toggleable — one place to see the week. Creating an event can also create a synced reminder at the start time.

## RemindMe {#remindme}

Reminders — one-off or recurring (with start date, end date or max count) — that fire as **in-app notifications** with a notification inbox.

- **Linked reminders** — attach a reminder to an item in another module (a goal, a subscription's billing, a journal review…) and it deep-links back to it. Most modules have an *Add reminder* button that pre-fills this.
- **Channels** — also deliver to **email, Telegram, Slack or Discord**. You bring your own bot/webhook credentials (stored encrypted); test-send before relying on one.

## Webhooks {#webhooks}

Give any external service a private URL to **POST alerts into OpenTraderWorld** — TradingView alerts, broker notifications, uptime monitors, anything that can fire an HTTP request. The payload is received and routed into a module.

- **Private URL, no headers** — each endpoint carries a **256-bit token in the URL path** (`/api/hooks/<token>`), because many alerting senders can't set an `Authorization` header. Tokens are stored **hashed** and shown **once** at creation; failed lookups are throttled.
- **Liberal payloads** — send plain text or JSON; the parser accepts loose field names, so most senders work without special formatting.
- **Routing** — each endpoint redirects its payload to a target module. The v1 target is **[RemindMe](#remindme)**: an incoming payload becomes an in-app notification, pushed on to your enabled channels (email/Telegram/Slack/Discord).
- **Delivery log** — the most recent deliveries per endpoint are kept so you can confirm a sender is reaching you and see what it sent.

Manage endpoints at **/webhooks**.

## Trading Routines {#routines}

Recurring **session checklists** due on the weekdays you pick — pre-market prep, in-session discipline, post-market review. Tick items off per day, browse past days, and watch the **14-day consistency strip** to see whether you actually stick to your process. Starter checklists are included.

## Time Tracker {#time}

Projects with start/stop **timers** (or manually added ranges), optional **time budgets** with over-budget warnings, planned end dates, and an **hourly rate** to value the time. The **Breakdown** tab charts tracked hours by day/week/month, filterable by project and category. If a timer was left running while the app was closed, it asks whether to keep or revert that time.

## Mindset {#mindset}

A daily **check-in** for the trader's psyche: answer a few prompts before or after the session — scales (focus, discipline), choices (calm / anxious / FOMO), free text. Prompts are **fully customizable**; a starter set is included. The **Trends** view charts your answers over recent check-ins, and History lets you reread any day.

## Prompt Store {#prompt-store}

A library for the **AI prompts** you reuse — market recaps, journaling questions, research templates. Prompts show as a grid of vignettes (name, tags, last-saved) with a search over name, tags and body.

- **Tags** — add free-form tags in the editor; filter the grid with the tag bar.
- **Rate & filter** — give a prompt a thumbs up or down and quickly filter to either.
- **Version history** — every save is kept; open a prompt's **History** to preview any earlier revision and **roll back** to it (the restore is saved as a new version, so nothing is lost).
- **Duplicate** — fork a prompt to branch a variant.
