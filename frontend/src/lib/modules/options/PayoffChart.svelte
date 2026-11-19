<script>
  // P&L against the underlying's price: at expiry (solid) and at the chosen date (dashed),
  // one colour slot per strategy, the zero line, the spot and the breakevens of the first.
  import QChart from '$lib/modules/quant/QChart.svelte';
  import { valueAxis, tooltip, legend } from '$lib/modules/quant/charts.js';
  import { t } from '$lib/i18n';
  import { price, num } from './columns.js';

  /** `curves`: [{ name, payoff: [{ spot, at_expiry, at_date }], breakevens, slot?, path? }];
   * `slot` is the colour index, so a strategy keeps its colour when another one drops out;
   * `path` the curves between today and expiry, drawn for the first strategy. `front` names the
   * solid curve the front expiry when the legs do not all expire together. */
  let { curves = [], spot = null, height = 320, showDate = true, compact = false, front = false } = $props();

  const build = $derived.by(() => {
    const list = curves.filter((c) => c.payoff?.length);
    if (!list.length) return null;
    const expiryLabel = $t(front ? 'options.payoff.atFront' : 'options.payoff.atExpiry');
    const dateLabel = $t('options.payoff.atDate');
    return (p) => {
      const series = [];
      list.forEach((c, i) => {
        const color = p.series[(c.slot ?? i) % p.series.length];
        const tag = list.length > 1 ? `${c.name} · ` : '';
        series.push({
          name: `${tag}${expiryLabel}`,
          type: 'line',
          symbol: 'none',
          data: c.payoff.map((q) => [q.spot, q.at_expiry]),
          lineStyle: { color, width: 1.8 },
          itemStyle: { color },
          markLine:
            i === 0
              ? {
                  silent: true,
                  symbol: 'none',
                  label: { color: p.muted, fontSize: 10, fontFamily: p.mono },
                  data: [
                    { yAxis: 0, lineStyle: { color: p.border, width: 1, type: 'solid' }, label: { show: false } },
                    ...(spot != null ? [{ xAxis: spot, lineStyle: { color: p.accent, width: 1, type: 'solid' }, label: { formatter: `${$t('options.payoff.spot')} ${price(spot)}`, position: 'insideEndTop' } }] : []),
                    ...(c.breakevens ?? []).map((b) => ({ xAxis: b, lineStyle: { color: p.amber, width: 1, type: 'dotted' }, label: { formatter: `BE ${price(b)}`, position: 'insideStartTop' } }))
                  ]
                }
              : undefined
        });
        if (showDate && c.payoff.some((q) => q.at_date != null)) {
          series.push({
            name: `${tag}${dateLabel}`,
            type: 'line',
            symbol: 'none',
            data: c.payoff.map((q) => [q.spot, q.at_date]),
            lineStyle: { color, width: 1.2, type: 'dashed' },
            itemStyle: { color }
          });
        }
        if (showDate && i === 0) {
          (c.path ?? []).forEach((step, n, all) => {
            series.push({
              name: step.date,
              type: 'line',
              symbol: 'none',
              data: c.payoff.map((q, j) => [q.spot, step.pnl[j]]),
              lineStyle: { color, width: 1, type: 'dotted', opacity: 0.9 - (0.5 * n) / all.length },
              itemStyle: { color }
            });
          });
        }
      });
      return {
        animation: false,
        legend: compact ? undefined : legend(p, { type: 'scroll', left: 0 }),
        grid: { left: 56, right: 16, top: compact ? 12 : 32, bottom: 28 },
        tooltip: tooltip(p, {
          valueFormatter: (v) => price(v),
          axisPointer: { type: 'line' }
        }),
        xAxis: valueAxis(p, {
          scale: true,
          axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: (v) => num(v, 0) },
          axisPointer: { label: { formatter: ({ value }) => price(value) } }
        }),
        yAxis: valueAxis(p, { scale: true, axisLabel: { color: p.dim, fontFamily: p.mono, fontSize: 10, formatter: (v) => num(v, 0) } }),
        series
      };
    };
  });
</script>

<QChart {build} {height} />
