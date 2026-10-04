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
          <th class="rank" scope="col">#</th>
          {#each axes as a, k (k)}
            <th class="param" scope="col" title={a.label}>{a.label}</th>
          {/each}
          {#each COLS as m (m.id)}
            <th
              class="metric"
              scope="col"
              class:on={sort === m.id}
              aria-sort={sort === m.id ? (dir === 'desc' ? 'descending' : 'ascending') : 'none'}
            >
              {#if m.rank === false}
                <span class="norank" title={$t('backtest.opt.oosNotRanked')}>{$t(`backtest.opt.metric.${m.id}`)}</span>
              {:else}
                <button onclick={() => onsort(m.id)} title={$t('backtest.opt.sortBy')}>
                  {$t(`backtest.opt.metric.${m.id}`)}
                  {#if sort === m.id}
                    <Icon name={dir === 'desc' ? 'chevron-down' : 'chevron-up'} size={11} />
                  {/if}
                </button>
              {/if}
            </th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each rows as r (r.i)}
          <tr class:picked={picked === r.i} onclick={() => onpick(r)}>
            <td class="rank">
              <button
                class="rank-link"
                onclick={(e) => { e.stopPropagation(); onpick(r); }}
                aria-label={`${$t('backtest.opt.tabVariant')} #${r.rank}`}
              >
                {r.rank}<Icon name="chevron-right" size={11} />
              </button>
            </td>
            {#each axes as a, k (k)}
              <td class="param" title={`${displayValue(a, r.params?.[k], dayNames)}${a.unit ?? ''}`}>{displayValue(a, r.params?.[k], dayNames)}{a.unit ?? ''}</td>
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
    <button class="btn" disabled={offset <= 0} onclick={() => onpage(Math.max(0, offset - limit))}>
      <Icon name="chevron-left" size={12} /> {$t('backtest.opt.prev')}
    </button>
    <button class="btn" disabled={offset + limit >= total} onclick={() => onpage(offset + limit)}>
      {$t('backtest.opt.next')} <Icon name="chevron-right" size={12} />
    </button>
    <span class="hint">{$t('backtest.opt.clickRow')}</span>
  </div>
</div>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    min-width: 0;
    gap: var(--space-3);
  }
  .scroll {
    overflow: auto;
    min-height: 0;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface);
  }
  table {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }
  thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--surface-2);
    border-bottom: var(--hairline) solid var(--border-control);
    padding: var(--space-3);
    text-align: right;
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
    white-space: nowrap;
  }
  thead th.on {
    color: var(--text);
    background: color-mix(in srgb, var(--accent) 12%, var(--surface));
  }
  thead th button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
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
  thead th .norank {
    cursor: help;
  }
  th.rank, td.rank {
    text-align: right;
    color: var(--muted);
    width: 64px;
  }
  th.rank {
    position: sticky;
    left: 0;
    z-index: 2;
  }
  td.rank {
    position: sticky;
    left: 0;
    background: var(--surface);
    z-index: 1;
    border-right: var(--hairline) solid var(--border);
  }
  .rank-link {
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-1);
    width: 100%;
    padding: 0;
    background: none;
    border: 0;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .rank-link :global(svg) {
    color: var(--faint);
    flex-shrink: 0;
  }
  .rank-link:focus-visible, thead th button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
  }
  th.param, td.param {
    text-align: left;
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  td {
    height: var(--row-h);
    padding: var(--space-2) var(--space-3);
    text-align: right;
    border-bottom: var(--hairline) solid var(--border);
    white-space: nowrap;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  td.param {
    color: var(--text);
  }
  td.metric {
    color: var(--muted);
  }
  td.metric.on {
    color: var(--text);
    font-weight: var(--fw-medium);
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  td.pos.on {
    color: var(--green-ink);
  }
  td.neg.on {
    color: var(--red-ink);
  }
  tbody tr {
    cursor: pointer;
  }
  tbody tr:nth-child(even) {
    background: color-mix(in srgb, var(--surface-2) 50%, var(--surface));
  }
  tbody tr:hover, tbody tr:hover td.rank {
    background: var(--surface-hover);
  }
  tbody tr.picked, tbody tr.picked td.rank {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface));
  }
  tbody tr.picked td.rank {
    box-shadow: inset 2px 0 var(--accent);
    color: var(--text);
  }
  tr.empty td {
    text-align: center;
    color: var(--muted);
    padding: var(--space-8);
    cursor: default;
    white-space: normal;
  }
  .pager {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-shrink: 0;
    flex-wrap: wrap;
  }
  .count {
    font-size: var(--text-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    margin-right: var(--space-2);
  }
  .pager button {
    font-size: var(--text-xs);
  }
  .hint {
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  @media (max-width: 760px) {
    .hint {
      flex-basis: 100%;
    }
  }
</style>
