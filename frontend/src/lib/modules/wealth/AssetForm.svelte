<script>
  // Create/edit an asset: pick a template (or none), name, type, currency, category.
  import { CURRENCIES, ASSET_TYPES } from './api.js';
  import { t } from '$lib/i18n';

  let {
    initial = null,
    templates = [],
    categories = [],
    // New assets default to the module's display currency; still editable, and an existing
    // asset always keeps its own stored currency (it's the FX reference).
    defaultCurrency = 'USD',
    onsubmit = () => {},
    oncancel = () => {}
  } = $props();

  let a = $state(blank());

  function blank() {
    const base = {
      template_id: null,
      name: '',
      asset_type: 'money',
      currency: defaultCurrency,
      category: ''
    };
    if (initial) {
      return { ...base, ...initial, category: initial.category ?? '' };
    }
    return base;
  }

  // Picking a template defaults the asset type to the template's type.
  function onTemplate(e) {
    const id = e.target.value || null;
    a.template_id = id;
    const tpl = templates.find((t) => t.id === id);
    if (tpl) a.asset_type = tpl.asset_type;
  }

  function submit() {
    onsubmit({
      template_id: a.template_id || null,
      name: a.name,
      asset_type: a.asset_type,
      currency: a.currency,
      category: a.category || null
    });
  }
</script>

<form class="asset-form" onsubmit={(e) => { e.preventDefault(); submit(); }}>
  <datalist id="asset-categories">
    {#each categories as c}<option value={c}></option>{/each}
  </datalist>

  <label class="field wide">
    <span>{$t('wealth.assetForm.name')}</span>
    <!-- svelte-ignore a11y_autofocus -->
    <input bind:value={a.name} autofocus placeholder={$t('wealth.assetForm.namePlaceholder')} />
  </label>

  <div class="grid">
    {#if templates.length > 0}
      <label class="field">
        <span>{$t('wealth.assetForm.template')}</span>
        <select value={a.template_id ?? ''} onchange={onTemplate}>
          <option value="">{$t('wealth.assetForm.noneOption')}</option>
          {#each templates as tpl}<option value={tpl.id}>{tpl.name}</option>{/each}
        </select>
      </label>
    {/if}
    <label class="field">
      <span>{$t('wealth.assetForm.type')}</span>
      <select bind:value={a.asset_type}>
        {#each ASSET_TYPES as at}<option value={at.id}>{at.icon} {at.label}</option>{/each}
      </select>
    </label>
    <label class="field">
      <span>{$t('wealth.assetForm.currency')}</span>
      <select bind:value={a.currency}>
        {#each CURRENCIES as c}<option value={c}>{c}</option>{/each}
      </select>
    </label>
    <label class="field">
      <span>{$t('wealth.assetForm.category')}</span>
      <input bind:value={a.category} list="asset-categories" autocomplete="off" placeholder={$t('wealth.assetForm.optionalPlaceholder')} />
    </label>
  </div>

  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="submit" class="primary">{initial ? $t('common.save') : $t('wealth.assetForm.addAsset')}</button>
  </div>
</form>

<style>
  .asset-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-3);
  }
  .field.wide {
    width: 100%;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }
</style>
