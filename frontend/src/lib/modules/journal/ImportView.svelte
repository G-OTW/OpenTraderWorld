<script>
  // Import a trade book kept somewhere else — a broker export, another journal, a
  // spreadsheet — without a per-broker parser.
  //
  // The flow is: pick a file → the server detects a mapping (headers in six languages,
  // plus what the values themselves look like) → **the user validates it against a
  // visual preview of real trades** → import. Validating can save the mapping, so the
  // next export from the same source maps itself.
  //
  // Two safety rails, both on purpose:
  //   - nothing is written before the user validates (analyze is read-only);
  //   - everything written carries a batch id, so a bad import is reverted whole
  //     instead of being picked out of the journal trade by trade.
  //
  // Vocabulary: an *import mapping* is not a journal *template*. The template is the
  // form used to log a trade by hand; the mapping says which column of a foreign file
  // is which trade field. They are listed separately and never mixed.
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Button from '$lib/ui/Button.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import PromptModal from '$lib/ui/PromptModal.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import FileDrop from '$lib/import/FileDrop.svelte';
  import FormatBar from '$lib/import/FormatBar.svelte';
  import ColumnList from '$lib/import/ColumnList.svelte';
  import ImportTradeCard from './ImportTradeCard.svelte';
  import BrokerSyncModal from './BrokerSyncModal.svelte';
  import { t, locale } from '$lib/i18n';
  import {
    journalApi,
    fileToBase64,
    IMPORT_TARGETS,
    CURRENCIES,
    ASSET_CLASSES,
    UNIT_TYPES,
    fmtSignedMoney
  } from './api.js';

  let {
    categoryId = '',
    categories = [],
    templates = [],
    onchanged = () => {}
  } = $props();

  let step = $state('pick'); // pick | map | done
  let busy = $state(false);
  let error = $state('');

  // The file stays in the browser: every call ships it again, so there is no
  // server-side upload state to expire, resume or clean up.
  let file = $state(null); // { name, content }
  let analysis = $state(null);
  let mapping = $state(null); // local source of truth, echoed back to analyze()
  // What the detector proposed, kept so the confidence dots survive the user's edits
  // (a re-analyze with a supplied mapping has nothing to be confident about).
  let detected = $state({}); // column index → { target, confidence }
  let selected = $state(0);
  let saveName = $state('');
  let report = $state(null);

  // Pulling a broker account is the same import with no file: it shares this view's
  // history and revert, and opens from here so both ways in sit side by side.
  let brokerOpen = $state(false);

  let batches = $state([]);
  let mappings = $state([]);
  let shelf = $state('history'); // history | mappings
  const shelfTabs = $derived([
    { id: 'history', label: $t('journal.import.history.title') },
    { id: 'mappings', label: $t('journal.import.mappings.title') }
  ]);

  loadHistory();

  async function loadHistory() {
    [batches, mappings] = await Promise.all([
      journalApi.listImportBatches().catch(() => []),
      journalApi.listImportMappings().catch(() => [])
    ]);
  }

  // ── Step 1: read the file, detect a mapping ──
  async function pickFile(f) {
    if (!f) return;
    busy = true;
    error = '';
    try {
      file = { name: f.name, content: await fileToBase64(f) };
      await detectInto({});
      saveName = '';
      step = 'map';
      // The defaults just applied (category, timezone) need one round-trip to show.
      queue();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  /**
   * Run a fresh detection (no mapping sent, so the server proposes one) and adopt it.
   * `section` points it at one table of a stacked statement.
   */
  async function detectInto({ section }) {
    const a = await journalApi.importAnalyze({ filename: file.name, content: file.content, section });
    analysis = a;
    detected = Object.fromEntries(
      a.columns.map((c) => [c.index, { target: c.target, confidence: c.confidence }])
    );
    mapping = {
      ...a.mapping,
      // Default the import into the category the journal is currently scoped to, and
      // read naive timestamps in the reader's own zone.
      defaults: {
        ...a.mapping.defaults,
        category_id: a.mapping.defaults.category_id ?? (categoryId || null)
      },
      tz_offset: -new Date().getTimezoneOffset()
    };
    selected = 0;
  }

  // ── Re-preview on every mapping edit (debounced) ──
  let timer = null;
  let previewing = $state(false);
  function queue() {
    clearTimeout(timer);
    timer = setTimeout(refresh, 300);
  }

  async function refresh() {
    if (!file || !mapping) return;
    previewing = true;
    error = '';
    try {
      const a = await journalApi.importAnalyze({
        filename: file.name,
        content: file.content,
        mapping
      });
      analysis = a;
      if (selected >= a.preview.length) selected = Math.max(0, a.preview.length - 1);
    } catch (e) {
      error = e.message;
    } finally {
      previewing = false;
    }
  }

  // ── Column mapping ──
  /** Targets offered for the current shape (a fill has no exit columns of its own). */
  const targets = $derived(
    IMPORT_TARGETS.filter((t0) => !(mapping?.shape === 'executions' && t0.roundtripOnly)).map(
      (t0) => ({ id: t0.id, label: targetLabel(t0) })
    )
  );

  function targetLabel(t0) {
    // In `executions` shape three fields describe the fill, not a round trip.
    const execKey = `journal.import.fieldExec.${t0.key}`;
    if (mapping?.shape === 'executions' && t0.exec) return $t(execKey);
    return $t(`journal.import.field.${t0.key}`);
  }

  /** Wording the shared column table and format bar show, in the journal's own words. */
  const columnLabels = $derived({
    ignore: $t('journal.import.target.ignore'),
    custom: $t('journal.import.target.custom'),
    noSamples: $t('journal.import.columns.noSamples'),
    ambiguousDates: $t('journal.import.ambiguousDates'),
    unnamed: (n) => $t('journal.import.columns.unnamed', { n }),
    kind: (k) => $t(`journal.import.kind.${k}`),
    confidence: (c) => $t(`journal.import.confidence.${c}`)
  });
  const formatLabels = $derived({
    section: $t('journal.import.section'),
    sectionFlat: $t('journal.import.section.flat'),
    dateOrder: $t('journal.import.dateOrder'),
    dateOrderAuto: $t('journal.import.dateOrder.auto'),
    dateOrderDmy: $t('journal.import.dateOrder.dmy'),
    dateOrderMdy: $t('journal.import.dateOrder.mdy'),
    dateOrderYmd: $t('journal.import.dateOrder.ymd'),
    decimal: $t('journal.import.decimal'),
    decimalAuto: $t('journal.import.decimal.auto'),
    decimalDot: $t('journal.import.decimal.dot'),
    decimalComma: $t('journal.import.decimal.comma'),
    timezone: $t('journal.import.timezone')
  });

  function headerOf(col) {
    return col.header?.trim() || $t('journal.import.columns.unnamed', { n: col.index + 1 });
  }

  /** Show the detector's confidence only while its proposal still stands. */
  function confidenceOf(col) {
    const d = detected[col.index];
    const current = mapping?.columns?.[String(col.index)] ?? 'ignore';
    if (!d || d.target !== current || current === 'ignore') return 'none';
    return d.confidence;
  }

  /**
   * Reading another table of a statement means other columns entirely: drop the mapping
   * and the pinned header row, and let the detector run again on the new grid.
   */
  async function setSection(name) {
    previewing = true;
    try {
      await detectInto({ section: name });
    } finally {
      previewing = false;
    }
  }

  /**
   * A fill has no exit columns of its own, so switching to `executions` releases any
   * column mapped to one — leaving it mapped would show a blank picker.
   */
  function setShape(shape) {
    const next = { ...mapping.columns };
    if (shape === 'executions') {
      for (const [k, v] of Object.entries(next)) {
        if (v === 'exit_price' || v === 'exit_at') next[k] = 'ignore';
      }
    }
    mapping = { ...mapping, shape, columns: next };
    queue();
  }

  // ── Point value per ticker ──
  let multipliersOn = $state(false);
  let tickerFilter = $state('');

  const visibleTickers = $derived.by(() => {
    const q = tickerFilter.trim().toLowerCase();
    const list = analysis?.tickers ?? [];
    return q ? list.filter((t) => t.ticker.toLowerCase().includes(q)) : list;
  });

  function toggleMultipliers(on) {
    multipliersOn = on;
    // Unticking means "every point is worth 1" — leaving the values behind would keep
    // applying them invisibly.
    if (!on && Object.keys(mapping.multipliers ?? {}).length) {
      mapping = { ...mapping, multipliers: {} };
      queue();
    }
  }

  function setMultiplier(ticker, raw) {
    const v = Number(raw);
    const next = { ...(mapping.multipliers ?? {}) };
    if (!Number.isFinite(v) || v <= 0 || v === 1) delete next[ticker];
    else next[ticker] = v;
    mapping = { ...mapping, multipliers: next };
    queue();
  }

  /** What the picker shows for a column: a target id, 'custom' or 'ignore'. */
  function selectValue(col) {
    const v = mapping?.columns?.[String(col.index)] ?? 'ignore';
    return v.startsWith('field:') ? 'custom' : v;
  }

  function setTarget(col, value) {
    const next = { ...mapping.columns };
    if (value === 'custom') {
      next[String(col.index)] = `field:${headerOf(col)}`;
    } else {
      // Reserved fields are one-to-one: taking one frees whoever held it.
      if (value !== 'ignore') {
        for (const [k, v] of Object.entries(next)) {
          if (v === value && k !== String(col.index)) next[k] = 'ignore';
        }
      }
      next[String(col.index)] = value;
    }
    mapping = { ...mapping, columns: next };
    queue();
  }

  function keepRestAsFields() {
    const next = { ...mapping.columns };
    for (const col of analysis.columns) {
      if ((next[String(col.index)] ?? 'ignore') === 'ignore' && col.header.trim()) {
        next[String(col.index)] = `field:${headerOf(col)}`;
      }
    }
    mapping = { ...mapping, columns: next };
    queue();
  }

  function ignoreRest() {
    const next = { ...mapping.columns };
    for (const [k, v] of Object.entries(next)) {
      if (v.startsWith('field:')) next[k] = 'ignore';
    }
    mapping = { ...mapping, columns: next };
    queue();
  }

  function set(patch) {
    mapping = { ...mapping, ...patch };
    queue();
  }
  function setDefault(patch) {
    mapping = { ...mapping, defaults: { ...mapping.defaults, ...patch } };
    queue();
  }
  function setConvention(patch) {
    mapping = { ...mapping, conventions: { ...mapping.conventions, ...patch } };
    queue();
  }

  async function applySavedMapping(id) {
    const saved = mappings.find((m) => m.id === id) ?? (await journalApi.listImportMappings()).find((m) => m.id === id);
    if (!saved) return;
    mapping = { ...saved.mapping, header_row: mapping.header_row, delimiter: mapping.delimiter };
    saveName = saved.name;
    await refresh();
  }

  // ── Saving the mapping (independent of importing) ──
  let namePromptOpen = $state(false);
  let namePromptFields = $state([]);

  function askSaveMapping() {
    namePromptFields = [
      {
        key: 'name',
        label: $t('journal.import.saveMapping'),
        value: saveName || analysis?.mapping_match?.name || defaultMappingName(),
        placeholder: $t('journal.import.saveMapping.placeholder'),
        required: true
      }
    ];
    namePromptOpen = true;
  }

  /** File name without its extension — the obvious first suggestion. */
  function defaultMappingName() {
    return (file?.name ?? '').replace(/\.[^.]+$/, '').slice(0, 60);
  }

  function onNameConfirmed({ name }) {
    const trimmed = name.trim();
    if (!trimmed) return;
    // Replacing is allowed, but never silently: a mapping is reused by name.
    const clash = mappings.find((m) => m.name === trimmed && m.name !== saveName);
    if (clash) {
      confirmTitle = $t('journal.import.saveMapping.replace.title');
      confirmMessage = $t('journal.import.saveMapping.replace.message', { name: trimmed });
      confirmAction = () => storeMapping(trimmed);
      confirmOpen = true;
      return;
    }
    storeMapping(trimmed);
  }

  async function storeMapping(name) {
    busy = true;
    error = '';
    try {
      await journalApi.saveImportMapping({ name, headers: analysis.headers, mapping });
      saveName = name;
      mappings = await journalApi.listImportMappings();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  // ── Commit ──
  const importable = $derived(
    analysis ? Math.max(0, analysis.stats.trades - analysis.stats.duplicates) : 0
  );

  async function commit() {
    busy = true;
    error = '';
    try {
      report = await journalApi.importCommit({
        filename: file.name,
        content: file.content,
        mapping,
        save_as: saveName.trim() || null
      });
      step = 'done';
      await loadHistory();
      onchanged();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  function reset() {
    step = 'pick';
    file = null;
    analysis = null;
    mapping = null;
    report = null;
    error = '';
  }

  // ── History ──
  let confirmOpen = $state(false);
  let confirmTitle = $state('');
  let confirmMessage = $state('');
  let confirmAction = $state(() => {});

  function askRevert(batch) {
    confirmTitle = $t('journal.import.history.confirmRevert.title');
    confirmMessage = $t('journal.import.history.confirmRevert.message', { n: batch.live_trades });
    confirmAction = async () => {
      await journalApi.revertImportBatch(batch.id);
      await loadHistory();
      onchanged();
    };
    confirmOpen = true;
  }

  function askForget(batch) {
    confirmTitle = $t('journal.import.history.confirmForget.title');
    confirmMessage = $t('journal.import.history.confirmForget.message');
    confirmAction = async () => {
      await journalApi.forgetImportBatch(batch.id);
      await loadHistory();
    };
    confirmOpen = true;
  }

  function askDeleteMapping(m) {
    confirmTitle = $t('journal.import.mappings.confirmDelete.title');
    confirmMessage = $t('journal.import.mappings.confirmDelete.message', { name: m.name });
    confirmAction = async () => {
      await journalApi.deleteImportMapping(m.id);
      await loadHistory();
    };
    confirmOpen = true;
  }

  async function revertLast() {
    if (!report?.batch_id) return;
    await journalApi.revertImportBatch(report.batch_id);
    await loadHistory();
    onchanged();
    reset();
  }

  function fmtDate(iso) {
    return iso ? new Date(iso).toLocaleString($locale, { dateStyle: 'medium', timeStyle: 'short' }) : '—';
  }
  function fmtDay(iso) {
    return iso ? new Date(iso).toLocaleDateString($locale, { dateStyle: 'medium' }) : '—';
  }
</script>

{#if step === 'pick'}
  <div class="pick">
    <!-- Two ways into the same journal: a file the user exported, or the account it was
         exported from. Both land in the history below and are reverted the same way. -->
    <div class="broker">
      <div class="btext">
        <span class="btitle">{$t('journal.import.broker.title')}</span>
        <span class="bhint">{$t('journal.import.broker.hint')}</span>
      </div>
      <Button icon="briefcase" onclick={() => (brokerOpen = true)}>
        {$t('journal.import.broker.open')}
      </Button>
    </div>

    <FileDrop
      title={$t('journal.import.pick.drop')}
      hint={$t('journal.import.pick.hint')}
      browseLabel={$t('journal.import.pick.browse')}
      busyLabel={$t('journal.import.analyzing')}
      {busy}
      onpick={pickFile}
    />

    {#if error}
      <p class="error"><Icon name="alert-triangle" size={14} /> {error}</p>
    {/if}

    <!-- Two shelves of the same drawer: what was imported, and the mappings those
         imports left behind. Tabs keep the drop zone at the top of the page instead of
         pushing it above two stacked lists that both grow without bound. -->
    <section class="shelf">
      <Tabs tabs={shelfTabs} bind:value={shelf} ariaLabel={$t('journal.import.history.title')} />

      {#if shelf === 'history'}
        {#if batches.length === 0}
          <EmptyState icon="clock" description={$t('journal.import.history.empty')} compact />
        {:else}
          <div class="scroller">
            <table class="tbl">
              <thead>
                <tr>
                  <th>{$t('journal.import.history.file')}</th>
                  <th>{$t('journal.import.history.when')}</th>
                  <th>{$t('journal.import.history.mapping')}</th>
                  <th class="num">{$t('journal.import.history.tradesCol')}</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {#each batches as b (b.id)}
                  <tr>
                    <td class="strong">
                      {#if b.source === 'broker'}<Icon name="briefcase" size={11} />{/if}
                      {b.filename}
                    </td>
                    <td class="mono">{fmtDate(b.created_at)}</td>
                    <td>{b.mapping_name || '—'}</td>
                    <td class="num">
                      {b.live_trades}
                      {#if b.duplicates > 0}<span class="dim"> · {$t('journal.import.history.dupes', { n: b.duplicates })}</span>{/if}
                    </td>
                    <td class="row-actions">
                      <Button
                        size="sm"
                        icon="rotate-ccw"
                        variant="danger"
                        disabled={b.live_trades === 0}
                        onclick={() => askRevert(b)}>{$t('journal.import.history.revert')}</Button
                      >
                      <!-- Named, not a bare ✕: next to "Revert" an unlabelled cross
                           reads as the same action with less commitment, when it is a
                           different one — it keeps the trades. -->
                      <Button size="sm" onclick={() => askForget(b)}>
                        {$t('journal.import.history.forget')}
                      </Button>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      {:else}
        {#if mappings.length === 0}
          <EmptyState
            icon="layers"
            title={$t('journal.import.mappings.empty')}
            description={$t('journal.import.mappings.hint')}
            compact
          />
        {:else}
          <p class="muted small hint">{$t('journal.import.mappings.hint')}</p>
          <div class="scroller">
            <ul class="maplist">
              {#each mappings as m (m.id)}
                <li>
                  <span class="mapname">{m.name}</span>
                  <span class="dim">
                    {m.last_used_at
                      ? $t('journal.import.mappings.used', { date: fmtDay(m.last_used_at) })
                      : $t('journal.import.mappings.never')}
                  </span>
                  <button
                    class="icon"
                    title={$t('journal.import.mappings.delete')}
                    aria-label={$t('journal.import.mappings.delete')}
                    onclick={() => askDeleteMapping(m)}><Icon name="trash" size={14} /></button
                  >
                </li>
              {/each}
            </ul>
          </div>
        {/if}
      {/if}
    </section>
  </div>
{:else if step === 'map' && analysis && mapping}
  <div class="mapper">
    <header class="bar">
      <div class="file">
        <Icon name="file-text" size={15} />
        <span class="fname">{file.name}</span>
        <span class="dim">{$t('journal.import.rows', { n: analysis.row_count })}</span>
        {#if previewing}<span class="dim">{$t('journal.import.analyzing')}</span>{/if}
      </div>
      <div class="bar-actions">
        <Button icon="save" onclick={askSaveMapping}>
          {saveName
            ? $t('journal.import.saveMapping.saved', { name: saveName })
            : $t('journal.import.saveMapping.button')}
        </Button>
        <Button onclick={reset}>{$t('journal.import.cancel')}</Button>
        <Button variant="primary" icon="upload" loading={busy} disabled={importable === 0} onclick={commit}>
          {importable === 0
            ? $t('journal.import.commit.none')
            : $t('journal.import.commit', { n: importable })}
        </Button>
      </div>
    </header>

    {#if analysis.mapping_match}
      <div class="match">
        <Icon name="sparkles" size={14} />
        <span>{$t('journal.import.match', { name: analysis.mapping_match.name })}</span>
        <button class="link" onclick={() => applySavedMapping(analysis.mapping_match.id)}
          >{$t('journal.import.match.apply')}</button
        >
      </div>
    {/if}

    {#if error}
      <p class="error"><Icon name="alert-triangle" size={14} /> {error}</p>
    {/if}

    <!-- File-wide reading rules. These change how every column is read, so they sit
         above the per-column table. -->
    <section class="settings">
      <label class="set">
        <span>{$t('journal.import.shape.label')}</span>
        <span class="seg">
          <button
            class="seg-btn"
            class:active={mapping.shape === 'roundtrip'}
            onclick={() => setShape('roundtrip')}>{$t('journal.import.shape.roundtrip')}</button
          >
          <button
            class="seg-btn"
            class:active={mapping.shape === 'executions'}
            onclick={() => setShape('executions')}>{$t('journal.import.shape.executions')}</button
          >
        </span>
      </label>
      <FormatBar
        {mapping}
        sections={analysis.sections}
        labels={formatLabels}
        onsection={setSection}
        onchange={set}
      />
      <div class="set">
        <span>{$t('journal.import.category')}</span>
        <Dropdown
          value={mapping.defaults.category_id ?? ''}
          onpick={(v) => setDefault({ category_id: v || null })}
          ariaLabel={$t('journal.import.category')}
          options={categories.map((c) => ({ value: c.id, label: c.name }))}
        />
      </div>
      <div class="set">
        <span>{$t('journal.import.template')}</span>
        <Dropdown
          value={mapping.defaults.template_id ?? ''}
          onpick={(v) => setDefault({ template_id: v || null })}
          ariaLabel={$t('journal.import.template')}
          options={[
            { value: '', label: $t('journal.import.template.none') },
            ...templates.map((tpl) => ({ value: tpl.id, label: tpl.name }))
          ]}
        />
      </div>
      <div class="set">
        <span>{$t('journal.import.defaultCurrency')}</span>
        <Dropdown
          value={mapping.defaults.currency}
          onpick={(v) => setDefault({ currency: v })}
          ariaLabel={$t('journal.import.defaultCurrency')}
          options={CURRENCIES.map((c) => ({ value: c.id, label: c.id }))}
        />
      </div>
      <!-- A broker can quote the instrument in one currency and bill its commission in
           another (IBKR charges a USD contract in the account's base currency). The fee
           is converted into the trade's currency at the trade's date before import. -->
      <div class="set">
        <span>{$t('journal.import.feeCurrency')}</span>
        <Dropdown
          value={mapping.defaults.fee_currency ?? ''}
          onpick={(v) => setDefault({ fee_currency: v || null })}
          ariaLabel={$t('journal.import.feeCurrency')}
          options={[
            { value: '', label: $t('journal.import.feeCurrency.same') },
            ...CURRENCIES.map((c) => ({ value: c.id, label: c.id }))
          ]}
        />
      </div>
      <div class="set">
        <span>{$t('journal.import.defaultAsset')}</span>
        <Dropdown
          value={mapping.defaults.asset_class}
          onpick={(v) => setDefault({ asset_class: v })}
          ariaLabel={$t('journal.import.defaultAsset')}
          options={ASSET_CLASSES.map((a) => ({ value: a.id, label: a.label }))}
        />
      </div>
      <div class="set">
        <span>{$t('journal.import.defaultUnit')}</span>
        <Dropdown
          value={mapping.defaults.unit_type}
          onpick={(v) => setDefault({ unit_type: v })}
          ariaLabel={$t('journal.import.defaultUnit')}
          options={UNIT_TYPES.map((u) => ({ value: u.id, label: u.label }))}
        />
      </div>
      <div class="conv">
        <label class="check">
          <input
            type="checkbox"
            checked={mapping.conventions.qty_sign_is_side}
            onchange={(e) => setConvention({ qty_sign_is_side: e.target.checked })}
          />
          {$t('journal.import.conv.qtySign')}
        </label>
        <label class="check">
          <input
            type="checkbox"
            checked={mapping.conventions.fees_abs}
            onchange={(e) => setConvention({ fees_abs: e.target.checked })}
          />
          {$t('journal.import.conv.feesAbs')}
        </label>
        <label class="check">
          <input
            type="checkbox"
            checked={mapping.conventions.swap_entry_exit}
            onchange={(e) => setConvention({ swap_entry_exit: e.target.checked })}
          />
          {$t('journal.import.conv.swap')}
        </label>
      </div>
    </section>

    {#if analysis.tickers.length}
      <!-- The one thing no file carries and no detection can infer: what a point of the
           instrument is worth. It differs per contract, so it is asked per ticker, against
           the list the file actually contains. -->
      <section class="mult">
        <label class="check">
          <input type="checkbox" checked={multipliersOn} onchange={(e) => toggleMultipliers(e.target.checked)} />
          {$t('journal.import.multipliers.toggle')}
        </label>
        {#if multipliersOn}
          <p class="dim small">{$t('journal.import.multipliers.hint')}</p>
          {#if analysis.tickers.length > 8}
            <input
              class="filter"
              placeholder={$t('journal.import.multipliers.filter')}
              bind:value={tickerFilter}
              autocomplete="off"
            />
          {/if}
          <div class="mult-grid">
            {#each visibleTickers as tk (tk.ticker)}
              <label class="mult-row" class:set={(mapping.multipliers?.[tk.ticker] ?? 1) !== 1}>
                <span class="mt-name">{tk.ticker}</span>
                <span class="mt-rows">{$t('journal.import.multipliers.rows', { n: tk.rows })}</span>
                <input
                  type="number"
                  min="0"
                  step="any"
                  value={mapping.multipliers?.[tk.ticker] ?? 1}
                  onchange={(e) => setMultiplier(tk.ticker, e.target.value)}
                />
              </label>
            {/each}
          </div>
          {#if visibleTickers.length === 0}
            <p class="dim small">{$t('journal.import.multipliers.noMatch')}</p>
          {/if}
        {/if}
      </section>
    {/if}

    <div class="split">
      <!-- Left: what each source column becomes. -->
      <section class="cols">
        <div class="cols-head">
          <h2>{$t('journal.import.columns.title')}</h2>
          <div class="cols-actions">
            <Button size="sm" onclick={keepRestAsFields}>{$t('journal.import.columns.keepAll')}</Button>
            <Button size="sm" onclick={ignoreRest}>{$t('journal.import.columns.ignoreAll')}</Button>
          </div>
        </div>
        {#if analysis.preamble.length}
          <p class="preamble">
            {$t('journal.import.preamble')}: <span class="mono">{analysis.preamble.join(' / ')}</span>
          </p>
        {/if}
        <ColumnList
          columns={analysis.columns}
          {targets}
          value={selectValue}
          confidence={confidenceOf}
          allowCustom
          labels={columnLabels}
          onselect={setTarget}
        />
      </section>

      <!-- Right: what the journal will hold. -->
      <section class="preview">
        <div class="stats">
          <div class="stat">
            <span class="sv">{analysis.stats.trades}</span>
            <span class="sl">{$t('journal.import.stats.trades')}</span>
          </div>
          <div class="stat">
            <span class="sv">{analysis.stats.closed}</span>
            <span class="sl">{$t('journal.import.stats.closed')}</span>
          </div>
          <div class="stat">
            <span class="sv">{analysis.stats.open}</span>
            <span class="sl">{$t('journal.import.stats.open')}</span>
          </div>
          {#if analysis.stats.duplicates > 0}
            <div class="stat">
              <span class="sv">{analysis.stats.duplicates}</span>
              <span class="sl">{$t('journal.import.stats.duplicates')}</span>
            </div>
          {/if}
          {#if analysis.stats.errors > 0}
            <div class="stat bad">
              <span class="sv">{analysis.stats.errors}</span>
              <span class="sl">{$t('journal.import.stats.errors')}</span>
            </div>
          {/if}
          {#if analysis.stats.pnl_mismatches > 0}
            <div class="stat bad">
              <span class="sv">{analysis.stats.pnl_mismatches}</span>
              <span class="sl">{$t('journal.import.stats.mismatches')}</span>
            </div>
          {/if}
        </div>

        <div class="totals">
          {#each analysis.stats.net_by_currency as [ccy, net]}
            <span class="total {net >= 0 ? 'pos' : 'neg'}">{fmtSignedMoney(net, ccy)}</span>
          {/each}
          {#if analysis.stats.first_date}
            <span class="dim">
              {fmtDay(analysis.stats.first_date)} → {fmtDay(analysis.stats.last_date)}
            </span>
          {/if}
        </div>

        {#if analysis.preview.length === 0}
          <EmptyState icon="alert-triangle" description={$t('journal.import.preview.none')} compact />
        {:else}
          <div class="pv-head">
            <h2>{$t('journal.import.preview.title')}</h2>
            <div class="nav">
              <button
                class="icon"
                disabled={selected === 0}
                aria-label={$t('journal.export.prevPeriod')}
                onclick={() => (selected = Math.max(0, selected - 1))}
                ><Icon name="chevron-left" size={15} /></button
              >
              <span class="mono"
                >{$t('journal.import.preview.of', {
                  i: selected + 1,
                  n: analysis.preview.length
                })}</span
              >
              <button
                class="icon"
                disabled={selected >= analysis.preview.length - 1}
                aria-label={$t('journal.export.nextPeriod')}
                onclick={() => (selected = Math.min(analysis.preview.length - 1, selected + 1))}
                ><Icon name="chevron-right" size={15} /></button
              >
            </div>
          </div>

          <ImportTradeCard item={analysis.preview[selected]} />

          <!-- The card proves one trade; the strip proves the file. -->
          <div class="strip">
            {#each analysis.preview as p, i (p.index)}
              <button class="row" class:active={i === selected} onclick={() => (selected = i)}>
                <span class="r-ticker">{p.trade.ticker || '—'}</span>
                <span class="r-side {p.trade.side}">{$t(`journal.side.${p.trade.side}`)}</span>
                <span class="r-date">{fmtDay(p.trade.exit_at ?? p.trade.entry_at)}</span>
                <span class="r-pnl mono {p.computed.net_pnl == null ? '' : p.computed.net_pnl >= 0 ? 'pos' : 'neg'}">
                  {p.computed.net_pnl == null
                    ? $t('journal.trades.open')
                    : fmtSignedMoney(p.computed.net_pnl, p.trade.currency)}
                </span>
                {#if p.duplicate}<Icon name="check" size={12} />{/if}
                {#if p.pnl_mismatch}<Icon name="alert-triangle" size={12} />{/if}
              </button>
            {/each}
          </div>
        {/if}

        {#if analysis.errors.length}
          <div class="errors">
            <h3><Icon name="alert-triangle" size={13} /> {$t('journal.import.errors.title')}</h3>
            <ul>
              {#each analysis.errors.slice(0, 8) as e}
                <li>
                  <span class="eline mono">{$t('journal.import.errors.line', { n: e.row })}</span>
                  <span class="emsg">{e.message}</span>
                  <span class="eraw mono">{e.raw}</span>
                </li>
              {/each}
            </ul>
            {#if analysis.errors.length > 8}
              <p class="dim">{$t('journal.import.errors.more', { n: analysis.errors.length - 8 })}</p>
            {/if}
          </div>
        {/if}

      </section>
    </div>
  </div>
{:else if step === 'done' && report}
  <div class="done">
    <Icon name="check-circle" size={26} />
    <h2>{$t('journal.import.done.title')}</h2>
    <p class="summary">
      {$t('journal.import.done.summary', {
        imported: report.imported,
        duplicates: report.duplicates,
        failed: report.failed
      })}
    </p>
    <div class="done-actions">
      <Button variant="danger" icon="rotate-ccw" onclick={revertLast}>
        {$t('journal.import.done.revert')}
      </Button>
      <Button variant="primary" onclick={reset}>{$t('journal.import.done.close')}</Button>
    </div>
    <p class="dim small">{$t('journal.import.done.revertHint')}</p>
  </div>
{/if}

<PromptModal
  bind:open={namePromptOpen}
  title={$t('journal.import.saveMapping.title')}
  fields={namePromptFields}
  confirmLabel={$t('common.save')}
  onconfirm={onNameConfirmed}
/>

<ConfirmModal
  bind:open={confirmOpen}
  title={confirmTitle}
  message={confirmMessage}
  confirmLabel={$t('journal.fees.deleteModal.confirm')}
  danger
  onconfirm={() => confirmAction()}
/>

<BrokerSyncModal
  bind:open={brokerOpen}
  {categoryId}
  {categories}
  onchanged={() => {
    loadHistory();
    onchanged();
  }}
/>

<style>
  /* The other way in: the account the file would have come from. Quiet by design, so the
     drop zone stays the first thing the page offers. */
  .broker {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3);
    background: var(--surface-2);
  }
  .btext {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .btitle {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .bhint {
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.45;
  }

  /* ── Step 1 ── */
  .pick {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }
  .shelf {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-height: 0;
  }
  .hint {
    margin-top: calc(-1 * var(--space-2));
  }
  /* Both lists grow with use; cap them here so the drop zone above stays reachable. */
  .scroller {
    max-height: 44vh;
    overflow: auto;
    border: 0.5px solid var(--border);
  }
  .scroller .tbl {
    margin: 0;
  }
  .maplist {
    list-style: none;
  }
  .maplist li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 6px var(--space-3);
    border-bottom: 0.5px solid var(--border);
  }
  .maplist li:last-child {
    border-bottom: none;
  }
  .mapname {
    flex: 1;
    font-weight: var(--fw-medium);
  }

  /* ── Step 2 ── */
  .mapper {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
    border-bottom: 0.5px solid var(--border);
    padding-bottom: var(--space-3);
  }
  .file {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    color: var(--muted);
  }
  .fname {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .bar-actions {
    display: flex;
    gap: var(--space-2);
  }
  .match {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    border-left: 1.5px solid var(--accent);
    background: var(--surface-2);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
  }
  .link {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font: inherit;
    text-decoration: underline;
  }
  .settings {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3) var(--space-4);
    align-items: flex-end;
  }
  .set {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .set > span {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .set.wide {
    flex: 1;
    min-width: 240px;
  }
  .conv {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  /* Same height and filet as the selects it sits between in the settings row. */
  .seg {
    display: inline-flex;
    height: var(--control-h);
    border: var(--hairline) solid var(--border-control);
  }
  .seg-btn {
    display: inline-flex;
    align-items: center;
    background: transparent;
    border: none;
    color: var(--muted);
    padding: 0 var(--space-3);
    font: inherit;
    font-size: var(--fs-body);
    cursor: pointer;
  }
  .seg-btn + .seg-btn {
    border-left: 0.5px solid var(--border-control);
  }
  .seg-btn.active {
    background: var(--surface-2);
    color: var(--text);
    font-weight: var(--fw-medium);
  }

  .mult {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border-top: 0.5px solid var(--border);
    padding-top: var(--space-3);
  }
  .mult .filter {
    max-width: 220px;
  }
  .mult-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 1px;
    border: 0.5px solid var(--border);
    background: var(--border);
    max-height: 240px;
    overflow-y: auto;
  }
  /* Three fixed tracks, so a row lays out identically whether or not a value was set —
     a grid can't reflow the way a flex row can when one cell grows. */
  .mult-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto 76px;
    align-items: center;
    gap: var(--space-2);
    background: var(--surface);
    padding: 4px var(--space-3);
    border-left: 1.5px solid transparent;
  }
  /* A point value other than 1 is the whole reason this table exists — mark it. */
  .mult-row.set {
    border-left-color: var(--accent);
  }
  .mt-name {
    font-weight: var(--fw-medium);
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mt-rows {
    text-align: right;
    color: var(--dim);
    font-size: var(--text-xs);
    font-family: var(--mono);
    white-space: nowrap;
  }
  .mult-row input {
    width: 100%;
    font-family: var(--mono);
  }

  .split {
    display: grid;
    grid-template-columns: minmax(320px, 2fr) minmax(360px, 3fr);
    gap: var(--space-6);
    align-items: start;
  }
  @media (max-width: 1100px) {
    .split {
      grid-template-columns: 1fr;
    }
  }
  .cols-head,
  .pv-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-2);
  }
  .cols-head h2,
  .pv-head h2,
  .errors h3 {
    font-family: var(--mono);
    font-size: var(--fs-section);
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--dim);
  }
  .cols-actions {
    display: flex;
    gap: var(--space-2);
  }
  .preamble {
    font-size: var(--text-xs);
    color: var(--dim);
    margin-bottom: var(--space-2);
  }

  .preview {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .stats {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    border: 0.5px solid var(--border);
    padding: var(--space-3);
  }
  .stat {
    display: flex;
    flex-direction: column;
  }
  .sv {
    font-family: var(--mono);
    font-size: var(--text-lg);
    font-variant-numeric: tabular-nums;
  }
  .sl {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .stat.bad .sv {
    color: var(--red);
  }
  .totals {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .total {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-weight: var(--fw-medium);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .nav {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }
  .strip {
    display: flex;
    flex-direction: column;
    border: 0.5px solid var(--border);
    max-height: 240px;
    overflow-y: auto;
  }
  .strip .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: transparent;
    border: none;
    border-bottom: 0.5px solid var(--border);
    border-left: 1.5px solid transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-sm);
    padding: 4px var(--space-2);
    cursor: pointer;
    text-align: left;
  }
  .strip .row:last-child {
    border-bottom: none;
  }
  .strip .row:hover {
    background: var(--surface-2);
  }
  .strip .row.active {
    border-left-color: var(--accent);
    background: var(--surface-2);
    color: var(--text);
  }
  .r-ticker {
    width: 90px;
    font-weight: var(--fw-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .r-side {
    width: 52px;
    text-transform: uppercase;
    font-size: var(--text-xs);
    letter-spacing: 0.04em;
  }
  .r-side.long {
    color: var(--green);
  }
  .r-side.short {
    color: var(--red);
  }
  .r-date {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .r-pnl {
    font-variant-numeric: tabular-nums;
  }
  .errors {
    border: 0.5px solid var(--border);
    border-left: 1.5px solid var(--red);
    padding: var(--space-3);
  }
  .errors ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: var(--space-2);
  }
  .errors li {
    display: flex;
    gap: var(--space-2);
    font-size: var(--text-xs);
    align-items: baseline;
  }
  .eline {
    color: var(--red);
    flex-shrink: 0;
  }
  .emsg {
    color: var(--muted);
    flex-shrink: 0;
  }
  .eraw {
    color: var(--dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Step 3 ── */
  .done {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-8) 0;
    text-align: center;
    color: var(--green);
  }
  .done h2 {
    color: var(--text);
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
  }
  .summary {
    color: var(--muted);
  }
  .done-actions {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }

  /* ── Shared ── */
  .muted {
    color: var(--muted);
  }
  .dim {
    color: var(--dim);
  }
  .small {
    font-size: var(--text-sm);
  }
  .error {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--red);
    font-size: var(--text-sm);
  }
  .num {
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .mono {
    font-family: var(--mono);
  }
  .strong {
    font-weight: var(--fw-medium);
  }
  .row-actions {
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .icon {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 2px 4px;
  }
  .icon:hover:not(:disabled) {
    color: var(--text);
  }
  .icon:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
</style>
