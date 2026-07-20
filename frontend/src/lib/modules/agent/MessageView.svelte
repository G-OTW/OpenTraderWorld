<script>
  // One chat message. User bubbles align right and render as plain text; assistant
  // messages align left and render Markdown. A collapsible "Thinking" fold shows the
  // model's reasoning when present. Tool-call chips are a Phase-2 seam (rendered if the
  // block array carries tool_use/tool_result).
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import { renderMarkdown } from '$lib/modules/agent/markdown.js';
  import ToolChip from '$lib/modules/agent/ToolChip.svelte';

  // props: role, text (plain), thinking (plain), tools ([{name,input,result,is_error}]), streaming
  let { role = 'assistant', text = '', thinking = '', tools = [], streaming = false } = $props();

  let showThinking = $state(false);

  const html = $derived(role === 'assistant' ? renderMarkdown(text) : '');
</script>

<div class="msg" class:user={role === 'user'} class:assistant={role === 'assistant'}>
  {#if thinking}
    <button class="think-toggle" onclick={() => (showThinking = !showThinking)}>
      <Icon name={showThinking ? 'chevron-down' : 'chevron-right'} size={13} />
      {$t('agent.msg.thinking')}
    </button>
    {#if showThinking}
      <pre class="thinking">{thinking}</pre>
    {/if}
  {/if}

  {#if role === 'user'}
    <div class="bubble">
      <div class="plain">{text}</div>
    </div>
  {:else if text || (streaming && !tools.length)}
    <div class="bubble">
      {#if text}
        <!-- Sanitized in renderMarkdown: input is HTML-escaped before any rule runs. -->
        <div class="md">{@html html}</div>
      {/if}
      {#if streaming && !text}
        <span class="cursor">▋</span>
      {/if}
    </div>
  {/if}

  {#if tools.length}
    <div class="tools">
      {#each tools as tool (tool.id)}
        <ToolChip name={tool.name} input={tool.input} result={tool.result} is_error={tool.is_error} />
      {/each}
    </div>
  {/if}
</div>

<style>
  /* Flat blocks separated by hairline filets — no floating rounded bubbles. */
  .msg {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    width: 100%;
    max-width: 100%;
    border-left: 1.5px solid var(--border);
    padding-left: var(--space-3);
  }
  .msg.user {
    align-items: flex-start;
    border-left-color: var(--border-control);
  }
  .msg.assistant {
    align-items: flex-start;
  }
  .bubble {
    border-radius: var(--radius);
    padding: var(--space-1) 0;
    font-size: var(--text-base);
    line-height: 1.55;
    overflow-wrap: anywhere;
    width: 100%;
  }
  .msg.user .bubble {
    color: var(--text);
  }
  .msg.assistant .bubble {
    color: var(--text);
  }
  .plain {
    white-space: pre-wrap;
  }
  .tools {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 100%;
  }
  .think-toggle {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 2px 0;
  }
  .think-toggle:hover {
    color: var(--text);
  }
  .thinking {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    padding: var(--space-2);
    font-size: var(--text-xs);
    white-space: pre-wrap;
    overflow-x: auto;
    max-width: 100%;
  }
  .cursor {
    animation: blink 1s step-start infinite;
    color: var(--muted);
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }

  /* Markdown block styling — scoped to assistant bubbles. */
  .md :global(p) {
    margin: 0 0 var(--space-2);
  }
  .md :global(p:last-child) {
    margin-bottom: 0;
  }
  .md :global(h1),
  .md :global(h2),
  .md :global(h3),
  .md :global(h4),
  .md :global(h5),
  .md :global(h6) {
    margin: var(--space-2) 0 var(--space-1);
    line-height: 1.3;
    font-weight: var(--fw-medium);
  }
  .md :global(strong),
  .md :global(b) {
    font-weight: var(--fw-medium);
  }
  .md :global(h1) { font-size: 1.25rem; }
  .md :global(h2) { font-size: 1.12rem; }
  .md :global(h3) { font-size: 1rem; }
  .md :global(ul),
  .md :global(ol) {
    margin: 0 0 var(--space-2);
    padding-left: 1.4em;
  }
  .md :global(li) {
    margin: 2px 0;
  }
  .md :global(code) {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1px 5px;
    font-family: var(--mono);
    font-size: 0.88em;
  }
  .md :global(pre) {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2);
    overflow-x: auto;
    margin: 0 0 var(--space-2);
  }
  .md :global(pre code) {
    background: none;
    border: none;
    padding: 0;
    font-size: var(--text-xs);
  }
  .md :global(blockquote) {
    border-left: 3px solid var(--border);
    margin: 0 0 var(--space-2);
    padding-left: var(--space-2);
    color: var(--muted);
  }
  .md :global(a) {
    color: var(--text);
    text-decoration: underline;
  }
  .md :global(hr) {
    border: none;
    border-top: 1px solid var(--border);
    margin: var(--space-2) 0;
  }
  /* Tables scroll inside their own box so a wide table never widens the bubble. */
  .md :global(.md-table-wrap) {
    overflow-x: auto;
    margin: 0 0 var(--space-2);
  }
  .md :global(.md-table) {
    border-collapse: collapse;
    width: 100%;
    font-size: 0.92em;
  }
  .md :global(.md-table th),
  .md :global(.md-table td) {
    border: 1px solid var(--border);
    padding: var(--space-1) var(--space-2);
    text-align: left;
    vertical-align: top;
  }
  .md :global(.md-table th) {
    background: var(--surface-2);
    font-weight: 600;
    white-space: nowrap;
  }
</style>
