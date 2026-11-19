<script>
  // Every loader is a stored read. Dashboard refresh never spends provider quota.
  import { fundamentalsApi, transform, sliceRange, fmtBig, fmtNum, fmtPct, STATEMENT_LINES } from '$lib/modules/fundamentals/api.js';
  import { t, locale } from '$lib/i18n';
  import { dateKey } from '$lib/format';
  import { livePulse } from '../live.svelte.js';
  import { COMPANY_METRICS, DEFAULT_METRICS, percentChange, statementYoY, statementRows, sortedCompanies } from './insights.js';
  import WidgetState from './WidgetState.svelte';
  import TrendChart from './parts/TrendChart.svelte';
  import Spark from './parts/Spark.svelte';
  import StatementBars from './parts/StatementBars.svelte';
  import Stat from './parts/Stat.svelte';

  let { item, editing, onconfigure = null } = $props();
  const variant = $derived(item.config?.variant ?? 'series');
  const limit = $derived(Math.max(1, Math.min(30, item.config?.limit ?? 8)));
  const metrics = $derived((item.config?.metrics ?? DEFAULT_METRICS).filter((k) => COMPANY_METRICS[k]).slice(0, 4));
  const columns = $derived((item.config?.columns ?? ['price', 'change_pct', 'pe', 'market_cap']).filter((k) => ['price', 'change_pct', ...Object.keys(COMPANY_METRICS)].includes(k)).slice(0, 4));
  let data = $state.raw(null);
  let err = $state('');
  let empty = $state('');
  let busy = $state(false);
  let asked = '';
  let sort = $state('ticker');
  let direction = $state('asc');
  const live = livePulse(300);

  const kindOf = (line) => Object.keys(STATEMENT_LINES).find((k) => STATEMENT_LINES[k].includes(line)) ?? 'income';
  const tone = (n) => n == null || Math.abs(n) < 0.005 ? '' : n < 0 ? 'neg' : 'pos';
  const day = (value) => {
    if (!value) return '—';
    const d = new Date(typeof value === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(value) ? `${value}T00:00:00Z` : value);
    return Number.isNaN(d.getTime()) ? '—' : d.toLocaleDateString($locale, { year: 'numeric', month: 'short', day: 'numeric', timeZone: 'UTC' });
  };
  const href = (ticker, tab = 'overview') => `/fundamentals/company?t=${encodeURIComponent(ticker)}&tab=${tab}`;
  const rate = (s) => s.unit === '%' || /percent/i.test(s.unit ?? '');
  const frequency = (freq) => {
    const key = ({ D: 'daily', W: 'weekly', M: 'monthly', Q: 'quarterly', A: 'annual' })[freq] ?? freq;
    return ['daily', 'weekly', 'monthly', 'quarterly', 'annual'].includes(key) ? $t(`dashboard.widgets.insights.freq.${key}`) : freq || '—';
  };
  const metricName = (key) => key === 'price' ? $t('dashboard.widgets.fundamentals.price') : key === 'change_pct' ? $t('dashboard.widgets.fundamentals.day') : $t(`fundamentals.metric.${COMPANY_METRICS[key]?.[0]}`);
  const metricValue = (key, v, currency = '') => v == null ? '—' : key === 'price' ? `${fmtNum(v, 2)}${currency ? ` ${currency}` : ''}`
    : key === 'change_pct' || COMPANY_METRICS[key]?.[1] === 'percent' ? fmtPct(v, 2, key === 'change_pct')
    : COMPANY_METRICS[key]?.[1] === 'money' ? fmtBig(v, currency) : `${fmtNum(v, 1)}×`;
  const missingMetric = (key, c) => c.loadError || (c.metrics?.[key] == null ? $t(key === 'pe' || key === 'ev_ebitda' ? 'dashboard.widgets.insights.ratioUnavailable' : 'dashboard.widgets.insights.metricUnavailable') : '');
  const lineFmt = (line) => (v) => line === 'eps_diluted' ? fmtNum(v, 2) : fmtBig(v);
  const seriesFmt = (s, tf) => (v) => tf === 'yoy' || tf === 'pop' || (tf === 'level' && rate(s)) ? fmtPct(v, 2) : tf === 'diff' && rate(s) ? `${fmtNum(v, 2)} ${$t('dashboard.widgets.insights.pp')}` : fmtNum(v, 2);
  const seriesDelta = (s, tf) => (v) => `${v > 0 ? '+' : ''}${fmtNum(v, 2)}${tf === 'yoy' || tf === 'pop' || rate(s) ? ` ${$t('dashboard.widgets.insights.pp')}` : tf === 'index' ? ` ${$t('dashboard.widgets.insights.points')}` : s.unit ? ` ${s.unit}` : ''}`;
  const boardDelta = (s) => rate(s) && s.prev != null ? `${fmtNum(s.last - s.prev, 2)} ${$t('dashboard.widgets.insights.pp')}` : s.prev != null ? `${fmtPct(percentChange(s.last, s.prev), 1, true)} ${$t('dashboard.widgets.insights.pop')}` : '—';
  const boardValue = (s) => !rate(s) && Math.abs(s.last) >= 1e6
    ? new Intl.NumberFormat($locale, { notation: 'compact', maximumFractionDigits: 2 }).format(s.last)
    : seriesFmt(s, 'level')(s.last);
  const sorted = $derived(['companies', 'valuation'].includes(variant) && Array.isArray(data?.rows) ? sortedCompanies(data.rows, sort, direction) : []);
  function sortBy(key) { direction = sort === key && direction === 'asc' ? 'desc' : 'asc'; sort = key; }
  const QUIET = ['3', '4', '5', '3/A', '4/A', '5/A', '144', 'FWP'];
  const formName = (form) => $t(`dashboard.widgets.insights.form.${({ '10-K': 'annual', '10-Q': 'quarterly', '8-K': 'current', '20-F': 'annual', '6-K': 'current', 'DEF 14A': 'proxy' })[form] ?? 'other'}`);
  const emptyHref = $derived(variant === 'series' || variant === 'board' ? '/fundamentals' : variant === 'earnings' ? '/fundamentals/company?tab=earnings' : '/fundamentals/company');

  async function pickCompany(wanted) {
    const list = await fundamentalsApi.companies();
    const c = wanted ? list.find((c) => c.ticker === wanted) : list.find((c) => c.followed) ?? list[0];
    return c ? fundamentalsApi.storedCompany(c.ticker) : null;
  }
  const noCompany = () => ({ empty: $t('dashboard.widgets.fundamentals.noCompany') });

  async function load(cfg, v) {
    if (v === 'series') {
      const list = await fundamentalsApi.series();
      const s = cfg.series ? list.find((s) => s.id === cfg.series) : list.find((s) => s.status === 'ok');
      if (!s) return { empty: $t('dashboard.widgets.fundamentals.noSeries') };
      const tf = cfg.tf ?? 'level';
      const rows = sliceRange(transform(await fundamentalsApi.observations(s.id), tf === 'level' ? '' : tf, s.freq), cfg.range ?? '10y').filter(([, value]) => value != null);
      return rows.length ? { s, tf, rows } : { empty: $t('dashboard.widgets.fundamentals.noObs') };
    }
    if (v === 'board') {
      const all = (await fundamentalsApi.series()).filter((s) => s.last != null && (!cfg.category || s.category === cfg.category));
      const shown = (cfg.seriesIds?.length ? cfg.seriesIds.map((id) => all.find((s) => s.id === id)).filter(Boolean) : all).slice(0, limit);
      if (!shown.length) return { empty: $t('dashboard.widgets.fundamentals.noSeries') };
      const rows = await Promise.all(shown.map(async (s) => {
        try {
          const obs = await fundamentalsApi.observations(s.id);
          const recent = sliceRange(obs, '1y');
          return { s, spark: (recent.length > 2 ? recent : obs.slice(-12)).map(([, value]) => value) };
        } catch { return { s, spark: [] }; }
      }));
      return { rows };
    }
    if (v === 'company' || v === 'line' || v === 'valuation') {
      const c = await pickCompany(cfg.ticker);
      if (!c) return noCompany();
      if (v === 'company') {
        const price = await fundamentalsApi.stored('price', c.ticker);
        return { c, price, px: price?.data?.series ?? [] };
      }
      if (v === 'valuation') {
        let tickers = cfg.peerTickers ?? [];
        if (!tickers.length) {
          const peers = await fundamentalsApi.storedView(c.ticker, 'peers');
          tickers = (peers.peers ?? []).filter((p) => p.stored && !p.self).map((p) => p.ticker);
        }
        const rows = await Promise.all([...new Set([c.ticker, ...tickers])].slice(0, limit).map((tk) => fundamentalsApi.storedCompany(tk)));
        return { c, rows };
      }
      const line = cfg.line ?? 'revenue';
      const freq = cfg.freq ?? 'quarterly';
      const raw = await fundamentalsApi.statements(c.ticker, kindOf(line), freq === 'ttm' ? 'quarterly' : freq);
      const all = statementRows(raw, line, freq === 'ttm' && kindOf(line) !== 'balance');
      const last = all.at(-1);
      if (!last || last.lines?.[line] == null) return { empty: $t('dashboard.widgets.fundamentals.noLine') };
      return { c, line, freq, rows: all.slice(freq === 'annual' ? -8 : -12), last: last.lines[line], yoy: statementYoY(all, last, line), period: last.period, end: last.period_end };
    }
    if (v === 'companies') {
      // Sort the complete stored list before applying the visible limit.
      const list = (await fundamentalsApi.companies()).filter((c) => cfg.scope !== 'followed' || c.followed);
      const rows = await Promise.all(list.map((c) => fundamentalsApi.storedCompany(c.ticker).catch((e) => ({ ...c, metrics: {}, loadError: e.message }))));
      return rows.length ? { rows } : noCompany();
    }
    if (v === 'earnings') {
      const companies = (await fundamentalsApi.companies()).filter((c) => cfg.scope === 'all' || c.followed);
      const calendar = await fundamentalsApi.stored('calendar');
      const today = dateKey(new Date());
      const rows = (await Promise.all(companies.map(async (c) => {
        const [profile, snap] = await Promise.all([fundamentalsApi.storedCompany(c.ticker), fundamentalsApi.stored('earnings', c.ticker)]);
        const e = snap?.data;
        const companyDate = e?.next?.date && e.next.date >= today;
        const next = companyDate ? e.next : [...(calendar?.data?.earnings ?? [])]
          .filter((event) => event.ticker === c.ticker && event.date >= today).sort((a, b) => a.date.localeCompare(b.date))[0];
        if (!next?.date) return null;
        const latest = [...(e?.history ?? [])].filter((h) => h.eps_actual != null).sort((a, b) => (b.date ?? b.period ?? '').localeCompare(a.date ?? a.period ?? ''))[0];
        return { c: profile, next, latest, snap: companyDate ? snap : calendar };
      }))).filter(Boolean).sort((a, b) => a.next.date.localeCompare(b.next.date)).slice(0, limit);
      return rows.length ? { rows } : { empty: $t('dashboard.widgets.insights.noEarnings') };
    }
    if (v === 'filings') {
      const followed = cfg.scope === 'followed' ? new Set((await fundamentalsApi.companies()).filter((c) => c.followed).map((c) => c.ticker)) : null;
      const rows = (await fundamentalsApi.documents()).filter((d) => d.kind !== 'transcript' && !QUIET.includes(d.form) && !(d.form ?? '').startsWith('424') && (!followed || followed.has(d.ticker)) && (!cfg.form || d.form === cfg.form)).slice(0, limit);
      return rows.length ? { rows } : { empty: $t('dashboard.widgets.fundamentals.noFilings') };
    }
    return { empty: $t('dashboard.widgets.fundamentals.noSeries') };
  }

  $effect(() => {
    const cfg = { ...item.config };
    const v = variant;
    const signature = JSON.stringify(cfg);
    live.n;
    if (editing) return;
    let alive = true;
    if (signature !== asked) { asked = signature; data = null; empty = ''; }
    err = '';
    busy = true;
    load(cfg, v).then((out) => { if (alive) { data = out; empty = out.empty ?? ''; } })
      .catch((e) => { if (alive) err = e.message; }).finally(() => { if (alive) busy = false; });
    return () => { alive = false; };
  });
