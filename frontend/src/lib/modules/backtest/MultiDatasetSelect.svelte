<script>
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';
  // Multi-select dataset picker (2–8 for a portfolio run). The field shows the basket as
  // removable chips; opening it shows every stored dataset, grouped by asset class, with a
  // checkbox each. Ticks are a draft: "Add N datasets" applies them, Escape or a click outside
  // leaves the basket as it was. A portfolio run needs one shared timeframe, so once a dataset
  // is ticked the others of another timeframe are greyed with the reason.
  // `max` caps the basket: a single-asset engine (grid) passes 1, which turns the picker into
  // a one-shot pick instead of letting a basket through that only the first asset would run.
  // `onlive()`, when given, adds a way out of an empty list: search a live symbol and
  // store it (the page owns that dialog).
  let { datasets = [], values = $bindable([]), max = 8, onlive = null } = $props();

  let open = $state(false);
  let query = $state('');
  let tfFilter = $state('');
  let draft = $state([]);
  let active = $state(0);
  let root = $state(null);
  let input = $state(null);
  let listEl = $state(null);

  const MAX = $derived(Math.max(1, max));
  const single = $derived(MAX === 1);
  const CLASS_ORDER = ['crypto', 'future', 'equity', 'etf', 'index', 'fx', 'option'];
  const KNOWN = new Set([...CLASS_ORDER, 'other']);
  const rank = (a) => {
    const i = CLASS_ORDER.indexOf(a);
    return i < 0 ? 99 : i;
  };

  const usable = $derived(datasets.filter((d) => d.bar_count > 0));
  const byId = $derived(new Map(usable.map((d) => [d.id, d])));
  const selected = $derived(values.map((id) => byId.get(id)).filter(Boolean));
  const picked = $derived(draft.map((id) => byId.get(id)).filter(Boolean));
  // Timeframe locked to the first tick; others of a different tf can't be added.
  const lockedTf = $derived(picked[0]?.timeframe ?? null);

  const matched = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return usable;
    return usable.filter((d) => `${d.ticker} ${d.timeframe} ${d.provider} ${d.asset_type}`.toLowerCase().includes(q));
  });
  const timeframes = $derived.by(() => {
    const n = new Map();
    for (const d of matched) n.set(d.timeframe, (n.get(d.timeframe) ?? 0) + 1);
    return [...n.entries()].sort((a, b) => tfSeconds(a[0]) - tfSeconds(b[0]));
  });
  const groups = $derived.by(() => {
    const map = new Map();
    for (const d of matched) {
      if (tfFilter && d.timeframe !== tfFilter) continue;
      const k = d.asset_type || 'other';
      if (!map.has(k)) map.set(k, []);
      map.get(k).push(d);
    }
    const on = (d) => (draft.includes(d.id) ? 0 : 1);
    return [...map.entries()]
      .sort((a, b) => rank(a[0]) - rank(b[0]) || a[0].localeCompare(b[0]))
      .map(([cls, sets]) => ({
        cls,
        sets: [...sets].sort((a, b) => on(a) - on(b) || a.ticker.localeCompare(b.ticker) || tfSeconds(a.timeframe) - tfSeconds(b.timeframe))
      }));
  });
  const flat = $derived(groups.flatMap((g) => g.sets));
  const indexOf = $derived(new Map(flat.map((d, i) => [d.id, i])));

  /** A timeframe string as seconds, to sort 1m before 15m before 1d. */
  function tfSeconds(tf) {
    const m = /^(\d+)\s*([smhdwM])/.exec(String(tf ?? ''));
    if (!m) return Number.MAX_SAFE_INTEGER;
    const unit = { s: 1, m: 60, h: 3600, d: 86400, w: 604800, M: 2592000 }[m[2]];
    return Number(m[1]) * unit;
  }

  const isOn = (d) => draft.includes(d.id);
  const canAdd = (d) => isOn(d) || single || (draft.length < MAX && (!lockedTf || d.timeframe === lockedTf));
  const why = (d) => (lockedTf && d.timeframe !== lockedTf ? $t('backtest.multi.otherTf') : $t('backtest.multi.full'));

  function openMenu() {
    draft = [...values];
    query = '';
    tfFilter = selected[0]?.timeframe ?? '';
    active = 0;
    open = true;
    queueMicrotask(() => input?.focus());
  }
  function close() {
    open = false;
  }
  function toggle(d) {
    if (!canAdd(d)) return;
    if (single) {
      values = [d.id];
      close();
      return;
    }
    draft = isOn(d) ? draft.filter((id) => id !== d.id) : [...draft, d.id];
  }
  function apply() {
    values = [...draft];
    close();
  }
  function remove(id) {
    values = values.filter((v) => v !== id);
  }
  function move(step) {
    if (!flat.length) return;
    active = (active + step + flat.length) % flat.length;
    queueMicrotask(() => listEl?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: 'nearest' }));
  }
  function onKey(e) {
    if (!open) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      move(1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      move(-1);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if ((e.metaKey || e.ctrlKey) && !single) apply();
      else if (flat[active]) toggle(flat[active]);
    } else if (e.key === 'Escape') {
      e.stopPropagation();
      close();
    }
  }
  $effect(() => {
    query;
    tfFilter;
    active = 0;
  });
  // Hover follows the pointer only when it really moves: a keyboard scroll slides rows under
  // a still pointer, and taking that as a hover would bounce the highlight.
  let lastPt = '';
  function hover(e, i) {
    const pt = `${e.clientX},${e.clientY}`;
    if (pt === lastPt) return;
    lastPt = pt;
    if (active !== i) active = i;
  }
  function onDocClick(e) {
    // A click on a row the update just removed lands on a detached node: not an outside click.
    if (!e.target?.isConnected) return;
    if (open && root && !root.contains(e.target)) close();
  }

  /** Avatar letters: the base of a pair (BTCUSDT → BTC), else the ticker's head. */
  function initials(d) {
    const s = String(d.ticker ?? '').toUpperCase().replace(/[^A-Z0-9]/g, '');
    const base = d.asset_type === 'crypto' ? s.replace(/(USDT|USDC|BUSD|FDUSD|USD|EUR|BTC|ETH)$/, '') || s : s;
    return base.slice(0, 4);
  }
  const TONES = ['accent', 'amber', 'blue', 'violet', 'teal'];
  function tone(d) {
    let h = 0;
    for (const ch of initials(d)) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
    return TONES[h % TONES.length];
  }
  /** binance_futures → Binance Futures. */
  const providerName = (p) =>
    String(p ?? '')
      .split(/[_-]+/)
      .filter(Boolean)
      .map((w) => (w.length <= 4 && w === w.toUpperCase() ? w : w[0].toUpperCase() + w.slice(1)))
      .join(' ');
  const classLabel = (c) => (KNOWN.has(c) ? $t(`histviz.data.class.${c}`) : c);
  const fmtBars = (n) => Number(n).toLocaleString();
  const applyLabel = $derived(
    draft.length === 0 ? $t('backtest.multi.done') : draft.length === 1 ? $t('backtest.multi.applyOne') : $t('backtest.multi.applyN', { n: draft.length })
  );
