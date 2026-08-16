<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Add/edit an indicator instance. In add mode the type is chosen from a searchable
  // dropdown (grouped by pane kind) and the param form rebuilds from the catalog; in edit
  // mode the type is fixed and only params/style are editable. Style overrides (line color,
  // fill color, line width) are optional — blank/zero means "use the indicator's default".
  // Saving emits the {type, params, style} draft to the parent.
  // A second tab holds custom indicators: the node-graph library shared with the backtest
  // module. Picking one there plots it here; building or editing one writes to that same
  // library, so both modules always see the same definition. Custom indicators also choose
  // their own pane (on the price, or a pane of their own) — the catalog's don't.
  import Modal from '$lib/ui/Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { t } from '$lib/i18n';
  import { CATALOG, catalogDef, defaultParams, DEFAULT_STYLE, PRICE_SOURCES } from './indicators.js';
  import { indicatorLib } from '$lib/indicators/api.js';
  import { defaultIndicatorDef } from '$lib/indicators/dag.js';
  import IndicatorBuilder from '$lib/indicators/IndicatorBuilder.svelte';

  let { open = $bindable(false), edit = null, onsave, onclose } = $props();

  // Local draft. Seeded from `edit` when editing, else first catalog entry.
  let mode = $state('catalog'); // 'catalog' | 'custom'
  let type = $state('sma');
  let params = $state({});
  let style = $state({ ...DEFAULT_STYLE });

  // ── Custom tab state ──
  let library = $state([]); // [{ id, name, definition }]
  let libError = $state('');
  let customId = $state(null); // library row backing the draft (null = unsaved local build)
  let customName = $state('');
  let customDef = $state(defaultIndicatorDef());
  let building = $state(false); // the builder is open
  let pane = $state('pane'); // 'overlay' | 'pane'
  let saving = $state(false);
  let saveNote = $state('');

  // Definitions travel between a $state proxy (the chart instance), the fetched library rows
  // and the builder's own state — structuredClone chokes on the proxy, and a shared reference
  // would let the builder mutate the live chart instance. Plain JSON both ways.
  const cloneDef = (d) => JSON.parse(JSON.stringify(d));

  async function loadLibrary() {
    try {
      library = await indicatorLib.list();
      libError = '';
    } catch (e) {
      libError = e.message;
    }
  }

  // `null` is the "not saved yet" entry, listed only while the draft has no library row.
  const libOptions = $derived([
    ...(customId ? [] : [{ value: null, label: $t('histviz.custom.none') }]),
    ...library.map((row) => ({ value: row.id, label: row.name }))
  ]);

  function pickCustom(id) {
    const row = library.find((i) => i.id === id);
    if (!row) return;
    customId = row.id;
    customName = row.name;
    customDef = cloneDef(row.definition);
    building = false;
    saveNote = '';
  }

  function newCustom() {
    customId = null;
    customName = '';
    customDef = defaultIndicatorDef();
    building = true;
    saveNote = '';
  }

  /** Write the draft back to the shared library (create or update), then re-select it. */
  async function saveToLibrary() {
    const name = customName.trim();
    if (!name) {
      saveNote = $t('histviz.custom.nameRequired');
      return;
    }
    saving = true;
    try {
      if (customId) await indicatorLib.update(customId, name, '', customDef);
      else customId = (await indicatorLib.create(name, '', customDef)).id;
      await loadLibrary();
      saveNote = $t('histviz.custom.saved');
    } catch (e) {
      saveNote = e.message;
    } finally {
      saving = false;
    }
  }

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
        type = edit.type === 'custom' ? CATALOG[0].type : edit.type;
        params = { ...(edit.params ?? {}) };
        style = { ...DEFAULT_STYLE, ...(edit.style ?? {}) };
      } else {
        type = CATALOG[0].type;
        params = defaultParams(CATALOG[0].type);
        style = { ...DEFAULT_STYLE };
      }
      // Editing a custom instance opens straight on its tab, seeded from the instance (the
      // definition travels with it, so it still opens after its library row is deleted).
      if (edit?.type === 'custom') {
        mode = 'custom';
        customId = edit.custom?.id ?? null;
        customName = edit.custom?.name ?? '';
        customDef = cloneDef(edit.custom?.def ?? defaultIndicatorDef());
        pane = edit.pane === 'overlay' ? 'overlay' : 'pane';
      } else {
        mode = 'catalog';
        customId = null;
        customName = '';
        customDef = defaultIndicatorDef();
        pane = 'pane';
      }
      building = false;
      saveNote = '';
      query = '';
      pickerOpen = false;
      loadLibrary();
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

  const cleanStyle = () => {
    const w = Number(style.width);
    return {
      color: style.color || '',
      // Fill only applies to band/channel indicators; drop it otherwise.
      fill: def?.fillable ? style.fill || '' : '',
      width: Number.isFinite(w) && w > 0 ? Math.min(8, w) : 0
    };
  };

  function saveCustom() {
    if (!customDef?.nodes?.length) {
      saveNote = $t('histviz.custom.needDef');
      return;
    }
    onsave?.({
      type: 'custom',
      pane,
      custom: { id: customId, name: customName.trim() || $t('histviz.custom.untitled'), def: customDef },
      params: {},
      style: cleanStyle()
    });
    open = false;
  }

  function save() {
    if (mode === 'custom') {
      saveCustom();
      return;
    }
    // Coerce params to numbers and clamp to bounds.
    const clean = {};
    for (const p of def.params) {
      let v = Number(params[p.key]);
      if (!Number.isFinite(v)) v = p.default;
      clean[p.key] = Math.min(p.max, Math.max(p.min, v));
    }
    // Preserve the (non-numeric) price source for sourceable indicators.
    if (def.sourceable) clean.source = params.source ?? 'close';
    onsave?.({ type, params: clean, style: cleanStyle() });
    open = false;
  }
