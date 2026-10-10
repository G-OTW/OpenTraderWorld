<script>
  // Config editor for a single widget. Switches on widget type to show the right fields.
  // Edits a local draft of `item.config` and commits it via `onsave(config)` so the parent
  // controls persistence. Every widget shares an optional custom `title`.
  import Modal from '$lib/ui/Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { widgetByType, widgetDefaults, widgetVariant, widgetVariantOptions } from './registry.js';
  import { newsApi } from '$lib/modules/news/api.js';
  import { resourcesApi } from '$lib/modules/resources/api.js';
  import { portfoliosApi } from '$lib/modules/portfolios/api.js';
  import { timeApi } from '$lib/modules/time/api.js';
  import { watchlistsApi } from '$lib/modules/watchlists/api.js';
  import { fundamentalsApi, MACRO_CATEGORIES, STATEMENT_LINES } from '$lib/modules/fundamentals/api.js';
  import { quantApi, dsLabel } from '$lib/modules/quant/api.js';
  import DatasetChecklist from '$lib/modules/quant/DatasetChecklist.svelte';
  import { COMPANY_METRICS, DEFAULT_METRICS, compatibleDataset, minimumBars } from './insights.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let {
    open = $bindable(false),
    item = null, // { type, config }
    onsave = () => {}
  } = $props();

  const def = $derived(item ? widgetByType(item.type) : null);
  const activeVariant = $derived(widgetVariant(def, cfg));
  let cfg = $state({});

  // Option lists loaded on demand per widget type.
  let feeds = $state(null);
  let categories = $state(null);
  let portfolios = $state(null);
  let projects = $state(null);
  let watchlists = $state(null);
  let fundSeries = $state(null);
  let fundCompanies = $state(null);
  let quantDatasets = $state(null);
  let loadErr = $state('');

  $effect(() => {
    if (!open || !item) return;
    cfg = { ...item.config };
    loadErr = '';
    loadOptions(item.type);
  });

  async function loadOptions(type) {
    try {
      if (type === 'news' && feeds === null) feeds = await newsApi.listFeeds();
      if (type === 'resources' && categories === null) categories = await resourcesApi.listCategories();
      if (type === 'portfolios' && portfolios === null) portfolios = await portfoliosApi.list();
      if (type === 'time' && projects === null) projects = await timeApi.listProjects();
      if (type === 'watchlists' && watchlists === null) watchlists = await watchlistsApi.list();
      if (type === 'fundamentals' && fundSeries === null) {
        [fundSeries, fundCompanies] = await Promise.all([fundamentalsApi.series(), fundamentalsApi.companies()]);
      }
      if (type === 'quant') quantDatasets = await quantApi.datasets();
    } catch (e) {
      loadErr = e.message;
    }
  }

  function set(key, val) {
    cfg = { ...cfg, [key]: val };
  }

  function toggleList(key, value, fallback = [], max = 4) {
    const list = cfg[key] ?? fallback;
    set(key, list.includes(value) ? list.filter((v) => v !== value) : list.length < max ? [...list, value] : list);
  }

  function moveSeries(index, offset) {
    const list = [...(cfg.seriesIds ?? [])];
    [list[index], list[index + offset]] = [list[index + offset], list[index]];
    set('seriesIds', list);
  }

  const quantView = $derived(cfg.variant ?? 'count');
  const datasetOpts = $derived([
    { value: '', label: $t('dashboard.widgets.insights.firstDataset') },
    ...(quantDatasets ?? []).filter((d) => d.bar_count >= minimumBars(quantView, cfg.states ?? 2))
      .map((d) => ({ value: d.id, label: `${dsLabel(d)} · ${d.provider}` }))
  ]);
  const compatibleDatasets = $derived.by(() => {
    const first = (quantDatasets ?? []).find((d) => d.id === cfg.datasetIds?.[0]);
    return (quantDatasets ?? []).filter((d) => d.bar_count >= minimumBars('correlation') && (!first || cfg.datasetIds?.includes(d.id) || compatibleDataset(first, d)));
  });
  const metricLabel = (key) => key === 'price' ? $t('dashboard.widgets.fundamentals.price') : key === 'change_pct' ? $t('dashboard.widgets.fundamentals.day') : $t(`fundamentals.metric.${COMPANY_METRICS[key]?.[0]}`);

  // Widgets whose only shared setting is how many rows they show. Everything else about
  // them is chosen by the variant, so they need no bespoke panel.
  const LIST_TYPES = [
    'goals', 'todos', 'calendar', 'mailbox', 'routines', 'prompts',
    'journal-stats', 'agent-stats', 'remindme-stats', 'editor', 'automator', 'webhooks',
    'histdata', 'backtest', 'findb', 'histviz', 'mportfolios', 'taxcalc'
  ];

  // A widget type changes what it answers (for example, "Top movers" instead of
  // "Quote table"), not how it is painted. Its documented defaults replace only
  // the data-specific keys; a user title and chosen cell height remain theirs.
  function setVariant(id) {
    const { title, height } = cfg;
    cfg = { ...widgetDefaults(item.type, id), ...(title !== undefined ? { title } : {}), ...(height !== undefined ? { height } : {}) };
  }

  // Dropdown works on strings; '' is the "none / all" row and stores back as null.
  const idOpts = (list, allLabel) => [
    { value: '', label: allLabel },
    ...(list ?? []).map((x) => ({ value: String(x.id), label: x.name }))
  ];
  const setId = (key) => (v) => set(key, v || null);

  // Fundamentals pickers: series grouped by category, statement lines grouped by statement.
  const fundView = $derived(cfg.variant ?? 'series');
  const seriesOpts = $derived.by(() => {
    const out = [{ value: '', label: $t('dashboard.widgets.config.firstSeries') }];
    for (const cat of MACRO_CATEGORIES) {
      const rows = (fundSeries ?? []).filter((x) => x.category === cat);
      if (!rows.length) continue;
      out.push({ value: `h:${cat}`, label: $t(`fundamentals.macro.cat.${cat}`), header: true });
      for (const x of rows) out.push({ value: x.id, label: `${x.title} · ${x.provider}` });
    }
    return out;
  });
  const companyOpts = $derived([
    { value: '', label: $t('dashboard.widgets.config.firstCompany') },
    ...(fundCompanies ?? []).map((c) => ({ value: c.ticker, label: `${c.ticker} · ${c.name}` }))
  ]);
  const lineOpts = $derived.by(() => {
    const out = [];
    const seen = new Set();
    for (const [kind, lines] of Object.entries(STATEMENT_LINES)) {
      out.push({ value: `h:${kind}`, label: $t(`fundamentals.fin.${kind}`), header: true });
      for (const l of lines) {
        if (seen.has(l)) continue;
        seen.add(l);
        out.push({ value: l, label: $t(`fundamentals.line.${l}`) });
      }
    }
    return out;
  });

  function submit() {
    onsave({ ...cfg });
    open = false;
  }