</script>

<svelte:document onclick={onDocClick} />

<div class="picker" bind:this={root} onkeydown={onKey} role="presentation">
  <!-- One frame: the basket as chips, then the way to open the list. -->
  <div
    class="basket"
    class:open
    role="button"
    tabindex="0"
    onclick={() => (open ? close() : openMenu())}
    onkeydown={(e) => {
      if (!open && (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowDown')) {
        e.preventDefault();
        openMenu();
      }
    }}
  >
    <Icon name="layers" size={15} />
    {#each selected as d (d.id)}
      <span class="dchip">
        <span class="tk">{d.ticker}</span>
        <span class="tf">{d.timeframe}</span>
        <button
          type="button"
          class="rm"
          title={$t('common.remove')}
          aria-label={$t('common.remove')}
          onclick={(e) => {
            e.stopPropagation();
            remove(d.id);
          }}
        >
          <Icon name="x" size={11} />
        </button>
      </span>
    {/each}
    <span class="ph">{selected.length ? (selected.length < MAX ? $t('backtest.multi.addAnother') : '') : $t('backtest.multi.addDatasets')}</span>
    <span class="caret"><Icon name={open ? 'chevron-up' : 'chevron-down'} size={14} /></span>
  </div>

  {#if open}
    <div class="menu">
      <div class="search">
        <Icon name="search" size={15} />
        <input
          bind:this={input}
          bind:value={query}
          placeholder={$t('backtest.dataset.searchPlaceholder')}
          aria-label={$t('backtest.dataset.searchAriaLabel')}
        />
      </div>

      {#if timeframes.length > 1 || (!single && MAX > 1)}
        <div class="tfbar">
          <span class="lab">{$t('backtest.multi.timeframe')}</span>
          <div class="tabs" role="tablist">
            <button type="button" role="tab" class="tab" class:on={!tfFilter} aria-selected={!tfFilter} onclick={() => (tfFilter = '')}>
              {$t('backtest.multi.all')}
            </button>
            {#each timeframes as [tf, n] (tf)}
              <button type="button" role="tab" class="tab" class:on={tfFilter === tf} aria-selected={tfFilter === tf} onclick={() => (tfFilter = tf)}>
                {tf}<span class="n">{n}</span>
              </button>
            {/each}
          </div>
          {#if !single}
            <span class="share"><Icon name="info" size={13} /> {$t('backtest.multi.shareTf')}</span>
          {/if}
        </div>
      {/if}

      <div class="opts" bind:this={listEl}>
        {#each groups as g (g.cls)}
          <div class="gh"><span>{classLabel(g.cls)}</span><span class="n">{g.sets.length}</span><span class="gl"></span></div>
          {#each g.sets as d (d.id)}
            {@const i = indexOf.get(d.id)}
            {@const on = isOn(d)}
            {@const allowed = canAdd(d)}
            <button
              type="button"
              class="opt"
              class:nocb={single}
              class:active={i === active}
              class:on
              class:off={!allowed}
              data-i={i}
              disabled={!allowed}
              aria-pressed={single ? undefined : on}
              onpointermove={(e) => hover(e, i)}
              onclick={() => toggle(d)}
            >
              {#if !single}
                <span class="cb" class:on>{#if on}<Icon name="check" size={11} />{/if}</span>
              {/if}
              <span class="av t-{tone(d)}">{initials(d)}</span>
              <span class="tk">{d.ticker}</span>
              <span class="tfc" class:on>{d.timeframe}</span>
              <span class="prov">{providerName(d.provider)}</span>
              <span class="end">{allowed ? fmtBars(d.bar_count) : why(d)}</span>
            </button>
          {/each}
        {/each}
        {#if !flat.length}
          <p class="none">{$t('backtest.dataset.noMatch')}</p>
        {/if}

        {#if onlive}
          <!-- Not in the catalog yet: the chart's symbol search, stored on the way in. -->
          <button
            type="button"
            class="live"
            onclick={() => {
              close();
              onlive();
            }}
          >
            <span class="live-ic"><Icon name="search" size={16} /></span>
            <span class="live-t">
              <span class="live-n">{$t('histdata.live.open')}</span>
              <span class="live-d">{$t('backtest.multi.liveSub')}</span>
            </span>
            <Icon name="chevron-right" size={15} />
          </button>
        {/if}
      </div>

      {#if !single}
        <div class="foot">
          {#if MAX <= 12}
            <span class="dots" aria-hidden="true">
              {#each Array.from({ length: MAX }) as _, k (k)}<span class="dt" class:on={k < draft.length}></span>{/each}
            </span>
          {/if}
          <span class="cnt">
            <b>{draft.length}</b>
            {$t('backtest.multi.ofMax', { max: MAX })}
            {#if lockedTf}· {$t('backtest.multi.runOn')} <b class="mono">{lockedTf}</b>{/if}
          </span>
          <button type="button" class="ghost" disabled={!draft.length} onclick={() => (draft = [])}>{$t('backtest.multi.clear')}</button>
          <button type="button" class="primary" onclick={apply}>{applyLabel}</button>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
  }
  /* Not `.field`: the theme's global `.field` is a label-above-control column, and its
     flex-direction and `> span` styles stacked the chips. */
  .basket {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    min-height: 44px;
    padding: 6px var(--space-2) 6px var(--space-3);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-lg);
    background: var(--surface);
    color: var(--muted);
    cursor: pointer;
    outline: none;
    transition: border-color var(--dur-fast) var(--ease);
  }
  .basket:hover,
  .basket:focus-visible,
  .basket.open {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border-control));
  }
  .dchip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 4px 0 var(--space-2);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text);
    font-family: var(--mono);
    font-size: var(--text-sm);
    cursor: default;
  }
  .dchip .tf {
    color: var(--muted);
  }
  .rm {
    display: inline-grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .rm:hover {
    background: color-mix(in srgb, var(--text) 8%, var(--surface-2));
    color: var(--red);
  }
  .ph {
    flex: 1;
    min-width: 120px;
    font-size: var(--text-sm);
  }
  .caret {
    display: inline-flex;
    color: var(--muted);
  }

  .menu {
    position: absolute;
    z-index: 30;
    top: calc(100% + 6px);
    left: 0;
    right: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--surface-raised, var(--surface));
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-2);
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    height: 46px;
    padding: 0 var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
    color: var(--muted);
  }
  .search input {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: none;
    background: transparent;
    box-shadow: none;
    color: var(--text);
    font-size: var(--text-base);
  }
  .search input:focus,
  .search input:focus-visible {
    border: none;
    box-shadow: none;
    outline: none;
  }
  .tfbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
    padding: var(--space-2) var(--space-3) 0;
  }
  .lab {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius);
    background: none;
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .n {
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .share {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-left: auto;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .opts {
    max-height: min(52vh, 420px);
    overflow-y: auto;
    overscroll-behavior: none;
    padding: 0 var(--space-2) var(--space-2);
  }
  .gh {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-3) var(--space-2) var(--space-1);
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .gl {
    flex: 1;
    height: var(--hairline);
    background: var(--border);
  }
  /* Fixed tracks, not `auto`: each row is its own grid, so content-sized columns drifted
     from row to row (a wider ticker pushed that row's timeframe right). */
  .opt {
    display: grid;
    grid-template-columns: 16px 30px minmax(0, 1fr) 56px minmax(0, 220px) 150px;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    min-height: 44px;
    padding: 4px var(--space-3);
    border: none;
    border-radius: var(--radius-lg);
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .opt.nocb {
    grid-template-columns: 30px minmax(0, 1fr) 56px minmax(0, 220px) 150px;
  }
  .opt.active:not(.off) {
    background: var(--surface-2);
  }
  .opt.on {
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .opt.on.active {
    background: color-mix(in srgb, var(--accent) 10%, var(--surface-2));
  }
  .opt.off {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .cb {
    display: inline-grid;
    place-items: center;
    width: 16px;
    height: 16px;
    border: 1.5px solid var(--border-control);
    border-radius: 4px;
    color: var(--accent-contrast);
  }
  .cb.on {
    border-color: var(--accent);
    background: var(--accent);
  }
  .av {
    display: inline-grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: var(--radius);
    font-family: var(--mono);
    font-size: 10px;
    font-weight: var(--fw-medium);
  }
  .t-accent {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
  }
  .t-amber {
    background: color-mix(in srgb, var(--amber) 14%, transparent);
    color: var(--amber);
  }
  .t-blue {
    background: color-mix(in srgb, #6ea8fe 14%, transparent);
    color: #8fb8f5;
  }
  .t-violet {
    background: color-mix(in srgb, #a78bfa 14%, transparent);
    color: #b9a5f5;
  }
  .t-teal {
    background: color-mix(in srgb, #4fd1c5 14%, transparent);
    color: #6fd6cc;
  }
  .opt .tk {
    overflow: hidden;
    font-family: var(--mono);
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tfc {
    min-width: 44px;
    padding: 3px var(--space-2);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--text-sm);
    text-align: center;
  }
  .tfc.on {
    background: color-mix(in srgb, var(--accent) 16%, var(--surface-2));
    color: var(--text);
  }
  .prov {
    overflow: hidden;
    color: var(--muted);
    font-size: var(--text-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .end {
    overflow: clip;
    color: var(--muted);
    font-family: var(--mono);
    font-size: var(--text-sm);
    text-align: right;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .opt.on .end {
    color: var(--text);
  }
  .opt.off .end {
    font-family: inherit;
  }
  .none {
    margin: var(--space-4) var(--space-2);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .live {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    margin-top: var(--space-3);
    padding: var(--space-3);
    border: none;
    border-radius: var(--radius-lg);
    background: var(--surface-2);
    color: var(--muted);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .live:hover {
    background: color-mix(in srgb, var(--text) 6%, var(--surface-2));
  }
  .live-ic {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: 32px;
    height: 32px;
    border-radius: var(--radius);
    background: var(--surface);
  }
  .live-t {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  .live-n {
    color: var(--text);
    font-size: var(--text-sm);
  }
  .live-d {
    font-size: var(--text-xs);
  }
  .foot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-top: var(--hairline) solid var(--border);
  }
  .dots {
    display: inline-flex;
    gap: 3px;
  }
  .dt {
    width: 12px;
    height: 4px;
    border-radius: 2px;
    background: var(--border);
  }
  .dt.on {
    background: var(--accent);
  }
  .cnt {
    flex: 1;
    min-width: 0;
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .cnt b {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .mono {
    font-family: var(--mono);
  }
  .ghost {
    height: 32px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius);
    background: none;
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .ghost:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .ghost:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .primary {
    height: 32px;
    padding: 0 var(--space-4);
    border: 0;
    border-radius: var(--radius);
    background: var(--accent);
    color: var(--accent-contrast);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    white-space: nowrap;
    cursor: pointer;
  }
  .primary:hover {
    background: var(--accent-hover);
  }
  @media (max-width: 640px) {
    .opt {
      grid-template-columns: 16px 30px minmax(0, 1fr) 56px;
    }
    .opt.nocb {
      grid-template-columns: 30px minmax(0, 1fr) 56px;
    }
    .prov,
    .end {
      display: none;
    }
    .share {
      margin-left: 0;
    }
  }
</style>
