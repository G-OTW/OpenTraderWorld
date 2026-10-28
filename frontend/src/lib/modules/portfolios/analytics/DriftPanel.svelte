<script>
  // `drift` — current allocation against the target, and the trades that close the gap.
  //
  // Display only. Nothing here places an order, which is why the suggestion is money and a
  // direction rather than a ticket.
  import { t } from '$lib/i18n';
  import { fmtMoney, fmtPct, fmtSignedPct, fmtNum } from '$lib/format.js';
  import Unavailable from './Unavailable.svelte';
  import Sample from './Sample.svelte';

  let { block = null, currency = 'USD', onfix = null } = $props();
  const d = $derived(block?.data ?? null);
  // The bar is scaled to the widest row so a 2% bucket is still visible next to a 60% one.
  const scale = $derived(
    d ? Math.max(10, ...d.rows.map((r) => Math.max(r.current_pct, r.target_pct))) : 100
  );
</script>

{#if block?.status !== 'ok' || !d}
  <Unavailable missing={block?.missing ?? []} sample={block?.sample} {onfix} />
{:else}
  <div class="head">
    <span class="badge" class:ok={d.in_band} class:off={!d.in_band}>
      {d.in_band
        ? $t('portfolios.analytics.drift.inBand')
        : $t('portfolios.analytics.drift.outOfBand', { pct: fmtPct(d.worst_deviation_pct) })}
    </span>
    <button class="btn sm ghost" onclick={() => onfix?.('targets')}>
      {$t('portfolios.analytics.drift.edit')}
    </button>
  </div>

  <table class="tbl">
    <thead>
      <tr>
        <th>{$t('portfolios.analytics.drift.bucket')}</th>
        <th class="r">{$t('portfolios.analytics.drift.current')}</th>
        <th class="r">{$t('portfolios.analytics.drift.target')}</th>
        <th class="r">{$t('portfolios.analytics.drift.deviation')}</th>
        <th class="w">{$t('portfolios.analytics.drift.chart')}</th>
        <th class="r">{$t('portfolios.analytics.drift.move')}</th>
      </tr>
    </thead>
    <tbody>
      {#each d.rows as r (r.bucket)}
        <tr class:off={r.out_of_band}>
          <td>{$t(`portfolios.class.${r.bucket}`)}</td>
          <td class="r num">{fmtPct(r.current_pct)}</td>
          <td class="r num muted">{fmtPct(r.target_pct)}</td>
          <td class="r num" class:up={r.deviation_pct > 0} class:down={r.deviation_pct < 0}>
            {fmtSignedPct(r.deviation_pct)}
          </td>
          <td class="w">
            <!-- The target is a tick on the bar, not a second bar: the question is how far
                 the fill is from the mark, and two bars make the eye do subtraction. -->
            <div class="track">
              <div class="fill" class:off={r.out_of_band} style="width:{(r.current_pct / scale) * 100}%"></div>
              <div class="tick" style="left:{(r.target_pct / scale) * 100}%"></div>
              <div
                class="band"
                style="left:{Math.max(0, ((r.target_pct - r.band_pct) / scale) * 100)}%;width:{((2 * r.band_pct) / scale) * 100}%"
              ></div>
            </div>
          </td>
          <td class="r num">{r.out_of_band ? fmtMoney(r.delta_value, currency) : '—'}</td>
        </tr>
      {/each}
    </tbody>
  </table>

  {#if d.unallocated.length}
    <!-- Held but not planned for. Reported, never folded into "other" or treated as a zero
         target: both hide a real hole in the plan. -->
    <p class="unalloc">
      {$t('portfolios.analytics.drift.unallocated')}
      {#each d.unallocated as u (u.bucket)}
        <span class="chip">{$t(`portfolios.class.${u.bucket}`)} {fmtPct(u.current_pct)}</span>
      {/each}
    </p>
  {/if}

  {#if d.trades.length}
    <div class="plan">
      <h4>{$t('portfolios.analytics.drift.plan')}</h4>
      <p class="disclaimer">{$t('portfolios.analytics.drift.disclaimer')}</p>
      {#each d.trades as tr (tr.bucket)}
        <div class="trade">
          <span class="side {tr.side}">{$t(`portfolios.kind.${tr.side}`)}</span>
          <strong>{fmtMoney(tr.amount, currency)}</strong>
          <span class="of">{$t(`portfolios.class.${tr.bucket}`)}</span>
          {#if tr.legs.length}
            <span class="legs">
              {#each tr.legs as leg (leg.asset_id)}
                <span class="leg">
                  {leg.symbol}
                  {#if leg.units != null}<i class="num">{fmtNum(leg.units, 4)}</i>{/if}
                </span>
              {/each}
            </span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <Sample sample={block.sample} />
{/if}

<style>
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
  }
  .badge {
    padding: 3px 10px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 600;
  }
  .badge.ok {
    background: color-mix(in srgb, var(--green) 15%, transparent);
    color: var(--green);
  }
  .badge.off {
    background: color-mix(in srgb, var(--amber) 18%, transparent);
    color: var(--amber);
  }
  .r {
    text-align: right;
  }
  .w {
    width: 28%;
    min-width: 140px;
  }
  .muted {
    color: var(--muted);
  }
  .track {
    position: relative;
    height: 10px;
    background: var(--surface-2);
    border-radius: 999px;
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
  }
  .fill.off {
    background: var(--amber);
  }
  .band {
    position: absolute;
    top: 0;
    height: 100%;
    background: color-mix(in srgb, var(--text) 10%, transparent);
  }
  .tick {
    position: absolute;
    top: -2px;
    width: 2px;
    height: 14px;
    background: var(--text);
  }
  .unalloc {
    margin-top: var(--space-3);
    font-size: 12px;
    color: var(--muted);
  }
  .chip {
    display: inline-block;
    margin-left: var(--space-2);
    padding: 2px 8px;
    border: 1px solid var(--border);
    border-radius: 999px;
  }
  .plan {
    margin-top: var(--space-4);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }
  h4 {
    margin: 0;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .disclaimer {
    margin: var(--space-1) 0 var(--space-3);
    font-size: 11px;
    color: var(--muted);
  }
  .trade {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: baseline;
    padding: var(--space-2) 0;
    border-top: 1px solid var(--border);
  }
  .side {
    padding: 2px 8px;
    border-radius: var(--radius);
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
  }
  .side.buy {
    background: color-mix(in srgb, var(--green) 15%, transparent);
    color: var(--green);
  }
  .side.sell {
    background: color-mix(in srgb, var(--red) 15%, transparent);
    color: var(--red);
  }
  .of {
    color: var(--muted);
  }
  .legs {
    display: flex;
    gap: var(--space-2);
    margin-left: auto;
    font-size: 12px;
    color: var(--muted);
  }
  .leg i {
    font-style: normal;
    opacity: 0.7;
    margin-left: 4px;
  }
</style>
