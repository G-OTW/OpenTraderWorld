<script>
  // Fundamentals: Macro. A searchable series catalog on the left; picked series plot
  // together on the right with a transform (computed on read), a range and recession
  // shading. Saved views recall a set of series in one click. Below, the Treasury curve
  // and central-bank policy rates; every block can be hidden, folded or reordered.
  // Series are stored by the backend: adding one fetches its history in the background,
  // and the list polls while anything is still fetching.
  import Shell from '$lib/modules/fundamentals/Shell.svelte';
  import { page } from '$app/stores';
  import Section from '$lib/modules/fundamentals/Section.svelte';
  import Chart from '$lib/modules/fundamentals/Chart.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { fundamentalsApi, MACRO_CATEGORIES, RECESSIONS, transform, sliceRange, fmtNum, fmtPct } from '$lib/modules/fundamentals/api.js';
  import { prefs, visibleSections } from '$lib/modules/fundamentals/prefs.svelte.js';
  import { errorText } from '$lib/modules/fundamentals/deferred.js';
  import { t } from '$lib/i18n';

  const MAX_PICKED = 4;
  const TRANSFORMS = ['level', 'yoy', 'pop', 'diff', 'index'];
  const RANGES = ['1y', '5y', '10y', 'max'];
  const STYLES = ['line', 'area', 'bar'];
  const SECTIONS = ['chart', 'curve', 'banks'];

  // Derived, not a copy: a reset swaps the whole object.
  const m = $derived(prefs.macro);
  let catalog = $state([]);
  let loaded = $state(false);
  let loadError = $state('');
  let curve = $state(null);
  let curveError = $state('');
  let banks = $state([]);
  let banksError = $state('');
  let q = $state('');
  let category = $state('');
  let mobileCatalogOpen = $state(false);
  let obs = $state({});
  let naming = $state(false);
  let viewName = $state('');
  let linkedApplied = false;

  async function loadCatalog() {
    try {
      const rows = await fundamentalsApi.series();
      // A series that just finished fetching gets its observations reloaded.
      for (const r of rows) {
        const before = catalog.find((c) => c.id === r.id);
        if (before && before.status !== 'ok' && r.status === 'ok') {
          const { [r.id]: _, ...rest } = obs;
          obs = rest;
        }
      }
      catalog = rows;
      const linked = $page.url.searchParams.get('series');
      if (!linkedApplied && rows.some((s) => s.id === linked)) {
        linkedApplied = true;
        m.picked = [linked];
        const tf = $page.url.searchParams.get('tf');
        const range = $page.url.searchParams.get('range');
        if (TRANSFORMS.includes(tf)) m.tf = tf;
        if (RANGES.includes(range)) m.range = range;
      }
      loadError = '';
    } catch (e) {
      loadError = e.message;
    }
    loaded = true;
  }

  $effect(() => {
    loadCatalog();
    fundamentalsApi
      .yieldCurve()
      .then((c) => (curve = c))
      .catch((e) => (curveError = e.message));
    fundamentalsApi
      .centralBanks()
      .then((b) => (banks = b))
      .catch((e) => (banksError = errorText($t, e)));
  });

  const fetching = $derived(catalog.some((s) => s.status === 'pending'));
  $effect(() => {
    if (!fetching) return;
    const timer = setInterval(loadCatalog, 3000);
    return () => clearInterval(timer);
  });

  $effect(() => {
    for (const id of m.picked) {
      const meta = catalog.find((s) => s.id === id);
      if (meta?.status === 'ok' && !obs[id]) fundamentalsApi.observations(id).then((o) => (obs = { ...obs, [id]: o }));
    }
  });

  // ── Adding series ─────────────────────────────────────────────────────────
  // Every macro provider, with the code format its own site shows (typed codes are
  // checked against the provider before they are stored).
  const PROVIDERS = [
    { value: 'fred', label: 'FRED', hint: 'CPIAUCSL' },
    { value: 'ecb', label: 'ECB', hint: 'FLOW/KEY: HICP/M.U2.N.000000.4D0.ANR' },
    { value: 'eurostat', label: 'Eurostat', hint: 'DATASET/KEY: une_rt_m/M.SA.TOTAL.PC_ACT.T.EA21' },
    { value: 'bis', label: 'BIS', hint: 'FLOW/KEY: WS_CBPOL/M.US' },
    { value: 'oecd', label: 'OECD', hint: 'AGENCY,DSD@FLOW,/KEY' },
    { value: 'imf', label: 'IMF', hint: 'AGENCY,FLOW/KEY: IMF.RES,WEO/USA.NGDP_RPCH.A' },
    { value: 'worldbank', label: 'World Bank', hint: 'COUNTRY/INDICATOR: USA/NY.GDP.MKTP.KD.ZG' },
    { value: 'bls', label: 'BLS', hint: 'CUUR0000SA0' },
    { value: 'bea', label: 'BEA', hint: 'DATASET/TABLE/LINE/FREQ: NIPA/T10101/1/Q' },
    { value: 'eia', label: 'EIA', hint: 'PET.WCESTUS1.W' },
    { value: 'census', label: 'US Census', hint: 'PROGRAM/CATEGORY/TYPE/SA: resconst/APERMITS/TOTAL/yes' },
    { value: 'cftc', label: 'CFTC (COT)', hint: 'contract market code: 13874A, or search a market name' },
    { value: 'treasury', label: 'US Treasury', hint: 'debt_to_penny' }
  ];
  const hint = $derived(PROVIDERS.find((p) => p.value === addProvider)?.hint ?? '');
  let adding = $state(false);
  let addProvider = $state('fred');
  let addQ = $state('');
  let addCategory = $state('other');
  let hits = $state([]);
  let searching = $state(false);
  let addError = $state('');
  let busy = $state('');

  async function search(e) {
    e?.preventDefault();
    searching = true;
    addError = '';
    try {
      hits = await fundamentalsApi.searchSeries(addProvider, addQ);
    } catch (err) {
      addError = err.message;
      hits = [];
    }
    searching = false;
  }

  async function add(provider, code, cat = addCategory) {
    busy = `${provider}:${code}`;
    addError = '';
    try {
      await fundamentalsApi.addSeries(provider, code, cat);
      await loadCatalog();
    } catch (err) {
      addError = err.message;
    }
    busy = '';
  }

  async function addStarter() {
    busy = 'starter';
    addError = '';
    const errors = [];
    const skipped = new Set();
    for (const s of await fundamentalsApi.starter()) {
      if (skipped.has(s.provider) || catalog.some((c) => c.id === `${s.provider}:${s.code}`)) continue;
      try {
        await fundamentalsApi.addSeries(s.provider, s.code, s.category);
      } catch (err) {
        errors.push(err.message);
        // A missing connector would fail every other series of that provider too.
        if (err.status === 400) skipped.add(s.provider);
      }
    }
    await loadCatalog();
    addError = errors.join(' · ');
    busy = '';
  }

  const filtered = $derived(
    catalog.filter(
      (s) =>
        (!category || s.category === category) &&
        (!q || `${s.title} ${s.id} ${s.provider} ${s.country}`.toLowerCase().includes(q.toLowerCase()))
    )
  );
  // Grouped by category so the list reads as a table of contents, not a wall of rows.
  const groups = $derived(MACRO_CATEGORIES.map((c) => [c, filtered.filter((s) => s.category === c)]).filter(([, l]) => l.length));

  const pickedMeta = $derived(m.picked.map((id) => catalog.find((s) => s.id === id)).filter(Boolean));
  const dual = $derived(m.tf === 'level' && new Set(pickedMeta.map((s) => s.unit)).size > 1);

  const chartSeries = $derived(
    pickedMeta
      .filter((s) => obs[s.id])
      .map((s) => ({
        name: `${s.title} (${s.country})`,
        type: m.chart === 'bar' ? 'bar' : 'line',
        area: m.chart === 'area',
        data: sliceRange(transform(obs[s.id], m.tf === 'level' ? '' : m.tf, s.freq), m.range),
        yAxisIndex: dual && s.unit !== pickedMeta[0].unit ? 1 : 0
      }))
  );

  const sections = $derived(visibleSections('macro', SECTIONS));

  function toggle(id) {
    if (m.picked.includes(id)) m.picked = m.picked.filter((x) => x !== id);
    else if (m.picked.length < MAX_PICKED) m.picked = [...m.picked, id];
    else m.picked = [...m.picked.slice(1), id];
  }

  function applyView(v) {
    m.picked = [...v.picked];
    m.tf = v.tf;
    m.range = v.range;
  }

  function saveView() {
    const name = viewName.trim();
    if (!name) return;
    m.views = [...m.views.filter((v) => v.name !== name), { name, picked: [...m.picked], tf: m.tf, range: m.range }];
    viewName = '';
    naming = false;
  }

  const change = (s) => (s.last == null || s.prev == null ? null : s.last - s.prev);
