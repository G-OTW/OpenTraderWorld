<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Record / edit value updates (revisions) for an asset. The form adds a new revision or
  // saves edits to the one being edited. Below it, a scrollable list shows every update
  // (date + value), newest first, each with edit / delete actions.
  import Delta from '$lib/ui/charts/Delta.svelte';
  import { money } from './view.js';
  import { dateKey } from '$lib/format.js';
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
  // Local day. toISOString() would prefill tomorrow's date for anyone west of Greenwich
  // late in the evening, and yesterday's for anyone east of it early in the morning.
  const today = dateKey();

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
    // No price typed yet: there is no value to show, not a value of zero.
    return null;
  });

  // Each revision's change against the one before it (the list is newest first).
  const changeAt = (i) => {
    const a = revisions[i]?.value;
    const b = revisions[i + 1]?.value;
    return a == null || b == null ? null : a - b;
  };

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
    <label class="field wide"><span>{$t('wealth.updateForm.note')}</span><input bind:value={r.note} placeholder={$t('wealth.assetForm.optionalPlaceholder')} /></label>

    {#each customFields as f (f.key)}
      <label class="field" class:full={f.type === 'textarea'}>
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

  <div class="value-bar">
    <span class="lbl">{editing ? $t('wealth.updateForm.editedValue') : $t('wealth.updateForm.newValue')}</span>
    <strong class:word={value == null}>{money(value, asset.currency, $t('wealth.page.na'))}</strong>
    <span class="grow"></span>
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
  <h4>{$t('wealth.updateForm.history')} <span class="n">{revisions.length}</span></h4>
  {#if revisions.length === 0}
    <p class="empty">{$t('wealth.updateForm.noUpdates')}</p>
  {:else}
    <div class="rev-wrap">
      <table class="tbl">
        <thead>
          <tr>
            <th class="c-date">{$t('wealth.updateForm.date')}</th>
            <th class="num c-val">{$t('wealth.pair.value')}</th>
            <th class="num c-chg">{$t('wealth.movers.change')}</th>
            <th>{$t('wealth.updateForm.note')}</th>
            <th class="c-act"></th>
          </tr>
        </thead>
        <tbody>
          {#each revisions as rev, i (rev.id)}
            <tr class:active={editing?.id === rev.id}>
              <td class="mono dim">{rev.valued_at}</td>
              <td class="num">{money(rev.value, asset.currency)}</td>
              <td class="num">
                {#if changeAt(i) != null}<Delta value={changeAt(i)} format={(v) => money(v, asset.currency)} invert={asset.sign < 0} arrow={false} sensitive />
                {:else}<span class="first">{$t('wealth.detail.first')}</span>{/if}
              </td>
              <td class="note" title={rev.note ?? ''}>{rev.note ?? ''}</td>
              <td class="acts">
                <button type="button" class="icon" title={$t('wealth.updateForm.editTitle')} aria-label="{$t('wealth.updateForm.editTitle')}: {rev.valued_at}" onclick={() => startEdit(rev)}><Icon name="pencil" size={13} /></button>
                <button type="button" class="icon danger-hover" title={$t('wealth.updateForm.deleteTitle')} aria-label="{$t('wealth.updateForm.deleteTitle')}: {rev.valued_at}" onclick={() => ondelete(rev)}><Icon name="trash" size={13} /></button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
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
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .field.wide {
    grid-column: span 1;
  }
  .field.full {
    grid-column: 1 / -1;
  }
  .value-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-top: var(--hairline) solid var(--border);
    border-bottom: var(--hairline) solid var(--border);
  }
  .lbl {
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .value-bar strong {
    font-size: 15px;
    font-weight: 600;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .value-bar strong.word {
    color: var(--dim);
    font-family: var(--font);
    font-size: 13px;
    font-weight: 400;
  }
  .grow {
    flex: 1;
  }
  .history {
    margin-top: var(--space-4);
  }
  .history h4 {
    margin: 0 0 var(--space-2);
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .n {
    margin-left: 4px;
    font-family: var(--mono);
  }
  .rev-wrap {
    max-height: 260px;
    overflow-y: auto;
  }
  table {
    width: 100%;
    table-layout: fixed;
  }
  thead th {
    position: sticky;
    top: 0;
    background: var(--surface-raised);
    z-index: 1;
  }
  .c-date {
    width: 110px;
  }
  .c-val {
    width: 150px;
  }
  .c-chg {
    width: 140px;
  }
  .c-act {
    width: 68px;
  }
  .mono {
    font-family: var(--mono);
    font-size: 12px;
  }
  .dim {
    color: var(--muted);
  }
  .first {
    color: var(--dim);
    font-family: var(--font);
    font-size: 11px;
  }
  .note {
    color: var(--muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .acts {
    text-align: right;
    white-space: nowrap;
  }
  tr.active td {
    background: var(--accent-soft);
  }
  .empty {
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }
  /* Phone: one field per line, the actions take a finger, the note column goes. */
  @media (max-width: 767px) {
    .grid { grid-template-columns: minmax(0, 1fr); }
    .value-bar { flex-wrap: wrap; overflow-wrap: anywhere; }
    .value-bar button { flex: 1 1 auto; min-height: 44px; }
    .grow { display: none; }
    .note, th:nth-child(4) { display: none; }
    .c-val { width: auto; }
    .c-chg { width: 110px; }
    .acts .icon { min-width: 40px; min-height: 44px; }
  }
</style>
