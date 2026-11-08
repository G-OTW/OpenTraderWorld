<script>
  // The strategy handbook, filtered by outlook. Picking one replaces the legs with the
  // template built on the loaded chain.
  import { HANDBOOK, OUTLOOKS } from './handbook.js';
  import { t } from '$lib/i18n';

  let { onpick = () => {}, disabled = false } = $props();
  let outlook = $state('all');

  const shown = $derived(outlook === 'all' ? HANDBOOK : HANDBOOK.filter((h) => h.outlook === outlook));
</script>

<section class="qp-block">
  <div class="head">
    <h3>{$t('options.handbook.title')}</h3>
    <div class="qp-seg">
      {#each ['all', ...OUTLOOKS] as o (o)}
        <button class:active={outlook === o} onclick={() => (outlook = o)}>{$t(`options.outlook.${o}`)}</button>
      {/each}
    </div>
  </div>
  <div class="list">
    {#each shown as h (h.id)}
      <button class="tpl {h.outlook}" onclick={() => onpick(h)} {disabled} title={$t(`options.handbook.${h.id}.hint`)}>
        {$t(`options.handbook.${h.id}`)}
      </button>
    {/each}
  </div>
  {#if disabled}<p class="qp-hint">{$t('options.handbook.needChain')}</p>{/if}
</section>

<style>
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .list {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .tpl {
    background: var(--surface-2);
    border: var(--hairline) solid transparent;
    border-left: 2px solid var(--muted);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: var(--space-1) var(--space-3);
    min-height: var(--control-h);
    cursor: pointer;
  }
  .tpl:hover:not(:disabled) {
    border-color: var(--border-control);
  }
  .tpl.bullish {
    border-left-color: var(--green);
  }
  .tpl.bearish {
    border-left-color: var(--red);
  }
  .tpl.neutral {
    border-left-color: var(--amber);
  }
  .tpl:disabled {
    opacity: 0.5;
    cursor: default;
  }
  /* Phone: the templates line up two by two, names wrap instead of widening the row. */
  @media (max-width: 767px) {
    .qp-seg button {
      min-height: 44px;
    }
    .list {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .tpl {
      min-width: 0;
      min-height: 44px;
      text-align: left;
      white-space: normal;
      overflow-wrap: anywhere;
    }
  }
</style>