</script>

{#snippet chartBlock()}
  <Section key="macro.chart" title={$t('fundamentals.macro.chart')} description={$t('fundamentals.macro.chartDesc')}>
    <div class="views">
      <span class="fd-label">{$t('fundamentals.macro.views')}</span>
      {#if naming}
        <form class="fd-row" onsubmit={(e) => (e.preventDefault(), saveView())}>
          <input class="vname" placeholder={$t('fundamentals.macro.viewName')} bind:value={viewName} />
          <Button size="sm" type="submit" variant="primary">{$t('fundamentals.save')}</Button>
          <Button size="sm" variant="ghost" onclick={() => (naming = false)}>{$t('fundamentals.cancel')}</Button>
        </form>
      {:else}
        <Button size="sm" variant="ghost" icon="plus" onclick={() => (naming = true)} disabled={!m.picked.length}>{$t('fundamentals.macro.saveView')}</Button>
      {/if}
      <div class="vtrack">
        {#each m.views as v (v.name)}
          <button class="chip" onclick={() => applyView(v)}>{v.name}</button>
        {/each}
      </div>
    </div>

    <div class="fd-toolbar">
      <div class="fd-row">
        <span class="fd-label">{$t('fundamentals.macro.transform')}</span>
        {#each TRANSFORMS as x (x)}
          <button class="chip" class:active={m.tf === x} onclick={() => (m.tf = x)}>{$t(`fundamentals.macro.tf.${x}`)}</button>
        {/each}
      </div>
      <div class="fd-row">
        <span class="fd-label">{$t('fundamentals.macro.range')}</span>
        {#each RANGES as r (r)}
          <button class="chip" class:active={m.range === r} onclick={() => (m.range = r)}>{r.toUpperCase()}</button>
        {/each}
      </div>
      <div class="fd-row">
        <span class="fd-label">{$t('fundamentals.macro.style')}</span>
        {#each STYLES as s (s)}
          <button class="chip" class:active={m.chart === s} onclick={() => (m.chart = s)}>{$t(`fundamentals.macro.style.${s}`)}</button>
        {/each}
        <label class="fd-muted check"><input type="checkbox" bind:checked={m.shade} /> {$t('fundamentals.macro.recessions')}</label>
      </div>
    </div>

    {#if pickedMeta.length}
      <div class="picked">
        {#each pickedMeta as s (s.id)}
          <span class="pill">
            <span class="pname">{s.title}</span>
            <span class="fd-muted">{s.unit} · {s.provider}</span>
            <button onclick={() => toggle(s.id)} aria-label={$t('fundamentals.remove')}><Icon name="x" size={12} /></button>
          </span>
        {/each}
      </div>
      <Chart series={chartSeries} shade={m.shade ? RECESSIONS : []} dualAxis={dual} height={380} />
      <p class="fd-note">{$t('fundamentals.macro.pickHint', { n: MAX_PICKED })}{dual ? ` ${$t('fundamentals.macro.dualHint')}` : ''}</p>
    {:else}
      <p class="fd-muted empty">{$t('fundamentals.macro.empty')}</p>
    {/if}
  </Section>
{/snippet}

{#snippet curveBlock()}
  <Section key="macro.curve" title={$t('fundamentals.macro.curve')} description={curve ? `${$t('fundamentals.macro.curveDesc')} ${$t('fundamentals.asOf', { date: curve.as_of })}` : $t('fundamentals.macro.curveDesc')} source="US Treasury">
    {#if curveError}
      <ErrorText error={curveError} />
    {:else if curve}
      <Chart
        categories={curve.tenors}
        series={curve.curves.map((c) => ({ name: $t(`fundamentals.macro.curve.${c.label}`), data: c.values }))}
        height={260}
        yFormat={(v) => `${Number(v).toFixed(2)}%`}
      />
    {/if}
  </Section>
{/snippet}

{#snippet banksBlock()}
  <Section key="macro.banks" title={$t('fundamentals.macro.banks')} description={$t('fundamentals.macro.banksDesc')} source="BIS">
    {#if banksError}<ErrorText error={banksError} />{/if}
    <div class="fd-scroll">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('fundamentals.macro.bank')}</th>
            <th class="num">{$t('fundamentals.macro.rate')}</th>
            <th class="num">{$t('fundamentals.macro.lastChange')}</th>
            <th>{$t('fundamentals.macro.lastChangeDate')}</th>
            <th>{$t('fundamentals.macro.asOf')}</th>
          </tr>
        </thead>
        <tbody>
          {#each banks as b (b.bank)}
            <tr>
              <td>{b.bank}</td>
              <td class="num">{fmtPct(b.rate, 2)}</td>
              <td class="num" class:fd-up={b.last_change > 0} class:fd-down={b.last_change < 0}>{b.last_change == null ? '·' : `${b.last_change > 0 ? '+' : ''}${Math.round(b.last_change * 100)} bp`}</td>
              <td>{b.last_change_date ?? '·'}</td>
              <td class="fd-muted">{b.as_of}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </Section>
{/snippet}

<Shell
  title={$t('fundamentals.macro.title')}
  subtitle={$t('fundamentals.macro.subtitle')}
  page="macro"
  sections={SECTIONS.map((id) => ({ id, label: $t(`fundamentals.macro.${id}`) }))}
>
  {#snippet options()}
    <div>
      <h3>{$t('fundamentals.macro.views')}</h3>
      {#if m.views.length}
        <ul class="vlist">
          {#each m.views as v (v.name)}
            <li>
              <span>{v.name} <span class="fd-muted">{v.picked.join(', ')}</span></span>
              <Button size="sm" variant="ghost" icon="trash" onclick={() => (m.views = m.views.filter((x) => x.name !== v.name))}>{$t('fundamentals.remove')}</Button>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="fd-muted">{$t('fundamentals.none')}</p>
      {/if}
    </div>
  {/snippet}

  <div class="layout">
    <button class="mobile-catalog-toggle" aria-expanded={mobileCatalogOpen} aria-controls="macro-catalog" onclick={() => (mobileCatalogOpen = !mobileCatalogOpen)}>
      {$t('fundamentals.macro.catalog')}
      <Icon name={mobileCatalogOpen ? 'chevron-up' : 'chevron-down'} size={14} />
    </button>
    <aside id="macro-catalog" class="rail" class:mobile-open={mobileCatalogOpen}>
      <div class="rail-head">
        <h2>{$t('fundamentals.macro.catalog')}</h2>
        <p class="fd-muted">{$t('fundamentals.macro.catalogDesc')}</p>
      </div>
      <input type="search" placeholder={$t('fundamentals.macro.search')} bind:value={q} />
      <Select
        bind:value={category}
        options={[{ value: '', label: $t('fundamentals.macro.allCategories') }, ...MACRO_CATEGORIES.map((c) => ({ value: c, label: $t(`fundamentals.macro.cat.${c}`) }))]}
      />
      <div class="fd-row">
        <Button size="sm" icon="plus" onclick={() => (adding = true)}>{$t('fundamentals.macro.add')}</Button>
        {#if fetching}<span class="fd-muted">{$t('fundamentals.fetching')}</span>{/if}
      </div>
      {#if !loaded}
        <Skeleton rows={8} />
      {:else if loadError}
        <ErrorText error={loadError} />
      {:else if !catalog.length}
        <div class="empty-cat">
          <p class="fd-muted">{$t('fundamentals.macro.noSeries')}</p>
          <Button size="sm" variant="primary" icon="download" disabled={busy === 'starter'} onclick={addStarter}>{$t('fundamentals.macro.starter')}</Button>
          {#if addError}<ErrorText error={addError} />{/if}
        </div>
      {:else}
        <div class="list">
          {#each groups as [cat, list] (cat)}
            <div class="grp">
              <h4>{$t(`fundamentals.macro.cat.${cat}`)}</h4>
              {#each list as s (s.id)}
                {@const d = change(s)}
                <button class="item" class:on={m.picked.includes(s.id)} onclick={() => toggle(s.id)} title={s.error ?? ''}>
                  <span class="name">{s.title}</span>
                  <span class="val">
                    {#if s.status === 'pending' && s.last == null}
                      <span class="fd-muted">{$t('fundamentals.fetching')}</span>
                    {:else if s.status === 'error'}
                      <span class="fd-down">{$t('fundamentals.failed')}</span>
                    {:else}
                      {fmtNum(s.last, 2)}
                      {#if d != null}<span class:fd-up={d > 0} class:fd-down={d < 0}>{d > 0 ? '▲' : d < 0 ? '▼' : ''}</span>{/if}
                    {/if}
                  </span>
                  <span class="meta">{s.country} · {s.provider} · {s.unit}</span>
                </button>
              {/each}
            </div>
          {/each}
        </div>
      {/if}
    </aside>

    <div class="fd-stack">
      {#each sections as id (id)}
        {#if id === 'chart'}{@render chartBlock()}{:else if id === 'curve'}{@render curveBlock()}{:else}{@render banksBlock()}{/if}
      {/each}
    </div>
  </div>
</Shell>

<Modal bind:open={adding} title={$t('fundamentals.macro.add')} size="md">
  <form class="addform" onsubmit={search}>
    <Select bind:value={addProvider} options={PROVIDERS} float />
    <input type="search" placeholder={$t('fundamentals.macro.addSearch')} bind:value={addQ} />
    <Button type="submit" variant="primary" icon="search" disabled={searching}>{$t('fundamentals.search')}</Button>
  </form>
  <div class="addcode">
    <p>{$t('fundamentals.macro.codeHint', { hint })}</p>
    <Button size="sm" variant="ghost" icon="plus" disabled={!addQ.trim() || busy !== ''} onclick={() => add(addProvider, addQ.trim())}>{$t('fundamentals.macro.addCode')}</Button>
  </div>
  <div class="addcat">
    <span>{$t('fundamentals.macro.category')}</span>
    <Select bind:value={addCategory} options={MACRO_CATEGORIES.map((c) => ({ value: c, label: $t(`fundamentals.macro.cat.${c}`) }))} float />
  </div>
  {#if addError}<ErrorText error={addError} />{/if}
  {#if searching}
    <Skeleton rows={4} />
  {:else if hits.length}
    <ul class="hits">
      {#each hits as h (h.code)}
        {@const have = catalog.some((c) => c.id === `${h.provider}:${h.code}`)}
        <li>
          <span class="htitle"><strong>{h.code}</strong> {h.title}</span>
          <span class="fd-muted">{h.unit}{h.frequency ? ` · ${h.frequency}` : ''}{h.seasonal_adj ? ' · SA' : ''}</span>
          <Button size="sm" variant={have ? 'ghost' : 'primary'} icon={have ? 'check' : 'plus'} disabled={have || busy === `${h.provider}:${h.code}`} onclick={() => add(h.provider, h.code)}>
            {have ? $t('fundamentals.added') : $t('fundamentals.add')}
          </Button>
        </li>
      {/each}
    </ul>
  {/if}
  {#snippet footer()}
    <Button size="sm" variant="ghost" icon="download" disabled={busy === 'starter'} onclick={addStarter}>{$t('fundamentals.macro.starter')}</Button>
  {/snippet}
</Modal>

<style>
  .addform {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    margin-bottom: var(--space-2);
  }
  .addform input {
    flex: 1;
    min-width: 0;
  }
  .addcode {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-4);
  }
  .addcode p {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    line-height: var(--lh-tight);
  }
  .addcat {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) 0;
    margin-bottom: var(--space-2);
    border-top: var(--hairline) solid var(--border);
    border-bottom: var(--hairline) solid var(--border);
  }
  .addcat > span {
    color: var(--muted);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .addcat :global(.field) {
    width: 220px;
  }
  .hits {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .hits li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: var(--space-3);
    align-items: center;
    padding: var(--space-2) 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  .htitle {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .empty-cat {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: flex-start;
  }
  .layout {
    display: grid;
    grid-template-columns: 300px minmax(0, 1fr);
    gap: var(--fd-gap);
    align-items: start;
  }
  .rail {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--fd-pad);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-radius: var(--fd-radius);
    box-shadow: var(--shadow-1);
    min-width: 0;
    max-height: calc(100vh - 220px);
    position: sticky;
    top: 0;
  }
  .rail-head h2 {
    margin: 0;
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
  }
  .rail-head {
    padding-bottom: var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
  }
  .rail-head p {
    margin: var(--space-1) 0 0;
  }
  /* The rail is capped in height: only the list gives, the controls keep theirs. */
  .rail > :not(.list) {
    flex-shrink: 0;
  }
  .rail input[type='search'] {
    width: 100%;
  }
  .list {
    overflow-y: auto;
    overflow-x: hidden;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    margin-top: var(--space-2);
  }
  .grp h4 {
    margin: 0 0 var(--space-1);
    padding: 0 var(--space-2);
    font-size: 10.5px;
    font-weight: var(--fw-medium);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .item {
    width: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 6px var(--space-3);
    text-align: left;
    padding: var(--fd-item-pad) var(--space-3);
    margin-top: var(--space-2);
    border: var(--hairline) solid var(--border);
    border-left: 2px solid transparent;
    border-radius: 6px;
    background: var(--surface);
    color: var(--text);
    cursor: pointer;
  }
  .item:hover {
    background: var(--surface-2);
  }
  .item.on {
    background: color-mix(in srgb, var(--accent) 9%, var(--surface));
    border-left-color: var(--accent);
    box-shadow: inset 1px 0 var(--accent);
  }
  .name {
    min-width: 0;
    font-size: var(--text-sm);
    line-height: 1.5;
    overflow-wrap: anywhere;
  }
  .meta {
    grid-column: 1 / -1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    color: var(--dim);
  }
  .val {
    font-family: var(--mono);
    font-size: var(--text-sm);
    white-space: nowrap;
    padding-top: 2px;
  }
  .views {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-bottom: var(--space-4);
    margin-bottom: var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
  }
  .views > :not(.vtrack) {
    flex-shrink: 0;
  }
  .vtrack {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: var(--space-2);
    overflow-x: auto;
    scrollbar-width: thin;
  }
  .vtrack .chip {
    flex-shrink: 0;
    white-space: nowrap;
  }
  .vname {
    width: 180px;
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin-left: var(--space-2);
  }
  .picked {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-bottom: var(--space-4);
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2) var(--space-1) var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: 6px;
    background: var(--surface-2);
    flex-wrap: wrap;
    font-size: var(--text-sm);
  }
  .pill .fd-muted {
    font-size: 11px;
  }
  .pill button {
    display: inline-grid;
    place-items: center;
    border: 0;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    padding: 2px;
  }
  .empty {
    padding: var(--space-8) 0;
    text-align: center;
    border: 1px dashed var(--border-control);
    border-radius: var(--fd-radius);
    background: var(--surface-2);
  }
  .vlist {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .vlist li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    font-size: var(--text-sm);
  }
  @media (max-width: 960px) {
    .layout {
      grid-template-columns: 1fr;
    }
    .rail {
      position: static;
      max-height: 440px;
    }
  }
  .mobile-catalog-toggle { display: none; }
  @media (max-width: 767px) {
    .views { align-items: flex-start; flex-wrap: wrap; }
    .vtrack { flex: 1 1 100%; flex-wrap: wrap; overflow: visible; }
    .vtrack .chip { white-space: normal; overflow-wrap: anywhere; }
    .mobile-catalog-toggle { display: flex; align-items: center; justify-content: space-between; min-height: 44px; width: 100%; padding: 0 var(--space-3); border: 1px solid var(--border); background: var(--surface); color: var(--text); }
    .rail:not(.mobile-open) { display: none; }
    .rail.mobile-open { max-height: min(60dvh, 440px); overflow-y: auto; }
  }
</style>
