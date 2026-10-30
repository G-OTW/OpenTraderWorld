<script>
  // An account's money, the same way for every broker. Three levels: what the account is worth
  // and its results, how much of it the positions tie up (a bar against the equity), then the
  // detail figures. A figure the broker does not state is not shown, never made up.
  import { t } from '$lib/i18n';

  let { summary } = $props();

  const has = (f) => Number.isFinite(summary?.[f]);
  const num = (v) => v.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const signed = (v) => `${v > 0 ? '+' : v < 0 ? '−' : ''}${num(Math.abs(v))}`;

  /** The results next to the equity, coloured by their sign. */
  const results = $derived(['unrealized', 'realized'].filter(has));
  /** The rest, in reading order. */
  const DETAIL = ['cash', 'available', 'buying_power', 'excess_liquidity', 'maintenance_margin', 'gross_position_value', 'fees'];
  const detail = $derived(DETAIL.filter(has));
  /** The chart pair's own assets (base, then quote), when the broker holds assets by name. */
  const assets = $derived(Array.isArray(summary?.assets) ? summary.assets : []);
  /** An asset's quantity, to its own precision (0.00012 BTC is not 0.00). */
  const qty = (v) => v.toLocaleString(undefined, { maximumFractionDigits: 8 });

  /** Share of the equity the initial margin ties up, and where the maintenance margin sits. */
  const usage = $derived.by(() => {
    if (!has('margin_used') || !has('equity') || !(summary.equity > 0)) return null;
    const pct = (v) => Math.max(0, Math.min(100, (v / summary.equity) * 100));
    return {
      pct: (summary.margin_used / summary.equity) * 100,
      fill: pct(summary.margin_used),
      maint: has('maintenance_margin') ? pct(summary.maintenance_margin) : null
    };
  });
  const level = $derived(!usage ? '' : usage.pct >= 80 ? 'high' : usage.pct >= 50 ? 'mid' : 'low');
</script>

<div class="acs">
  <div class="top">
    {#if has('equity')}
      <div class="eq">
        <span class="lbl">{$t('brokers.account.equity')}</span>
        <span class="big">{num(summary.equity)} <span class="cur">{summary.currency}</span></span>
      </div>
    {/if}
    {#each results as f (f)}
      {@const v = summary[f]}
      <div class="res">
        <span class="lbl">{$t(`brokers.account.${f}`)}</span>
        <span class="val" class:up={v > 0} class:down={v < 0}>{signed(v)}</span>
      </div>
    {/each}
  </div>

  {#if usage}
    <div class="margin">
      <div class="mhead">
        <span class="lbl">{$t('brokers.account.margin_used')}</span>
        <span class="mono">{num(summary.margin_used)}</span>
        <span class="pct {level}">{usage.pct.toFixed(1)}%</span>
      </div>
      <div class="bar" role="meter" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(usage.fill)} aria-label={$t('brokers.account.margin_used')}>
        <span class="fill {level}" style:width="{usage.fill}%"></span>
        {#if usage.maint != null}
          <span class="maint" style:left="{usage.maint}%" title={$t('brokers.account.maintenance_margin')}></span>
        {/if}
      </div>
    </div>
  {/if}

  {#if detail.length}
    <dl class="grid">
      {#each detail as f (f)}
        <div>
          <dt>{$t(`brokers.account.${f}`)}</dt>
          <dd>{num(summary[f])}</dd>
        </div>
      {/each}
    </dl>
  {/if}

  {#if assets.length}
    <dl class="grid">
      {#each assets as a (a.asset)}
        <div>
          <dt>{$t('brokers.account.assetFree', { asset: a.asset })}</dt>
          <dd>{qty(a.free)}</dd>
          {#if a.locked > 0}<dd class="locked">{$t('brokers.account.assetLocked', { qty: qty(a.locked) })}</dd>{/if}
        </div>
      {/each}
    </dl>
  {/if}
</div>

<style>
  .acs {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .top {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-2) var(--space-6);
  }
  .eq,
  .res {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .lbl,
  dt {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .big {
    font-family: var(--mono);
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
    line-height: 1.1;
  }
  .val {
    font-family: var(--mono);
    font-size: var(--text-md);
  }
  .cur {
    color: var(--muted);
    font-size: var(--text-xs);
    font-weight: normal;
  }
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }
  .margin {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .mhead {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }
  .mono {
    font-family: var(--mono);
  }
  .pct {
    margin-left: auto;
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .pct.mid,
  .fill.mid {
    color: var(--amber);
  }
  .pct.high,
  .fill.high {
    color: var(--red);
  }
  .pct.low {
    color: var(--muted);
  }
  .bar {
    position: relative;
    height: 6px;
    border-radius: 3px;
    background: var(--surface-2);
    overflow: hidden;
  }
  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: 3px;
    background: currentColor;
    color: var(--green);
  }
  .maint {
    position: absolute;
    top: -1px;
    bottom: -1px;
    width: 2px;
    background: var(--text);
    opacity: 0.6;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
    gap: var(--space-2) var(--space-4);
    margin: 0;
    padding-top: var(--space-2);
    border-top: var(--hairline) solid var(--border);
  }
  dd {
    margin: 0;
    font-family: var(--mono);
    font-size: var(--text-sm);
  }
  dd.locked {
    color: var(--muted);
    font-size: var(--text-xs);
  }
</style>
