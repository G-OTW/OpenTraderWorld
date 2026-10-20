# Automator

*Workflows that run your own app.* A workflow is a list of steps: call an endpoint of your OpenTraderWorld API, call any URL outside it, ask your AI provider one question, reshape the answer, notify yourself. Run it by hand, or on a schedule.

Typical uses: a Monday pre-market brief that reads the week's economic events and your watchlists and pushes one Telegram message; a nightly export of the journal to an external service; a price webhook fanned out into a notification; a weekly summary written by the assistant from your own numbers.

The module lives at **/automator** with four sections: **Workflows**, **Schedules**, **Agenda** (a calendar of the runs to come), **Runs**.

## The grid

The editor is a grid, not a canvas: no wires to draw.

- **Steps run top to bottom, tasks inside a step run left to right.** Everything runs one after another. A step groups the tasks belonging to the same moment of the workflow, it does not start them together.
- Drag a block from the palette onto a **gap**: the gap between two steps opens a new step, the gap between two tasks drops it into that step. Every valid drop target is highlighted before you release.
- A task can only read what ran **before** it. Move a card and its references are checked again on the next save.
- **Autosave** 1.2 s after each edit, **Ctrl/Cmd+Z** to undo. A block still being filled in is kept as a **draft**: it never runs and no schedule picks it up until it validates.

## Blocks

| Block | What it does |
|---|---|
| **App call** | An endpoint of your own API, run in-process. The palette lists the endpoints a workflow may reach; click one and the block lands already pointed at it, with the expected body shape on hand. |
| **Web call** | Any URL outside the app: method, query, headers, JSON/text/form body, answer read as JSON, text, CSV or binary, with a response size limit. |
| **AI step** | One model turn, no conversation around it. Pick a persona and a stored prompt or write the instructions; ask for a **JSON object** when the next block has to read fields rather than prose. It calls your provider and costs tokens on every run. |
| **Transform** | Reshape what came before: `pick` one value, `set` an object, `format` a string, `csv_parse` a CSV, `join` a list into one line. |
| **Notify** | An in-app notification and your [notification channels](/config/settings#notifications) (email, Telegram, Slack, Discord). Nothing selected means every enabled channel granted to the Automator. |
| **Wait** | Pause the run. **Stop** still answers during the wait. |

## Passing data between blocks

Each block has an **id**, shown at the top of its editor. A later block reads its result with an expression:

```
{{steps.http1.output}}                     the whole answer
{{steps.http1.output.items.0.name}}        one field of it
{{steps.http1.status}}                     ok | simulated | failed | skipped
{{steps.http1.error}}                      the failure message, empty on success
{{run.started_at}} {{run.trigger}} {{workflow.name}} {{input.key}}
```

Filters chain after a pipe: `json`, `upper`, `lower`, `trim`, `round:2`, `date:"DD/MM/YYYY"`, `default:"n/a"`. Only `default` rescues a value that is not there.

This is **not a language**: no arithmetic, no code. Every reference is checked **when the graph is saved**, against the blocks that really precede it, so a broken link is refused in the editor rather than at three in the morning.

## Secrets

A password or an API key belongs in the [Vault](/config/settings#vault), never typed into a field. Reference it with <code v-pre>{{vault.myvault.mykey}}</code> in a header, a query value or a request body, the only places it is accepted (never in a URL), and the resolved value is scrubbed from the run history. A query value still reaches the logs of the site you call, so prefer a header.

## Permissions

- **App call needs an access token.** Pick one in the workflow's **Settings**; tokens are created in [Settings → AI agents](/config/ai-agents). No token means no internal call at all, and a workflow can only reach what its token grants, within the same allowlist the MCP gateway uses.
- **Web call refuses your own network.** Loopback, private, CGNAT and link-local addresses are rejected unless the block explicitly allows internal targets. The host is resolved first and the connection is pinned to the address that was checked, and every redirect hop is checked again.
- **Notify needs a grant.** A channel offers itself here only after the Automator has been granted it in Settings → Notifications.

## Trying it, then running it

- **Test run** performs the reads and reports what a write or a send *would* have done, so nothing leaves the app. A block reading a value that only a simulated write could have produced is itself reported as **simulated**, rather than failing a test a real run would pass.
- **Test this block** runs one block alone, same rules.
- **Run now** does it for real. **Stop** is checked between blocks and during a wait; a run past the workflow's **run limit** is closed as a timeout.

## Runs

Every run keeps **one line per block**: what was sent, what came back, the status and the timing, so a workflow that failed at 3 a.m. names the block and shows the payload. An output over 256 KB is parked and fetched on demand. History is trimmed to the last 50 runs per workflow (10 for test runs).

## Schedules

Every N minutes, daily, some days of the week, monthly, or once at a given moment, each with its own **IANA timezone**, so a rule set in Europe/Paris follows Paris and not the server.

- **No overlap**: if a run is still going when the next occurrence falls due, that occurrence is **dropped**, not queued.
- **Catch up** (optional): if the app was down at the planned time, run once at startup.
- Daylight saving is resolved, not ignored: a skipped local hour runs at the end of the gap, a doubled one runs once. A monthly rule set past the end of a short month runs on its last day.
- A schedule can be **pinned to a version** of the workflow. Saving a new graph asks whether the pinned schedules should follow it; autosave never repoints one on its own.
- **Pause / resume** from the workflow card or the Schedules list. A disabled workflow is never started by a schedule, though running it by hand still works.

::: warning A scheduled write writes
An App call pointed at an endpoint that changes your data will do so on every run, unattended. The editor flags those endpoints; test the workflow before scheduling it.
:::

## Letting an agent build a workflow

Composing a graph is the hardest thing this module asks of you, and it is exactly the kind
of work an [AI agent](/config/ai-agents) is good at. So an agent holding a token with the
**Automator** permission can read your workflows, create one, write its graph and test it.

What it cannot do is put one into service. The split is deliberate:

- A graph an agent saves lands as a **draft**, never as the graph that runs. Open the
  workflow, read what it wrote, and Save to adopt it. Until you do, nothing changes: a
  workflow already on a schedule keeps running the version you saved.
- An agent cannot attach a workflow's **access token**. That envelope is what turns a graph
  into permissions, so you grant it by hand, after reading the graph it will run.
- An agent cannot **run** a workflow, restore a revision, delete one, or touch a schedule.
- Its **test runs** are sealed: notifications and external calls are forced off, and with no
  envelope attached an App call fails closed. A test checks that the graph runs, its
  expressions resolve and its transforms do what they claim, without reaching anything. A
  workflow that already carries a token is refused: that one you test yourself.

The reason for the line is that a workflow runs under **its own** token, not the caller's.
An agent able to both write a graph and start it would inherit whatever that token grants,
whatever its own permissions said. Writing and arming are two grants, and only one of them
is yours to delegate.

The Automator is disabled in [demo mode](/guide/demo).