</script>

<Modal
  bind:open
  size={mode === 'custom' ? 'lg' : 'md'}
  title={edit ? $t('histviz.modal.editTitle') : $t('histviz.modal.addTitle')}
  {onclose}
>
  <div class="form">
    <div class="tabs" role="tablist">
      <button
        role="tab"
        aria-selected={mode === 'catalog'}
        class:on={mode === 'catalog'}
        disabled={edit?.type === 'custom'}
        onclick={() => (mode = 'catalog')}
      >
        {$t('histviz.custom.tabCatalog')}
      </button>
      <button
        role="tab"
        aria-selected={mode === 'custom'}
        class:on={mode === 'custom'}
        disabled={!!edit && edit.type !== 'custom'}
        onclick={() => (mode = 'custom')}
      >
        {$t('histviz.custom.tabCustom')}
      </button>
    </div>

    {#if mode === 'custom'}
      <div class="field">
        <span>{$t('histviz.custom.pick')}</span>
        <div class="lib-row">
          <Dropdown
            value={customId}
            options={libOptions}
            ariaLabel={$t('histviz.custom.pick')}
            searchable
            searchPlaceholder={$t('histviz.custom.search')}
            onpick={pickCustom}
          />
          <button type="button" class="mini" onclick={() => (building = !building)} disabled={!customId && !building}>
            <Icon name="pencil" size={12} /> {$t('histviz.custom.edit')}
          </button>
          <button type="button" class="mini" onclick={newCustom}>
            <Icon name="plus" size={12} /> {$t('histviz.custom.new')}
          </button>
        </div>
        <small class="hint">{$t('histviz.custom.sharedHint')}</small>
        {#if libError}<p class="err">{libError}</p>{/if}
      </div>

      {#if building}
        <label class="field">
          <span>{$t('histviz.custom.name')}</span>
          <input bind:value={customName} placeholder={$t('histviz.custom.namePlaceholder')} />
        </label>
        <IndicatorBuilder bind:def={customDef} />
        <div class="lib-row">
          <button type="button" class="mini" onclick={saveToLibrary} disabled={saving}>
            <Icon name="save" size={12} /> {$t('histviz.custom.saveToLib')}
          </button>
          {#if saveNote}<span class="note">{saveNote}</span>{/if}
        </div>
      {:else if saveNote}
        <p class="err">{saveNote}</p>
      {/if}

      <div class="field">
        <span>{$t('histviz.custom.pane')}</span>
        <div class="seg">
          <button type="button" class:on={pane === 'overlay'} onclick={() => (pane = 'overlay')}>
            {$t('histviz.custom.paneOverlay')}
          </button>
          <button type="button" class:on={pane === 'pane'} onclick={() => (pane = 'pane')}>
            {$t('histviz.custom.paneSeparate')}
          </button>
        </div>
      </div>
    {:else}
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
      <div class="field">
        <span>{$t('histviz.modal.source')}</span>
        <Dropdown
          bind:value={params.source}
          ariaLabel={$t('histviz.modal.source')}
          options={PRICE_SOURCES.map((s) => ({ value: s.key, label: s.label }))}
        />
      </div>
    {/if}
    {/if}

    <div class="style-head">{$t('histviz.modal.style')} <small>{$t('histviz.modal.styleHint')}</small></div>
    <div class="style-row">
      <label class="swatch">
        <span>{$t('histviz.modal.line')}</span>
        <span class="pick">
          <input
            type="color"
            value={style.color || '#c9a45c'}
            oninput={(e) => (style.color = e.currentTarget.value)}
          />
          {#if style.color}<button type="button" class="reset" title={$t('histviz.modal.default')} onclick={() => (style.color = '')}><Icon name="refresh-cw" size={12} /></button>{/if}
        </span>
      </label>
      {#if mode === 'catalog' && def.fillable}
        <label class="swatch">
          <span>{$t('histviz.modal.fill')}</span>
          <span class="pick">
            <input
              type="color"
              value={style.fill || '#c9a45c'}
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
  /* Catalog / Custom switch */
  .tabs {
    display: flex;
    gap: var(--space-1);
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-2);
  }
  .tabs button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .tabs button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .lib-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .lib-row :global(.dd) {
    flex: 1;
    min-width: 160px;
  }
  .mini {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-2);
  }
  .mini:hover:not(:disabled) {
    border-color: var(--border-control);
  }
  .mini:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .seg {
    display: flex;
  }
  .seg button {
    background: var(--surface-2);
    border: 1px solid var(--border);
    color: var(--muted);
    font-size: var(--text-sm);
    padding: var(--space-1) var(--space-3);
  }
  .seg button + button {
    border-left: none;
  }
  .seg button.on {
    color: var(--text);
    border-color: var(--border-control);
    background: var(--surface);
  }
  .note {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .err {
    font-size: var(--text-xs);
    color: var(--red);
    margin: 0;
  }
  input[type='number'] {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
  }
  .fixed {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
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
    font-size: var(--text-base);
    cursor: pointer;
  }
  .caret {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .menu {
    position: absolute;
    z-index: var(--z-dropdown);
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .menu-search {
    width: 100%;
    background: var(--surface-2);
    border: none;
    border-bottom: 1px solid var(--border);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
  }
  .menu-list {
    max-height: 240px;
    overflow-y: auto;
  }
  .menu-empty {
    padding: var(--space-2) var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
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
    font-size: var(--text-base);
    cursor: pointer;
    text-align: left;
  }
  .menu-item:hover {
    background: var(--surface-2);
  }
  .menu-item.sel {
    color: var(--text);
  }
  .mi-kind {
    text-transform: uppercase;
    font-size: var(--text-xs);
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .mi-kind.overlay {
    color: var(--muted);
  }
  .mi-kind.oscillator {
    color: var(--amber);
  }
  /* Style editors */
  .style-head {
    font-size: var(--text-sm);
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
    font-size: var(--text-sm);
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
    font-size: var(--text-base);
  }
  .reset:hover {
    color: var(--text);
  }
  button {
    border-radius: var(--radius);
    cursor: pointer;
  }
</style>
