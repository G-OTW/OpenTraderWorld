# AI agents (MCP)

OpenTraderWorld ships a built-in **MCP server** so AI agents (any [MCP](https://modelcontextprotocol.io)-compatible client) can read and update your modules through a controlled gateway. An agent can log journal trades for you, summarize your news feeds, add todos, query your backtest results, and so on.

**It is off by default.** Nothing listens for agents until you enable it.

::: tip Looking for the in-app chat assistant?
This page is about **external** agents connecting *into* OpenTraderWorld. If you want the built-in chat assistant that lives inside the app (bring your own provider), see the [Agent module](/modules/agent): it can *use* this same gateway to reach your data.
:::

## Security model

Several layers, all of which must pass:

1. **Global switch**: the MCP endpoint is disabled until you enable it in **Settings → MCP**. You can prepare tokens while it's off; every agent request is rejected until enabled.
2. **Bearer tokens**: one per agent or use case. Tokens are stored **hashed** and shown only **once** at creation; failed attempts are throttled. Revoke a token any time.
3. **Per-token module permissions**: each token grants *no access*, *read*, *read + write*, or *full (read + write + delete)* **per module**. Agents only discover the modules you granted.
4. **Hard allowlist**: account, network, secrets, file storage and data-wipe operations are **never exposed** to agents, regardless of permissions.

::: tip The allowlist is deliberate, not automatic
An endpoint is reachable by agents only because someone added it to the catalog by hand. A new module, or a new route on an existing one, is **invisible to every agent** until that entry exists, so the gateway can never widen by accident as the app grows. Persona and skill management is kept out on purpose: no agent, and no content an agent reads, can edit a persona or widen its skill shelf.
:::

::: tip Versions work like commits
With versioning on in **Settings → Versioning**, an agent granted **Editor** or **Backtest** can save a version of a document or strategy after updating it, with a note saying what changed, and list, read, restore or delete versions. It can switch versioning per file or strategy, but not the global switches in Settings.
:::

::: warning The Automator grants writing, not arming
Granting **Automator** lets an agent read your workflows, create one, write its graph and test it. It does **not** let it put one into service: a graph an agent saves lands as a draft you adopt from the editor, it cannot attach a workflow's access token, and it cannot run a workflow or touch a schedule. A workflow runs under its own token rather than the caller's, so writing a graph and arming it are two separate grants. See [the module page](/modules/automator#letting-an-agent-build-a-workflow).
:::

## Enable and create a token

1. Go to **Settings → MCP** and switch it on.
2. **New token**: name it after the client (e.g. `My Agent`), set per-module permissions (or use *All read* / *All read+write* / *All full* as a starting point).
3. **Copy the token immediately**: it is shown only once.

**Allow external access** is a separate tick on the same dialog. It grants nothing extra: it only lets that token back a chat binding in [External control](/config/external-control), where a message from Telegram, Slack or Discord runs under these same per-module levels.

The creation dialog also shows a **ready-to-paste configuration snippet**, with a tab per client family: the raw endpoint + header, a `mcpServers` JSON block (Cursor, Cline, Windsurf, VS Code…), and a `claude mcp add` command line. The same snippets stay available below the token table with `<TOKEN>` as a placeholder, for setting up a second machine later.

## Connect a client

The endpoint speaks **MCP over Streamable HTTP** at:

```
POST http://<your-host>/api/mcp
Authorization: Bearer <TOKEN>
```

Any compliant client works. Example for a MCP config:

```json
{
  "mcpServers": {
    "opentraderworld": {
      "type": "http",
      "url": "http://localhost:5454/api/mcp",
      "headers": { "Authorization": "Bearer <TOKEN>" }
    }
  }
}
```

Replace the URL with your domain if you use a LAN/HTTPS mode.

::: tip Localhost-only installs
If the app is reachable only on `localhost` (the default network mode), agents must run **on the same machine**.
:::

## Connect with OAuth (claude.ai, ChatGPT)

Some clients cannot hold a fixed token: the claude.ai and ChatGPT connectors only sign in with OAuth. For them, switch on **OAuth sign-in** at the bottom of **Settings → MCP** (it asks for your password) and give the client the server URL alone, `https://<your-domain>/api/mcp`, with no token.

1. The client registers itself and opens a consent page on your instance (sign in first if needed).
2. The page shows the client's name and **where your answer is sent**. Pick the modules and levels, then **Allow**: your password is asked for every approval.
3. The connection appears in the token table with an **OAuth** badge. Edit its permissions or revoke it there like any token; revoking disconnects the client.

Access tokens last an hour and are renewed in the background; a connection unused for 30 days has to sign in again. If a renewal token is ever replayed by someone else, the connection is revoked and you are notified.

::: warning Remote clients need public HTTPS
claude.ai and ChatGPT connect from their own servers, so the instance must be reachable over public HTTPS ([Web mode](/config/network)). Only approve a consent page you opened yourself, just now: a link sent by someone else can name itself after any client.
:::

::: tip Updated from 0.0.15 or earlier with `otw update`?
OAuth needs a new route in `deploy/Caddyfile`. See [Updating](/guide/updating#oauth-caddyfile).
:::

## How agents see the app

Agents get four gateway tools:

- **`otw_catalog`**: lists the modules and operations the token may call. Only granted modules appear. A module listing shows each operation's method, path, query parameters and top-level body fields; `endpoint` (`POST /api/backtest/run`) returns that one operation's full body schema.
- **`otw_read`**: read operations (need at least *read* on the module).
- **`otw_compute`**: operations marked *(compute)* in the catalog, which answer a question and store nothing: a backtest, a parameter sweep, risk metrics. They need *read + write* like any POST, but your client will not ask you to approve a calculation.
- **`otw_write`**: create and update operations (need *read + write*); **delete** operations require *full* on the module.

Responses that carry text from outside (feed articles, incoming mail) reach the agent inside a labelled block telling it to treat the content as data and to ignore any instruction hidden in it.

The token table in Settings shows each token's last-used time, so you can spot and revoke stale ones. A token can also be given an expiry date when you create or edit it: past that date it stops working everywhere, including for the in-app agent.
