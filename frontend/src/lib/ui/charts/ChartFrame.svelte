<script>
  // Header + body + legend around one chart. Frameless on purpose: it sits inside the
  // module's own card or panel, and a frame inside a frame is the one thing the design
  // forbids. The body keeps `height` in every state (chart, loading, unavailable), so
  // the layout never jumps when data arrives or goes away.
  //
  //   <ChartFrame title="Net worth" subtitle="Monthly, in EUR" height={240}
  //               legend={[{ label: 'Recorded', color: c.accent, kind: 'line' }]}
  //               unavailable={points.length ? null : { reason: 'No valuation yet.',
  //                 action: { label: 'Add an asset', onclick: add } }}>
  //     {#snippet actions()}<PeriodChips ... />{/snippet}
  //     <TimeSeries series={...} height={240} />
  //   </ChartFrame>
  //
  // props: title, subtitle, legend [{ key?, label, color, kind?, dashed?, value? }],
  //        height (px min height of the body; pass the chart's height so the empty and
  //        loading states match it, default none: 200px for those states), loading, unavailable { reason, title?, icon?,
  //        action?: { label, onclick?, href?, icon? } }, emphasized ($bindable legend key),
  //        toggle (legend rows emphasise on click), sensitive (mask legend values under
  //        the privacy screen, default true), actions (snippet), children (snippet)
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { privacy } from '$lib/theme/privacy.svelte.js';
  import Legend from './Legend.svelte';
  import { MASK } from './palette.js';

  let {
    title = '',
    subtitle = '',
    legend = [],
    height = null,
    loading = false,
    unavailable = null,
    emphasized = $bindable(null),
    toggle = false,
    sensitive = true,
    actions,
    children
  } = $props();

  const items = $derived(
    (legend ?? []).map((it) => ({
      ...it,
      value: it.value != null && sensitive && privacy.hidden ? MASK : it.value
    }))
  );
  // Loading and unavailable states hold the chart's height (200px when none is given).
  const stateHeight = $derived(height || 200);
  const showHead = $derived(!!(title || subtitle || actions));
</script>

<section class="chart-frame">
  {#if showHead}
    <header class="cf-head">
      <div class="cf-titles">
        {#if title}<h3 class="cf-title">{title}</h3>{/if}
        {#if subtitle}<p class="cf-sub">{subtitle}</p>{/if}
      </div>
      {#if actions}<div class="cf-actions">{@render actions()}</div>{/if}
    </header>
  {/if}

  <div class="cf-body" style:min-height={height ? `${height}px` : undefined}>
    {#if loading}
      <Skeleton height="{stateHeight}px" />
    {:else if unavailable}
      <div class="cf-state" style:height="{stateHeight}px">
        <EmptyState
          icon={unavailable.icon ?? 'bar-chart'}
          title={unavailable.title ?? ''}
          description={unavailable.reason ?? ''}
          action={unavailable.action ? stateAction : undefined}
          compact
        />
      </div>
    {:else}
      {@render children?.()}
    {/if}
  </div>

  {#if items.length && !loading && !unavailable}
    <div class="cf-legend">
      <Legend {items} active={emphasized} onToggle={toggle ? (k) => (emphasized = k) : null} />
    </div>
  {/if}
</section>

{#snippet stateAction()}
  {#if unavailable?.action?.href}
    <a class="btn sm" href={unavailable.action.href}>{unavailable.action.label}</a>
  {:else if unavailable?.action}
    <Button size="sm" icon={unavailable.action.icon ?? ''} onclick={unavailable.action.onclick}>
      {unavailable.action.label}
    </Button>
  {/if}
{/snippet}

<style>
  .chart-frame {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .cf-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-3);
    min-width: 0;
  }
  .cf-titles {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .cf-title {
    margin: 0;
    color: var(--text);
    font-size: 13px;
    font-weight: 600;
    line-height: var(--lh-tight);
  }
  .cf-sub {
    margin: 0;
    color: var(--dim);
    font-size: 12px;
    line-height: var(--lh-tight);
  }
  .cf-actions {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .cf-body {
    position: relative;
    min-width: 0;
  }
  .cf-state {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 160px;
  }
  .cf-state > :global(.ui-empty) {
    padding-top: 0;
    padding-bottom: 0;
  }
</style>
