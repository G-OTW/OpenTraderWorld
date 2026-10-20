/**
 * Curated MCP server catalog — a small, reviewable list of well-known REMOTE
 * (Streamable HTTP) MCP servers. Entries only pre-fill the add-server form; nothing is
 * installed automatically and credentials are always the user's own. Contributions via
 * PR; keep it to reputable, documented, HTTP-transport servers (no stdio/npm packages).
 *
 * Fields: id (stable slug), name, url, description key handled in the page (plain text
 * here to keep the catalog self-contained), authHeader/authHint ('' = no auth), docsUrl.
 *
 * `selfHosted: true` marks the built-in OpenTraderWorld gateway: it is not "added" as a
 * remote server (the agent reaches it in-process), so its card links to Settings → AI
 * agents instead of offering an Add button.
 */
export const MCP_CATALOG = [
  {
    id: 'opentraderworld',
    name: 'OpenTraderWorld',
    url: '/api/mcp',
    description:
      'This instance: journal, portfolios, watchlists, news and more, exposed to the agent through the built-in gateway.',
    authHeader: 'Authorization',
    authHint: 'Enable it and mint a token in Settings → AI agents.',
    docsUrl: '/settings#mcp',
    selfHosted: true
  }
];
