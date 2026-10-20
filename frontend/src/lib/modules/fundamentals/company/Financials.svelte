<script>
  // Income, balance sheet and cash flow, annual or quarterly. Click a line to chart it,
  // hide the lines you never read; units and the growth row follow Customize.
  import Section from '../Section.svelte';
  import Chart from '../Chart.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { fundamentalsApi, STATEMENT_LINES, fmtBig, fmtNum } from '../api.js';
  import { prefs } from '../prefs.svelte.js';
  import { t } from '$lib/i18n';

  let { company } = $props();

  const KINDS = ['income', 'balance', 'cashflow'];
  let kind = $state('income');
  let freq = $state('annual');
  let rows = $state([]);
  let line = $state('revenue');
  let showHidden = $state(false);

  const f = $derived(prefs.financials);

  // The most recent periods only: EDGAR goes back to 2009, which no table can show.
  const KEEP = { annual: 10, quarterly: 12 };

  $effect(() => {
    const n = KEEP[freq];
    fundamentalsApi.statements(company.ticker, kind, freq).then((r) => (rows = r.slice(-n)));
  });
  $effect(() => {
    if (!STATEMENT_LINES[kind].includes(line)) line = STATEMENT_LINES[kind][0];
  });

  const lines = $derived(STATEMENT_LINES[kind].filter((k) => showHidden || !f.hiddenLines.includes(k)));
  const hiddenHere = $derived(STATEMENT_LINES[kind].filter((k) => f.hiddenLines.includes(k)).length);

  function fmt(key, v) {
    if (key === 'eps_diluted') return fmtNum(v, 2);
    if (f.units === 'M') return fmtNum(v / 1e6, 0);
    if (f.units === 'B') return fmtNum(v / 1e9, 2);
    return fmtBig(v);
  }

  function growth(i, key) {
    if (!i) return null;
    const a = rows[i - 1]?.lines[key];
    const b = rows[i]?.lines[key];
    return a && b != null ? ((b - a) / Math.abs(a)) * 100 : null;
  }

  function hide(key) {
    f.hiddenLines = f.hiddenLines.includes(key) ? f.hiddenLines.filter((k) => k !== key) : [...f.hiddenLines, key];
  }

  const unitLabel = $derived(f.units === 'auto' ? company.currency : `${company.currency} ${$t(f.units === 'M' ? 'fundamentals.fin.millions' : 'fundamentals.fin.billions')}`);
</script>

<Section key="company.statements" title={$t(`fundamentals.fin.${kind}`)} description={$t('fundamentals.fin.desc')} source="SEC XBRL">
  {#snippet actions()}
    {#each KINDS as k (k)}
      <button class="chip" class:active={kind === k} onclick={() => (kind = k)}>{$t(`fundamentals.fin.${k}`)}</button>
    {/each}
    <span class="sep"></span>
    <button class="chip" class:active={freq === 'annual'} onclick={() => (freq = 'annual')}>{$t('fundamentals.fin.annual')}</button>
    <button class="chip" class:active={freq === 'quarterly'} onclick={() => (freq = 'quarterly')}>{$t('fundamentals.fin.quarterly')}</button>
  {/snippet}

  <div class="chart">
    <span class="fd-label">{$t(`fundamentals.line.${line}`)}</span>
    <Chart
      categories={rows.map((r) => r.period)}
      series={[{ name: $t(`fundamentals.line.${line}`), type: 'bar', data: rows.map((r) => r.lines[line] ?? null) }]}
      yFormat={(v) => fmt(line, v)}
      legend={false}
      height={220}
    />
  </div>

  <div class="fd-scroll">
    <table class="tbl">
      <thead>
        <tr>
          <th>{unitLabel}</th>
          {#each rows as r (r.period)}<th class="num">{r.period}</th>{/each}
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each lines as key (key)}
          <tr class:sel={line === key} class:muted={f.hiddenLines.includes(key)} onclick={() => (line = key)}>
            <td>{$t(`fundamentals.line.${key}`)}</td>
            {#each rows as r, i (r.period)}
              {@const g = growth(i, key)}
              <td class="num" title={r.tags?.[key] ?? ''}>
                {fmt(key, r.lines[key])}
                {#if f.growth && g != null}<small class:fd-up={g > 0} class:fd-down={g < 0}>{g > 0 ? '+' : ''}{g.toFixed(1)}%</small>{/if}
              </td>
            {/each}
            <td class="hide">
              <button onclick={(e) => (e.stopPropagation(), hide(key))} title={f.hiddenLines.includes(key) ? $t('fundamentals.fin.showLine') : $t('fundamentals.fin.hideLine')}>
                <Icon name={f.hiddenLines.includes(key) ? 'eye' : 'eye-off'} size={13} />
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
  <div class="fd-row foot">
    <p class="fd-note">{$t('fundamentals.fin.note')}</p>
    {#if hiddenHere}
      <button class="link" onclick={() => (showHidden = !showHidden)}>
        {showHidden ? $t('fundamentals.fin.hideHidden') : $t('fundamentals.fin.showHidden', { n: hiddenHere })}
      </button>
    {/if}
  </div>
</Section>

<style>
  .sep {
    width: 1px;
    height: 18px;
    background: var(--border);
    margin: 0 var(--space-2);
  }
  .chart {
    margin-bottom: var(--space-6);
  }
  tr {
    cursor: pointer;
  }
  tr.sel td {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  tr.muted td {
    color: var(--dim);
  }
  td small {
    display: block;
    font-size: 10px;
    line-height: 1.1;
  }
  .hide {
    width: 32px;
    text-align: right;
  }
  .hide button {
    border: 0;
    background: transparent;
    color: var(--dim);
    cursor: pointer;
    opacity: 0;
  }
  tr:hover .hide button,
  .hide button:focus-visible {
    opacity: 1;
  }
  .foot {
    justify-content: space-between;
  }
</style>
