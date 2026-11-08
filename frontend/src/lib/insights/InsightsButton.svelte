<script>
  // Shortcut to the shared insights page from inside a module: drop it in a page header.
  // `source` names what the page reads: { kind: 'journal', filters }, { kind: 'backtest',
  // run }, { kind: 'paper', session }. Without the id a run or a session needs, it is
  // disabled rather than pointing at the wrong trades.
  import Icon from '$lib/ui/Icon.svelte';
  import { hrefOf } from './insights.js';
  import { t } from '$lib/i18n';

  let { source, size = 'md' } = $props();

  const ready = $derived(
    source?.kind === 'journal' || (source?.kind === 'backtest' && !!source.run) || (source?.kind === 'paper' && !!source.session)
  );
</script>

{#if ready}
  <a class="ibtn" class:sm={size === 'sm'} href={hrefOf(source)} title={$t('insights.button.title')}>
    <Icon name="lightbulb" size={size === 'sm' ? 12 : 14} />
    {$t('insights.button.label')}
  </a>
{:else}
  <span class="ibtn off" class:sm={size === 'sm'} aria-disabled="true" title={$t('insights.button.unsaved')}>
    <Icon name="lightbulb" size={size === 'sm' ? 12 : 14} />
    {$t('insights.button.label')}
  </span>
{/if}

<style>
  .ibtn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    line-height: 1;
    text-decoration: none;
    cursor: pointer;
    transition: background-color 0.12s ease, border-color 0.12s ease;
  }
  .ibtn:hover {
    background: var(--surface-2);
  }
  .ibtn.off {
    color: var(--muted);
    cursor: default;
  }
  .ibtn.off:hover {
    background: transparent;
  }
  .ibtn.sm {
    height: 24px;
    padding: 0 var(--space-3);
    font-size: var(--text-xs);
  }
</style>
