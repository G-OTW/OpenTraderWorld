<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Template creator. List templates; build one from scratch by adding reserved and/or
  // custom fields, ordering them, then saving with a name. Also seeds the prebuilt one.
  import {
    journalApi,
    RESERVED_FIELDS,
    CUSTOM_FIELD_TYPES,
    PREBUILT_TEMPLATE,
    shortId
  } from './api.js';
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let { templates = [], feeSchedules = [], onchanged = () => {} } = $props();

  // Inline validation + modal confirm (replace native alert()/confirm()).
  let nameError = $state('');
  let confirmOpen = $state(false);
  let confirmMessage = $state('');
  let onConfirmYes = $state(() => {});
  function askConfirm(message, onyes) {
    confirmMessage = message;
    onConfirmYes = onyes;
    confirmOpen = true;
  }

  let showEditor = $state(false);
  let editing = $state(null);

  // Live search: case-insensitive match on template name + description.
  let search = $state('');
  const filteredTemplates = $derived.by(() => {
    const q = search.trim().toLowerCase();
    if (!q) return templates;
    return templates.filter(
      (t) =>
        (t.name ?? '').toLowerCase().includes(q) ||
        (t.description ?? '').toLowerCase().includes(q)
    );
  });

  // Editor working state.
  let name = $state('');
  let description = $state('');
  let fields = $state([]); // [{ key, label, type, reserved, options }]
  let defaultFeeScheduleId = $state('');

  function openNew() {
    editing = null;
    name = '';
    description = '';
    fields = [];
    defaultFeeScheduleId = '';
    showEditor = true;
  }

  function openPrebuilt() {
    editing = null;
    name = PREBUILT_TEMPLATE.name;
    description = PREBUILT_TEMPLATE.description;
    fields = PREBUILT_TEMPLATE.fields.map((f) => ({ ...f }));
    defaultFeeScheduleId = '';
    showEditor = true;
  }

  function openEdit(t) {
    if (t.is_builtin) return;
    editing = t;
    name = t.name;
    description = t.description ?? '';
    fields = (t.fields ?? []).map((f) => ({ options: {}, ...f }));
    defaultFeeScheduleId = t.default_fee_schedule_id ?? '';
    showEditor = true;
  }

  function addReserved(reserved) {
    const def = RESERVED_FIELDS.find((r) => r.reserved === reserved);
    if (!def) return;
    if (fields.some((f) => f.reserved === reserved)) return; // one per reserved column
    fields = [...fields, { key: reserved, label: def.label, type: def.type, reserved }];
  }

  function addCustom() {
    fields = [
      ...fields,
      { key: `c_${shortId()}`, label: 'New field', type: 'text', reserved: null, options: {} }
    ];
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

  function setChoices(i, value) {
    const copy = [...fields];
    copy[i] = {
      ...copy[i],
      options: { ...(copy[i].options ?? {}), choices: value.split(',').map((s) => s.trim()).filter(Boolean) }
    };
    fields = copy;
  }

  const usedReserved = $derived(new Set(fields.filter((f) => f.reserved).map((f) => f.reserved)));

  let saving = $state(false);

  async function save() {
    if (saving) return; // a second click would create a second template
    if (!name.trim()) {
      nameError = $t('journal.templates.nameRequired');
      return;
    }
    nameError = '';
    const payload = {
      name: name.trim(),
      description: description || null,
      fields,
      default_fee_schedule_id: defaultFeeScheduleId || null
    };
    saving = true;
    try {
      if (editing) {
        await journalApi.updateTemplate(editing.id, payload);
      } else {
        await journalApi.addTemplate(payload);
      }
      showEditor = false;
      onchanged();
    } finally {
      saving = false;
    }
  }

  function del(tpl) {
    if (tpl.is_builtin) return;
    askConfirm($t('journal.templates.confirmDelete', { name: tpl.name }), async () => {
      await journalApi.deleteTemplate(tpl.id);
      onchanged();
    });
  }
</script>

<div class="templates">
  <div class="toolbar">
    <Button variant="primary" icon="plus" onclick={openNew}>{$t('journal.templates.newTemplate')}</Button>
    <button class="chip" onclick={openPrebuilt}>{$t('journal.templates.startFromPrebuilt')}</button>
    {#if templates.length > 0}
      <input
        class="search"
        type="search"
        placeholder={$t('journal.templates.searchPlaceholder')}
        bind:value={search}
      />
    {/if}
  </div>

  {#if templates.length === 0}
    <EmptyState icon="clipboard-list" description={$t('journal.templates.empty')}>
      {#snippet action()}
        <Button variant="primary" icon="plus" onclick={openNew}>
          {$t('journal.templates.newTemplate')}
        </Button>
      {/snippet}
    </EmptyState>
  {:else if filteredTemplates.length === 0}
    <!-- Nothing matched the search: the way out is clearing it, so offer it here. -->
    <EmptyState icon="search" description={$t('journal.templates.noMatch', { search })} compact>
      {#snippet action()}
        <Button icon="x" onclick={() => (search = '')}>{$t('common.clear')}</Button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="grid">
      {#each filteredTemplates as tpl (tpl.id)}
        <div class="tcard">
          <div class="tcard-head">
            <span class="tname">{tpl.name}</span>
            {#if tpl.is_builtin}<Badge>{$t('journal.templates.builtIn')}</Badge>{/if}
          </div>
          {#if tpl.description}<p class="tdesc">{tpl.description}</p>{/if}
          <p class="count">{$t('journal.templates.fieldCount', { count: tpl.fields?.length ?? 0 })}</p>
          <div class="tcard-actions">
            {#if !tpl.is_builtin}
              <button class="link" onclick={() => openEdit(tpl)}>{$t('journal.templates.edit')}</button>
              <button class="link danger" onclick={() => del(tpl)}>{$t('journal.fees.deleteModal.confirm')}</button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<Modal bind:open={showEditor} title={editing ? $t('journal.templates.editTitle') : $t('journal.templates.newTitle')}>
  <div class="editor">
    <label class="field">
      <span>{$t('journal.templates.name')}</span>
      <!-- The invalid ring is colour alone; aria-invalid states it, and aria-describedby
           ties the message to the field so a reader hears them together. Not <ErrorText>:
           it renders a <p>, which a <label> may not contain. -->
      <input
        bind:value={name}
        oninput={() => (nameError = '')}
        class:invalid={nameError}
        aria-invalid={nameError ? 'true' : undefined}
        aria-describedby={nameError ? 'tpl-name-error' : undefined}
      />
      {#if nameError}<span id="tpl-name-error" class="err" role="alert">{nameError}</span>{/if}
    </label>
    <label class="field"
      ><span>{$t('journal.templates.description')}</span><input bind:value={description} placeholder={$t('journal.templates.optional')} /></label
    >

    <label class="field">
      <span>{$t('journal.templates.defaultFeeSchedule')}</span>
      <select bind:value={defaultFeeScheduleId}>
        <option value="">{$t('journal.templates.none')}</option>
        {#each feeSchedules as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
      </select>
      <small class="hint">{$t('journal.templates.defaultFeeScheduleHint')}</small>
    </label>

    <div class="add-controls">
      <div class="reserved-picker">
        <span class="lbl">{$t('journal.templates.addReservedField')}</span>
        <div class="chips">
          {#each RESERVED_FIELDS as r (r.reserved)}
            <button
              class="mini"
              disabled={usedReserved.has(r.reserved)}
              onclick={() => addReserved(r.reserved)}>{r.label}</button
            >
          {/each}
        </div>
      </div>
      <button class="chip" onclick={addCustom}>{$t('journal.templates.addCustomField')}</button>
    </div>

    <div class="field-list">
      {#if fields.length === 0}
        <p class="muted">{$t('journal.templates.fieldsHint')}</p>
      {/if}
      {#each fields as f, i (f.key)}
        <div class="frow">
          <div class="order">
            <button class="icon" onclick={() => move(i, -1)} disabled={i === 0}><Icon name="arrow-up" size={12} /></button>
            <button class="icon" onclick={() => move(i, 1)} disabled={i === fields.length - 1}
              ><Icon name="arrow-down" size={12} /></button
            >
          </div>
          <input class="flabel" bind:value={f.label} placeholder={$t('journal.templates.labelPlaceholder')} />
          {#if f.reserved}
            <span class="tag">{$t('journal.templates.reservedTag', { reserved: f.reserved })}</span>
          {:else}
            <select bind:value={f.type}>
              {#each CUSTOM_FIELD_TYPES as ct (ct.id)}<option value={ct.id}>{ct.label}</option>{/each}
            </select>
            {#if f.type === 'select'}
              <input
                class="choices"
                placeholder={$t('journal.templates.choicesPlaceholder')}
                value={(f.options?.choices ?? []).join(', ')}
                oninput={(e) => setChoices(i, e.target.value)}
              />
            {/if}
          {/if}
          <button class="icon danger-hover" onclick={() => removeField(i)} aria-label={$t('journal.templates.removeField')} title={$t('journal.templates.removeField')}><Icon name="x" size={13} /></button>
        </div>
      {/each}
    </div>

    <div class="actions">
      <Button variant="ghost" onclick={() => (showEditor = false)} disabled={saving}>
        {$t('common.cancel')}
      </Button>
      <Button variant="primary" onclick={save} loading={saving}>
        {editing ? $t('journal.templates.saveTemplate') : $t('journal.templates.createTemplate')}
      </Button>
    </div>
  </div>
</Modal>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('journal.templates.deleteTitle')}
  message={confirmMessage}
  confirmLabel={$t('journal.fees.deleteModal.confirm')}
  danger
  onconfirm={onConfirmYes}
/>

<style>
  /* A field error is quieter than a block error (no icon, --text-xs) — it sits under the
     input it belongs to. --red-ink, not --red: the raw hue is a fill colour and drops to
     4.35:1 as text on --surface-2. */
  .err {
    color: var(--red-ink);
    font-size: var(--text-xs);
    margin-top: 4px;
  }
  .invalid {
    border-color: var(--red) !important;
  }
  .templates {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .toolbar {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  /* The global control layer already dresses `input`; only its placement is local. */
  .search {
    margin-left: auto;
    width: 220px;
    max-width: 100%;
  }
  /* Single horizontal strip; scrolls sideways with no visible scrollbar. */
  .grid {
    display: flex;
    gap: var(--space-3);
    overflow-x: auto;
    overflow-y: hidden;
    padding-bottom: var(--space-1);
    scrollbar-width: none; /* Firefox */
    -ms-overflow-style: none; /* IE/Edge */
    scroll-snap-type: x proximity;
  }
  .grid::-webkit-scrollbar {
    display: none; /* Chrome/Safari */
  }
  .tcard {
    flex: 0 0 240px;
    scroll-snap-align: start;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .tcard-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .tname {
    font-weight: var(--fw-semibold);
  }
  /* The "built-in" pill is Badge.svelte now. */
  .tdesc {
    font-size: var(--text-sm);
    color: var(--muted);
    line-height: var(--lh-base);
  }
  .count {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .tcard-actions {
    display: flex;
    gap: var(--space-3);
    margin-top: auto;
  }
  /* button.link / button.link.danger come from theme/components.css. */

  /* Editor */
  .editor {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-height: 70vh;
    overflow-y: auto;
  }
  .add-controls {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    padding: var(--space-3) 0;
  }
  .lbl {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 4px;
  }
  .mini {
    font-size: var(--text-xs);
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    border-radius: 999px;
    padding: 3px 8px;
    cursor: pointer;
    transition: border-color var(--dur-fast) var(--ease);
  }
  .mini:hover:not(:disabled) {
    border-color: var(--accent);
  }
  .mini:disabled {
    opacity: 0.4;
    cursor: not-allowed;
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
  /* The reorder arrows are SVG icons, so they take their size from the Icon prop,
     not from a font-size. Only the box needs tightening. */
  .order .icon {
    line-height: 1;
    padding: 1px;
    height: auto;
    width: auto;
  }
  .flabel {
    flex: 1;
  }
  .choices {
    flex: 1;
  }
  .tag {
    font-size: var(--text-xs);
    color: var(--accent);
    white-space: nowrap;
  }
  /* Sticky footer inside a scrolling modal body. */
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    position: sticky;
    bottom: 0;
    background: var(--surface);
    padding-top: var(--space-2);
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-xs);
  }
</style>
