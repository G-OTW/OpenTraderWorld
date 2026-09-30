<script>
  // Managers'-portfolios widgets — the cards of the Mportfolios mockup that this module's
  // data can actually carry. Reads `list()` for the tracked set and `detail(slug)` for one
  // manager's holdings.
  //
  // The mockup's "performance vs benchmark" and "top performing manager" are not built:
  // the module stores holdings and their last price, not a per-manager return series or a
  // benchmark, and a chart drawn from what is here would be a guess.
  //
  // Config: { variant, slug, limit }.
  import { mportfoliosApi } from '$lib/modules/mportfolios/api.js';
  import { fmtCompactMoney, fmtSignedPct, ago } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';
  import RankBars from './parts/RankBars.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'count');
  const limit = $derived(Math.max(1, Math.min(30, item.config?.limit ?? 5)));
  const slug = $derived(item.config?.slug || '');

  let list = $state(null);
  let detail = $state.raw(null);
  let err = $state('');

  $effect(() => {
    if (editing) return;
    let alive = true;
    mportfoliosApi.list()
      .then((r) => { if (alive) list = r.portfolios ?? r; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const chosen = $derived((list ?? []).find((p) => p.slug === slug) ?? (list ?? [])[0] ?? null);

  $effect(() => {
    if (editing || !['drift', 'movers'].includes(variant) || !chosen) return;
    const s = chosen.slug;
    let alive = true;
    mportfoliosApi.detail(s)
      .then((d) => { if (alive) detail = d; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const totalHoldings = $derived((list ?? []).reduce((s, p) => s + (p.stock_count ?? 0), 0));
  const lastUpdate = $derived(
    (list ?? []).map((p) => p.updated_at).filter(Boolean).sort().at(-1) ?? null
  );

  const largest = $derived(
    (list ?? [])
      .filter((p) => Number.isFinite(p.value_num))
      .sort((a, b) => b.value_num - a.value_num)
      .slice(0, limit)
      .map((p) => ({ label: p.name, value: p.value_num, display: fmtCompactMoney(p.value_num) ?? p.value_text }))
  );

  // The scraped "activity" column is the module's own record of what moved since the last
  // filing: added on one side, reduced or sold on the other.
  const holdings = $derived(detail?.holdings ?? []);
  const added = $derived(holdings.filter((h) => /buy|add/i.test(h.activity)));
  const removed = $derived(holdings.filter((h) => /sell|reduce/i.test(h.activity)));
</script>

<WidgetState
  {editing}
  error={err}
  loading={list === null || (['drift', 'movers'].includes(variant) && chosen && detail === null)}
  empty={(list ?? []).length === 0}
  preview={$t('dashboard.widgets.mportfolios.preview')}
  emptyText={$t('dashboard.widgets.mportfolios.empty')}
  rows={4}
>
  {#if variant === 'largest'}
    <RankBars rows={largest} />
  {:else if variant === 'drift'}
    <div class="w-body">
      <span class="w-eyebrow">{chosen?.name}</span>
      <div class="cols">
        <div class="col">
          <span class="w-sub">{$t('dashboard.widgets.mportfolios.added')}</span>
          <span class="fig w-pos">{added.length}</span>
          <ul class="tickers">
            {#each added.slice(0, limit) as h (h.ticker)}
              <li><span class="w-dot up"></span>{h.ticker}</li>
            {/each}
          </ul>
        </div>
        <div class="col">
          <span class="w-sub">{$t('dashboard.widgets.mportfolios.removed')}</span>
          <span class="fig w-neg">{removed.length}</span>
          <ul class="tickers">
            {#each removed.slice(0, limit) as h (h.ticker)}
              <li><span class="w-dot down"></span>{h.ticker}</li>
            {/each}
          </ul>
        </div>
      </div>
    </div>
  {:else if variant === 'movers'}
    <ul class="w-list">
      {#each [...holdings].filter((h) => h.change_pct != null).sort((a, b) => b.change_pct - a.change_pct).slice(0, limit) as h (h.ticker)}
        <li class="w-row">
          <span class="tick">{h.ticker}</span>
          <span class="w-name grow">{h.company}</span>
          <span class="w-num" class:w-pos={h.change_pct >= 0} class:w-neg={h.change_pct < 0}>
            {fmtSignedPct(h.change_pct)}
          </span>
        </li>
      {/each}
    </ul>
  {:else}
    <div class="w-body">
      <Stat value={String((list ?? []).length)} note={$t('dashboard.widgets.mportfolios.tracked')} />
      <ul class="w-list">
        <li class="w-row">
          <span class="w-name grow">{$t('dashboard.widgets.mportfolios.holdings')}</span>
          <span class="w-num">{totalHoldings}</span>
        </li>
        {#if lastUpdate}
          <li class="w-row">
            <span class="w-name grow">{$t('dashboard.widgets.mportfolios.lastUpdate')}</span>
            <span class="w-num">{$ago(lastUpdate)}</span>
          </li>
        {/if}
      </ul>
    </div>
  {/if}
</WidgetState>

<style>
  .grow {
    flex: 1;
    min-width: 0;
  }
  .tick {
    font-family: var(--mono);
    font-size: var(--text-sm);
    flex-shrink: 0;
  }
  /* Added on the left, removed on the right, split by one hairline. */
  .cols {
    display: flex;
    gap: var(--space-4);
    min-width: 0;
  }
  .col {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .col + .col {
    padding-left: var(--space-4);
    border-left: var(--hairline) solid var(--border);
  }
  .fig {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 24px;
    font-weight: var(--fw-medium);
    line-height: 1.1;
  }
  .tickers {
    list-style: none;
    margin: var(--space-2) 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .tickers li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-family: var(--mono);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .w-dot.up {
    background: var(--green);
  }
  .w-dot.down {
    background: var(--red);
  }
</style>
