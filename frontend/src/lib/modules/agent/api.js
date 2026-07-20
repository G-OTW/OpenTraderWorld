/**
 * Agent API client — providers, the single default agent, conversations/messages, and the
 * streaming run endpoint. Provider API keys are write-only (never returned by GET).
 *
 * The run endpoint is Server-Sent Events over a POST (a JSON body is needed, so a bare
 * EventSource won't do): `runStream` opens a fetch, parses the SSE frames from the
 * ReadableStream, and invokes callbacks. Aborting the passed AbortSignal cancels the run —
 * the server sees the disconnect and drops the provider task.
 */
import { redirectIfUnauthorized } from '$lib/auth.js';

async function req(path, options = {}) {
  const res = await fetch(`/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...options
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    /* empty */
  }
  redirectIfUnauthorized(res);
  if (!res.ok) throw new Error(body?.error ?? `request failed (${res.status})`);
  return body;
}

export const agentApi = {
  // Providers
  listProviders: () => req('/agent/providers').then((r) => r.providers),
  addProvider: (p) =>
    req('/agent/providers', { method: 'POST', body: JSON.stringify(p) }).then((r) => r.provider),
  updateProvider: (id, p) =>
    req(`/agent/providers/${id}`, { method: 'PUT', body: JSON.stringify(p) }).then((r) => r.provider),
  deleteProvider: (id) => req(`/agent/providers/${id}`, { method: 'DELETE' }),
  /** Live model ids from the provider (queried server-side; key never reaches the browser). */
  listProviderModels: (id) => req(`/agent/providers/${id}/models`).then((r) => r.models),

  // Agent (single default)
  getAgent: () => req('/agent/agent').then((r) => r.agent),
  updateAgent: (u) => req('/agent/agent', { method: 'PUT', body: JSON.stringify(u) }).then((r) => r.agent),

  // MCP tokens (the agent's tool permission envelope) — reuse the Settings → MCP list.
  listMcpTokens: () => req('/mcp/tokens').then((r) => r.tokens),
  mcpSettings: () => req('/mcp/settings'),

  // External MCP servers (remote Streamable HTTP; auth value write-only, sealed at rest).
  listMcpServers: () => req('/agent/mcp-servers').then((r) => r.servers),
  addMcpServer: (s) =>
    req('/agent/mcp-servers', { method: 'POST', body: JSON.stringify(s) }).then((r) => r.server),
  updateMcpServer: (id, s) =>
    req(`/agent/mcp-servers/${id}`, { method: 'PUT', body: JSON.stringify(s) }).then((r) => r.server),
  deleteMcpServer: (id) => req(`/agent/mcp-servers/${id}`, { method: 'DELETE' }),
  /** Connect + list tools (validation probe). Resolves to the tool-name list. */
  testMcpServer: (id) => req(`/agent/mcp-servers/${id}/test`, { method: 'POST' }).then((r) => r.tools),

  // Long-term memory (slug-keyed). Upsert = POST (creates or replaces by slug).
  listMemories: () => req('/agent/memories'),
  upsertMemory: (m) => req('/agent/memories', { method: 'POST', body: JSON.stringify(m) }).then((r) => r.memory),
  deleteMemory: (slug) => req(`/agent/memories/${encodeURIComponent(slug)}`, { method: 'DELETE' }),

  // Skills (name-keyed, md instructions).
  listSkills: () => req('/agent/skills').then((r) => r.skills),
  addSkill: (s) => req('/agent/skills', { method: 'POST', body: JSON.stringify(s) }).then((r) => r.skill),
  updateSkill: (id, s) => req(`/agent/skills/${id}`, { method: 'PUT', body: JSON.stringify(s) }).then((r) => r.skill),
  deleteSkill: (id) => req(`/agent/skills/${id}`, { method: 'DELETE' }),

  // Conversations
  listConversations: () => req('/agent/conversations').then((r) => r.conversations),
  createConversation: () => req('/agent/conversations', { method: 'POST' }).then((r) => r.conversation),
  getConversation: (id) => req(`/agent/conversations/${id}`),
  /** Patch a conversation: { title } and/or { mcp_token_id } (null = chat only). */
  updateConversation: (id, patch) =>
    req(`/agent/conversations/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.conversation
    ),
  deleteConversation: (id) => req(`/agent/conversations/${id}`, { method: 'DELETE' }),

  /** URL for downloading the markdown export (used by an <a download>). */
  exportUrl: (id) => `/api/agent/conversations/${id}/export`
};

