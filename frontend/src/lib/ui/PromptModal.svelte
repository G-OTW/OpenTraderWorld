<script>
  import Modal from './Modal.svelte';
  import { t } from '$lib/i18n';

  // A small form modal. `fields` is an array of:
  //   { key, label, placeholder?, type?, value?, required?, options? }
  // type 'select' renders a dropdown from options: [{ value, label }].
  // props:
  //   open (bindable), title, fields, confirmLabel, onconfirm(values), oncancel
  let {
    open = $bindable(false),
    title = '',
    fields = [],
    confirmLabel = null, // falls back to a translated "OK" when unset

    onconfirm = () => {},
    oncancel = () => {}
  } = $props();

  // Local editable copy of the field values, keyed by field.key.
  let values = $state({});

  // Reset values whenever the modal opens (or its fields change).
  $effect(() => {
    if (open) {
      const next = {};
      for (const f of fields) next[f.key] = f.value ?? '';
      values = next;
    }
  });

  const canConfirm = $derived(fields.every((f) => !f.required || String(values[f.key] ?? '').trim()));

  function confirm() {
    if (!canConfirm) return;
    open = false;
    onconfirm({ ...values });
  }
  function cancel() {
    open = false;
    oncancel();
  }
</script>

<Modal bind:open {title} onclose={oncancel}>
  <form
    onsubmit={(e) => {
      e.preventDefault();
      confirm();
    }}
  >
    {#each fields as f, i (f.key)}
      <label class="field">
        {#if f.label}<span class="lbl">{f.label}</span>{/if}
        {#if f.type === 'select'}
          <select bind:value={values[f.key]}>
            {#each f.options ?? [] as o (o.value)}
              <option value={o.value}>{o.label}</option>
            {/each}
          </select>
        {:else}
          <!-- svelte-ignore a11y_autofocus -->
          <input
            type={f.type ?? 'text'}
            placeholder={f.placeholder ?? ''}
            bind:value={values[f.key]}
            autofocus={i === 0}
          />
        {/if}
      </label>
    {/each}
    <!-- hidden submit lets Enter confirm -->
    <button type="submit" hidden></button>
  </form>

  {#snippet footer()}
    <button class="ghost" onclick={cancel}>{$t('common.cancel')}</button>
    <button class="primary" onclick={confirm} disabled={!canConfirm}>{confirmLabel ?? $t('common.ok')}</button>
  {/snippet}
</Modal>

<style>
  .field {
    display: block;
    margin-bottom: var(--space-3);
  }
  .field:last-of-type {
    margin-bottom: 0;
  }
  .lbl {
    display: block;
    color: var(--muted);
    font-size: var(--text-xs);
    margin-bottom: 4px;
  }
  .field input,
  .field select {
    width: 100%;
  }
</style>
