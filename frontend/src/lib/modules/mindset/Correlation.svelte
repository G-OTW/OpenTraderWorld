<script>
  // Mindset against trading: the user picks a journal (a journal category is one book),
  // and the days that carry both a scored check-in and a realized trade are compared.
  // Two reads: the headline split (above-average mindset days vs below) and the scatter
  // behind it, with a Pearson r for the sceptic.
  import { onMount } from 'svelte';
  import Select from '$lib/ui/Select.svelte';
  import { journalApi } from '$lib/modules/journal/api.js';
  import { fmtMoney } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import { pref } from '$lib/prefs.svelte.js';
  import { pearson, meanOf, roundOf } from './scoring.js';
  import WordCloud from './WordCloud.svelte';
  import { tagStats, wordClouds } from './text.js';

  // `days` = scored days from the dashboard ({ date, score, tags, texts }); `mean` their
  // mean; `extremes` the days outside the band, told here in their own words.
  // `compared` tells the page a journal with overlapping days is in play, so the word
  // clouds live here (split by P&L) instead of on the dashboard (split by score).
  let { days = [], mean = null, extremes = [], from = '', to = '', onpick = null, compared = $bindable(false) } = $props();

  const STORE = 'otw-mindset-journal';
  let journals = $state([]);
  // The picked journal, a user preference; a pick that no longer exists falls back to
  // the default journal.
  const journalPref = pref(STORE, '');
  const journalId = $derived.by(() => {
    const saved = journalPref.value;
    if (journals.some((j) => j.id === saved)) return saved;
    return journals.find((j) => j.is_default)?.id ?? journals[0]?.id ?? '';
  });
  let cal = $state(null);
  let currency = $state('USD');
  let error = $state('');
  let loading = $state(false);

  onMount(async () => {
    try {
      journals = await journalApi.listCategories();
    } catch (e) {
      error = e.message;
    }
  });

  $effect(() => {
    if (!journalId || !from || !to) return;
    loading = true;
    error = '';
    journalApi
      .calendar({ category_id: journalId, since: `${from}T00:00:00Z`, until: `${to}T23:59:59Z` })
      .then((r) => {
        cal = r.days ?? [];
        currency = r.display_currency ?? 'USD';
      })
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  });

  const options = $derived(journals.map((j) => ({ value: j.id, label: j.name })));

  // A day with only open trades has a net of 0 in the calendar: that is no result, so it
  // counts as neither a win nor a loss and stays out of every P&L read.
  const closed = (d) => d.wins + d.losses > 0;

  // Paired days: both a score and at least one closed trade with a result.
  const pairs = $derived.by(() => {
    if (!cal) return [];
    const pnl = new Map(cal.filter(closed).map((d) => [d.date, d]));
    return days
      .filter((d) => pnl.has(d.date))
      .map((d) => ({ date: d.date, score: d.score, pnl: pnl.get(d.date).net_pnl, trades: pnl.get(d.date).trades }));
  });
  const r = $derived(pearson(pairs.map((p) => [p.score, p.pnl])));
  const pnlByDate = $derived(new Map((cal ?? []).filter(closed).map((d) => [d.date, d.net_pnl])));
  const tags = $derived(tagStats(days, pnlByDate));
  const clouds = $derived(
    wordClouds(days, (d) => {
      const pnl = pnlByDate.get(d.date);
      return !pnl ? null : pnl > 0;
    })
  );
  $effect(() => {
    compared = pairs.length > 0;
  });
  const money = (v) => (v == null ? '·' : fmtMoney(v, currency, 0));
  const pct = (v) => (v == null ? '' : `${Math.round(v * 100)}%`);
  const fmtDay = (iso) =>
    new Date(`${iso}T00:00:00`).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
  const split = $derived.by(() => {
    if (mean == null) return null;
    const grp = (xs) => ({
      n: xs.length,
      avg: meanOf(xs.map((p) => p.pnl)),
      win: xs.length ? xs.filter((p) => p.pnl > 0).length / xs.length : null
    });
    return { above: grp(pairs.filter((p) => p.score >= mean)), below: grp(pairs.filter((p) => p.score < mean)) };
  });

  // Scatter geometry: x = score 0..100, y = P&L on a symmetric domain around zero.
  const W = 800;
  const H = 220;
  const L = 60;
  const R = 12;
  const T = 12;
  const B = 26;
  const plotW = W - L - R;
  const plotH = H - T - B;
  const maxAbs = $derived(Math.max(1, ...pairs.map((p) => Math.abs(p.pnl))));
  const x = (s) => L + (s / 100) * plotW;
  const y = (v) => T + (1 - (v + maxAbs) / (2 * maxAbs)) * plotH;
