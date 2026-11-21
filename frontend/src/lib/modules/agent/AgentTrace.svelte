<script>
  // Diagnostics for the open conversation: rounds, tool calls, errors, catalog reads,
  // warnings and whether the run ended on a question. Read from the stored messages on open,
  // so it reflects the last finished turn.
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import { clickOutside } from '$lib/ui/clickOutside.js';
  import { agentApi } from './api.js';

  // props: conversationId (the open conversation)
  let { conversationId } = $props();

  let open = $state(false);
  let trace = $state(null);
  let err = $state('');

  async function toggle() {
    open = !open;
    if (!open || !conversationId) return;
    err = '';
    try {
      trace = await agentApi.getTrace(conversationId);
    } catch (e) {
      trace = null;
      err = e.message;
    }
  }

  const fmtUsd = (v) => `$${v < 1 ? v.toFixed(4) : v.toFixed(2)}`;
  const rows = $derived(
    trace
      ? [
          [$t('agent.trace.rounds'), trace.rounds],
          [$t('agent.trace.perRound'), trace.calls_per_round.join(' · ') || '0'],
          [$t('agent.trace.toolCalls'), trace.tool_calls],
          [$t('agent.trace.errors'), trace.errors],
          [$t('agent.trace.catalogChars'), trace.catalog_chars.toLocaleString()],
          [$t('agent.trace.warnings'), trace.warnings_seen],
          [$t('agent.trace.stoppedToAsk'), trace.stopped_to_ask ? $t('agent.trace.yes') : $t('agent.trace.no')],
          [$t('agent.trace.tokens'), `${trace.input_tokens.toLocaleString()} / ${trace.output_tokens.toLocaleString()}`],
          [$t('agent.trace.cost'), fmtUsd(trace.cost_usd)]
        ]
      : []
  );
</script>

<span class="trace" use:clickOutside={() => (open = false)}>
  <button
    type="button"
    class="icon"
    class:on={open}
    title={$t('agent.trace.title')}
    aria-label={$t('agent.trace.title')}
    aria-expanded={open}
    onclick={toggle}
  >
    <Icon name="timeline" size={15} />
  </button>
  {#if open}
    <div class="pop" role="dialog" aria-label={$t('agent.trace.title')}>
      <p class="title">{$t('agent.trace.title')}</p>
      {#if err}
        <p class="err">{err}</p>
      {:else if trace}
        <dl>
          {#each rows as [label, value] (label)}
            <dt>{label}</dt>
            <dd>{value}</dd>
          {/each}
        </dl>
      {:else}
        <p class="muted">{$t('common.loading')}</p>
      {/if}
    </div>
  {/if}
</span>

<style>
  .trace {
    position: relative;
    display: inline-flex;
  }
  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    width: 26px;
    height: 26px;
    border-radius: var(--radius);
  }
  .icon:hover,
  .icon.on {
    color: var(--text);
    background: var(--surface);
  }
  .pop {
    position: absolute;
    top: calc(100% + var(--space-2));
    right: 0;
    z-index: var(--z-dropdown);
    width: 260px;
    max-width: calc(100vw - var(--space-4));
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
    font-size: var(--text-sm);
    color: var(--text);
    padding: var(--space-3);
  }
  .title {
    margin: 0 0 var(--space-2);
    font-weight: 600;
  }
  dl {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: var(--space-1) var(--space-3);
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .muted {
    margin: 0;
    color: var(--muted);
  }
  .err {
    margin: 0;
    color: var(--red);
  }
</style>
