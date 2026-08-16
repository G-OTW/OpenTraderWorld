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

## Enable and create a token

1. Go to **Settings → MCP** and switch it on.
2. **New token**: name it after the client (e.g. `My Agent`), set per-module permissions (or use *All read* / *All read+write* / *All full* as a starting point).
3. **Copy the token immediately**: it is shown only once.

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

## How agents see the app

Agents get three gateway tools:

- **`otw_catalog`**: lists the modules and operations the token may call. Only granted modules appear.
- **`otw_read`**: read operations (need at least *read* on the module).
- **`otw_write`**: create and update operations (need *read + write*); **delete** operations require *full* on the module.

The token table in Settings shows each token's last-used time, so you can spot and revoke stale ones.
