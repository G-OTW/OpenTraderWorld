<script>
  // What the candles said about the filtered trades.
  //
  // Everything on this tab needs bars, and bars are the one thing a trade log cannot
  // produce: MAE and MFE are where price went while the position was open, not where it
  // was when it closed. So the tab has two states and nothing in between. Measured, and
  // it answers the questions a journal alone never can:
  //
  //   was the stop sitting inside the noise
  //   was the target too close
  //   how many winners were given back
  //   how much of the move was actually kept
  //   when does the strategy work, in which volatility and which trend
  //
  // Unmeasured, it says so and points at the bar above rather than drawing zeros.
  import { fmtMoney, fmtSignedMoney, fmtPct, fmtNum } from './api.js';
  import DistChart from './DistChart.svelte';
  import GroupTable from './GroupTable.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let { market = null, currency = 'USD' } = $props();

  const money = (v) => (v == null ? '—' : fmtMoney(v, currency));
  const signed = (v) => (v == null ? '—' : fmtSignedMoney(v, currency));
  const pct = (v) => (v == null ? '—' : fmtPct(v));
  const num = (v) => (v == null ? '—' : fmtNum(v));
  const rr = (v) => (v == null ? '—' : `${fmtNum(v)}R`);
  const tone = (v) => (v == null ? '' : v > 0 ? 'pos' : v < 0 ? 'neg' : '');

  function dur(m) {
    if (m == null) return '—';
    if (m < 90) return $t('journal.behavior.unit.min', { n: Math.round(m) });
    if (m < 60 * 36) return $t('journal.behavior.unit.hour', { n: fmtNum(m / 60, 1) });
    return $t('journal.behavior.unit.day', { n: fmtNum(m / 1440, 1) });
  }

  const m = $derived(market);

  // ── Excursion tiles ──
  const tiles = $derived.by(() => {
    if (!m) return [];
    return [
      {
        label: $t('journal.market.stat.mae'),
        value: money(m.avg_mae),
        tone: -1,
        sub: $t('journal.market.stat.maeSub', { r: rr(m.avg_mae_r) })
      },
      {
        label: $t('journal.market.stat.mfe'),
        value: money(m.avg_mfe),
        tone: 1,
        sub: $t('journal.market.stat.mfeSub', { r: rr(m.avg_mfe_r) })
      },
      {
        label: $t('journal.market.stat.efficiency'),
        value: pct(m.avg_efficiency),
        sub: $t('journal.market.stat.efficiencySub', { median: pct(m.median_efficiency) })
      },
      {
        label: $t('journal.market.stat.giveback'),
        value: money(m.total_giveback),
        tone: -1,
        sub: $t('journal.market.stat.givebackSub')
      },
      {
        label: $t('journal.market.stat.timeToMae'),
        value: dur(m.median_time_to_mae),
        sub: $t('journal.market.stat.timeToMaeSub')
      },
      {
        label: $t('journal.market.stat.timeToMfe'),
        value: dur(m.median_time_to_mfe),
        sub: $t('journal.market.stat.timeToMfeSub')
      }
    ];
  });

  const maeLabel = (b) => $t(`journal.market.maeBucket.${b.key}`);
  const regimeLabel = (row) => $t(`journal.market.regime.${row.key}`);
  const trendLabel = (row) => $t(`journal.market.trend.${row.key}`);

  // Share of winners that nearly reached their own stop, and of winners that kept less
  // than half the move. Both are the honest denominators, not the whole book.
  const nearStopPct = $derived(
    m && m.winners_with_stop > 0 ? (m.near_stop_winners / m.winners_with_stop) * 100 : null
  );
  const shortTargetPct = $derived(
    m && m.winners_measured > 0 ? (m.short_targets / m.winners_measured) * 100 : null
  );
  const tightPct = $derived(
    m && m.stops_with_atr > 0 ? (m.tight_stops / m.stops_with_atr) * 100 : null
  );
</script>

