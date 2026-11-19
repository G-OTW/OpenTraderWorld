<script>
  // The behaviour tab: what the trader does around the edge, and what it costs.
  //
  // Five questions, one card each, in the order they are worth reading:
  //
  //   concentration : is the result a habit or a handful of trades
  //   sizing        : does the size grow after a losing run
  //   pace          : does the next trade come sooner after a loss, and does it pay
  //   session       : does the day get worse as it goes on
  //   after         : the after-a-win trader against the after-a-loss trader
  //
  // Everything is computed server-side over the same filtered trades as the rest of the
  // analytics screen (`analytics.behavior`); this file only reads and renders. The
  // insight strip at the top is the server's list of statements that cleared both a
  // sample floor and an effect threshold. The wording lives in the language packs, the
  // numbers come down raw so they format in the reader's locale and currency.
  import { fmtMoney, fmtSignedMoney, fmtPct, fmtNum } from './api.js';
  import DistChart from './DistChart.svelte';
  import LorenzChart from './LorenzChart.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { insightText, SEVERITY_ICON } from './insights.js';
  import { t } from '$lib/i18n';

  let { behavior, points = [], currency = 'USD' } = $props();

  const b = $derived(behavior);
  const money = (v) => (v == null ? '—' : fmtMoney(v, currency));
  const signed = (v) => (v == null ? '—' : fmtSignedMoney(v, currency));
  const pct = (v) => (v == null ? '—' : fmtPct(v));
  const signedPct = (v) => (v == null ? '—' : `${v > 0 ? '+' : ''}${fmtPct(v)}`);
  const num = (v) => (v == null ? '—' : fmtNum(v));
  const rr = (v) => (v == null ? '—' : `${v > 0 ? '+' : ''}${fmtNum(v)}R`);
  const int = (v) => (v == null ? '—' : String(Math.round(v)));

  // A gap or a hold is a duration, not a number of minutes: read it in the unit that
  // keeps it short.
  function dur(m) {
    if (m == null) return '—';
    if (m < 90) return $t('journal.behavior.unit.min', { n: Math.round(m) });
    if (m < 60 * 36) return $t('journal.behavior.unit.hour', { n: fmtNum(m / 60, 1) });
    return $t('journal.behavior.unit.day', { n: fmtNum(m / 1440, 1) });
  }

  const tone = (v) => (v == null ? '' : v > 0 ? 'pos' : v < 0 ? 'neg' : '');

  // ── Insights ──
  // Which formatter each statement's numbers take. The key set is closed (the server
  // owns it), so a table here beats guessing from the value.
  const INSIGHT_FMT = {
    fragileEdge: { net: signed, without: signed },
    concentrated: { share: pct, winners: int },
    sizeEscalation: { escalation: signedPct, trades: int, net: signed },
    sizeDiscipline: { trades: int },
    revenge: { trades: int, net: signed, expectancy: signed, normal: signed },
    overtrading: { loss: num, clean: num },
    sessionDecay: { first: signed, later: signed, trades: int },
    afterLossTilt: { afterWin: signed, afterLoss: signed, trades: int },
    afterLossSteady: { afterLoss: signed },
    regular: { gini: num }
  };
  const text = (i) =>
    insightText(i, $t, 'journal.behavior.insight', INSIGHT_FMT[i.key] ?? {}, num);

  // ── Concentration ──
  const conc = $derived(b.concentration);
  const concTiles = $derived([
    {
      label: $t('journal.behavior.concentration.top5'),
      value: pct(conc.top5_share),
      sub: $t('journal.behavior.concentration.top5Sub', { count: conc.winners })
    },
    {
      label: $t('journal.behavior.concentration.winsForHalf'),
      value: conc.wins_for_half == null ? '—' : String(conc.wins_for_half),
      sub: $t('journal.behavior.concentration.winsForHalfSub', { gross: money(conc.gross_win) })
    },
    {
      label: $t('journal.behavior.concentration.exTop5'),
      value: signed(conc.net_ex_top5),
      tone: conc.net_ex_top5,
      sub: $t('journal.behavior.concentration.exTop5Sub', { net: signed(conc.net) })
    },
    {
      label: $t('journal.behavior.concentration.gini'),
      value: num(conc.gini),
      sub: $t('journal.behavior.concentration.giniSub')
    },
    {
      label: $t('journal.behavior.concentration.skew'),
      value: conc.win_skew == null ? '—' : `${fmtNum(conc.win_skew)}×`,
      sub: $t('journal.behavior.concentration.skewSub', {
        avg: money(conc.avg_win),
        median: money(conc.median_win)
      })
    },
    {
      label: $t('journal.behavior.concentration.worst5'),
      value: pct(conc.worst5_share),
      tone: -1,
      sub: $t('journal.behavior.concentration.worst5Sub', { count: conc.losers })
    }
  ]);

  // ── Sizing after losses ──
  const sizing = $derived(b.sizing);
  const streakLabel = (k) => $t(`journal.behavior.sizing.streak.${k}`);

  // ── Pace ──
  const over = $derived(b.overtrading);
  const paceTiles = $derived([
    {
      label: $t('journal.behavior.pace.median'),
      value: dur(over.median_gap_min),
      sub: $t('journal.behavior.pace.medianSub', { count: over.pairs })
    },
    {
      label: $t('journal.behavior.pace.afterWin'),
      value: dur(over.gap_after_win_min),
      sub: $t('journal.behavior.pace.afterWinSub')
    },
    {
      label: $t('journal.behavior.pace.afterLoss'),
      value: dur(over.gap_after_loss_min),
      sub: $t('journal.behavior.pace.afterLossSub')
    },
    {
      label: $t('journal.behavior.pace.speedup'),
      value: over.speedup_pct == null ? '—' : signedPct(over.speedup_pct),
      // Faster after a loss is the warning sign, so a positive speed-up reads red.
      tone: over.speedup_pct == null ? null : -over.speedup_pct,
      sub: $t('journal.behavior.pace.speedupSub')
    }
  ]);

  // ── Session ──
  const session = $derived(b.session);
  const rankLabel = (bk) => $t(`journal.behavior.session.rank.${bk.key}`);

  // ── After a win vs after a loss ──
  const after = $derived(b.after);
  const AFTER_ROWS = $derived([
    { key: 'trades', label: $t('journal.behavior.after.trades'), fmt: (v) => String(v) },
    { key: 'win_rate', label: $t('journal.behavior.after.winRate'), fmt: pct },
    { key: 'expectancy', label: $t('journal.behavior.after.expectancy'), fmt: signed, toned: true },
    { key: 'avg_r', label: $t('journal.behavior.after.avgR'), fmt: rr, toned: true },
    { key: 'median_size', label: $t('journal.behavior.after.size'), fmt: money },
    { key: 'avg_risk_pct', label: $t('journal.behavior.after.risk'), fmt: pct },
    { key: 'median_hold_min', label: $t('journal.behavior.after.hold'), fmt: dur },
    { key: 'median_gap_min', label: $t('journal.behavior.after.gap'), fmt: dur }
  ]);
