<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Add/edit an indicator instance. In add mode the type is chosen from a searchable
  // dropdown (grouped by pane kind) and the param form rebuilds from the catalog; in edit
  // mode the type is fixed and only params/style are editable. Style overrides (line color,
  // fill color, line width) are optional — blank/zero means "use the indicator's default".
  // Saving emits the {type, params, style} draft to the parent.
  // A third tab holds Pine Script v5/v6 compatible scripts (`$lib/script`), same library.
  // A second tab holds custom indicators: the node-graph library shared with the backtest
  // module. Picking one there plots it here; building or editing one writes to that same
  // library, so both modules always see the same definition. Custom indicators also choose
  // their own pane (on the price, or a pane of their own) — the catalog's don't.
  import Modal from '$lib/ui/Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { t } from '$lib/i18n';
  import {
    CATALOG,
    catalogDef,
    defaultParams,
    DEFAULT_STYLE,
    PRICE_SOURCES,
    IND,
    EXPR,
    isDerivedSource
  } from './indicators.js';
  import { FORMULA_VARS, FORMULA_FUNCS, formulaError } from './formula.js';
  import { LINE_STYLES } from './settings.js';
  import { indicatorLib } from '$lib/indicators/api.js';
  import { defaultIndicatorDef } from '$lib/indicators/dag.js';
  import IndicatorBuilder from '$lib/indicators/IndicatorBuilder.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ScriptTab from '$lib/script/ScriptTab.svelte';
  import { inputValues } from '$lib/script/host.js';
  import { compileScript } from '$lib/script/client.js';

  // `sources`: the other studies on the chart, [{ id, label, pane, outputs: [name] }], so one
  // can read another's output. `formulas`: the user's favourite source formulas,
  // [{ name, expr }], saved through `onformulas`.
  let {
    open = $bindable(false),
    edit = null,
    sources = [],
    formulas = [],
    onformulas,
    onsave,
    onclose
  } = $props();

  // Inputs / Style, for both the catalog and the custom tab.
  let section = $state('inputs');

  // ── Source ──
  // The picker's value: a price key, an `ind:` key, an `expr:` favourite, or FORMULA (the
  // free formula line below it).
  const FORMULA = '__formula';
  let sourceSel = $state('close');
  let formulaText = $state('');
  let formulaPane = $state('overlay'); // where a formula-fed study draws: 'overlay' | 'pane'
  let favName = $state('');
  let formulaInput = $state(null);

  const formulaErr = $derived(sourceSel === FORMULA ? formulaError(formulaText) : null);
  const activeExpr = $derived(
    sourceSel === FORMULA ? formulaText.trim() : sourceSel?.startsWith(EXPR) ? sourceSel.slice(EXPR.length) : ''
  );
  const isFav = $derived(!!activeExpr && formulas.some((f) => f.expr === activeExpr));

  const sourceOpts = $derived([
    { value: 'h:price', label: $t('histviz.modal.srcPrice'), header: true },
    ...PRICE_SOURCES.map((s) => ({ value: s.key, label: s.label })),
    ...(sources.length
      ? [
          { value: 'h:ind', label: $t('histviz.modal.srcStudies'), header: true },
          ...sources.flatMap((src) =>
            src.outputs.map((name, k) => ({
              value: `${IND}${src.id}:${k}`,
              label: src.outputs.length > 1 ? `${src.label} · ${name}` : src.label
            }))
          )
        ]
      : []),
    { value: 'h:formula', label: $t('histviz.modal.srcFormulas'), header: true },
    ...formulas.map((f) => ({ value: `${EXPR}${f.expr}`, label: `★ ${f.name}` })),
    { value: FORMULA, label: $t('histviz.modal.srcFormula') }
  ]);

  /** Insert a token at the caret of the formula line. */
  function insert(text) {
    const el = formulaInput;
    const at = el?.selectionStart ?? formulaText.length;
    const end = el?.selectionEnd ?? at;
    formulaText = formulaText.slice(0, at) + text + formulaText.slice(end);
    queueMicrotask(() => {
      el?.focus();
      const caret = at + (text.includes('(') ? text.indexOf('(') + 1 : text.length);
      el?.setSelectionRange(caret, caret);
    });
  }

  function addFavorite() {
    const expr = activeExpr;
    if (!expr || formulaError(expr)) return;
    const name = favName.trim() || expr.slice(0, 24);
    onformulas?.([...formulas.filter((f) => f.expr !== expr), { name, expr }]);
    favName = '';
  }
  const removeFavorite = (expr) => onformulas?.(formulas.filter((f) => f.expr !== expr));

  /** What the instance stores for its source: key, display name and pane. */
  function resolvedSource() {
    if (sourceSel === FORMULA || sourceSel?.startsWith(EXPR)) {
      const expr = activeExpr;
      const fav = formulas.find((f) => f.expr === expr);
      return {
        source: `${EXPR}${expr}`,
        sourceName: fav ? fav.name : `ƒ ${expr.length > 18 ? `${expr.slice(0, 17)}…` : expr}`,
        pane: formulaPane
      };
    }
    if (sourceSel?.startsWith(IND)) {
      const [id, k] = sourceSel.slice(IND.length).split(':');
      const src = sources.find((x) => String(x.id) === id);
      const name = src?.outputs.length > 1 ? src.outputs[Number(k)] : src?.label;
      return {
        source: sourceSel,
        sourceName: name ?? '?',
        // Drawn with the study it reads: an SMA of an RSI belongs on the RSI's scale.
        pane: !src || src.pane === 'overlay' ? 'overlay' : `src:${src.pane}`
      };
    }
    return { source: sourceSel || 'close', sourceName: undefined, pane: undefined };
  }

  // Local draft. Seeded from `edit` when editing, else first catalog entry. The params start
  // complete rather than empty: the form binds a picker to `params.source`, and a bound picker
  // cannot be handed `undefined`: the first frame is painted before the seeding effect runs.
  let mode = $state('catalog'); // 'catalog' | 'custom' | 'script'
  let type = $state(CATALOG[0].type);
  let params = $state(defaultParams(CATALOG[0].type));
  let style = $state({ ...DEFAULT_STYLE });

  // ── Script tab state (the instance's `script` plus its pane) ──
  const blankScript = () => ({ id: null, name: '', source: '', inputs: {}, hidden: [], pane: 'pane', paneSet: false });
  let scriptDraft = $state(blankScript());
  let scriptCompiled = $state(null);
  let scriptEditing = $state(false); // false = the saved-script list, nothing to add yet

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
  let noteErr = $state(false); // the note is a failure, not a confirmation
  let confirmDelete = $state(false); // app modal, never native confirm()

  // Definitions travel between a $state proxy (the chart instance), the fetched library rows
  // and the builder's own state — structuredClone chokes on the proxy, and a shared reference
  // would let the builder mutate the live chart instance. Plain JSON both ways.
  const cloneDef = (d) => JSON.parse(JSON.stringify(d));

  async function loadLibrary() {
    try {
      library = await indicatorLib.list('dag');
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
    noteErr = false;
  }

  /** Write the draft back to the shared library (create or update), then re-select it. */
  async function saveToLibrary() {
    const name = customName.trim();
    if (!name) {
      saveNote = $t('histviz.custom.nameRequired');
      noteErr = true;
      return;
    }
    saving = true;
    try {
      if (customId) await indicatorLib.update(customId, name, '', { definition: customDef });
      else customId = (await indicatorLib.create(name, '', { definition: customDef })).id;
      await loadLibrary();
      saveNote = $t('histviz.custom.saved');
      noteErr = false;
    } catch (e) {
      saveNote = e.message;
      noteErr = true;
    } finally {
      saving = false;
    }
  }

  /** Drop the selected row from the shared library. The draft keeps its name and definition,
   *  so a deletion loses the saved copy, not the work in front of you. */
  async function deleteFromLibrary() {
    if (!customId) return;
    saving = true;
    try {
      await indicatorLib.remove(customId);
      customId = null;
      await loadLibrary();
      saveNote = $t('histviz.custom.deleted');
      noteErr = false;
    } catch (e) {
      saveNote = e.message;
      noteErr = true;
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
        const own = edit.type === 'custom' || edit.type === 'script'; // not a catalog type
        const t = own ? CATALOG[0].type : edit.type;
        type = t;
        // Over the type's own defaults: an instance only carries the params it was built with
        // (one derived from a strategy has no price source, for instance).
        params = { ...defaultParams(t), ...(own ? {} : (edit.params ?? {})) };
        style = { ...DEFAULT_STYLE, ...(edit.style ?? {}) };
        seedSource(params.source, edit.pane);
      } else {
        type = CATALOG[0].type;
        params = defaultParams(CATALOG[0].type);
        style = { ...DEFAULT_STYLE };
        seedSource('close', null);
      }
      section = 'inputs';
      favName = '';
      // Editing a custom instance opens straight on its tab, seeded from the instance (the
      // definition travels with it, so it still opens after its library row is deleted).
      scriptCompiled = null;
      scriptDraft =
        edit?.type === 'script'
          ? { ...blankScript(), ...cloneDef(edit.script ?? {}), pane: edit.pane === 'overlay' ? 'overlay' : 'pane', paneSet: true }
          : blankScript();
      scriptEditing = edit?.type === 'script';
      if (edit?.type === 'script') {
        mode = 'script';
      } else if (edit?.type === 'custom') {
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
      noteErr = false;
      confirmDelete = false;
      query = '';
      pickerOpen = false;
      loadLibrary();
    }
    lastOpen = open;
    lastEdit = edit;
  });

  const def = $derived(catalogDef(type));

  function seedSource(key, pane) {
    const k = key ?? 'close';
    const fav = k.startsWith(EXPR) && formulas.some((f) => `${EXPR}${f.expr}` === k);
    if (k.startsWith(EXPR) && !fav) {
      sourceSel = FORMULA;
      formulaText = k.slice(EXPR.length);
    } else {
      sourceSel = k;
      formulaText = k.startsWith(EXPR) ? k.slice(EXPR.length) : '';
    }
    formulaPane = pane === 'pane' ? 'pane' : 'overlay';
  }

  function pickSource(v) {
    // A favourite opens in the formula line too, so it can be read and adjusted.
    if (v?.startsWith(EXPR)) formulaText = v.slice(EXPR.length);
    if (v === FORMULA && !formulaText) formulaText = 'close';
    sourceSel = v;
  }

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
      width: Number.isFinite(w) && w > 0 ? Math.min(8, w) : 0,
      lineType: LINE_STYLES.includes(style.lineType) ? style.lineType : ''
    };
  };

  function saveCustom() {
    if (!customDef?.nodes?.length) {
      saveNote = $t('histviz.custom.needDef');
      noteErr = true;
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

  async function saveScript() {
    const d = scriptDraft;
    if (!d.source?.trim()) {
      saveNote = $t('histviz.script.empty');
      noteErr = true;
      return;
    }
    // Saved before the editor's first lint (it waits 300 ms and the engine loads): compile now.
    if (!scriptCompiled) scriptCompiled = await compileScript(d.source).catch(() => null);
    if (!scriptCompiled?.ok) {
      saveNote = $t('histviz.script.needCompile');
      noteErr = true;
      return;
    }
    // A refused input is shown next to its field: fix it there rather than add a broken run.
    if (inputValues(scriptCompiled.meta.inputs, d.inputs).errors.length) {
      section = 'inputs';
      return;
    }
    const name = d.name.trim() || scriptCompiled.meta.shorttitle || scriptCompiled.meta.title || $t('histviz.script.untitled');
    onsave?.({
      type: 'script',
      pane: d.pane === 'overlay' ? 'overlay' : 'pane',
      script: { id: d.id, name, source: d.source, inputs: { ...d.inputs }, hidden: [...(d.hidden ?? [])] },
      params: {},
      style: {}
    });
    open = false;
  }

  function save() {
    if (mode === 'script') {
      saveScript();
      return;
    }
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
    // The (non-numeric) source of sourceable indicators: a price, a study, a formula.
    let pane;
    if (def.sourceable) {
      if (sourceSel === FORMULA && formulaErr) {
        section = 'inputs';
        return;
      }
      const src = resolvedSource();
      clean.source = src.source;
      if (src.sourceName) clean.sourceName = src.sourceName;
      pane = isDerivedSource(src.source) ? src.pane : undefined;
    }
    onsave?.({ type, params: clean, style: cleanStyle(), pane });
    open = false;
  }
</script>

<Modal
  bind:open
  size={mode === 'catalog' ? 'md' : 'lg'}
  title={edit ? $t('histviz.modal.editTitle') : $t('histviz.modal.addTitle')}
  {onclose}
>
  <div class="form">
    <!-- Catalog / Custom only matters while choosing what to add: an indicator being edited
         is already one or the other, so its sheet opens straight on Inputs / Style. -->
    {#if !edit}
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
      <button role="tab" aria-selected={mode === 'script'} class:on={mode === 'script'} onclick={() => ((mode = 'script'), (saveNote = ''))}>
        {$t('histviz.script.tab')}
      </button>
    </div>
    {/if}

    {#if mode === 'catalog'}
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

    {/if}

    {#if mode !== 'script' || scriptEditing}
    <div class="tabs" class:sub={!edit} role="tablist">
      <button role="tab" aria-selected={section === 'inputs'} class:on={section === 'inputs'} onclick={() => (section = 'inputs')}>
        {$t('histviz.modal.tabInputs')}
      </button>
      <button role="tab" aria-selected={section === 'style'} class:on={section === 'style'} onclick={() => (section = 'style')}>
        {$t('histviz.modal.tabStyle')}
      </button>
    </div>

    {/if}

    {#if mode === 'script'}
      <ScriptTab bind:draft={scriptDraft} bind:compiled={scriptCompiled} bind:editing={scriptEditing} {section} />
      {#if saveNote}<p class:err={noteErr} class:note={!noteErr}>{saveNote}</p>{/if}
    {:else if section === 'inputs'}
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
          <button
            type="button"
            class="act danger"
            title={$t('histviz.custom.delete')}
            aria-label={$t('histviz.custom.delete')}
            disabled={!customId || saving}
            onclick={() => (confirmDelete = true)}
          >
            <Icon name="x" size={14} />
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
        <p class:err={noteErr} class:note={!noteErr}>{saveNote}</p>
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
              value={sourceSel}
              ariaLabel={$t('histviz.modal.source')}
              options={sourceOpts}
              searchable
              onpick={pickSource}
            />
          </div>

          {#if sourceSel === FORMULA || sourceSel?.startsWith(EXPR)}
            <div class="formula">
              <input
                bind:this={formulaInput}
                class="fx"
                class:bad={!!formulaErr}
                spellcheck="false"
                autocomplete="off"
                placeholder="(high + low + 2 * close) / 4"
                value={formulaText}
                oninput={(e) => {
                  formulaText = e.currentTarget.value;
                  sourceSel = FORMULA;
                }}
              />
              <p class="fx-status" class:err={!!formulaErr}>
                {formulaErr ?? $t('histviz.modal.formulaOk')}
              </p>
              <div class="chips">
                {#each FORMULA_VARS as v (v)}
                  <button type="button" class="chip var" onclick={() => insert(v)}>{v}</button>
                {/each}
              </div>
              <div class="chips">
                {#each Object.entries(FORMULA_FUNCS) as [name, fn] (name)}
                  <button type="button" class="chip fn" title={fn.hint} onclick={() => insert(`${name}(`)}>{name}</button>
                {/each}
              </div>
              <div class="fav-row">
                {#if isFav}
                  <span class="note">★ {$t('histviz.modal.formulaIsFav')}</span>
                {:else}
                  <input class="fav-name" bind:value={favName} placeholder={$t('histviz.modal.formulaName')} />
                  <button type="button" class="mini" disabled={!!formulaErr || !activeExpr} onclick={addFavorite}>
                    <Icon name="star" size={12} /> {$t('histviz.modal.formulaFav')}
                  </button>
                {/if}
              </div>
              {#if formulas.length}
                <div class="chips">
                  {#each formulas as fav (fav.expr)}
                    <span class="chip favchip" title={fav.expr}>
                      <button type="button" class="favpick" onclick={() => pickSource(`${EXPR}${fav.expr}`)}>★ {fav.name}</button>
                      <button
                        type="button"
                        class="favrm"
                        title={$t('histviz.modal.formulaUnfav')}
                        aria-label={$t('histviz.modal.formulaUnfav')}
                        onclick={() => removeFavorite(fav.expr)}><Icon name="x" size={10} /></button
                      >
                    </span>
                  {/each}
                </div>
              {/if}
              <div class="field">
                <span>{$t('histviz.custom.pane')}</span>
                <div class="seg">
                  <button type="button" class:on={formulaPane === 'overlay'} onclick={() => (formulaPane = 'overlay')}>
                    {$t('histviz.custom.paneOverlay')}
                  </button>
                  <button type="button" class:on={formulaPane === 'pane'} onclick={() => (formulaPane = 'pane')}>
                    {$t('histviz.custom.paneSeparate')}
                  </button>
                </div>
              </div>
            </div>
          {:else if sourceSel?.startsWith(IND)}
            <small class="hint">{$t('histviz.modal.srcStudyHint')}</small>
          {/if}
        {/if}
      {/if}
    {:else}
      <div class="style-grid">
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
        <div class="width">
          <span>{$t('histviz.modal.lineType')}</span>
          <div class="seg">
            <button type="button" class:on={!style.lineType} onclick={() => (style.lineType = '')}>{$t('histviz.modal.default')}</button>
            {#each LINE_STYLES as ls (ls)}
              <button type="button" class:on={style.lineType === ls} onclick={() => (style.lineType = ls)}>
                {$t(`histviz.settings.line${ls[0].toUpperCase()}${ls.slice(1)}`)}
              </button>
            {/each}
          </div>
        </div>
      </div>
      <small class="hint">{$t('histviz.modal.styleHint')}</small>
    {/if}
  </div>

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={save} disabled={mode === 'script' && !scriptEditing}>{edit ? $t('common.save') : $t('histviz.modal.add')}</button>
  {/snippet}
</Modal>

<ConfirmModal
  bind:open={confirmDelete}
  title={$t('histviz.custom.deleteTitle')}
  message={$t('histviz.custom.deleteMessage', { name: customName || $t('histviz.custom.untitled') })}
  confirmLabel={$t('common.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={deleteFromLibrary}
/>

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
  .tabs.sub {
    border-bottom: none;
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 2px;
    align-self: flex-start;
  }
  .tabs.sub button {
    border-bottom: none;
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
  }
  .tabs.sub button.on {
    background: var(--surface);
  }
  .hint {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .formula {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .fx {
    font-family: var(--mono);
    font-size: var(--text-base);
    width: 100%;
  }
  .fx.bad {
    border-color: var(--red);
  }
  .fx-status {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--green);
  }
  .fx-status.err {
    color: var(--red);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    font-family: var(--mono);
    font-size: var(--text-xs);
    padding: 1px 6px;
  }
  .chip.fn {
    color: var(--accent);
  }
  .chip:hover {
    border-color: var(--border-control);
  }
  .favchip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 0 2px 0 6px;
  }
  .favpick,
  .favrm {
    background: transparent;
    border: none;
    color: inherit;
    font: inherit;
    padding: 1px 0;
    display: inline-flex;
    align-items: center;
  }
  .favrm {
    color: var(--muted);
  }
  .favrm:hover {
    color: var(--red);
  }
  .fav-row {
    display: flex;
    align-items: stretch;
    gap: var(--space-2);
  }
  /* The button takes the input's height, whatever the theme makes that. */
  .fav-row .mini {
    flex: none;
  }
  .fav-name {
    flex: 1;
    min-width: 0;
  }
  /* Labels on one line, controls on the next, every control the same height. */
  .style-grid {
    display: flex;
    gap: var(--space-4);
    align-items: flex-start;
    flex-wrap: wrap;
    --style-h: 32px;
  }
  .style-grid .pick,
  .style-grid .seg {
    height: var(--style-h);
  }
  .style-grid input[type='color'] {
    width: 44px;
    height: var(--style-h);
    padding: 2px;
    box-sizing: border-box;
  }
  .style-grid input[type='number'] {
    height: var(--style-h);
    box-sizing: border-box;
    padding-block: 0;
  }
  .style-grid .seg button {
    height: var(--style-h);
    box-sizing: border-box;
    padding-block: 0;
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
  .act {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: var(--space-1);
    display: inline-flex;
  }
  .act:hover:not(:disabled) {
    color: var(--red);
  }
  .act:disabled {
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
    margin: 0;
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