/**
 * Stream a run. Calls the provided handlers as SSE frames arrive:
 *   onDelta(text), onThinking(text), onTool(obj), onDone(obj), onError(msg)
 * Returns a promise that resolves when the stream ends. Abort via `signal`.
 */
export async function runStream(conversationId, message, handlers, signal) {
  const res = await fetch(`/api/agent/conversations/${conversationId}/run`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ message }),
    signal
  });
  redirectIfUnauthorized(res);
  if (!res.ok) {
    let msg = `run failed (${res.status})`;
    try {
      const b = await res.json();
      if (b?.error) msg = b.error;
    } catch {
      /* not JSON */
    }
    throw new Error(msg);
  }

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = '';
  let event = '';
  let data = '';

  const dispatch = () => {
    if (!data) {
      event = '';
      return;
    }
    let parsed;
    try {
      parsed = JSON.parse(data);
    } catch {
      parsed = {};
    }
    switch (event) {
      case 'delta':
        handlers.onDelta?.(parsed.text ?? '');
        break;
      case 'thinking':
        handlers.onThinking?.(parsed.text ?? '');
        break;
      case 'tool':
        handlers.onTool?.(parsed);
        break;
      case 'done':
        handlers.onDone?.(parsed);
        break;
      case 'error':
        handlers.onError?.(parsed.message ?? 'stream error');
        break;
    }
    event = '';
    data = '';
  };

  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    let nl;
    while ((nl = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, nl).replace(/\r$/, '');
      buf = buf.slice(nl + 1);
      if (line === '') {
        dispatch(); // blank line ends an SSE event
      } else if (line.startsWith('event:')) {
        event = line.slice(6).trim();
      } else if (line.startsWith('data:')) {
        data += line.slice(5).replace(/^ /, '');
      }
    }
  }
  dispatch();
}

/** Collapse a message's block array into plain display text (text blocks only). */
export function blocksToText(blocks) {
  if (!Array.isArray(blocks)) return '';
  return blocks
    .filter((b) => b?.type === 'text')
    .map((b) => b.text)
    .join('');
}

/** Extract thinking text from a message's block array. */
export function blocksToThinking(blocks) {
  if (!Array.isArray(blocks)) return '';
  return blocks
    .filter((b) => b?.type === 'thinking')
    .map((b) => b.text)
    .join('');
}

/** Extract tool_use blocks as chip data (result filled in later from the tool row). */
export function blocksToToolUses(blocks) {
  if (!Array.isArray(blocks)) return [];
  return blocks
    .filter((b) => b?.type === 'tool_use')
    .map((b) => ({ id: b.id, name: b.name, input: b.input, result: null, is_error: false }));
}

/**
 * Fold stored messages into a display thread. Assistant messages surface their tool_use
 * chips; the immediately-following "tool" message supplies each chip's result (matched by
 * tool_use_id). Tool rows themselves are not shown as separate bubbles.
 */
export function foldThread(messages) {
  const results = {};
  for (const m of messages) {
    if (m.role !== 'tool') continue;
    for (const b of m.content || []) {
      if (b?.type === 'tool_result') results[b.tool_use_id] = { content: b.content, is_error: !!b.is_error };
    }
  }
  const out = [];
  for (const m of messages) {
    if (m.role === 'tool') continue;
    const tools = blocksToToolUses(m.content).map((t) => {
      const r = results[t.id];
      return r ? { ...t, result: r.content, is_error: r.is_error } : t;
    });
    out.push({
      id: m.id,
      role: m.role,
      text: blocksToText(m.content),
      thinking: blocksToThinking(m.content),
      tools
    });
  }
  return out;
}
