<script>
  import { page } from '$app/stores';
  import { t } from '$lib/i18n';
  // Print-clean report for a saved run. Loads the run, reruns it to get the full result
  // (trades, equity, benchmark, per-asset), and renders a layout tuned for the browser's
  // Print → Save as PDF (an @media print block strips chrome). Also links the raw .md export.
  import { backtestApi, migrateSettings, normalizeSettings, embedIndicators, fmtNum, conditionText, inverseSide } from '$lib/modules/backtest/api.js';
  import ResultChart from '$lib/modules/backtest/ResultChart.svelte';
  import StatsGrid from '$lib/modules/backtest/StatsGrid.svelte';
  import PerfTable from '$lib/modules/backtest/PerfTable.svelte';
  import AssetBreakdown from '$lib/modules/backtest/AssetBreakdown.svelte';
  import OosBlock from '$lib/modules/backtest/OosBlock.svelte';
  import RequireModule from '$lib/modules/RequireModule.svelte';

  const id = $derived($page.params.id);
  // Return to the page we came from (expert or normal), reloading the same run.
  const backUrl = $derived(
    ($page.url.searchParams.get('from') === 'expert' ? '/backtest/expert' : '/backtest') + `?run=${id}`
  );

  let run = $state(null);
  let result = $state(null);
  let bars = $state(null);
  let datasetIds = $state([]);
  let chartAssetId = $state(null);
  let error = $state('');
  let loading = $state(true);

  // Assets for the chart picker: pair each dataset id with its ticker from per_asset (same order
  // as the run's dataset_ids). Single-asset runs yield one entry → picker hidden.
  const chartAssets = $derived(
    datasetIds.map((id, i) => ({ id, ticker: result?.per_asset?.[i]?.ticker ?? run?.ticker ?? String(id) }))
  );
  async function switchChartAsset(id) {
    const nid = datasetIds.find((d) => String(d) === String(id)) ?? id;
    chartAssetId = nid;
    try {
      bars = await backtestApi.bars(nid);
    } catch (e) {
      error = e.message;
    }
  }

  async function load() {
    loading = true;
    error = '';
    try {
      const runs = await backtestApi.runs();
      run = runs.find((r) => r.id === id) ?? null;
      if (!run) throw new Error($t('backtest.report.notFound'));
      const ids = run.dataset_ids?.length ? run.dataset_ids : run.dataset_id ? [run.dataset_id] : [];
      if (!ids.length) throw new Error($t('backtest.page.datasetDeletedErr'));
      const settings = embedIndicators(normalizeSettings(migrateSettings(run.settings)), []);
      // Replaying a stored run to rebuild its chart — not a new run, so keep it out of history.
      const [res, b] = await Promise.all([
        backtestApi.run(ids, settings, { record: false }),
        backtestApi.bars(ids[0])
      ]);
      result = res;
      bars = b;
      datasetIds = ids;
      chartAssetId = ids[0];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    if (id) load();
  });

  const generatedAt = new Date().toLocaleString();

  // Strategy definition for the report body (migrated from the saved run).
  const strat = $derived(run?.settings ? migrateSettings(run.settings) : null);
  const sizingText = $derived.by(() => {
    const z = strat?.sizing;
    if (!z) return null;
    if (z.mode === 'percent_equity') return `${fmtNum(z.percent)}% of equity`;
    if (z.mode === 'fixed_qty') return `${fmtNum(z.qty)} units${strat.leverage > 1 ? ` × ${fmtNum(strat.leverage)} leverage` : ''}`;
    if (z.mode === 'fixed_cash') return `${fmtNum(z.cash)} cash`;
    return z.mode;
  });
  // [label, side] pairs actually used by the run's direction.
  const sides = $derived(
    strat
      ? [
          ['Long', strat.mode !== 'short' ? strat.long : null],
          ['Short', strat.mode !== 'long' ? (strat.reverse_side ? inverseSide(strat.long) : strat.short) : null]
        ].filter(([, s]) => s)
      : []
  );
