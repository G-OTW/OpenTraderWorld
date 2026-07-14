<script>
  // Monte-Carlo of a saved backtest run's realized trade sequence. Pick a run from the
  // Backtest history; the server replays it to regenerate the exact trades, resamples them into
  // many equity paths, and returns percentile bands + a max-drawdown / final-equity distribution
  // and the risk of ruin. Left: run picker + parameters. Right: fan chart, stat cards, histograms.
  import { quantApi, fmtNum, fmtRatioPct } from '$lib/modules/quant/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import McFanChart from '$lib/modules/quant/McFanChart.svelte';
  import McDistChart from '$lib/modules/quant/McDistChart.svelte';

  let runs = $state([]);
  let runId = $state(null);
  let error = $state('');
  let busy = $state(false);
  let mc = $state(null); // { name, ticker, timeframe, result }

  // Parameters.
  let iterations = $state(5000);
  let block = $state(1); // 1 = IID bootstrap; >1 = block bootstrap (streak-preserving)
  let ruinPct = $state(50); // whole percent of starting capital
  let horizon = $state(''); // blank → use the run's own trade count

  const ITER_PRESETS = [1000, 5000, 20000];

  $effect(() => {
    quantApi
      .backtestRuns()
      .then((r) => {
        runs = r;
        if (!runId && r.length) runId = r[0].id;
      })
      .catch((e) => (error = e.message));
  });

  const selectedRun = $derived(runs.find((r) => r.id === runId) ?? null);

  async function run() {
    if (!runId) return;
    error = '';
    busy = true;
    try {
      // Empty/cleared field (bind gives '' or null) → null → the run's own trade count.
      const n = Math.round(Number(horizon));
      mc = await quantApi.monteCarlo(runId, {
        iterations,
        block: Math.max(1, Math.round(Number(block))),
        ruin_pct: Math.min(0.99, Math.max(0, Number(ruinPct) / 100)),
        horizon: horizon !== '' && horizon != null && Number.isFinite(n) && n >= 1 ? n : null
      });
    } catch (e) {
      error = e.message;
      mc = null;
    } finally {
      busy = false;
    }
  }

  const cards = $derived.by(() => {
    if (!mc) return [];
    const r = mc.result;
    return [
      {
        label: $t('quant.mc.riskOfRuin', { pct: fmtRatioPct(r.ruin_level / r.start_capital, 0) }),
        value: fmtRatioPct(r.risk_of_ruin, 1),
        tone: r.risk_of_ruin > 0.05 ? 'red' : r.risk_of_ruin > 0 ? 'amber' : 'green',
        hint: $t('quant.mc.riskOfRuinHint')
      },
      {
        label: $t('quant.mc.medianMaxDd'),
        value: `−${fmtRatioPct(r.max_drawdown.p50, 1)}`,
        tone: 'red',
        hint: $t('quant.mc.medianMaxDdHint', { p95: fmtRatioPct(r.max_drawdown.p95, 1) })
      },
      {
        label: $t('quant.mc.probLoss'),
        value: fmtRatioPct(r.prob_loss, 1),
        tone: r.prob_loss > 0.5 ? 'red' : 'amber',
        hint: $t('quant.mc.probLossHint')
      },
      {
        label: $t('quant.mc.medianFinal'),
        value: fmtNum(r.final_equity.p50, 0),
        tone: r.median_return >= 0 ? 'green' : 'red',
        hint: $t('quant.mc.medianFinalHint', {
          p5: fmtNum(r.final_equity.p5, 0),
          p95: fmtNum(r.final_equity.p95, 0)
        })
      }
    ];
  });
</script>