{#if !m}
  <EmptyState
    icon="candlestick"
    title={$t('journal.market.empty.title')}
    description={$t('journal.market.empty.description')}
  />
{:else}
  <div class="market">
    <div class="note">
      <Icon name="candlestick" size={13} />
      {$t('journal.market.measuredOn', {
        measured: m.measured,
        closed: m.closed,
        timeframe: m.timeframe
      })}
    </div>

    <section class="card">
      <h3>
        {$t('journal.market.excursions.title')}
        <span class="cur">{$t('journal.market.excursions.hint')}</span>
      </h3>
      <div class="grid">
        {#each tiles as s (s.label)}
          <div class="stat">
            <span class="stat-label">{s.label}</span>
            <span class="stat-value num {tone(s.tone)}" class:is-empty={s.value === '—'}>
              {s.value}
            </span>
            {#if s.sub}<span class="stat-sub">{s.sub}</span>{/if}
          </div>
        {/each}
      </div>
    </section>

    <div class="two">
      <section class="card">
        <h3>
          {$t('journal.market.stops.title')}
          <span class="cur">{$t('journal.market.stops.hint')}</span>
        </h3>
        {#if m.stops_with_atr === 0 && m.winners_with_stop === 0}
          <EmptyState
            compact
            icon="target"
            title={$t('journal.market.stops.noneTitle')}
            description={$t('journal.market.stops.noneHint')}
          />
        {:else}
          <p class="sentence">
            {$t('journal.market.stops.sentence', {
              near: m.near_stop_winners,
              winners: m.winners_with_stop,
              hits: m.stop_hits
            })}
          </p>
          <div class="mgrid">
            <div class="stat">
              <span class="stat-label">{$t('journal.market.stops.distance')}</span>
              <span class="stat-value num">{m.median_stop_atr == null ? '—' : `${fmtNum(m.median_stop_atr)}×`}</span>
              <span class="stat-sub">{$t('journal.market.stops.distanceSub')}</span>
            </div>
            <div class="stat">
              <span class="stat-label">{$t('journal.market.stops.tight')}</span>
              <span class="stat-value num" class:neg={(tightPct ?? 0) > 30}>{pct(tightPct)}</span>
              <span class="stat-sub">
                {$t('journal.market.stops.tightSub', { count: m.tight_stops, total: m.stops_with_atr })}
              </span>
            </div>
            <div class="stat">
              <span class="stat-label">{$t('journal.market.stops.nearWinners')}</span>
              <span class="stat-value num" class:neg={(nearStopPct ?? 0) > 25}>{pct(nearStopPct)}</span>
              <span class="stat-sub">
                {$t('journal.market.stops.nearWinnersSub', {
                  count: m.near_stop_winners,
                  total: m.winners_with_stop
                })}
              </span>
            </div>
          </div>
        {/if}
      </section>

      <section class="card">
        <h3>
          {$t('journal.market.targets.title')}
          <span class="cur">{$t('journal.market.targets.hint')}</span>
        </h3>
        <p class="sentence">
          {$t('journal.market.targets.sentence', {
            short: m.short_targets,
            winners: m.winners_measured,
            efficiency: pct(m.median_efficiency)
          })}
        </p>
        <div class="mgrid">
          <div class="stat">
            <span class="stat-label">{$t('journal.market.targets.short')}</span>
            <span class="stat-value num" class:neg={(shortTargetPct ?? 0) > 40}>
              {pct(shortTargetPct)}
            </span>
            <span class="stat-sub">
              {$t('journal.market.targets.shortSub', {
                count: m.short_targets,
                total: m.winners_measured
              })}
            </span>
          </div>
          <div class="stat">
            <span class="stat-label">{$t('journal.market.targets.gaveBack')}</span>
            <span class="stat-value num" class:neg={m.gave_back > 0}>{m.gave_back}</span>
            <span class="stat-sub">
              {$t('journal.market.targets.gaveBackSub', {
                mfe: money(m.gave_back_mfe),
                net: signed(m.gave_back_net)
              })}
            </span>
          </div>
          <div class="stat">
            <span class="stat-label">{$t('journal.market.targets.left')}</span>
            <span class="stat-value num neg">{money(m.total_giveback)}</span>
            <span class="stat-sub">{$t('journal.market.targets.leftSub')}</span>
          </div>
        </div>
      </section>
    </div>

    <section class="card">
      <h3>
        {$t('journal.market.maeDist.title')}
        <span class="cur">{$t('journal.market.maeDist.hint')}</span>
      </h3>
      <DistChart buckets={m.mae_buckets} {currency} label={maeLabel} />
    </section>

    <div class="two">
      <section class="card">
        <h3>
          {$t('journal.market.regimes.title')}
          <span class="cur">{$t('journal.market.regimes.hint')}</span>
        </h3>
        <GroupTable
          rows={m.regimes.filter((r) => r.trades > 0)}
          {currency}
          nameOf={regimeLabel}
          emptyTitle={$t('journal.market.regimes.none')}
        />
      </section>
      <section class="card">
        <h3>
          {$t('journal.market.trends.title')}
          <span class="cur">{$t('journal.market.trends.hint')}</span>
        </h3>
        <GroupTable
          rows={m.trends.filter((r) => r.trades > 0)}
          {currency}
          nameOf={trendLabel}
          emptyTitle={$t('journal.market.trends.none')}
        />
      </section>
    </div>

    <p class="foot">{$t('journal.market.scatterHint')}</p>
  </div>
{/if}

<style>
  .market {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }
  .card {
    background: var(--surface);
    border: 0.5px solid var(--border);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .card h3 {
    font-size: 12.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.03em;
  }
  .cur {
    color: var(--dim);
    font-weight: var(--fw-normal);
    font-size: 11px;
  }
  .note {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    border: 0.5px solid var(--border);
    background: var(--surface);
    color: var(--muted);
    padding: var(--space-3) var(--space-4);
    font-size: var(--text-sm);
  }
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
    gap: var(--space-6);
  }
  .grid,
  .mgrid {
    display: grid;
    gap: 0.5px;
    background: var(--border);
    border: 0.5px solid var(--border);
  }
  .grid {
    grid-template-columns: repeat(3, 1fr);
  }
  .mgrid {
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  }
  .stat {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    background: var(--bg);
    padding: var(--pad-metric);
  }
  .stat-label {
    font-size: var(--fs-metric-label);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: var(--fw-normal);
    color: var(--dim);
  }
  .stat-value {
    font-family: var(--mono);
    font-size: var(--fs-metric-value);
    font-weight: var(--fw-normal);
    color: var(--text);
    line-height: var(--lh-tight);
  }
  .stat-sub {
    font-size: 10.5px;
    color: var(--faint);
    line-height: var(--lh-tight);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .stat-value.is-empty {
    color: var(--faint);
  }
  .sentence {
    font-size: var(--text-sm);
    color: var(--muted);
    line-height: var(--lh-base);
  }
  .foot {
    font-size: 11px;
    color: var(--faint);
  }
  @media (max-width: 860px) {
    .grid {
      grid-template-columns: repeat(2, 1fr);
    }
  }
</style>
