<script>
  // Period versus period. Pick the granularity (day / week / month / quarter / year, or
  // the filter's own custom range) and the panel answers one question: is this period
  // better or worse than the last, and on which measure.
  //
  // The strip underneath is the same measure over the last twelve periods, so a single
  // good month is not mistaken for a trend.
  import { journalApi, fmtMoney, fmtSignedMoney, fmtPct, fmtNum } from './api.js';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let { filter = {}, displayCurrency = 'USD' } = $props();

  const PERIOD_KEY = 'otw.journal.compare.period.v1';
  let period = $state('month');
  (function load() {
    try {
      const p = localStorage.getItem(PERIOD_KEY);
      if (p) period = p;
    } catch {
      /* ignore */
    }
  })();
  $effect(() => {
    try {
      localStorage.setItem(PERIOD_KEY, period);
    } catch {
      /* non-fatal */
    }
  });

  // Which period the strip is anchored on. Empty = today (the API's default); clicking a
  // bar re-anchors, so the user can walk back through history without a date picker.
  let anchor = $state('');

  let cmp = $state(null);
  let loading = $state(true);

  $effect(() => {
    const f = filter;
    const p = period;
    const a = anchor;
    loading = true;
    journalApi
      .compare(f, p, a)
      .then((d) => {
        cmp = d;
      })
      .finally(() => {
        loading = false;
      });
  });

  const PERIODS = ['day', 'week', 'month', 'quarter', 'year', 'custom'];

  // A period is named by its start date, in the granularity's own terms.
  function periodLabel(p) {
    if (!p) return '';
    const d = new Date(`${p.start}T00:00:00`);
    if (period === 'day' || period === 'custom') return d.toLocaleDateString();
    if (period === 'week') return $t('journal.compare.weekOf', { date: d.toLocaleDateString() });
    if (period === 'year') return String(d.getFullYear());
    if (period === 'quarter') return `Q${Math.floor(d.getMonth() / 3) + 1} ${d.getFullYear()}`;
    return d.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
  }
  // Compact label for the twelve bars of the strip.
  function stripLabel(p) {
    const d = new Date(`${p.start}T00:00:00`);
    if (period === 'day') return d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
    if (period === 'week') return d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
    if (period === 'year') return String(d.getFullYear());
    if (period === 'quarter') return `Q${Math.floor(d.getMonth() / 3) + 1}`;
    return d.toLocaleDateString(undefined, { month: 'short' });
  }

  const money = (n) => fmtMoney(n, displayCurrency);
  const signed = (n) => (n == null ? '—' : fmtSignedMoney(n, displayCurrency));

  // The comparison rows. `better` says which direction is good, so drawdown can be red
  // when it grows without hardcoding a sign anywhere in the markup.
  const rows = $derived.by(() => {
    if (!cmp) return [];
    const c = cmp.current;
    const p = cmp.previous;
    const row = (labelKey, key, fmt, better = 'up') => ({
      label: $t(labelKey),
      cur: c[key],
      prev: p[key],
      fmt,
      better
    });
    return [
      row('journal.compare.row.net', 'net', signed),
      row('journal.compare.row.trades', 'trades', (v) => String(v ?? 0)),
      row('journal.compare.row.winRate', 'win_rate', (v) => (v == null ? '—' : fmtPct(v))),
      row('journal.compare.row.expectancy', 'expectancy', signed),
      row('journal.compare.row.avgR', 'avg_r', (v) =>
        v == null ? '—' : `${v > 0 ? '+' : ''}${fmtNum(v)}R`
      ),
      row('journal.compare.row.profitFactor', 'profit_factor', (v) =>
        v == null ? '—' : fmtNum(v)
      ),
      row('journal.compare.row.maxDrawdown', 'max_drawdown', (v) => (v == null ? '—' : fmtPct(v)), 'down'),
      row('journal.compare.row.fees', 'fees', money, 'down'),
      row('journal.compare.row.tradingDays', 'trading_days', (v) => String(v ?? 0), 'flat')
    ];
  });

  function delta(r) {
    if (r.cur == null || r.prev == null) return null;
    return r.cur - r.prev;
  }
  function deltaTone(r) {
    const d = delta(r);
    if (d == null || d === 0 || r.better === 'flat') return '';
    const good = r.better === 'up' ? d > 0 : d < 0;
    return good ? 'pos' : 'neg';
  }
  function deltaText(r) {
    const d = delta(r);
    if (d == null) return '—';
    const sign = d > 0 ? '+' : d < 0 ? '−' : '';
    return `${sign}${r.fmt(Math.abs(d)).replace(/^[+−-]/, '')}`;
  }

  const stripMax = $derived(
    Math.max(1e-9, ...(cmp?.history ?? []).map((p) => Math.abs(p.net)))
  );
