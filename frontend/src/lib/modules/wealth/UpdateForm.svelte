<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Record / edit value updates (revisions) for an asset. The form adds a new revision or
  // saves edits to the one being edited. Below it, a scrollable list shows every update
  // (date + value), newest first, each with edit / delete actions.
  import { fmtMoney } from './api.js';
  import { t } from '$lib/i18n';

  let {
    asset,
    template = null,
    revisions = [],
    onsubmit = () => {},
    onupdate = () => {},
    ondelete = () => {},
    oncancel = () => {}
  } = $props();

  const customFields = $derived(template ? (template.fields ?? []).filter((f) => !f.reserved) : []);
  const today = new Date().toISOString().slice(0, 10);

  // The revision currently being edited (null = adding a new one). Prefill from the most
  // recent revision when adding, so price/quantity carry forward.
  let editing = $state(null);

  let r = $state(blank());

  function blank() {
    const prev = revisions[0] ?? null;
    return {
      valued_at: today,
      price: prev?.price ?? null,
      quantity: prev?.quantity ?? null,
      fields: { ...(prev?.fields ?? {}) },
      note: ''
    };
  }

  function startEdit(rev) {
    editing = rev;
    r = {
      valued_at: rev.valued_at,
      price: rev.price ?? null,
      quantity: rev.quantity ?? null,
      fields: { ...(rev.fields ?? {}) },
      note: rev.note ?? ''
    };
  }
  function cancelEdit() {
    editing = null;
    r = blank();
  }

  function num(v) {
    return v === '' || v === null || v === undefined ? null : Number(v);
  }

  const value = $derived.by(() => {
    const p = num(r.price);
    const q = num(r.quantity);
    if (p != null && q != null) return p * q;
    if (p != null) return p;
    return 0;
  });

  function submit() {
    const payload = {
      valued_at: r.valued_at || null,
      price: num(r.price),
      quantity: num(r.quantity),
      value: undefined,
      fields: r.fields,
      note: r.note || null
    };
    if (editing) onupdate(editing.id, payload);
    else onsubmit(payload);
    cancelEdit();
  }
</script>

<form class="upd-form" onsubmit={(e) => { e.preventDefault(); submit(); }}>
  <div class="grid">
    <label class="field">
      <span>{$t('wealth.updateForm.date')}</span>
      <input type="date" bind:value={r.valued_at} />
    </label>

    <label class="field"><span>{$t('wealth.updateForm.price', { currency: asset.currency })}</span><input type="number" step="any" bind:value={r.price} /></label>
    <label class="field"><span>{$t('wealth.updateForm.quantity')}</span><input type="number" step="any" bind:value={r.quantity} placeholder="1" /></label>

    {#each customFields as f (f.key)}
      <label class="field" class:wide={f.type === 'textarea'}>
        <span>{f.label}</span>
        {#if f.type === 'textarea'}
          <textarea bind:value={r.fields[f.key]}></textarea>
        {:else if f.type === 'number'}
          <input type="number" step="any" bind:value={r.fields[f.key]} />
        {:else if f.type === 'date'}
          <input type="date" bind:value={r.fields[f.key]} />
        {:else}
          <input bind:value={r.fields[f.key]} />
        {/if}
      </label>
    {/each}
  </div>

  <label class="field wide"><span>{$t('wealth.updateForm.note')}</span><input bind:value={r.note} placeholder={$t('wealth.assetForm.optionalPlaceholder')} /></label>

  <div class="value-bar">
    <span>{editing ? $t('wealth.updateForm.editedValue') : $t('wealth.updateForm.newValue')}</span>
    <strong>{fmtMoney(value, asset.currency)}</strong>
  </div>

  <div class="actions">
    {#if editing}
      <button type="button" class="ghost" onclick={cancelEdit}>{$t('wealth.updateForm.cancelEdit')}</button>
      <button type="submit" class="primary">{$t('wealth.updateForm.saveChanges')}</button>
    {:else}
      <button type="button" class="ghost" onclick={oncancel}>{$t('wealth.updateForm.close')}</button>
      <button type="submit" class="primary">{$t('wealth.updateForm.recordUpdate')}</button>
    {/if}
  </div>
</form>

<div class="history">
  <h4>{$t('wealth.updateForm.history')} <span class="muted">{revisions.length}</span></h4>
  {#if revisions.length === 0}
    <p class="muted empty">{$t('wealth.updateForm.noUpdates')}</p>
  {:else}
    <ul class="rev-list">
      {#each revisions as rev (rev.id)}
        <li class="rev" class:active={editing?.id === rev.id}>
          <div class="rev-main">
            <span class="rev-date">{rev.valued_at}</span>
            <span class="rev-val">{fmtMoney(rev.value, asset.currency)}</span>
          </div>
          {#if rev.note}<span class="rev-note">{rev.note}</span>{/if}
          <div class="rev-actions">
            <button type="button" class="icon" title={$t('wealth.updateForm.editTitle')} onclick={() => startEdit(rev)}><Icon name="pencil" size={14} /></button>
            <button type="button" class="icon" title={$t('wealth.updateForm.deleteTitle')} onclick={() => ondelete(rev)}><Icon name="trash" size={14} /></button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .upd-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: var(--space-3);
  }
  .field.wide {
    grid-column: 1 / -1;
    width: 100%;
  }
  .value-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-2) var(--space-3);
    background: var(--surface-2, var(--surface));
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 0.85rem;
  }
  .value-bar strong {
    font-size: 1.1rem;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }

  .history {
    margin-top: var(--space-4);
    border-top: 1px solid var(--border);
    padding-top: var(--space-3);
  }
  .history h4 {
    font-size: 0.78rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
    margin-bottom: var(--space-2);
  }
  .history h4 .muted {
    margin-left: 4px;
  }
  .rev-list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 220px;
    overflow-y: auto;
  }
  .rev {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    gap: var(--space-2);
    padding: 6px 9px;
    background: var(--surface-2, var(--surface));
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .rev.active {
    border-color: var(--accent);
  }
  .rev-main {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    min-width: 0;
  }
  .rev-date {
    font-size: 0.8rem;
    color: var(--muted);
    white-space: nowrap;
  }
  .rev-val {
    font-size: 0.9rem;
    font-weight: 600;
  }
  .rev-note {
    grid-column: 1 / 2;
    font-size: 0.75rem;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rev-actions {
    grid-row: 1 / 2;
    grid-column: 2 / 3;
    display: flex;
    gap: 2px;
  }
  .empty {
    font-size: 0.82rem;
  }
  .muted {
    color: var(--muted);
  }
</style>
