<script>
  // Searchable multi-select of datasets (checkbox list), for the multi-asset panels.
  import { dsLabel } from './api.js';
  import { t } from '$lib/i18n';

  // `labelOf` / `subOf` let the same list pick other rows (saved backtest runs).
  let {
    datasets = [],
    value = $bindable([]),
    exclude = null,
    max = 30,
    labelOf = dsLabel,
    subOf = (d) => d.provider
  } = $props();

  let q = $state('');
  const list = $derived.by(() => {
    const s = q.trim().toLowerCase();
    return datasets.filter(
      (d) => d.id !== exclude && (!s || `${labelOf(d)} ${subOf(d) ?? ''}`.toLowerCase().includes(s))
    );
  });

  function toggle(id) {
    if (value.includes(id)) value = value.filter((x) => x !== id);
    else if (value.length < max) value = [...value, id];
  }
</script>

<div class="dl">
  <input class="search" placeholder={$t('quant.page.searchDatasetsPlaceholder')} bind:value={q} />
  <div class="list">
    {#each list as d (d.id)}
      <label class="chk">
        <input type="checkbox" checked={value.includes(d.id)} onchange={() => toggle(d.id)} />
        <span>{labelOf(d)}</span>
        {#if subOf(d)}<span class="prov">{subOf(d)}</span>{/if}
      </label>
    {/each}
    {#if list.length === 0}
      <p class="none">{$t('quant.page.noMatches')}</p>
    {/if}
  </div>
</div>

<style>
  .dl {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
    width: 100%;
  }
  .search {
    width: 100%;
  }
  .list {
    display: flex;
    flex-direction: column;
    max-height: 220px;
    overflow-y: auto;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--space-1);
  }
  .chk {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--fs-body);
    padding: var(--space-2);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .prov {
    color: var(--muted);
    font-size: var(--fs-desc);
    margin-left: auto;
  }
  .none {
    color: var(--muted);
    font-size: var(--fs-body);
    padding: var(--space-1);
  }

  .chk:hover {
    background: var(--surface-hover);
  }
  .chk:has(input:checked) {
    background: var(--surface-2);
  }
  .chk input {
    flex-shrink: 0;
  }
  .chk span {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .prov {
    max-width: 40%;
    text-align: right;
  }

</style>