</script>

<div class="compare">
  <div class="head">
    <Dropdown
      bind:value={period}
      title={$t('journal.compare.period')}
      ariaLabel={$t('journal.compare.period')}
      options={PERIODS.map((p) => ({ value: p, label: $t(`journal.compare.period.${p}`) }))}
    />
    {#if anchor}
      <button class="reset" onclick={() => (anchor = '')}>{$t('journal.compare.backToNow')}</button>
    {/if}
    {#if period === 'custom'}
      <span class="hint">{$t('journal.compare.customHint')}</span>
    {/if}
  </div>

  {#if loading && !cmp}
    <Skeleton height="320px" />
  {:else if cmp}
    <div class="pair">
      <table class="tbl">
        <thead>
          <tr>
            <th></th>
            <th class="num">{periodLabel(cmp.current)}</th>
            <th class="num">{periodLabel(cmp.previous)}</th>
            <th class="num">{$t('journal.compare.delta')}</th>
          </tr>
        </thead>
        <tbody>
          {#each rows as r (r.label)}
            <tr>
              <td>{r.label}</td>
              <td class="num strong">{r.fmt(r.cur)}</td>
              <td class="num dim">{r.fmt(r.prev)}</td>
              <td class="num {deltaTone(r)}">{deltaText(r)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <div class="strip">
      <h4>{$t('journal.compare.stripTitle')}</h4>
      {#if cmp.history.every((p) => p.trades === 0)}
        <EmptyState compact icon="bar-chart" title={$t('journal.compare.stripEmpty')} />
      {:else}
        <div class="bars">
          {#each cmp.history as p (p.start)}
            <button
              class="bcol"
              class:current={p.start === cmp.current.start}
              title={`${periodLabel(p)}\n${$t('journal.compare.row.net')}: ${signed(p.net)}\n${$t('journal.compare.row.trades')}: ${p.trades}`}
              onclick={() => (anchor = p.start)}
            >
              <span class="up">
                {#if p.net > 0}<span class="b pos" style="height:{(p.net / stripMax) * 100}%"></span>{/if}
              </span>
              <span class="mid"></span>
              <span class="down">
                {#if p.net < 0}<span class="b neg" style="height:{(-p.net / stripMax) * 100}%"></span>{/if}
              </span>
              <span class="lab">{stripLabel(p)}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .compare {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .hint,
  .reset {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .reset {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
  }
  .reset:hover {
    text-decoration: underline;
  }
  .strong {
    color: var(--text);
  }
  .dim {
    color: var(--dim);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  h4 {
    font-size: 12.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.03em;
    color: var(--muted);
    margin-bottom: var(--space-3);
  }
  .bars {
    display: flex;
    align-items: stretch;
    gap: 2px;
  }
  .bcol {
    flex: 1 1 0;
    min-width: 22px;
    display: grid;
    grid-template-rows: 1fr 0 1fr auto;
    height: 160px;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
  }
  .bcol:hover .b {
    filter: brightness(1.15);
  }
  .bcol.current .lab {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .up {
    display: flex;
    align-items: flex-end;
    justify-content: center;
  }
  .down {
    display: flex;
    align-items: flex-start;
    justify-content: center;
  }
  .mid {
    border-top: 0.5px solid var(--border);
  }
  .b {
    width: 100%;
    max-width: 40px;
    min-height: 1px;
  }
  .b.pos {
    background: color-mix(in srgb, var(--green) 62%, transparent);
  }
  .b.neg {
    background: color-mix(in srgb, var(--red) 62%, transparent);
  }
  .lab {
    text-align: center;
    font-size: 10px;
    color: var(--dim);
    padding-top: var(--space-1);
    white-space: nowrap;
  }
</style>