</script>

<div class="corr">
  <div class="head">
    <Select
      label={$t('mindset.history.corrJournal')}
      {options}
      bind:value={() => journalId, (v) => (journalPref.value = v)}
      placeholder={$t('mindset.history.corrPick')}
    />
    {#if r != null}
      <span class="r">{$t('mindset.history.corrR', { r: r.toFixed(2), n: pairs.length })}</span>
    {/if}
  </div>
  {#if error}
    <p class="err">{error}</p>
  {:else if !journalId}
    <p class="muted">{$t('mindset.history.corrPick')}</p>
  {:else if loading && !cal}
    <p class="muted">…</p>
  {:else if pairs.length === 0}
    <p class="muted">{$t('mindset.history.corrNoDays')}</p>
  {:else}
    {#if split}
      <div class="split">
        {#each [['above', split.above], ['below', split.below]] as [k, g] (k)}
          <div class="tile">
            <span class="tl">{$t(k === 'above' ? 'mindset.history.corrAbove' : 'mindset.history.corrBelow')}</span>
            <span class="tv num" class:pos={g.avg > 0} class:neg={g.avg < 0}>{g.n ? fmtMoney(g.avg, currency) : '·'}</span>
            <span class="th">{$t('mindset.history.corrAvgPnl')}</span>
            <span class="tm">{$t('mindset.history.corrWinDays', { pct: pct(g.win), n: g.n })}</span>
          </div>
        {/each}
      </div>
    {/if}
    <svg viewBox="0 0 {W} {H}" role="img" aria-label={$t('mindset.history.corrScatter')}>
      <line x1={L} y1={y(0)} x2={L + plotW} y2={y(0)} class="zero" />
      {#each [0, 50, 100] as g (g)}
        <text x={x(g)} y={H - 8} class="axis" text-anchor={g === 0 ? 'start' : g === 100 ? 'end' : 'middle'}>{g}</text>
      {/each}
      <text x={L - 6} y={y(maxAbs) + 4} class="axis y">{fmtMoney(maxAbs, currency, 0)}</text>
      <text x={L - 6} y={y(-maxAbs) + 4} class="axis y">{fmtMoney(-maxAbs, currency, 0)}</text>
      {#if mean != null}
        <line x1={x(mean)} y1={T} x2={x(mean)} y2={T + plotH} class="meanline" />
      {/if}
      {#each pairs as p (p.date)}
        <circle cx={x(p.score)} cy={y(p.pnl)} r="5" class="pt" class:pos={p.pnl > 0} class:neg={p.pnl < 0}>
          <title>{p.date}: {roundOf(p.score)} / {fmtMoney(p.pnl, currency)} ({p.trades})</title>
        </circle>
      {/each}
    </svg>
  {/if}

  <!-- Tags: the structured half of the free answers. Works without a journal; the P&L
       columns fill once one is chosen. -->
  <div class="block">
    <h3>{$t('mindset.history.corrTags')}</h3>
    <p class="hint">{$t('mindset.history.corrTagsHint')}</p>
    {#if tags.length === 0}
      <p class="muted">{$t('mindset.history.corrTagsNone')}</p>
    {:else}
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('mindset.history.colTag')}</th>
            <th class="num">{$t('mindset.history.colDays')}</th>
            <th class="num">{$t('mindset.history.colScore')}</th>
            <th class="num">{$t('mindset.history.colPnl')}</th>
            <th class="num">{$t('mindset.history.colWin')}</th>
          </tr>
        </thead>
        <tbody>
          {#each tags as tg (tg.tag)}
            <tr>
              <td><span class="chip sm">{tg.tag}</span></td>
              <td class="num">{tg.n}</td>
              <td class="num" class:pos={mean != null && tg.score >= mean} class:neg={mean != null && tg.score < mean}>{roundOf(tg.score)}</td>
              <td class="num" class:pos={tg.pnl > 0} class:neg={tg.pnl < 0}>{money(tg.pnl)}</td>
              <td class="num">{tg.traded ? $t('mindset.history.winOf', { pct: pct(tg.win), n: tg.traded }) : '·'}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>

  {#if pairs.length > 0}
    <div class="block">
      <h3>{$t('mindset.history.cloudTitle')}</h3>
      <p class="hint">{$t('mindset.history.cloudHintTrading')}</p>
      <WordCloud
        good={clouds.good}
        bad={clouds.bad}
        docs={clouds.docs}
        goodLabel={$t('mindset.history.wordsWin')}
        badLabel={$t('mindset.history.wordsLoss')}
      />
    </div>
  {/if}

  <!-- Extreme days, in their own words and with their P&L. -->
  {#if extremes.length > 0}
    <div class="block">
      <h3>{$t('mindset.history.corrExtremes')}</h3>
      <p class="hint">{$t('mindset.history.corrExtremesHint')}</p>
      <ul class="xdays">
        {#each extremes as d (d.date)}
          <li class="xday {d.dir}">
            <button class="xhead" onclick={() => onpick?.(d.date)}>
              <span class="xdate">{fmtDay(d.date)}</span>
              <span class="xscore num">{$t('mindset.history.score')} {roundOf(d.score)}</span>
              {#if pnlByDate.has(d.date)}
                <span class="xpnl num" class:pos={pnlByDate.get(d.date) > 0} class:neg={pnlByDate.get(d.date) < 0}>{fmtMoney(pnlByDate.get(d.date), currency)}</span>
              {/if}
              {#if d.tags?.length}
                <span class="xtags">{[...new Set(d.tags)].join(', ')}</span>
              {/if}
            </button>
            {#if d.texts?.length}
              <ul class="xtexts">
                {#each d.texts as tx, i (i)}
                  <li><span class="xq">{tx.label}</span> {tx.text}</li>
                {/each}
              </ul>
            {:else}
              <span class="muted small">{$t('mindset.history.noText')}</span>
            {/if}
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</div>

<style>
  .corr {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: flex-end;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  .head :global(.field) {
    min-width: 240px;
  }
  .r {
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--muted);
    padding-bottom: var(--space-2);
  }
  .split {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: var(--space-3);
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--bg);
  }
  .tl {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .tv {
    font-size: 1.5rem;
    font-weight: var(--fw-medium);
  }
  .tv.pos {
    color: var(--green);
  }
  .tv.neg {
    color: var(--red);
  }
  .th,
  .tm {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  svg {
    width: 100%;
    height: auto;
    display: block;
  }
  .zero {
    stroke: var(--grid-line);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  .meanline {
    stroke: var(--dim);
    stroke-width: 1;
    stroke-dasharray: 4 4;
    vector-effect: non-scaling-stroke;
  }
  .axis {
    fill: var(--dim);
    font-family: var(--mono);
    font-size: 11px;
  }
  .axis.y {
    text-anchor: end;
  }
  .pt {
    fill: var(--chart-1);
    stroke: var(--surface);
    stroke-width: 2;
    vector-effect: non-scaling-stroke;
  }
  .pt.pos {
    fill: var(--green);
  }
  .pt.neg {
    fill: var(--red);
  }
  .block {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
  }
  h3 {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .hint {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .tbl {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }
  .tbl th,
  .tbl td {
    padding: var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    text-align: left;
  }
  .tbl th {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    font-weight: var(--fw-medium);
  }
  .tbl .num {
    text-align: right;
    font-family: var(--mono);
    white-space: nowrap;
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .xdays {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .xday {
    border-left: 2px solid var(--muted);
    padding-left: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .xday.high {
    border-color: var(--green);
  }
  .xday.low {
    border-color: var(--red);
  }
  .xhead {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-3);
    background: transparent;
    border: none;
    padding: 0;
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
    text-align: left;
  }
  .xhead:hover .xdate {
    color: var(--accent);
  }
  .xdate {
    font-weight: var(--fw-medium);
  }
  .xscore,
  .xtags {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .xtexts {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .xtexts li {
    font-size: var(--text-sm);
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .xq {
    color: var(--dim);
    font-size: var(--text-xs);
  }
  .small {
    font-size: var(--text-xs);
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .err {
    color: var(--red);
    font-size: var(--text-sm);
  }
</style>
