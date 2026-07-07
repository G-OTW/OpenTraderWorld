<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Add/edit an indicator instance. In add mode the type is chosen from a searchable
  // dropdown (grouped by pane kind) and the param form rebuilds from the catalog; in edit
  // mode the type is fixed and only params/style are editable. Style overrides (line color,
  // fill color, line width) are optional — blank/zero means "use the indicator's default".
  // Saving emits the {type, params, style} draft to the parent.
  import Modal from '$lib/ui/Modal.svelte';
  import { t } from '$lib/i18n';
  import { CATALOG, catalogDef, defaultParams, DEFAULT_STYLE, PRICE_SOURCES } from './indicators.js';

  let { open = $bindable(false), edit = null, onsave, onclose } = $props();

  // Local draft. Seeded from `edit` when editing, else first catalog entry.
  let type = $state('sma');
  let params = $state({});
  let style = $state({ ...DEFAULT_STYLE });

  // Searchable type picker (add mode only).
  let query = $state('');
  let pickerOpen = $state(false);

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return CATALOG.filter((c) => !q || c.label.toLowerCase().includes(q) || c.type.includes(q));
  });

  // Re-seed only on the open transition (or when the edit target changes), not on every
  // local change — otherwise picking a new type snaps back to the seed.
  let lastOpen = false;
  let lastEdit = null;
  $effect(() => {
    if (open && (!lastOpen || edit !== lastEdit)) {
      if (edit) {
        type = edit.type;
        params = { ...edit.params };
        style = { ...DEFAULT_STYLE, ...(edit.style ?? {}) };
      } else {
        type = CATALOG[0].type;
        params = defaultParams(CATALOG[0].type);
        style = { ...DEFAULT_STYLE };
      }
      query = '';
      pickerOpen = false;
    }
    lastOpen = open;
    lastEdit = edit;
  });

  const def = $derived(catalogDef(type));

  function pick(t) {
    type = t;
    params = defaultParams(t);
    pickerOpen = false;
    query = '';
  }

  function save() {
    // Coerce params to numbers and clamp to bounds.
    const clean = {};
    for (const p of def.params) {
      let v = Number(params[p.key]);
      if (!Number.isFinite(v)) v = p.default;
      clean[p.key] = Math.min(p.max, Math.max(p.min, v));
    }
    // Preserve the (non-numeric) price source for sourceable indicators.
    if (def.sourceable) clean.source = params.source ?? 'close';
    // Normalize style: keep only meaningful overrides.
    const w = Number(style.width);
    const cleanStyle = {
      color: style.color || '',
      // Fill only applies to band/channel indicators; drop it otherwise.
      fill: def.fillable ? style.fill || '' : '',
      width: Number.isFinite(w) && w > 0 ? Math.min(8, w) : 0
    };
    onsave?.({ type, params: clean, style: cleanStyle });
    open = false;
  }
</script>