</script>

<WidgetState {editing} error={data === null ? err : ''} loading={data === null} empty={!!empty}
  preview={$t('dashboard.widgets.fundamentals.preview')} emptyText={empty}
  emptyAction={$t('dashboard.widgets.insights.openFundamentals')} {emptyHref} rows={4}>
  <div class="insight w-body" aria-busy={busy}>
    {#if variant === 'board'}
      <ul class="macro-board w-scroll">
        {#each data.rows as r (r.s.id)}
          <li class="macro-card">
            <a class="macro-title link" href={`/fundamentals?series=${encodeURIComponent(r.s.id)}`}>{r.s.title}</a>
            <div class="macro-meta context">
              {#if r.s.freq}<span class="macro-frequency">{frequency(r.s.freq)}</span>{/if}
              <span>{day(r.s.last_period)}</span>
              {#if r.s.provider}<span class="macro-source">{r.s.provider}</span>{/if}
            </div>
            <div class="macro-reading">
              <div class="macro-figure" title={`${seriesFmt(r.s, 'level')(r.s.last)}${!rate(r.s) && r.s.unit ? ` ${r.s.unit}` : ''}`}>
                <span class="macro-value">{boardValue(r.s)}</span>
                {#if !rate(r.s) && r.s.unit}<span class="macro-unit context">{r.s.unit}</span>{/if}
              </div>
              {#if r.spark.length > 1}<span class="macro-spark"><Spark values={r.spark} height={32} area={true} tone="accent" valueFormat={seriesFmt(r.s, 'level')} label={r.s.title} /></span>{/if}
            </div>
            <span class="macro-change context">{boardDelta(r.s)}</span>
          </li>
        {/each}
      </ul>
    {:else if variant === 'company'}
      {@const m = data.c.metrics ?? {}}
      <a class="ident row link" href={href(data.c.ticker)}><span class="ticker">{data.c.ticker}</span><span class="w-name grow">{data.c.name}</span></a>
      <Stat value={metricValue('price', m.price, data.c.currency)} size="sm"
        delta={m.change_pct == null ? '' : `${fmtPct(m.change_pct, 2, true)} · ${$t('dashboard.widgets.insights.previousClose')}`} deltaTone={tone(m.change_pct)} />
      {#if data.px.length > 1}<div class="w-fill chart"><TrendChart points={data.px.map(([at, value]) => ({ at, value }))} format={(v) => metricValue('price', v, data.c.currency)} ranges={['1m', '3m', '1y', 'all']} defaultRange="1y" showValue={false} label={data.c.ticker} /></div>
      {:else}<p class="w-state">{$t('dashboard.widgets.fundamentals.noPrice')}</p>{/if}
      <div class="w-metrics">{#each metrics as key (key)}<div title={missingMetric(key, data.c)}><span class="context">{metricName(key)}</span><span class="w-num">{metricValue(key, m[key], data.c.currency)}</span></div>{/each}</div>
      <p class="context">{m.basis || '—'} · {$t('dashboard.widgets.insights.closeAsOf', { date: day(m.price_date) })} · {data.price?.provider_label ?? ''}</p>
      {#if data.price?.stale}<p class="context w-warn">{$t('dashboard.widgets.insights.staleStored')}</p>{/if}
    {:else if variant === 'line'}
      <a class="ident row link" href={href(data.c.ticker, 'financials')}><span class="ticker">{data.c.ticker}</span><span class="w-name grow">{$t(`fundamentals.line.${data.line}`)}</span></a>
      <p class="context">{data.period} · {data.c.currency} · {data.freq === 'ttm' ? 'TTM' : $t(`fundamentals.fin.${data.freq}`)}</p>
      <Stat value={lineFmt(data.line)(data.last)} size="sm" delta={data.yoy == null ? '' : `${fmtPct(data.yoy, 1, true)} ${$t('dashboard.widgets.fundamentals.yoy')}`} />
      {#if data.yoy == null}<p class="context">{$t('dashboard.widgets.insights.noComparison')}</p>{/if}
      <div class="w-fill statement"><StatementBars rows={data.rows} line={data.line} format={lineFmt(data.line)} label={$t(`fundamentals.line.${data.line}`)} /></div>
      <p class="context">SEC XBRL · {$t('dashboard.widgets.insights.periodEnd', { date: day(data.end) })}</p>
    {:else if variant === 'companies' || variant === 'valuation'}
      {#if variant === 'valuation'}<a class="context link" href={href(data.c.ticker, 'peers')}>{data.c.ticker} · {$t('dashboard.widgets.insights.storedPeers')}</a>{/if}
      {@const keys = variant === 'valuation' ? ['pe', 'ev_ebitda', 'operating_margin', 'fcf_yield'] : columns}
      <div class="w-scroll table-scroll"><table class="w-tbl">
        <thead><tr><th aria-sort={sort === 'ticker' ? direction === 'asc' ? 'ascending' : 'descending' : 'none'}><button class="sort" onclick={() => sortBy('ticker')}>{$t('fundamentals.ticker')}</button></th>
          {#each keys as key, i (key)}<th class="num" class:extra={i >= 2} aria-sort={sort === key ? direction === 'asc' ? 'ascending' : 'descending' : 'none'}><button class="sort" onclick={() => sortBy(key)}>{metricName(key)}{sort === key ? direction === 'asc' ? ' ↑' : ' ↓' : ''}</button></th>{/each}
        </tr></thead>
        <tbody>{#each sorted.slice(0, limit) as c (c.ticker)}<tr class:self={variant === 'valuation' && c.ticker === data.c.ticker}>
          <td><a class="ident link" href={href(c.ticker, variant === 'valuation' ? 'peers' : 'overview')}><span class="ticker strong">{c.ticker}{c.followed ? ' ★' : ''}</span><span class="context w-name" title={c.name}>{variant === 'valuation' ? `${c.metrics?.basis || '—'} · ${c.currency}` : c.name}</span></a></td>
          {#each keys as key, i (key)}<td class="num" class:extra={i >= 2} title={missingMetric(key, c)} class:w-pos={key === 'change_pct' && c.metrics?.[key] > 0} class:w-neg={key === 'change_pct' && c.metrics?.[key] < 0}>{metricValue(key, c.metrics?.[key], key === 'price' || key === 'market_cap' ? c.currency : '')}</td>{/each}
        </tr>{/each}</tbody>
      </table></div>
      {#if variant === 'valuation'}
        <p class="context">{$t('dashboard.widgets.insights.valuationBasis')}</p>
        {#if data.rows.length < 2 && onconfigure}<button class="text-action" onclick={onconfigure}>{$t('dashboard.widgets.insights.choosePeers')}</button>{/if}
      {/if}
    {:else if variant === 'earnings'}
      <ul class="w-list w-scroll">{#each data.rows as r (r.c.ticker)}<li class="earn-row">
        <a class="ident link" href={href(r.c.ticker, 'earnings')}><span class="ticker strong">{r.c.ticker}</span><span class="context">{day(r.next.date)}{r.next.time ? ` · ${r.next.time}` : ''}</span></a>
        <span class="fig"><span class="context">{$t('fundamentals.earn.nextEps')}</span><span class="w-num">{fmtNum(r.next.eps_estimate, 2)} {r.c.currency}</span></span>
        <span class="fig"><span class="context">{$t('fundamentals.earn.revEst')}</span><span class="w-num">{fmtBig(r.next.revenue_estimate, r.c.currency)}</span></span>
        {#if r.latest}<span class="earn-note context">{$t('dashboard.widgets.insights.lastSurprise', { period: r.latest.period, value: fmtPct(r.latest.surprise_pct, 1, true) })}</span>{/if}
        <span class="earn-note context">{r.snap.provider_label} · {$t('dashboard.widgets.insights.updated', { date: day(r.snap.fetched_at) })}{r.snap.stale ? ` · ${$t('dashboard.widgets.insights.staleStored')}` : ''}</span>
      </li>{/each}</ul>
    {:else if variant === 'filings'}
      <ul class="w-list w-scroll">{#each data.rows as d (d.id)}<li class="w-row">
        <span class="form">{d.form}</span>
        <a class="ident grow link" href={d.url} target="_blank" rel="noopener noreferrer"><span class="w-name" title={d.title}>{d.ticker} · {formName(d.form)}</span><span class="context w-name" title={d.title}>{d.title}</span></a>
        <span class="context">{day(d.filed_at)}</span>
      </li>{/each}</ul>
    {:else}
      <a class="ident row link" href={`/fundamentals?series=${encodeURIComponent(data.s.id)}&tf=${data.tf}&range=${item.config?.range ?? '10y'}`}><span class="w-name grow" title={data.s.title}>{data.s.title}</span></a>
      <p class="context">{frequency(data.s.freq)} · {day(data.s.last_period)} · {data.s.provider}</p>
      {#if data.rows.length > 1}<div class="w-fill chart"><TrendChart points={data.rows.map(([at, value]) => ({ at, value }))}
        format={seriesFmt(data.s, data.tf)} valueLabel={data.tf === 'level' ? data.s.unit : $t(`fundamentals.macro.tf.${data.tf}`)}
        changeFormat={seriesDelta(data.s, data.tf)} showRelativeChange={false} showPill={false} changeLabel={$t('dashboard.widgets.insights.selectedWindow')}
        tone="accent" ranges={['1y', '2y', 'all']} label={data.s.title} /></div>
      {:else}<Stat value={seriesFmt(data.s, data.tf)(data.rows[0][1])} />{/if}
      <p class="context">{$t('dashboard.widgets.insights.updated', { date: day(data.s.updated) })}</p>
    {/if}
    {#if err}<p class="context w-warn" role="status">{$t('dashboard.widgets.insights.refreshFailed', { error: err })}</p>{/if}
  </div>
</WidgetState>

<style>
  .insight { gap: var(--space-2); }
  .context { margin: 0; color: var(--muted); font-size: 11px; line-height: 1.4; }
  .context.w-warn { color: var(--amber-ink); }
  .ident { display: flex; flex-direction: column; min-width: 0; gap: 3px; padding: 5px 0; }
  .ident.row { flex-direction: row; align-items: baseline; gap: var(--space-2); padding: 0; }
  .grow { flex: 1; min-width: 0; }
  .link { color: inherit; text-decoration: none; }
  .link:hover { color: var(--accent); }
  .link:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .ticker { font: 11px var(--mono); color: var(--muted); flex-shrink: 0; }
  .ticker.strong { font-size: var(--text-sm); color: var(--text); }
  .fig { display: flex; flex-direction: column; align-items: flex-end; flex-shrink: 0; gap: 3px; }
  /* Grid tracks retain their natural height when the widget is short: the board
     scrolls instead of compressing multi-line content into overlapping rows. */
  .macro-board { display: grid; grid-template-columns: minmax(0, 1fr); align-content: start; gap: 12px; list-style: none; margin: 0; padding: 2px; }
  .macro-card { display: flex; flex-direction: column; gap: 8px; min-width: 0; padding: 16px; border: var(--hairline) solid var(--border); border-radius: var(--radius); background: var(--surface); }
  .macro-card:focus-within { border-color: var(--accent); }
  .macro-title { font-size: 13px; font-weight: 600; line-height: 1.5; overflow-wrap: anywhere; }
  .macro-meta { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; overflow-wrap: anywhere; }
  .macro-frequency { padding: 2px 6px; border-radius: var(--radius-sm); background: var(--surface-2); }
  .macro-source { flex-basis: 100%; }
  .macro-reading { display: flex; align-items: center; gap: 16px; margin-top: auto; padding-top: 6px; }
  .macro-figure { display: flex; flex: 1; flex-direction: column; gap: 4px; min-width: 0; }
  .macro-value { font: 600 20px/1.3 var(--mono); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .macro-unit { overflow-wrap: anywhere; }
  .macro-spark { width: 88px; flex-shrink: 0; }
  .macro-change { overflow-wrap: anywhere; }
  @container widget (min-width: 640px) { .macro-board { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  @container widget (min-width: 1120px) { .macro-board { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
  @container widget (max-width: 280px) { .macro-spark { width: 48px; } .macro-card { padding: 12px; } .macro-value { font-size: 17px; } }
  .form { flex-shrink: 0; min-width: 44px; font: 11px var(--mono); color: var(--muted); }
  .chart { min-height: 80px; }
  .statement { min-height: 118px; }
  .table-scroll { overflow-x: auto; }
  .w-tbl td:first-child { max-width: 0; width: 100%; }
  .sort { font: inherit; color: var(--muted); border: 0; background: transparent; padding: 3px 0; cursor: pointer; white-space: nowrap; }
  .sort:hover { color: var(--accent); }
  .self { background: var(--accent-soft); }
  .text-action { color: var(--accent); background: transparent; border: 0; padding: 4px 0; text-align: left; cursor: pointer; }
  .earn-row { display: grid; grid-template-columns: 1fr auto auto; gap: 6px 12px; padding: 8px 0; border-bottom: var(--hairline) solid var(--border); }
  .earn-note { grid-column: 1 / -1; }
  @container widget (max-width: 360px) { .extra { display: none; } .earn-row { grid-template-columns: 1fr auto; } .earn-row > .fig:nth-child(3) { grid-column: 1 / -1; align-items: flex-start; } }
</style>
