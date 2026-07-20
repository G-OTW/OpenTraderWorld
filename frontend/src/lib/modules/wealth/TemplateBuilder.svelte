<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Wealth asset-template builder: name, asset type, and an ordered list of reserved
  // (price/quantity) + custom fields.
  import { ASSET_TYPES, RESERVED_FIELDS, CUSTOM_FIELD_TYPES, shortId } from './api.js';
  import { t } from '$lib/i18n';

  let { initial = null, onsubmit = () => {}, oncancel = () => {} } = $props();

  let name = $state(initial?.name ?? '');
  let description = $state(initial?.description ?? '');
  let assetType = $state(initial?.asset_type ?? 'stock');
  // New templates start with Price + Quantity by default; editing keeps existing fields.
  let fields = $state(
    initial
      ? (initial.fields ?? []).map((f) => ({ options: {}, ...f }))
      : RESERVED_FIELDS.map((r) => ({ key: r.reserved, label: r.label, type: r.type, reserved: r.reserved }))
  );

  const usedReserved = $derived(new Set(fields.filter((f) => f.reserved).map((f) => f.reserved)));

  function addReserved(reserved) {
    const def = RESERVED_FIELDS.find((r) => r.reserved === reserved);
    if (!def || usedReserved.has(reserved)) return;
    fields = [...fields, { key: reserved, label: def.label, type: def.type, reserved }];
  }
  function addCustom() {
    fields = [...fields, { key: `c_${shortId()}`, label: $t('wealth.templates.newFieldLabel'), type: 'text', reserved: null, options: {} }];
  }
  function removeField(i) {
    fields = fields.filter((_, idx) => idx !== i);
  }
  function move(i, dir) {
    const j = i + dir;
    if (j < 0 || j >= fields.length) return;
    const copy = [...fields];
    [copy[i], copy[j]] = [copy[j], copy[i]];
    fields = copy;
  }

  // A field-level validation message belongs beside the field, not in a modal the browser
  // paints over the page. It also clears as soon as the user types.
  let nameError = $state('');

  function submit() {
    if (!name.trim()) {
      nameError = $t('wealth.templates.nameRequired');
      return;
    }
    nameError = '';
    onsubmit({ name: name.trim(), description: description || null, asset_type: assetType, fields });
  }
</script>

<div class="builder">
  <div class="row">
    <label class="field">
      <span>{$t('wealth.templates.name')}</span>
      <!-- aria-invalid states in text what the ring says in colour; aria-describedby ties
           the message to the field. A <span>, not <ErrorText>: that renders a <p>, which a
           <label> may not contain — and a field error is quieter than a block one. -->
      <input
        bind:value={name}
        placeholder={$t('wealth.templates.namePlaceholder')}
        class:invalid={nameError}
        aria-invalid={nameError ? 'true' : undefined}
        aria-describedby={nameError ? 'tpl-name-err' : undefined}
        oninput={() => (nameError = '')}
      />
      {#if nameError}<span id="tpl-name-err" class="err" role="alert">{nameError}</span>{/if}
    </label>
    <label class="field">
      <span>{$t('wealth.templates.assetType')}</span>
      <select bind:value={assetType}>
        {#each ASSET_TYPES as at}<option value={at.id}>{at.icon} {at.label}</option>{/each}
      </select>
    </label>
  </div>
  <label class="field"><span>{$t('wealth.templates.description')}</span><input bind:value={description} placeholder={$t('wealth.assetForm.optionalPlaceholder')} /></label>

  <div class="add-controls">
    <div class="reserved">
      <span class="lbl">{$t('wealth.templates.addReservedField')}</span>
      <div class="chips">
        {#each RESERVED_FIELDS as r}
          <button class="mini" disabled={usedReserved.has(r.reserved)} onclick={() => addReserved(r.reserved)}>
            {r.label}
          </button>
        {/each}
      </div>
    </div>
    <button class="chip" onclick={addCustom}>{$t('wealth.templates.addField')}</button>
  </div>

  <div class="field-list">
    {#if fields.length === 0}
      <p class="muted">{$t('wealth.templates.addFieldsHint')}</p>
    {/if}
    {#each fields as f, i (f.key)}
      <div class="frow">
        <div class="order">
          <button class="icon" onclick={() => move(i, -1)} disabled={i === 0}><Icon name="arrow-up" size={12} /></button>
          <button class="icon" onclick={() => move(i, 1)} disabled={i === fields.length - 1}><Icon name="arrow-down" size={12} /></button>
        </div>
        <input class="flabel" bind:value={f.label} placeholder={$t('wealth.templates.fieldLabelPlaceholder')} />
        {#if f.reserved}
          <span class="tag">{$t('wealth.templates.reservedTag', { reserved: f.reserved })}</span>
        {:else}
          <select bind:value={f.type}>
            {#each CUSTOM_FIELD_TYPES as ct}<option value={ct.id}>{ct.label}</option>{/each}
          </select>
        {/if}
        <button class="icon danger" onclick={() => removeField(i)} title={$t('wealth.templates.removeField')}><Icon name="x" size={13} /></button>
      </div>
    {/each}
  </div>

  <div class="actions">
    <button class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button class="primary" onclick={submit}>{initial ? $t('wealth.templates.saveTemplate') : $t('wealth.templates.createTemplate')}</button>
  </div>
</div>

<style>
  /* A field error is quieter than a block error (no icon, --text-xs) and sits under the
     input it belongs to. --red-ink, not --red: the raw hue is a fill colour and drops to
     4.35:1 as text on --surface-2. The border keeps the raw hue — its bar is 3:1. */
  .err {
    color: var(--red-ink);
    font-size: var(--text-xs);
    margin-top: 4px;
  }
  .invalid {
    border-color: var(--red) !important;
  }
  .builder {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-height: 70vh;
    overflow-y: auto;
  }
  .row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .add-controls {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border-top: 0.5px solid var(--border);
    border-bottom: 0.5px solid var(--border);
    padding: var(--space-3) 0;
  }
  .lbl {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 4px;
  }
  .mini {
    font-size: var(--text-xs);
    border: 0.5px solid var(--border-control);
    background: transparent;
    color: var(--text);
    border-radius: 0;
    padding: 3px 8px;
    cursor: pointer;
  }
  .mini:hover:not(:disabled) {
    background: var(--surface-2);
  }
  .mini:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .chip {
    background: transparent;
    border: 0.5px solid var(--border-control);
    color: var(--text);
    border-radius: 0;
    padding: 6px 12px;
    cursor: pointer;
    font-size: var(--text-sm);
    align-self: flex-start;
  }
  .chip:hover {
    background: var(--surface-2);
  }
  .field-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .frow {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .order {
    display: flex;
    flex-direction: column;
  }
  .order .icon {
    font-size: 0.6rem;
    line-height: 1;
    padding: 1px;
  }
  .flabel {
    flex: 1;
  }
  .tag {
    font-size: 0.68rem;
    color: var(--dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    white-space: nowrap;
  }
  .icon.danger:hover {
    color: var(--red);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
  }
</style>
