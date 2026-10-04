<script>
  // Dock tab 1 — every run, newest first. Clicking a row loads its settings back into the
  // wizard; the star marks a named (pinned) run, which the 300-run cap never removes.
  import Icon from '$lib/ui/Icon.svelte';
  import { fmtNum, KIND_LABEL_KEYS, settingsKind } from './api.js';
  import { t } from '$lib/i18n';

  let { runs = [], activeId = null, onload, onreport, onremove } = $props();
</script>

{#if !runs.length}
  <p class="empty">{$t('backtest.page.noSavedRuns')}</p>
{:else}
  <div class="rows">
    {#each runs as r (r.id)}
      <div class="row" class:on={r.id === activeId}>
        <button class="main" onclick={() => onload?.(r)} title={$t('backtest.dock.loadRun')}>
          <span class="nm">
            {#if r.pinned}<Icon name="star" size={11} />{/if}
            {r.name}
          </span>
          <span class="meta">
            <span class="kind">{$t(KIND_LABEL_KEYS[settingsKind(r.settings)])}</span>
            {r.ticker} · {r.timeframe}
          </span>
          <span class="stat" class:pos={r.stats?.return_pct > 0} class:neg={r.stats?.return_pct < 0}>
            {$t('backtest.page.runStat', { returnPct: fmtNum(r.stats?.return_pct), trades: fmtNum(r.stats?.trades, 0) })}
          </span>
        </button>
        <button class="act" title={$t('backtest.report.open')} onclick={() => onreport?.(r.id)}><Icon name="file-text" size={12} /></button>
        <button class="act danger" title={$t('common.remove')} onclick={() => onremove?.(r.id)}><Icon name="x" size={12} /></button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .empty {
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-3);
  }
  .rows {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    border-bottom: var(--hairline) solid var(--border);
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.on {
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .main {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 190px 160px;
    align-items: center;
    gap: var(--space-3);
    background: transparent;
    border: none;
    color: var(--text);
    text-align: left;
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    font-size: var(--text-sm);
  }
  .nm {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta,
  .stat {
    color: var(--muted);
    font-size: var(--text-xs);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* The engine kind leads the row's metadata: a grid run and a DCA plan read nothing alike. */
  .kind {
    display: inline-block;
    margin-right: var(--space-1);
    padding: 0 4px;
    border: var(--hairline) solid var(--border);
    color: var(--text);
  }
  .stat.pos {
    color: var(--green);
  }
  .stat.neg {
    color: var(--red);
  }
  .act {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: var(--space-2);
    display: inline-flex;
  }
  .act:hover {
    color: var(--text);
  }
  .act.danger:hover {
    color: var(--red);
  }
  @media (max-width: 900px) {
    .main {
      grid-template-columns: minmax(0, 1fr) 170px;
    }
    .stat {
      display: none;
    }
  }
</style>
