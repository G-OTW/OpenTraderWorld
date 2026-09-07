<script>
  import Modal from './Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from './Icon.svelte';
  import { t } from '$lib/i18n';

  // A small form modal. `fields` is an array of:
  //   { key, label, placeholder?, type?, value?, required?, options? }
  // type 'select' renders a dropdown from options: [{ value, label }].
  // type 'color' renders the shared swatch row (none + palette + custom picker), so a
  // colour is picked where the thing is edited rather than from a popover of its own.
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

  // Identifying colours, shared with the news sources and the editor chips.
  const COLOR_SWATCHES = ['#7d8a99', '#c9776b', '#b79a6b', '#7fb894', '#8494a7', '#9a95a3', '#b58a94'];

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
      {#if f.type === 'color'}
        <!-- Buttons can't live in a <label>: it would forward every click to the
             colour input nested at the end of the row. -->
        <div class="field">
          {#if f.label}<span class="lbl">{f.label}</span>{/if}
          <div class="colors">
            <button
              type="button"
              class="swatch none"
              class:selected={!values[f.key]}
              title={$t('common.colorNone')}
              aria-label={$t('common.colorNone')}
              onclick={() => (values[f.key] = '')}><Icon name="x" size={12} /></button
            >
            {#each f.options ?? COLOR_SWATCHES as hex}
              <button
                type="button"
                class="swatch"
                class:selected={String(values[f.key] ?? '').toLowerCase() === hex}
                style:background={hex}
                title={hex}
                aria-label={hex}
                onclick={() => (values[f.key] = hex)}
              ></button>
            {/each}
            <label class="swatch custom" title={$t('common.colorCustom')}>
              <input
                type="color"
                value={values[f.key] || '#7d8a99'}
                oninput={(e) => (values[f.key] = e.target.value)}
              />
              <Icon name="pencil" size={12} />
            </label>
          </div>
        </div>
      {:else}
        <label class="field">
          {#if f.label}<span class="lbl">{f.label}</span>{/if}
          {#if f.type === 'select'}
            <Dropdown
              bind:value={values[f.key]}
              ariaLabel={f.label}
              options={(f.options ?? []).map((o) => ({ value: o.value, label: o.label }))}
            />
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
      {/if}
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
  .field input {
    width: 100%;
  }
  .colors {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .swatch {
    width: 22px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border-control);
    border-radius: var(--radius);
    cursor: pointer;
    padding: 0;
    color: var(--muted);
    background: transparent;
  }
  .swatch.selected {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .swatch.custom {
    position: relative;
    overflow: hidden;
  }
  /* The native picker covers the tile; the pencil is what the user sees. */
  .swatch.custom input[type='color'] {
    position: absolute;
    inset: 0;
    opacity: 0;
    width: 100%;
    height: 100%;
    cursor: pointer;
    padding: 0;
    border: none;
  }
</style>
