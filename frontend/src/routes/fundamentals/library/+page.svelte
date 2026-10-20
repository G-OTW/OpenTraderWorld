<script>
  // Fundamentals: Library. One tab per list, each full width so a long one scrolls with
  // the page: stored series and companies, the order providers are tried in for each
  // dataset (the user's, else the app's), and which provider serves which data family
  // (connected when a connector is granted to the module). Tabs follow Customize.
  import Shell from '$lib/modules/fundamentals/Shell.svelte';
  import Section from '$lib/modules/fundamentals/Section.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Select from '$lib/ui/Select.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { fundamentalsApi, DATA_ELEMENTS, fmtNum } from '$lib/modules/fundamentals/api.js';
  import { prefs, visibleSections } from '$lib/modules/fundamentals/prefs.svelte.js';
  import { t } from '$lib/i18n';

  const SECTIONS = ['series', 'companies', 'priority', 'coverage'];
  let companies = $state([]);
  let catalog = $state([]);
  let sources = $state([]);
  let only = $state('');
  let orders = $state([]);
  let orderError = $state('');
  let q = $state('');

  $effect(() => {
    fundamentalsApi.companies().then((x) => (companies = x)).catch(() => {});
    fundamentalsApi.series().then((x) => (catalog = x)).catch(() => {});
    fundamentalsApi.sources().then((x) => (sources = x)).catch(() => {});
    loadOrders();
  });

  // Datasets with more than one source, plus transcripts: the ones whose order matters.
  async function loadOrders() {
    try {
      const r = await fundamentalsApi.datasets();
      orders = [...r.datasets, r.transcripts_order].filter((d) => d.providers.length > 1);
    } catch (e) {
      orderError = e.message;
    }
  }

  // Picking rank n for a provider moves it there; the others keep their relative order.
  async function move(d, provider, rank) {
    const ids = d.providers.map((p) => p.id).filter((id) => id !== provider);
    ids.splice(rank - 1, 0, provider);
    orderError = '';
    try {
      await fundamentalsApi.setOrder(d.id, ids);
    } catch (e) {
      orderError = e.message;
    }
    await loadOrders();
  }

  async function reset(d) {
    orderError = '';
    try {
      await fundamentalsApi.resetOrder(d.id);
    } catch (e) {
      orderError = e.message;
    }
    await loadOrders();
  }

  async function removeSeries(s) {
    await fundamentalsApi.removeSeries(s.uuid);
    catalog = catalog.filter((x) => x.uuid !== s.uuid);
  }

  async function removeCompany(c) {
    await fundamentalsApi.removeCompany(c.ticker);
    companies = companies.filter((x) => x.ticker !== c.ticker);
  }

  const day = (ms) => (ms ? new Date(ms).toISOString().slice(0, 10) : '');
  const has = (...fields) => fields.some((f) => (f ?? '').toLowerCase().includes(q.trim().toLowerCase()));

  const tabs = $derived(visibleSections('library', SECTIONS));
  // The remembered tab, unless Customize hid it since.
  const tab = $derived(tabs.includes(prefs.library.tab) ? prefs.library.tab : tabs[0]);
  const counts = $derived({
    series: catalog.length,
    companies: companies.length,
    priority: orders.length,
    coverage: sources.length
  });
  const shownSeries = $derived(catalog.filter((s) => has(s.title, s.code, s.provider)));
  const shownCompanies = $derived(companies.filter((c) => has(c.ticker, c.name)));
  const shownProviders = $derived(only ? sources.filter((p) => p.serves.includes(only)) : sources);
  const custom = $derived(orders.filter((d) => d.custom).length);
</script>

<Shell
  title={$t('fundamentals.lib.title')}
  subtitle={$t('fundamentals.lib.subtitle')}
  page="library"
  sections={SECTIONS.map((id) => ({ id, label: $t(`fundamentals.lib.${id}`) }))}
