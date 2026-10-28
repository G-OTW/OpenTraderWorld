<script>
  // Create/edit an asset: pick a template (or none), name, type, currency, category, plus
  // the two fields that make the headline honest: whether it is owed rather than owned, and
  // how often its valuation should be revisited.
  import { CURRENCIES, ASSET_TYPES, CREATABLE_TYPES, signOf, REVIEW_DEFAULTS } from './api.js';
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
      category: '',
      sign: 1,
      portfolio_id: null,
      review_days: 0
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

  // A linked asset is valued from its portfolio, so its type, sign and review interval are
  // not the user's to change here: the link flow owns them.
  const linked = $derived(a.portfolio_id != null);
  // An older row may carry a type that is no longer offered; keep it in the list so editing
  // it does not silently reclassify it.
  const typeOptions = $derived(
    CREATABLE_TYPES.some((t) => t.id === a.asset_type)
      ? CREATABLE_TYPES
      : ASSET_TYPES.filter((t) => CREATABLE_TYPES.includes(t) || t.id === a.asset_type)
  );

  function onType(v) {
    a.asset_type = v;
    // The sign follows the kind of thing: a loan is owed. Still overridable below, because
    // a negative balance can show up on any account.
    a.sign = signOf(v);
    if (!initial) a.review_days = REVIEW_DEFAULTS[v] ?? 0;
  }

  function submit() {
    onsubmit({
      template_id: a.template_id || null,
      name: a.name,
      asset_type: a.asset_type,
      currency: a.currency,
      category: a.category || null,
      sign: linked ? 1 : Number(a.sign) || 1,
      portfolio_id: a.portfolio_id ?? null,
      review_days: linked ? 0 : Number(a.review_days) || 0
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
        value={a.asset_type}
        onpick={onType}
        disabled={linked}
        ariaLabel={$t('wealth.assetForm.type')}
        options={typeOptions.map((at) => ({ value: at.id, label: `${at.icon} ${at.label}` }))}
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
    {#if !linked}
      <div class="field">
        <span>{$t('wealth.assetForm.side')}</span>
        <!-- Owned or owed. A net worth that cannot subtract a mortgage is not a net worth,
             and this is the one field that makes the headline true. -->
        <Dropdown
          value={String(a.sign)}
          onpick={(v) => (a.sign = Number(v))}
          ariaLabel={$t('wealth.assetForm.side')}
          options={[
            { value: '1', label: $t('wealth.assetForm.owned') },
            { value: '-1', label: $t('wealth.assetForm.owed') }
          ]}
        />
      </div>
      <label class="field">
        <span>{$t('wealth.assetForm.review')}</span>
        <!-- A house valued three years ago is wrong in silence. This is how long before the
             page says so. 0 never nags. -->
        <input type="number" min="0" step="1" bind:value={a.review_days} />
      </label>
    {/if}
  </div>

  {#if linked}
    <p class="linked">{$t('wealth.assetForm.linkedNote')}</p>
  {/if}

  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="submit" class="primary">{initial ? $t('common.save') : $t('wealth.assetForm.addAsset')}</button>
  </div>
</form>

<style>
  .linked {
    margin: 0;
    font-size: 12px;
    color: var(--muted);
  }
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
