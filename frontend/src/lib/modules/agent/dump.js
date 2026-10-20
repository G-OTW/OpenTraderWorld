/**
 * Dump a conversation into an Editor page: one section per message (who, when), the
 * model's reasoning in a blockquote, then the answer. `details` adds tool calls (input +
 * result) and token counts. The thread is rendered to HTML and parsed with the editor's
 * own node set, so the page opens exactly as if it had been typed there.
 */
import { generateJSON } from '@tiptap/core';
import StarterKit from '@tiptap/starter-kit';
import { CopyableCodeBlock } from '$lib/modules/editor/CopyableCodeBlock.js';
import { renderMarkdown } from './markdown.js';
import { blocksToText, blocksToThinking, blocksToToolUses } from './api.js';

const EXTENSIONS = [
  StarterKit.configure({ heading: { levels: [1, 2, 3] }, codeBlock: false }),
  CopyableCodeBlock
];

function esc(s) {
  return String(s ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/** Plain text → paragraphs (blank line = new paragraph, single newline = hard break). */
function paras(text) {
  return text
    .trim()
    .split(/\n\s*\n/)
    .map((p) => `<p>${esc(p).replace(/\n/g, '<br>')}</p>`)
    .join('');
}

function pretty(v) {
  if (v == null) return '';
  if (typeof v === 'string') return v;
  // A tool result is either a string or an array of content blocks.
  if (Array.isArray(v) && v.every((b) => b?.type === 'text')) return v.map((b) => b.text).join('\n');
  return JSON.stringify(v, null, 2);
}

/**
 * @param {object} p
 * @param {Array} p.messages   raw stored messages (getConversation().messages)
 * @param {Array} p.personas   [{ agent }] to name each assistant turn
 * @param {boolean} p.details  include tool calls and token counts
 * @param {(v: string) => string} p.fmtDate
 * @param {{ you: string, assistant: string, thinking: string, tool: string, result: string, error: string, tokens: string }} p.labels
 * @returns ProseMirror JSON for the page
 */
export function threadToDoc({ messages, personas, details, fmtDate, labels }) {
  const names = new Map(personas.map((p) => [p.agent.id, p.agent.name]));
  const results = {};
  for (const m of messages) {
    if (m.role !== 'tool') continue;
    for (const b of m.content || []) {
      if (b?.type === 'tool_result') results[b.tool_use_id] = b;
    }
  }

  const sections = [];
  let lastWho = null; // a tool loop stores one row per step: keep them under one header
  for (const m of messages) {
    if (m.role === 'tool') continue;
    const text = blocksToText(m.content);
    const thinking = blocksToThinking(m.content);
    const tools = details ? blocksToToolUses(m.content) : [];
    if (!text.trim() && !thinking.trim() && !tools.length) continue;

    if (m.role === 'system') {
      sections.push(`<p><em>${esc(fmtDate(m.created_at))} · ${esc(text)}</em></p>`);
      lastWho = null;
      continue;
    }

    let who = labels.you;
    if (m.role === 'assistant') {
      who = names.get(m.agent_id) ?? labels.assistant;
      if (m.model) who += ` (${m.model})`;
    }
    const cont = m.role === 'assistant' && who === lastWho;
    lastWho = m.role === 'assistant' ? who : null;
    let html = cont ? '' : `<p><strong>${esc(who)}</strong> · <em>${esc(fmtDate(m.created_at))}</em></p>`;
    if (thinking.trim()) {
      html += `<blockquote><p><strong>${esc(labels.thinking)}</strong></p>${paras(thinking)}</blockquote>`;
    }
    for (const tl of tools) {
      const r = results[tl.id];
      html += `<p><strong>${esc(labels.tool)}</strong> <code>${esc(tl.name)}</code></p>`;
      html += `<pre><code class="language-json">${esc(pretty(tl.input))}</code></pre>`;
      if (r) {
        html += `<p><strong>${esc(r.is_error ? labels.error : labels.result)}</strong></p>`;
        html += `<pre><code>${esc(pretty(r.content))}</code></pre>`;
      }
    }
    if (text.trim()) html += m.role === 'assistant' ? renderMarkdown(text) : paras(text);
    if (details && m.role === 'assistant' && (m.input_tokens || m.output_tokens)) {
      html += `<p><em>${esc(labels.tokens)}: ${m.input_tokens} / ${m.output_tokens}</em></p>`;
    }
    if (cont) sections[sections.length - 1] += html;
    else sections.push(html);
  }
  return generateJSON(sections.join('<hr>'), EXTENSIONS);
}