>
  <div class="fd-scroll">
    <Tabs
      tabs={tabs.map((id) => ({ id, label: `${$t(`fundamentals.lib.${id}`)} · ${counts[id]}` }))}
      bind:value={() => tab, (v) => ((prefs.library.tab = v), (q = ''))}
      ariaLabel={$t('fundamentals.lib.title')}
    />
  </div>

  <div role="tabpanel" id="panel-{tab}" aria-labelledby="tab-{tab}">
    {#if tab === 'series'}
      <Section key="library.series" title={$t('fundamentals.lib.series')} description={$t('fundamentals.lib.seriesDesc')}>
        {#snippet actions()}
          <input class="filter" type="search" placeholder={$t('fundamentals.lib.filterSeries')} bind:value={q} />
        {/snippet}
        <div class="fd-scroll">
          <table class="tbl">
            <thead>
              <tr>
                <th>{$t('fundamentals.lib.seriesCol')}</th>
                <th>{$t('fundamentals.lib.category')}</th>
                <th>{$t('fundamentals.doc.source')}</th>
                <th class="num">{$t('fundamentals.lib.last')}</th>
                <th>{$t('fundamentals.lib.updated')}</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {#each shownSeries as s (s.id)}
                <tr>
                  <td class="main">
                    <span class="name">{s.title}</span>
                    <span class="sub">{s.code}</span>
                  </td>
                  <td class="fd-muted nowrap">{$t(`fundamentals.macro.cat.${s.category}`)}</td>
                  <td class="fd-muted">{s.provider}</td>
                  <td class="num">{fmtNum(s.last, 2)} <span class="fd-muted">{s.unit}</span></td>
                  <td class="nowrap" title={s.error ?? ''}>
                    {#if s.status === 'ok' && s.updated}
                      <span class="fd-muted">{day(s.updated)}</span>
                    {:else}
                      <Badge tone={s.status === 'error' ? 'danger' : 'neutral'}>{$t(`fundamentals.status.${s.status}`)}</Badge>
                    {/if}
                  </td>
                  <td class="act"><button class="icon danger-hover" onclick={() => removeSeries(s)} title={$t('fundamentals.remove')} aria-label={$t('fundamentals.remove')}><Icon name="trash" size={13} /></button></td>
                </tr>
              {:else}
                <tr><td colspan="6" class="fd-muted">{$t(catalog.length ? 'fundamentals.lib.noMatch' : 'fundamentals.none')}</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      </Section>
    {:else if tab === 'companies'}
      <Section key="library.companies" title={$t('fundamentals.lib.companies')} description={$t('fundamentals.lib.companiesDesc')}>
        {#snippet actions()}
          <input class="filter" type="search" placeholder={$t('fundamentals.lib.filterCompanies')} bind:value={q} />
        {/snippet}
        <div class="fd-scroll">
          <table class="tbl">
            <thead>
              <tr>
                <th>{$t('fundamentals.ticker')}</th>
                <th></th>
                <th>{$t('fundamentals.lib.updated')}</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {#each shownCompanies as c (c.ticker)}
                <tr>
                  <td class="main">
                    <a class="name" href="/fundamentals/company?t={c.ticker}">{c.ticker}</a>
                    <span class="sub">{c.name}</span>
                  </td>
                  <td>{#if c.followed}<Badge tone="accent">{$t('fundamentals.following')}</Badge>{/if}</td>
                  <td class="nowrap" title={c.error ?? ''}>
                    {#if c.status === 'ok' && c.updated}
                      <span class="fd-muted">{day(c.updated)}</span>
                    {:else}
                      <Badge tone={c.status === 'error' ? 'danger' : 'neutral'}>{$t(`fundamentals.status.${c.status}`)}</Badge>
                    {/if}
                  </td>
                  <td class="act"><button class="icon danger-hover" onclick={() => removeCompany(c)} title={$t('fundamentals.remove')} aria-label={$t('fundamentals.remove')}><Icon name="trash" size={13} /></button></td>
                </tr>
              {:else}
                <tr><td colspan="4" class="fd-muted">{$t(companies.length ? 'fundamentals.lib.noMatch' : 'fundamentals.none')}</td></tr>
              {/each}
            </tbody>
          </table>
        </div>
      </Section>
    {:else if tab === 'priority'}
      <Section key="library.priority" title={$t('fundamentals.lib.priority')} description={$t('fundamentals.lib.priorityDesc')}>
        {#snippet actions()}
          {#if custom}<Badge tone="accent">{$t('fundamentals.lib.customCount', { n: custom })}</Badge>{/if}
        {/snippet}
        {#if orderError}<ErrorText>{orderError}</ErrorText>{/if}
        <div class="prio">
          {#each orders as d (d.id)}
            <article class="ds" class:custom={d.custom}>
              <header>
                <h3>{$t(`fundamentals.ds.${d.id}`)}</h3>
                {#if d.custom}<Badge tone="accent">{$t('fundamentals.lib.custom')}</Badge>{/if}
                <Button
                  size="sm"
                  variant="ghost"
                  icon="rotate-ccw"
                  disabled={!d.custom}
                  onclick={() => reset(d)}
                  title={$t('fundamentals.lib.resetOrderHint')}
                >
                  {$t('fundamentals.lib.resetOrder')}
                </Button>
              </header>
              <ol class="ranks">
                {#each d.providers as p, i (p.id)}
                  <li class:off={!p.connected}>
                    <span class="rank">
                      <Select
                        value={i + 1}
                        options={d.providers.map((_, n) => ({ value: n + 1, label: String(n + 1) }))}
                        onpick={(v) => v !== i + 1 && move(d, p.id, v)}
                      />
                    </span>
                    <span class="pname">{p.label}</span>
                    {#if !p.connected}<span class="note">{$t('fundamentals.lib.notConnected')}</span>{/if}
                  </li>
                {/each}
              </ol>
            </article>
          {/each}
        </div>
      </Section>
    {:else if tab === 'coverage'}
      <Section key="library.coverage" title={$t('fundamentals.lib.coverage')} description={$t('fundamentals.lib.coverageNote')}>
        <div class="serves">
          <span class="fd-label">{$t('fundamentals.lib.filter')}</span>
          <div class="chips">
            <button class="chip" class:active={!only} onclick={() => (only = '')}>{$t('fundamentals.all')}</button>
            {#each DATA_ELEMENTS as d (d)}
              <button class="chip" class:active={only === d} onclick={() => (only = d)}>{$t(`fundamentals.el.${d}`)}</button>
            {/each}
          </div>
        </div>
        <div class="fd-scroll">
          <table class="tbl cov">
            <thead>
              <tr>
                <th class="sticky">{$t('fundamentals.lib.provider')}</th>
                <th>{$t('fundamentals.lib.key')}</th>
                {#each DATA_ELEMENTS as d (d)}<th class="c" class:hl={only === d}>{$t(`fundamentals.el.${d}`)}</th>{/each}
              </tr>
            </thead>
            <tbody>
              {#each shownProviders as p (p.id)}
                <tr>
                  <td class="sticky nowrap">
                    {p.name}
                    {#if p.connected}<Badge tone="success">{$t('fundamentals.lib.connected')}</Badge>{/if}
                  </td>
                  <td class="fd-muted">{$t(p.key === 'none' ? 'fundamentals.lib.keyNone' : 'fundamentals.lib.keyNeeded')}</td>
                  {#each DATA_ELEMENTS as d (d)}
                    <td class="c" class:hl={only === d}>{#if p.serves.includes(d)}<Icon name="check" size={13} />{/if}</td>
                  {/each}
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </Section>
    {/if}
  </div>
</Shell>

<style>
  .filter {
    width: 260px;
    max-width: 100%;
  }
  .main {
    min-width: 260px;
  }
  .main .name {
    display: block;
    font-weight: var(--fw-medium);
  }
  .main .sub {
    display: block;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--dim);
    overflow-wrap: anywhere;
  }
  .nowrap {
    white-space: nowrap;
  }
  .act {
    text-align: right;
    width: 1%;
  }

  .prio {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 320px), 1fr));
    gap: var(--space-4);
  }
  .ds {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    border: var(--hairline) solid var(--border);
    border-radius: var(--fd-radius, 8px);
    background: var(--surface);
  }
  .ds.custom {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
  }
  .ds header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 26px;
  }
  .ds h3 {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .ds header :global(.btn) {
    margin-left: auto;
  }
  .ranks {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .ranks li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }
  .rank {
    flex: 0 0 4.25rem;
  }
  .pname {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .note {
    margin-left: auto;
    font-size: 11px;
    color: var(--dim);
    white-space: nowrap;
  }
  .ranks li.off .pname {
    color: var(--muted);
  }

  .serves {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-4);
    min-width: 0;
  }
  .serves .chips {
    display: flex;
    gap: var(--space-2);
    overflow-x: auto;
    min-width: 0;
    scrollbar-width: thin;
  }
  .serves .chip {
    flex-shrink: 0;
    white-space: nowrap;
  }
  .cov .c {
    text-align: center;
    color: var(--accent);
  }
  .cov .hl {
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .cov .sticky {
    position: sticky;
    left: 0;
    z-index: 1;
    background: var(--surface);
  }
  .cov thead .sticky {
    background: var(--surface-2);
  }
</style>
