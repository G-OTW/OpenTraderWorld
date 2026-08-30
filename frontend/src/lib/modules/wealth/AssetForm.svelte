<script>
  // Create/edit an asset: pick a template (or none), name, type, currency, category.
  import { CURRENCIES, ASSET_TYPES } from './api.js';
  import { t } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';

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
  function onTemplate(v) {
    const id = v || null;
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
      <div class="field">
        <span>{$t('wealth.assetForm.template')}</span>
        <Dropdown
          value={a.template_id ?? ''}
          onpick={onTemplate}
          ariaLabel={$t('wealth.assetForm.template')}
          options={[
            { value: '', label: $t('wealth.assetForm.noneOption') },
            ...templates.map((tpl) => ({ value: tpl.id, label: tpl.name }))
          ]}
        />
      </div>
    {/if}
    <div class="field">
      <span>{$t('wealth.assetForm.type')}</span>
      <Dropdown
        bind:value={a.asset_type}
        ariaLabel={$t('wealth.assetForm.type')}
        options={ASSET_TYPES.map((at) => ({ value: at.id, label: `${at.icon} ${at.label}` }))}
      />
    </div>
    <div class="field">
      <span>{$t('wealth.assetForm.currency')}</span>
      <Dropdown
        bind:value={a.currency}
        ariaLabel={$t('wealth.assetForm.currency')}
        options={CURRENCIES.map((c) => ({ value: c, label: c }))}
      />
    </div>
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
