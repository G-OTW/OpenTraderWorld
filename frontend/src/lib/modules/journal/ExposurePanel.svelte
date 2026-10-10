<script>
  // Open-position risk: the only tab about the present.
  //
  // Everything else in the analytics screen measures what already happened. This one
  // answers what is still on the line, and it answers it in the order a risk read goes:
  //
  //   totals        how much is open, and how much of the account is at stake
  //   positions     line by line, with what each one costs if its stop is hit
  //   concentration where that risk actually sits
  //   correlation   whether those lines are separate bets or one bet in disguise
  //
  // Closed trades are deliberately absent: they carry no exposure. The correlation and
  // the marks come from bars already in store, so opening this tab fetches nothing.
  import { journalApi, fmtMoney, fmtSignedMoney, fmtPct, fmtNum, ASSET_CLASSES } from './api.js';
  import { insightText, SEVERITY_ICON } from './insights.js';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { fmtDate } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let { filter = {}, displayCurrency = 'USD' } = $props();

  let ex = $state.raw(null);
  let loading = $state(true);

  $effect(() => {
    const f = filter;
    loading = true;
    journalApi
      .exposure(f)
      .then((d) => {
        ex = d;
      })
      .finally(() => {
        loading = false;
      });
  });

  const money = (v) => (v == null ? '—' : fmtMoney(v, displayCurrency));
  const signed = (v) => (v == null ? '—' : fmtSignedMoney(v, displayCurrency));
  const pct = (v) => (v == null ? '—' : fmtPct(v));
  const num = (v) => (v == null ? '—' : fmtNum(v));
  const tone = (v) => (v == null ? '' : v > 0 ? 'pos' : v < 0 ? 'neg' : '');

  const INSIGHT_FMT = {
    noStop: { count: String, total: String },
    concentratedTicker: { share: pct, risk: money },
    concentratedClass: { share: pct },
    thinBook: { effective: num, positions: String },
    correlatedPair: { corr: num },
    stacked: { stacking: num, correlated: money, independent: money },
    diversified: { avg: num }
  };
  const text = (i) => insightText(i, $t, 'journal.exposure.warning', INSIGHT_FMT[i.key] ?? {}, num);

  const assetLabel = (id) => ASSET_CLASSES.find((a) => a.id === id)?.label ?? id;
  const sideLabel = (id) => $t(`journal.side.${id}`) ?? id;

  const tiles = $derived.by(() => {
    if (!ex) return [];
    const c = ex.correlation;
    return [
      {
        label: $t('journal.exposure.stat.open'),
        value: String(ex.open_count),
        sub: $t('journal.exposure.stat.openSub', {
          stop: ex.with_stop,
          nostop: ex.without_stop
        })
      },
      {
        label: $t('journal.exposure.stat.gross'),
        value: money(ex.gross_notional),
        sub: $t('journal.exposure.stat.grossSub', { pct: pct(ex.gross_pct) })
      },
      {
        label: $t('journal.exposure.stat.net'),
        value: money(ex.net_notional),
        sub: $t('journal.exposure.stat.netSub', {
          long: money(ex.long_notional),
          short: money(ex.short_notional)
        })
      },
      {
        label: $t('journal.exposure.stat.risk'),
        value: money(ex.total_risk),
        tone: -1,
        sub: $t('journal.exposure.stat.riskSub', { pct: pct(ex.total_risk_pct) })
      },
      {
        label: $t('journal.exposure.stat.unrealized'),
        value: ex.marked === 0 ? '—' : signed(ex.unrealized),
        tone: ex.marked === 0 ? null : ex.unrealized,
        sub: $t('journal.exposure.stat.unrealizedSub', {
          marked: ex.marked,
          total: ex.open_count
        })
      },
      {
        label: $t('journal.exposure.stat.effective'),
        value: num(c?.effective_bets ?? ex.effective_positions),
        sub: c?.effective_bets != null
          ? $t('journal.exposure.stat.effectiveSubCorr', { count: ex.open_count })
          : $t('journal.exposure.stat.effectiveSub', { count: ex.open_count })
      }
    ];
  });

  const GROUPS = $derived(
    !ex
      ? []
      : [
          { key: 'ticker', label: $t('journal.exposure.by.ticker'), rows: ex.by_ticker, name: (r) => r.name },
          {
            key: 'assetClass',
            label: $t('journal.exposure.by.assetClass'),
            rows: ex.by_asset_class,
            name: (r) => assetLabel(r.key)
          },
          { key: 'side', label: $t('journal.exposure.by.side'), rows: ex.by_side, name: (r) => sideLabel(r.key) },
          {
            key: 'strategy',
            label: $t('journal.exposure.by.strategy'),
            rows: ex.by_strategy,
            name: (r) => r.name || $t('journal.analytics.group.unassigned')
          }
        ]
  );

  // Correlation cell colour: red for moving together, green for hedging each other.
  // The alpha carries the magnitude, so a weak pair stays quiet.
  function cellStyle(v) {
    if (v == null) return '';
    const a = Math.min(1, Math.abs(v)) * 0.55;
    const c = v >= 0 ? 'var(--red)' : 'var(--green)';
    return `background: color-mix(in srgb, ${c} ${(a * 100).toFixed(0)}%, transparent)`;
  }
