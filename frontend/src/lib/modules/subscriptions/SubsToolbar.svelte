<script>
  // One chip row over everything it scopes: the view (List, Calendar, Worth it?), the
  // search, then the filters. Platform and category refetch the breakdown (charts and
  // totals follow); status, payment method and price band narrow the rows client-side.
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Seg from './Seg.svelte';
  import { t } from '$lib/i18n';

  let {
    view = $bindable('list'),
    search = $bindable(''),
    platform = $bindable(''),
    category = $bindable(''),
    status = $bindable('running'),
    method = $bindable(''),
    band = $bindable(''),
    bands = [],
    suggestions = { platforms: [], categories: [], payment_methods: [] },
    shown = 0,
    total = 0,
    onclear = () => {}
  } = $props();

  const active = $derived([platform, category, band, method].filter(Boolean).length);
  // A filter value no longer offered (renamed platform) still shows as picked.
  const withValue = (list, v) => (v && !list.includes(v) ? [...list, v] : list);
</script>

<section class="toolbar">
  <Seg
    size="md"
    bind:value={view}
    ariaLabel={$t('subscriptions.page.view')}
    options={[
      { value: 'list', label: $t('subscriptions.page.viewList') },
      { value: 'calendar', label: $t('subscriptions.page.viewCalendar') },
      { value: 'worth', label: $t('subscriptions.page.viewWorth') }
    ]}
  />
  <label class="search">
    <Icon name="search" size={14} />
    <input placeholder={$t('subscriptions.page.searchPlaceholder')} autocomplete="off" bind:value={search} aria-label={$t('subscriptions.page.searchPlaceholder')} />
  </label>
  <Dropdown
    bind:value={platform}
    ariaLabel={$t('subscriptions.form.platform')}
    options={[
      { value: '', label: $t('subscriptions.page.anyPlatform') },
      ...withValue(suggestions.platforms ?? [], platform).map((v) => ({ value: v, label: v }))
    ]}
  />
  <Dropdown
    bind:value={category}
    ariaLabel={$t('subscriptions.form.category')}
    options={[
      { value: '', label: $t('subscriptions.page.anyCategory') },
      ...withValue(suggestions.categories ?? [], category).map((v) => ({ value: v, label: v }))
    ]}
  />
  <Dropdown
    bind:value={status}
    ariaLabel={$t('subscriptions.page.status')}
    options={[
      { value: 'running', label: $t('subscriptions.page.statusRunning') },
      { value: 'ended', label: $t('subscriptions.page.statusEnded') },
      { value: 'all', label: $t('subscriptions.page.statusAll') }
    ]}
  />
  {#if (suggestions.payment_methods ?? []).length}
    <Dropdown
      bind:value={method}
      ariaLabel={$t('subscriptions.form.paymentMethod')}
      options={[
        { value: '', label: $t('subscriptions.page.anyMethod') },
        ...suggestions.payment_methods.map((m) => ({ value: m, label: m }))
      ]}
    />
  {/if}
  <Dropdown
    bind:value={band}
    ariaLabel={$t('subscriptions.page.priceBand')}
    options={[{ value: '', label: $t('subscriptions.page.anyPrice') }, ...bands.map((b) => ({ value: b.id, label: b.label }))]}
  />
  {#if active > 0}
    <button type="button" class="ghost sm" onclick={onclear}>{$t('subscriptions.page.clearCount', { count: active })}</button>
  {/if}
  <span class="count">{$t('subscriptions.page.countOf', { shown, total })}</span>
</section>

<style>
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  /* One frame: the label is the box, the input inside it is bare. */
  .search {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    flex: 1 1 200px;
    min-width: 180px;
    max-width: 320px;
    height: var(--control-h);
    padding: 0 var(--space-3);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--dim);
    transition: border-color 160ms ease;
  }
  .search:focus-within {
    border-color: var(--accent);
  }
  /* `all: unset` drops the element frame and its focus ring: the box owns both. */
  .search input {
    all: unset;
    flex: 1;
    min-width: 0;
    height: 100%;
    color: var(--text);
    font-family: var(--font);
    font-size: 13px;
  }
  .search input::placeholder {
    color: var(--faint);
  }
  .count {
    margin-left: auto;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 11px;
    color: var(--dim);
    white-space: nowrap;
  }
  @media (max-width: 767px) {
    .search {
      max-width: none;
      flex-basis: 100%;
    }
  }
</style>
