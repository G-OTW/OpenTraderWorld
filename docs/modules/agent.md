# Agent

A built-in **AI chat assistant** — a chat pane inside the app that can also act on your OpenTraderWorld data. Open it from the **Agent** module or the sparkles shortcut in the top bar (next to the search).

The assistant is **bring-your-own-provider**: nothing is enabled or vendor-defaulted until you add a provider and key of your own.

::: tip Two different "AI agents"
This page is the **chat assistant that lives in the app** and talks to a provider *you* configure. That is not the same as the [AI agents (MCP)](/config/ai-agents) page, which is about *external* agents connecting **into** OpenTraderWorld through the outbound MCP server. The chat assistant can *use* that same gateway to reach your data — see [Tools](#tools-over-your-data) below.
:::

## Add a provider

In **Settings** (the gear in the chat sidebar) → **General**, add one or more providers. Two wire formats are supported:

- **Anthropic** — the Claude Messages API.
- **OpenAI-compatible** — any endpoint that speaks the OpenAI chat format: OpenRouter, OpenAI, DeepSeek, Moonshot, Groq, Mistral, Gemini's compatibility endpoint, a local proxy, and so on.

Each provider has its own **base URL** (OpenAI-compatible only), **API key** and **default model**. The key is **write-only** — it is encrypted at rest with the app's master key and never shown again after you save it. Leave the key field blank when editing to keep the current one.

A provider can be disabled without deleting it. The assistant is only "ready" once it has an enabled provider with a key and a model.

## Configure the assistant

In the same settings pane you set the **system prompt**, the active **provider / model**, **max tokens** and **temperature**. An **Advanced parameters (JSON)** field passes any extra request field verbatim to the provider — set a value to `null` to *remove* a key the app would otherwise send (e.g. `max_completion_tokens` for newer OpenAI models, or dropping `stream_options`).

## Chat

- Replies **stream live** and render as Markdown. Models that expose reasoning get an optional **Thinking** fold.
- Conversations are saved in the **sidebar** — new, select, rename, delete, and one-click **Markdown export**.
- You can **stop** a run mid-stream.
- The chat header shows a running **token count** (input + output) for the conversation, so you can see what a thread is costing.
- A **wide mode** toggle drops the reading-width limit. The thread is sized for prose, which is the wrong shape for the tables the assistant produces — a trial ledger or a stats breakdown needs the room. The choice sticks per browser.
- Provider failures read as a plain sentence in a dismissible banner — a rejected key, a rate limit (with the provider's retry-after when it sends one), a wrong model or base URL, a content-filter refusal, or a reply cut off at the token limit.

### Switch provider or model per chat

A compact picker in the chat header shows the active **provider · model**. Open it to switch provider or pick a model from the provider's **live model list** — queried server-side, so your key never reaches the browser. Free text still works for proxies that don't expose a list. Changes save instantly and apply to the next message.

The choice belongs to **that conversation**, not to the assistant: a cheap fast model can review your journal in one tab while the strongest reasoning model argues about a backtest in another. A conversation that has made no choice of its own inherits the persona's, then what you set in **Settings → General** — a dot on the picker marks the ones running on something of their own, and one click puts them back on the inherited setting. Switching provider clears the model id along with it, since a model name only means something to the vendor it came from.

## Memory & skills

Two tabs in settings let the assistant carry knowledge across conversations:

- **Memory** — small, durable facts (a preference, a stable detail) that persist across chats. Only the **index** (slug + one-line description) rides in the prompt; the full content is pulled on demand. You browse, edit and delete every memory yourself — nothing is hidden. Each memory records **which persona wrote it**, shown both in the manager and in the index the assistant reads: memory is one shared store, so a constraint the Day Trader wrote would otherwise read to the Analyst as its own. The assistant can also prune memories itself when the store fills up, and cannot silently overwrite one you wrote by hand.
- **Skills** — reusable Markdown instruction sets you define. A skill's **name + description** are always in context; the assistant loads the full body on demand when a task calls for it. Enable/disable each skill individually.

Long conversations also get a **rolling summary**: once a chat grows large, older turns are compressed into a running summary so the thread stays cheap, keeping only the most recent messages verbatim.

## Personas

A **persona** is a role-shaped version of the assistant: a system prompt with a stance and an explicit refusal boundary, plus a curated **skill shelf**. Five ship built-in — **Quant**, **Portfolio Manager**, **Day Trader**, **Researcher**, **Financial Analyst** — and you pick one when you open a conversation, from the persona picker in the chat header.

The idea is narrowness. A generic assistant with two hundred endpoints is worse at any single job than one that knows a few of them deeply and refuses the rest. The Quant will not report a backtest without the trial count and an out-of-sample figure; the Day Trader will not name an entry; the Analyst reports the fundamentals it *could not* obtain rather than filling them in.

### What a persona is not

**A persona is not a permission set.** What the assistant can reach is the conversation's MCP token — module-scoped, set by you, identical whichever persona is speaking. Switching persona narrows the *stance and the shelf*, never the data access. To change what it can touch, change the token.

### Switching mid-conversation

You can switch persona mid-thread. It takes effect **from the next message**, and a marker lands in the transcript recording the handover — the turns above it were produced by the previous persona and stay attributed to it.

### Editing your own

**Settings → Personas** lists every persona with the shelf it will actually get. From there you can:

- **create** one from blank, or **duplicate** a shipped one and rewrite the copy;
- edit the **prompt**, the **shelf**, and whether it **auto-approves writes**;
- **reset** a built-in back to shipped (your edits to it are lost, nothing else is touched);
- **delete** one you made. Its conversations are **kept** — they move to the default assistant, and each transcript gets a note saying so. Built-ins cannot be deleted: the app re-creates them on the next restart, so a delete would only appear to work.

Skills are **not** created here. There is one catalog, managed in the Skills tab, and personas pick from it. That means editing a skill body changes it for every persona holding it — the skills list shows how many, so the edit is never blind.

### Export and import

Any persona exports as a **JSON file with the skill bodies inline**, so one file reproduces it on another box. You can also export a single skill, or the whole shelf at once. Import accepts either shape; an existing name is skipped rather than overwritten. There is no review gate — this is your box, and what you load into your own assistant is your call.

Persona and skill management is deliberately **absent from the MCP catalog**: no agent, and no content an agent reads, can edit a persona or widen a shelf.

## Write confirmation

When the assistant wants to change your data, the run **pauses** and shows you the exact call — method, path and body — with Approve and Decline. Nothing is written until you answer, and declining is reported back to the model as a refusal rather than an error to route around. If you walk away, the wait times out and the write does not happen.

A persona can be set to **auto-approve writes**, which skips the prompt for ordinary changes. **Deletions always ask**, whatever that setting says — ticking the box was a decision about routine writes, not permission to erase a journal.

Endpoints that only *compute* (a backtest, a Sharpe ratio, a Monte-Carlo) do not prompt. They change nothing you would miss, and a confirmation dialog on every calculation is how people learn to click Approve without reading it.

## Tools over your data

Attach an **MCP token** to a conversation and the assistant can read and update your modules through the [same in-process gateway](/config/ai-agents) that external MCP clients use. The token's **per-module permission levels** (Read / Read+write / Full, set in **Settings → MCP**) apply **directly** — the token *is* the permission envelope; there is no second agent-side gate. Settings, secrets, network and data-wipe operations are never exposed, and there is no shell or filesystem access by construction.

Tool calls appear inline as **collapsible chips** showing the arguments and result. A run is bounded at 15 tool rounds, and each conversation carries a **simulation budget** — backtests and sweeps are the one thing an assistant can spend without limit, so once the budget is gone it is told to stop searching and report what it has, including how many trials it ran.

### Writing your own skill

A skill is one procedure, not a manual. The shape that works:

- a **description** that says *when* to reach for it — that line is the retrieval key and rides in every prompt, so "Use when the user proposes a strategy" beats "About strategies";
- a **body** that names the exact endpoints, step by step, with the specific ways the task goes wrong in this app;
- a **verification** step: how to prove the result before reporting it;
- a **report shape**: what the reply must contain.

Keep the body short. It lands whole in the context window when loaded, so a long one crowds out the task it was meant to help with — the editor warns you past roughly two thousand words.

### Per-conversation tools

Each conversation carries **its own** MCP token (the token you set in settings is just the default for new conversations), switchable from a **tools dropdown** in the chat header. Two conversations can run with different data scopes side by side. The dropdown:

- has a **search box** to filter tokens and external servers by name;
- notes when the selected token grants **write/delete**;
- offers inline actions to **add an MCP server** and quick-links to **Settings → MCP** (create/manage tokens) and the **MCP store**.

## MCP store — connect external platforms

**Agent → Manage servers** is a full-page section for adding remote MCP servers so the assistant can reach outside platforms:

- a **curated catalog** of well-known servers (DeepWiki, Context7, GitHub, Hugging Face — bring your own key), plus **custom servers** by URL;
- **Streamable-HTTP only** — nothing ever executes locally;
- auth values are **encrypted at rest and write-only**;
- a **Test** button connects and lists the server's tools;
- enable a server per conversation from the tools dropdown.

External tools are **namespaced** (e.g. `deepwiki__ask_question`) and labeled with their server, calls are time-boxed, and an unreachable server **degrades to a warning** instead of blocking the chat.

::: warning External content is untrusted
An external MCP server sees your conversation, and what it returns is third-party content. Combining an external server with a token that grants **write access** to your data means injected content could try to trigger changes — the tools dropdown warns you when that combination is active. Only add servers you trust, and keep an eye on the tool-call chips.
:::
