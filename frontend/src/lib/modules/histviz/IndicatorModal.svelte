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
    isDerivedSource,
    instanceLabel
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
    const op = Number(style.fillOpacity);
    return {
      color: style.color || '',
      // Fill only applies to band/channel indicators; drop it otherwise.
      fill: def?.fillable ? style.fill || '' : '',
      fillOpacity: Number.isFinite(op) ? Math.min(100, Math.max(0, Math.round(op))) : DEFAULT_STYLE.fillOpacity,
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

  // ── Sheet chrome: header identity, steppers, width list, reset, keyboard add ──

  const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.userAgent);

  /** A short badge for a catalog entry: its acronym ("RSI", "BB", "W%R"), else three letters. */
  function badgeOf(label) {
    const head = label.split(/ \u2014 | \(/)[0];
    const caps = head.match(/[A-Z%]/g)?.join('') ?? '';
    return caps.length >= 2 ? caps.slice(0, 4) : head.slice(0, 3).toUpperCase();
  }

  const placementLabel = (p) => (p === 'overlay' ? $t('histviz.custom.paneOverlay') : $t('histviz.custom.paneSeparate'));

  const headTitle = $derived(
    mode === 'custom'
      ? customName.trim() || $t('histviz.custom.untitled')
      : mode === 'script'
        ? scriptDraft.name?.trim() || scriptCompiled?.meta?.shorttitle || scriptCompiled?.meta?.title || $t('histviz.script.untitled')
        : def.label
  );
  const headSub = $derived.by(() => {
    if (mode === 'custom') return placementLabel(pane);
    if (mode === 'script') return placementLabel(scriptDraft.pane);
    const src = def.sourceable ? resolvedSource() : null;
    const label = instanceLabel(type, src ? { ...params, source: src.source, sourceName: src.sourceName } : params);
    const where = src && isDerivedSource(src.source) ? src.pane : def.kind;
    return `${label} \u00b7 ${placementLabel(where === 'overlay' ? 'overlay' : 'pane')}`;
  });

  // One-line descriptions under a catalog param, by key; a key without one shows none.
  const PARAM_HINTS = ['period', 'fast', 'slow', 'signal', 'k', 'd', 'mult', 'step', 'max'];
  const paramHint = (key) => (PARAM_HINTS.includes(key) ? $t(`histviz.modal.paramHint.${key}`) : '');

  function stepParam(p, dir) {
    const v = Number(params[p.key]);
    const next = Math.min(p.max, Math.max(p.min, (Number.isFinite(v) ? v : p.default) + dir * p.step));
    const dec = (String(p.step).split('.')[1] ?? '').length;
    params[p.key] = Number(next.toFixed(dec));
  }

  // Line widths offered by the picker; a width saved off this list still shows as itself.
  const WIDTHS = [0.5, 1, 1.5, 2, 2.5, 3, 4, 5, 6, 8];
  const widthOpts = $derived([
    { value: 0, label: $t('histviz.modal.autoLabel') },
    ...[...new Set([...WIDTHS, Number(style.width) || 0])]
      .filter((w) => w > 0)
      .sort((a, b) => a - b)
      .map((w) => ({ value: w, label: `${w} px` }))
  ]);

  // Resets what the open section shows: the params and source, or the style overrides.
  const canReset = $derived(mode === 'catalog' || (mode === 'custom' && section === 'style'));
  function resetDefaults() {
    if (section === 'style') {
      style = { ...DEFAULT_STYLE };
      return;
    }
    params = defaultParams(type);
    seedSource('close', null);
  }

  // Cmd/Ctrl+Enter adds (or saves). An editor that used the keystroke itself has already
  // prevented its default, so a newline in the script editor never submits the sheet.
  function onKey(e) {
    if (!open || confirmDelete || e.defaultPrevented) return;
    if (e.key !== 'Enter' || !(e.metaKey || e.ctrlKey)) return;
    if (mode === 'script' && !scriptEditing) return;
    e.preventDefault();
    save();
  }
</script>

<svelte:window onkeydown={onKey} />

{#snippet head()}
  {#if edit}
    <div class="badge" class:mono={mode === 'catalog'}>
      {#if mode === 'catalog'}{badgeOf(def.label)}
      {:else if mode === 'custom'}<Icon name="layers" size={16} />
      {:else}<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 6l-6 6 6 6M16 6l6 6-6 6"></path></svg>{/if}
    </div>
    <div class="ht">
      <div class="ht-row">
        <h3>{headTitle}</h3>
        <span class="tag">
          {mode === 'catalog' ? $t('histviz.modal.tagBuiltin') : mode === 'custom' ? $t('histviz.custom.tag') : $t('histviz.modal.tagScript')}
        </span>
      </div>
      <div class="sub" class:mono={mode === 'catalog'}>{headSub}</div>
    </div>
  {:else}
    <div class="ht">
      <h3>{$t('histviz.modal.addTitle')}</h3>
      <div class="sub">{$t('histviz.modal.addHint')}</div>
    </div>
  {/if}
{/snippet}

{#snippet placement(value, set)}
  <div class="line-row">
    <div class="rl">
      <span class="lbl">{$t('histviz.modal.placement')}</span>
      <span class="hint">{$t('histviz.modal.placementHint')}</span>
    </div>
    <div class="seg" role="radiogroup" aria-label={$t('histviz.modal.placement')}>
      <button type="button" role="radio" aria-checked={value === 'overlay'} class:on={value === 'overlay'} onclick={() => set('overlay')}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"></rect><path d="M6 15l4-4 3 2 5-5"></path></svg>
        {$t('histviz.custom.paneOverlay')}
      </button>
      <button type="button" role="radio" aria-checked={value !== 'overlay'} class:on={value !== 'overlay'} onclick={() => set('pane')}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"></rect><path d="M3 13h18"></path></svg>
        {$t('histviz.custom.paneSeparate')}
      </button>
    </div>
  </div>
{/snippet}

{#snippet colorField(key, label)}
  <div class="cfield">
    <label class="cpick">
      <span class="sw" class:auto={!style[key]} style:background={style[key] || null}></span>
      <span class="mono" class:faint={!style[key]}>{style[key] ? style[key].toUpperCase() : $t('histviz.modal.autoLabel')}</span>
      <input type="color" aria-label={label} value={style[key] || '#c9a45c'} oninput={(e) => (style[key] = e.currentTarget.value)} />
    </label>
    {#if style[key]}
      <button type="button" class="cclear" title={$t('histviz.modal.default')} aria-label={$t('histviz.modal.default')} onclick={() => (style[key] = '')}>
        <Icon name="x" size={11} />
      </button>
    {/if}
  </div>
{/snippet}

<Modal
  bind:open
  size="lg"
  title={edit ? $t('histviz.modal.editTitle') : $t('histviz.modal.addTitle')}
  {head}
  {onclose}
>
  <div class="form">
    <!-- Catalog / Custom / Pine only matters while choosing what to add: an indicator being
         edited is already one of them, so its sheet opens straight on Inputs / Style. -->
    {#if !edit}
      <div class="seg src" role="tablist" aria-label={$t('histviz.modal.indicator')}>
        <button type="button" role="tab" aria-selected={mode === 'catalog'} class:on={mode === 'catalog'} onclick={() => (mode = 'catalog')}>
          {$t('histviz.custom.tabCatalog')}
        </button>
        <button type="button" role="tab" aria-selected={mode === 'custom'} class:on={mode === 'custom'} onclick={() => (mode = 'custom')}>
          {$t('histviz.custom.tabCustom')}
        </button>
        <button type="button" role="tab" aria-selected={mode === 'script'} class:on={mode === 'script'} onclick={() => ((mode = 'script'), (saveNote = ''))}>
          {$t('histviz.script.tabShort')} <span class="tag">v5 · v6</span>
        </button>
      </div>
    {/if}

    {#if mode === 'catalog' && !edit}
      <div class="block">
        <span class="lbl">{$t('histviz.modal.indicator')}</span>
        <div class="combo">
          <button type="button" class="field-btn" aria-haspopup="listbox" aria-expanded={pickerOpen} onclick={() => (pickerOpen = !pickerOpen)}>
            <span class="fb-label">{def.label}</span>
            <span class="kind">{def.kind === 'overlay' ? $t('histviz.modal.kindOverlay') : $t('histviz.modal.kindPane')}</span>
            <Icon name="chevron-down" size={14} />
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
                  <button type="button" class="menu-item" class:sel={c.type === type} onclick={() => pick(c.type)}>
                    <span class="mi-label">{c.label}</span>
                    <span class="kind">{c.kind === 'overlay' ? $t('histviz.modal.kindOverlay') : $t('histviz.modal.kindPane')}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      </div>
    {/if}

    {#if mode === 'custom'}
      <div class="block">
        <span class="lbl">{$t('histviz.custom.pick')}</span>
        <div class="lib-row">
          <Dropdown
            value={customId}
            options={libOptions}
            ariaLabel={$t('histviz.custom.pick')}
            searchable
            searchPlaceholder={$t('histviz.custom.search')}
            onpick={pickCustom}
          />
          <button
            type="button"
            class="ib bd"
            class:on={building}
            title={$t('histviz.custom.edit')}
            aria-label={$t('histviz.custom.edit')}
            aria-pressed={building}
            disabled={!customId && !building}
            onclick={() => (building = !building)}
          >
            <Icon name="pencil" size={14} />
          </button>
          <button type="button" class="ib bd" title={$t('histviz.custom.new')} aria-label={$t('histviz.custom.new')} onclick={newCustom}>
            <Icon name="plus" size={14} />
          </button>
          <button
            type="button"
            class="ib bd danger"
            title={$t('histviz.custom.delete')}
            aria-label={$t('histviz.custom.delete')}
            disabled={!customId || saving}
            onclick={() => (confirmDelete = true)}
          >
            <Icon name="trash-2" size={14} />
          </button>
        </div>
        <span class="hint with-icon"><Icon name="link" size={12} /> {$t('histviz.custom.sharedHint')}</span>
        {#if libError}<p class="err">{libError}</p>{/if}
      </div>
    {/if}

    {#if mode !== 'script' || scriptEditing}
      <div class="tabs" class:flush={!!edit} role="tablist" aria-label={$t('histviz.modal.style')}>
        <button type="button" role="tab" aria-selected={section === 'inputs'} class:on={section === 'inputs'} onclick={() => (section = 'inputs')}>
          {$t('histviz.modal.tabInputs')}
        </button>
        <button type="button" role="tab" aria-selected={section === 'style'} class:on={section === 'style'} onclick={() => (section = 'style')}>
          {$t('histviz.modal.tabStyle')}
        </button>
      </div>
    {/if}

    {#if mode === 'script'}
      <ScriptTab bind:draft={scriptDraft} bind:compiled={scriptCompiled} bind:editing={scriptEditing} {section} />
      {#if saveNote}<p class:err={noteErr} class:note={!noteErr}>{saveNote}</p>{/if}
    {:else if section === 'inputs'}
      {#if mode === 'custom'}
        {#if building}
          <label class="block">
            <span class="lbl">{$t('histviz.custom.name')}</span>
            <input bind:value={customName} placeholder={$t('histviz.custom.namePlaceholder')} />
          </label>
          <IndicatorBuilder bind:def={customDef} />
          <div class="lib-row">
            <button type="button" class="btn" onclick={saveToLibrary} disabled={saving}>
              <Icon name="save" size={13} /> {$t('histviz.custom.saveToLib')}
            </button>
            {#if saveNote}<span class:err={noteErr} class:note={!noteErr}>{saveNote}</span>{/if}
          </div>
        {:else}
          <div class="empty">
            <div class="empty-icon"><Icon name="layers" size={16} /></div>
            <div class="empty-title">{customId || edit ? headTitle : $t('histviz.custom.emptyTitle')}</div>
            <div class="hint">{customId || edit ? $t('histviz.custom.builtHint') : $t('histviz.custom.emptyHint')}</div>
            <button type="button" class="btn" onclick={() => (building = true)}>{$t('histviz.custom.openBuilder')}</button>
          </div>
          {#if saveNote}<p class:err={noteErr} class:note={!noteErr}>{saveNote}</p>{/if}
        {/if}
      {:else}
        <div class="rows">
          {#each def.params as p (p.key)}
            <div class="row">
              <div class="rl">
                <label class="lbl" for="ind-p-{p.key}">{p.label}</label>
                {#if paramHint(p.key)}<span class="hint">{paramHint(p.key)}</span>{/if}
              </div>
              <div class="step">
                <input id="ind-p-{p.key}" type="number" inputmode="decimal" min={p.min} max={p.max} step={p.step} bind:value={params[p.key]} />
                <button type="button" aria-label={$t('histviz.modal.decrease', { name: p.label })} onclick={() => stepParam(p, -1)}>
                  <Icon name="minus" size={12} />
                </button>
                <button type="button" aria-label={$t('histviz.modal.increase', { name: p.label })} onclick={() => stepParam(p, 1)}>
                  <Icon name="plus" size={12} />
                </button>
              </div>
            </div>
          {/each}

          {#if def.sourceable}
            <div class="row">
              <div class="rl">
                <span class="lbl">{$t('histviz.modal.source')}</span>
                <span class="hint">{$t('histviz.modal.sourceHint')}</span>
              </div>
              <Dropdown value={sourceSel} ariaLabel={$t('histviz.modal.source')} options={sourceOpts} searchable onpick={pickSource} />
            </div>
          {/if}
        </div>

        {#if def.sourceable && (sourceSel === FORMULA || sourceSel?.startsWith(EXPR))}
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
                <button type="button" class="btn" disabled={!!formulaErr || !activeExpr} onclick={addFavorite}>
                  <Icon name="star" size={13} /> {$t('histviz.modal.formulaFav')}
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
          </div>
        {:else if def.sourceable && sourceSel?.startsWith(IND)}
          <small class="hint">{$t('histviz.modal.srcStudyHint')}</small>
        {/if}
      {/if}
    {:else}
      <div class="style">
        {#if mode === 'custom'}
          {@render placement(pane, (v) => (pane = v))}
        {:else if def.sourceable && (sourceSel === FORMULA || sourceSel?.startsWith(EXPR))}
          {@render placement(formulaPane, (v) => (formulaPane = v))}
        {/if}

        <div class="block">
          <div class="appear-head">
            <span class="lbl">{$t('histviz.modal.appearance')}</span>
            <span class="hint">{$t('histviz.modal.appearanceHint')}</span>
          </div>
          <div class="tbl">
            <div class="tr th">
              <span>{$t('histviz.modal.colElement')}</span>
              <span>{$t('histviz.modal.colColor')}</span>
              <span>{$t('histviz.modal.width')}</span>
              <span>{$t('histviz.modal.tabStyle')}</span>
            </div>
            <div class="tr">
              <span class="el">{$t('histviz.modal.line')}</span>
              {@render colorField('color', $t('histviz.modal.line'))}
              <Dropdown value={Number(style.width) || 0} options={widthOpts} ariaLabel={$t('histviz.modal.width')} float onpick={(v) => (style.width = v)} />
              <div class="seg sm" role="radiogroup" aria-label={$t('histviz.modal.lineType')}>
                {#each LINE_STYLES as ls (ls)}
                  {@const name = $t(`histviz.settings.line${ls[0].toUpperCase()}${ls.slice(1)}`)}
                  <!-- The picked style again goes back to the indicator's own. -->
                  <button
                    type="button"
                    role="radio"
                    aria-checked={style.lineType === ls}
                    aria-label={name}
                    title={name}
                    class:on={style.lineType === ls}
                    onclick={() => (style.lineType = style.lineType === ls ? '' : ls)}
                  >
                    <svg width="18" height="10" viewBox="0 0 18 10" aria-hidden="true">
                      <path
                        d="M2 5h14"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap={ls === 'dashed' ? 'butt' : 'round'}
                        stroke-dasharray={ls === 'dashed' ? '4 3' : ls === 'dotted' ? '0.1 4' : null}
                      ></path>
                    </svg>
                  </button>
                {/each}
              </div>
            </div>
            {#if mode === 'catalog' && def.fillable}
              <div class="tr">
                <span class="el">{$t('histviz.modal.fill')}</span>
                {@render colorField('fill', $t('histviz.modal.fill'))}
                <div class="opacity">
                  <input
                    type="range"
                    min="0"
                    max="100"
                    step="1"
                    aria-label={$t('histviz.modal.fillOpacity')}
                    disabled={!style.fill}
                    bind:value={style.fillOpacity}
                  />
                  <span class="mono pct">{style.fillOpacity ?? DEFAULT_STYLE.fillOpacity}%</span>
                </div>
              </div>
            {/if}
          </div>
        </div>
      </div>
    {/if}
  </div>

  {#snippet footer()}
    {#if canReset}
      <button type="button" class="ghost reset" onclick={resetDefaults}>
        <Icon name="rotate-ccw" size={14} /> {$t('histviz.modal.resetDefaults')}
      </button>
    {/if}
    {#if !edit}
      <span class="kbd-hint"><kbd>{isMac ? '⌘' : 'Ctrl'}</kbd><kbd>↵</kbd> {$t('histviz.modal.kbdAdd')}</span>
    {/if}
    <span class="grow"></span>
    <button type="button" class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button type="button" class="primary" onclick={save} disabled={mode === 'script' && !scriptEditing}>
      {edit ? $t('histviz.modal.saveChanges') : $t('histviz.modal.addTitle')}
    </button>
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
    gap: var(--space-4);
  }
  .mono {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .lbl {
    font-size: var(--fs-body);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .hint {
    font-size: var(--fs-desc);
    color: var(--dim);
    line-height: 1.45;
  }
  .with-icon {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
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
  .tag {
    font-family: var(--mono);
    font-size: 10px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
    border: var(--hairline) solid var(--border-control);
    border-radius: 4px;
    padding: 1px 5px;
    line-height: 14px;
    white-space: nowrap;
  }

  /* ── Header (rendered in the modal's head) ── */
  .badge {
    width: 36px;
    height: 36px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    color: var(--accent);
    font-size: 11px;
    font-weight: var(--fw-medium);
  }
  .ht {
    flex: 1;
    min-width: 0;
  }
  .ht-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .ht h3 {
    margin: 0;
    font-size: 15px;
    font-weight: var(--fw-medium);
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    margin-top: 2px;
    font-size: var(--fs-desc);
    color: var(--dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub.mono {
    font-size: 11px;
  }

  /* ── Segmented controls: source switch, placement, line style ── */
  .seg {
    display: flex;
    gap: 2px;
    padding: 3px;
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
  }
  .seg button {
    height: 28px;
    padding: 0 var(--space-3);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-body);
    cursor: pointer;
    white-space: nowrap;
    transition: color var(--dur-fast) var(--ease), background var(--dur-fast) var(--ease);
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button.on {
    background: var(--surface);
    color: var(--text);
    box-shadow: inset 0 0 0 var(--hairline) var(--border-control);
  }
  .seg.src button {
    flex: 1;
  }
  .seg.sm {
    padding: 2px;
  }
  .seg.sm button {
    flex: 1;
    height: 26px;
    padding: 0;
    min-width: 0;
  }

  /* ── Inputs / Style strip, edge to edge under the head ── */
  .tabs {
    display: flex;
    gap: var(--space-6);
    margin: 0 calc(-1 * var(--space-4));
    padding: 0 var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
  }
  .tabs.flush {
    margin-top: calc(-1 * var(--space-3));
  }
  .tabs button {
    height: 36px;
    padding: 0;
    margin-bottom: -1px;
    background: none;
    border: none;
    border-bottom: var(--active-rule) solid transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-body);
    cursor: pointer;
  }
  .tabs button:hover {
    color: var(--text);
  }
  .tabs button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }

  /* ── Labelled blocks and label/control rows ── */
  .block {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .rows {
    display: flex;
    flex-direction: column;
    margin-top: calc(-1 * var(--space-2));
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 176px;
    column-gap: var(--space-6);
    align-items: center;
    padding: var(--space-3) 0;
    border-bottom: var(--hairline) solid var(--border);
  }
  .row:last-child {
    border-bottom: none;
  }
  .rl {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .line-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }

  /* Number with steppers: one frame, the input inside it borderless. */
  .step {
    height: var(--control-h);
    box-sizing: border-box;
    display: flex;
    align-items: stretch;
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    overflow: hidden;
    transition: border-color var(--dur-fast) var(--ease);
  }
  .step:focus-within {
    border-color: var(--accent);
  }
  .step input {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: none;
    border-radius: 0;
    background: transparent;
    box-shadow: none;
    font-family: var(--mono);
    font-size: var(--fs-body);
    font-variant-numeric: tabular-nums;
    padding: 0 var(--space-3);
    appearance: textfield;
    -moz-appearance: textfield;
  }
  .step input:focus {
    box-shadow: none;
  }
  .step input::-webkit-inner-spin-button,
  .step input::-webkit-outer-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }
  .step button {
    width: 30px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: none;
    border-left: var(--hairline) solid var(--border-control);
    color: var(--muted);
    cursor: pointer;
  }
  .step button:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  /* ── Catalog picker ── */
  .combo {
    position: relative;
  }
  .field-btn {
    width: 100%;
    height: var(--control-h);
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    padding: 0 var(--space-3);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-body);
    cursor: pointer;
    text-align: left;
  }
  .field-btn:hover {
    background: var(--surface-2);
  }
  .field-btn[aria-expanded='true'] {
    border-color: var(--accent);
  }
  .field-btn :global(svg) {
    color: var(--muted);
    flex: none;
  }
  .fb-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .kind {
    font-family: var(--mono);
    font-size: 10px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .menu {
    position: absolute;
    z-index: var(--z-dropdown);
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    background: var(--surface-raised);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-2);
    overflow: hidden;
  }
  .menu-search {
    width: 100%;
    border: none;
    border-radius: 0;
    border-bottom: var(--hairline) solid var(--border);
    background: transparent;
  }
  .menu-search:focus {
    box-shadow: none;
  }
  .menu-list {
    max-height: 260px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .menu-empty {
    padding: var(--space-2) var(--space-3);
    color: var(--dim);
    font-size: var(--fs-body);
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
    font: inherit;
    font-size: var(--fs-body);
    cursor: pointer;
    text-align: left;
    transition: background var(--dur-fast) var(--ease);
  }
  .menu-item:hover {
    background: var(--surface-2);
  }
  .menu-item.sel {
    color: var(--accent);
  }

  /* ── Custom library row: picker + square icon buttons ── */
  .lib-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .lib-row :global(.dd) {
    flex: 1;
    min-width: 0;
  }
  .ib {
    width: var(--control-h);
    height: var(--control-h);
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    color: var(--muted);
    cursor: pointer;
  }
  .ib:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--text);
  }
  .ib.on {
    color: var(--accent);
  }
  .ib.danger:hover:not(:disabled) {
    color: var(--red);
  }
  .ib:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  /* Custom tab, builder closed. */
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-6);
    text-align: center;
    border: var(--hairline) dashed var(--border-control);
    border-radius: var(--radius);
  }
  .empty-icon {
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    color: var(--accent);
  }
  .empty-title {
    font-size: var(--fs-body);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .empty .hint {
    max-width: 320px;
  }
  .empty .btn {
    margin-top: var(--space-1);
  }

  /* ── Formula source ── */
  .formula {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
  }
  .fx {
    font-family: var(--mono);
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
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: 4px;
    color: var(--text);
    font-family: var(--mono);
    font-size: var(--text-xs);
    padding: 1px 6px;
    cursor: pointer;
  }
  .chip.fn {
    color: var(--accent);
  }
  .chip:hover {
    border-color: var(--border-strong);
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
    cursor: pointer;
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
  .fav-row .btn {
    flex: none;
  }
  .fav-name {
    flex: 1;
    min-width: 0;
  }

  /* ── Style tab ── */
  .style {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }
  .appear-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
  }
  .tbl {
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .tr {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 128px 84px 104px;
    column-gap: var(--space-2);
    align-items: center;
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
  }
  .tr:last-child {
    border-bottom: none;
  }
  .th span {
    font-size: var(--fs-metric-label);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--dim);
  }
  .el {
    font-weight: var(--fw-medium);
    color: var(--text);
    font-size: var(--fs-body);
  }
  .tr :global(.trigger) {
    height: 30px;
    font-family: var(--mono);
  }
  /* Colour: a swatch and its hex, the native picker laid invisibly over them. */
  .cfield {
    position: relative;
    height: 30px;
    display: flex;
    align-items: center;
    background: var(--surface);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-sm);
  }
  .cfield:hover {
    background: var(--surface-2);
  }
  .cpick {
    position: relative;
    flex: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-2);
    font-size: var(--fs-body);
    color: var(--text);
    cursor: pointer;
  }
  .cpick input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    padding: 0;
    border: none;
    cursor: pointer;
  }
  .sw {
    width: 14px;
    height: 14px;
    flex: none;
    border-radius: 3px;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--text) 14%, transparent);
  }
  .sw.auto {
    box-shadow: inset 0 0 0 1px var(--border-control);
  }
  .faint {
    color: var(--faint);
  }
  .cclear {
    flex: none;
    width: 22px;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
  }
  .cclear:hover {
    color: var(--text);
  }
  .opacity {
    grid-column: span 2;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .opacity input {
    flex: 1;
    min-width: 0;
    margin: 0;
    accent-color: var(--accent);
  }
  .opacity input:disabled {
    opacity: 0.4;
  }
  .pct {
    width: 4ch;
    text-align: right;
    font-size: var(--text-xs);
    color: var(--muted);
  }

  /* ── Footer ── */
  .reset {
    padding: 0 var(--space-2);
  }
  .kbd-hint {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: var(--fs-desc);
    color: var(--dim);
  }
  kbd {
    font-family: var(--mono);
    font-size: 11px;
    color: var(--muted);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border-control);
    border-bottom-width: 1.5px;
    border-radius: 4px;
    padding: 0 5px;
    line-height: 16px;
  }
  .grow {
    flex: 1;
  }

  @media (max-width: 767px) {
    .row {
      grid-template-columns: minmax(0, 1fr) 140px;
      column-gap: var(--space-3);
    }
    .tr {
      grid-template-columns: minmax(0, 1fr) 112px;
      row-gap: var(--space-2);
    }
    .tr.th {
      display: none;
    }
    .kbd-hint {
      display: none;
    }
    .seg.src button {
      padding: 0 var(--space-2);
    }
  }
</style>