</script>

<RequireModule module="backtest">
<div class="scroll">
<div class="report">
  <div class="toolbar no-print">
    <a class="btn" href={backUrl}>← {$t('backtest.report.back')}</a>
    <div class="spacer"></div>
    {#if run}
      <a class="btn" href={backtestApi.reportUrl(run.id)} download={`${run.name}.md`}>⬇ {$t('backtest.report.downloadMd')}</a>
      <button class="btn primary" onclick={() => window.print()}>🖨 {$t('backtest.report.printPdf')}</button>
    {/if}
  </div>

  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else if error}
    <p class="err">{error}</p>
  {:else if run && result}
    <header class="doc-head">
      <h1>{run.name}</h1>
      <p class="sub">
        {#if result.per_asset?.length > 1}{result.per_asset.map((a) => a.ticker).join(' · ')}{:else}{run.ticker}{/if}
        · {run.timeframe} · <span class="muted">{$t('backtest.report.generated', { at: generatedAt })}</span>
      </p>
    </header>

    <StatsGrid stats={result.stats} />

    {#if strat}
      <section class="strategy">
        <h2>{$t('backtest.report.strategy')}</h2>
        <div class="params">
          <div><span class="k">{$t('backtest.report.direction')}</span><span class="v">{strat.mode}</span></div>
          <div><span class="k">{$t('backtest.report.startingCapital')}</span><span class="v">{fmtNum(strat.starting_capital)}</span></div>
          {#if strat.leverage != null}<div><span class="k">{$t('backtest.report.leverage')}</span><span class="v">{strat.leverage}×</span></div>{/if}
          {#if sizingText}<div><span class="k">{$t('backtest.report.sizing')}</span><span class="v">{sizingText}</span></div>{/if}
          {#if strat.pyramiding > 1}<div><span class="k">{$t('backtest.report.pyramiding')}</span><span class="v">{strat.pyramiding}</span></div>{/if}
          {#if strat.fees}<div><span class="k">{$t('backtest.report.fees')}</span><span class="v">{fmtNum(strat.fees.amount)}{strat.fees.amount_kind === 'pct' ? '%' : ''} / {strat.fees.per}</span></div>{/if}
          {#if strat.spread_pct}<div><span class="k">{$t('backtest.report.spread')}</span><span class="v">{fmtNum(strat.spread_pct)}%</span></div>{/if}
          {#if strat.oos_split_pct}<div><span class="k">{$t('backtest.report.oosSplit')}</span><span class="v">{fmtNum(strat.oos_split_pct)}%</span></div>{/if}
        </div>
        {#each sides as [label, side] (label)}
          <div class="side">
            <span class="side-label">{label}</span>
            <ul>
              {#each side.entry?.conditions ?? [] as c, i (i)}
                <li><b>{i === 0 ? $t('backtest.report.entry') : (side.entry.logic === 'any' ? 'OR' : 'AND')}</b> {conditionText(c)}</li>
              {/each}
              {#each side.exit?.conditions ?? [] as c, i (i)}
                <li><b>{i === 0 ? $t('backtest.report.exit') : (side.exit.logic === 'any' ? 'OR' : 'AND')}</b> {conditionText(c)}</li>
              {/each}
              {#if side.stop_loss_pct != null || side.take_profit_pct != null || side.exit_on_reverse}
                <li class="risk">
                  {#if side.stop_loss_pct != null}SL {fmtNum(side.stop_loss_pct)}%{/if}
                  {#if side.take_profit_pct != null} · TP {fmtNum(side.take_profit_pct)}%{/if}
                  {#if side.exit_on_reverse} · exit on reverse{/if}
                </li>
              {/if}
            </ul>
          </div>
        {/each}
      </section>
    {/if}

    {#if result.grid}
      <div class="grid-stats">
        <span class="cap">{$t('backtest.grid.statsTitle')}</span>
        <span>{$t('backtest.grid.fills')}: <b>{result.grid.fills}</b></span>
        <span>{$t('backtest.grid.roundTrips')}: <b>{result.grid.round_trips}</b></span>
        <span>{$t('backtest.grid.endInventory')}: <b>{fmtNum(result.grid.end_inventory, 4)}</b> ({fmtNum(result.grid.end_inventory_value)})</span>
      </div>
    {/if}

    <div class="chart-box">
      <ResultChart {bars} trades={result.trades} equity={result.equity} benchmark={result.benchmark ?? []}
        assets={chartAssets} bind:activeAssetId={chartAssetId} onassetchange={switchChartAsset} />
    </div>

    {#if result.oos}<OosBlock oos={result.oos} />{/if}
    <AssetBreakdown perAsset={result.per_asset ?? []} />
    <PerfTable stats={result.stats} />
  {/if}
</div>
</div>
</RequireModule>

<style>
  /* The app shell (`.module-context`) clips overflow, so the report page provides its own
     scroll viewport on screen; released again for print so pages flow. */
  .scroll {
    height: 100%;
    overflow-y: auto;
  }
  .report {
    max-width: 960px;
    margin: 0 auto;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .spacer {
    flex: 1;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 0;
    color: var(--text);
    text-decoration: none;
    font-size: var(--text-base);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--accent);
  }
  .btn:hover {
    border-color: var(--border-control);
  }
  .doc-head h1 {
    font-size: 1.5rem;
    font-weight: var(--fw-medium);
  }
  .sub {
    color: var(--muted);
    font-size: var(--text-base);
    margin-top: 2px;
  }
  .muted {
    color: var(--muted);
  }
  .err {
    color: var(--red);
  }
  .chart-box {
    height: 460px;
  }
  .grid-stats {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-base);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--surface-2) 40%, transparent);
  }
  .grid-stats .cap {
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    font-size: 0.7rem;
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  .grid-stats b {
    font-variant-numeric: tabular-nums;
  }
  .strategy {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    break-inside: avoid;
  }
  .strategy h2 {
    font-size: 0.7rem;
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  .params {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: var(--space-2) var(--space-4);
    font-size: var(--text-base);
  }
  .params .k {
    color: var(--muted);
    margin-right: var(--space-2);
  }
  .params .v {
    font-weight: var(--fw-medium);
    font-variant-numeric: tabular-nums;
  }
  .side {
    border-top: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
    padding-top: var(--space-2);
  }
  .side-label {
    font-weight: var(--fw-medium);
    font-size: var(--text-base);
  }
  .side ul {
    list-style: none;
    margin-top: var(--space-1);
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-base);
  }
  .side ul b {
    display: inline-block;
    min-width: 3.2em;
    color: var(--muted);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .side .risk {
    color: var(--muted);
  }

  /* Print: strip toolbar + app chrome, force a light, ink-friendly page. */
  @media print {
    .no-print {
      display: none !important;
    }
    /* The SPA shell caps the module area at one viewport (`.app` height:100vh,
       `.module-context` overflow:hidden), which truncates the print to a single page.
       Release those constraints so the full report flows across printed pages. */
    :global(html),
    :global(body),
    :global(.app),
    :global(.module-context) {
      height: auto !important;
      min-height: 0 !important;
      max-height: none !important;
      overflow: visible !important;
      display: block !important;
    }
    :global(.topbar) {
      display: none !important;
    }
    .scroll {
      height: auto;
      overflow: visible;
    }
    .report {
      max-width: none;
      padding: 0;
    }
    .chart-box {
      height: 360px;
      break-inside: avoid;
    }
    /* Keep tables and blocks from splitting awkwardly across page breaks. */
    :global(table),
    :global(.grid-stats) {
      break-inside: avoid;
    }
  }
</style>
