<script>
  // The ranking: one row per variant, the swept parameters first, then the metrics.
  //
  // Sorting is the server's job (a quarter of a million rows never reach the browser), so a
  // header click asks for that page again rather than reordering what is on screen.
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  import { fmtNum } from './api.js';
  import { OPT_METRICS, displayValue } from './optimize.js';

  let {
    axes = [],
    rows = [],
    sort = 'sharpe',
    dir = 'desc',
    total = 0,
    offset = 0,
    limit = 50,
    /** Trial index of the row being read in the breakdown tab. */
    picked = null,
    onsort = () => {},
    onpage = () => {},
    onpick = () => {}
  } = $props();

  // Out-of-sample only earns a column when the strategy actually splits its sample.
  const hasOos = $derived(rows.some((r) => r.oos_return_pct != null));
  const COLS = $derived(
    OPT_METRICS.filter((m) => m.id !== 'oos_return_pct' || hasOos)
  );

  const dayNames = $derived({
    all: $t('backtest.opt.everyDay'),
    short: ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'].map((k) => $t(`common.weekday.${k}`))
  });

  const cell = (row, m) => {
    const v = row[m.id];
    if (v == null) return '–';
    return `${fmtNum(v, m.digits)}${m.suffix ?? ''}`;
  };

  const last = $derived(Math.min(offset + rows.length, total));
</script>

<div class="wrap">
  <div class="scroll">
    <table>
      <thead>
        <tr>
          <th class="rank">#</th>
          {#each axes as a, k (k)}
            <th class="param" title={a.label}>{a.label}</th>
          {/each}
          {#each COLS as m (m.id)}
            <th class="metric" class:on={sort === m.id}>
              <button onclick={() => onsort(m.id)} title={$t('backtest.opt.sortBy')}>
                {$t(`backtest.opt.metric.${m.id}`)}
                {#if sort === m.id}
                  <Icon name={dir === 'desc' ? 'chevron-down' : 'chevron-up'} size={11} />
                {/if}
              </button>
            </th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each rows as r (r.i)}
          <tr class:picked={picked === r.i} onclick={() => onpick(r)}>
            <td class="rank">{r.rank}</td>
            {#each axes as a, k (k)}
              <td class="param">{displayValue(a, r.params?.[k], dayNames)}{a.unit ?? ''}</td>
            {/each}
            {#each COLS as m (m.id)}
              <td
                class="metric"
                class:on={sort === m.id}
                class:pos={m.id !== 'max_drawdown_pct' && r[m.id] > 0}
                class:neg={m.id !== 'max_drawdown_pct' && r[m.id] < 0}
              >{cell(r, m)}</td>
            {/each}
          </tr>
        {/each}
        {#if !rows.length}
          <tr class="empty"><td colspan={axes.length + COLS.length + 1}>{$t('backtest.opt.noRowsYet')}</td></tr>
        {/if}
      </tbody>
    </table>
  </div>

  <div class="pager">
    <span class="count">{$t('backtest.opt.showing', { from: total ? offset + 1 : 0, to: last, total: fmtNum(total, 0) })}</span>
    <button disabled={offset <= 0} onclick={() => onpage(Math.max(0, offset - limit))}>
      <Icon name="chevron-left" size={12} /> {$t('backtest.opt.prev')}
    </button>
    <button disabled={offset + limit >= total} onclick={() => onpage(offset + limit)}>
      {$t('backtest.opt.next')} <Icon name="chevron-right" size={12} />
    </button>
    <span class="hint">{$t('backtest.opt.clickRow')}</span>
  </div>
</div>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    min-height: 0;
    gap: var(--space-2);
  }
  .scroll {
    overflow: auto;
    border: var(--hairline) solid var(--border);
    background: var(--surface);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }
  thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--surface-2);
    border-bottom: var(--hairline) solid var(--border);
    padding: var(--space-1) var(--space-2);
    text-align: right;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
    white-space: nowrap;
  }
  thead th.on {
    color: var(--text);
  }
  thead th button {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    background: transparent;
    border: none;
    color: inherit;
    font: inherit;
    padding: 0;
    cursor: pointer;
  }
  thead th button:hover {
    color: var(--text);
  }
  th.rank,
  td.rank {
    text-align: right;
    color: var(--muted);
    width: 48px;
  }
  th.param,
  td.param {
    text-align: left;
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  td {
    padding: 2px var(--space-2);
    text-align: right;
    border-bottom: var(--hairline) solid color-mix(in srgb, var(--border) 55%, transparent);
  }
  /* The parameter block is what the eye reads first: keep it opaque and the metrics quieter. */
  td.param {
    color: var(--text);
  }
  td.metric {
    color: var(--muted);
  }
  td.metric.on {
    color: var(--text);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  td.pos.on {
    color: var(--green);
  }
  td.neg.on {
    color: var(--red);
  }
  tbody tr {
    cursor: pointer;
  }
  tbody tr:hover {
    background: color-mix(in srgb, var(--accent) 7%, transparent);
  }
  tbody tr.picked {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
  }
  tr.empty td {
    text-align: center;
    color: var(--muted);
    font-style: italic;
    padding: var(--space-4);
    cursor: default;
  }
  .pager {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
  }
  .count {
    font-size: var(--text-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .pager button {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 0;
    color: var(--text);
    font-size: var(--text-xs);
    padding: 2px var(--space-2);
    cursor: pointer;
  }
  .pager button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .hint {
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--muted);
    font-style: italic;
  }
</style>