<div class="mc-layout">
  <aside class="panel">
    <h3>{$t('quant.mc.sourceRun')}</h3>
    <p class="muted small">{$t('quant.mc.sourceRunHint')}</p>
    {#if runs.length === 0}
      <p class="muted small">{@html $t('quant.mc.noRunsHint')}</p>
    {:else}
      <select bind:value={runId}>
        {#each runs as r (r.id)}
          <option value={r.id}>{r.name} · {r.ticker} {r.timeframe} · {fmtNum(r.stats?.trades ?? 0, 0)} {$t('quant.mc.trades')}</option>
        {/each}
      </select>
    {/if}

    <div class="field">
      <span class="ctrl-label">{$t('quant.mc.iterations')}</span>
      <div class="seg">
        {#each ITER_PRESETS as n (n)}
          <button class="preset" class:active={iterations === n} onclick={() => (iterations = n)}>
            {n >= 1000 ? `${n / 1000}k` : n}
          </button>
        {/each}
      </div>
    </div>

    <div class="field">
      <span class="ctrl-label">{$t('quant.mc.method')}</span>
      <div class="seg">
        <button class="preset" class:active={block <= 1} onclick={() => (block = 1)}>{$t('quant.mc.bootstrap')}</button>
        <button class="preset" class:active={block > 1} onclick={() => (block = block > 1 ? block : 5)}>{$t('quant.mc.block')}</button>
      </div>
      {#if block > 1}
        <label class="inline">
          <span class="muted small">{$t('quant.mc.blockLen')}</span>
          <input type="number" min="2" max="50" step="1" bind:value={block} />
        </label>
      {/if}
    </div>

    <div class="field">
      <label class="inline">
        <span class="ctrl-label">{$t('quant.mc.ruinThreshold')}</span>
        <span class="num-wrap"><input type="number" min="1" max="99" step="1" bind:value={ruinPct} /><span class="suffix">%</span></span>
      </label>
      <span class="muted small">{$t('quant.mc.ruinThresholdHint')}</span>
    </div>

    <div class="field">
      <label class="inline">
        <span class="ctrl-label">{$t('quant.mc.horizon')}</span>
        <input type="number" min="1" step="1" placeholder={selectedRun ? fmtNum(selectedRun.stats?.trades ?? 0, 0) : ''} bind:value={horizon} />
      </label>
      <span class="muted small">{$t('quant.mc.horizonHint')}</span>
    </div>

    <button class="primary" onclick={run} disabled={!runId || busy}>
      {busy ? $t('quant.mc.running') : $t('quant.mc.runSimulation')}
    </button>
  </aside>

  <main class="mc-main">
    <ErrorText {error} copyable />
    {#if mc}
      <div class="head">
        <span class="title">{mc.name}</span>
        <span class="sub">{mc.ticker} · {mc.timeframe} · {$t('quant.mc.summary', {
          iters: fmtNum(mc.result.iterations, 0),
          n: fmtNum(mc.result.source_trades, 0),
          h: fmtNum(mc.result.horizon, 0)
        })}</span>
      </div>

      <div class="cards">
        {#each cards as c (c.label)}
          <div class="card">
            <span class="lbl">{c.label}</span>
            <span class="val" class:red={c.tone === 'red'} class:amber={c.tone === 'amber'} class:green={c.tone === 'green'}>{c.value}</span>
            <span class="note">{c.hint}</span>
          </div>
        {/each}
      </div>

      <section class="block">
        <h3>{$t('quant.mc.equityFan')}</h3>
        <p class="muted small">{$t('quant.mc.equityFanHint')}</p>
        <McFanChart result={mc.result} />
      </section>

      <section class="block">
        <h3>{$t('quant.mc.distributions')}</h3>
        <p class="muted small">{$t('quant.mc.distributionsHint')}</p>
        <McDistChart result={mc.result} />
      </section>
    {:else if !busy}
      <p class="hint">{$t('quant.mc.pickRunHint')}</p>
    {/if}
  </main>
</div>

<style>
  .mc-layout {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 300px 1fr;
    gap: var(--space-4);
  }
  .panel {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    height: fit-content;
  }
  .panel h3 {
    font-size: var(--text-base);
    font-weight: var(--fw-semibold);
  }
  select,
  input[type='number'] {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: var(--space-2);
    font-size: var(--text-sm);
    width: 100%;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .ctrl-label {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .seg {
    display: inline-flex;
    gap: var(--space-1);
  }
  .preset {
    flex: 1;
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-sm);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
    transition:
      background 0.12s,
      border-color 0.12s,
      color 0.12s;
  }
  .preset:hover:not(.active) {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }
  .preset.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-contrast);
  }
  .inline {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .inline input[type='number'] {
    width: 90px;
  }
  .num-wrap {
    position: relative;
    display: inline-flex;
    align-items: center;
  }
  .num-wrap input {
    width: 90px;
    padding-right: 22px;
  }
  .num-wrap .suffix {
    position: absolute;
    right: 8px;
    color: var(--muted);
    font-size: var(--text-sm);
    pointer-events: none;
  }
  .mc-main {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .title {
    font-weight: var(--fw-semibold);
  }
  .sub {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-3);
  }
  .card {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-2);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .lbl {
    font-size: var(--text-xs);
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .val {
    font-size: var(--text-xl);
    font-weight: var(--fw-semibold);
    font-variant-numeric: tabular-nums;
  }
  .val.red {
    color: var(--red);
  }
  .val.amber {
    color: var(--amber);
  }
  .val.green {
    color: var(--green);
  }
  .note {
    font-size: var(--text-sm);
    color: var(--muted);
    line-height: 1.3;
  }
  .block {
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .block h3 {
    font-size: var(--text-md);
    font-weight: var(--fw-semibold);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-sm);
  }
  .hint {
    color: var(--muted);
    padding: var(--space-4);
  }
</style>
