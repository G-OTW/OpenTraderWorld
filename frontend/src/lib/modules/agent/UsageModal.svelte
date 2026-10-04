<script>
  // Agent usage breakdown. Tokens and provider requests over a window, as a bar series plus
  // the same totals cut by persona, model and provider.
  //
  // Tokens only, never a price: converting them would need a per-model rate table kept by
  // hand, and a stale rate is worse than no rate. The cache share is the cost signal that
  // does not need one — the same prompt read from cache is billed a fraction of one sent
  // whole.
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { fmtTokens, fmtNum } from '$lib/format.js';
  import { agentApi } from '$lib/modules/agent/api.js';

  let { open = $bindable(false) } = $props();

  let days = $state(30);
  let report = $state(null);
  let loading = $state(true);
  let error = $state('');

  // A day's worth of history is read hour by hour; anything longer, day by day. 24 bars
  // either way, which is what the strip is sized for.
  let bucket = $derived(days === 1 ? 'hour' : 'day');

  onMount(reload);

  async function reload() {
    loading = true;
    error = '';
    try {
      report = await agentApi.usage(days, bucket);
    } catch (e) {
      error = e.message;
      report = null;
    } finally {
      loading = false;
    }
  }

  function setWindow(d) {
    days = Number(d);
    reload();
  }

  const sum = (rows, field) => (rows ?? []).reduce((acc, r) => acc + (r[field] || 0), 0);
  /** Everything that occupied the context window, cached or not. */
  const rowTotal = (r) =>
    (r.input_tokens || 0) +
    (r.output_tokens || 0) +
    (r.cache_write_tokens || 0) +
    (r.cache_read_tokens || 0);

  let series = $derived(report?.series ?? []);
  let totalTokens = $derived(series.reduce((acc, r) => acc + rowTotal(r), 0));
  let totalRequests = $derived(sum(series, 'requests'));
  let totalCached = $derived(sum(series, 'cache_read_tokens'));
  // Share of prompt tokens that were served from cache rather than re-processed. 0 means
  // caching is off or the provider does not report it.
  let cacheShare = $derived(totalTokens > 0 ? totalCached / totalTokens : 0);
  let peak = $derived(Math.max(1, ...series.map(rowTotal)));

  // The bucket key is "YYYY-MM-DDTHH:MM" already shifted to the user's offset server-side,
  // so it is formatted as a local wall-clock label and never re-shifted.
  function bucketLabel(key) {
    if (!key) return '';
    const [date, time] = key.split('T');
    if (bucket === 'hour') return `${time?.slice(0, 2) ?? ''}h`;
    return date?.slice(5) ?? key;
  }

  // Hover readout. The native title tooltip was unreadable (late, unstyled, no colours), so
  // the breakdown is drawn in place with the same swatches as the bars.
  let hi = $state(-1);
  let tipX = $state(0);
  let chartEl = $state(null);

  function enter(i, el) {
    hi = i;
    tipX = el.offsetLeft + el.offsetWidth / 2 - (chartEl?.scrollLeft ?? 0);
  }

  let hovered = $derived(hi >= 0 ? (series[hi] ?? null) : null);
  const cacheOf = (r) => (r.cache_read_tokens || 0) + (r.cache_write_tokens || 0);

  const barLabel = (r) =>
    `${bucketLabel(r.key)} · ${fmtTokens(rowTotal(r))} · ${$t('agent.usage.requestsN', {
      count: fmtNum(r.requests, 0)
    })}`;

  const tabs = $derived([
    { key: 'persona', label: $t('agent.usage.byPersona'), rows: report?.by_persona ?? [] },
    { key: 'model', label: $t('agent.usage.byModel'), rows: report?.by_model ?? [] },
    { key: 'provider', label: $t('agent.usage.byProvider'), rows: report?.by_provider ?? [] }
  ]);
  let tab = $state('persona');
  let rows = $derived(tabs.find((x) => x.key === tab)?.rows ?? []);
</script>

