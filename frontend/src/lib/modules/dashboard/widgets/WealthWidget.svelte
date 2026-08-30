<script>
  // Net-worth widget: the MyWealth headline — current net worth, its change over the
  // configured window, and a sparkline of that window. Values are the module's display
  // currency (FX-converted server-side), same as the module page.
  // Config: { months } — how far back the window reaches (default 12).
  import { wealthApi } from '$lib/modules/wealth/api.js';
  import { fmtMoney, fmtSignedMoney, fmtSignedPct, fmtMonth } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let { item, editing } = $props();
  const months = $derived(Math.max(1, Math.min(60, item.config?.months ?? 12)));

  let bd = $state(null);
  let err = $state('');

  async function load(back) {
    err = '';
    try {
      bd = await wealthApi.breakdown({ granularity: 'month', points_back: back });
    } catch (e) {
      err = e.message;
    }
  }
  $effect(() => {
    const back = months;
    if (!editing) load(back);
  });

  const cur = $derived(bd?.display_currency ?? 'USD');
  const points = $derived(bd?.points ?? []);

  // Baseline = the first point that is actually worth something. Months before the first
  // asset existed sit at zero, and measuring growth from zero is a meaningless ∞%.
  const base = $derived(points.find((p) => p.net_worth > 0) ?? null);
  const delta = $derived(base ? (bd?.net_worth ?? 0) - base.net_worth : null);
  const deltaPct = $derived(base && base.net_worth ? (delta / base.net_worth) * 100 : null);

  // Sparkline over the window, normalized; drawn only once there are two points to join.
  const W = 240;
  const H = 40;
  const line = $derived.by(() => {
    const vals = points.map((p) => p.net_worth);
    if (vals.length < 2) return '';
    const min = Math.min(...vals);
    const max = Math.max(...vals);
    const span = max - min || 1;
    const step = (W - 2) / (vals.length - 1);
    return vals
      .map((v, i) => `${1 + i * step},${1 + (1 - (v - min) / span) * (H - 2)}`)
      .join(' ');
  });
</script>

{#if editing}
  <p class="hint">{$t('dashboard.widgets.wealth.preview')}</p>
{:else if err}
  <ErrorText error={err} compact />
{:else if bd === null}
  <div class="sk" aria-busy="true"><Skeleton rows={3} height="1.1rem" gap="var(--space-2)" /></div>
{:else}
  <div class="nw">
    <div class="value">{fmtMoney(bd.net_worth, cur, 0)}</div>

    {#if delta != null}
      <div class="delta" class:pos={delta >= 0} class:neg={delta < 0}>
        {fmtSignedMoney(delta, cur, 0)}{#if deltaPct != null} · {fmtSignedPct(deltaPct, 1)}{/if}
        <span class="since">{$t('dashboard.widgets.wealth.since', { month: fmtMonth(base.at) })}</span>
      </div>
    {/if}

    <!-- The sparkline takes whatever height is left, so the widget reads the same at any
         of the three height presets instead of overflowing the compact one. -->
    {#if line}
      <div class="spark">
        <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" role="img"
          aria-label={$t('dashboard.widgets.wealth.trend')}>
          <polyline points={line} fill="none" stroke={delta != null && delta < 0 ? 'var(--red)' : 'var(--green)'}
            stroke-width="1" vector-effect="non-scaling-stroke" />
        </svg>
      </div>
    {/if}

    <div class="meta">
      {#if (bd.asset_count ?? 0) === 1}
        {$t('dashboard.widgets.wealth.metaOne')}
      {:else}
        {$t('dashboard.widgets.wealth.metaMany', { count: bd.asset_count ?? 0 })}
      {/if}
      {#if bd.unconverted > 0}
        <span class="warn">· {$t('dashboard.widgets.wealth.unconverted', { count: bd.unconverted, cur })}</span>
      {/if}
    </div>
  </div>
{/if}

<style>
  .hint {
    color: var(--dim);
  }
  .sk {
    padding: var(--space-1) 0;
  }
  .nw {
    display: flex;
    flex-direction: column;
    gap: 3px;
    height: 100%;
    min-height: 0;
  }
  .value {
    font-size: 1.6rem;
    font-weight: var(--fw-normal);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    line-height: 1.15;
  }
  .delta {
    font-size: var(--text-base);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .delta.pos {
    color: var(--green);
  }
  .delta.neg {
    color: var(--red);
  }
  .since {
    font-family: var(--font);
    font-size: var(--text-xs);
    color: var(--dim);
    margin-left: var(--space-1, 4px);
  }
  .spark {
    flex: 1;
    min-height: 26px;
    margin: var(--space-1, 4px) 0;
  }
  .spark svg {
    display: block;
    width: 100%;
    height: 100%;
  }
  .meta {
    font-size: var(--text-xs);
    color: var(--dim);
    margin-top: auto;
  }
  .warn {
    color: var(--amber);
  }
</style>