</script>

{#if loading && !ex}
  <Skeleton height="360px" />
{:else if ex && ex.open_count === 0}
  <EmptyState
    icon="briefcase"
    title={$t('journal.exposure.empty.title')}
    description={$t('journal.exposure.empty.description')}
  />
{:else if ex}
  <div class="exposure">
    {#if ex.unconverted > 0}
      <div class="note warn">
        <Icon name="alert-triangle" size={13} />
        {$t('journal.exposure.unconverted', {
          count: ex.unconverted,
          currency: ex.display_currency
        })}
      </div>
    {/if}

    {#if ex.warnings.length > 0}
      <ul class="insights">
        {#each ex.warnings as w (w.key)}
          <li class="insight {w.severity}">
            <Icon name={SEVERITY_ICON[w.severity] ?? 'lightbulb'} size={14} />
            <span>{text(w)}</span>
          </li>
        {/each}
      </ul>
    {/if}

    <section class="grid">
      {#each tiles as s (s.label)}
        <div class="stat">
          <span class="stat-label">{s.label}</span>
          <span class="stat-value num {tone(s.tone)}" class:is-empty={s.value === '—'}>
            {s.value}
          </span>
          {#if s.sub}<span class="stat-sub">{s.sub}</span>{/if}
        </div>
      {/each}
    </section>

    <section class="card">
      <h3>
        {$t('journal.exposure.positions.title')}
        <span class="cur">{$t('journal.exposure.positions.hint')}</span>
      </h3>
      <div class="scroll">
        <table class="tbl">
          <thead>
            <tr>
              <th>{$t('journal.exposure.col.instrument')}</th>
              <th>{$t('journal.exposure.col.side')}</th>
              <th class="num">{$t('journal.exposure.col.qty')}</th>
              <th class="num">{$t('journal.exposure.col.entry')}</th>
              <th class="num">{$t('journal.exposure.col.stop')}</th>
              <th class="num">{$t('journal.exposure.col.last')}</th>
              <th class="num">{$t('journal.exposure.col.notional')}</th>
              <th class="num">{$t('journal.exposure.col.risk')}</th>
              <th class="num">{$t('journal.exposure.col.riskShare')}</th>
              <th class="num">{$t('journal.exposure.col.unrealized')}</th>
              <th class="num">{$t('journal.exposure.col.days')}</th>
            </tr>
          </thead>
          <tbody>
            {#each ex.positions as p (p.id)}
              <tr>
                <td>
                  <span class="sym">{p.ticker}</span>
                  <span class="cur">{assetLabel(p.asset_class)}</span>
                </td>
                <td class:neg={p.side === 'short'}>{sideLabel(p.side)}</td>
                <td class="num">{num(p.open_qty)}</td>
                <td class="num">{num(p.avg_entry)}</td>
                <td class="num" class:is-empty={p.stop_price == null}>
                  {p.stop_price == null ? $t('journal.exposure.noStopCell') : num(p.stop_price)}
                </td>
                <td class="num">
                  {num(p.last_price)}
                  {#if p.last_at}<span class="cur">{fmtDate(p.last_at)}</span>{/if}
                </td>
                <td class="num">{money(p.notional)}</td>
                <td class="num">
                  {money(p.risk)}
                  {#if p.risk_pct != null}<span class="cur">{pct(p.risk_pct)}</span>{/if}
                </td>
                <td class="num share">
                  {#if p.risk_share != null}
                    <span class="bar" style="width:{Math.min(100, p.risk_share)}%"></span>
                    <span class="val">{pct(p.risk_share)}</span>
                  {:else}
                    <span class="val is-empty">—</span>
                  {/if}
                </td>
                <td class="num {tone(p.unrealized)}">{signed(p.unrealized)}</td>
                <td class="num">{p.days_open ?? '—'}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>

    <section class="card">
      <h3>
        {$t('journal.exposure.concentration.title')}
        <span class="cur">
          {ex.total_risk > 0
            ? $t('journal.exposure.concentration.hintRisk')
            : $t('journal.exposure.concentration.hintSize')}
        </span>
      </h3>
      <div class="two">
        {#each GROUPS as g (g.key)}
          {#if g.rows.length > 0}
            <div class="box">
              <h4>{g.label}</h4>
              <table class="tbl">
                <thead>
                  <tr>
                    <th>{$t('journal.exposure.col.group')}</th>
                    <th class="num">{$t('journal.exposure.col.positions')}</th>
                    <th class="num">{$t('journal.exposure.col.notional')}</th>
                    <th class="num">{$t('journal.exposure.col.share')}</th>
                    <th class="num">{$t('journal.exposure.col.risk')}</th>
                  </tr>
                </thead>
                <tbody>
                  {#each g.rows as r (r.key)}
                    <tr>
                      <td>{g.name(r)}</td>
                      <td class="num">{r.positions}</td>
                      <td class="num">{money(r.notional)}</td>
                      <td class="num share">
                        <span
                          class="bar"
                          style="width:{Math.min(100, r.risk_share ?? r.notional_share)}%"
                        ></span>
                        <span class="val">{pct(r.risk_share ?? r.notional_share)}</span>
                      </td>
                      <td class="num">{money(r.risk)}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}
        {/each}
      </div>
    </section>

    <section class="card">
      <h3>
        {$t('journal.exposure.correlation.title')}
        <span class="cur">{$t('journal.exposure.correlation.hint')}</span>
      </h3>
      {#if !ex.correlation}
        <EmptyState
          compact
          icon="share-2"
          title={$t('journal.exposure.correlation.noneTitle')}
          description={$t('journal.exposure.correlation.noneHint')}
        />
      {:else}
        {@const c = ex.correlation}
        <p class="sentence">
          {$t('journal.exposure.correlation.sentence', {
            count: c.labels.length,
            observations: c.observations,
            timeframe: c.timeframe,
            avg: num(c.avg_corr)
          })}
        </p>
        <div class="mgrid">
          <div class="stat">
            <span class="stat-label">{$t('journal.exposure.correlation.sum')}</span>
            <span class="stat-value num">{money(c.risk_sum)}</span>
            <span class="stat-sub">{$t('journal.exposure.correlation.sumSub')}</span>
          </div>
          <div class="stat">
            <span class="stat-label">{$t('journal.exposure.correlation.independent')}</span>
            <span class="stat-value num">{money(c.risk_independent)}</span>
            <span class="stat-sub">{$t('journal.exposure.correlation.independentSub')}</span>
          </div>
          <div class="stat">
            <span class="stat-label">{$t('journal.exposure.correlation.correlated')}</span>
            <span class="stat-value num neg">{money(c.risk_correlated)}</span>
            <span class="stat-sub">{$t('journal.exposure.correlation.correlatedSub')}</span>
          </div>
          <div class="stat">
            <span class="stat-label">{$t('journal.exposure.correlation.stacking')}</span>
            <span class="stat-value num" class:neg={(c.stacking ?? 0) >= 1.3}>
              {c.stacking == null ? '—' : `${fmtNum(c.stacking)}×`}
            </span>
            <span class="stat-sub">{$t('journal.exposure.correlation.stackingSub')}</span>
          </div>
        </div>

        <div class="scroll">
          <table class="matrix">
            <thead>
              <tr>
                <th></th>
                {#each c.labels as l (l)}<th>{l}</th>{/each}
              </tr>
            </thead>
            <tbody>
              {#each c.labels as row, i (row)}
                <tr>
                  <th>{row}</th>
                  {#each c.labels as col, j (col)}
                    <td style={i === j ? '' : cellStyle(c.matrix[i][j])}>
                      {fmtNum(c.matrix[i][j])}
                    </td>
                  {/each}
                </tr>
              {/each}
            </tbody>
          </table>
        </div>

        {#if c.uncovered.length > 0}
          <p class="foot">
            {$t('journal.exposure.correlation.uncovered', { tickers: c.uncovered.join(', ') })}
          </p>
        {/if}
      {/if}
    </section>
  </div>
{/if}

<style>
  .exposure {
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
    margin-left: var(--space-2);
  }
  .note {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    border: 0.5px solid var(--border);
    padding: var(--space-3) var(--space-4);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .note.warn {
    background: color-mix(in srgb, var(--amber) 14%, transparent);
    border-color: color-mix(in srgb, var(--amber) 45%, transparent);
    color: var(--text);
  }
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
  }
  .insight :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .insight.warn {
    background: color-mix(in srgb, var(--amber) 10%, var(--bg));
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
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
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
    color: var(--dim);
  }
  .stat-value {
    font-family: var(--mono);
    font-size: var(--fs-metric-value);
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
  .is-empty,
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
  /* A wide table scrolls inside its own card, never the page. */
  .scroll {
    overflow-x: auto;
  }
  .sym {
    font-family: var(--mono);
  }
  /* Share cell: the bar sits behind the number, anchored right like its column. */
  td.share {
    position: relative;
  }
  .share .bar {
    position: absolute;
    right: 0;
    top: 50%;
    transform: translateY(-50%);
    height: 60%;
    background: color-mix(in srgb, var(--amber) 20%, transparent);
    pointer-events: none;
  }
  .share .val {
    position: relative;
  }
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
    gap: var(--space-4);
  }
  .box {
    border: 0.5px solid var(--border);
    padding: var(--space-3);
  }
  .matrix {
    border-collapse: collapse;
    font-family: var(--mono);
    font-size: 11px;
  }
  .matrix th,
  .matrix td {
    border: 0.5px solid var(--border);
    padding: 4px 8px;
    text-align: right;
    white-space: nowrap;
  }
  .matrix thead th,
  .matrix tbody th {
    color: var(--dim);
    font-weight: var(--fw-normal);
  }
  @media (max-width: 860px) {
    .grid {
      grid-template-columns: repeat(2, 1fr);
    }
  }
</style>