</script>

<Modal bind:open size="sm" title={$t('dashboard.widgets.config.title', { label: def?.label ?? $t('dashboard.widgets.config.widget') })}>
  <div class="form">
    <ErrorText error={loadErr} compact />

    <label>
      <span>{$t('dashboard.widgets.config.titleField')} <small>{$t('dashboard.widgets.config.optional')}</small></span>
      <input value={cfg.title ?? ''} oninput={(e) => set('title', e.currentTarget.value)}
        placeholder={def?.label} />
    </label>

    <div class="fld">
      <span>{$t('dashboard.widgets.config.height')}</span>
      <Dropdown
        value={cfg.height ?? 'standard'}
        onpick={(v) => set('height', v)}
        ariaLabel={$t('dashboard.widgets.config.height')}
        options={[
          { value: 'title', label: $t('dashboard.widgets.config.titleHeight') },
          { value: 'compact', label: $t('dashboard.widgets.config.compact') },
          { value: 'standard', label: $t('dashboard.widgets.config.standard') },
          { value: 'tall', label: $t('dashboard.widgets.config.tall') }
        ]}
      />
    </div>

    {#if (def?.variants?.length ?? 0) > 1}
      <div class="fld">
        <span>Widget type</span>
        <Dropdown
          value={activeVariant?.id ?? ''}
          onpick={setVariant}
          ariaLabel="Widget type"
          options={widgetVariantOptions(def)}
        />
        <small class="type-help">Changes the data presentation and resets its data-specific defaults.</small>
      </div>
    {/if}

    {#if item?.type === 'text'}
      <label>
        <span>{$t('dashboard.widgets.config.text')}</span>
        <textarea rows="6" value={cfg.body ?? ''} oninput={(e) => set('body', e.currentTarget.value)}
          placeholder={$t('dashboard.widgets.config.textPlaceholder')}></textarea>
      </label>
      <div class="row">
        <div class="fld">
          <span>Size</span>
          <Dropdown
            value={cfg.size ?? 'base'}
            onpick={(v) => set('size', v)}
            ariaLabel="Size"
            options={[
              { value: 'xs', label: 'Extra small' },
              { value: 'sm', label: 'Small' },
              { value: 'base', label: 'Normal' },
              { value: 'lg', label: 'Large' },
              { value: 'xl', label: 'Extra large' }
            ]}
          />
        </div>
        <div class="fld">
          <span>Style</span>
          <Dropdown
            value={cfg.textStyle ?? 'normal'}
            onpick={(v) => set('textStyle', v)}
            ariaLabel="Style"
            options={[
              { value: 'normal', label: 'Normal' },
              { value: 'bold', label: 'Bold' },
              { value: 'italic', label: 'Italic' },
              { value: 'bold-italic', label: 'Bold italic' }
            ]}
          />
        </div>
      </div>
      <div class="row">
        <div class="fld">
          <span>Color</span>
          <Dropdown
            value={cfg.color ?? 'muted'}
            onpick={(v) => set('color', v)}
            ariaLabel="Color"
            options={[
              { value: 'muted', label: 'Default' },
              { value: 'text', label: 'Strong' },
              { value: 'accent', label: 'Accent', color: 'var(--accent)' },
              { value: 'green', label: 'Green', color: 'var(--green)' },
              { value: 'red', label: 'Red', color: 'var(--red)' },
              { value: 'amber', label: 'Amber', color: 'var(--amber)' }
            ]}
          />
        </div>
        <div class="fld">
          <span>Highlight</span>
          <Dropdown
            value={cfg.highlight ?? 'none'}
            onpick={(v) => set('highlight', v)}
            ariaLabel="Highlight"
            options={[
              { value: 'none', label: 'None' },
              { value: 'accent', label: 'Accent', color: 'var(--accent)' },
              { value: 'green', label: 'Green', color: 'var(--green)' },
              { value: 'red', label: 'Red', color: 'var(--red)' },
              { value: 'amber', label: 'Amber', color: 'var(--amber)' }
            ]}
          />
        </div>
      </div>

    {:else if item?.type === 'news'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.feed')}</span>
        {#if feeds === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <Dropdown
            value={cfg.feed_id == null ? '' : String(cfg.feed_id)}
            onpick={setId('feed_id')}
            ariaLabel={$t('dashboard.widgets.config.feed')}
            options={idOpts(feeds, $t('dashboard.widgets.config.allFeeds'))}
          />
        {/if}
      </div>
      <label>
        <span>{$t('dashboard.widgets.config.maxItems')}</span>
        <input type="number" min="1" max="50" value={cfg.limit ?? 10}
          oninput={(e) => set('limit', Math.max(1, Math.min(50, +e.currentTarget.value || 10)))} />
      </label>
      <div class="fld">
        <span>{$t('dashboard.widgets.config.layout')}</span>
        <Dropdown
          value={cfg.view ?? 'list'}
          onpick={(v) => set('view', v)}
          ariaLabel={$t('dashboard.widgets.config.layout')}
          options={[
            { value: 'list', label: $t('dashboard.widgets.config.list') },
            { value: 'grid', label: $t('dashboard.widgets.config.grid') }
          ]}
        />
      </div>

    {:else if item?.type === 'resources'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.category')}</span>
        {#if categories === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <Dropdown
            value={cfg.category_id == null ? '' : String(cfg.category_id)}
            onpick={setId('category_id')}
            ariaLabel={$t('dashboard.widgets.config.category')}
            options={idOpts(categories, $t('dashboard.widgets.config.allCategories'))}
          />
        {/if}
      </div>
      <label>
        <span>{$t('dashboard.widgets.config.maxItems')}</span>
        <input type="number" min="1" max="50" value={cfg.limit ?? 10}
          oninput={(e) => set('limit', Math.max(1, Math.min(50, +e.currentTarget.value || 10)))} />
      </label>

    {:else if item?.type === 'portfolios'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.portfolio')}</span>
        {#if portfolios === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <Dropdown
            value={cfg.portfolio_id == null ? '' : String(cfg.portfolio_id)}
            onpick={setId('portfolio_id')}
            ariaLabel={$t('dashboard.widgets.config.portfolio')}
            options={idOpts(portfolios, $t('dashboard.widgets.config.firstPortfolio'))}
          />
        {/if}
      </div>
      <label>
        <span>{$t('dashboard.widgets.config.maxItems')}</span>
        <input type="number" min="1" max="50" value={cfg.limit ?? 6}
          oninput={(e) => set('limit', Math.max(1, Math.min(50, +e.currentTarget.value || 6)))} />
      </label>

    {:else if item?.type === 'time'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.project')}</span>
        {#if projects === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <Dropdown
            value={cfg.project_id == null ? '' : String(cfg.project_id)}
            onpick={setId('project_id')}
            ariaLabel={$t('dashboard.widgets.config.project')}
            options={idOpts(projects, $t('dashboard.widgets.config.allProjects'))}
          />
        {/if}
      </div>
      <div class="fld">
        <span>Timer scope</span>
        <Dropdown
          value={cfg.scope ?? 'all'}
          onpick={(v) => set('scope', v)}
          ariaLabel="Timer scope"
          options={[
            { value: 'all', label: 'All projects' },
            { value: 'running', label: 'Running only' }
          ]}
        />
      </div>
      <label>
        <span>{$t('dashboard.widgets.config.maxItems')}</span>
        <input type="number" min="1" max="50" value={cfg.limit ?? 8}
          oninput={(e) => set('limit', Math.max(1, Math.min(50, +e.currentTarget.value || 8)))} />
      </label>

    {:else if item?.type === 'watchlists'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.watchlist')}</span>
        {#if watchlists === null}
          <span class="muted">{$t('common.loading')}</span>
        {:else}
          <Dropdown
            value={cfg.watchlist_id == null ? '' : String(cfg.watchlist_id)}
            onpick={setId('watchlist_id')}
            ariaLabel={$t('dashboard.widgets.config.watchlist')}
            options={idOpts(watchlists, $t('dashboard.widgets.config.firstWatchlist'))}
          />
        {/if}
      </div>
      <label>
        <span>{$t('dashboard.widgets.config.maxItems')}</span>
        <input type="number" min="1" max="50" value={cfg.limit ?? 10}
          oninput={(e) => set('limit', Math.max(1, Math.min(50, +e.currentTarget.value || 10)))} />
      </label>

    {:else if item?.type === 'fundamentals'}
      {#if fundSeries === null}
        <span class="muted">{$t('common.loading')}</span>
      {:else if fundView === 'series'}
        <div class="fld">
          <span>{$t('dashboard.widgets.config.series')}</span>
          <Dropdown value={cfg.series ?? ''} onpick={(v) => set('series', v || null)} ariaLabel={$t('dashboard.widgets.config.series')} options={seriesOpts} />
        </div>
        <div class="row">
          <div class="fld">
            <span>{$t('fundamentals.macro.transform')}</span>
            <Dropdown
              value={cfg.tf ?? 'level'}
              onpick={(v) => set('tf', v)}
              ariaLabel={$t('fundamentals.macro.transform')}
              options={['level', 'yoy', 'pop', 'diff', 'index'].map((v) => ({ value: v, label: $t(`fundamentals.macro.tf.${v}`) }))}
            />
          </div>
          <div class="fld">
            <span>{$t('fundamentals.macro.range')}</span>
            <Dropdown
              value={cfg.range ?? '10y'}
              onpick={(v) => set('range', v)}
              ariaLabel={$t('fundamentals.macro.range')}
              options={['1y', '5y', '10y', 'max'].map((v) => ({ value: v, label: $t(`dashboard.widgets.config.range.${v}`) }))}
            />
          </div>
        </div>
      {:else if fundView === 'board'}
        <div class="fld">
          <span>{$t('dashboard.widgets.config.category')}</span>
          <Dropdown
            value={cfg.category ?? ''}
            onpick={(v) => { set('category', v); set('seriesIds', []); }}
            ariaLabel={$t('dashboard.widgets.config.category')}
            options={[{ value: '', label: $t('fundamentals.macro.allCategories') }, ...MACRO_CATEGORIES.map((c) => ({ value: c, label: $t(`fundamentals.macro.cat.${c}`) }))]}
          />
        </div>
        <fieldset class="choices"><legend>{$t('dashboard.widgets.insights.boardSeries')}</legend>
          {#each (fundSeries ?? []).filter((s) => !cfg.category || s.category === cfg.category) as s (s.id)}
            <label class="check"><input type="checkbox" checked={cfg.seriesIds?.includes(s.id) ?? false}
              onchange={() => toggleList('seriesIds', s.id, [], 30)} /><span>{s.title}</span></label>
          {/each}
        </fieldset>
        {#if cfg.seriesIds?.length}<ol class="order-list">
          {#each cfg.seriesIds as id, i (id)}<li><span>{fundSeries?.find((s) => s.id === id)?.title ?? id}</span>
            <button type="button" disabled={i === 0} aria-label={$t('dashboard.widgets.insights.moveUp')} onclick={() => moveSeries(i, -1)}>↑</button>
            <button type="button" disabled={i === cfg.seriesIds.length - 1} aria-label={$t('dashboard.widgets.insights.moveDown')} onclick={() => moveSeries(i, 1)}>↓</button></li>{/each}
        </ol>{/if}
      {:else if ['company', 'line', 'valuation'].includes(fundView)}
        <div class="fld">
          <span>{$t('dashboard.widgets.config.company')}</span>
          <Dropdown value={cfg.ticker ?? ''} onpick={(v) => set('ticker', v || null)} ariaLabel={$t('dashboard.widgets.config.company')} options={companyOpts} />
        </div>
        {#if fundView === 'line'}
          <div class="row">
            <div class="fld">
              <span>{$t('dashboard.widgets.config.line')}</span>
              <Dropdown value={cfg.line ?? 'revenue'} onpick={(v) => { set('line', v); if (STATEMENT_LINES.balance.includes(v) && cfg.freq === 'ttm') set('freq', 'quarterly'); }} ariaLabel={$t('dashboard.widgets.config.line')} options={lineOpts} />
            </div>
            <div class="fld">
              <span>{$t('dashboard.widgets.config.freq')}</span>
              <Dropdown
                value={cfg.freq ?? 'quarterly'}
                onpick={(v) => set('freq', v)}
                ariaLabel={$t('dashboard.widgets.config.freq')}
                options={[...['quarterly', 'annual'].map((v) => ({ value: v, label: $t(`fundamentals.fin.${v}`) })), ...(!STATEMENT_LINES.balance.includes(cfg.line ?? 'revenue') ? [{ value: 'ttm', label: $t('dashboard.widgets.insights.ttm') }] : [])]}
              />
            </div>
          </div>
        {/if}
        {#if fundView === 'company'}<fieldset class="choices"><legend>{$t('dashboard.widgets.insights.fourMetrics')}</legend>
          {#each Object.keys(COMPANY_METRICS) as key (key)}<label class="check"><input type="checkbox"
            checked={(cfg.metrics ?? DEFAULT_METRICS).includes(key)} disabled={!(cfg.metrics ?? DEFAULT_METRICS).includes(key) && (cfg.metrics ?? DEFAULT_METRICS).length >= 4}
            onchange={() => toggleList('metrics', key, DEFAULT_METRICS)} /><span>{metricLabel(key)}</span></label>{/each}
        </fieldset>{/if}
        {#if fundView === 'valuation'}<fieldset class="choices"><legend>{$t('dashboard.widgets.insights.choosePeers')}</legend>
          {#each (fundCompanies ?? []).filter((c) => c.ticker !== cfg.ticker) as c (c.ticker)}<label class="check"><input type="checkbox" checked={cfg.peerTickers?.includes(c.ticker) ?? false} onchange={() => toggleList('peerTickers', c.ticker, [], 29)} /><span>{c.ticker} · {c.name}</span></label>{/each}
        </fieldset><small class="type-help">{$t('dashboard.widgets.insights.peerDefaults')}</small>{/if}
      {:else if ['companies', 'filings', 'earnings'].includes(fundView)}
        <div class="fld">
          <span>{$t('dashboard.widgets.config.show')}</span>
          <Dropdown
            value={cfg.scope ?? (fundView === 'earnings' ? 'followed' : 'all')}
            onpick={(v) => set('scope', v)}
            ariaLabel={$t('dashboard.widgets.config.show')}
            options={[
              { value: 'all', label: $t('dashboard.widgets.config.allCompanies') },
              { value: 'followed', label: $t('dashboard.widgets.config.followedCompanies') }
            ]}
          />
        </div>
        {#if fundView === 'companies'}<fieldset class="choices"><legend>{$t('dashboard.widgets.insights.fourColumns')}</legend>
          {#each ['price', 'change_pct', ...Object.keys(COMPANY_METRICS)] as key (key)}<label class="check"><input type="checkbox"
            checked={(cfg.columns ?? ['price', 'change_pct', 'pe', 'market_cap']).includes(key)}
            disabled={!(cfg.columns ?? ['price', 'change_pct', 'pe', 'market_cap']).includes(key) && (cfg.columns ?? ['price', 'change_pct', 'pe', 'market_cap']).length >= 4}
            onchange={() => toggleList('columns', key, ['price', 'change_pct', 'pe', 'market_cap'])} /><span>{metricLabel(key)}</span></label>{/each}
        </fieldset>{/if}
        {#if fundView === 'filings'}<div class="fld"><span>{$t('dashboard.widgets.insights.filingType')}</span>
          <Dropdown value={cfg.form ?? ''} onpick={(v) => set('form', v)} ariaLabel={$t('dashboard.widgets.insights.filingType')}
            options={[{ value: '', label: $t('dashboard.widgets.insights.allFilings') }, ...['10-K', '10-Q', '8-K', '20-F', '6-K', 'DEF 14A'].map((v) => ({ value: v, label: v }))]} />
        </div>{/if}
      {/if}
      {#if ['board', 'companies', 'filings', 'earnings', 'valuation'].includes(fundView)}
        <label>
          <span>{$t('dashboard.widgets.config.maxItems')}</span>
          <input type="number" min="1" max="30" value={cfg.limit ?? 8}
            oninput={(e) => set('limit', Math.max(1, Math.min(30, +e.currentTarget.value || 8)))} />
        </label>
      {/if}

    {:else if item?.type === 'quant'}
      {#if quantView !== 'count'}
        {#if quantDatasets === null}<p class="muted">{$t('common.loading')}</p>
        {:else if quantView === 'correlation'}
          <div class="fld"><span>{$t('dashboard.widgets.insights.basketDatasets')}</span>
            <DatasetChecklist datasets={compatibleDatasets} max={20}
              bind:value={() => cfg.datasetIds ?? [], (value) => set('datasetIds', value)} />
            <small class="type-help">{$t('dashboard.widgets.insights.compatibleBasket')}</small>
          </div>
        {:else}<div class="fld"><span>{$t('quant.page.dataset')}</span>
          <Dropdown value={cfg.datasetId ?? ''} onpick={(v) => set('datasetId', v || null)} ariaLabel={$t('quant.page.dataset')} options={datasetOpts} />
        </div>{/if}
        {#if quantView === 'risk'}<div class="fld"><span>{$t('dashboard.widgets.insights.confidence')}</span>
          <Dropdown value={String(cfg.confidence ?? 0.95)} onpick={(v) => set('confidence', Number(v))} ariaLabel={$t('dashboard.widgets.insights.confidence')}
            options={[0.9, 0.95, 0.99].map((v) => ({ value: String(v), label: `${v * 100}%` }))} />
        </div>{/if}
        {#if quantView === 'seasonality'}<div class="fld"><span>{$t('quant.seasonality.metric')}</span>
          <Dropdown value={cfg.metric ?? 'return'} onpick={(v) => set('metric', v)} ariaLabel={$t('quant.seasonality.metric')}
            options={['return', 'volatility', 'volume', 'range'].map((v) => ({ value: v, label: $t(`dashboard.widgets.insights.season.${v}`) }))} />
        </div>{/if}
        {#if quantView === 'volatility'}<div class="fld"><span>{$t('quant.vol.windowBars')}</span>
          <Dropdown value={String(cfg.window ?? 21)} onpick={(v) => set('window', Number(v))} ariaLabel={$t('quant.vol.windowBars')}
            options={[5, 10, 21, 63, 126, 252].map((v) => ({ value: String(v), label: String(v) }))} />
        </div>{/if}
        {#if quantView === 'regime'}<div class="fld"><span>{$t('quant.regimes.states')}</span>
          <Dropdown value={String(cfg.states ?? 2)} onpick={(v) => set('states', Number(v))} ariaLabel={$t('quant.regimes.states')}
            options={[2, 3, 4].map((v) => ({ value: String(v), label: String(v) }))} />
        </div>{/if}
        <a href="/histdata">{$t('dashboard.widgets.insights.manageDatasets')}</a>
      {/if}

    {:else if item?.type === 'economics'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.importance')}</span>
        <Dropdown
          value={cfg.importance ?? '-1,0,1'}
          onpick={(v) => set('importance', v)}
          ariaLabel={$t('dashboard.widgets.config.importance')}
          options={[
            { value: '-1,0,1', label: $t('dashboard.widgets.config.importanceAll') },
            { value: '0,1', label: $t('dashboard.widgets.config.importanceMedHigh') },
            { value: '1', label: $t('dashboard.widgets.config.importanceHigh') }
          ]}
        />
      </div>
      <label>
        <span>{$t('dashboard.widgets.config.countries')} <small>{$t('dashboard.widgets.config.optional')}</small></span>
        <input value={cfg.countries ?? ''} oninput={(e) => set('countries', e.currentTarget.value)}
          placeholder={$t('dashboard.widgets.config.countriesPlaceholder')} />
      </label>

    {:else if item?.type === 'wealth'}
      <div class="fld">
        <span>{$t('dashboard.widgets.config.window')}</span>
        <Dropdown
          value={String(cfg.months ?? 12)}
          onpick={(v) => set('months', +v)}
          ariaLabel={$t('dashboard.widgets.config.window')}
          options={[
            { value: '6', label: $t('dashboard.widgets.config.months', { count: 6 }) },
            { value: '12', label: $t('dashboard.widgets.config.months', { count: 12 }) },
            { value: '24', label: $t('dashboard.widgets.config.months', { count: 24 }) }
          ]}
        />
      </div>

    {:else if item?.type === 'subscriptions'}
      <label>
        <span>{$t('dashboard.widgets.config.maxRenewals')}</span>
        <input type="number" min="1" max="20" value={cfg.limit ?? 5}
          oninput={(e) => set('limit', Math.max(1, Math.min(20, +e.currentTarget.value || 5)))} />
      </label>

    {:else if LIST_TYPES.includes(item?.type)}
      <label>
        <span>{$t('dashboard.widgets.config.maxItems')}</span>
        <input type="number" min="1" max="50" value={cfg.limit ?? 8}
          oninput={(e) => set('limit', Math.max(1, Math.min(50, +e.currentTarget.value || 8)))} />
      </label>

      {#if item?.type === 'calendar'}
        <div class="fld">
          <span>Time horizon</span>
          <Dropdown
            value={String(cfg.days ?? 7)}
            onpick={(v) => set('days', +v)}
            ariaLabel="Time horizon"
            options={[
              { value: '7', label: 'Next 7 days' },
              { value: '14', label: 'Next 14 days' },
              { value: '30', label: 'Next 30 days' }
            ]}
          />
        </div>
      {/if}

    {:else}
      <p class="muted">{$t('dashboard.widgets.config.noOptions')}</p>
    {/if}
  </div>

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={submit}>{$t('common.save')}</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  label,
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    font-size: var(--text-base);
    color: var(--dim);
  }
  small {
    font-weight: var(--fw-normal);
  }
  .muted {
    color: var(--dim);
    font-size: var(--text-base);
  }
  /* Two short pickers per line: the note's look is four one-word choices, and a
     column of four full-width dropdowns pushes Save off the modal. */
  .row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .type-help {
    color: var(--dim);
    font-size: var(--text-xs);
    line-height: 1.35;
  }
  .choices { display: flex; flex-direction: column; gap: 7px; max-height: 220px; overflow-y: auto; padding: 10px; border: var(--hairline) solid var(--border); border-radius: var(--radius-sm); }
  .choices legend { color: var(--muted); font-size: var(--text-sm); }
  .check { flex-direction: row; align-items: center; color: var(--text); font-size: var(--text-sm); }
  .order-list { padding-left: 20px; margin: 0; }
  .order-list li { display: flex; align-items: center; gap: 5px; margin: 5px 0; font-size: var(--text-sm); }
  .order-list span { flex: 1; min-width: 0; }
  .order-list button { border: 0; background: var(--surface-2); color: var(--text); width: 26px; height: 26px; cursor: pointer; }
</style>