</script>

<div class="behavior">
  {#if b.sample < b.min_sample}
    <div class="note">
      <Icon name="lightbulb" size={13} />
      {$t('journal.behavior.thin', { count: b.sample, min: b.min_sample })}
    </div>
  {:else if b.insights.length === 0}
    <div class="note">
      <Icon name="check-circle" size={13} />
      {$t('journal.behavior.clean')}
    </div>
  {:else}
    <ul class="insights">
      {#each b.insights as i (i.key)}
        <li class="insight {i.severity}">
          <Icon name={SEVERITY_ICON[i.severity] ?? 'lightbulb'} size={14} />
          <span>{text(i)}</span>
        </li>
      {/each}
    </ul>
  {/if}

  <section class="card">
    <h3>
      {$t('journal.behavior.concentration.title')}
      <span class="cur">{$t('journal.behavior.concentration.hint')}</span>
    </h3>
    <div class="grid">
      {#each concTiles as s (s.label)}
        <div class="stat">
          <span class="stat-label">{s.label}</span>
          <span class="stat-value num {tone(s.tone)}" class:is-empty={s.value === '—'}>
            {s.value}
          </span>
          {#if s.sub}<span class="stat-sub">{s.sub}</span>{/if}
        </div>
      {/each}
    </div>
    <LorenzChart {points} {currency} />
  </section>

  <section class="card">
    <h3>
      {$t('journal.behavior.sizing.title')}
      <span class="cur">{$t('journal.behavior.sizing.hint')}</span>
    </h3>
    {#if sizing.escalation_pct != null}
      <p class="sentence">
        {$t('journal.behavior.sizing.sentence', {
          escalation: signedPct(sizing.escalation_pct),
          trades: sizing.after_streak_trades,
          net: signed(sizing.after_streak_net),
          expectancy: signed(sizing.after_streak_expectancy),
          baseline: signed(sizing.baseline_expectancy)
        })}
      </p>
    {/if}
    <table class="tbl">
      <thead>
        <tr>
          <th>{$t('journal.behavior.sizing.after')}</th>
          <th class="num">{$t('journal.behavior.sizing.trades')}</th>
          <th class="num">{$t('journal.behavior.sizing.medianSize')}</th>
          <th class="num">{$t('journal.behavior.sizing.risk')}</th>
          <th class="num">{$t('journal.behavior.sizing.winRate')}</th>
          <th class="num">{$t('journal.behavior.sizing.expectancy')}</th>
          <th class="num">{$t('journal.behavior.sizing.avgR')}</th>
          <th class="num">{$t('journal.behavior.sizing.net')}</th>
        </tr>
      </thead>
      <tbody>
        {#each sizing.rows as r (r.key)}
          <tr class:muted={r.trades === 0}>
            <td>{streakLabel(r.key)}</td>
            <td class="num">{r.trades}</td>
            <td class="num">{money(r.median_size)}</td>
            <td class="num">{pct(r.avg_risk_pct)}</td>
            <td class="num">{pct(r.win_rate)}</td>
            <td class="num {tone(r.expectancy)}">{signed(r.expectancy)}</td>
            <td class="num {tone(r.avg_r)}">{rr(r.avg_r)}</td>
            <td class="num {tone(r.net)}">{signed(r.net)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>

  <section class="card">
    <h3>
      {$t('journal.behavior.pace.title')}
      <span class="cur">{$t('journal.behavior.pace.hint')}</span>
    </h3>
    {#if over.pairs === 0}
      <EmptyState
        compact
        icon="clock"
        title={$t('journal.behavior.pace.noneTitle')}
        description={$t('journal.behavior.pace.noneHint')}
      />
    {:else}
      <div class="grid four">
        {#each paceTiles as s (s.label)}
          <div class="stat">
            <span class="stat-label">{s.label}</span>
            <span class="stat-value num {tone(s.tone)}" class:is-empty={s.value === '—'}>
              {s.value}
            </span>
            {#if s.sub}<span class="stat-sub">{s.sub}</span>{/if}
          </div>
        {/each}
      </div>
      <div class="split">
        <div class="box" class:warn={over.revenge_trades > 0}>
          <h4>
            {$t('journal.behavior.pace.revenge')}
            <span class="cur">
              {$t('journal.behavior.pace.revengeHint', {
                threshold: dur(over.revenge_threshold_min)
              })}
            </span>
          </h4>
          {#if over.revenge_trades === 0}
            <p class="sentence">{$t('journal.behavior.pace.revengeNone')}</p>
          {:else}
            <p class="sentence">
              {$t('journal.behavior.pace.revengeSentence', {
                trades: over.revenge_trades,
                net: signed(over.revenge_net),
                expectancy: signed(over.revenge_expectancy),
                normal: signed(over.normal_expectancy)
              })}
            </p>
            <div class="pair">
              <div>
                <span class="stat-label">{$t('journal.behavior.pace.revengeWinRate')}</span>
                <span class="stat-value num">{pct(over.revenge_win_rate)}</span>
              </div>
              <div>
                <span class="stat-label">{$t('journal.behavior.pace.normalWinRate')}</span>
                <span class="stat-value num">{pct(over.normal_win_rate)}</span>
              </div>
            </div>
          {/if}
        </div>
        <div class="box">
          <h4>
            {$t('journal.behavior.pace.perDay')}
            <span class="cur">{$t('journal.behavior.pace.perDayHint')}</span>
          </h4>
          <div class="pair">
            <div>
              <span class="stat-label">
                {$t('journal.behavior.pace.lossDays', { count: over.loss_days })}
              </span>
              <span class="stat-value num">{num(over.avg_trades_loss_day)}</span>
            </div>
            <div>
              <span class="stat-label">
                {$t('journal.behavior.pace.cleanDays', { count: over.clean_days })}
              </span>
              <span class="stat-value num">{num(over.avg_trades_clean_day)}</span>
            </div>
          </div>
        </div>
      </div>
    {/if}
  </section>

  <section class="card">
    <h3>
      {$t('journal.behavior.session.title')}
      <span class="cur">{$t('journal.behavior.session.hint')}</span>
    </h3>
    {#if session.multi_trade_days === 0}
      <EmptyState
        compact
        icon="timeline"
        title={$t('journal.behavior.session.noneTitle')}
        description={$t('journal.behavior.session.noneHint')}
      />
    {:else}
      <DistChart buckets={session.by_rank} {currency} label={rankLabel} metric="avg" />
      <!-- Same ranking in units of risk: money says how much, R says whether the later
           trades were worth what they risked. -->
      <ul class="rstrip">
        <li class="k">{$t('journal.behavior.session.rStrip')}</li>
        {#each session.by_rank as bk, i (bk.key)}
          {#if bk.trades > 0}
            <li>
              <span class="k">{rankLabel(bk)}</span>
              <span class="v num {tone(session.avg_r[i])}">{rr(session.avg_r[i])}</span>
            </li>
          {/if}
        {/each}
      </ul>
      <div class="grid four decay">
        <div class="stat">
          <span class="stat-label">{$t('journal.behavior.session.first')}</span>
          <span class="stat-value num {tone(session.first_expectancy)}">
            {signed(session.first_expectancy)}
          </span>
          <span class="stat-sub">
            {$t('journal.behavior.session.firstSub', {
              count: session.first_trades,
              rate: pct(session.first_win_rate)
            })}
          </span>
        </div>
        <div class="stat">
          <span class="stat-label">{$t('journal.behavior.session.later')}</span>
          <span class="stat-value num {tone(session.later_expectancy)}">
            {signed(session.later_expectancy)}
          </span>
          <span class="stat-sub">
            {$t('journal.behavior.session.laterSub', {
              count: session.later_trades,
              rate: pct(session.later_win_rate)
            })}
          </span>
        </div>
        <div class="stat">
          <span class="stat-label">{$t('journal.behavior.session.slope')}</span>
          <span class="stat-value num {tone(session.slope)}">{signed(session.slope)}</span>
          <span class="stat-sub">{$t('journal.behavior.session.slopeSub')}</span>
        </div>
        <div class="stat">
          <span class="stat-label">{$t('journal.behavior.session.perDay')}</span>
          <span class="stat-value num">{num(session.median_trades_day)}</span>
          <span class="stat-sub">
            {$t('journal.behavior.session.perDaySub', {
              days: session.multi_trade_days,
              max: session.max_trades_day
            })}
          </span>
        </div>
      </div>
    {/if}
  </section>

  <section class="card">
    <h3>
      {$t('journal.behavior.after.title')}
      <span class="cur">{$t('journal.behavior.after.hint')}</span>
    </h3>
    {#if after.after_win.trades === 0 && after.after_loss.trades === 0}
      <EmptyState
        compact
        icon="repeat"
        title={$t('journal.behavior.after.noneTitle')}
        description={$t('journal.behavior.after.noneHint')}
      />
    {:else}
      <table class="tbl compare">
        <thead>
          <tr>
            <th></th>
            <th class="num">{$t('journal.behavior.after.afterWin')}</th>
            <th class="num">{$t('journal.behavior.after.afterLoss')}</th>
          </tr>
        </thead>
        <tbody>
          {#each AFTER_ROWS as row (row.key)}
            <tr>
              <td>{row.label}</td>
              <td class="num {row.toned ? tone(after.after_win[row.key]) : ''}">
                {after.after_win[row.key] == null ? '—' : row.fmt(after.after_win[row.key])}
              </td>
              <td class="num {row.toned ? tone(after.after_loss[row.key]) : ''}">
                {after.after_loss[row.key] == null ? '—' : row.fmt(after.after_loss[row.key])}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      <div class="grid three">
        <div class="stat">
          <span class="stat-label">{$t('journal.behavior.after.expectancyDelta')}</span>
          <span class="stat-value num {tone(after.expectancy_delta)}">
            {signed(after.expectancy_delta)}
          </span>
          <span class="stat-sub">{$t('journal.behavior.after.expectancyDeltaSub')}</span>
        </div>
        <div class="stat">
          <span class="stat-label">{$t('journal.behavior.after.sizeDelta')}</span>
          <span class="stat-value num {tone(after.size_delta_pct == null ? null : -after.size_delta_pct)}">
            {signedPct(after.size_delta_pct)}
          </span>
          <span class="stat-sub">{$t('journal.behavior.after.sizeDeltaSub')}</span>
        </div>
        <div class="stat">
          <span class="stat-label">{$t('journal.behavior.after.holdDelta')}</span>
          <span class="stat-value num">{signedPct(after.hold_delta_pct)}</span>
          <span class="stat-sub">{$t('journal.behavior.after.holdDeltaSub')}</span>
        </div>
      </div>
    {/if}
  </section>
</div>

<style>
  .behavior {
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
  .card h4 {
    font-size: 11.5px;
    font-weight: var(--fw-medium);
    margin-bottom: var(--space-2);
  }
  .cur {
    color: var(--dim);
    font-weight: var(--fw-normal);
    font-size: 11px;
  }
  /* Insight strip: one statement per line, coloured by what it is worth acting on. */
  .insights {
    display: flex;
    flex-direction: column;
    gap: 0.5px;
    background: var(--border);
    border: 0.5px solid var(--border);
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .insight {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    background: var(--bg);
    padding: var(--space-3) var(--space-4);
    font-size: var(--text-sm);
    line-height: var(--lh-base);
    color: var(--text);
  }
  .insight :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .insight.warn {
    background: color-mix(in srgb, var(--amber) 10%, var(--bg));
    color: var(--text);
  }
  .insight.warn :global(svg) {
    color: var(--amber);
  }
  .insight.good :global(svg) {
    color: var(--green);
  }
  .insight.info :global(svg) {
    color: var(--accent);
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
  /* Continuous filet grid, same shape as the overview's stat grid. */
  .grid {
    display: grid;
    gap: 0.5px;
    background: var(--border);
    border: 0.5px solid var(--border);
    grid-template-columns: repeat(3, 1fr);
  }
  .grid.four {
    grid-template-columns: repeat(4, 1fr);
  }
  .grid.three {
    grid-template-columns: repeat(3, 1fr);
  }
  .stat {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    background: var(--bg);
    padding: var(--pad-metric);
  }
  .stat-label {
    display: block;
    font-size: var(--fs-metric-label);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: var(--fw-normal);
    color: var(--dim);
  }
  .stat-value {
    display: block;
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
  .split {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: var(--space-4);
  }
  .box {
    border: 0.5px solid var(--border);
    padding: var(--space-4);
  }
  .box.warn {
    border-color: color-mix(in srgb, var(--amber) 45%, transparent);
  }
  .pair {
    display: flex;
    gap: var(--space-8);
    margin-top: var(--space-3);
  }
  /* Per-rank R, read as one line under the money chart it qualifies. */
  .rstrip {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-1) var(--space-4);
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: var(--text-xs);
  }
  .rstrip li {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .rstrip .k {
    color: var(--dim);
  }
  .rstrip .v {
    font-family: var(--mono);
    color: var(--text);
  }
  tr.muted td {
    color: var(--faint);
  }
  .compare td:first-child {
    color: var(--muted);
  }
  @media (max-width: 860px) {
    .grid,
    .grid.four,
    .grid.three {
      grid-template-columns: repeat(2, 1fr);
    }
  }
</style>
