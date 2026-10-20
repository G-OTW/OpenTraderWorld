<script>
  // Concentration curve: how much of the gross profit the best trades carry.
  //
  // X = share of the winning trades, ranked best first. Y = share of the gross profit
  // they add up to. The diagonal is the account where every winner contributed the same
  // amount; the further the curve bows above it, the more the result rests on a handful
  // of trades. A vertical wall at the left edge means one trade made the year.
  //
  // Derived client-side from the per-trade payload the analytics endpoint already sends:
  // the headline scalars come from the server, the resolution of the drawing is a
  // rendering concern.
  import { fmtMoney, fmtPct } from './api.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let { points = [], currency = 'USD', height = 190 } = $props();

  // Winners only: a Lorenz curve over mixed signs has no meaning. Biggest first, so the
  // curve reads "the top X% of my winners make Y% of my profit".
  const wins = $derived(
    points
      .map((p) => p.net)
      .filter((n) => n > 0)
      .sort((a, b) => b - a)
  );
  const gross = $derived(wins.reduce((a, b) => a + b, 0));

  // Cumulative share, as {x, y} in percent. Starts at the origin so the polyline closes
  // onto the axis instead of floating.
  const curve = $derived.by(() => {
    if (wins.length < 2 || gross <= 0) return [];
    const out = [{ x: 0, y: 0 }];
    let acc = 0;
    for (let i = 0; i < wins.length; i++) {
      acc += wins[i];
      out.push({ x: ((i + 1) / wins.length) * 100, y: (acc / gross) * 100 });
    }
    return out;
  });

  const W = 100;
  const H = 100;
  const path = $derived(curve.map((p) => `${p.x.toFixed(2)},${(H - p.y).toFixed(2)}`).join(' '));
  const area = $derived(curve.length ? `0,${H} ${path} ${W},${H}` : '');

  // The reading marks: where the top decile and the top quartile of winners land.
  function shareAt(fraction) {
    if (wins.length < 2 || gross <= 0) return null;
    const n = Math.max(1, Math.round(wins.length * fraction));
    let acc = 0;
    for (let i = 0; i < n; i++) acc += wins[i];
    return { count: n, share: (acc / gross) * 100 };
  }
  const top10 = $derived(shareAt(0.1));
  const top25 = $derived(shareAt(0.25));
</script>

{#if curve.length === 0}
  <EmptyState compact icon="pie-chart" title={$t('journal.behavior.concentration.curveEmpty')} />
{:else}
  <div class="wrap">
    <svg
      class="plot"
      style="--h:{height}px"
      viewBox="0 0 {W} {H}"
      preserveAspectRatio="none"
      role="img"
      aria-label={$t('journal.behavior.concentration.curveTitle')}
    >
      <!-- The even account: every winner contributes the same slice. -->
      <line class="ref" x1="0" y1={H} x2={W} y2="0" vector-effect="non-scaling-stroke" />
      <polygon class="fill" points={area} />
      <polyline class="line" points={path} vector-effect="non-scaling-stroke" fill="none" />
    </svg>
    <div class="axis">
      <span>{$t('journal.behavior.concentration.axisX')}</span>
      <span>{$t('journal.behavior.concentration.axisY')}</span>
    </div>
    <ul class="marks">
      {#if top10}
        <li>
          <span class="k">{$t('journal.behavior.concentration.top10', { count: top10.count })}</span>
          <span class="v num">{fmtPct(top10.share)}</span>
        </li>
      {/if}
      {#if top25}
        <li>
          <span class="k">{$t('journal.behavior.concentration.top25', { count: top25.count })}</span>
          <span class="v num">{fmtPct(top25.share)}</span>
        </li>
      {/if}
      <li>
        <span class="k">{$t('journal.behavior.concentration.grossWin')}</span>
        <span class="v num">{fmtMoney(gross, currency)}</span>
      </li>
    </ul>
  </div>
{/if}

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .plot {
    width: 100%;
    height: var(--h);
    display: block;
    border: 0.5px solid var(--border);
    background: var(--surface-2);
  }
  .ref {
    stroke: var(--border);
    stroke-width: 1;
    stroke-dasharray: 3 3;
  }
  .line {
    stroke: var(--accent);
    stroke-width: 1.5;
    stroke-linejoin: round;
  }
  .fill {
    fill: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .axis {
    display: flex;
    justify-content: space-between;
    font-size: 10.5px;
    color: var(--faint);
  }
  .marks {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-6);
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .marks li {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }
  .k {
    color: var(--dim);
  }
  .v {
    font-family: var(--mono);
    color: var(--text);
  }
</style>
