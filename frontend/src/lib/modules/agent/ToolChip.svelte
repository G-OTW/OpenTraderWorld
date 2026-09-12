<script>
  // A tool-call chip: the tool name + a collapsible view of its arguments and result.
  // Read calls are quiet; a failed call is flagged. Rendered under the assistant bubble
  // that made the call (both live-streamed and reconstructed from history).
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';

  // props: name, input (obj), result (string|null — null = still running), is_error
  let { name = '', input = {}, result = null, is_error = false } = $props();

  let open = $state(false);

  const pending = $derived(result === null);
  const inputStr = $derived(() => {
    try {
      return JSON.stringify(input, null, 2);
    } catch {
      return String(input);
    }
  });
</script>

<div class="chip" class:error={is_error}>
  <button class="chip-head" onclick={() => (open = !open)}>
    <Icon name={open ? 'chevron-down' : 'chevron-right'} size={12} />
    <Icon name={is_error ? 'alert-triangle' : 'zap'} size={12} />
    <code>{name}</code>
    {#if pending}
      <span class="status">{$t('agent.msg.running')}</span>
    {:else if is_error}
      <span class="status err">{$t('agent.msg.failed')}</span>
    {:else}
      <span class="status ok">{$t('agent.msg.done')}</span>
    {/if}
  </button>
  {#if open}
    <div class="chip-body">
      {#if input && Object.keys(input).length}
        <div class="lbl">{$t('agent.msg.args')}</div>
        <pre>{inputStr()}</pre>
      {/if}
      {#if !pending}
        <div class="lbl">{$t('agent.msg.result')}</div>
        <pre class:err={is_error}>{result || $t('agent.msg.resultEmpty')}</pre>
      {/if}
    </div>
  {/if}
</div>

<style>
  .chip {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    font-size: var(--text-xs);
    overflow: hidden;
  }
  .chip.error {
    border-color: var(--red);
  }
  .chip-head {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 5px 8px;
    text-align: left;
  }
  .chip-head:hover {
    color: var(--text);
    background: var(--surface-2);
  }
  .chip-head code {
    color: var(--text);
    font-family: var(--mono);
  }
  .status {
    margin-left: auto;
    font-size: 0.7rem;
    font-family: var(--mono);
  }
  .status.ok {
    color: var(--green);
  }
  .status.err {
    color: var(--red);
  }
  .chip-body {
    border-top: 1px solid var(--border);
    padding: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .lbl {
    color: var(--muted);
    text-transform: uppercase;
    font-size: 0.65rem;
    letter-spacing: 0.03em;
  }
  pre {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 6px 8px;
    overflow-x: auto;
    max-height: 240px;
    overflow-y: auto;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: var(--mono);
  }
  pre.err {
    color: var(--red);
  }
</style>
