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
- Provider failures read as a plain sentence in a dismissible banner — a rejected key, a rate limit (with the provider's retry-after when it sends one), a wrong model or base URL, a content-filter refusal, or a reply cut off at the token limit.

### Switch provider or model per chat

A compact picker in the chat header shows the active **provider · model**. Open it to switch provider or pick a model from the provider's **live model list** — queried server-side, so your key never reaches the browser. Free text still works for proxies that don't expose a list. Changes save instantly and apply to the next message.

## Memory & skills

Two tabs in settings let the assistant carry knowledge across conversations:

- **Memory** — small, durable facts (a preference, a stable detail) that persist across chats. Only the **index** (slug + one-line description) rides in the prompt; the full content is pulled on demand. You browse, edit and delete every memory yourself — nothing is hidden.
- **Skills** — reusable Markdown instruction sets you define. A skill's **name + description** are always in context; the assistant loads the full body on demand when a task calls for it. Enable/disable each skill individually.

Long conversations also get a **rolling summary**: once a chat grows large, older turns are compressed into a running summary so the thread stays cheap, keeping only the most recent messages verbatim.

## Tools over your data

Attach an **MCP token** to a conversation and the assistant can read and update your modules through the [same in-process gateway](/config/ai-agents) that external MCP clients use. The token's **per-module permission levels** (Read / Read+write / Full, set in **Settings → MCP**) apply **directly** — the token *is* the permission envelope; there is no second agent-side gate. Settings, secrets, network and data-wipe operations are never exposed, and there is no shell or filesystem access by construction.

Tool calls appear inline as **collapsible chips** showing the arguments and result. A run is bounded at 15 tool rounds.

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