<Modal bind:open title={$t('agent.usage.title')} size="lg">
  <div class="usage">
    <div class="toolbar">
      <Dropdown
        value={String(days)}
        onpick={setWindow}
        ariaLabel={$t('agent.usage.window')}
        options={[
          { value: '1', label: $t('agent.usage.win24h') },
          { value: '7', label: $t('agent.usage.winDays', { count: 7 }) },
          { value: '30', label: $t('agent.usage.winDays', { count: 30 }) },
          { value: '90', label: $t('agent.usage.winDays', { count: 90 }) }
        ]}
      />
      <button class="ghost" onclick={reload} disabled={loading}>
        <Icon name="refresh-cw" size={13} /> {$t('common.refresh')}
      </button>
    </div>

    <ErrorText error={error} />

    <div class="tiles">
      <div class="tile">
        <span class="k">{$t('agent.usage.tokens')}</span>
        {#if loading}<Skeleton height="1.1rem" width="60%" />{:else}<span class="v">{fmtTokens(totalTokens)}</span>{/if}
      </div>
      <div class="tile">
        <span class="k">{$t('agent.usage.requests')}</span>
        {#if loading}<Skeleton height="1.1rem" width="50%" />{:else}<span class="v">{fmtNum(totalRequests, 0)}</span>{/if}
      </div>
      <div class="tile">
        <span class="k">{$t('agent.usage.cached')}</span>
        {#if loading}
          <Skeleton height="1.1rem" width="50%" />
        {:else}
          <span class="v" class:good={cacheShare > 0.2}>{Math.round(cacheShare * 100)}%</span>
        {/if}
      </div>
    </div>

    {#if !loading && !series.length}
      <EmptyState icon="bar-chart" compact title={$t('agent.usage.noData')} />
    {:else}
      <div class="chartwrap">
      <div class="chart" bind:this={chartEl} aria-busy={loading ? 'true' : undefined}>
        {#if loading}
          {#each Array.from({ length: 16 }, (_, i) => i) as i (i)}
            <div class="col"><Skeleton height="{20 + ((i * 37) % 60)}%" width="100%" /></div>
          {/each}
        {:else}
          {#each series as r, i (r.key)}
            <div
              class="col"
              class:on={hi === i}
              role="img"
              aria-label={barLabel(r)}
              onmouseenter={(e) => enter(i, e.currentTarget)}
              onmouseleave={() => (hi = -1)}
            >
              <!-- Stacked so the cached share reads at a glance: output on top of fresh
                   input, cache reads at the base. -->
              <div class="bar">
                <div class="seg out" style="height:{((r.output_tokens || 0) / peak) * 100}%"></div>
                <div class="seg in" style="height:{((r.input_tokens || 0) / peak) * 100}%"></div>
                <div
                  class="seg cache"
                  style="height:{(((r.cache_read_tokens || 0) + (r.cache_write_tokens || 0)) / peak) * 100}%"
                ></div>
              </div>
              <span class="xlab">{bucketLabel(r.key)}</span>
            </div>
          {/each}
        {/if}
      </div>
        {#if hovered}
          <div class="tip" style="left:{tipX}px">
            <div class="tiphead">
              <span>{bucketLabel(hovered.key)}</span>
              <b>{fmtTokens(rowTotal(hovered))}</b>
            </div>
            <div class="tiprow"><i class="sw in"></i>{$t('agent.usage.in')}<b>{fmtTokens(hovered.input_tokens)}</b></div>
            <div class="tiprow"><i class="sw out"></i>{$t('agent.usage.out')}<b>{fmtTokens(hovered.output_tokens)}</b></div>
            <div class="tiprow"><i class="sw cache"></i>{$t('agent.usage.cache')}<b>{fmtTokens(cacheOf(hovered))}</b></div>
            <div class="tipfoot">{$t('agent.usage.requestsN', { count: fmtNum(hovered.requests, 0) })}</div>
          </div>
        {/if}
      </div>
      <div class="legend">
        <span><i class="sw in"></i>{$t('agent.usage.in')}</span>
        <span><i class="sw out"></i>{$t('agent.usage.out')}</span>
        <span><i class="sw cache"></i>{$t('agent.usage.cache')}</span>
      </div>
    {/if}

    <div class="tabs" role="tablist">
      {#each tabs as x (x.key)}
        <button role="tab" class="tab" class:on={tab === x.key} aria-selected={tab === x.key} onclick={() => (tab = x.key)}>
          {x.label}
        </button>
      {/each}
    </div>

    <div class="tablewrap">
      <table>
        <thead>
          <tr>
            <th>{$t('agent.usage.name')}</th>
            <th class="num">{$t('agent.usage.tokens')}</th>
            <th class="num">{$t('agent.usage.in')}</th>
            <th class="num">{$t('agent.usage.out')}</th>
            <th class="num">{$t('agent.usage.cache')}</th>
            <th class="num">{$t('agent.usage.requests')}</th>
          </tr>
        </thead>
        <tbody>
          {#if loading}
            {#each Array.from({ length: 3 }, (_, i) => i) as i (i)}
              <tr>
                <td><Skeleton height="0.85rem" width="55%" /></td>
                <td class="num"><Skeleton height="0.85rem" width="40%" /></td>
                <td class="num"><Skeleton height="0.85rem" width="40%" /></td>
                <td class="num"><Skeleton height="0.85rem" width="40%" /></td>
                <td class="num"><Skeleton height="0.85rem" width="40%" /></td>
                <td class="num"><Skeleton height="0.85rem" width="30%" /></td>
              </tr>
            {/each}
          {:else if !rows.length}
            <tr><td colspan="6" class="muted">{$t('agent.usage.noData')}</td></tr>
          {:else}
            {#each rows as r (r.key + r.label)}
              <tr>
                <td class="name">{r.label}</td>
                <td class="num strong">{fmtTokens(rowTotal(r))}</td>
                <td class="num">{fmtTokens(r.input_tokens)}</td>
                <td class="num">{fmtTokens(r.output_tokens)}</td>
                <td class="num">{fmtTokens((r.cache_read_tokens || 0) + (r.cache_write_tokens || 0))}</td>
                <td class="num">{fmtNum(r.requests, 0)}</td>
              </tr>
            {/each}
          {/if}
        </tbody>
      </table>
    </div>

    <p class="foot">{$t('agent.usage.foot')}</p>
  </div>
</Modal>

<style>
  .usage {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .ghost {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    padding: 5px var(--space-2);
    font-size: 12px;
    cursor: pointer;
  }
  .ghost:hover:not(:disabled) {
    color: var(--text);
  }
  .ghost:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-2);
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    min-width: 0;
  }
  .tile .k {
    font-size: 11px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .tile .v {
    font-size: 18px;
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .tile .v.good {
    color: var(--green);
  }

  .chartwrap {
    position: relative;
  }
  .chart {
    display: flex;
    align-items: flex-end;
    gap: 3px;
    height: 120px;
    padding: var(--space-2) 0 0;
    overflow-x: auto;
  }
  .col {
    flex: 1 1 0;
    min-width: 10px;
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 3px;
  }
  .col.on .bar {
    filter: brightness(1.15);
  }
  .bar {
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    height: 100%;
    border-radius: 2px;
    overflow: hidden;
  }
  .seg {
    width: 100%;
    min-height: 0;
  }
  .seg.in {
    background: var(--accent);
  }
  .seg.out {
    background: var(--green);
  }
  .seg.cache {
    background: var(--border);
  }
  .xlab {
    font-size: 9px;
    color: var(--muted);
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
  }

  .tip {
    position: absolute;
    bottom: calc(100% - 118px);
    transform: translateX(-50%);
    z-index: 3;
    pointer-events: none;
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 150px;
    max-width: 90%;
    padding: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: 0 6px 20px rgb(0 0 0 / 0.22);
    font-size: 11px;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .tiphead {
    display: flex;
    justify-content: space-between;
    gap: var(--space-3);
    padding-bottom: 3px;
    border-bottom: 1px solid var(--border);
    color: var(--muted);
  }
  .tiphead b {
    color: var(--text);
  }
  .tiprow {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--muted);
  }
  .tiprow b {
    margin-left: auto;
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .tipfoot {
    color: var(--muted);
  }

  .legend {
    display: flex;
    gap: var(--space-3);
    font-size: 11px;
    color: var(--muted);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .sw {
    width: 9px;
    height: 9px;
    border-radius: 2px;
    display: inline-block;
  }
  .sw.in {
    background: var(--accent);
  }
  .sw.out {
    background: var(--green);
  }
  .sw.cache {
    background: var(--border);
  }

  .tabs {
    display: flex;
    gap: var(--space-1);
    border-bottom: 1px solid var(--border);
  }
  .tab {
    background: none;
    border: 0;
    border-bottom: 2px solid transparent;
    color: var(--muted);
    padding: var(--space-2) var(--space-2);
    font-size: 12px;
    cursor: pointer;
  }
  .tab.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }

  .tablewrap {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  th {
    text-align: left;
    font-weight: var(--fw-medium);
    color: var(--muted);
    padding: var(--space-1) var(--space-2);
    white-space: nowrap;
  }
  td {
    padding: var(--space-2);
    border-top: 1px solid var(--border);
    color: var(--text);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .strong {
    font-weight: var(--fw-medium);
  }
  .name {
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .muted {
    color: var(--muted);
  }
  .foot {
    margin: 0;
    font-size: 11px;
    color: var(--muted);
  }

  @media (max-width: 560px) {
    .tiles {
      grid-template-columns: 1fr;
    }
    .xlab {
      display: none;
    }
  }
</style>