<Modal bind:open title={edit ? $t('histviz.modal.editTitle') : $t('histviz.modal.addTitle')} {onclose}>
  <div class="form">
    <div class="field">
      <span>{$t('histviz.modal.indicator')}</span>
      {#if edit}
        <div class="fixed">{def.label}</div>
      {:else}
        <div class="combo">
          <button type="button" class="combo-btn" onclick={() => (pickerOpen = !pickerOpen)}>
            {def.label}
            <span class="caret"><Icon name="chevron-down" size={12} /></span>
          </button>
          {#if pickerOpen}
            <div class="menu">
              <!-- svelte-ignore a11y_autofocus -->
              <input
                class="menu-search"
                type="search"
                placeholder={$t('histviz.modal.searchIndicators')}
                bind:value={query}
                autofocus
              />
              <div class="menu-list">
                {#if !filtered.length}
                  <div class="menu-empty">{$t('histviz.picker.noMatches')}</div>
                {/if}
                {#each filtered as c (c.type)}
                  <button
                    type="button"
                    class="menu-item"
                    class:sel={c.type === type}
                    onclick={() => pick(c.type)}
                  >
                    <span class="mi-label">{c.label}</span>
                    <span class="mi-kind {c.kind}">{c.kind === 'overlay' ? $t('histviz.modal.kindOverlay') : $t('histviz.modal.kindPane')}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {/if}
    </div>

    {#each def.params as p (p.key)}
      <label class="field">
        <span>{p.label}</span>
        <input type="number" min={p.min} max={p.max} step={p.step} bind:value={params[p.key]} />
      </label>
    {/each}

    {#if def.sourceable}
      <label class="field">
        <span>{$t('histviz.modal.source')}</span>
        <select class="src" bind:value={params.source}>
          {#each PRICE_SOURCES as s (s.key)}
            <option value={s.key}>{s.label}</option>
          {/each}
        </select>
      </label>
    {/if}

    <div class="style-head">{$t('histviz.modal.style')} <small>{$t('histviz.modal.styleHint')}</small></div>
    <div class="style-row">
      <label class="swatch">
        <span>{$t('histviz.modal.line')}</span>
        <span class="pick">
          <input
            type="color"
            value={style.color || '#4a90d9'}
            oninput={(e) => (style.color = e.currentTarget.value)}
          />
          {#if style.color}<button type="button" class="reset" title={$t('histviz.modal.default')} onclick={() => (style.color = '')}><Icon name="refresh-cw" size={12} /></button>{/if}
        </span>
      </label>
      {#if def.fillable}
        <label class="swatch">
          <span>{$t('histviz.modal.fill')}</span>
          <span class="pick">
            <input
              type="color"
              value={style.fill || '#4a90d9'}
              oninput={(e) => (style.fill = e.currentTarget.value)}
            />
            {#if style.fill}<button type="button" class="reset" title={$t('histviz.modal.default')} onclick={() => (style.fill = '')}><Icon name="refresh-cw" size={12} /></button>{/if}
          </span>
        </label>
      {/if}
      <label class="width">
        <span>{$t('histviz.modal.width')}</span>
        <input type="number" min="0" max="8" step="0.5" placeholder={$t('histviz.modal.auto')} bind:value={style.width} />
      </label>
    </div>
  </div>

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={save}>{edit ? $t('common.save') : $t('histviz.modal.add')}</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  input[type='number'],
  .src {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: 0.9rem;
  }
  .fixed {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: 0.9rem;
  }
  /* Searchable combo */
  .combo {
    position: relative;
  }
  .combo-btn {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: 0.9rem;
    cursor: pointer;
  }
  .caret {
    color: var(--muted);
    font-size: 0.7rem;
  }
  .menu {
    position: absolute;
    z-index: 30;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }
  .menu-search {
    width: 100%;
    background: var(--surface-2);
    border: none;
    border-bottom: 1px solid var(--border);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: 0.85rem;
  }
  .menu-list {
    max-height: 240px;
    overflow-y: auto;
  }
  .menu-empty {
    padding: var(--space-2) var(--space-3);
    color: var(--muted);
    font-size: 0.8rem;
  }
  .menu-item {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    background: transparent;
    border: none;
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: 0.85rem;
    cursor: pointer;
    text-align: left;
  }
  .menu-item:hover {
    background: var(--surface-2);
  }
  .menu-item.sel {
    color: var(--accent);
  }
  .mi-kind {
    text-transform: uppercase;
    font-size: 0.6rem;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .mi-kind.overlay {
    color: var(--accent);
  }
  .mi-kind.oscillator {
    color: var(--amber);
  }
  /* Style editors */
  .style-head {
    font-size: 0.8rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    border-top: 1px solid var(--border);
    padding-top: var(--space-2);
  }
  .style-head small {
    text-transform: none;
    letter-spacing: 0;
    color: var(--muted);
    opacity: 0.7;
  }
  .style-row {
    display: flex;
    gap: var(--space-3);
    align-items: flex-end;
    flex-wrap: wrap;
  }
  .swatch,
  .width {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: 0.8rem;
    color: var(--muted);
  }
  .pick {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  input[type='color'] {
    width: 40px;
  }
  .width input {
    width: 70px;
  }
  .reset {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.85rem;
  }
  .reset:hover {
    color: var(--text);
  }
  button {
    border-radius: var(--radius);
    cursor: pointer;
  }
</style>
