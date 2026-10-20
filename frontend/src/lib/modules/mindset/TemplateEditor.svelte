<script>
  // Create/edit a check-in template. Three stacked sections, in the order a check-in is
  // actually thought about: what it is (name, category, phase, description, notes), and
  // what it asks (the prompt list).
  //
  // A prompt keeps every option the old Customize modal had — scale 1–5 with its two end
  // labels, single choice, multi tags, free text — but each is now edited in place with a
  // live preview of the control the trader will see, instead of a comma-separated string
  // in a bare input. An optional hint line sits under any prompt that needs framing.
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import RichNote from '$lib/ui/RichNote.svelte';
  import { mindsetApi, KINDS, PHASES, optionPresets } from './api.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    templateId = null,
    categories = [],
    // Pre-selects a category when the page opens the editor from inside a category group.
    presetCategoryId = null,
    presetPhase = 'pre',
    onsaved = () => {}
  } = $props();

  let name = $state('');
  let description = $state('');
  let notes = $state('');
  let categoryId = $state('');
  let phase = $state('pre');
  let active = $state(true);
  let prompts = $state([]); // [{ id?, kind, label, hint, config, _open }]

  let error = $state('');
  let saving = $state(false);
  let loading = $state(false);

  // `_open` is view state (is this prompt expanded) and never leaves the component.
  function blankPrompt(kind = 'scale') {
    return { kind, label: '', hint: '', config: defaultConfig(kind), _open: true };
  }

  // A kind switch rewrites config rather than clearing it, so the preview never breaks:
  // scales need their two end labels, choice/tags need at least one option.
  function defaultConfig(kind) {
    if (kind === 'scale') return { low: '', high: '' };
    if (kind === 'choice' || kind === 'tags') return { options: [''] };
    return {};
  }

  $effect(() => {
    if (!open) return;
    error = '';
    if (templateId) {
      loading = true;
      mindsetApi
        .templateDetail(templateId)
        .then((r) => {
          name = r.template.name;
          description = r.template.description ?? '';
          notes = r.template.notes ?? '';
          categoryId = r.template.category_id ?? '';
          phase = r.template.phase;
          active = r.template.active;
          prompts = r.prompts.map((p) => ({
            id: p.id,
            kind: p.kind,
            label: p.label,
            hint: p.hint ?? '',
            config: { ...defaultConfig(p.kind), ...(p.config ?? {}) },
            _open: false
          }));
        })
        .catch((e) => (error = e.message))
        .finally(() => (loading = false));
    } else {
      name = '';
      description = '';
      notes = '';
      categoryId = presetCategoryId ?? '';
      phase = presetPhase;
      active = true;
      prompts = [blankPrompt()];
    }
  });

  function addPrompt(kind = 'scale') {
    prompts = [...prompts, blankPrompt(kind)];
  }
  function removePrompt(idx) {
    prompts = prompts.filter((_, i) => i !== idx);
  }
  function toggleDetail(idx) {
    prompts = prompts.map((p, i) => (i === idx ? { ...p, _open: !p._open } : p));
  }
  function move(idx, delta) {
    const to = idx + delta;
    if (to < 0 || to >= prompts.length) return;
    const next = [...prompts];
    [next[idx], next[to]] = [next[to], next[idx]];
    prompts = next;
  }
  function setKind(idx, kind) {
    prompts = prompts.map((p, i) =>
      i === idx ? { ...p, kind, config: { ...defaultConfig(kind), ...keepCompatible(p, kind) } } : p
    );
  }

  // Moving between two option-based kinds keeps the options the user already typed.
  function keepCompatible(p, kind) {
    const optionKinds = ['choice', 'tags'];
    if (optionKinds.includes(kind) && optionKinds.includes(p.kind)) {
      return { options: p.config?.options ?? [''] };
    }
    return {};
  }

  function setConfig(idx, patch) {
    prompts = prompts.map((p, i) => (i === idx ? { ...p, config: { ...p.config, ...patch } } : p));
  }
  function setOption(idx, oi, value) {
    const opts = [...(prompts[idx].config.options ?? [])];
    opts[oi] = value;
    setConfig(idx, { options: opts });
  }
  function addOption(idx) {
    setConfig(idx, { options: [...(prompts[idx].config.options ?? []), ''] });
  }
  function removeOption(idx, oi) {
    const opts = (prompts[idx].config.options ?? []).filter((_, i) => i !== oi);
    setConfig(idx, { options: opts.length ? opts : [''] });
  }
  function applyPreset(idx, preset) {
    setConfig(idx, { options: [...preset.options] });
  }

  // Enter in a prompt label adds the next one — a check-in is written in one pass.
  function onLabelKeydown(e, idx) {
    if (e.key !== 'Enter') return;
    e.preventDefault();
    if (idx === prompts.length - 1) addPrompt(prompts[idx].kind);
  }

  const presets = $derived(optionPresets($t));
  const filled = $derived(prompts.filter((p) => p.label.trim()).length);

  async function save() {
    const cleaned = prompts
      .map((p) => ({
        id: p.id,
        kind: p.kind,
        label: p.label.trim(),
        hint: (p.hint ?? '').trim(),
        config: cleanConfig(p)
      }))
      .filter((p) => p.label);

    if (!name.trim()) {
      error = $t('mindset.builder.nameRequired');
      return;
    }
    if (cleaned.length === 0) {
      error = $t('mindset.builder.addAtLeastOnePrompt');
      return;
    }
    const emptyOptions = cleaned.find(
      (p) => ['choice', 'tags'].includes(p.kind) && (p.config.options ?? []).length === 0
    );
    if (emptyOptions) {
      error = $t('mindset.builder.optionsRequired', { label: emptyOptions.label });
      return;
    }

    saving = true;
    error = '';
    const payload = {
      name: name.trim(),
      description: description.trim(),
      notes,
      category_id: categoryId || null,
      phase,
      prompts: cleaned
    };
    try {
      if (templateId) await mindsetApi.updateTemplate(templateId, { ...payload, active });
      else await mindsetApi.createTemplate(payload);
      open = false;
      onsaved();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  // Blank option rows are scaffolding, not data — they never reach the server.
  function cleanConfig(p) {
    if (p.kind === 'scale') {
      return { low: (p.config?.low ?? '').trim(), high: (p.config?.high ?? '').trim() };
    }
    if (p.kind === 'choice' || p.kind === 'tags') {
      return { options: (p.config?.options ?? []).map((o) => o.trim()).filter(Boolean) };
    }
    return {};
  }

  // Deleting a template takes its check-in history with it — ConfirmModal, not confirm().
  // Snapshot the id: confirming closes this modal, which may clear `templateId`.
  let confirmOpen = $state(false);
  let pendingDelete = $state(null);

  function remove() {
    if (!templateId) return;
    pendingDelete = templateId;
    confirmOpen = true;
  }

  async function confirmDelete() {
    const id = pendingDelete;
    pendingDelete = null;
    if (!id) return;
    try {
      await mindsetApi.deleteTemplate(id);
      open = false;
      onsaved();
    } catch (e) {
      error = e.message;
    }
  }
</script>

<Modal
  bind:open
  size="lg"
  title={templateId ? $t('mindset.builder.editTemplate') : $t('mindset.builder.newTemplate')}
>
  <div class="form">
    {#if loading}
      <p class="loading">{$t('common.loading')}</p>
    {/if}

    <!-- ── 1. Identity ── -->
    <section>
      <h3 class="shead">{$t('mindset.builder.aboutSection')}</h3>
      <div class="row">
        <label class="grow">
          {$t('mindset.builder.name')}
          <input placeholder={$t('mindset.builder.namePlaceholder')} bind:value={name} />
        </label>
        <div class="fld">
          <span class="flbl">{$t('mindset.builder.category')}</span>
          <Dropdown
            bind:value={categoryId}
            ariaLabel={$t('mindset.builder.category')}
            options={[
              { value: '', label: $t('mindset.builder.noCategory') },
              ...categories.map((c) => ({ value: c.id, label: c.name, color: c.color }))
            ]}
          />
        </div>
      </div>

      <div class="fld">
        <span class="flbl">{$t('mindset.builder.phase')}</span>
        <div class="chips">
          {#each PHASES as p (p.key)}
            <button
              type="button"
              class="chip"
              class:active={phase === p.key}
              onclick={() => (phase = p.key)}
            >
              {p.icon}
              {$t(`mindset.phase.${p.key}`)}
            </button>
          {/each}
        </div>
        <p class="fhint">{$t(`mindset.phaseHint.${phase}`)}</p>
      </div>

      <label>
        {$t('mindset.builder.description')}
        <input placeholder={$t('mindset.builder.descriptionPlaceholder')} bind:value={description} />
      </label>
      <label class="stack">
        {$t('mindset.builder.notes')}
        <RichNote bind:value={notes} placeholder={$t('mindset.builder.notesPlaceholder')} />
      </label>
    </section>

    <!-- ── 2. Prompts ── -->
    <section>
      <h3 class="shead">
        {$t('mindset.builder.promptsSection')}
        <span class="count">{$t('mindset.builder.promptCount', { n: filled })}</span>
      </h3>

      <ul class="plist">
        {#each prompts as p, i (p.id ?? i)}
          <li class="prow" class:expanded={p._open}>
            <div class="ptop">
              <span class="handle" aria-hidden="true"><Icon name="grip-vertical" size={13} /></span>
              <input
                class="plabel"
                placeholder={$t('mindset.builder.promptPlaceholder')}
                bind:value={p.label}
                onkeydown={(e) => onLabelKeydown(e, i)}
              />
              <button
                type="button"
                class="pbtn kindbtn"
                onclick={() => toggleDetail(i)}
                aria-expanded={p._open}
                title={$t('mindset.builder.editPrompt')}
              >
                <Icon name={KINDS.find((k) => k.key === p.kind)?.icon ?? 'bar-chart'} size={12} />
                {$t(`mindset.kind.${p.kind}`)}
                <Icon name={p._open ? 'chevron-down' : 'chevron-right'} size={12} />
              </button>
              <button type="button" class="pbtn" onclick={() => move(i, -1)} disabled={i === 0} title={$t('mindset.builder.moveUp')} aria-label={$t('mindset.builder.moveUp')}>
                <Icon name="arrow-up" size={13} />
              </button>
              <button type="button" class="pbtn" onclick={() => move(i, 1)} disabled={i === prompts.length - 1} title={$t('mindset.builder.moveDown')} aria-label={$t('mindset.builder.moveDown')}>
                <Icon name="arrow-down" size={13} />
              </button>
              <button type="button" class="pbtn del" onclick={() => removePrompt(i)} title={$t('mindset.builder.removePrompt')} aria-label={$t('mindset.builder.removePrompt')}>
                <Icon name="x" size={13} />
              </button>
            </div>

            {#if p._open}
              <div class="pdetail">
                <!-- Control type -->
                <div class="fld">
                  <span class="flbl">{$t('mindset.builder.controlType')}</span>
                  <div class="chips">
                    {#each KINDS as k (k.key)}
                      <button
                        type="button"
                        class="chip"
                        class:active={p.kind === k.key}
                        onclick={() => setKind(i, k.key)}
                      >
                        <Icon name={k.icon} size={12} />
                        {$t(`mindset.kind.${k.key}`)}
                      </button>
                    {/each}
                  </div>
                </div>

                <!-- Kind-specific config -->
                {#if p.kind === 'scale'}
                  <div class="row">
                    <label class="grow">
                      {$t('mindset.builder.scaleLow')}
                      <input
                        placeholder={$t('mindset.builder.scaleLowPlaceholder')}
                        value={p.config?.low ?? ''}
                        oninput={(e) => setConfig(i, { low: e.currentTarget.value })}
                      />
                    </label>
                    <label class="grow">
                      {$t('mindset.builder.scaleHigh')}
                      <input
                        placeholder={$t('mindset.builder.scaleHighPlaceholder')}
                        value={p.config?.high ?? ''}
                        oninput={(e) => setConfig(i, { high: e.currentTarget.value })}
                      />
                    </label>
                  </div>
                {:else if p.kind === 'choice' || p.kind === 'tags'}
                  <div class="fld">
                    <span class="flbl">{$t('mindset.builder.options')}</span>
                    <!-- One row per option, not a comma-separated string: an option can
                         contain a comma, and a list you can reorder reads better. -->
                    <ul class="opts">
                      {#each p.config?.options ?? [] as opt, oi (oi)}
                        <li>
                          <input
                            value={opt}
                            placeholder={$t('mindset.builder.optionPlaceholder')}
                            oninput={(e) => setOption(i, oi, e.currentTarget.value)}
                          />
                          <button
                            type="button"
                            class="pbtn del"
                            onclick={() => removeOption(i, oi)}
                            aria-label={$t('mindset.builder.removeOption')}
                          >
                            <Icon name="x" size={12} />
                          </button>
                        </li>
                      {/each}
                    </ul>
                    <div class="optfoot">
                      <button type="button" class="add" onclick={() => addOption(i)}>
                        <Icon name="plus" size={12} />
                        {$t('mindset.builder.addOption')}
                      </button>
                      <span class="orlbl">{$t('mindset.builder.orUsePreset')}</span>
                      {#each presets as preset (preset.id)}
                        <button type="button" class="chip sm" onclick={() => applyPreset(i, preset)}>
                          {preset.label}
                        </button>
                      {/each}
                    </div>
                  </div>
                {/if}

                <label>
                  {$t('mindset.builder.hint')}
                  <input placeholder={$t('mindset.builder.hintPlaceholder')} bind:value={p.hint} />
                </label>

                <!-- Live preview: exactly the control the check-in card will render. -->
                <div class="preview">
                  <span class="plbl">{p.label || $t('mindset.builder.promptPlaceholder')}</span>
                  {#if p.hint}<span class="phint">{p.hint}</span>{/if}
                  {#if p.kind === 'scale'}
                    <div class="scale">
                      {#if p.config?.low}<span class="end">{p.config.low}</span>{/if}
                      <div class="steps">
                        {#each [1, 2, 3, 4, 5] as n (n)}<span>{n}</span>{/each}
                      </div>
                      {#if p.config?.high}<span class="end">{p.config.high}</span>{/if}
                    </div>
                  {:else if p.kind === 'choice' || p.kind === 'tags'}
                    <div class="chips">
                      {#each (p.config?.options ?? []).filter(Boolean) as opt (opt)}
                        <span class="chip">{opt}</span>
                      {/each}
                    </div>
                  {:else}
                    <div class="fauxtext">{$t('mindset.checkinCard.writeItDown')}</div>
                  {/if}
                </div>
              </div>
            {/if}
          </li>
        {/each}
      </ul>

      <div class="addrow">
        <span class="orlbl">{$t('mindset.builder.addPromptOfType')}</span>
        {#each KINDS as k (k.key)}
          <button type="button" class="chip" onclick={() => addPrompt(k.key)}>
            <Icon name={k.icon} size={12} />
            {$t(`mindset.kind.${k.key}`)}
          </button>
        {/each}
      </div>
    </section>

    <ErrorText error={error} />

    <div class="foot">
      {#if templateId}
        <button type="button" class="btn danger" onclick={remove}>{$t('common.delete')}</button>
        <label class="pause">
          <input type="checkbox" bind:checked={active} />
          {$t('mindset.builder.activeToggle')}
        </label>
      {/if}
      <div class="spacer"></div>
      <button type="button" class="btn" onclick={() => (open = false)}>{$t('common.cancel')}</button>
      <button type="button" class="btn primary" onclick={save} disabled={saving}>
        {saving ? $t('common.saving') : templateId ? $t('common.save') : $t('mindset.builder.create')}
      </button>
    </div>
  </div>
</Modal>

<!-- After the host Modal in DOM order, so at equal --z-modal it stacks on top. -->
<ConfirmModal
  bind:open={confirmOpen}
  title={$t('mindset.builder.deleteTemplate')}
  message={$t('mindset.builder.confirmDelete')}
  confirmLabel={$t('common.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmDelete}
  oncancel={() => (pendingDelete = null)}
/>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-8);
    padding-bottom: var(--space-2);
  }
  .loading {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
  }

  section {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  /* A quiet caption over a rule, not a heading competing with the modal title. */
  .shead {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
    padding-bottom: var(--space-2);
    border-bottom: 0.5px solid var(--border);
  }
  .count {
    letter-spacing: 0;
    text-transform: none;
    font-weight: 400;
  }

  label,
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .flbl {
    line-height: var(--lh-tight, 1.2);
  }
  .fhint {
    margin: 0;
    font-weight: 400;
    color: var(--faint, var(--muted));
  }
  .row {
    display: flex;
    gap: var(--space-4);
    flex-wrap: wrap;
  }
  .grow {
    flex: 1 1 200px;
  }
  .row .fld {
    flex: 0 1 220px;
  }
  .stack {
    gap: var(--space-2);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .chip.sm {
    font-size: var(--text-xs);
    padding: 2px var(--space-2);
  }

  /* ── Prompt rows ── */
  .plist {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .prow {
    border: 0.5px solid transparent;
    border-radius: var(--radius);
  }
  .prow.expanded {
    border-color: var(--border);
    background: var(--surface-2);
    padding: var(--space-2);
  }
  .ptop {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .handle {
    color: var(--muted);
    display: inline-flex;
    flex: none;
  }
  .plabel {
    flex: 1;
    min-width: 0;
  }
  .pbtn {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 3px 4px;
    border-radius: var(--radius);
    display: inline-flex;
    align-items: center;
    gap: 3px;
    flex: none;
  }
  .pbtn:hover:not(:disabled) {
    color: var(--text);
    background: var(--surface-2);
  }
  .pbtn:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .pbtn.del:hover {
    color: var(--red);
  }
  /* The kind badge doubles as the expander, so the row says what it is and opens on it. */
  .kindbtn {
    border: 0.5px solid var(--border);
    font-size: var(--text-xs);
    white-space: nowrap;
  }

  .pdetail {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-2) var(--space-1) var(--space-4);
  }

  .opts {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .opts li {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .opts input {
    flex: 1;
    min-width: 0;
  }
  .optfoot {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .orlbl {
    font-size: var(--text-xs);
    color: var(--faint, var(--muted));
    font-weight: 400;
  }
  .add {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 0;
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .add:hover {
    color: var(--text);
  }

  /* ── Live preview ── */
  .preview {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    background: var(--surface);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3);
  }
  .preview .plbl {
    font-size: var(--text-sm);
    color: var(--text);
    font-weight: 400;
  }
  .preview .phint {
    font-size: var(--text-xs);
    color: var(--muted);
    font-weight: 400;
  }
  .scale {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .scale .end {
    font-size: var(--text-xs);
    color: var(--muted);
    font-weight: 400;
  }
  .steps {
    display: flex;
    gap: var(--space-1);
  }
  .steps span {
    width: 30px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .fauxtext {
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2);
    min-height: 40px;
    color: var(--faint, var(--muted));
    font-size: var(--text-xs);
    font-weight: 400;
  }

  .addrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .foot {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .pause {
    flex-direction: row;
    align-items: center;
    gap: var(--space-1);
    font-weight: 400;
  }
  .spacer {
    flex: 1;
  }
  .btn {
    background: var(--surface-2);
    border: 0.5px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--border-control);
    font-weight: var(--fw-medium);
  }
  .btn.danger {
    color: var(--red);
    border-color: var(--red);
    background: transparent;
  }
</style>
